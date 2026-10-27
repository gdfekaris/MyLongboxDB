//! Tests of folding one series into another.

use crate::model::*;
use crate::{Amount, Currency, Database, Error, IssueNumber, SeriesMerge};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn series(db: &Database, name: &str, category: Category) -> SeriesId {
    db.create_series(&SeriesFields::new(name, category))
        .unwrap()
}

fn issue(db: &Database, series: SeriesId, n: &str) -> IssueId {
    db.create_issue(
        series,
        &IssueFields::new(Some(IssueNumber::new(n).unwrap())),
    )
    .unwrap()
}

fn own(db: &Database, issue: IssueId, variant: &str) -> CopyId {
    let fields = EditionFields {
        variant: variant.into(),
        ..EditionFields::default()
    };
    let edition = db.create_edition(issue, &fields).unwrap();
    db.create_copy(edition, &CopyFields::default()).unwrap()
}

fn numbers(db: &Database, series: SeriesId) -> Vec<String> {
    db.issues_in_series(series)
        .unwrap()
        .into_iter()
        .map(|i| i.fields.number.unwrap().as_str().to_owned())
        .collect()
}

fn copies_of(db: &Database, issue: IssueId) -> usize {
    db.editions_of_issue(issue)
        .unwrap()
        .iter()
        .map(|e| db.copies_of_edition(e.id).unwrap().len())
        .sum()
}

#[test]
fn issues_move_and_matching_numbers_combine() {
    let db = db();
    let real = series(&db, "Amazing Spider-Man", Category::Comic);
    let typo = series(&db, "Amazing Spiderman", Category::Comic);
    let real_120 = issue(&db, real, "120");
    let real_121 = issue(&db, real, "121");
    let typo_121 = issue(&db, typo, "121");
    let typo_122 = issue(&db, typo, "122");
    let first = own(&db, real_120, "");
    let box_1 = own(&db, real_121, "");
    let box_3 = own(&db, typo_121, "");
    own(&db, typo_122, "");

    let summary = db.merge_series(typo, real).unwrap();
    assert_eq!(
        summary,
        SeriesMerge {
            moved: 1,
            combined: 1
        }
    );
    assert_eq!(numbers(&db, real), ["120", "121", "122"]);
    assert!(matches!(db.series(typo), Err(Error::NotFound)));
    assert!(matches!(db.issue(typo_121), Err(Error::NotFound)));
    // Both physical copies of #121 are still separate entries under one issue.
    assert_eq!(copies_of(&db, real_121), 2);
    for copy in [first, box_1, box_3] {
        assert_eq!(db.copy(copy).unwrap().trash, None);
    }
    assert_eq!(db.issue(typo_122).unwrap().series, real);
}

#[test]
fn combined_issues_keep_every_link_and_fill_blanks() {
    let db = db();
    let real = series(&db, "Amazing Spider-Man", Category::Comic);
    let typo = series(&db, "Amazing Spiderman", Category::Comic);
    let mut kept = IssueFields::new(Some(IssueNumber::new("121").unwrap()));
    kept.title = "The Night Gwen Stacy Died".into();
    let keep = db.create_issue(real, &kept).unwrap();
    let mut extra = IssueFields::new(Some(IssueNumber::new("121").unwrap()));
    extra.title = "ignored: the survivor has a title".into();
    extra.cover_date = CoverDate::new(1973, Some(6));
    extra.is_key = true;
    extra.key_reason = "Death of Gwen Stacy".into();
    let fold = db.create_issue(typo, &extra).unwrap();

    let gil = db.create_lookup::<CreatorId>("Gil Kane").unwrap();
    let gerry = db.create_lookup::<CreatorId>("Gerry Conway").unwrap();
    db.set_credits(keep, &[(gerry, Role::Writer)]).unwrap();
    db.set_credits(fold, &[(gerry, Role::Writer), (gil, Role::Penciller)])
        .unwrap();
    let gwen = db.create_lookup::<CharacterId>("Gwen Stacy").unwrap();
    db.set_characters(keep, &[(gwen, false)]).unwrap();
    db.set_characters(fold, &[(gwen, true)]).unwrap();

    db.merge_series(typo, real).unwrap();

    let merged = db.issue(keep).unwrap().fields;
    assert_eq!(merged.title, "The Night Gwen Stacy Died");
    assert_eq!(merged.cover_date, CoverDate::new(1973, Some(6)));
    assert!(merged.is_key);
    assert_eq!(merged.key_reason, "Death of Gwen Stacy");
    let credits: Vec<_> = db
        .credits(keep)
        .unwrap()
        .into_iter()
        .map(|c| c.role)
        .collect();
    assert_eq!(credits, [Role::Writer, Role::Penciller]);
    let characters = db.characters(keep).unwrap();
    assert_eq!(characters.len(), 1);
    assert!(characters[0].first_appearance);
}

