//! Tests of storing cover photos.

use crate::model::*;
use crate::{CoverPhoto, Database, Error, MAX_PHOTO_SIDE};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn copy(db: &Database) -> CopyId {
    let series = db
        .create_series(&SeriesFields::new("X-Men", Category::Comic))
        .unwrap();
    let issue = db
        .create_issue(
            series,
            &IssueFields::new(Some(crate::IssueNumber::new("1").unwrap())),
        )
        .unwrap();
    let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
    db.create_copy(edition, &CopyFields::default()).unwrap()
}

/// A stand-in photo: only its opening bytes need to say JPEG.
fn photo(tag: u8) -> CoverPhoto {
    CoverPhoto {
        full_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, tag, tag, tag],
        thumb_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, tag],
        width: 1000 + u32::from(tag),
        height: 2000 + u32::from(tag),
        sha256: [tag; 32],
    }
}

#[test]
fn photos_round_trip_and_replace() {
    let db = db();
    let copy = copy(&db);
    assert_eq!(db.photo(copy, Side::Front).unwrap(), None);
    db.set_photo(copy, Side::Front, &photo(1)).unwrap();
    assert_eq!(db.photo(copy, Side::Front).unwrap(), Some(photo(1)));
    assert_eq!(
        db.photo_thumbnail(copy, Side::Front).unwrap(),
        Some(photo(1).thumb_jpeg)
    );
    assert_eq!(db.photo(copy, Side::Back).unwrap(), None);

    db.set_photo(copy, Side::Front, &photo(2)).unwrap();
    assert_eq!(db.photo(copy, Side::Front).unwrap(), Some(photo(2)));
    let rows: i64 = db
        .conn
        .query_row("SELECT count(*) FROM cover_photo", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "replacing keeps one photo a side");
    let digest: String = db
        .conn
        .query_row("SELECT sha256 FROM cover_photo", [], |r| r.get(0))
        .unwrap();
    assert_eq!(digest, "02".repeat(32));
}

#[test]
fn sides_are_listed_front_first() {
    let db = db();
    let copy = copy(&db);
    assert!(db.photo_sides(copy).unwrap().is_empty());
    db.set_photo(copy, Side::Back, &photo(2)).unwrap();
    assert_eq!(db.photo_sides(copy).unwrap(), [Side::Back]);
    db.set_photo(copy, Side::Front, &photo(1)).unwrap();
    assert_eq!(db.photo_sides(copy).unwrap(), [Side::Front, Side::Back]);
    let edition = db.copy(copy).unwrap().edition;
    let issue = db.edition(edition).unwrap().issue;
    let detail = db.issue_detail(issue).unwrap();
    assert_eq!(
        detail.editions[0].copies[0].photos,
        [Side::Front, Side::Back]
    );
}

#[test]
fn removing_gives_the_photo_back_for_undo() {
    let db = db();
    let copy = copy(&db);
    db.set_photo(copy, Side::Back, &photo(3)).unwrap();
    let removed = db.remove_photo(copy, Side::Back).unwrap();
    assert_eq!(removed, photo(3));
    assert_eq!(db.photo(copy, Side::Back).unwrap(), None);
    assert!(matches!(
        db.remove_photo(copy, Side::Back),
        Err(Error::NotFound)
    ));
    db.set_photo(copy, Side::Back, &removed).unwrap();
    assert_eq!(db.photo(copy, Side::Back).unwrap(), Some(photo(3)));
}

#[test]
fn swapping_exchanges_or_moves_the_sides() {
    let db = db();
    let copy = copy(&db);
    db.set_photo(copy, Side::Front, &photo(1)).unwrap();
    db.set_photo(copy, Side::Back, &photo(2)).unwrap();
    db.swap_photos(copy).unwrap();
    assert_eq!(db.photo(copy, Side::Front).unwrap(), Some(photo(2)));
    assert_eq!(db.photo(copy, Side::Back).unwrap(), Some(photo(1)));

    db.remove_photo(copy, Side::Back).unwrap();
    db.swap_photos(copy).unwrap();
    assert_eq!(db.photo(copy, Side::Front).unwrap(), None);
    assert_eq!(db.photo(copy, Side::Back).unwrap(), Some(photo(2)));
}

#[test]
fn only_sensible_jpegs_are_stored() {
    let db = db();
    let copy = copy(&db);
    let refused = |p: CoverPhoto| {
        assert!(
            matches!(db.set_photo(copy, Side::Front, &p), Err(Error::Invalid(_))),
            "{p:?}"
        );
    };
    refused(CoverPhoto {
        full_jpeg: b"\x89PNG\r\n\x1a\n".to_vec(),
        ..photo(1)
    });
    refused(CoverPhoto {
        thumb_jpeg: b"GIF89a".to_vec(),
        ..photo(1)
    });
    refused(CoverPhoto {
        full_jpeg: Vec::new(),
        ..photo(1)
    });
    refused(CoverPhoto {
        full_jpeg: vec![0xFF, 0xD8, 0x00, 0x00],
        ..photo(1)
    });
    refused(CoverPhoto {
        width: 0,
        ..photo(1)
    });
    refused(CoverPhoto {
        height: 0,
        ..photo(1)
    });
    refused(CoverPhoto {
        width: MAX_PHOTO_SIDE + 1,
        ..photo(1)
    });
    refused(CoverPhoto {
        height: MAX_PHOTO_SIDE + 1,
        ..photo(1)
    });
    db.set_photo(
        copy,
        Side::Front,
        &CoverPhoto {
            width: MAX_PHOTO_SIDE,
            height: 1,
            ..photo(1)
        },
    )
    .unwrap();
}

#[test]
fn photos_follow_their_copy_into_and_out_of_trash() {
    let db = db();
    let copy = copy(&db);
    db.set_photo(copy, Side::Front, &photo(1)).unwrap();
    let trash = db.trash_copy(copy).unwrap();
    assert!(matches!(
        db.set_photo(copy, Side::Back, &photo(2)),
        Err(Error::InTrash)
    ));
    assert!(matches!(
        db.remove_photo(copy, Side::Front),
        Err(Error::InTrash)
    ));
    assert!(matches!(db.swap_photos(copy), Err(Error::InTrash)));
    db.restore(trash).unwrap();
    assert_eq!(db.photo(copy, Side::Front).unwrap(), Some(photo(1)));

    let trash = db.trash_copy(copy).unwrap();
    db.purge(trash).unwrap();
    let rows: i64 = db
        .conn
        .query_row("SELECT count(*) FROM cover_photo", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "deleting a copy for good deletes its photos");
}
