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
            imprint: String::new(),
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
    assert_eq!(suggested, ["X-Men (1991 series)"]);
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

#[test]
fn names_are_added_to_a_list_before_any_book_uses_them() {
    use crate::ShelfList;
    let db = db();
    let shelf = db
        .add_to_list(ShelfList::Characters, " Captain Vega ")
        .unwrap();
    let listed = db.shelf_list(ShelfList::Characters).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].shelf, listed[0].name.as_str()),
        (shelf, "Captain Vega")
    );
    assert_eq!(listed[0].copies, 0);
    assert!(matches!(
        db.add_to_list(ShelfList::Characters, "captain vega"),
        Err(Error::NameTaken { .. })
    ));
    assert!(db.add_to_list(ShelfList::Characters, "  ").is_err());
    assert!(db.add_to_list(ShelfList::Series, "X-Men").is_err());
    db.add_to_list(ShelfList::StorageBoxes, "Shelf 2").unwrap();
    assert_eq!(db.shelf_list(ShelfList::StorageBoxes).unwrap().len(), 1);
}

#[test]
fn a_series_is_added_with_its_publisher_and_cast() {
    use crate::ShelfList;
    let db = db();
    let mut fields = SeriesFields::new("Detective Comics", Category::Comic);
    fields.start_year = Some(1937);
    let id = db
        .add_series(&fields, "DC", &["Batman".into()], &["Bat-Family".into()])
        .unwrap();
    let read = db.series(id).unwrap().fields;
    assert_eq!(read.start_year, Some(1937));
    assert_eq!(db.lookup(read.publisher.unwrap()).unwrap().name, "DC");
    assert_eq!(db.series_characters(id).unwrap()[0].name, "Batman");
    assert_eq!(db.series_teams(id).unwrap()[0].name, "Bat-Family");
    let listed = db.shelf_list(ShelfList::Series).unwrap();
    assert_eq!(listed[0].name, "Detective Comics (1937 series)");
    assert_eq!(listed[0].copies, 0);
}

const LISTS: [crate::ShelfList; 7] = [
    crate::ShelfList::Series,
    crate::ShelfList::Publishers,
    crate::ShelfList::Creators,
    crate::ShelfList::Characters,
    crate::ShelfList::Teams,
    crate::ShelfList::StoryArcs,
    crate::ShelfList::StorageBoxes,
];

/// Adds a book, then deletes its only copy for good, leaving its issue
/// and edition in the catalog with no copy.
fn leftover(db: &Database, series: &str, n: &str, writer: &str, team: &str) -> crate::SavedBook {
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: series.into(),
            start_year: Some(1984),
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new(n).unwrap()),
    );
    e.credits = vec![(writer.into(), Role::Writer)];
    e.teams = vec![(team.into(), false)];
    e.characters = vec![("Ethan Zachary".into(), false)];
    let saved = db.add_book(&e).unwrap();
    db.trash_copy(saved.copy).unwrap();
    db.empty_trash().unwrap();
    saved
}

