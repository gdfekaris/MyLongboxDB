//! Tests of browsing shelves and the name lists that lead to them.

use crate::fixtures::{FixtureSpec, generate};
use crate::model::*;
use crate::{Amount, Currency, Database, IssueNumber, Shelf, ShelfList};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn series(db: &Database, name: &str, category: Category) -> SeriesId {
    let mut f = SeriesFields::new(name, category);
    f.start_year = Some(1963);
    db.create_series(&f).unwrap()
}

fn issue(db: &Database, series: SeriesId, n: &str) -> IssueId {
    db.create_issue(
        series,
        &IssueFields::new(Some(IssueNumber::new(n).unwrap())),
    )
    .unwrap()
}

fn edition(db: &Database, issue: IssueId, variant: &str) -> EditionId {
    let fields = EditionFields {
        variant: variant.into(),
        ..EditionFields::default()
    };
    db.create_edition(issue, &fields).unwrap()
}

fn own(db: &Database, issue: IssueId) -> CopyId {
    let edition = edition(db, issue, "");
    db.create_copy(edition, &CopyFields::default()).unwrap()
}

fn copies(db: &Database, shelf: Shelf) -> Vec<CopyId> {
    db.browse(shelf).unwrap().iter().map(|r| r.copy).collect()
}

fn labels(db: &Database, shelf: Shelf) -> Vec<String> {
    db.browse(shelf)
        .unwrap()
        .iter()
        .map(|r| match r.variant.as_str() {
            "" => r.label(),
            variant => format!("{} {variant}", r.label()),
        })
        .collect()
}

#[test]
fn books_come_in_natural_order() {
    let db = db();
    let x = series(&db, "X-Men", Category::Comic);
    let asm = series(&db, "amazing Spider-Man", Category::Comic);
    for n in ["10", "Annual 1", "2", "1"] {
        own(&db, issue(&db, x, n));
    }
    let asm1 = issue(&db, asm, "1");
    let cover_b = edition(&db, asm1, "Cover B");
    db.create_copy(cover_b, &CopyFields::default()).unwrap();
    own(&db, asm1);

    assert_eq!(
        labels(&db, Shelf::Everything),
        [
            "amazing Spider-Man (1963) #1",
            "amazing Spider-Man (1963) #1 Cover B",
            "X-Men (1963) #1",
            "X-Men (1963) #2",
            "X-Men (1963) #10",
            "X-Men (1963) #Annual 1",
        ]
    );
}

#[test]
fn copies_of_an_issue_come_in_edition_then_copy_order() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let i = issue(&db, s, "1");
    let make = |variant: &str, printing: Option<u32>, distribution: Option<Distribution>| {
        let fields = EditionFields {
            variant: variant.into(),
            printing,
            distribution,
            ..EditionFields::default()
        };
        db.create_edition(i, &fields).unwrap()
    };
    let stan = db.create_lookup::<CreatorId>("Stan Lee").unwrap();
    let copy = |edition: EditionId, grade_type: Option<GradeType>, signed: bool| {
        let fields = CopyFields {
            grade_type,
            ..CopyFields::default()
        };
        let copy = db.create_copy(edition, &fields).unwrap();
        if signed {
            let signature = SignatureFields::new(Verification::Witnessed);
            db.set_signatures(copy, &[(stan, signature)]).unwrap();
        }
        copy
    };
    // Each made before the ones it should follow.
    let cover_b = make("cover b", None, None);
    let cover_a_newsstand = make("Cover A", None, Some(Distribution::Newsstand));
    let cover_a = make("Cover A", Some(1), Some(Distribution::Direct));
    let second = make("", Some(2), Some(Distribution::Direct));
    let unrecorded = make("", Some(1), None);
    let newsstand = make("", None, Some(Distribution::Newsstand));
    let direct = make("", Some(1), Some(Distribution::Direct));
    let cover_b_copy = copy(cover_b, None, false);
    let cover_a_newsstand_copy = copy(cover_a_newsstand, None, false);
    let cover_a_copy = copy(cover_a, None, false);
    let second_copy = copy(second, None, false);
    let unrecorded_copy = copy(unrecorded, Some(GradeType::Pgx), false);
    let newsstand_copy = copy(newsstand, None, false);
    let graded_signed = copy(direct, Some(GradeType::Cgc), true);
    let graded = copy(direct, Some(GradeType::Cbcs), false);
    let raw_signed = copy(direct, Some(GradeType::Raw), true);
    let no_grade_type = copy(direct, None, false);
    let order = [
        no_grade_type,
        raw_signed,
        graded,
        graded_signed,
        newsstand_copy,
        unrecorded_copy,
        second_copy,
        cover_a_copy,
        cover_a_newsstand_copy,
        cover_b_copy,
    ];
    assert_eq!(copies(&db, Shelf::Everything), order);

    // The book page lists them the same way.
    let detail = db.issue_detail(i).unwrap();
    let on_page: Vec<CopyId> = detail
        .editions
        .iter()
        .flat_map(|e| e.copies.iter().map(|c| c.copy.id))
        .collect();
    assert_eq!(on_page, order);
}

