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

/// The folder inside the store that keeps the safety copies, one for
/// each collection, made before risky changes.
const SAFETY: &str = "safety";

/// Says why a safety copy was made, beside it.
const REASON: &str = "reason";

/// The copy of a collection made before its latest risky change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafetyCopy {
    pub path: PathBuf,
    /// e.g. `before importing books`.
    pub reason: String,
    pub size_bytes: u64,
    pub made: std::time::SystemTime,
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

    /// Opens a collection like [`CollectionStore::open`], first keeping a
    /// safety copy if `safety_copies` is on and its schema is about to be
    /// upgraded.
    pub fn open_keeping(&self, name: &str, safety_copies: bool) -> Result<Database> {
        if !safety_copies {
            return self.open(name);
        }
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(Error::NotFound);
        }
        let partial = self.safety_partial(name)?;
        let db = Database::open_keeping(&path, Some(&partial))?;
        if partial.exists() {
            self.keep_safety_copy(name, &partial, "before the collection was upgraded")?;
        }
        fs::write(self.dir.join(LAST_OPENED), name)?;
        Ok(db)
    }

    /// Replaces the safety copy of collection `name` with a copy of `db`
    /// as it is now, saying why.
    pub fn make_safety_copy(&self, db: &Database, name: &str, reason: &str) -> Result<()> {
        let partial = self.safety_partial(name)?;
        db.write_copy(&partial)?;
        self.keep_safety_copy(name, &partial, reason)
    }

    /// The safety copy of collection `name`, if one was made.
    pub fn safety_copy(&self, name: &str) -> Option<SafetyCopy> {
        let path = self.safety_path(name, EXTENSION);
        let meta = fs::metadata(&path).ok()?;
        Some(SafetyCopy {
            reason: fs::read_to_string(self.safety_path(name, REASON)).unwrap_or_default(),
            size_bytes: meta.len(),
            made: meta.modified().ok()?,
            path,
        })
    }

    /// Brings back the safety copy of collection `name` as a collection
    /// of its own, leaving both the copy and the collection as they are.
    pub fn restore_safety_copy(&self, name: &str) -> Result<CollectionInfo> {
        let copy = self.safety_copy(name).ok_or(Error::NotFound)?;
        self.import_as(&copy.path, &format!("{name} (safety copy)"))
    }

    /// Writes a backup of the open collection into the folder `dest_dir`
    /// as `file_name`, or with a number added if that is taken, and
    /// returns where it went and its size. The copy is checked in full
    /// before it is written there, so a backup that exists is sound.
    pub fn back_up(
        &self,
        db: &Database,
        dest_dir: &Path,
        file_name: &str,
    ) -> Result<(PathBuf, u64)> {
        // Made in the store first, because SQLite cannot write through
        // the document portal; the checked bytes are then copied over.
        let staging = self.staging("backup")?;
        db.write_copy(&staging)?;
        self.finish_backup(&staging, dest_dir, file_name)
    }

    /// Where a copy of the open collection is made for work on it away
    /// from the window, such as a backup: a file in the store, which
    /// SQLite can write, cleared and ready. `what` names the work, so
    /// different work never shares a file.
    pub fn staging(&self, what: &str) -> Result<PathBuf> {
        let path = self.dir.join(format!("{what}.{IMPORTING}"));
        remove_with_side_files(&path);
        Ok(path)
    }

    /// Removes a copy made in [`CollectionStore::staging`] once its work
    /// is done.
    pub fn discard(&self, staging: &Path) {
        remove_with_side_files(staging);
    }

    /// Finishes a backup from a copy in staging: checks it in full,
    /// writes it into `dest_dir` as `file_name` (or with a number added),
    /// and removes the copy, returning where the backup went and its size.
    pub fn finish_backup(
        &self,
        staging: &Path,
        dest_dir: &Path,
        file_name: &str,
    ) -> Result<(PathBuf, u64)> {
        let result = (|| {
            if !check_file(staging)?.is_empty() {
                return Err(Error::Damaged);
            }
            let dest = free_file(dest_dir, file_name);
            fs::copy(staging, &dest)?;
            Ok((dest, fs::metadata(staging)?.len()))
        })();
        self.discard(staging);
        result
    }

    /// Checks a copy in staging in full and removes it, returning what
    /// the check found wrong; nothing means the collection is sound.
    pub fn check_copy(&self, staging: &Path) -> Result<Vec<String>> {
        let result = check_file(staging);
        self.discard(staging);
        result
    }

    /// Writes a compacted, checked copy of a copy in staging and removes
    /// the staging copy, returning where the compacted one is and the
    /// sizes before and after. [`CollectionStore::replace_with`] then puts
    /// it in place of the collection.
    pub fn compact_copy(&self, staging: &Path) -> Result<(PathBuf, u64, u64)> {
        let compacted = self.staging("compacted")?;
        let result = (|| {
            let (before, after) = Database::open(staging)?.compact_into(&compacted)?;
            if !check_file(&compacted)?.is_empty() {
                return Err(Error::Damaged);
            }
            Ok((compacted.clone(), before, after))
        })();
        self.discard(staging);
        if result.is_err() {
            self.discard(&compacted);
        }
        result
    }

    /// Puts `replacement`, such as a compacted copy, in place of the
    /// closed collection `name`.
    pub fn replace_with(&self, name: &str, replacement: &Path) -> Result<()> {
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(Error::NotFound);
        }
        // Taking the lock proves no window has it open, and opening the
        // replacement proves it is a sound collection before it goes in.
        drop(Database::open(&path)?);
        drop(Database::open(replacement)?);
        for side in SIDE_FILES {
            let _ = fs::remove_file(side_file(&path, side));
        }
        fs::rename(replacement, &path)?;
        for side in SIDE_FILES {
            let _ = fs::remove_file(side_file(replacement, side));
        }
        Ok(())
    }

    /// Where a safety copy is made in steps; hand it to
    /// [`CollectionStore::adopt_safety_copy`] once it is complete.
    pub fn safety_staging(&self, name: &str) -> Result<PathBuf> {
        self.safety_partial(name)
    }

    /// Makes a complete copy from [`CollectionStore::safety_staging`]
    /// the safety copy of collection `name`, replacing the last one.
    pub fn adopt_safety_copy(&self, name: &str, staging: &Path, reason: &str) -> Result<()> {
        self.keep_safety_copy(name, staging, reason)
    }

    fn safety_path(&self, name: &str, extension: &str) -> PathBuf {
        self.dir.join(SAFETY).join(format!("{name}.{extension}"))
    }

    /// Where a safety copy is written before it replaces the last one,
    /// cleared and ready.
    fn safety_partial(&self, name: &str) -> Result<PathBuf> {
        fs::create_dir_all(self.dir.join(SAFETY))?;
        let partial = self.safety_path(name, IMPORTING);
        remove_with_side_files(&partial);
        Ok(partial)
    }

    fn keep_safety_copy(&self, name: &str, partial: &Path, reason: &str) -> Result<()> {
        fs::rename(partial, self.safety_path(name, EXTENSION))?;
        fs::write(self.safety_path(name, REASON), reason)?;
        Ok(())
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
        for extension in [EXTENSION, REASON] {
            let old = self.safety_path(name, extension);
            if old.exists() {
                fs::rename(old, self.safety_path(&new_name, extension))?;
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
        // Deleted means gone, safety copy and all.
        for extension in [EXTENSION, REASON] {
            let _ = fs::remove_file(self.safety_path(name, extension));
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
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported collection");
        self.import_as(source, stem)
    }

    /// Imports the collection file at `source` under the name `wanted`,
    /// or with a number added if that is taken.
    fn import_as(&self, source: &Path, wanted: &str) -> Result<CollectionInfo> {
        let mut header = [0u8; 16];
        let mut file = fs::File::open(source)?;
        if file.read_exact(&mut header).is_err() || &header != SQLITE_HEADER {
            return Err(Error::NotACollection);
        }
        drop(file);

        let name =
            self.free_name(&clean_name(wanted).unwrap_or_else(|_| "Imported collection".into()))?;
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

/// Runs the full check on a collection file that is not open.
fn check_file(path: &Path) -> Result<Vec<String>> {
    Database::open(path)?.integrity_check()
}

/// Removes a database file and any files SQLite kept beside it.
fn remove_with_side_files(path: &Path) {
    let _ = fs::remove_file(path);
    for side in SIDE_FILES {
        let _ = fs::remove_file(side_file(path, side));
    }
}

/// `dir/file_name`, or `dir/stem-2.ext`, `-3`, … whichever does not exist
/// yet, so a backup never replaces another.
fn free_file(dir: &Path, file_name: &str) -> PathBuf {
    let first = dir.join(file_name);
    let (stem, extension) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    let mut candidate = first;
    let mut n = 1;
    while candidate.exists() {
        n += 1;
        candidate = dir.join(if extension.is_empty() {
            format!("{stem}-{n}")
        } else {
            format!("{stem}-{n}.{extension}")
        });
    }
    candidate
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

    /// Adds a book to the series `Thor` in `db`.
    fn add_thor(db: &Database, number: &str) {
        let e = crate::BookEntry::new(
            crate::Category::Comic,
            crate::SeriesChoice::New {
                name: "Thor".into(),
                start_year: None,
                volume: None,
                publisher: None,
                magazine_format: None,
                black_and_white: None,
            },
            crate::IssueNumber::new(number).ok(),
        );
        db.add_book(&e).unwrap();
    }

    fn books(db: &Database) -> usize {
        db.browse(crate::Shelf::Everything).unwrap().len()
    }

    #[test]
    fn a_safety_copy_keeps_the_collection_as_it_was() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        assert_eq!(store.safety_copy("Comics"), None);
        let db = store.open_keeping("Comics", true).unwrap();
        assert_eq!(store.safety_copy("Comics"), None, "no upgrade, no copy");
        add_thor(&db, "1");
        store
            .make_safety_copy(&db, "Comics", "before importing books")
            .unwrap();
        add_thor(&db, "2");
        // Each new copy replaces the last.
        store
            .make_safety_copy(&db, "Comics", "before emptying Trash")
            .unwrap();
        add_thor(&db, "3");
        let copy = store.safety_copy("Comics").unwrap();
        assert_eq!(copy.reason, "before emptying Trash");
        assert!(copy.size_bytes > 0);
        assert_eq!(
            fs::read_dir(dir.0.join("collections/safety"))
                .unwrap()
                .count(),
            2
        );
        assert_eq!(names(&store), ["Comics"], "safety copies are not listed");

        let restored = store.restore_safety_copy("Comics").unwrap();
        assert_eq!(restored.name, "Comics (safety copy)");
        assert_eq!(books(&store.open(&restored.name).unwrap()), 2);
        assert_eq!(books(&db), 3, "the collection itself is untouched");
        assert!(store.safety_copy("Comics").is_some());

        // Copies follow their collection when it is renamed or deleted.
        drop(db);
        store.rename("Comics", "Marvel").unwrap();
        assert_eq!(store.safety_copy("Comics"), None);
        assert_eq!(
            store.safety_copy("Marvel").unwrap().reason,
            "before emptying Trash"
        );
        store.delete("Marvel").unwrap();
        assert_eq!(store.safety_copy("Marvel"), None);
        assert!(matches!(
            store.restore_safety_copy("Marvel"),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn backups_are_checked_and_never_replace_one_another() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let db = store.open("Comics").unwrap();
        add_thor(&db, "1");
        let backups = dir.0.join("backups");
        fs::create_dir_all(&backups).unwrap();
        let name = "mylongboxdb-Comics-2026-10-11-0930.sqlite";
        let (first, size) = store.back_up(&db, &backups, name).unwrap();
        assert_eq!(first, backups.join(name));
        assert_eq!(size, fs::metadata(&first).unwrap().len());
        add_thor(&db, "2");
        let (second, _) = store.back_up(&db, &backups, name).unwrap();
        assert_eq!(
            second,
            backups.join("mylongboxdb-Comics-2026-10-11-0930-2.sqlite")
        );
        // Nothing is left behind in the store, and each backup is a
        // collection as it was.
        assert_eq!(fs::read_dir(&backups).unwrap().count(), 2);
        assert_eq!(names(&store), ["Comics"]);
        let leftovers = fs::read_dir(store.dir())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .to_string_lossy()
                    .contains("backup")
            })
            .count();
        assert_eq!(leftovers, 0);
        let imported = store.import(&first).unwrap();
        assert_eq!(books(&store.open(&imported.name).unwrap()), 1);
        assert!(store.back_up(&db, &dir.0.join("missing"), name).is_err());
    }

    /// Copies the open collection into `path` in small steps, as the
    /// window does.
    fn copy_in_steps(db: &Database, path: &Path) {
        let mut target = crate::CopyTarget::create(path).unwrap();
        {
            let mut job = db.start_copy(&mut target).unwrap();
            while !job.step(3).unwrap() {}
        }
        target.finish().unwrap();
    }

    #[test]
    fn a_compacted_copy_takes_the_place_of_the_collection() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let db = store.open("Comics").unwrap();
        add_thor(&db, "1");
        let mut fields = crate::SeriesFields::new("Hulk", crate::Category::Comic);
        fields.notes = "x".repeat(3_000_000);
        let hulk = db.create_series(&fields).unwrap();
        fields.notes.clear();
        db.update_series(hulk, &fields).unwrap();

        let staging = store.staging("compact").unwrap();
        copy_in_steps(&db, &staging);
        let (compacted, before, after) = store.compact_copy(&staging).unwrap();
        assert!(before > after + 2_000_000, "{before} {after}");
        assert!(!staging.exists(), "the uncompacted copy is cleared away");
        let staging = compacted;
        // Not while the collection is open, and nothing is lost trying.
        assert!(matches!(
            store.replace_with("Comics", &staging),
            Err(Error::Locked)
        ));
        assert!(staging.exists());
        drop(db);
        let size = |name: &str| fs::metadata(store.path_for(name)).unwrap().len();
        let old = size("Comics");
        store.replace_with("Comics", &staging).unwrap();
        assert!(size("Comics") < old);
        assert!(!staging.exists());
        let db = store.open("Comics").unwrap();
        assert_eq!(books(&db), 1);
        assert_eq!(db.series(hulk).unwrap().fields.name, "Hulk");
        assert_eq!(names(&store), ["Comics"]);
        assert!(store.replace_with("Nope", &staging).is_err());
    }

    #[test]
    fn copies_are_checked_backed_up_and_cleared_away() {
        let dir = TempDir::new();
        let store = store(&dir);
        store.create("Comics").unwrap();
        let db = store.open("Comics").unwrap();
        add_thor(&db, "1");
        let staging = store.staging("check").unwrap();
        copy_in_steps(&db, &staging);
        assert_eq!(store.check_copy(&staging).unwrap(), Vec::<String>::new());
        assert!(!staging.exists());

        let backups = dir.0.join("backups");
        fs::create_dir_all(&backups).unwrap();
        let staging = store.staging("backup").unwrap();
        copy_in_steps(&db, &staging);
        let (path, size) = store.finish_backup(&staging, &backups, "b.sqlite").unwrap();
        assert_eq!(path, backups.join("b.sqlite"));
        assert!(size > 0 && !staging.exists());

        // A safety copy made in steps is adopted like one made at once.
        let staging = store.safety_staging("Comics").unwrap();
        copy_in_steps(&db, &staging);
        store
            .adopt_safety_copy("Comics", &staging, "before emptying Trash")
            .unwrap();
        assert_eq!(
            store.safety_copy("Comics").unwrap().reason,
            "before emptying Trash"
        );
        let restored = store.restore_safety_copy("Comics").unwrap();
        assert_eq!(books(&store.open(&restored.name).unwrap()), 1);
    }
}
