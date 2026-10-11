//! The folder that holds the owner's collections.
//!
//! Inside the Flatpak sandbox SQLite cannot open files through the
//! document portal, so collections always live in the app's own data
//! folder. Files from elsewhere come in by import, which copies them and
//! checks the copy, and go out by export, which writes a plain copy.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::db::Database;
use crate::error::{Error, Result};

const EXTENSION: &str = "sqlite";

/// Names a partly copied import, so listing never shows it.
const IMPORTING: &str = "importing";

/// Remembers which collection to open at startup.
const LAST_OPENED: &str = "last-opened";

/// Every SQLite database file begins with these 16 bytes.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Files SQLite may keep beside a database, named by adding these.
const SIDE_FILES: [&str; 3] = ["-journal", "-wal", "-shm"];

fn side_file(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Longest collection name, in characters.
pub const MAX_NAME: usize = 60;

/// A collection in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// The folder of collections.
#[derive(Clone, Debug)]
pub struct CollectionStore {
    dir: PathBuf,
}

impl CollectionStore {
    /// Uses `dir`, creating it if needed.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = std::path::absolute(dir.into())?;
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every collection, by name.
    pub fn list(&self) -> Result<Vec<CollectionInfo>> {
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if !file_type.is_file() || path.extension().and_then(|e| e.to_str()) != Some(EXTENSION)
            {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            found.push(CollectionInfo {
                name: name.to_owned(),
                size_bytes: entry.metadata()?.len(),
                path,
            });
        }
        found.sort_by_key(|c| c.name.to_lowercase());
        Ok(found)
    }

    /// Makes a new, empty collection. If the name is taken, a number is
    /// added: `Comics (2)`.
    pub fn create(&self, name: &str) -> Result<CollectionInfo> {
        let name = self.free_name(&clean_name(name)?)?;
        let path = self.path_for(&name);
        drop(Database::open(&path)?);
        self.info(&name)
    }

    /// Opens a collection and remembers it for next time.
    pub fn open(&self, name: &str) -> Result<Database> {
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(Error::NotFound);
        }
        let db = Database::open(&path)?;
        fs::write(self.dir.join(LAST_OPENED), name)?;
        Ok(db)
    }

    /// Renames a closed collection. A name another collection has gets a
    /// number added, as when creating one.
    pub fn rename(&self, name: &str, new_name: &str) -> Result<CollectionInfo> {
        let from = self.path_for(name);
        if !from.is_file() {
            return Err(Error::NotFound);
        }
        let wanted = clean_name(new_name)?;
        // Changing only the case keeps the name; anything else must not
        // take another collection's.
        let new_name = if wanted.eq_ignore_ascii_case(name) {
            wanted
        } else {
            self.free_name(&wanted)?
        };
        // Taking the lock proves no window has it open.
        drop(Database::open(&from)?);
        let to = self.path_for(&new_name);
        fs::rename(&from, &to)?;
        for side in SIDE_FILES {
            let old = side_file(&from, side);
            if old.exists() {
                fs::rename(old, side_file(&to, side))?;
            }
        }
        if self.remembered().as_deref() == Some(name) {
            fs::write(self.dir.join(LAST_OPENED), &new_name)?;
        }
        self.info(&new_name)
    }

    /// Deletes a closed collection for good.
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(Error::NotFound);
        }
        drop(Database::open(&path)?);
        fs::remove_file(&path)?;
        for side in SIDE_FILES {
            let file = side_file(&path, side);
            if file.exists() {
                fs::remove_file(file)?;
            }
        }
        if self.remembered().as_deref() == Some(name) {
            fs::remove_file(self.dir.join(LAST_OPENED))?;
        }
        Ok(())
    }

    /// The name in the last-opened note, whether or not it still exists.
    fn remembered(&self) -> Option<String> {
        fs::read_to_string(self.dir.join(LAST_OPENED)).ok()
    }

    /// The collection opened most recently, if it still exists.
    pub fn last_opened(&self) -> Option<String> {
        let name = fs::read_to_string(self.dir.join(LAST_OPENED)).ok()?;
        // Only a plain collection name may become a path.
        if clean_name(&name).ok()? != name {
            return None;
        }
        self.path_for(&name).is_file().then_some(name)
    }

    /// Copies the collection file at `source` into the store under its
    /// own name, after checking it the same way opening does and running
    /// SQLite's quick check. The source is only read, never changed.
    pub fn import(&self, source: &Path) -> Result<CollectionInfo> {
        let mut header = [0u8; 16];
        let mut file = fs::File::open(source)?;
        if file.read_exact(&mut header).is_err() || &header != SQLITE_HEADER {
            return Err(Error::NotACollection);
        }
        drop(file);

        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported collection");
        let name =
            self.free_name(&clean_name(stem).unwrap_or_else(|_| "Imported collection".into()))?;
        let staging = self.dir.join(format!("{name}.{IMPORTING}"));
        let _ = fs::remove_file(&staging);
        let checked = (|| -> Result<()> {
            fs::copy(source, &staging)?;
            let db = Database::open(&staging)?;
            db.quick_check()?;
            // The search index is derived data; it is rebuilt rather than
            // trusted, since a file from elsewhere may carry one crafted
            // not to match its tables or to trip up the index's reader.
            db.rebuild_search_index()?;
            Ok(())
        })();
        if let Err(e) = checked {
            let _ = fs::remove_file(&staging);
            return Err(e);
        }
        fs::rename(&staging, self.path_for(&name))?;
        self.info(&name)
    }

    /// Writes a copy of the open collection to `dest`, such as a file the
    /// user picked in a save dialog, replacing it if it exists.
    pub fn export(&self, db: &Database, dest: &Path) -> Result<()> {
        // The copy is made in the store first, because SQLite cannot write
        // through the document portal; the bytes are then copied over.
        let staging = self.dir.join(format!("export.{IMPORTING}"));
        let _ = fs::remove_file(&staging);
        let result = db.write_copy(&staging).and_then(|()| {
            fs::copy(&staging, dest)?;
            Ok(())
        });
        let _ = fs::remove_file(&staging);
        result
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.{EXTENSION}"))
    }

    fn info(&self, name: &str) -> Result<CollectionInfo> {
        let path = self.path_for(name);
        Ok(CollectionInfo {
            name: name.to_owned(),
            size_bytes: fs::metadata(&path)?.len(),
            path,
        })
    }

    /// `name`, or `name (2)`, `name (3)`, … whichever is not yet taken,
    /// ignoring case.
    fn free_name(&self, name: &str) -> Result<String> {
        let taken: Vec<String> = self
            .list()?
            .into_iter()
            .map(|c| c.name.to_lowercase())
            .collect();
        let mut candidate = name.to_owned();
        let mut n = 1;
        while taken.contains(&candidate.to_lowercase()) {
            n += 1;
            candidate = format!("{name} ({n})");
        }
        Ok(candidate)
    }
}

