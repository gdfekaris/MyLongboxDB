//! Tests of the schema created by the migrations.

use rusqlite::{Connection, params};

use crate::{Database, IssueNumber};

fn uuid(n: i64) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn sort_key(number: &str) -> String {
    IssueNumber::new(number).unwrap().sort_key().to_owned()
}

/// A small collection touching every table. Returns ids of interest.
struct Sample {
    series: i64,
    issue: i64,
    copy: i64,
    collection_issue: i64,
}

fn sample(conn: &Connection) -> Sample {
    let exec = |sql: &str, p: &[&dyn rusqlite::ToSql]| {
        conn.execute(sql, p).unwrap();
        conn.last_insert_rowid()
    };
    let publisher = exec(
        "INSERT INTO publisher (uuid, name) VALUES (?1, 'Marvel')",
        &[&uuid(1)],
    );
    let series = exec(
        "INSERT INTO series (uuid, name, category, publisher_id, volume, start_year)
         VALUES (?1, 'Amazing Spider-Man', 'comic', ?2, 1, 1963)",
        &[&uuid(2), &publisher],
    );
    let issue = exec(
        "INSERT INTO issue (uuid, series_id, number, number_sort, title, cover_year, cover_month, is_key)
         VALUES (?1, ?2, '121', ?3, 'The Night Gwen Stacy Died', 1973, 6, 1)",
        &[&uuid(3), &series, &sort_key("121")],
    );
    let edition = exec(
        "INSERT INTO edition (uuid, issue_id, distribution, cover_price_units, cover_price_currency)
         VALUES (?1, ?2, 'newsstand', 20, 'USD')",
        &[&uuid(4), &issue],
    );
    let box_id = exec(
        "INSERT INTO storage_box (uuid, name) VALUES (?1, 'Box 1')",
        &[&uuid(5)],
    );
    let copy = exec(
        "INSERT INTO copy (uuid, edition_id, grade, grade_type, purchase_price_units,
                           purchase_price_currency, purchase_date, box_id, box_position)
         VALUES (?1, ?2, 80, 'raw', 15000, 'USD', '2020-05-01', ?3, 14)",
        &[&uuid(6), &edition, &box_id],
    );
    exec(
        "INSERT INTO value_entry (uuid, copy_id, date, value_units, value_currency, source)
         VALUES (?1, ?2, '2026-01-01', 90000, 'USD', 'GoCollect')",
        &[&uuid(7), &copy],
    );
    exec(
        "INSERT INTO cover_photo (uuid, copy_id, side, full_jpeg, thumb_jpeg, width, height, sha256)
         VALUES (?1, ?2, 'front', x'FFD8', x'FFD8', 1600, 2400, ?3)",
        &[&uuid(8), &copy, &"0".repeat(64)],
    );
    let creator = exec(
        "INSERT INTO creator (uuid, name) VALUES (?1, 'Gil Kane')",
        &[&uuid(9)],
    );
    exec(
        "INSERT INTO creator_alias (creator_id, alias) VALUES (?1, 'G. Kane')",
        &[&creator],
    );
    exec(
        "INSERT INTO credit (issue_id, creator_id, role) VALUES (?1, ?2, 'penciller')",
        &[&issue, &creator],
    );
    exec(
        "INSERT INTO edition_cover_artist (edition_id, creator_id) VALUES (?1, ?2)",
        &[&edition, &creator],
    );
    exec(
        "INSERT INTO signature (copy_id, position, creator_id, verification, place, sketch)
         VALUES (?1, 1, ?2, 'cgc_ss', 'NYCC 2015', 1)",
        &[&copy, &creator],
    );
    let character = exec(
        "INSERT INTO character (uuid, name) VALUES (?1, 'Gwen Stacy')",
        &[&uuid(10)],
    );
    exec(
        "INSERT INTO character_alias (character_id, alias) VALUES (?1, 'Ghost-Spider')",
        &[&character],
    );
    exec(
        "INSERT INTO issue_character (issue_id, character_id) VALUES (?1, ?2)",
        &[&issue, &character],
    );
    let team = exec(
        "INSERT INTO team (uuid, name) VALUES (?1, 'Avengers')",
        &[&uuid(11)],
    );
    exec(
        "INSERT INTO team_alias (team_id, alias) VALUES (?1, 'Earth''s Mightiest Heroes')",
        &[&team],
    );
    exec(
        "INSERT INTO issue_team (issue_id, team_id) VALUES (?1, ?2)",
        &[&issue, &team],
    );
    let arc = exec(
        "INSERT INTO story_arc (uuid, name) VALUES (?1, 'The Night Gwen Stacy Died')",
        &[&uuid(12)],
    );
    exec(
        "INSERT INTO story_arc_alias (story_arc_id, alias) VALUES (?1, 'Death of Gwen Stacy')",
        &[&arc],
    );
    exec(
        "INSERT INTO issue_story_arc (issue_id, story_arc_id, part) VALUES (?1, ?2, 1)",
        &[&issue, &arc],
    );
    let tag = exec(
        "INSERT INTO tag (uuid, name) VALUES (?1, 'Key')",
        &[&uuid(13)],
    );
    exec(
        "INSERT INTO copy_tag (copy_id, tag_id) VALUES (?1, ?2)",
        &[&copy, &tag],
    );
    let collected_series = exec(
        "INSERT INTO series (uuid, name, category) VALUES (?1, 'Epic Collection: Amazing Spider-Man', 'collected')",
        &[&uuid(14)],
    );
    let collection_issue = exec(
        "INSERT INTO issue (uuid, series_id, number, number_sort) VALUES (?1, ?2, '7', ?3)",
        &[&uuid(15), &collected_series, &sort_key("7")],
    );
    exec(
        "INSERT INTO edition (uuid, issue_id, isbn, binding, has_dust_jacket)
         VALUES (?1, ?2, '9781302904203', 'softcover', 0)",
        &[&uuid(16), &collection_issue],
    );
    exec(
        "INSERT INTO collects (collection_issue_id, collected_issue_id, position) VALUES (?1, ?2, 1)",
        &[&collection_issue, &issue],
    );
    exec(
        "INSERT INTO exchange_rate (date, usd_cents_per_ounce, usd_cents_per_bitcoin)
         VALUES ('2026-01-01', 400000, 10000000)",
        &[],
    );
    exec(
        "INSERT INTO saved_shelf (name, position, rules)
         VALUES ('Signed', 1, '{\"rules\":[],\"owned_as\":\"either\"}')",
        &[],
    );
    exec("INSERT INTO hidden_section (code) VALUES ('teams')", &[]);
    Sample {
        series,
        issue,
        copy,
        collection_issue,
    }
}

