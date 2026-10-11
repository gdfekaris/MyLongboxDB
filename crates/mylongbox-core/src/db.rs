use std::path::Path;

use rusqlite::config::DbConfig;
use rusqlite::limits::Limit;
use rusqlite::{Connection, OpenFlags};

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
        let db = Self::init(conn)?;
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
        let dest = std::path::absolute(dest).map_err(Error::Io)?;
        if dest.exists() {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::AlreadyExists,
            )));
        }
        self.conn
            .backup(rusqlite::MAIN_DB, &dest, None)
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&dest);
            })?;
        Ok(())
    }

    /// Opens a new, empty collection held only in memory.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        let mut conn = connect(conn)?;
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
    conn.pragma_update(None, "mmap_size", 0)?;

    check_ownership(&conn)?;
    Ok(conn)
}

/// One entry of `sqlite_schema`: type, name, table, and defining SQL.
type SchemaEntry = (String, String, String, Option<String>);

fn read_schema(conn: &Connection) -> Result<Vec<SchemaEntry>> {
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
    let mut reference = connect(Connection::open_in_memory()?)?;
    migrate::run(&mut reference, MIGRATIONS)?;
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
}