/// Keeps a name safe to use as a file name: letters, digits, spaces, and
/// a little punctuation, with no leading dot.
fn clean_name(name: &str) -> Result<String> {
    let kept: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_'&(),!".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let name: String = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(Error::Invalid("enter a name for the collection".into()));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "mylongbox-store-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn store(dir: &TempDir) -> CollectionStore {
        CollectionStore::new(dir.0.join("collections")).unwrap()
    }

    fn names(store: &CollectionStore) -> Vec<String> {
        store.list().unwrap().into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn creates_and_lists_collections_by_name() {
        let dir = TempDir::new();
        let store = store(&dir);
        assert!(store.list().unwrap().is_empty());
        store.create("Main Comics").unwrap();
        store.create("archive").unwrap();
        let again = store.create("main comics").unwrap();
        assert_eq!(again.name, "main comics (2)");
        assert_eq!(names(&store), ["archive", "Main Comics", "main comics (2)"]);
        assert!(store.list().unwrap()[0].size_bytes > 0);
    }

    #[test]
    fn names_are_made_safe_for_files() {
        let dir = TempDir::new();
        let store = store(&dir);
        for (typed, kept) in [
            ("  My/Comics\\..  ", "My Comics"),
            ("../../etc/passwd", "etc passwd"),
            (".hidden", "hidden"),
            ("Zoë's Box #1", "Zoë's Box 1"),
        ] {
            assert_eq!(store.create(typed).unwrap().name, kept, "{typed}");
        }
        assert!(matches!(store.create("///"), Err(Error::Invalid(_))));
        let long = store.create(&"x".repeat(200)).unwrap();
        assert_eq!(long.name.chars().count(), MAX_NAME);
        // Everything stayed inside the store.
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn collections_rename_and_keep_being_remembered() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        store.create("Books").unwrap();
        drop(store.open("Comics").unwrap());
        let renamed = store.rename("Comics", "My Comics").unwrap();
        assert_eq!(renamed.name, "My Comics");
        assert_eq!(store.last_opened().as_deref(), Some("My Comics"));
        let names: Vec<_> = store.list().unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Books", "My Comics"]);
        assert_eq!(
            store.rename("My Comics", "books").unwrap().name,
            "books (2)"
        );
        assert_eq!(
            store.rename("books (2)", "BOOKS (2)").unwrap().name,
            "BOOKS (2)"
        );
        assert!(matches!(store.rename("Missing", "X"), Err(Error::NotFound)));
    }

    #[test]
    fn an_open_collection_is_neither_renamed_nor_deleted() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let open = store.open("Comics").unwrap();
        assert!(matches!(
            store.rename("Comics", "Other"),
            Err(Error::Locked)
        ));
        assert!(matches!(store.delete("Comics"), Err(Error::Locked)));
        drop(open);
        store.delete("Comics").unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.last_opened(), None);
        assert!(!store.dir().join(LAST_OPENED).exists());
    }

    #[test]
    fn remembers_the_last_opened_collection() {
        let dir = TempDir::new();
        let store = store(&dir);
        assert_eq!(store.last_opened(), None);
        store.create("A").unwrap();
        store.create("B").unwrap();
        drop(store.open("B").unwrap());
        assert_eq!(store.last_opened().as_deref(), Some("B"));
        // A name that climbs out of the store is ignored even when the
        // file it points at exists.
        fs::write(dir.0.join("outside.sqlite"), b"x").unwrap();
        fs::write(store.dir().join(LAST_OPENED), "../outside").unwrap();
        assert_eq!(store.last_opened(), None);
        fs::write(store.dir().join(LAST_OPENED), "B").unwrap();
        fs::remove_file(store.path_for("B")).unwrap();
        assert_eq!(store.last_opened(), None);
        assert!(matches!(store.open("B"), Err(Error::NotFound)));
    }

    #[test]
    fn an_open_collection_cannot_be_opened_twice() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let first = store.open("Comics").unwrap();
        assert!(matches!(store.open("Comics"), Err(Error::Locked)));
        drop(first);
        store.open("Comics").unwrap();
    }

    #[test]
    fn export_then_import_round_trips() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let db = store.open("Comics").unwrap();
        let series = db
            .create_series(&crate::SeriesFields::new("Zot!", crate::Category::Comic))
            .unwrap();
        let outside = dir.0.join("backup.sqlite");
        fs::write(&outside, b"old contents are replaced").unwrap();
        store.export(&db, &outside).unwrap();
        // Exporting works while the collection stays open.
        db.series(series).unwrap();
        assert_eq!(names(&store), ["Comics"], "no staging file is left");

        let imported = store.import(&outside).unwrap();
        assert_eq!(imported.name, "backup");
        let copy = store.open("backup").unwrap();
        assert_eq!(copy.list_series().unwrap()[0].fields.name, "Zot!");
        assert!(outside.exists(), "the source is never changed or removed");
    }

    #[test]
    fn imports_rebuild_the_search_index() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let db = store.open("Comics").unwrap();
        db.create_series(&crate::SeriesFields::new("Zot!", crate::Category::Comic))
            .unwrap();
        // A file from elsewhere whose index finds something its tables
        // do not hold, and has lost what they do.
        db.conn
            .execute_batch(
                "INSERT INTO search_text (search_text) VALUES ('delete-all');
                 INSERT INTO search_text (rowid, text) VALUES (999, 'planted')",
            )
            .unwrap();
        let outside = dir.0.join("odd.sqlite");
        store.export(&db, &outside).unwrap();
        drop(db);
        store.import(&outside).unwrap();
        let imported = store.open("odd").unwrap();
        let indexed: Vec<String> = crate::catalog::query_all(
            &imported.conn,
            "SELECT s.text FROM search_text f JOIN search_source s ON s.id = f.rowid
             WHERE search_text MATCH ?1",
            ["zot"],
            |r| r.get(0),
        )
        .unwrap();
        assert_eq!(indexed, ["Zot!"]);
        let planted: i64 = imported
            .conn
            .query_row(
                "SELECT count(*) FROM search_text WHERE search_text MATCH 'planted'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(planted, 0);
    }

    #[test]
    fn imports_refuse_files_that_are_not_collections() {
        let dir = TempDir::new();
        let store = store(&dir);
        let cases: [(&str, &[u8]); 3] = [
            ("empty.sqlite", b""),
            (
                "text.sqlite",
                b"just some text, nothing like a database at all",
            ),
            ("short.sqlite", b"SQLite"),
        ];
        for (name, bytes) in cases {
            let path = dir.0.join(name);
            fs::write(&path, bytes).unwrap();
            assert!(
                matches!(store.import(&path), Err(Error::NotACollection)),
                "{name}"
            );
        }
        let foreign = dir.0.join("foreign.sqlite");
        rusqlite::Connection::open(&foreign)
            .unwrap()
            .execute_batch("CREATE TABLE bookmarks (url TEXT)")
            .unwrap();
        assert!(matches!(store.import(&foreign), Err(Error::NotACollection)));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(store.dir()).unwrap().count(),
            0,
            "failed imports leave nothing behind"
        );
    }

    #[test]
    fn a_damaged_collection_is_refused() {
        let dir = TempDir::new();
        let store = store(&dir);
        let good = store.create("Good").unwrap();
        let mut bytes = fs::read(&good.path).unwrap();
        // Scribble over the last page, an index that opening never reads,
        // so only the integrity check can notice.
        let len = bytes.len();
        for b in &mut bytes[len - 4096..] {
            *b = 0xAB;
        }
        let damaged = dir.0.join("damaged.sqlite");
        fs::write(&damaged, &bytes).unwrap();
        assert!(matches!(store.import(&damaged), Err(Error::Damaged)));
        assert_eq!(names(&store), ["Good"]);
    }
}
