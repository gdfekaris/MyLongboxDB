//! Tests of reading an issue's detail page.

use crate::model::*;
use crate::{Amount, Currency, Database, Date, Error, IssueNumber};

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

fn value(db: &Database, copy: CopyId, n: u8, date: &str, cents: i64, source: &str) {
    db.conn
        .execute(
            "INSERT INTO value_entry (uuid, copy_id, date, value_units, value_currency, source)
             VALUES (?1, ?2, ?3, ?4, 'USD', ?5)",
            rusqlite::params![
                format!("00000000-0000-4000-8000-0000000000{n:02}"),
                copy,
                date,
                cents,
                source
            ],
        )
        .unwrap();
}

#[test]
fn detail_gathers_the_whole_issue() {
    let db = db();
    let publisher = db.create_lookup::<PublisherId>("Marvel").unwrap();
    let mut f = SeriesFields::new("Amazing Spider-Man", Category::Comic);
    f.start_year = Some(1963);
    f.publisher = Some(publisher);
    let asm = db.create_series(&f).unwrap();
    let id = issue(&db, asm, "121");

    let kane = db.create_lookup::<CreatorId>("Gil Kane").unwrap();
    let conway = db.create_lookup::<CreatorId>("Gerry Conway").unwrap();
    db.set_credits(id, &[(kane, Role::Penciller), (conway, Role::Writer)])
        .unwrap();
    let gwen = db.create_lookup::<CharacterId>("Gwen Stacy").unwrap();
    db.set_characters(id, &[(gwen, false)]).unwrap();

    let standard = db.create_edition(id, &EditionFields::default()).unwrap();
    let romita = db.create_lookup::<CreatorId>("John Romita").unwrap();
    db.set_cover_artists(standard, &[romita]).unwrap();
    let longbox = db.create_lookup::<BoxId>("Box 1").unwrap();
    let boxed = db
        .create_copy(
            standard,
            &CopyFields {
                storage_box: Some(longbox),
                box_position: Some(4),
                ..CopyFields::default()
            },
        )
        .unwrap();
    let key = db.create_lookup::<TagId>("Key").unwrap();
    db.set_tags(boxed, &[key]).unwrap();
    value(&db, boxed, 1, "2023-01-01", 50_000, "eBay sold");
    value(&db, boxed, 2, "2024-01-01", 65_000, "GoCollect");
    value(&db, boxed, 3, "2022-01-01", 90_000, "old guess");
    let trashed_copy = db.create_copy(standard, &CopyFields::default()).unwrap();
    db.trash_copy(trashed_copy).unwrap();

    let newsstand = db
        .create_edition(
            id,
            &EditionFields {
                distribution: Some(Distribution::Newsstand),
                ..EditionFields::default()
            },
        )
        .unwrap();
    let gone = db
        .create_edition(
            id,
            &EditionFields {
                variant: "Cover B".into(),
                ..EditionFields::default()
            },
        )
        .unwrap();
    db.trash_edition(gone).unwrap();

    let line = series(&db, "Marvel Masterworks", Category::CollectedEdition);
    let book = issue(&db, line, "13");
    db.set_collects(book, "Amazing Spider-Man (1963) 121")
        .unwrap();
    let book_edition = db.create_edition(book, &EditionFields::default()).unwrap();
    db.create_copy(book_edition, &CopyFields::default())
        .unwrap();

    let d = db.issue_detail(id).unwrap();
    assert_eq!(d.label, "Amazing Spider-Man (1963) #121");
    assert_eq!(d.publisher.as_deref(), Some("Marvel"));
    let credits: Vec<_> = d.credits.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(credits, ["Gerry Conway", "Gil Kane"]);
    assert_eq!(d.characters[0].name, "Gwen Stacy");

    let editions: Vec<_> = d.editions.iter().map(|e| e.edition.id).collect();
    assert_eq!(editions, [standard, newsstand]);
    assert_eq!(d.editions[0].cover_artists[0].name, "John Romita");
    assert!(d.editions[1].copies.is_empty());

    let copies = &d.editions[0].copies;
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].copy.id, boxed);
    assert_eq!(copies[0].storage_box.as_deref(), Some("Box 1"));
    assert_eq!(copies[0].tags[0].name, "Key");
    let newest = copies[0].value.as_ref().unwrap();
    assert_eq!(newest.date, Date::new(2024, 1, 1).unwrap());
    assert_eq!(newest.value, Amount::new(Currency::Usd, 65_000));
    assert_eq!(newest.source, "GoCollect");

    assert_eq!(d.collected_in.len(), 1);
    assert_eq!(d.collected_in[0].collection, book);
    assert_eq!(d.ownership.single_copies, 1);
    assert_eq!(d.ownership.collections_owned, 1);
    assert!(d.collects.is_empty());

    let book_detail = db.issue_detail(book).unwrap();
    assert_eq!(book_detail.collects.len(), 1);
    assert_eq!(book_detail.collects[0].issue, id);
}

#[test]
fn trashed_issues_have_no_detail_page() {
    let db = db();
    let s = series(&db, "X-Men", Category::Comic);
    let gone = issue(&db, s, "1");
    db.trash_issue(gone).unwrap();
    assert!(matches!(db.issue_detail(gone), Err(Error::InTrash)));

    let kept = issue(&db, s, "2");
    db.trash_series(s).unwrap();
    assert!(matches!(db.issue_detail(kept), Err(Error::InTrash)));
}
