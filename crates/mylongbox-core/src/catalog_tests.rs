//! Tests of creating, reading, updating, and trashing catalog rows.

use crate::model::*;
use crate::{Amount, Currency, Database, Date, Error, IssueNumber};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn number(text: &str) -> Option<IssueNumber> {
    Some(IssueNumber::new(text).unwrap())
}

fn usd(text: &str) -> Amount {
    Amount::parse(Currency::Usd, text).unwrap()
}

fn comic_series(db: &Database, name: &str) -> SeriesId {
    let mut f = SeriesFields::new(name, Category::Comic);
    f.start_year = Some(1963);
    db.create_series(&f).unwrap()
}

/// Series → issue → edition → copy, returning all four ids.
fn comic(db: &Database, series: SeriesId, n: &str) -> (IssueId, EditionId, CopyId) {
    let issue = db
        .create_issue(series, &IssueFields::new(number(n)))
        .unwrap();
    let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
    let copy = db.create_copy(edition, &CopyFields::default()).unwrap();
    (issue, edition, copy)
}

fn numbers(db: &Database, series: SeriesId) -> Vec<String> {
    db.issues_in_series(series)
        .unwrap()
        .into_iter()
        .map(|i| i.fields.number.unwrap().as_str().to_owned())
        .collect()
}

fn invalid(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Invalid(why)) => why,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

// Round trips ---------------------------------------------------------------

#[test]
fn every_field_round_trips() {
    let db = db();
    db.conn
        .execute(
            "INSERT INTO storage_box (uuid, name) VALUES ('00000000-0000-4000-8000-000000000001', 'Box 1')",
            [],
        )
        .unwrap();
    let storage_box = BoxId(db.conn.last_insert_rowid());

    let mut series = SeriesFields::new("Savage Sword of Conan", Category::Magazine);
    series.volume = Some(1);
    series.start_year = Some(1974);
    series.known_issue_count = Some(235);
    series.magazine_format = Some(MagazineFormat::Standard);
    series.black_and_white = Some(true);
    series.notes = "Curtis, then Marvel".into();
    let series_id = db.create_series(&series).unwrap();
    let read = db.series(series_id).unwrap();
    assert_eq!(read.fields, series);
    assert_eq!(read.trash, None);

    let issue = IssueFields {
        number: number("1"),
        title: "Curse of the Cat-Goddess".into(),
        cover_date: CoverDate::new(1974, Some(8)),
        release_date: Some("1974-05-14".parse().unwrap()),
        page_count: Some(68),
        synopsis: "Conan in black and white.".into(),
        is_key: true,
        key_reason: "First issue".into(),
    };
    let issue_id = db.create_issue(series_id, &issue).unwrap();
    let read = db.issue(issue_id).unwrap();
    assert_eq!(read.fields, issue);
    assert_eq!(read.series, series_id);

    let edition = EditionFields {
        variant: "".into(),
        printing: Some(1),
        distribution: Some(Distribution::Newsstand),
        cover_price: Some(usd("1.00")),
        barcode: Some("071486".into()),
        ..EditionFields::default()
    };
    let edition_id = db.create_edition(issue_id, &edition).unwrap();
    assert_eq!(db.edition(edition_id).unwrap().fields, edition);

    let copy = CopyFields {
        grade: Grade::parse("9.4"),
        grade_type: Some(GradeType::Cgc),
        cert_number: Some("1234567001".into()),
        label_type: Some("Universal".into()),
        purchase_price: Some(Amount::parse(Currency::Btc, "0.0015").unwrap()),
        purchase_date: Some("2021-03-01".parse().unwrap()),
        seller: "Local shop".into(),
        storage_box: Some(storage_box),
        box_position: Some(14),
        notes: "Slabbed".into(),
        ..CopyFields::default()
    };
    let copy_id = db.create_copy(edition_id, &copy).unwrap();
    let read = db.copy(copy_id).unwrap();
    assert_eq!(read.fields, copy);
    assert_eq!(read.edition, edition_id);
    assert_eq!(read.added_at.len(), 20, "{}", read.added_at);
}

