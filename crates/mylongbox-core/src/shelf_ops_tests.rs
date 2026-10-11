//! Tests of managing the entry behind a shelf.

use crate::model::*;
use crate::{BookEntry, Database, Error, IssueNumber, SeriesChoice, Shelf};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn add(db: &Database, series: &str, year: u16, n: &str, writer: &str) -> crate::SavedBook {
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: series.into(),
            start_year: Some(year),
            volume: None,
            publisher: None,
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new(n).unwrap()),
    );
    e.credits = vec![(writer.into(), Role::Writer)];
    e.storage_box = Some("Box 1".into());
    db.add_book(&e).unwrap()
}

fn creator(db: &Database, name: &str) -> CreatorId {
    db.find_lookup::<CreatorId>(name).unwrap().unwrap()
}

#[test]
fn shelves_rename_their_entries() {
    let db = db();
    let saved = add(&db, "X-Men", 1963, "1", "Stan Lee");
    let stan = creator(&db, "Stan Lee");
    db.rename_shelf(Shelf::Creator(stan), "Stan \"The Man\" Lee")
        .unwrap();
    assert_eq!(db.lookup(stan).unwrap().name, "Stan \"The Man\" Lee");
    db.rename_shelf(Shelf::Series(saved.series), "Uncanny X-Men")
        .unwrap();
    assert_eq!(
        db.series(saved.series).unwrap().fields.name,
        "Uncanny X-Men"
    );
    assert!(matches!(
        db.rename_shelf(Shelf::Everything, "x"),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn merging_moves_everything_to_the_named_entry() {
    let db = db();
    let a = add(&db, "Fantastic Four", 1961, "1", "J. Kirby");
    add(&db, "Fantastic Four", 1961, "2", "Jack Kirby");
    let alias = creator(&db, "J. Kirby");
    let jack = creator(&db, "Jack Kirby");

    let kept = db.merge_shelf(Shelf::Creator(alias), "jack kirby").unwrap();
    assert_eq!(kept, Shelf::Creator(jack));
    assert_eq!(db.credits(a.issue).unwrap()[0].name, "Jack Kirby");
    assert_eq!(db.shelf_aliases(kept).unwrap(), ["J. Kirby"]);
    add(&db, "Fantastic Four", 1961, "3", "Jack Kurtzman");
    assert_eq!(
        db.shelf_suggestions(kept, "jack").unwrap(),
        ["Jack Kurtzman"],
        "never itself"
    );

    assert!(matches!(
        db.merge_shelf(kept, "Jack Kirby"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.merge_shelf(kept, "Nobody"),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn series_merge_by_their_listed_name_and_year() {
    let db = db();
    let old = add(&db, "X-Men", 1963, "1", "Stan Lee");
    let new = add(&db, "X-Men", 1991, "1", "Chris Claremont");
    let typo = add(&db, "X-Men", 1964, "2", "Stan Lee");
    assert!(matches!(
        db.merge_shelf(Shelf::Series(typo.series), "X-Men"),
        Err(Error::Invalid(_))
    ));
    let kept = db
        .merge_shelf(Shelf::Series(typo.series), "x-men (1963)")
        .unwrap();
    assert_eq!(kept, Shelf::Series(old.series));
    assert_eq!(db.issues_in_series(old.series).unwrap().len(), 2);
    assert_eq!(db.issues_in_series(new.series).unwrap().len(), 1);
    let suggested = db
        .shelf_suggestions(Shelf::Series(old.series), "x-men")
        .unwrap();
    assert_eq!(suggested, ["X-Men (1991)"]);
}

#[test]
fn deleting_a_series_goes_to_trash_and_a_used_name_is_refused() {
    let db = db();
    let saved = add(&db, "X-Men", 1963, "1", "Stan Lee");
    let stan = creator(&db, "Stan Lee");
    assert!(matches!(
        db.delete_shelf(Shelf::Creator(stan)),
        Err(Error::InUse { .. })
    ));
    let unused = db.create_lookup::<CreatorId>("Nobody").unwrap();
    assert_eq!(db.delete_shelf(Shelf::Creator(unused)).unwrap(), None);

    let trash = db.delete_shelf(Shelf::Series(saved.series)).unwrap();
    assert!(trash.is_some());
    assert!(db.list_series().unwrap().is_empty());
}

#[test]
fn alternate_names_come_and_go() {
    let db = db();
    add(&db, "X-Men", 1963, "1", "Jack Kirby");
    let jack = Shelf::Creator(creator(&db, "Jack Kirby"));
    db.add_shelf_alias(jack, "King Kirby").unwrap();
    assert_eq!(db.shelf_aliases(jack).unwrap(), ["King Kirby"]);
    db.remove_shelf_alias(jack, "King Kirby").unwrap();
    assert!(db.shelf_aliases(jack).unwrap().is_empty());
    let shelf_box = Shelf::StorageBox(db.find_lookup::<BoxId>("Box 1").unwrap().unwrap());
    assert!(!Database::shelf_has_aliases(shelf_box));
    assert!(Database::shelf_has_aliases(jack));
}
