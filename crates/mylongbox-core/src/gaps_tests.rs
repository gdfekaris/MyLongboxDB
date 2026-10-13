//! Tests of the collection views: gaps in a run, duplicates, and books
//! that need attention.

use std::collections::BTreeSet;

use crate::gaps::{missing_ranges, whole_number};
use crate::model::*;
use crate::{
    Amount, BookEntry, CoverPhoto, Currency, Database, IssueNumber, Need, SeriesChoice, Shelf,
};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

/// Adds a copy of `series` #`number`, creating the series if needed.
fn add(db: &Database, series: &str, number: &str) -> CopyId {
    add_in(db, Category::Comic, series, number)
}

fn add_in(db: &Database, category: Category, series: &str, number: &str) -> CopyId {
    let existing = db
        .list_series()
        .unwrap()
        .into_iter()
        .find(|s| s.fields.name == series);
    let choice = match existing {
        Some(s) => SeriesChoice::Existing(s.id),
        None => SeriesChoice::New {
            name: series.into(),
            start_year: Some(1963),
            volume: None,
            publisher: None,
            magazine_format: None,
            black_and_white: None,
        },
    };
    let e = BookEntry::new(category, choice, Some(IssueNumber::new(number).unwrap()));
    db.add_book(&e).unwrap().copy
}

fn series_id(db: &Database, name: &str) -> SeriesId {
    db.list_series()
        .unwrap()
        .into_iter()
        .find(|s| s.fields.name == name)
        .unwrap()
        .id
}

/// Each series with gaps, by name, with its missing ranges.
fn gaps(db: &Database, count_collected: bool) -> Vec<(String, Vec<(u32, u32)>)> {
    db.gaps(count_collected)
        .unwrap()
        .into_iter()
        .map(|g| (g.name, g.missing))
        .collect()
}

#[test]
fn only_plain_whole_numbers_make_a_run() {
    assert_eq!(whole_number("7"), Some(7));
    assert_eq!(whole_number(" 007 "), Some(7));
    assert_eq!(whole_number("0"), Some(0));
    for not in ["Annual 1", "1.5", "½", "1a", "-1", "", "#3", "+5"] {
        assert_eq!(whole_number(not), None, "{not}");
    }
}

#[test]
fn missing_numbers_are_grouped_into_ranges() {
    let owned: BTreeSet<u32> = [1, 2, 5, 9, 10].into();
    assert_eq!(missing_ranges(&owned, 1, 12), [(3, 4), (6, 8), (11, 12)]);
    assert_eq!(missing_ranges(&owned, 1, 2), []);
    assert_eq!(missing_ranges(&[4].into(), 1, 4), [(1, 3)]);
}

#[test]
fn runs_go_from_the_lowest_owned_to_the_highest() {
    let db = db();
    for n in ["1", "2", "5", "5", "8", "Annual 1", "6.5"] {
        add(&db, "X-Men", n);
    }
    for n in ["10", "11", "12"] {
        add(&db, "Avengers", n);
    }
    assert_eq!(
        gaps(&db, false),
        [("X-Men".to_owned(), vec![(3, 4), (6, 7)])]
    );
    let found = db.gaps(false).unwrap().remove(0);
    assert_eq!((found.first, found.last, found.owned), (1, 8, 4));
    assert_eq!(found.missing_count(), 4);
}

#[test]
fn a_known_issue_count_extends_the_run_from_number_one() {
    let db = db();
    for n in ["3", "4"] {
        add(&db, "Zot!", n);
    }
    let zot = series_id(&db, "Zot!");
    let mut fields = db.series(zot).unwrap().fields;
    fields.known_issue_count = Some(6);
    db.update_series(zot, &fields).unwrap();
    assert_eq!(
        gaps(&db, false),
        [("Zot!".to_owned(), vec![(1, 2), (5, 6)])]
    );
    // An issue numbered 0 starts the run there instead.
    add(&db, "Zot!", "0");
    assert_eq!(
        gaps(&db, false),
        [("Zot!".to_owned(), vec![(1, 2), (5, 6)])]
    );
    let found = db.gaps(false).unwrap().remove(0);
    assert_eq!((found.first, found.last, found.owned), (0, 6, 3));
}