#[test]
fn collected_edition_fields_round_trip() {
    let db = db();
    let series = db
        .create_series(&SeriesFields::new(
            "Marvel Masterworks: Fantastic Four",
            Category::CollectedEdition,
        ))
        .unwrap();
    // A standalone book may have no number.
    let issue = db.create_issue(series, &IssueFields::new(None)).unwrap();
    let edition = EditionFields {
        isbn: Isbn::parse("978-1-302-90420-3"),
        binding: Some(Binding::Hardcover),
        has_dust_jacket: Some(true),
        has_slipcase: Some(false),
        ..EditionFields::default()
    };
    let edition_id = db.create_edition(issue, &edition).unwrap();
    assert_eq!(db.edition(edition_id).unwrap().fields, edition);
    let copy = CopyFields {
        book_condition: Some(BookCondition::LikeNew),
        dust_jacket_condition: Some(BookCondition::VeryGood),
        ..CopyFields::default()
    };
    let copy_id = db.create_copy(edition_id, &copy).unwrap();
    assert_eq!(db.copy(copy_id).unwrap().fields, copy);
}

#[test]
fn every_row_gets_its_own_uuid() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (a, _, _) = comic(&db, series, "1");
    let (b, _, _) = comic(&db, series, "2");
    let (ua, ub) = (db.issue(a).unwrap().uuid, db.issue(b).unwrap().uuid);
    assert_ne!(ua, ub);
    assert_eq!(ua.0.get_version_num(), 4);
}

#[test]
fn text_is_trimmed_and_blank_optionals_become_none() {
    let db = db();
    let id = comic_series(&db, "  Amazing Spider-Man  ");
    assert_eq!(db.series(id).unwrap().fields.name, "Amazing Spider-Man");
    let (_, edition, _) = comic(&db, id, "1");
    let copy = CopyFields {
        cert_number: Some("   ".into()),
        seller: " eBay ".into(),
        ..CopyFields::default()
    };
    let copy = db.create_copy(edition, &copy).unwrap();
    let read = db.copy(copy).unwrap().fields;
    assert_eq!(read.cert_number, None);
    assert_eq!(read.seller, "eBay");
}

#[test]
fn updates_replace_fields() {
    let db = db();
    let series = comic_series(&db, "Amazng Spider-Man");
    let mut fields = db.series(series).unwrap().fields;
    fields.name = "Amazing Spider-Man".into();
    db.update_series(series, &fields).unwrap();
    assert_eq!(db.series(series).unwrap().fields.name, "Amazing Spider-Man");

    let (issue, edition, copy) = comic(&db, series, "1");
    let mut f = IssueFields::new(number("1"));
    f.title = "Spider-Man".into();
    db.update_issue(issue, &f).unwrap();
    assert_eq!(db.issue(issue).unwrap().fields.title, "Spider-Man");

    let f = EditionFields {
        variant: "Cover B".into(),
        ..EditionFields::default()
    };
    db.update_edition(edition, &f).unwrap();
    assert_eq!(db.edition(edition).unwrap().fields.variant, "Cover B");

    let f = CopyFields {
        grade: Grade::parse("6.5"),
        ..CopyFields::default()
    };
    db.update_copy(copy, &f).unwrap();
    assert_eq!(db.copy(copy).unwrap().fields.grade, Grade::parse("6.5"));
}

#[test]
fn missing_rows_are_not_found() {
    let db = db();
    assert!(matches!(db.series(SeriesId(99)), Err(Error::NotFound)));
    assert!(matches!(db.issue(IssueId(99)), Err(Error::NotFound)));
    assert!(matches!(
        db.create_issue(SeriesId(99), &IssueFields::new(number("1"))),
        Err(Error::NotFound)
    ));
    assert!(matches!(db.restore(TrashId(99)), Err(Error::NotFound)));
    assert!(matches!(db.purge(TrashId(99)), Err(Error::NotFound)));
}

// Ordering and numbering ---------------------------------------------------

#[test]
fn series_list_by_name_and_issues_in_natural_order() {
    let db = db();
    let b = comic_series(&db, "batman");
    let a = comic_series(&db, "Amazing Spider-Man");
    for n in ["10", "Annual 1", "2", "½", "1", "1A"] {
        comic(&db, a, n);
    }
    let names: Vec<_> = db
        .list_series()
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(names, [a, b]);
    assert_eq!(numbers(&db, a), ["½", "1", "1A", "2", "10", "Annual 1"]);
}

