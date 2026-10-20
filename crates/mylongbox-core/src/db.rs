use std::path::Path;

use rusqlite::config::DbConfig;
use rusqlite::limits::Limit;
use rusqlite::{Connection, OpenFlags};

use crate::catalog::query_all;
use crate::error::{Error, Result};
use crate::migrate::{self, MIGRATIONS};

/// Marks a SQLite file as a MyLongboxDB collection ("MLBX"), so the app
/// never migrates a database that belongs to something else.
pub const APPLICATION_ID: i32 = 0x4D4C_4258;

/// Largest single string or blob. Cover photos are re-encoded to a few
/// hundred kilobytes, so this leaves ample headroom while stopping a
/// hostile file from making the app allocate huge values.
const MAX_VALUE_BYTES: i32 = 32 * 1024 * 1024;
const MAX_SQL_BYTES: i32 = 1024 * 1024;

/// An open collection.
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    /// Opens the collection at `path`, creating it if it does not exist,
    /// and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_keeping(path, None)
    }

    /// Opens the collection at `path` like [`Database::open`], first
    /// writing a copy of it to `before_upgrade` if its schema is about to
    /// be upgraded, so the upgrade can be undone by hand.
    pub fn open_keeping(path: &Path, before_upgrade: Option<&Path>) -> Result<Self> {
        // The bundled SQLite is built with SQLITE_USE_URI, so any name
        // starting with "file:" is parsed as a URI whose query string can
        // change how the database is opened. An absolute path starts with
        // "/" and is therefore always taken literally.
        let path = std::path::absolute(path).map_err(|_| Error::NotACollection)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let conn = Connection::open_with_flags(path, flags)?;
        // rusqlite waits up to five seconds for a lock by default. Only
        // another open window can hold one, so say so straight away.
        conn.busy_timeout(std::time::Duration::ZERO)?;
        // Hold the file's lock for as long as it is open, so a second
        // window or process cannot write to it at the same time.
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        let conn = connect(conn)?;
        verify_stored_schema(&conn)?;
        if let Some(copy) = before_upgrade
            && migrate::pending(&conn, MIGRATIONS)?
        {
            write_copy(&conn, copy)?;
        }
        let db = Self::migrate(conn)?;
        db.conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")?;
        Ok(db)
    }

    /// Runs SQLite's quick integrity check over the whole file.
    pub fn quick_check(&self) -> Result<()> {
        let result: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(Error::Damaged)
        }
    }

    /// Writes a consistent copy of the open collection to `dest`, which
    /// must not exist yet. Works while the collection is in use.
    pub fn write_copy(&self, dest: &Path) -> Result<()> {
        write_copy(&self.conn, dest)
    }

    /// Runs SQLite's full integrity check and its check of every
    /// reference between rows, returning what each found wrong; none
    /// means the collection is sound. Slower than the quick check, since
    /// it reads every index in full.
    pub fn integrity_check(&self) -> Result<Vec<String>> {
        let mut problems: Vec<String> =
            query_all(&self.conn, "PRAGMA integrity_check", [], |row| row.get(0))?;
        problems.retain(|p| p != "ok");
        let broken: Vec<(String, i64, String)> =
            query_all(&self.conn, "PRAGMA foreign_key_check", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        problems.extend(broken.into_iter().map(|(table, row, parent)| {
            format!("row {row} of {table} refers to a missing {parent}")
        }));
        Ok(problems)
    }

    /// Writes a compacted copy of the collection to `dest`, which must
    /// not exist yet, without the space deleted rows left behind. Unlike
    /// compacting in place, this needs no temporary copy beside it, so
    /// it works where only the collection's own folders can be written.
    /// Returns the collection's size in bytes and the copy's.
    pub fn compact_into(&self, dest: &Path) -> Result<(u64, u64)> {
        let dest = std::path::absolute(dest).map_err(Error::Io)?;
        if dest.exists() {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::AlreadyExists,
            )));
        }
        let before = self.size_bytes()?;
        // VACUUM INTO writes the new file as an attached database, which
        // the connection otherwise refuses; the limit is restored whatever
        // happens, and no SQL the app runs can attach anything meanwhile.
        self.conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 1)?;
        let vacuumed = self
            .conn
            .execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()]);
        self.conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
        if let Err(e) = vacuumed {
            let _ = std::fs::remove_file(&dest);
            return Err(e.into());
        }
        Ok((before, std::fs::metadata(&dest)?.len()))
    }

    /// The collection's size in bytes, as its pages count it.
    fn size_bytes(&self) -> Result<u64> {
        let pages: i64 = self
            .conn
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let size: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        Ok(u64::try_from(pages.saturating_mul(size)).unwrap_or(0))
    }

    /// Opens a new, empty collection held only in memory.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        let conn = connect(conn)?;
        verify_stored_schema(&conn)?;
        Self::migrate(conn)
    }

    fn migrate(mut conn: Connection) -> Result<Self> {
        migrate::run(&mut conn, MIGRATIONS)?;
        verify_schema(&conn)?;
        Ok(Self { conn })
    }

    /// The schema version the collection is at.
    pub fn schema_version(&self) -> Result<u32> {
        migrate::schema_version(&self.conn)
    }

    #[cfg(test)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// A unit of work that either all happens or none of it does. Units
