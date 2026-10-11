//! Tests of saving books from the add and edit forms.

use crate::model::*;
use crate::{
    Amount, BookEntry, Currency, Database, Error, IssueNumber, Prefill, SeriesChoice, Shelf,
};

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
        magazine_format: None,
        black_and_white: None,
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

fn book(line: &str, n: Option<&str>) -> BookEntry {
    BookEntry::new(
        Category::CollectedEdition,
        SeriesChoice::New {
            name: line.into(),
            start_year: Some(2014),
            volume: None,
            publisher: None,
            magazine_format: None,
            black_and_white: None,
        },
        n.and_then(number),
    )
}

#[test]
fn magazines_keep_their_format_and_article_credits() {
    let db = db();
    let mut e = BookEntry::new(
        Category::Magazine,
        SeriesChoice::New {
            name: "Savage Sword of Conan".into(),
            start_year: Some(1974),
            volume: None,
            publisher: None,
            magazine_format: Some(MagazineFormat::Standard),
            black_and_white: Some(true),
        },
        number("1"),
    );
    e.credits = vec![("Roy Thomas".into(), Role::Article)];
    e.copy.grade = Grade::parse("8.5");
    let saved = db.add_book(&e).unwrap();
    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.series.fields.category, Category::Magazine);
    assert_eq!(d.series.fields.black_and_white, Some(true));
    assert_eq!(
        d.series.fields.magazine_format,
        Some(MagazineFormat::Standard)
    );
    assert_eq!(d.credits[0].role, Role::Article);
}

#[test]
fn collected_editions_save_their_book_details_and_contents() {
    let db = db();
    let mut e = book("Epic Collection: Amazing Spider-Man", Some("1"));
    e.edition.isbn = Some(Isbn::parse("978-0-306-40615-7").unwrap());
    e.edition.binding = Some(Binding::Softcover);
    e.edition.has_dust_jacket = Some(false);
    e.copy.book_condition = Some(BookCondition::LikeNew);
    e.collects = "Amazing Spider-Man (1963) 1-3; Amazing Fantasy (1962) 15".into();
    let saved = db.add_book(&e).unwrap();

    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.collects.len(), 4);
    let fields = &d.editions[0].edition.fields;
    assert_eq!(fields.binding, Some(Binding::Softcover));
    assert!(fields.isbn.is_some());
    assert_eq!(
        d.editions[0].copies[0].copy.fields.book_condition,
        Some(BookCondition::LikeNew)
    );

    let mut edit = db.book_entry(saved.copy).unwrap();
    assert_eq!(
        edit.collects,
        "Amazing Spider-Man (1963) 1\u{2013}3; Amazing Fantasy (1962) 15"
    );
    edit.collects = "Amazing Spider-Man (1963) 1-2".into();
    db.update_book(saved.copy, &edit).unwrap();
    assert_eq!(db.issue_detail(saved.issue).unwrap().collects.len(), 2);
    edit.collects.clear();
    db.update_book(saved.copy, &edit).unwrap();
    assert!(db.issue_detail(saved.issue).unwrap().collects.is_empty());
}

#[test]
fn adding_another_copy_of_a_book_keeps_its_contents_unless_retyped() {
    let db = db();
    let mut e = book("Omnibus", Some("1"));
    e.collects = "X-Men (1963) 1-5".into();
    let saved = db.add_book(&e).unwrap();
    let again = db.add_book(&book("Omnibus", Some("1"))).unwrap();
    assert_eq!(again.issue, saved.issue);
    assert_eq!(db.issue_detail(saved.issue).unwrap().collects.len(), 5);

    let mut retyped = book("Omnibus", Some("1"));
    retyped.collects = "X-Men (1963) 1-2".into();
    db.add_book(&retyped).unwrap();
    assert_eq!(db.issue_detail(saved.issue).unwrap().collects.len(), 2);
}

#[test]
fn standalone_books_are_each_their_own_entry() {
    let db = db();
    let mut first = book("Graphic Novels", None);
    first.issue.title = "Watchmen".into();
    let mut second = book("Graphic Novels", None);
    second.issue.title = "Maus".into();
    let a = db.add_book(&first).unwrap();
    let b = db.add_book(&second).unwrap();
    assert_eq!(a.series, b.series);
    assert_ne!(a.issue, b.issue);
}

#[test]
fn bad_contents_undo_the_whole_book() {
    let db = db();
    let mut e = book("Omnibus", Some("1"));
    e.collects = "X-Men (1963) 1-5; Avengers (1963) 10-1".into();
    assert!(matches!(db.add_book(&e), Err(Error::Invalid(_))));
    assert!(db.list_series().unwrap().is_empty());
    assert!(db.browse(Shelf::Everything).unwrap().is_empty());
}

#[test]
fn contents_belong_only_to_collected_editions() {
    let db = db();
    let mut e = comic("X-Men", "1");
    e.collects = "Avengers (1963) 1".into();
    assert!(matches!(db.add_book(&e), Err(Error::Invalid(_))));
    assert!(db.list_series().unwrap().is_empty());
}