#[test]
fn issues_in_owned_collected_editions_can_fill_gaps() {
    let db = db();
    for n in ["1", "4"] {
        add(&db, "Fantastic Four", n);
    }
    let mut book = BookEntry::new(
        Category::CollectedEdition,
        SeriesChoice::New {
            name: "Masterworks".into(),
            start_year: None,
            volume: None,
            publisher: None,
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new("1").unwrap()),
    );
    book.collects = "Fantastic Four (1963) 2-3".into();
    let masterworks = db.add_book(&book).unwrap().copy;
    assert_eq!(
        gaps(&db, false),
        [("Fantastic Four".to_owned(), vec![(2, 3)])]
    );
    assert!(gaps(&db, true).is_empty());
    // Only books still owned count.
    db.trash_copy(masterworks).unwrap();
    assert_eq!(
        gaps(&db, true),
        [("Fantastic Four".to_owned(), vec![(2, 3)])]
    );
}

#[test]
fn volumes_of_a_book_line_have_gaps_too() {
    let db = db();
    for n in ["1", "3"] {
        add_in(&db, Category::CollectedEdition, "Epic Collection", n);
    }
    let found = db.gaps(false).unwrap();
    assert_eq!(found[0].category, Category::CollectedEdition);
    assert_eq!(found[0].missing, [(2, 2)]);
}

#[test]
fn trashed_copies_do_not_count_as_owned() {
    let db = db();
    add(&db, "Hulk", "1");
    let two = add(&db, "Hulk", "2");
    add(&db, "Hulk", "3");
    assert!(gaps(&db, false).is_empty());
    db.trash_copy(two).unwrap();
    assert_eq!(gaps(&db, false), [("Hulk".to_owned(), vec![(2, 2)])]);
}

fn shelf(db: &Database, shelf: Shelf) -> BTreeSet<CopyId> {
    db.browse(shelf)
        .unwrap()
        .into_iter()
        .map(|r| r.copy)
        .collect()
}

#[test]
fn duplicates_are_editions_owned_more_than_once() {
    let db = db();
    let first = add(&db, "Thor", "1");
    let edition = db.copy(first).unwrap().edition;
    let second = db.create_copy(edition, &CopyFields::default()).unwrap();
    add(&db, "Thor", "2");
    assert_eq!(shelf(&db, Shelf::Duplicates), [first, second].into());
    db.trash_copy(second).unwrap();
    assert!(shelf(&db, Shelf::Duplicates).is_empty());
}