#[test]
fn every_copy_of_an_edition_is_its_own_row() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let e = edition(&db, issue(&db, s, "1"), "");
    let first = db.create_copy(e, &CopyFields::default()).unwrap();
    let second = db.create_copy(e, &CopyFields::default()).unwrap();
    assert_eq!(copies(&db, Shelf::Everything), [first, second]);
}

#[test]
fn trash_hides_books_at_every_level() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let kept = own(&db, issue(&db, s, "1"));
    let gone_copy = own(&db, issue(&db, s, "2"));
    let gone_issue = issue(&db, s, "3");
    own(&db, gone_issue);
    let gone_edition = edition(&db, issue(&db, s, "4"), "");
    db.create_copy(gone_edition, &CopyFields::default())
        .unwrap();
    let other = series(&db, "Avengers", Category::Comic);
    own(&db, issue(&db, other, "1"));

    db.trash_copy(gone_copy).unwrap();
    db.trash_issue(gone_issue).unwrap();
    db.trash_edition(gone_edition).unwrap();
    db.trash_series(other).unwrap();

    assert_eq!(copies(&db, Shelf::Everything), [kept]);
    let names: Vec<_> = db
        .shelf_list(ShelfList::Series)
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.copies))
        .collect();
    assert_eq!(names, [("X-Men (1963)".to_owned(), 1)]);
}

#[test]
fn category_shelves_hold_only_their_category() {
    let db = db();
    let comic = own(&db, issue(&db, series(&db, "X-Men", Category::Comic), "1"));
    let magazine = own(
        &db,
        issue(&db, series(&db, "Heavy Metal", Category::Magazine), "1"),
    );
    let book = own(
        &db,
        issue(
            &db,
            series(&db, "Epic Collection", Category::CollectedEdition),
            "1",
        ),
    );
    assert_eq!(copies(&db, Shelf::Category(Category::Comic)), [comic]);
    assert_eq!(copies(&db, Shelf::Category(Category::Magazine)), [magazine]);
    assert_eq!(
        copies(&db, Shelf::Category(Category::CollectedEdition)),
        [book]
    );
}

#[test]
fn key_issue_shelf_holds_key_issues() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let mut f = IssueFields::new(Some(IssueNumber::new("1").unwrap()));
    f.is_key = true;
    let key = own(&db, db.create_issue(s, &f).unwrap());
    own(&db, issue(&db, s, "2"));
    assert_eq!(copies(&db, Shelf::KeyIssues), [key]);
}

