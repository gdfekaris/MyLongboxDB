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
    conn: Connection,
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
        Self::init(Connection::open_with_flags(path, flags)?)
    }

    /// Opens a new, empty collection held only in memory.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        let mut conn = connect(conn)?;
        migrate::run(&mut conn, MIGRATIONS)?;
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
}