#[test]
fn books_missing_a_photo_grade_box_or_value_need_attention() {
    let db = db();
    let complete_in = |db: &Database, category, series, number| {
        let copy = add_in(db, category, series, number);
        let mut fields = db.copy(copy).unwrap().fields;
        // A book condition stands in for a grade.
        if category == Category::CollectedEdition {
            fields.book_condition = Some(BookCondition::LikeNew);
        } else {
            fields.grade = Grade::parse("9.0");
        }
        let shelf = db.find_lookup::<BoxId>("Box 1").unwrap();
        fields.storage_box =
            Some(shelf.unwrap_or_else(|| db.create_lookup::<BoxId>("Box 1").unwrap()));
        db.update_copy(copy, &fields).unwrap();
        db.add_value(
            copy,
            "2024-01-01".parse().unwrap(),
            Amount::parse(Currency::Usd, "10").unwrap(),
            "",
        )
        .unwrap();
        for side in [Side::Front, Side::Back] {
            db.set_photo(
                copy,
                side,
                &CoverPhoto {
                    full_jpeg: vec![0xFF, 0xD8, 0xFF],
                    thumb_jpeg: vec![0xFF, 0xD8, 0xFF],
                    width: 1,
                    height: 1,
                    sha256: [0; 32],
                },
            )
            .unwrap();
        }
        copy
    };
    let complete = |db: &Database, number| complete_in(db, Category::Comic, "Daredevil", number);
    let done = complete(&db, "1");
    let no_photo = complete(&db, "2");
    db.remove_photo(no_photo, Side::Front).unwrap();
    let no_grade = complete(&db, "3");
    let mut fields = db.copy(no_grade).unwrap().fields;
    fields.grade = None;
    db.update_copy(no_grade, &fields).unwrap();
    let no_box = complete(&db, "4");
    let mut fields = db.copy(no_box).unwrap().fields;
    fields.storage_box = None;
    db.update_copy(no_box, &fields).unwrap();
    let no_value = complete(&db, "5");
    let value = db.values(no_value).unwrap().remove(0);
    db.delete_value(value.id).unwrap();
    let bare = add(&db, "Daredevil", "6");
    let no_back = complete(&db, "8");
    db.remove_photo(no_back, Side::Back).unwrap();
    let book = complete_in(&db, Category::CollectedEdition, "Omnibus", "1");
    assert_eq!(
        shelf(&db, Shelf::NeedsAttention),
        [no_photo, no_grade, no_box, no_value, bare, no_back].into()
    );
    // Each book says what it needs, and the shelf holds exactly those
    // that need something.
    let rows = db.browse(Shelf::Everything).unwrap();
    let needs = |copy| rows.iter().find(|r| r.copy == copy).unwrap().needs();
    assert_eq!(needs(no_photo), [Need::FrontPhoto]);
    assert_eq!(needs(no_back), [Need::BackPhoto]);
    assert_eq!(needs(no_grade), [Need::Grade]);
    assert_eq!(needs(no_box), [Need::StorageBox]);
    assert_eq!(needs(no_value), [Need::Value]);
    assert_eq!(
        needs(bare),
        [
            Need::FrontPhoto,
            Need::BackPhoto,
            Need::Grade,
            Need::StorageBox,
            Need::Value
        ]
    );
    assert!(needs(done).is_empty() && needs(book).is_empty());
    let needing: BTreeSet<CopyId> = rows
        .iter()
        .filter(|r| !r.needs().is_empty())
        .map(|r| r.copy)
        .collect();
    assert_eq!(needing, shelf(&db, Shelf::NeedsAttention));
    let attention = shelf(&db, Shelf::NeedsAttention);
    assert!(!attention.contains(&done) && !attention.contains(&book));
}

/// Every shelf, name list, and report at 10,000 copies, each under the
/// 100 ms search target. Run with `cargo test --release -- --ignored`.
#[test]
#[ignore = "slow in debug builds; run in release"]
fn browsing_ten_thousand_copies_is_quick() {
    use crate::ShelfList;
    use crate::fixtures::{FixtureSpec, generate};
    let db = db();
    generate(
        &db,
        FixtureSpec {
            copies: 10_000,
            seed: 42,
        },
    )
    .unwrap();
    let timed = |what: &str, run: &dyn Fn() -> usize| {
        let start = std::time::Instant::now();
        let found = run();
        let took = start.elapsed();
        assert!(took.as_millis() < 100, "{what} took {took:?} for {found}");
    };
    let lists = [
        ShelfList::Series,
        ShelfList::Publishers,
        ShelfList::Creators,
        ShelfList::Characters,
        ShelfList::Teams,
        ShelfList::StoryArcs,
        ShelfList::StorageBoxes,
    ];
    let mut shelves = vec![
        Shelf::Everything,
        Shelf::Category(Category::Comic),
        Shelf::KeyIssues,
        Shelf::Graded,
        Shelf::Signed,
        Shelf::Duplicates,
        Shelf::NeedsAttention,
    ];
    for list in lists {
        timed(&format!("{list:?}"), &|| db.shelf_list(list).unwrap().len());
        // The biggest entry's shelf.
        let biggest = db
            .shelf_list(list)
            .unwrap()
            .into_iter()
            .max_by_key(|e| e.copies)
            .unwrap();
        shelves.push(biggest.shelf);
    }
    for shelf in shelves {
        timed(&format!("{shelf:?}"), &|| db.browse(shelf).unwrap().len());
    }
    for collected in [false, true] {
        timed(&format!("gaps {collected}"), &|| {
            db.gaps(collected).unwrap().len()
        });
    }
}