/// Expands to a `match` from each table name to its own literal count
/// query, so no SQL is assembled at runtime.
macro_rules! count_query {
    ($table:expr; $($name:literal),+ $(,)?) => {
        match $table {
            $($name => concat!("SELECT count(*) FROM ", $name),)+
            other => panic!("add {other} to count_query"),
        }
    };
}

fn count(conn: &Connection, table: &str) -> i64 {
    let sql = count_query!(table;
        "meta", "trash", "publisher", "series", "issue", "edition", "storage_box", "copy",
        "value_entry", "cover_photo", "creator", "creator_alias", "credit",
        "edition_cover_artist", "character", "character_alias", "issue_character", "team",
        "team_alias", "issue_team", "story_arc", "story_arc_alias", "issue_story_arc", "tag",
        "copy_tag", "collects", "exchange_rate", "signature", "search_source", "saved_shelf",
        "hidden_section",
    );
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn user_tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM pragma_table_list
             WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn schema_is_at_the_newest_version() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 8);
}

#[test]
fn every_table_is_strict() {
    let db = Database::open_in_memory().unwrap();
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT name FROM pragma_table_list
             WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%' AND strict = 0",
        )
        .unwrap();
    let loose: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(loose.is_empty(), "tables not STRICT: {loose:?}");
}