/// X-Men #41, fully described, in Box 1 at position 7.
fn described(db: &Database, n: &str) -> crate::SavedBook {
    let mut e = comic("X-Men", n);
    e.issue.title = "The Sentinels Live".into();
    e.issue.cover_date = CoverDate::new(1967, Some(12));
    e.issue.synopsis = "A synopsis.".into();
    e.issue.is_key = true;
    e.credits = vec![("Roy Thomas".into(), Role::Writer)];
    e.characters = vec![("Cyclops".into(), true)];
    e.teams = vec![("X-Men".into(), false)];
    e.story_arcs = vec![("Sentinels".into(), Some(1))];
    e.edition.variant = "Cover B".into();
    e.edition.distribution = Some(Distribution::Newsstand);
    e.edition.cover_price = Some(Amount::new(Currency::Usd, 12));
    e.cover_artists = vec!["Don Heck".into()];
    e.copy.grade = Grade::parse("7.0");
    e.copy.purchase_date = Some(crate::Date::new(2024, 5, 1).unwrap());
    e.copy.purchase_price = Some(Amount::new(Currency::Usd, 4000));
    e.copy.seller = "Local shop".into();
    e.copy.box_position = Some(7);
    e.copy.notes = "Small tear.".into();
    e.storage_box = Some("Box 1".into());
    e.tags = vec!["Key".into()];
    db.add_book(&e).unwrap()
}

#[test]
fn next_issue_carries_the_run_forward() {
    let db = db();
    let saved = described(&db, "41");
    let next = db.prefilled_entry(saved.copy, Prefill::NextIssue).unwrap();

    assert_eq!(next.series, SeriesChoice::Existing(saved.series));
    assert_eq!(next.issue.number, number("42"));
    assert_eq!(next.issue.cover_date, CoverDate::new(1968, Some(1)));
    assert_eq!(next.issue.title, "");
    assert_eq!(next.issue.synopsis, "");
    assert!(!next.issue.is_key);
    assert_eq!(next.credits, [("Roy Thomas".to_owned(), Role::Writer)]);
    assert!(next.characters.is_empty());
    assert!(next.story_arcs.is_empty());
    assert_eq!(next.teams.len(), 1);
    assert_eq!(next.edition.variant, "");
    assert_eq!(next.edition.distribution, Some(Distribution::Newsstand));
    assert_eq!(
        next.edition.cover_price,
        Some(Amount::new(Currency::Usd, 12))
    );
    assert!(next.cover_artists.is_empty());
    assert_eq!(next.copy.grade, None);
    assert_eq!(next.copy.purchase_price, None);
    assert_eq!(next.copy.seller, "Local shop");
    assert_eq!(next.copy.notes, "");
    assert_eq!(next.copy.box_position, Some(8));
    assert_eq!(next.storage_box.as_deref(), Some("Box 1"));
    assert!(next.tags.is_empty());

    let added = db.add_book(&next).unwrap();
    assert_eq!(added.series, saved.series);
    assert_ne!(added.issue, saved.issue);
    assert_eq!(
        db.issue_detail(added.issue).unwrap().label,
        "X-Men (1963) #42"
    );
}

#[test]
fn next_issue_advances_prefixed_numbers_and_stops_without_digits() {
    let db = db();
    let annual = db.add_book(&comic("X-Men", "Annual 3")).unwrap();
    let next = db.prefilled_entry(annual.copy, Prefill::NextIssue).unwrap();
    assert_eq!(next.issue.number, number("Annual 4"));

    let alpha = db.add_book(&comic("X-Men", "Alpha")).unwrap();
    let next = db.prefilled_entry(alpha.copy, Prefill::NextIssue).unwrap();
    assert_eq!(next.issue.number, None);
}

#[test]
fn next_volume_keeps_the_binding_and_drops_the_contents() {
    let db = db();
    let mut e = book("Omnibus", Some("2"));
    e.edition.binding = Some(Binding::Hardcover);
    e.edition.isbn = Some(Isbn::parse("978-0-306-40615-7").unwrap());
    e.collects = "X-Men (1963) 1-5".into();
    let saved = db.add_book(&e).unwrap();
    let next = db.prefilled_entry(saved.copy, Prefill::NextIssue).unwrap();
    assert_eq!(next.issue.number, number("3"));
    assert_eq!(next.edition.binding, Some(Binding::Hardcover));
    assert_eq!(next.edition.isbn, None);
    assert_eq!(next.collects, "");
}

#[test]
fn another_copy_joins_the_same_edition() {
    let db = db();
    let saved = described(&db, "41");
    let again = db
        .prefilled_entry(saved.copy, Prefill::AnotherCopy)
        .unwrap();
    assert_eq!(again.issue.title, "The Sentinels Live");
    assert_eq!(again.edition.variant, "Cover B");
    assert_eq!(again.copy.grade, None);
    assert_eq!(again.copy.seller, "");
    assert_eq!(again.copy.box_position, Some(8));

    let added = db.add_book(&again).unwrap();
    assert_eq!(added.edition, saved.edition);
    assert_ne!(added.copy, saved.copy);
    let d = db.issue_detail(saved.issue).unwrap();
    assert_eq!(d.editions[0].copies.len(), 2);
    assert_eq!(d.characters.len(), 1, "nothing about the issue changed");
    assert_eq!(d.issue.fields.synopsis, "A synopsis.");
}

#[test]
fn a_variant_starts_a_new_edition_of_the_same_issue() {
    let db = db();
    let saved = described(&db, "41");
    let mut variant = db.prefilled_entry(saved.copy, Prefill::Variant).unwrap();
    assert_eq!(variant.issue.title, "The Sentinels Live");
    assert_eq!(variant.edition, EditionFields::default());
    assert!(variant.cover_artists.is_empty());
    variant.edition.variant = "Cover C".into();

    let added = db.add_book(&variant).unwrap();
    assert_eq!(added.issue, saved.issue);
    assert_ne!(added.edition, saved.edition);
}

#[test]
fn prefilling_from_a_trashed_copy_is_refused() {
    let db = db();
    let saved = described(&db, "41");
    db.trash_copy(saved.copy).unwrap();
    assert!(matches!(
        db.prefilled_entry(saved.copy, Prefill::NextIssue),
        Err(Error::InTrash)
    ));
}
