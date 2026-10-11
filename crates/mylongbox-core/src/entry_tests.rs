//! Tests of saving books from the add and edit forms.

use crate::model::*;
use crate::{Amount, BookEntry, Currency, Database, Error, IssueNumber, SeriesChoice, Shelf};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn number(text: &str) -> Option<IssueNumber> {
    Some(IssueNumber::new(text).unwrap())
}

fn new_series(name: &str) -> SeriesChoice {
    SeriesChoice::New {
        name: name.into(),
        start_year: Some(1963),
        volume: None,
        publisher: Some("Marvel".into()),
    }
}

fn comic(series: &str, n: &str) -> BookEntry {
    BookEntry::new(Category::Comic, new_series(series), number(n))
}

fn names<I: crate::LookupId>(db: &Database) -> Vec<String> {
    db.list_lookups::<I>()
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

#[test]
fn adding_creates_everything_it_names() {
    let db = db();
    let mut e = comic("Amazing Spider-Man", "121");
    e.issue.title = "The Night Gwen Stacy Died".into();
    e.credits = vec![
        ("Gerry Conway".into(), Role::Writer),
        ("Gil Kane".into(), Role::Penciller),
        ("  ".into(), Role::Inker),
    ];
    e.characters = vec![("Gwen Stacy".into(), false)];
    e.story_arcs = vec![("Death of Gwen Stacy".into(), Some(1))];
    e.edition.cover_price = Some(Amount::new(Currency::Usd, 20));
    e.cover_artists = vec!["John Romita".into()];
    e.copy.grade = Grade::parse("9.4");
    e.copy.box_position = Some(3);
    e.storage_box = Some("Box 1".into());
    e.tags = vec!["Key".into()];

    let saved = db.add_book(&e).unwrap();
    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.label, "Amazing Spider-Man (1963) #121");
    assert_eq!(d.publisher.as_deref(), Some("Marvel"));
    assert_eq!(d.issue.fields.title, "The Night Gwen Stacy Died");
    assert_eq!(d.credits.len(), 2, "blank names are skipped");
    assert_eq!(d.story_arcs[0].part, Some(1));
    let copy = &d.editions[0].copies[0];
    assert_eq!(copy.copy.id, saved.copy);
    assert_eq!(copy.storage_box.as_deref(), Some("Box 1"));
    assert_eq!(copy.copy.fields.box_position, Some(3));
    assert_eq!(copy.tags[0].name, "Key");
    assert_eq!(d.editions[0].cover_artists[0].name, "John Romita");
}

#[test]
fn typed_names_reuse_entries_and_alternate_names() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    db.add_alias(kirby, "J. Kirby").unwrap();
    let mut e = comic("Fantastic Four", "1");
    e.credits = vec![("j. kirby".into(), Role::Penciller)];
    e.storage_box = Some("box 1".into());
    db.add_book(&e).unwrap();
    let mut again = comic("fantastic four", "2");
    again.storage_box = Some("Box 1".into());
    db.add_book(&again).unwrap();

    assert_eq!(names::<CreatorId>(&db), ["Jack Kirby"]);
    assert_eq!(names::<BoxId>(&db), ["box 1"]);
    assert_eq!(db.list_series().unwrap().len(), 1, "same name and year");
    assert_eq!(names::<PublisherId>(&db), ["Marvel"]);
}

#[test]
fn every_add_is_another_physical_copy() {
    let db = db();
    let e = comic("X-Men", "1");
    let first = db.add_book(&e).unwrap();
    let second = db.add_book(&e).unwrap();
    assert_ne!(first.copy, second.copy);
    assert_eq!(first.edition, second.edition);
    assert_eq!(db.browse(Shelf::Everything).unwrap().len(), 2);
}

#[test]
fn adding_to_a_listed_issue_fills_in_without_erasing() {
    let db = db();
    let mut first = comic("X-Men", "1");
    first.issue.synopsis = "Five students.".into();
    first.issue.title = "Old title".into();
    first.credits = vec![("Stan Lee".into(), Role::Writer)];
    first.characters = vec![("Cyclops".into(), false), ("Magneto".into(), true)];
    first.story_arcs = vec![("Origins".into(), Some(1))];
    let saved = db.add_book(&first).unwrap();

    let mut second = comic("X-Men", "1");
    second.issue.title = "X-Men".into();
    second.credits = vec![("Jack Kirby".into(), Role::Penciller)];
    // Not ticking "first appearance" again does not untick it.
    second.characters = vec![("Cyclops".into(), true), ("Magneto".into(), false)];
    second.story_arcs = vec![("Origins".into(), None)];
    second.edition.variant = "Cover B".into();
    let again = db.add_book(&second).unwrap();

    assert_eq!(again.issue, saved.issue);
    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.issue.fields.synopsis, "Five students.", "blank kept it");
    assert_eq!(d.issue.fields.title, "X-Men", "typed text replaced it");
    assert_eq!(d.credits.len(), 2);
    assert_eq!(d.story_arcs[0].part, Some(1), "no part given kept it");
    let firsts: Vec<_> = d
        .characters
        .iter()
        .map(|c| (c.name.as_str(), c.first_appearance))
        .collect();
    assert_eq!(firsts, [("Cyclops", true), ("Magneto", true)]);
    assert_eq!(d.editions.len(), 2, "another variant is another edition");
}