#[test]
fn names_that_hold_no_books_are_unused() {
    use crate::ShelfList;
    let db = db();
    let kept = add(&db, "X-Men", 1963, "1", "Stan Lee");
    let trashed = add(&db, "Tales of Suspense", 1959, "39", "Larry Lieber");
    db.trash_issue(trashed.issue).unwrap();
    let left = leftover(&db, "Transformers", "24", "Bob Budiansky", "Combaticons");
    for name in ["Unused One", "Unused Two"] {
        db.add_to_list(ShelfList::Creators, name).unwrap();
    }
    // Used only by a series' cast.
    let wolverine = db.create_lookup::<CharacterId>("Wolverine").unwrap();
    db.set_series_characters(db.issue(kept.issue).unwrap().series, &[wolverine])
        .unwrap();
    db.add_to_list(ShelfList::Characters, "Nobody").unwrap();
    db.create_series(&SeriesFields::new("Empty Run", Category::Comic))
        .unwrap();

    assert_eq!(
        db.unused_in_list(ShelfList::Creators).unwrap(),
        ["Bob Budiansky", "Larry Lieber", "Unused One", "Unused Two"],
        "credits on a book in Trash or an issue with no copy hold no books"
    );
    assert_eq!(
        db.unused_in_list(ShelfList::Teams).unwrap(),
        ["Combaticons"]
    );
    assert_eq!(
        db.unused_in_list(ShelfList::Characters).unwrap(),
        ["Ethan Zachary", "Nobody"]
    );
    assert_eq!(
        db.unused_in_list(ShelfList::Series).unwrap(),
        [
            "Empty Run",
            "Tales of Suspense (1959 series)",
            "Transformers (1984 series)"
        ]
    );
    // What Remove Unused offers is exactly what the list shows as empty.
    for list in LISTS {
        let empty: Vec<String> = db
            .shelf_list(list)
            .unwrap()
            .into_iter()
            .filter(|e| e.copies == 0)
            .map(|e| e.name)
            .collect();
        assert_eq!(db.unused_in_list(list).unwrap(), empty, "{list:?}");
    }

    assert_eq!(db.remove_unused(ShelfList::Teams).unwrap(), 1);
    assert!(db.find_lookup::<TeamId>("Combaticons").unwrap().is_none());
    // The issue with no copy stays, less the team.
    assert!(db.teams(left.issue).unwrap().is_empty());
    assert_eq!(db.credits(left.issue).unwrap().len(), 1);

    assert_eq!(db.remove_unused(ShelfList::Creators).unwrap(), 4);
    assert_eq!(db.remove_unused(ShelfList::Characters).unwrap(), 2);
    assert_eq!(db.remove_unused(ShelfList::Series).unwrap(), 3);
    for list in LISTS {
        assert!(db.unused_in_list(list).unwrap().is_empty(), "{list:?}");
        assert_eq!(db.remove_unused(list).unwrap(), 0, "{list:?}");
    }
    assert!(db.issue(left.issue).is_err());
    assert!(db.find_lookup::<CreatorId>("Stan Lee").unwrap().is_some());
    assert!(
        db.find_lookup::<CharacterId>("Wolverine")
            .unwrap()
            .is_some()
    );
    // The Trash group of the deleted series' issue went with it.
    assert!(db.trash_groups().unwrap().is_empty());
    assert_eq!(db.shelf_list(ShelfList::Series).unwrap().len(), 1);
}

#[test]
fn a_series_reprinted_in_a_book_you_own_is_kept() {
    use crate::ShelfList;
    let db = db();
    let line = |name: &str| {
        db.create_series(&SeriesFields::new(name, Category::CollectedEdition))
            .unwrap()
    };
    let volume = |series: SeriesId, n: &str| {
        db.create_issue(
            series,
            &IssueFields::new(Some(IssueNumber::new(n).unwrap())),
        )
        .unwrap()
    };
    let own = |issue: IssueId| {
        let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
        db.create_copy(edition, &CopyFields::default()).unwrap()
    };
    // Owned: its contents stay.
    let masterworks = line("Masterworks");
    let owned = volume(masterworks, "1");
    own(owned);
    db.set_collects(owned, "Amazing Spider-Man (1963) 1")
        .unwrap();
    // A second volume with no copy, in a line that stays because the
    // first is owned: its contents stay too.
    let unowned = volume(masterworks, "2");
    db.set_collects(unowned, "Tales to Astonish (1959) 27")
        .unwrap();
    // A line with no copy at all goes, and so does what only it reprints.
    let omnibus = line("Omnibus");
    let gone = volume(omnibus, "1");
    db.set_collects(gone, "Strange Tales (1951) 110").unwrap();

    let unused = db.unused_in_list(ShelfList::Series).unwrap();
    assert_eq!(unused, ["Omnibus", "Strange Tales (1951 series)"]);
    assert_eq!(db.remove_unused(ShelfList::Series).unwrap(), 2);
    assert!(db.series(omnibus).is_err());
    assert_eq!(db.collects(owned).unwrap().len(), 1);
    assert_eq!(db.collects(unowned).unwrap().len(), 1);
    assert_eq!(db.remove_unused(ShelfList::Series).unwrap(), 0);
}