#[test]
fn reprints_follow_the_combined_issue() {
    let db = db();
    let real = series(&db, "Amazing Spider-Man", Category::Comic);
    let typo = series(&db, "Amazing Spiderman", Category::Comic);
    let keep = issue(&db, real, "1");
    let fold = issue(&db, typo, "1");
    let mut line = SeriesFields::new("Epic Collection", Category::CollectedEdition);
    line.start_year = Some(2014);
    let line = db.create_series(&line).unwrap();
    let both = issue(&db, line, "1");
    let one = issue(&db, line, "2");
    let link = |collection: IssueId, collected: IssueId, position: i64| {
        db.conn
            .execute(
                "INSERT INTO collects (collection_issue_id, collected_issue_id, position)
                 VALUES (?1, ?2, ?3)",
                [collection.0, collected.0, position],
            )
            .unwrap();
    };
    link(both, keep, 1);
    link(both, fold, 2);
    link(one, fold, 1);

    db.merge_series(typo, real).unwrap();

    let ids = |collection| -> Vec<IssueId> {
        db.collects(collection)
            .unwrap()
            .into_iter()
            .map(|c| c.issue)
            .collect()
    };
    assert_eq!(ids(both), [keep]);
    assert_eq!(ids(one), [keep]);
}

#[test]
fn collected_lines_combine_their_contents_in_order() {
    let db = db();
    let a = series(&db, "Omnibus", Category::CollectedEdition);
    let b = series(&db, "Omnibus (typo)", Category::CollectedEdition);
    let keep = issue(&db, a, "1");
    let fold = issue(&db, b, "1");
    db.set_collects(keep, "Fantastic Four (1961) 1-2").unwrap();
    db.set_collects(fold, "Fantastic Four (1961) 2-4").unwrap();
    db.merge_series(b, a).unwrap();
    assert_eq!(db.collects_text(keep).unwrap(), "Fantastic Four (1961) 1–4");
}

#[test]
fn trashed_issues_move_and_can_still_be_restored() {
    let db = db();
    let real = series(&db, "Amazing Spider-Man", Category::Comic);
    let typo = series(&db, "Amazing Spiderman", Category::Comic);
    issue(&db, real, "1");
    let clash = issue(&db, typo, "1");
    let lone = issue(&db, typo, "2");
    let clash_group = db.trash_issue(clash).unwrap();
    let lone_group = db.trash_issue(lone).unwrap();

    let summary = db.merge_series(typo, real).unwrap();
    assert_eq!(summary, SeriesMerge::default());
    assert_eq!(db.issue(clash).unwrap().series, real);
    db.restore(lone_group).unwrap();
    assert_eq!(numbers(&db, real), ["1", "2"]);
    // #1 already exists live, so the trashed #1 cannot come back as-is.
    assert!(matches!(
        db.restore(clash_group),
        Err(Error::DuplicateIssueNumber { .. })
    ));
}

