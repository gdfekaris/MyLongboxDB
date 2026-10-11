//! Tests of changing many copies at once.

use crate::model::*;
use crate::{Amount, BulkChange, Currency, Database, Error, IssueNumber};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

/// One copy each of issues 1..=n of a comic series.
fn copies(db: &Database, name: &str, category: Category, n: u32) -> Vec<CopyId> {
    let series = db
        .create_series(&SeriesFields::new(name, category))
        .unwrap();
    (1..=n)
        .map(|i| {
            let issue = db
                .create_issue(
                    series,
                    &IssueFields::new(Some(IssueNumber::new(&i.to_string()).unwrap())),
                )
                .unwrap();
            let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
            db.create_copy(edition, &CopyFields::default()).unwrap()
        })
        .collect()
}

fn place(db: &Database, copy: CopyId) -> (Option<String>, Option<u32>) {
    let f = db.copy(copy).unwrap().fields;
    let name = f.storage_box.map(|b| db.lookup(b).unwrap().name);
    (name, f.box_position)
}

#[test]
fn copies_go_into_a_box_after_what_is_there() {
    let db = db();
    let c = copies(&db, "X-Men", Category::Comic, 4);
    let shelf = db.create_lookup::<BoxId>("Box 1").unwrap();
    let mut f = db.copy(c[3]).unwrap().fields;
    f.storage_box = Some(shelf);
    f.box_position = Some(5);
    db.update_copy(c[3], &f).unwrap();

    let change = BulkChange {
        storage_box: Some(Some("box 1".into())),
        ..BulkChange::default()
    };
    db.bulk_edit(&[c[2], c[0], c[3], c[1]], &change).unwrap();
    assert_eq!(place(&db, c[2]), (Some("Box 1".into()), Some(6)));
    assert_eq!(place(&db, c[0]), (Some("Box 1".into()), Some(7)));
    assert_eq!(
        place(&db, c[3]),
        (Some("Box 1".into()), Some(5)),
        "already there"
    );
    assert_eq!(place(&db, c[1]), (Some("Box 1".into()), Some(8)));

    let out = BulkChange {
        storage_box: Some(None),
        ..BulkChange::default()
    };
    db.bulk_edit(&c, &out).unwrap();
    assert!(c.iter().all(|&copy| place(&db, copy) == (None, None)));
}

#[test]
fn tags_are_added_alongside_existing_ones() {
    let db = db();
    let c = copies(&db, "X-Men", Category::Comic, 2);
    let gift = db.create_lookup::<TagId>("Gift").unwrap();
    db.set_tags(c[0], &[gift]).unwrap();
    let change = BulkChange {
        add_tags: vec!["Key".into(), "For sale".into(), " ".into(), "key".into()],
        ..BulkChange::default()
    };
    db.bulk_edit(&c, &change).unwrap();
    let names =
        |copy| -> Vec<String> { db.tags(copy).unwrap().into_iter().map(|t| t.name).collect() };
    assert_eq!(names(c[0]), ["For sale", "Gift", "Key"]);
    assert_eq!(names(c[1]), ["For sale", "Key"], "typed twice, added once");
}

#[test]
fn grade_type_skips_collected_editions() {
    let db = db();
    let comics = copies(&db, "X-Men", Category::Comic, 1);
    let books = copies(&db, "Omnibus", Category::CollectedEdition, 1);
    let change = BulkChange {
        grade_type: Some(Some(GradeType::Cgc)),
        ..BulkChange::default()
    };
    db.bulk_edit(&[comics[0], books[0]], &change).unwrap();
    assert_eq!(
        db.copy(comics[0]).unwrap().fields.grade_type,
        Some(GradeType::Cgc)
    );
    assert_eq!(db.copy(books[0]).unwrap().fields.grade_type, None);
}

#[test]
fn a_value_is_added_to_every_copy() {
    let db = db();
    let c = copies(&db, "X-Men", Category::Comic, 3);
    let change = BulkChange {
        value: Some((
            "2024-05-01".parse().unwrap(),
            Amount::new(Currency::Usd, 2500),
            "Lot appraisal".into(),
        )),
        ..BulkChange::default()
    };
    db.bulk_edit(&c, &change).unwrap();
    for copy in c {
        let values = db.values(copy).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].source, "Lot appraisal");
    }
}

#[test]
fn one_copy_that_cannot_change_stops_them_all() {
    let db = db();
    let c = copies(&db, "X-Men", Category::Comic, 3);
    db.trash_copy(c[2]).unwrap();
    let change = BulkChange {
        storage_box: Some(Some("New Box".into())),
        add_tags: vec!["Key".into()],
        ..BulkChange::default()
    };
    assert!(matches!(db.bulk_edit(&c, &change), Err(Error::InTrash)));
    assert_eq!(place(&db, c[0]), (None, None));
    assert!(db.list_lookups::<BoxId>().unwrap().is_empty());
    assert!(db.list_lookups::<TagId>().unwrap().is_empty());
}

#[test]
fn several_copies_go_to_trash_and_come_back_together() {
    let db = db();
    let c = copies(&db, "X-Men", Category::Comic, 3);
    let trash = db.trash_copies(&c).unwrap();
    let groups = db.trash_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "3 copies");
    assert!(
        c.iter()
            .all(|&copy| db.copy(copy).unwrap().trash == Some(trash))
    );
    db.restore(trash).unwrap();
    assert!(c.iter().all(|&copy| db.copy(copy).unwrap().trash.is_none()));
    assert!(matches!(db.trash_copies(&[]), Err(Error::Invalid(_))));
}