/// nest: saving a whole book is one unit made of smaller ones, such as
/// creating its series, and if any part fails, everything rolls back.
///
/// Built on SQLite savepoints, which nest where plain transactions
/// cannot. The outermost savepoint opens a transaction of its own.
pub(crate) struct Tx<'a> {
    conn: &'a Connection,
    done: bool,
}

impl Database {
    pub(crate) fn tx(&self) -> Result<Tx<'_>> {
        self.conn.execute_batch("SAVEPOINT unit")?;
        Ok(Tx {
            conn: &self.conn,
            done: false,
        })
    }
}

impl Tx<'_> {
    pub(crate) fn commit(mut self) -> Result<()> {
        self.conn.execute_batch("RELEASE unit")?;
        self.done = true;
        Ok(())
    }
}

impl std::ops::Deref for Tx<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.conn
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            // Undoes this unit only; an enclosing unit decides for itself.
            let _ = self.conn.execute_batch("ROLLBACK TO unit; RELEASE unit");
        }
    }
}

/// Runs fixed statements in order with `?1` and `?2` bound to `ids`.
/// A step that names only `?1` gets only the first, because rusqlite
/// refuses values a statement does not use.
pub(crate) fn run_steps(conn: &Connection, steps: &[&str], ids: [i64; 2]) -> Result<()> {
    for step in steps {
        let mut stmt = conn.prepare(step)?;
        let count = stmt.parameter_count();
        stmt.execute(rusqlite::params_from_iter(&ids[..count]))?;
    }
    Ok(())
}

/// Hardens a fresh connection and confirms the file is ours to use.
///
/// Every collection file is treated as untrusted, since it may have come
/// from someone else. The settings follow SQLite's guidance for
/// untrusted databases: <https://sqlite.org/security.html>.
/// Writes a consistent copy of the database `conn` has open to `dest`,
/// which must not exist yet.
fn write_copy(conn: &Connection, dest: &Path) -> Result<()> {
    let dest = std::path::absolute(dest).map_err(Error::Io)?;
    if dest.exists() {
        return Err(Error::Io(std::io::Error::from(
            std::io::ErrorKind::AlreadyExists,
        )));
    }
    conn.backup(rusqlite::MAIN_DB, &dest, None)
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&dest);
        })?;
    Ok(())
}

pub(crate) fn connect(conn: Connection) -> Result<Connection> {
    // Defensive mode blocks writes to the schema and other features that
    // can corrupt a database even from ordinary SQL.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    // Functions and virtual tables named in a file's schema (views,
    // triggers, defaults) may not run with side effects.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, false)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DML, false)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL, false)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;

    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_VALUE_BYTES)?;
    conn.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, MAX_SQL_BYTES)?;
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;

    conn.pragma_update(None, "cell_size_check", true)?;
    // Temporary tables and sorts stay in memory: they hold book details,
    // never photos, and files beside them could not be written once the
    // app limits where it writes.
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "mmap_size", 0)?;

    check_ownership(&conn)?;
    Ok(conn)
}

/// One entry of `sqlite_schema`: type, name, table, and defining SQL.
pub(crate) type SchemaEntry = (String, String, String, Option<String>);