#[test]
fn every_foreign_key_is_indexed() {
    // Without an index, deleting a parent row scans the whole child table,
    // which makes emptying Trash slow on a large collection.
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let mut missing = Vec::new();
    for table in user_tables(conn) {
        let mut fks = conn
            .prepare("SELECT \"from\" FROM pragma_foreign_key_list(?1)")
            .unwrap();
        let columns: Vec<String> = fks
            .query_map([&table], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for column in columns {
            let indexed: bool = conn
                .query_row(
                    "SELECT EXISTS (
                         SELECT 1 FROM pragma_index_list(?1) AS l
                         JOIN pragma_index_info(l.name) AS i
                         WHERE i.seqno = 0 AND i.name = ?2)",
                    [&table, &column],
                    |row| row.get(0),
                )
                .unwrap();
            if !indexed {
                missing.push(format!("{table}.{column}"));
            }
        }
    }
    assert!(missing.is_empty(), "unindexed foreign keys: {missing:?}");
}

#[test]
fn a_full_sample_collection_is_consistent() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    sample(conn);
    let fk_violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fk_violations, 0);
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    for table in user_tables(conn) {
        if table != "trash" && table != "meta" {
            assert!(count(conn, &table) > 0, "{table} has no sample rows");
        }
    }
}

#[test]
fn constraints_reject_bad_rows() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let s = sample(conn);
    let bad: &[(&str, &str)] = &[
        (
            "unknown category",
            "INSERT INTO series (uuid, name, category) VALUES ('00000000-0000-4000-8000-000000000900', 'X', 'manga')",
        ),
        (
            "empty series name",
            "INSERT INTO series (uuid, name, category) VALUES ('00000000-0000-4000-8000-000000000901', '', 'comic')",
        ),
        (
            "magazine fields on a comic",
            "INSERT INTO series (uuid, name, category, magazine_format) VALUES ('00000000-0000-4000-8000-000000000902', 'X', 'comic', 'digest')",
        ),
        (
            "short uuid",
            "INSERT INTO tag (uuid, name) VALUES ('1234', 'X')",
        ),
        (
            "text in an integer column",
            "UPDATE copy SET grade = 'near mint'",
        ),
        ("grade above 10.0", "UPDATE copy SET grade = 101"),
        ("grade below 0.5", "UPDATE copy SET grade = 4"),
        (
            "unknown grade type",
            "UPDATE copy SET grade_type = 'eyeball'",
        ),
        (
            "price without currency",
            "UPDATE copy SET purchase_price_currency = NULL",
        ),
        (
            "unknown currency",
            "UPDATE copy SET purchase_price_currency = 'EUR'",
        ),
        (
            "negative price",
            "UPDATE copy SET purchase_price_units = -1",
        ),
        (
            "malformed date",
            "UPDATE copy SET purchase_date = '5/1/2020'",
        ),
        ("position without a box", "UPDATE copy SET box_id = NULL"),
        ("month without a year", "UPDATE issue SET cover_year = NULL"),
        ("month 13", "UPDATE issue SET cover_month = 13"),
        (
            "number without sort key",
            "UPDATE issue SET number_sort = ''",
        ),
        (
            "short ISBN",
            "UPDATE edition SET isbn = '12345' WHERE isbn IS NOT NULL",
        ),
        ("unknown role", "UPDATE credit SET role = 'inspiration'"),
        ("third cover side", "UPDATE cover_photo SET side = 'spine'"),
        (
            "second front photo",
            "INSERT INTO cover_photo (uuid, copy_id, side, full_jpeg, thumb_jpeg, width, height, sha256) SELECT '00000000-0000-4000-8000-000000000903', copy_id, 'front', x'00', x'00', 1, 1, sha256 FROM cover_photo",
        ),
        (
            "duplicate tag, other case",
            "INSERT INTO tag (uuid, name) VALUES ('00000000-0000-4000-8000-000000000904', 'KEY')",
        ),
        (
            "zero exchange rate",
            "UPDATE exchange_rate SET usd_cents_per_ounce = 0",
        ),
        (
            "issue collecting itself",
            "UPDATE collects SET collected_issue_id = collection_issue_id",
        ),
        (
            "credit for a missing creator",
            "UPDATE credit SET creator_id = 999",
        ),
        ("deleting a credited creator", "DELETE FROM creator"),
        (
            "deleting a box that holds copies",
            "DELETE FROM storage_box",
        ),
        (
            "a shelf whose rules are not JSON",
            "INSERT INTO saved_shelf (name, position, rules) VALUES ('X', 2, 'signed')",
        ),
        (
            "a shelf without a name",
            "INSERT INTO saved_shelf (name, position, rules) VALUES ('', 2, '{}')",
        ),
        (
            "two shelves named alike",
            "INSERT INTO saved_shelf (name, position, rules) VALUES ('SIGNED', 2, '{}')",
        ),
        (
            "a hidden entry without a code",
            "INSERT INTO hidden_section (code) VALUES ('')",
        ),
    ];
    for (why, sql) in bad {
        assert!(conn.execute(sql, []).is_err(), "accepted: {why}");
    }
    // The sample is unchanged after all the refused writes.
    let grade: i64 = conn
        .query_row("SELECT grade FROM copy WHERE id = ?1", [s.copy], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(grade, 80);
}