#[test]
fn the_survivor_keeps_its_details_and_fills_blanks() {
    let db = db();
    let marvel = db.create_lookup::<PublisherId>("Marvel").unwrap();
    let mut real = SeriesFields::new("Amazing Spider-Man", Category::Comic);
    real.notes = "Keep these notes".into();
    let real = db.create_series(&real).unwrap();
    let mut typo = SeriesFields::new("Amazing Spiderman", Category::Comic);
    typo.start_year = Some(1963);
    typo.publisher = Some(marvel);
    typo.notes = "Not these".into();
    let typo = db.create_series(&typo).unwrap();
    db.merge_series(typo, real).unwrap();
    let merged = db.series(real).unwrap().fields;
    assert_eq!(merged.name, "Amazing Spider-Man");
    assert_eq!(merged.start_year, Some(1963));
    assert_eq!(merged.publisher, Some(marvel));
    assert_eq!(merged.notes, "Keep these notes");
}

#[test]
fn only_live_series_of_the_same_kind_merge() {
    let db = db();
    let comic = series(&db, "Conan the Barbarian", Category::Comic);
    let magazine = series(&db, "Savage Sword of Conan", Category::Magazine);
    let other = series(&db, "Conan", Category::Comic);
    assert!(matches!(
        db.merge_series(magazine, comic),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.merge_series(comic, comic),
        Err(Error::Invalid(_))
    ));
    db.trash_series(other).unwrap();
    assert!(matches!(db.merge_series(other, comic), Err(Error::InTrash)));
    assert!(matches!(db.merge_series(comic, other), Err(Error::InTrash)));
}

#[test]
fn a_blank_title_is_filled_from_the_folded_issue() {
    let db = db();
    let real = series(&db, "Amazing Spider-Man", Category::Comic);
    let typo = series(&db, "Amazing Spiderman", Category::Comic);
    let keep = issue(&db, real, "121");
    let mut titled = IssueFields::new(Some(IssueNumber::new("121").unwrap()));
    titled.title = "The Night Gwen Stacy Died".into();
    db.create_issue(typo, &titled).unwrap();
    db.merge_series(typo, real).unwrap();
    assert_eq!(
        db.issue(keep).unwrap().fields.title,
        "The Night Gwen Stacy Died"
    );
}

#[test]
fn those_in_every_issue_join_the_series_merged_into() {
    let db = db();
    let from = series(&db, "Amazing Spiderman", Category::Comic);
    let into = series(&db, "Amazing Spider-Man", Category::Comic);
    let spidey = db.create_lookup::<CharacterId>("Spider-Man").unwrap();
    let mj = db.create_lookup::<CharacterId>("Mary Jane").unwrap();
    let team = db.create_lookup::<TeamId>("Avengers").unwrap();
    db.set_series_characters(from, &[spidey, mj]).unwrap();
    db.set_series_teams(from, &[team]).unwrap();
    db.set_series_characters(into, &[spidey]).unwrap();
    db.merge_series(from, into).unwrap();
    let names: Vec<String> = db
        .series_characters(into)
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert_eq!(names, ["Mary Jane", "Spider-Man"]);
    assert_eq!(db.series_teams(into).unwrap()[0].id, team);
}

