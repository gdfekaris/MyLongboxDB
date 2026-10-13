use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::db::APPLICATION_ID;
use crate::error::{Error, Result};

/// One numbered step in the schema's history.
pub(crate) struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// Every schema change, in order. Append only: once a migration has been
/// committed, never edit, reorder, or remove it, because collections in
/// the wild have already applied it.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/0002_saved_shelves.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/0003_page_quality.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("migrations/0004_signature_details.sql"),
    },
];

const META_DDL: &str = "CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT";

const READ_VERSION: &str = "SELECT value FROM meta WHERE key = 'schema_version'";

const WRITE_VERSION: &str = "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
    ON CONFLICT (key) DO UPDATE SET value = excluded.value";

/// Brings the schema up to the newest migration in `migrations`.
///
/// All pending migrations run in one transaction, so a failure leaves the
/// collection exactly as it was. A collection whose schema is newer than
/// any known migration is refused rather than touched.
pub(crate) fn run(conn: &mut Connection, migrations: &[Migration]) -> Result<()> {
    debug_assert!(
        migrations
            .iter()
            .enumerate()
            .all(|(i, m)| usize::try_from(m.version) == Ok(i + 1)),
        "migrations must be numbered 1, 2, 3, ... with no gaps"
    );
    let latest = migrations.last().map_or(0, |m| m.version);

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(META_DDL)?;
    let current = read_version(&tx)?;
    if current > latest {
        return Err(Error::NewerSchema {
            found: current,
            supported: latest,
        });
    }
    if current == 0 {
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    }
    for migration in migrations.iter().filter(|m| m.version > current) {
        tx.execute_batch(migration.sql)?;
        tx.execute(WRITE_VERSION, [migration.version.to_string()])?;
    }
    tx.commit()?;
    Ok(())
}

/// Whether opening would upgrade an existing collection: it has a
/// schema, at a version older than the newest migration.
pub(crate) fn pending(conn: &Connection, migrations: &[Migration]) -> Result<bool> {
    let has_meta: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = 'meta'",
        [],
        |row| row.get(0),
    )?;
    if !has_meta {
        return Ok(false);
    }
    let latest = migrations.last().map_or(0, |m| m.version);
    let current = read_version(conn)?;
    Ok(current > 0 && current < latest)
}

/// The schema version recorded in `meta`, or 0 for a new collection.
pub(crate) fn schema_version(conn: &Connection) -> Result<u32> {
    read_version(conn)
}

fn read_version(conn: &Connection) -> Result<u32> {
    let value: Option<String> = conn
        .query_row(READ_VERSION, [], |row| row.get(0))
        .optional()?;
    match value {
        None => Ok(0),
        Some(v) => v.parse().map_err(|_| Error::CorruptMeta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect;

    const TEST_MIGRATIONS: &[Migration] = &[
        Migration {
            version: 1,
            sql: "CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT",
        },
        Migration {
            version: 2,
            sql: "CREATE TABLE b (id INTEGER PRIMARY KEY) STRICT",
        },
    ];

    fn fresh() -> Connection {
        connect(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    #[test]
    fn production_migrations_are_numbered_without_gaps() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(usize::try_from(m.version).unwrap(), i + 1);
        }
    }

    #[test]
    fn applies_all_pending_migrations() {
        let mut conn = fresh();
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
        assert!(table_exists(&conn, "a"));
        assert!(table_exists(&conn, "b"));
    }

    #[test]
    fn only_an_older_schema_is_pending() {
        let mut conn = fresh();
        assert!(!pending(&conn, TEST_MIGRATIONS).unwrap(), "a new file");
        run(&mut conn, &TEST_MIGRATIONS[..1]).unwrap();
        assert!(pending(&conn, TEST_MIGRATIONS).unwrap());
        assert!(!pending(&conn, &TEST_MIGRATIONS[..1]).unwrap());
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        assert!(!pending(&conn, TEST_MIGRATIONS).unwrap());
    }

    #[test]
    fn running_again_is_a_no_op() {
        let mut conn = fresh();
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
    }

    #[test]
    fn applies_only_newer_migrations() {
        let mut conn = fresh();
        run(&mut conn, &TEST_MIGRATIONS[..1]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
        assert!(table_exists(&conn, "b"));
    }

    #[test]
    fn failed_migration_rolls_back_the_whole_upgrade() {
        let broken = [
            Migration {
                version: 1,
                sql: "CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT",
            },
            Migration {
                version: 2,
                sql: "CREATE TABLE oops (",
            },
        ];
        let mut conn = fresh();
        assert!(run(&mut conn, &broken).is_err());
        assert_eq!(schema_version(&conn).unwrap_or(0), 0);
        assert!(!table_exists(&conn, "a"));
    }

    #[test]
    fn newer_schema_is_refused_and_left_untouched() {
        let mut conn = fresh();
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        let result = run(&mut conn, &TEST_MIGRATIONS[..1]);
        assert!(matches!(
            result,
            Err(Error::NewerSchema {
                found: 2,
                supported: 1
            })
        ));
        assert_eq!(schema_version(&conn).unwrap(), 2);
    }

    #[test]
    fn unreadable_version_is_reported() {
        let mut conn = fresh();
        run(&mut conn, TEST_MIGRATIONS).unwrap();
        conn.execute(WRITE_VERSION, ["two"]).unwrap();
        assert!(matches!(
            run(&mut conn, TEST_MIGRATIONS),
            Err(Error::CorruptMeta)
        ));
    }
}