#[test]
fn an_issue_number_is_used_once_per_series_ignoring_case() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let s = sample(conn);
    let insert = |number: &str, n: i64| {
        conn.execute(
            "INSERT INTO issue (uuid, series_id, number, number_sort) VALUES (?1, ?2, ?3, ?4)",
            params![uuid(n), s.series, number, sort_key(number)],
        )
    };
    insert("Annual 3", 100).unwrap();
    assert!(insert("annual  3", 101).is_err());
    assert!(insert("121", 102).is_err());
    insert("122", 103).unwrap();
}

#[test]
fn a_trashed_issue_frees_its_number() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let s = sample(conn);
    conn.execute("INSERT INTO trash (label) VALUES ('ASM #121')", [])
        .unwrap();
    let trash = conn.last_insert_rowid();
    conn.execute(
        "UPDATE issue SET trash_id = ?1 WHERE id = ?2",
        [trash, s.issue],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO issue (uuid, series_id, number, number_sort) VALUES (?1, ?2, '121', ?3)",
        params![uuid(200), s.series, sort_key("121")],
    )
    .unwrap();
}

#[test]
fn emptying_trash_deletes_the_group_and_everything_beneath() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let s = sample(conn);
    // The collected edition reprints the issue, so detach it first; that
    // case is covered by the next test.
    conn.execute("DELETE FROM collects", []).unwrap();
    conn.execute(
        "INSERT INTO trash (label) VALUES ('Amazing Spider-Man')",
        [],
    )
    .unwrap();
    let trash = conn.last_insert_rowid();
    // Only this series' rows: the collected series stays live.
    for sql in [
        "UPDATE copy SET trash_id = ?1 WHERE edition_id IN (
             SELECT edition.id FROM edition JOIN issue ON issue.id = edition.issue_id
             WHERE issue.series_id = ?2)",
        "UPDATE edition SET trash_id = ?1 WHERE issue_id IN (
             SELECT id FROM issue WHERE series_id = ?2)",
        "UPDATE issue SET trash_id = ?1 WHERE series_id = ?2",
        "UPDATE series SET trash_id = ?1 WHERE id = ?2",
    ] {
        conn.execute(sql, [trash, s.series]).unwrap();
    }
    conn.execute("DELETE FROM trash WHERE id = ?1", [trash])
        .unwrap();

    // Only the collected edition's own issue and edition remain.
    for (table, expected) in [
        ("issue", 1),
        ("edition", 1),
        ("copy", 0),
        ("value_entry", 0),
        ("cover_photo", 0),
        ("credit", 0),
        ("edition_cover_artist", 0),
        ("issue_character", 0),
        ("issue_team", 0),
        ("issue_story_arc", 0),
        ("copy_tag", 0),
        ("signature", 0),
    ] {
        assert_eq!(count(conn, table), expected, "{table}");
    }
    assert_eq!(
        count(conn, "series"),
        1,
        "only the collected series is left"
    );
    // Lookups are shared, so they survive.
    assert_eq!(count(conn, "creator"), 1);
    assert_eq!(count(conn, "storage_box"), 1);
}

#[test]
fn a_reprinted_issue_cannot_be_deleted() {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn();
    let s = sample(conn);
    assert!(
        conn.execute("DELETE FROM issue WHERE id = ?1", [s.issue])
            .is_err()
    );
    // Deleting the collection itself just drops its contents list.
    conn.execute("DELETE FROM issue WHERE id = ?1", [s.collection_issue])
        .unwrap();
    assert_eq!(count(conn, "collects"), 0);
}