#[test]
fn an_issue_number_is_used_once_per_series() {
    let db = db();
    let a = comic_series(&db, "Amazing Spider-Man");
    let b = comic_series(&db, "Spectacular Spider-Man");
    let (issue, _, _) = comic(&db, a, "Annual 3");
    let duplicate = db.create_issue(a, &IssueFields::new(number("annual 3")));
    assert!(matches!(
        duplicate,
        Err(Error::DuplicateIssueNumber { number }) if number == "annual 3"
    ));
    // Another series may use it, and an issue may keep its own number.
    comic(&db, b, "Annual 3");
    db.update_issue(issue, &IssueFields::new(number("Annual 3")))
        .unwrap();
    let (other, _, _) = comic(&db, a, "4");
    assert!(matches!(
        db.update_issue(other, &IssueFields::new(number("Annual 3"))),
        Err(Error::DuplicateIssueNumber { .. })
    ));
}

// Category rules -----------------------------------------------------------

#[test]
fn fields_must_suit_the_category() {
    let db = db();
    let comics = comic_series(&db, "Amazing Spider-Man");
    let books = db
        .create_series(&SeriesFields::new("Omnibus", Category::CollectedEdition))
        .unwrap();
    let (_, comic_edition, _) = comic(&db, comics, "1");
    let book_issue = db.create_issue(books, &IssueFields::new(None)).unwrap();
    let book_edition = db
        .create_edition(book_issue, &EditionFields::default())
        .unwrap();

    let mut magazine_only = SeriesFields::new("X", Category::Comic);
    magazine_only.black_and_white = Some(true);
    invalid(db.create_series(&magazine_only));
    invalid(db.create_series(&SeriesFields::new("   ", Category::Comic)));
    invalid(db.create_issue(comics, &IssueFields::new(None)));
    let isbn = EditionFields {
        isbn: Isbn::parse("9781302904203"),
        ..EditionFields::default()
    };
    invalid(db.create_edition(db.edition(comic_edition).unwrap().issue, &isbn));
    let graded = CopyFields {
        grade: Grade::parse("9.8"),
        ..CopyFields::default()
    };
    invalid(db.create_copy(book_edition, &graded));
    let book_condition = CopyFields {
        book_condition: Some(BookCondition::Good),
        ..CopyFields::default()
    };
    invalid(db.create_copy(comic_edition, &book_condition));
    let position_only = CopyFields {
        box_position: Some(3),
        ..CopyFields::default()
    };
    invalid(db.create_copy(comic_edition, &position_only));
    let negative = CopyFields {
        purchase_price: Some(Amount::new(Currency::Usd, -1)),
        ..CopyFields::default()
    };
    invalid(db.create_copy(comic_edition, &negative));
}

#[test]
fn a_category_change_may_not_strand_fields() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (_, edition, _) = comic(&db, series, "1");
    let graded = CopyFields {
        grade: Grade::parse("9.0"),
        ..CopyFields::default()
    };
    db.create_copy(edition, &graded).unwrap();
    let mut fields = db.series(series).unwrap().fields;
    fields.category = Category::CollectedEdition;
    invalid(db.update_series(series, &fields));
    fields.category = Category::Magazine;
    db.update_series(series, &fields).unwrap();
}

// Trash --------------------------------------------------------------------

#[test]
fn trashing_a_series_hides_it_and_everything_in_it() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, edition, copy) = comic(&db, series, "121");
    let trash = db.trash_series(series).unwrap();

    assert!(db.list_series().unwrap().is_empty());
    assert!(db.issues_in_series(series).unwrap().is_empty());
    assert!(db.editions_of_issue(issue).unwrap().is_empty());
    assert!(db.copies_of_edition(edition).unwrap().is_empty());
    assert_eq!(db.copy(copy).unwrap().trash, Some(trash));
    assert_eq!(db.issue(issue).unwrap().trash, Some(trash));

    let groups = db.trash_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Amazing Spider-Man (1963)");
}