#[test]
fn graded_shelf_holds_books_graded_by_a_grading_company() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let graded: Vec<CopyId> = [GradeType::Cgc, GradeType::Cbcs, GradeType::Pgx]
        .into_iter()
        .zip(["1", "2", "3"])
        .map(|(grade_type, n)| {
            let copy = own(&db, issue(&db, s, n));
            let mut f = db.copy(copy).unwrap().fields;
            f.grade = Some(Grade::from_tenths(94).unwrap());
            f.grade_type = Some(grade_type);
            db.update_copy(copy, &f).unwrap();
            copy
        })
        .collect();
    let raw = own(&db, issue(&db, s, "4"));
    let mut f = db.copy(raw).unwrap().fields;
    f.grade = Some(Grade::from_tenths(94).unwrap());
    f.grade_type = Some(GradeType::Raw);
    db.update_copy(raw, &f).unwrap();
    own(&db, issue(&db, s, "5"));
    assert_eq!(copies(&db, Shelf::Graded), graded);
    db.trash_copy(graded[0]).unwrap();
    assert_eq!(copies(&db, Shelf::Graded), graded[1..]);
}

#[test]
fn signed_shelf_holds_books_with_a_signature() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let stan = db.create_lookup::<CreatorId>("Stan Lee").unwrap();
    let once = own(&db, issue(&db, s, "1"));
    let twice = own(&db, issue(&db, s, "2"));
    own(&db, issue(&db, s, "3"));
    let witnessed = SignatureFields::new(Verification::Witnessed);
    db.set_signatures(once, &[(stan, witnessed.clone())])
        .unwrap();
    db.set_signatures(twice, &[(stan, witnessed.clone()), (stan, witnessed)])
        .unwrap();
    assert_eq!(copies(&db, Shelf::Signed), [once, twice]);
}

#[test]
fn creator_shelves_include_cover_art_and_reprints() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    let ditko = db.create_lookup::<CreatorId>("Steve Ditko").unwrap();
    let ff = series(&db, "Fantastic Four", Category::Comic);

    let credited = issue(&db, ff, "1");
    db.set_credits(credited, &[(kirby, Role::Penciller)])
        .unwrap();
    let credited_copy = own(&db, credited);

    let other = issue(&db, ff, "2");
    let covered = edition(&db, other, "Cover B");
    db.set_cover_artists(covered, &[kirby]).unwrap();
    let covered_copy = db.create_copy(covered, &CopyFields::default()).unwrap();
    // The standard cover of the same issue is someone else's.
    own(&db, other);

    let line = series(&db, "Epic Collection", Category::CollectedEdition);
    let book = issue(&db, line, "1");
    db.set_collects(book, "Fantastic Four (1963) 1").unwrap();
    let book_copy = own(&db, book);

    assert_eq!(
        copies(&db, Shelf::Creator(kirby)),
        [book_copy, credited_copy, covered_copy]
    );
    assert_eq!(copies(&db, Shelf::Creator(ditko)), []);
}

#[test]
fn character_team_and_arc_shelves_include_reprints() {
    let db = db();
    let logan = db.create_lookup::<CharacterId>("Wolverine").unwrap();
    let team = db.create_lookup::<TeamId>("X-Men").unwrap();
    let arc = db.create_lookup::<StoryArcId>("Weapon X").unwrap();
    let s = series(&db, "Marvel Comics Presents", Category::Comic);
    let single = issue(&db, s, "72");
    db.set_characters(single, &[(logan, false)]).unwrap();
    db.set_teams(single, &[(team, false)]).unwrap();
    db.set_story_arcs(single, &[(arc, Some(1))]).unwrap();
    // Owned only in the reprint.
    let line = series(&db, "Weapon X", Category::CollectedEdition);
    let book = issue(&db, line, "1");
    db.set_collects(book, "Marvel Comics Presents (1963) 72")
        .unwrap();
    let book_copy = own(&db, book);

    assert_eq!(copies(&db, Shelf::Character(logan)), [book_copy]);
    assert_eq!(copies(&db, Shelf::Team(team)), [book_copy]);
    assert_eq!(copies(&db, Shelf::StoryArc(arc)), [book_copy]);
}

