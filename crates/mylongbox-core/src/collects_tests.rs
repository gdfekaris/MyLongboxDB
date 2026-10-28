//! Tests of collected editions' contents and where issues are reprinted.

use crate::model::*;
use crate::{Database, Error, IssueNumber, Ownership};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn series(db: &Database, name: &str, year: u16, category: Category) -> SeriesId {
    let mut f = SeriesFields::new(name, category);
    f.start_year = Some(year);
    db.create_series(&f).unwrap()
}

fn issue(db: &Database, series: SeriesId, n: &str) -> IssueId {
    db.create_issue(
        series,
        &IssueFields::new(Some(IssueNumber::new(n).unwrap())),
    )
    .unwrap()
}

fn own(db: &Database, issue: IssueId) -> CopyId {
    let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
    db.create_copy(edition, &CopyFields::default()).unwrap()
}

/// A collected-edition volume to hold contents.
fn volume(db: &Database, line: &str) -> IssueId {
    let line = series(db, line, 2014, Category::CollectedEdition);
    issue(db, line, "1")
}

fn invalid<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::Invalid(why)) => why,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn preview_shows_what_saving_will_create() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let one = issue(&db, asm, "1");
    let preview = db
        .preview_collects("amazing spider-man (1963) 1-2; Strange Tales (1951) 110")
        .unwrap();
    let summary: Vec<_> = preview
        .iter()
        .map(|p| (p.entry.number.as_str().to_owned(), p.series, p.issue))
        .collect();
    assert_eq!(
        summary,
        [
            ("1".to_owned(), Some(asm), Some(one)),
            ("2".to_owned(), Some(asm), None),
            ("110".to_owned(), None, None),
        ]
    );
    // Nothing was written.
    assert_eq!(db.issues_in_series(asm).unwrap().len(), 1);
    assert_eq!(db.list_series().unwrap().len(), 1);
}

#[test]
fn saving_creates_missing_catalog_entries_in_reading_order() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let existing = issue(&db, asm, "2");
    let vol = volume(&db, "Epic Collection: Amazing Spider-Man");
    db.set_collects(
        vol,
        "Amazing Spider-Man (1963) 1–3, Annual 1; Strange Tales (1951) 110",
    )
    .unwrap();

    let contents = db.collects(vol).unwrap();
    let numbers: Vec<_> = contents
        .iter()
        .map(|c| format!("{} #{}", c.entry.series.name, c.entry.number))
        .collect();
    assert_eq!(
        numbers,
        [
            "Amazing Spider-Man #1",
            "Amazing Spider-Man #2",
            "Amazing Spider-Man #3",
            "Amazing Spider-Man #Annual 1",
            "Strange Tales #110",
        ]
    );
    assert_eq!(contents[1].issue, existing);
    // New issues are catalog entries only: nothing owned.
    assert!(db.editions_of_issue(contents[0].issue).unwrap().is_empty());
    let strange = db.series(contents[4].series).unwrap();
    assert_eq!(strange.fields.category, Category::Comic);
    assert_eq!(strange.fields.start_year, Some(1951));
    assert_eq!(
        db.collects_text(vol).unwrap(),
        "Amazing Spider-Man (1963) 1–3, Annual 1; Strange Tales (1951) 110"
    );
}

#[test]
fn saving_again_replaces_and_empty_text_clears() {
    let db = db();
    let vol = volume(&db, "Omnibus");
    db.set_collects(vol, "Watchmen (1986) 1-12").unwrap();
    db.set_collects(vol, "Watchmen (1986) 1, 1, 2").unwrap();
    assert_eq!(db.collects_text(vol).unwrap(), "Watchmen (1986) 1–2");
    db.set_collects(vol, "  ").unwrap();
    assert!(db.collects(vol).unwrap().is_empty());
    // The issues stay in the catalog.
    let watchmen = db.list_series().unwrap();
    assert!(watchmen.iter().any(|s| s.fields.name == "Watchmen"));
}

#[test]
fn a_bad_list_changes_nothing() {
    let db = db();
    let vol = volume(&db, "Omnibus");
    db.set_collects(vol, "Watchmen (1986) 1").unwrap();
    invalid(db.set_collects(vol, "Saga (2012) 1-6; Watchmen (1986) 12-1"));
    assert_eq!(db.collects_text(vol).unwrap(), "Watchmen (1986) 1");
    assert!(
        !db.list_series()
            .unwrap()
            .iter()
            .any(|s| s.fields.name == "Saga")
    );
}

#[test]
fn only_collected_editions_have_contents_and_they_reprint_single_issues() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let single = issue(&db, asm, "1");
    invalid(db.set_collects(single, "Amazing Spider-Man (1963) 2"));
    let vol = volume(&db, "Epic Collection");
    series(&db, "Masterworks", 2003, Category::CollectedEdition);
    let why = invalid(db.set_collects(vol, "Masterworks (2003) 1"));
    assert!(why.contains("collected editions"), "{why}");
}