#[test]
fn editing_replaces_what_the_form_shows() {
    let db = db();
    let mut e = comic("X-Men", "1");
    e.credits = vec![
        ("Stan Lee".into(), Role::Writer),
        ("Jack Kirby".into(), Role::Penciller),
    ];
    e.issue.synopsis = "Five students.".into();
    e.tags = vec!["Key".into()];
    e.storage_box = Some("Box 1".into());
    let saved = db.add_book(&e).unwrap();

    let mut edit = db.book_entry(saved.copy).unwrap();
    assert_eq!(edit.credits.len(), 2);
    assert_eq!(edit.storage_box.as_deref(), Some("Box 1"));
    edit.credits.retain(|(name, _)| name == "Stan Lee");
    edit.issue.synopsis.clear();
    edit.issue.number = number("2");
    edit.tags.clear();
    edit.storage_box = None;
    edit.copy.grade = Grade::parse("8.0");
    let after = db.update_book(saved.copy, &edit).unwrap();
    assert_eq!(after, saved);

    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.label, "X-Men (1963) #2");
    assert_eq!(d.credits.len(), 1);
    assert_eq!(d.issue.fields.synopsis, "");
    let copy = &d.editions[0].copies[0];
    assert!(copy.tags.is_empty());
    assert_eq!(copy.storage_box, None);
    assert_eq!(copy.copy.fields.grade, Grade::parse("8.0"));
}

#[test]
fn an_unchanged_edit_changes_nothing() {
    let db = db();
    let mut e = comic("X-Men", "1");
    e.characters = vec![("Magneto".into(), true)];
    e.story_arcs = vec![("Origins".into(), None)];
    e.cover_artists = vec!["Jack Kirby".into()];
    let saved = db.add_book(&e).unwrap();
    let before = db.issue_detail(saved.issue).unwrap();
    let entry = db.book_entry(saved.copy).unwrap();
    db.update_book(saved.copy, &entry).unwrap();
    assert_eq!(db.issue_detail(saved.issue).unwrap(), before);
    assert_eq!(db.book_entry(saved.copy).unwrap(), entry);
}

#[test]
fn editing_into_another_series_moves_the_issue() {
    let db = db();
    let saved = db.add_book(&comic("X-Men", "5")).unwrap();
    let taken = db.add_book(&comic("Uncanny X-Men", "5")).unwrap();

    let mut edit = db.book_entry(saved.copy).unwrap();
    edit.series = SeriesChoice::Existing(taken.series);
    assert!(matches!(
        db.update_book(saved.copy, &edit),
        Err(Error::DuplicateIssueNumber { .. })
    ));

    edit.issue.number = number("6");
    let moved = db.update_book(saved.copy, &edit).unwrap();
    assert_eq!(moved.series, taken.series);
    assert_eq!(
        db.issue_detail(saved.issue).unwrap().label,
        "Uncanny X-Men (1963) #6"
    );
}

#[test]
fn a_failed_save_leaves_nothing_behind() {
    let db = db();
    let mut e = comic("X-Men", "1");
    e.credits = vec![("Stan Lee".into(), Role::Writer)];
    e.tags = vec!["Key".into()];
    e.copy.box_position = Some(0);
    e.storage_box = Some("Box 1".into());
    assert!(matches!(db.add_book(&e), Err(Error::Invalid(_))));

    assert!(db.list_series().unwrap().is_empty());
    assert!(names::<CreatorId>(&db).is_empty());
    assert!(names::<PublisherId>(&db).is_empty());
    assert!(names::<BoxId>(&db).is_empty());
    assert!(names::<TagId>(&db).is_empty());
}

#[test]
fn a_series_of_another_category_is_refused() {
    let db = db();
    let magazine = db
        .create_series(&SeriesFields::new("Heavy Metal", Category::Magazine))
        .unwrap();
    let e = BookEntry::new(
        Category::Comic,
        SeriesChoice::Existing(magazine),
        number("1"),
    );
    let why = match db.add_book(&e) {
        Err(Error::Invalid(why)) => why,
        other => panic!("{other:?}"),
    };
    assert!(why.contains("magazine"), "{why}");
}

#[test]
fn find_issue_spots_numbers_already_listed() {
    let db = db();
    let saved = db.add_book(&comic("X-Men", "Annual 1")).unwrap();
    let annual = IssueNumber::new("annual 1").unwrap();
    assert_eq!(
        db.find_issue(saved.series, Some(&annual)).unwrap(),
        Some(saved.issue)
    );
    let two = IssueNumber::new("2").unwrap();
    assert_eq!(db.find_issue(saved.series, Some(&two)).unwrap(), None);
    assert_eq!(db.find_issue(saved.series, None).unwrap(), None);
}

#[test]
fn adding_never_unmarks_or_clears_what_is_there() {
    let db = db();
    let mut first = comic("X-Men", "1");
    first.issue.is_key = true;
    first.issue.key_reason = "First appearance of the X-Men".into();
    first.edition.barcode = Some("071486027026".into());
    first.edition.cover_price = Some(Amount::new(Currency::Usd, 12));
    first.cover_artists = vec!["Jack Kirby".into()];
    let saved = db.add_book(&first).unwrap();

    let mut second = comic("X-Men", "1");
    second.cover_artists = vec!["Sol Brodsky".into()];
    let again = db.add_book(&second).unwrap();
    assert_eq!(again.edition, saved.edition);

    let d = db.issue_detail(saved.issue).unwrap();
    assert!(d.issue.fields.is_key);
    assert_eq!(d.issue.fields.key_reason, "First appearance of the X-Men");
    let edition = &d.editions[0];
    assert_eq!(
        edition.edition.fields.barcode.as_deref(),
        Some("071486027026")
    );
    assert_eq!(
        edition.edition.fields.cover_price,
        Some(Amount::new(Currency::Usd, 12))
    );
    assert_eq!(edition.cover_artists.len(), 2);
    assert_eq!(edition.copies.len(), 2);
}