#[test]
fn boxes_list_books_by_position() {
    let db = db();
    let shelf_box = db.create_lookup::<BoxId>("Box 1").unwrap();
    let s = series(&db, "X-Men", Category::Comic);
    let mut placed = Vec::new();
    for (n, position) in [("1", None), ("2", Some(2)), ("3", Some(1))] {
        let e = edition(&db, issue(&db, s, n), "");
        let fields = CopyFields {
            storage_box: Some(shelf_box),
            box_position: position,
            ..CopyFields::default()
        };
        placed.push(db.create_copy(e, &fields).unwrap());
    }
    own(&db, issue(&db, s, "4"));
    let trashed = db
        .create_copy(
            edition(&db, issue(&db, s, "5"), ""),
            &CopyFields {
                storage_box: Some(shelf_box),
                ..CopyFields::default()
            },
        )
        .unwrap();
    db.trash_copy(trashed).unwrap();

    assert_eq!(
        copies(&db, Shelf::StorageBox(shelf_box)),
        [placed[2], placed[1], placed[0]]
    );
    assert_eq!(db.shelf_list(ShelfList::StorageBoxes).unwrap()[0].copies, 3);
    let row = &db.browse(Shelf::StorageBox(shelf_box)).unwrap()[0];
    assert_eq!(
        (row.storage_box.as_deref(), row.box_position),
        (Some("Box 1"), Some(1))
    );
}

#[test]
fn rows_show_the_newest_value() {
    let db = db();
    let copy = own(&db, issue(&db, series(&db, "X-Men", Category::Comic), "1"));
    for (uuid, date, units, currency) in [
        (
            "00000000-0000-4000-8000-000000000001",
            "2020-01-01",
            500,
            "USD",
        ),
        (
            "00000000-0000-4000-8000-000000000002",
            "2024-06-01",
            12,
            "XAU",
        ),
        (
            "00000000-0000-4000-8000-000000000003",
            "2022-01-01",
            900,
            "USD",
        ),
    ] {
        db.conn
            .execute(
                "INSERT INTO value_entry (uuid, copy_id, date, value_units, value_currency)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![uuid, copy, date, units, currency],
            )
            .unwrap();
    }
    let rows = db.browse(Shelf::Everything).unwrap();
    assert_eq!(rows[0].value, Some(Amount::new(Currency::Gold, 12)));
}

#[test]
fn series_list_names_publishers_and_unowned_series() {
    let db = db();
    let publisher = db.create_lookup::<PublisherId>("Lantern Press").unwrap();
    let mut f = SeriesFields::new("Fantastic Four", Category::Comic);
    f.publisher = Some(publisher);
    let ff = db.create_series(&f).unwrap();
    issue(&db, ff, "1");

    let list = db.shelf_list(ShelfList::Series).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].shelf, Shelf::Series(ff));
    assert_eq!(
        (
            list[0].name.as_str(),
            list[0].detail.as_str(),
            list[0].copies
        ),
        ("Fantastic Four", "Lantern Press", 0)
    );
}

#[test]
fn list_counts_match_their_shelves() {
    let db = db();
    let summary = generate(
        &db,
        FixtureSpec {
            copies: 400,
            seed: 7,
        },
    )
    .unwrap();
    assert_eq!(db.browse(Shelf::Everything).unwrap().len(), summary.copies);

    let lists = [
        ShelfList::Series,
        ShelfList::Publishers,
        ShelfList::Creators,
        ShelfList::Characters,
        ShelfList::Teams,
        ShelfList::StoryArcs,
        ShelfList::StorageBoxes,
    ];
    for list in lists {
        let entries = db.shelf_list(list).unwrap();
        assert!(
            entries.iter().any(|e| e.copies > 0),
            "{list:?} has no books at all"
        );
        for entry in entries {
            let rows = db.browse(entry.shelf).unwrap();
            assert_eq!(rows.len(), entry.copies as usize, "{list:?} {}", entry.name);
        }
    }
}