#[test]
fn a_year_picks_between_series_with_the_same_name() {
    let db = db();
    let old = series(&db, "Captain Marvel", 1968, Category::Comic);
    let new = series(&db, "Captain Marvel", 2019, Category::Comic);
    let vol = volume(&db, "Omnibus");
    let why = invalid(db.set_collects(vol, "Captain Marvel 1"));
    assert!(why.contains("add its start year"), "{why}");
    db.set_collects(vol, "Captain Marvel (1968) 1; Captain Marvel (2019) 1")
        .unwrap();
    let series_ids: Vec<_> = db.collects(vol).unwrap().iter().map(|c| c.series).collect();
    assert_eq!(series_ids, [old, new]);
}

#[test]
fn a_book_line_sharing_a_name_is_not_taken_for_the_series() {
    let db = db();
    let saga = series(&db, "Saga", 2012, Category::Comic);
    // The trade paperbacks are kept as a line of the same name.
    let vol = volume(&db, "Saga");
    db.set_collects(vol, "Saga 1-6").unwrap();
    let found: Vec<_> = db.collects(vol).unwrap().iter().map(|c| c.series).collect();
    assert_eq!(found, [saga; 6]);
    // Two series that issues can come from still need telling apart.
    series(&db, "Saga", 2030, Category::Magazine);
    let why = invalid(db.set_collects(vol, "Saga 1"));
    assert!(why.contains("add its start year"), "{why}");
}

#[test]
fn trashed_issues_are_not_reused() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let trashed = issue(&db, asm, "1");
    db.trash_issue(trashed).unwrap();
    let vol = volume(&db, "Omnibus");
    db.set_collects(vol, "Amazing Spider-Man (1963) 1").unwrap();
    assert_ne!(db.collects(vol).unwrap()[0].issue, trashed);
}

#[test]
fn issues_know_which_books_reprint_them() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let one = issue(&db, asm, "1");
    let epic = volume(&db, "Epic Collection");
    let omnibus = volume(&db, "Omnibus");
    let trashed = volume(&db, "Masterworks");
    for vol in [epic, omnibus, trashed] {
        db.set_collects(vol, "Amazing Spider-Man (1963) 1").unwrap();
    }
    own(&db, omnibus);
    db.trash_issue(trashed).unwrap();

    let found: Vec<_> = db
        .collected_in(one)
        .unwrap()
        .into_iter()
        .map(|c| (c.label, c.owned))
        .collect();
    assert_eq!(
        found,
        [
            ("Omnibus (2014 series) #1".to_owned(), true),
            ("Epic Collection (2014 series) #1".to_owned(), false),
        ]
    );
}

#[test]
fn ownership_counts_single_copies_and_owned_collections() {
    let db = db();
    let asm = series(&db, "Amazing Spider-Man", 1963, Category::Comic);
    let one = issue(&db, asm, "1");
    let two = issue(&db, asm, "2");
    assert_eq!(db.ownership(one).unwrap(), Ownership::default());

    own(&db, one);
    let second = own(&db, one);
    let vol = volume(&db, "Omnibus");
    db.set_collects(vol, "Amazing Spider-Man (1963) 1-2")
        .unwrap();
    assert_eq!(
        db.ownership(two).unwrap(),
        Ownership {
            single_copies: 0,
            collections_owned: 0
        }
    );
    let book = own(&db, vol);
    assert_eq!(
        db.ownership(one).unwrap(),
        Ownership {
            single_copies: 2,
            collections_owned: 1
        }
    );
    assert_eq!(db.ownership(two).unwrap().collections_owned, 1);

    // Copies in Trash are not owned.
    db.trash_copy(second).unwrap();
    db.trash_copy(book).unwrap();
    assert_eq!(
        db.ownership(one).unwrap(),
        Ownership {
            single_copies: 1,
            collections_owned: 0
        }
    );
}

#[test]
fn an_issue_taken_off_the_list_goes_unless_something_else_holds_it() {
    let db = db();
    let vol = volume(&db, "Epic Collection");
    db.set_collects(vol, "Fantastic Four (1961) 1-3").unwrap();
    let ff = db
        .issues_in_series(db.collects(vol).unwrap()[0].series)
        .unwrap();
    // Owned as a single issue.
    own(&db, ff[1].id);
    // Reprinted by another book too.
    let other = volume(&db, "Omnibus");
    db.set_collects(other, "Fantastic Four (1961) 3").unwrap();

    db.set_collects(vol, "Fantastic Four (1961) 1").unwrap();
    let left: Vec<_> = db
        .issues_in_series(ff[0].series)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(left, [ff[0].id, ff[1].id, ff[2].id]);
    db.set_collects(other, "").unwrap();
    db.set_collects(vol, "").unwrap();
    let left: Vec<_> = db
        .issues_in_series(ff[0].series)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(left, [ff[1].id], "only the issue owned is left");
}