pub(crate) fn read_schema(conn: &Connection) -> Result<Vec<SchemaEntry>> {
    // sqlite_stat tables appear after ANALYZE or PRAGMA optimize and hold
    // only query-planner statistics.
    let mut stmt = conn.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_stat%'
         ORDER BY type, name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Refuses a collection whose schema differs in any way from the one the
/// migrations build, such as an added trigger or view, or a loosened
/// constraint. Such a file was edited outside the app and may be hostile.
fn verify_schema(conn: &Connection) -> Result<()> {
    verify_schema_at(conn, MIGRATIONS)
}

/// Refuses a collection whose schema differs from the one the migrations
/// build up to the version it records, before any migration runs on it.
/// Checked only after upgrading, a hostile file's triggers would fire on
/// the upgrade's own statements, and an extra object on a table that an
/// upgrade rebuilds would vanish with it unnoticed. A file newer than
/// every migration is left for the upgrade to refuse.
fn verify_stored_schema(conn: &Connection) -> Result<()> {
    let version = migrate::stored_version(conn)?;
    match usize::try_from(version) {
        Ok(n) if n <= MIGRATIONS.len() => verify_schema_at(conn, &MIGRATIONS[..n]),
        _ => Ok(()),
    }
}

fn verify_schema_at(conn: &Connection, migrations: &[migrate::Migration]) -> Result<()> {
    let mut reference = connect(Connection::open_in_memory()?)?;
    if !migrations.is_empty() {
        migrate::run(&mut reference, migrations)?;
    }
    if read_schema(conn)? == read_schema(&reference)? {
        Ok(())
    } else {
        Err(Error::UnexpectedSchema)
    }
}

/// Accepts a file that is ours or completely empty; refuses anything else.
fn check_ownership(conn: &Connection) -> Result<()> {
    let app_id: i32 = conn.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if app_id == APPLICATION_ID {
        return Ok(());
    }
    if app_id != 0 {
        return Err(Error::NotACollection);
    }
    let objects: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
    if objects == 0 {
        Ok(())
    } else {
        Err(Error::NotACollection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_1_collection_is_upgraded_and_still_passes_its_checks() {
        let dir = std::env::temp_dir().join(format!("mlbx-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        {
            let mut conn = connect(Connection::open(&path).unwrap()).unwrap();
            migrate::run(&mut conn, &MIGRATIONS[..1]).unwrap();
            conn.execute(
                "INSERT INTO tag (uuid, name) VALUES ('00000000-0000-4000-8000-000000000001', 'Keys')",
                [],
            )
            .unwrap();
        }
        let before = dir.join("before.sqlite");
        let db = Database::open_keeping(&path, Some(&before)).unwrap();
        assert_eq!(db.schema_version().unwrap(), 8);
        assert!(db.saved_shelves().unwrap().is_empty());
        db.add_saved_shelf("Everything", &crate::FilterSet::default())
            .unwrap();
        drop(db);
        // Opened again, the upgraded file is checked against a schema made
        // fresh, and keeps what it held.
        let db = Database::open(&path).unwrap();
        assert_eq!(db.saved_shelves().unwrap().len(), 1);
        assert!(db.find_lookup::<crate::TagId>("Keys").unwrap().is_some());
        drop(db);
        let kept = Connection::open(&before).unwrap();
        assert_eq!(migrate::schema_version(&kept).unwrap(), 1);
        drop(kept);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_version_2_collection_keeps_its_copies_and_gains_page_quality() {
        use crate::model::{
            Category, CopyFields, EditionFields, Grade, GradeType, IssueFields, PageQuality,
            SeriesFields,
        };
        let dir = std::env::temp_dir().join(format!("mlbx-upgrade-3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        {
            let mut conn = connect(Connection::open(&path).unwrap()).unwrap();
            migrate::run(&mut conn, &MIGRATIONS[..2]).unwrap();
            conn.execute_batch(
                "INSERT INTO series (id, uuid, name, category)
                     VALUES (1, '00000000-0000-4000-8000-000000000001', 'X-Men', 'comic');
                 INSERT INTO issue (id, uuid, series_id, number, number_sort)
                     VALUES (1, '00000000-0000-4000-8000-000000000002', 1, '1', '1');
                 INSERT INTO edition (id, uuid, issue_id)
                     VALUES (1, '00000000-0000-4000-8000-000000000003', 1);
                 INSERT INTO copy (id, uuid, edition_id, grade, grade_type, notes)
                     VALUES (1, '00000000-0000-4000-8000-000000000004', 1, 94, 'cgc', 'Slabbed');",
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 8);
        let copy = db.copy(crate::model::CopyId(1)).unwrap();
        let mut fields = CopyFields {
            grade: Grade::from_tenths(94),
            grade_type: Some(GradeType::Cgc),
            notes: "Slabbed".into(),
            ..CopyFields::default()
        };
        assert_eq!(copy.fields, fields);
        assert_eq!(
            copy.uuid.to_string(),
            "00000000-0000-4000-8000-000000000004"
        );
        fields.page_quality = Some(PageQuality::OffWhiteToWhite);
        db.update_copy(copy.id, &fields).unwrap();
        // A new copy made after the upgrade works like any other.
        let series = db
            .create_series(&SeriesFields::new("Hulk", Category::Comic))
            .unwrap();
        let issue = db
            .create_issue(series, &IssueFields::new(crate::IssueNumber::new("1").ok()))
            .unwrap();
        let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
        db.create_copy(edition, &fields).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        assert_eq!(
            db.copy(copy.id).unwrap().fields.page_quality,
            Some(PageQuality::OffWhiteToWhite)
        );
        drop(db);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_version_3_collection_keeps_its_signatures_and_photos() {
        use crate::model::{CopyId, CreatorId, Side, SignaturePlacement, Verification};
        let dir = std::env::temp_dir().join(format!("mlbx-upgrade-4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        {
            let mut conn = connect(Connection::open(&path).unwrap()).unwrap();
            migrate::run(&mut conn, &MIGRATIONS[..3]).unwrap();
            conn.execute_batch(
                "INSERT INTO series (id, uuid, name, category)
                     VALUES (1, '00000000-0000-4000-8000-000000000001', 'X-Men', 'comic');
                 INSERT INTO issue (id, uuid, series_id, number, number_sort)
                     VALUES (1, '00000000-0000-4000-8000-000000000002', 1, '1', '1');
                 INSERT INTO edition (id, uuid, issue_id)
                     VALUES (1, '00000000-0000-4000-8000-000000000003', 1);
                 INSERT INTO copy (id, uuid, edition_id)
                     VALUES (1, '00000000-0000-4000-8000-000000000004', 1);
                 INSERT INTO creator (id, uuid, name)
                     VALUES (1, '00000000-0000-4000-8000-000000000005', 'Stan Lee');
                 INSERT INTO signature (copy_id, position, creator_id, verification,
                     cert_number, signed_on, place, sketch)
                     VALUES (1, 1, 1, 'cbcs', 'B-1', '2010-07-22', 'SDCC', 1);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cover_photo (id, uuid, copy_id, side, full_jpeg, thumb_jpeg,
                     width, height, sha256)
                 VALUES (7, '00000000-0000-4000-8000-000000000006', 1, 'front', ?1, ?1,
                     10, 15, ?2)",
                rusqlite::params![&jpeg[..], "a".repeat(64)],
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 8);
        assert!(db.integrity_check().unwrap().is_empty());
        let copy = CopyId(1);
        let signatures = db.signatures(copy).unwrap();
        assert_eq!(signatures.len(), 1);
        let f = &signatures[0].fields;
        assert_eq!(f.verification, Verification::CbcsVerified);
        assert_eq!(f.cert_number.as_deref(), Some("B-1"));
        assert_eq!(
            f.signed_on.map(|d| d.to_string()).as_deref(),
            Some("2010-07-22")
        );
        assert_eq!(
            (f.place.as_str(), f.sketch, f.placement),
            ("SDCC", true, None)
        );
        let front = db.photo(copy, Side::Front).unwrap().unwrap();
        assert_eq!((front.full_jpeg.as_slice(), front.width), (&jpeg[..], 10));
        // What the upgrade allows now.
        let mut f = f.clone();
        f.verification = Verification::CbcsWitnessed;
        f.placement = Some(SignaturePlacement::InsideFrontCover);
        db.set_signatures(copy, &[(CreatorId(1), f.clone())])
            .unwrap();
        db.set_photo(copy, Side::Signature, &front).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.signatures(copy).unwrap()[0].fields, f);
        assert_eq!(
            db.photo_sides(copy).unwrap(),
            [Side::Front, Side::Signature]
        );
        drop(db);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_version_4_collection_lists_its_unnumbered_issues_first() {
        use crate::Shelf;
        let dir = std::env::temp_dir().join(format!("mlbx-upgrade-5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        {
            let mut conn = connect(Connection::open(&path).unwrap()).unwrap();
            migrate::run(&mut conn, &MIGRATIONS[..4]).unwrap();
            conn.execute(
                "INSERT INTO series (id, uuid, name, category)
                 VALUES (1, '00000000-0000-4000-8000-000000000001', 'Bone', 'comic')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO issue (id, uuid, series_id, number, number_sort)
                 VALUES (1, '00000000-0000-4000-8000-000000000002', 1, '1', ?1)",
                [crate::IssueNumber::new("1").unwrap().sort_key()],
            )
            .unwrap();
            conn.execute_batch(
                "INSERT INTO issue (id, uuid, series_id, number, number_sort)
                     VALUES (2, '00000000-0000-4000-8000-000000000003', 1, 'NN', '2nn'),
                         (3, '00000000-0000-4000-8000-000000000004', 1, 'Alpha', '2alpha');
                 INSERT INTO edition (id, uuid, issue_id)
                     VALUES (1, '00000000-0000-4000-8000-000000000005', 1),
                         (2, '00000000-0000-4000-8000-000000000006', 2),
                         (3, '00000000-0000-4000-8000-000000000007', 3);
                 INSERT INTO copy (id, uuid, edition_id)
                     VALUES (1, '00000000-0000-4000-8000-000000000008', 1),
                         (2, '00000000-0000-4000-8000-000000000009', 2),
                         (3, '00000000-0000-4000-8000-00000000000a', 3);",
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 8);
        assert!(db.integrity_check().unwrap().is_empty());
        let numbers: Vec<String> = db
            .browse(Shelf::Everything)
            .unwrap()
            .into_iter()
            .map(|r| r.number.unwrap_or_default())
            .collect();
        assert_eq!(numbers, ["NN", "1", "Alpha"]);
        assert_eq!(
            db.issue(crate::model::IssueId(1))
                .unwrap()
                .fields
                .legacy_number,
            None
        );
        // The stored key is the one the number makes now, so the issue
        // is still found as a duplicate when entered again.
        let key: String = db
            .conn()
            .query_row("SELECT number_sort FROM issue WHERE id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(key, crate::IssueNumber::new("nn").unwrap().sort_key());
        drop(db);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A collection at `version` with `extra` run on it, as if edited
    /// outside the app, in a fresh folder named for `name`.
    fn edited_collection(name: &str, version: usize, extra: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mlbx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edited.sqlite");
        let mut conn = connect(Connection::open(&path).unwrap()).unwrap();
        migrate::run(&mut conn, &MIGRATIONS[..version]).unwrap();
        conn.execute_batch(extra).unwrap();
        path
    }

    #[test]
    fn an_edited_older_collection_is_refused_before_its_upgrade_runs() {
        // The trigger would fire on the upgrade's re-keying of "nn" issues.
        let path = edited_collection(
            "trigger",
            4,
            "INSERT INTO series (id, uuid, name, category)
                 VALUES (1, '00000000-0000-4000-8000-000000000001', 'Bone', 'comic');
             INSERT INTO issue (id, uuid, series_id, number, number_sort)
                 VALUES (1, '00000000-0000-4000-8000-000000000002', 1, 'nn', '2nn');
             CREATE TRIGGER planted AFTER UPDATE ON issue BEGIN
                 INSERT INTO tag (uuid, name)
                 VALUES ('00000000-0000-4000-8000-000000000003', 'ran');
             END;",
        );
        let before = path.with_file_name("before.sqlite");
        assert!(matches!(
            Database::open_keeping(&path, Some(&before)),
            Err(Error::UnexpectedSchema)
        ));
        assert!(!before.exists(), "no copy is kept of a refused file");
        let conn = Connection::open(&path).unwrap();
        assert_eq!(migrate::schema_version(&conn).unwrap(), 4, "not upgraded");
        let tags: i64 = conn
            .query_row("SELECT count(*) FROM tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tags, 0, "the planted trigger never ran");
        drop(conn);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_extra_object_on_a_table_an_upgrade_rebuilds_is_still_found() {
        // Upgrading from 3 rebuilds the signature table, which would drop
        // this index along with it.
        let path = edited_collection(
            "rebuilt",
            3,
            "CREATE INDEX planted ON signature (cert_number)",
        );
        assert!(matches!(
            Database::open(&path),
            Err(Error::UnexpectedSchema)
        ));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unupgraded_collection_of_every_version_opens() {
        for version in 1..=MIGRATIONS.len() {
            let path = edited_collection(&format!("v{version}"), version, "");
            let db = Database::open(&path).unwrap();
            assert_eq!(db.schema_version().unwrap(), 8, "from version {version}");
            drop(db);
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn connection_is_hardened() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        for (config, expected) in [
            (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
            (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
            (DbConfig::SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, false),
            (DbConfig::SQLITE_DBCONFIG_DQS_DML, false),
            (DbConfig::SQLITE_DBCONFIG_DQS_DDL, false),
            (DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true),
        ] {
            assert_eq!(conn.db_config(config).unwrap(), expected, "{config:?}");
        }
        assert_eq!(
            conn.limit(Limit::SQLITE_LIMIT_LENGTH).unwrap(),
            MAX_VALUE_BYTES
        );
        assert_eq!(
            conn.limit(Limit::SQLITE_LIMIT_SQL_LENGTH).unwrap(),
            MAX_SQL_BYTES
        );
        assert_eq!(conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(), 0);

        let pragma = |name: &str| -> i64 {
            conn.pragma_query_value(None, name, |row| row.get(0))
                .unwrap()
        };
        assert_eq!(pragma("foreign_keys"), 1);
        assert_eq!(pragma("trusted_schema"), 0);
        assert_eq!(pragma("cell_size_check"), 1);
    }

    #[test]
    fn defensive_mode_blocks_schema_writes() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute_batch("PRAGMA writable_schema = ON").unwrap();
        let result = conn.execute("UPDATE sqlite_schema SET sql = 'x' WHERE name = 'meta'", []);
        assert!(result.is_err());
    }

    #[test]
    fn attach_is_refused() {
        let db = Database::open_in_memory().unwrap();
        assert!(
            db.conn()
                .execute_batch("ATTACH ':memory:' AS other")
                .is_err()
        );
    }

    #[test]
    fn extension_loading_is_off() {
        let db = Database::open_in_memory().unwrap();
        let result: rusqlite::Result<i64> =
            db.conn()
                .query_row("SELECT load_extension('/nonexistent')", [], |row| {
                    row.get(0)
                });
        let message = result.unwrap_err().to_string();
        assert!(message.contains("not authorized"), "{message}");
    }

    #[test]
    fn double_quoted_strings_are_identifiers_only() {
        let db = Database::open_in_memory().unwrap();
        let result: rusqlite::Result<String> =
            db.conn()
                .query_row(r#"SELECT "not a column""#, [], |row| row.get(0));
        assert!(result.is_err());
    }

    #[test]
    fn oversized_values_are_refused() {
        let db = Database::open_in_memory().unwrap();
        let result: rusqlite::Result<Vec<u8>> = db.conn().query_row(
            "SELECT zeroblob(?1)",
            [i64::from(MAX_VALUE_BYTES) + 1],
            |row| row.get(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_collection_is_stamped_as_ours() {
        let db = Database::open_in_memory().unwrap();
        let app_id: i32 = db
            .conn()
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap();
        assert_eq!(app_id, APPLICATION_ID);
    }

    #[test]
    fn foreign_application_id_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "application_id", 1234).unwrap();
        assert!(matches!(connect(conn), Err(Error::NotACollection)));
    }

    #[test]
    fn unmarked_database_with_tables_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE someone_elses (x)")
            .unwrap();
        assert!(matches!(connect(conn), Err(Error::NotACollection)));
    }

    fn publishers(db: &Database) -> Vec<String> {
        db.list_lookups::<crate::PublisherId>()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }

    #[test]
    fn units_nest_and_roll_back_on_their_own() {
        let db = Database::open_in_memory().unwrap();
        let outer = db.tx().unwrap();
        db.create_lookup::<crate::PublisherId>("Kept").unwrap();
        {
            let _inner = db.tx().unwrap();
            db.create_lookup::<crate::PublisherId>("Dropped").unwrap();
            // Dropped without commit: only the inner unit is undone.
        }
        outer.commit().unwrap();
        assert_eq!(publishers(&db), ["Kept"]);
    }

    #[test]
    fn an_outer_rollback_undoes_committed_inner_units() {
        let db = Database::open_in_memory().unwrap();
        {
            let _outer = db.tx().unwrap();
            let inner = db.tx().unwrap();
            db.create_lookup::<crate::PublisherId>("Gone").unwrap();
            inner.commit().unwrap();
        }
        assert!(publishers(&db).is_empty());
        // The connection is usable afterwards, outside any transaction.
        assert!(db.conn.is_autocommit());
    }

    #[test]
    fn the_integrity_check_finds_rows_that_lost_their_parent() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.integrity_check().unwrap(), Vec::<String>::new());
        let series = db
            .create_series(&crate::SeriesFields::new("Thor", crate::Category::Comic))
            .unwrap();
        let number = crate::IssueNumber::new("1").ok();
        db.create_issue(series, &crate::IssueFields::new(number))
            .unwrap();
        // Only damage from outside the app could do this.
        db.conn.pragma_update(None, "foreign_keys", false).unwrap();
        db.conn.execute("DELETE FROM series", []).unwrap();
        let problems = db.integrity_check().unwrap();
        assert_eq!(problems, ["row 1 of issue refers to a missing series"]);
    }

    #[test]
    fn compacting_gives_back_the_space_of_deleted_rows() {
        let db = Database::open_in_memory().unwrap();
        let series = db
            .create_series(&crate::SeriesFields::new("Thor", crate::Category::Comic))
            .unwrap();
        let mut fields = crate::SeriesFields::new("Thor", crate::Category::Comic);
        fields.notes = "x".repeat(2_000_000);
        db.update_series(series, &fields).unwrap();
        fields.notes.clear();
        db.update_series(series, &fields).unwrap();
        let dest =
            std::env::temp_dir().join(format!("mylongbox-compact-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let (before, after) = db.compact_into(&dest).unwrap();
        assert!(before > after + 1_000_000, "{before} {after}");
        assert_eq!(after, std::fs::metadata(&dest).unwrap().len());
        assert_eq!(
            db.conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(),
            0,
            "attaching is refused again"
        );
        assert!(db.compact_into(&dest).is_err(), "never over a file");
        let copy = Database::open(&dest).unwrap();
        assert_eq!(copy.series(series).unwrap().fields.name, "Thor");
        assert!(copy.integrity_check().unwrap().is_empty());
        drop(copy);
        std::fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn compacting_keeps_alternate_names_searchable_and_removable() {
        // Alternate names find their search entries by rowid, which
        // VACUUM may renumber in a table without an INTEGER PRIMARY KEY;
        // gaps left by removed names are where that would show.
        let db = Database::open_in_memory().unwrap();
        let kirby = db.create_lookup::<crate::CreatorId>("Jack Kirby").unwrap();
        for alias in ["Alpha", "Bravo", "Charlie", "Delta"] {
            db.add_alias(kirby, alias).unwrap();
        }
        for alias in ["Alpha", "Charlie"] {
            db.remove_alias(kirby, alias).unwrap();
        }
        let dest = std::env::temp_dir().join(format!(
            "mylongbox-compact-alias-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&dest);
        db.compact_into(&dest).unwrap();
        let copy = Database::open(&dest).unwrap();
        copy.remove_alias(kirby, "Delta").unwrap();
        let entries = |kind: i64| -> Vec<String> {
            let mut stmt = copy
                .conn
                .prepare("SELECT text FROM search_source WHERE kind = ?1 ORDER BY text")
                .unwrap();
            stmt.query_map([kind], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(entries(5), ["Bravo"]);
        let found: i64 = copy
            .conn
            .query_row(
                "SELECT count(*) FROM search_text WHERE search_text MATCH '\"Delta\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, 0, "the removed name is gone from the index");
        drop(copy);
        std::fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn temporary_data_stays_in_memory() {
        let db = Database::open_in_memory().unwrap();
        let store: i64 = db
            .conn
            .pragma_query_value(None, "temp_store", |r| r.get(0))
            .unwrap();
        assert_eq!(store, 2, "2 means memory");
    }
}