#[test]
fn removing_unused_names_clears_them_from_books_in_trash() {
    use crate::ShelfList;
    let db = db();
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: "Ka-Zar".into(),
            start_year: Some(1981),
            volume: None,
            publisher: Some("Gone Publisher".into()),
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new("1").unwrap()),
    );
    e.storage_box = Some("Box 9".into());
    e.cover_artists = vec!["Cover Artist".into()];
    e.signatures = vec![(
        "Signer".into(),
        SignatureFields::new(Verification::Unverified),
    )];
    let saved = db.add_book(&e).unwrap();
    let trash = db.trash_copy(saved.copy).unwrap();

    assert_eq!(
        db.unused_in_list(ShelfList::Publishers).unwrap(),
        ["Gone Publisher"]
    );
    assert_eq!(
        db.unused_in_list(ShelfList::StorageBoxes).unwrap(),
        ["Box 9"]
    );
    assert_eq!(
        db.unused_in_list(ShelfList::Creators).unwrap(),
        ["Cover Artist", "Signer"]
    );
    for list in [
        ShelfList::Publishers,
        ShelfList::StorageBoxes,
        ShelfList::Creators,
    ] {
        db.remove_unused(list).unwrap();
    }
    // Restoring brings the book back without them.
    db.restore(trash).unwrap();
    let copy = db.copy(saved.copy).unwrap();
    assert_eq!(copy.fields.storage_box, None);
    assert!(db.signatures(saved.copy).unwrap().is_empty());
    assert!(db.cover_artists(saved.edition).unwrap().is_empty());
    assert_eq!(db.series(saved.series).unwrap().fields.publisher, None);
}

fn lookalike(db: &Database, volume: Option<u32>, n: &str) -> SeriesId {
    let mut fields = SeriesFields::new("Fantastic Four", Category::Comic);
    fields.start_year = Some(1961);
    fields.volume = volume;
    let id = db.create_series(&fields).unwrap();
    db.create_issue(id, &IssueFields::new(Some(IssueNumber::new(n).unwrap())))
        .unwrap();
    id
}

#[test]
fn lookalike_series_are_told_apart_by_volume_and_merge_either_way() {
    let db = db();
    let blank = lookalike(&db, None, "29");
    let vol_1 = lookalike(&db, Some(1), "3");
    assert_eq!(
        db.shelf_suggestions(Shelf::Series(blank), "fantastic")
            .unwrap(),
        ["Fantastic Four (1961 series, Vol. 1)"]
    );
    assert_eq!(
        db.shelf_suggestions(Shelf::Series(vol_1), "fantastic")
            .unwrap(),
        ["Fantastic Four (1961 series, no volume)"]
    );
    // The label both share means the other one, never the series itself.
    let kept = db
        .merge_shelf(Shelf::Series(blank), "Fantastic Four (1961 series)")
        .unwrap();
    assert_eq!(kept, Shelf::Series(vol_1));
    assert_eq!(db.issues_in_series(vol_1).unwrap().len(), 2);

    let db = Database::open_in_memory().unwrap();
    let blank = lookalike(&db, None, "29");
    let vol_1 = lookalike(&db, Some(1), "3");
    let kept = db
        .merge_shelf(
            Shelf::Series(vol_1),
            "fantastic four (1961 series, no volume)",
        )
        .unwrap();
    assert_eq!(kept, Shelf::Series(blank));
}

#[test]
fn a_label_that_names_several_series_is_refused() {
    let db = db();
    let from = lookalike(&db, None, "1");
    lookalike(&db, Some(1), "2");
    lookalike(&db, Some(2), "3");
    let refused = db.merge_shelf(Shelf::Series(from), "Fantastic Four (1961 series)");
    assert!(
        matches!(&refused, Err(Error::Invalid(why)) if why.contains("more than one series")),
        "{refused:?}"
    );
    assert_eq!(db.list_series().unwrap().len(), 3);
    let kept = db
        .merge_shelf(Shelf::Series(from), "Fantastic Four (1961 series, Vol. 2)")
        .unwrap();
    assert_eq!(
        db.issues_in_series(match kept {
            Shelf::Series(id) => id,
            _ => unreachable!(),
        })
        .unwrap()
        .len(),
        2
    );
}