#[test]
fn nothing_can_be_added_to_or_edited_in_trash() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, edition, copy) = comic(&db, series, "1");
    db.trash_series(series).unwrap();
    assert!(matches!(
        db.create_issue(series, &IssueFields::new(number("2"))),
        Err(Error::InTrash)
    ));
    assert!(matches!(
        db.create_copy(edition, &CopyFields::default()),
        Err(Error::InTrash)
    ));
    assert!(matches!(
        db.update_issue(issue, &IssueFields::new(number("1"))),
        Err(Error::InTrash)
    ));
    assert!(matches!(db.trash_copy(copy), Err(Error::InTrash)));
}

#[test]
fn restore_puts_back_exactly_what_was_trashed() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, _, copy) = comic(&db, series, "121");
    comic(&db, series, "122");
    let before = db.issues_in_series(series).unwrap();
    let trash = db.trash_issue(issue).unwrap();
    assert_eq!(
        db.trash_groups().unwrap()[0].label,
        "Amazing Spider-Man (1963) #121"
    );
    assert_eq!(numbers(&db, series), ["122"]);
    db.restore(trash).unwrap();
    assert_eq!(db.issues_in_series(series).unwrap(), before);
    assert_eq!(db.copy(copy).unwrap().trash, None);
    assert!(db.trash_groups().unwrap().is_empty());
}

#[test]
fn a_group_cannot_be_restored_under_a_trashed_parent() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (_, _, copy) = comic(&db, series, "1");
    let copy_group = db.trash_copy(copy).unwrap();
    let series_group = db.trash_series(series).unwrap();
    assert!(matches!(
        db.restore(copy_group),
        Err(Error::ParentInTrash { parent }) if parent == "Amazing Spider-Man (1963)"
    ));
    db.restore(series_group).unwrap();
    // The copy was trashed on its own, so it stays in Trash.
    assert!(db.copy(copy).unwrap().trash.is_some());
    db.restore(copy_group).unwrap();
    assert_eq!(db.copy(copy).unwrap().trash, None);
}

#[test]
fn restoring_an_issue_whose_number_was_reused_is_refused() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, _, _) = comic(&db, series, "5");
    let trash = db.trash_issue(issue).unwrap();
    comic(&db, series, "5");
    assert!(matches!(
        db.restore(trash),
        Err(Error::DuplicateIssueNumber { number }) if number == "5"
    ));
    assert_eq!(db.issue(issue).unwrap().trash, Some(trash));
}

#[test]
fn purging_deletes_the_group_for_good() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, _, copy) = comic(&db, series, "1");
    let (keep, _, _) = comic(&db, series, "2");
    let trash = db.trash_issue(issue).unwrap();
    db.purge(trash).unwrap();
    assert!(matches!(db.issue(issue), Err(Error::NotFound)));
    assert!(matches!(db.copy(copy), Err(Error::NotFound)));
    assert!(db.issue(keep).is_ok());
    assert!(db.trash_groups().unwrap().is_empty());
}

#[test]
fn purging_a_parent_also_clears_groups_beneath_it() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (_, _, copy) = comic(&db, series, "1");
    db.trash_copy(copy).unwrap();
    let series_group = db.trash_series(series).unwrap();
    db.purge(series_group).unwrap();
    assert!(matches!(db.copy(copy), Err(Error::NotFound)));
    assert!(db.trash_groups().unwrap().is_empty());
}

#[test]
fn empty_trash_removes_every_group() {
    let db = db();
    let a = comic_series(&db, "Amazing Spider-Man");
    let b = comic_series(&db, "Batman");
    db.trash_series(a).unwrap();
    db.trash_series(b).unwrap();
    db.empty_trash().unwrap();
    assert!(db.trash_groups().unwrap().is_empty());
    assert!(matches!(db.series(a), Err(Error::NotFound)));
}