#[test]
fn matching_editions_of_combined_issues_become_one() {
    let db = db();
    let real = series(&db, "Thor", Category::Comic);
    let typo = series(&db, "Thor ", Category::Comic);
    let real_337 = issue(&db, real, "337");
    let typo_337 = issue(&db, typo, "337");
    let real_standard = db
        .create_edition(real_337, &EditionFields::default())
        .unwrap();
    let mine = db
        .create_copy(real_standard, &CopyFields::default())
        .unwrap();
    // The same edition in the other series, with what the first lacks,
    // and a cover artist.
    let typo_standard = db
        .create_edition(
            typo_337,
            &EditionFields {
                variant: "".into(),
                barcode: Some("75960601772003711".into()),
                cover_price: Some(Amount::new(Currency::Usd, 60)),
                ..EditionFields::default()
            },
        )
        .unwrap();
    let theirs = db
        .create_copy(typo_standard, &CopyFields::default())
        .unwrap();
    let simonson = db.create_lookup::<CreatorId>("Walt Simonson").unwrap();
    db.set_cover_artists(typo_standard, &[simonson]).unwrap();
    // Different variants, printings, and distributions stay apart.
    own(&db, typo_337, "Cover B");
    let second = db
        .create_edition(
            typo_337,
            &EditionFields {
                printing: Some(2),
                ..EditionFields::default()
            },
        )
        .unwrap();
    let newsstand = db
        .create_edition(
            typo_337,
            &EditionFields {
                distribution: Some(Distribution::Newsstand),
                ..EditionFields::default()
            },
        )
        .unwrap();

    db.merge_series(typo, real).unwrap();
    let editions = db.editions_of_issue(real_337).unwrap();
    assert_eq!(editions.len(), 4);
    assert!(matches!(db.edition(typo_standard), Err(Error::NotFound)));
    let kept = db.edition(real_standard).unwrap();
    assert_eq!(kept.fields.barcode.as_deref(), Some("75960601772003711"));
    assert_eq!(
        kept.fields.cover_price,
        Some(Amount::new(Currency::Usd, 60))
    );
    let copies: Vec<CopyId> = db
        .copies_of_edition(real_standard)
        .unwrap()
        .iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(copies, [mine, theirs]);
    let artists = db.cover_artists(real_standard).unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].id, simonson);
    for apart in [second, newsstand] {
        assert_eq!(db.edition(apart).unwrap().issue, real_337);
    }
}

#[test]
fn editions_whose_details_differ_stay_apart() {
    let db = db();
    let real = series(&db, "Hulk", Category::Comic);
    let typo = series(&db, "Hulk ", Category::Comic);
    let price = |cents| EditionFields {
        cover_price: Some(Amount::new(Currency::Usd, cents)),
        ..EditionFields::default()
    };
    let real_1 = issue(&db, real, "1");
    let cents_12 = db.create_edition(real_1, &price(12)).unwrap();
    // A price variant entered without a variant name.
    let cents_15 = db
        .create_edition(issue(&db, typo, "1"), &price(15))
        .unwrap();
    db.merge_series(typo, real).unwrap();
    let editions: Vec<EditionId> = db
        .editions_of_issue(real_1)
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(editions, [cents_12, cents_15]);
    assert_eq!(
        db.edition(cents_15).unwrap().fields.cover_price,
        Some(Amount::new(Currency::Usd, 15))
    );

    // So do a hardcover and a softcover of the same book.
    let real = series(&db, "Watchmen", Category::CollectedEdition);
    let typo = series(&db, "Watchmen ", Category::CollectedEdition);
    let bound = |binding| EditionFields {
        binding: Some(binding),
        ..EditionFields::default()
    };
    let real_1 = issue(&db, real, "1");
    db.create_edition(real_1, &bound(Binding::Hardcover))
        .unwrap();
    db.create_edition(issue(&db, typo, "1"), &bound(Binding::Softcover))
        .unwrap();
    db.merge_series(typo, real).unwrap();
    assert_eq!(db.editions_of_issue(real_1).unwrap().len(), 2);
}

#[test]
fn an_edition_in_trash_moves_without_combining() {
    let db = db();
    let real = series(&db, "Hulk", Category::Comic);
    let typo = series(&db, "Hulk ", Category::Comic);
    let real_1 = issue(&db, real, "1");
    own(&db, real_1, "");
    let typo_1 = issue(&db, typo, "1");
    let binned = db
        .create_edition(typo_1, &EditionFields::default())
        .unwrap();
    let trash = db.trash_edition(binned).unwrap();
    db.merge_series(typo, real).unwrap();
    assert_eq!(db.edition(binned).unwrap().issue, real_1);
    db.restore(trash).unwrap();
    assert_eq!(db.editions_of_issue(real_1).unwrap().len(), 2);
}