#[test]
fn a_reprinted_issue_survives_until_its_collection_goes_too() {
    let db = db();
    let asm = comic_series(&db, "Amazing Spider-Man");
    let (asm1, _, _) = comic(&db, asm, "1");
    let mut epic = SeriesFields::new("Epic Collection", Category::CollectedEdition);
    epic.start_year = Some(2014);
    let epic = db.create_series(&epic).unwrap();
    let volume = db
        .create_issue(epic, &IssueFields::new(number("1")))
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO collects (collection_issue_id, collected_issue_id, position)
             VALUES (?1, ?2, 1)",
            [volume.0, asm1.0],
        )
        .unwrap();

    db.trash_series(asm).unwrap();
    match db.empty_trash() {
        Err(Error::StillReprinted { issue, collection }) => {
            assert_eq!(issue, "Amazing Spider-Man (1963) #1");
            assert_eq!(collection, "Epic Collection (2014) #1");
        }
        other => panic!("expected StillReprinted, got {other:?}"),
    }
    // Nothing was deleted.
    assert!(db.issue(asm1).unwrap().trash.is_some());
    assert_eq!(db.trash_groups().unwrap().len(), 1);

    db.trash_series(epic).unwrap();
    db.empty_trash().unwrap();
    assert!(matches!(db.issue(asm1), Err(Error::NotFound)));
}

// Hostile files ------------------------------------------------------------

#[test]
fn values_that_break_the_rules_are_refused_on_read() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let (issue, edition, copy) = comic(&db, series, "1");
    // A file edited outside the app can hold values its CHECK constraints
    // forbid; reading must fail cleanly rather than trust them.
    db.conn
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    for (sql, id) in [
        ("UPDATE copy SET grade = 3 WHERE id = ?1", copy.0),
        (
            "UPDATE copy SET grade = 80, grade_type = 'eyeball' WHERE id = ?1",
            copy.0,
        ),
        (
            "UPDATE copy SET grade_type = NULL, purchase_price_units = 5 WHERE id = ?1",
            copy.0,
        ),
        (
            "UPDATE copy SET purchase_price_units = NULL, purchase_date = '2021-02-30' WHERE id = ?1",
            copy.0,
        ),
    ] {
        db.conn.execute(sql, [id]).unwrap();
        assert!(db.copy(copy).is_err(), "{sql}");
    }
    db.conn
        .execute(
            "UPDATE issue SET cover_month = 7, cover_year = NULL WHERE id = ?1",
            [issue.0],
        )
        .unwrap();
    assert!(db.issue(issue).is_err());
    db.conn
        .execute(
            "UPDATE edition SET isbn = '9781302904204' WHERE id = ?1",
            [edition.0],
        )
        .unwrap();
    assert!(db.edition(edition).is_err());
    db.conn
        .execute(
            "UPDATE series SET uuid = 'not-a-uuid-but-thirty-six-chars-long' WHERE id = ?1",
            [series.0],
        )
        .unwrap();
    assert!(db.series(series).is_err());
}

#[test]
fn dates_read_back_as_dates() {
    let db = db();
    let series = comic_series(&db, "Amazing Spider-Man");
    let mut f = IssueFields::new(number("1"));
    f.release_date = Some(Date::new(1963, 1, 10).unwrap());
    let issue = db.create_issue(series, &f).unwrap();
    assert_eq!(
        db.issue(issue).unwrap().fields.release_date,
        Some(Date::new(1963, 1, 10).unwrap())
    );
}

#[test]
fn a_reprint_is_named_even_when_trashed_before_its_series() {
    let db = db();
    let asm = comic_series(&db, "Amazing Spider-Man");
    let (asm1, _, _) = comic(&db, asm, "1");
    let epic = db
        .create_series(&SeriesFields::new(
            "Epic Collection",
            Category::CollectedEdition,
        ))
        .unwrap();
    let volume = db
        .create_issue(epic, &IssueFields::new(number("1")))
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO collects (collection_issue_id, collected_issue_id, position)
             VALUES (?1, ?2, 1)",
            [volume.0, asm1.0],
        )
        .unwrap();
    // The issue goes to Trash in its own group, then its series in another;
    // purging the series would delete the issue beneath it.
    db.trash_issue(asm1).unwrap();
    let series_group = db.trash_series(asm).unwrap();
    assert!(matches!(
        db.purge(series_group),
        Err(Error::StillReprinted { issue, .. }) if issue == "Amazing Spider-Man (1963) #1"
    ));
}
