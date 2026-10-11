//! Tests of CSV export.

use crate::export::{COLUMNS, escape, join, join_parts};
use crate::model::*;
use crate::{Amount, BookEntry, CoverPhoto, Currency, Database, Date, IssueNumber, SeriesChoice};

fn date(t: &str) -> Date {
    t.parse().unwrap()
}

fn usd(t: &str) -> Amount {
    Amount::parse(Currency::Usd, t).unwrap()
}

fn new_series(name: &str, year: u16, publisher: Option<&str>) -> SeriesChoice {
    SeriesChoice::New {
        name: name.into(),
        start_year: Some(year),
        volume: None,
        publisher: publisher.map(Into::into),
        magazine_format: None,
        black_and_white: None,
    }
}

/// The CSV as rows of cells, header first.
fn export(db: &Database, copies: Option<&[CopyId]>, photos: bool) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    db.export_csv(copies, &mut out, photos).unwrap();
    csv::Reader::from_reader(out.as_slice())
        .records()
        .map(|r| r.unwrap().iter().map(str::to_owned).collect())
        .collect()
}

/// A cell of `row` by its column's name.
fn cell<'a>(row: &'a [String], column: &str) -> &'a str {
    let at = COLUMNS.iter().position(|c| *c == column).unwrap();
    &row[at]
}

#[test]
fn lists_keep_their_separators_literal() {
    assert_eq!(escape(r"Earth;616|a\b"), r"Earth\;616\|a\\b");
    assert_eq!(join(["Wolverine", "Hulk"]), "Wolverine; Hulk");
    assert_eq!(join(["A;B", "C"]), r"A\;B; C");
    assert_eq!(
        join_parts(["Stan Lee", "coa", "", "NYCC | 2015"]),
        r"Stan Lee | coa |  | NYCC \| 2015"
    );
    assert_eq!(join(Vec::<String>::new()), "");
}

#[test]
fn an_empty_collection_exports_just_the_header() {
    let db = Database::open_in_memory().unwrap();
    let mut out = Vec::new();
    assert_eq!(db.export_csv(None, &mut out, false).unwrap(), 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("Copy ID,Edition ID,Issue ID,Series ID,Category,Series,"));
    assert!(text.ends_with("Back Photo\r\n"), "RFC 4180 line endings");
}

#[test]
fn every_field_is_written() {
    let db = Database::open_in_memory().unwrap();
    db.set_exchange_rate(
        crate::ExchangeRate::new(date("2020-01-01"), usd("1500"), usd("7000")).unwrap(),
    )
    .unwrap();
    let mut e = BookEntry::new(
        Category::Comic,
        new_series("Amazing Spider-Man", 1963, Some("Marvel")),
        Some(IssueNumber::new("121").unwrap()),
    );
    e.issue.title = "The Night Gwen Stacy Died".into();
    e.issue.cover_date = CoverDate::new(1973, Some(6));
    e.issue.release_date = Some(date("1973-03-13"));
    e.issue.page_count = Some(36);
    e.issue.is_key = true;
    e.issue.key_reason = "Death of Gwen Stacy".into();
    e.issue.synopsis = "Line one,\n\"quoted\" line two".into();
    e.credits = vec![
        ("Gerry Conway".into(), Role::Writer),
        ("Gil Kane".into(), Role::Penciller),
        ("John Romita".into(), Role::Inker),
        ("John Romita".into(), Role::CoverArtist),
    ];
    e.characters = vec![("Green Goblin".into(), false), ("Gwen Stacy".into(), true)];
    e.teams = vec![("Avengers".into(), false)];
    e.story_arcs = vec![
        ("Death of Gwen".into(), Some(1)),
        ("Goblin War".into(), None),
    ];
    e.edition.variant = "Newsstand".into();
    e.edition.printing = Some(2);
    e.edition.distribution = Some(Distribution::Newsstand);
    e.edition.cover_price = Some(usd("0.20"));
    e.edition.barcode = Some("071486024217".into());
    e.cover_artists = vec!["John Romita".into()];
    e.copy.grade = Grade::parse("9.4");
    e.copy.grade_type = Some(GradeType::Cgc);
    e.copy.cert_number = Some("1234567001".into());
    e.copy.label_type = Some("Universal".into());
    e.copy.purchase_price = Some(Amount::parse(Currency::Gold, "1").unwrap());
    e.copy.purchase_date = Some(date("2020-05-01"));
    e.copy.seller = "eBay".into();
    e.copy.box_position = Some(3);
    e.copy.notes = "Pressed; über-clean ✓".into();
    e.storage_box = Some("Box 1".into());
    e.tags = vec!["Key".into(), "For sale".into()];
    let mut signed = SignatureFields::new(Verification::CgcSignatureSeries);
    signed.cert_number = Some("SS-99".into());
    signed.signed_on = Some(date("2015-10-10"));
    signed.place = "NYCC | 2015".into();
    signed.sketch = true;
    e.signatures = vec![
        ("Gerry Conway".into(), signed),
        (
            "Gil Kane".into(),
            SignatureFields::new(Verification::Unverified),
        ),
    ];
    let copy = db.add_book(&e).unwrap().copy;
    db.add_value(copy, date("2022-01-01"), usd("1800"), "GoCollect")
        .unwrap();
    db.add_value(
        copy,
        date("2023-01-01"),
        usd("2000"),
        "eBay sold; best offer",
    )
    .unwrap();

    let rows = export(&db, None, false);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.len(), COLUMNS.len());
    let expect = |column: &str, value: &str| {
        assert_eq!(cell(row, column), value, "{column}");
    };
    let stored = db.copy(copy).unwrap();
    expect("Copy ID", &stored.uuid.to_string());
    expect("Category", "comic");
    expect("Series", "Amazing Spider-Man");
    expect("Start Year", "1963");
    expect("Volume", "");
    expect("Publisher", "Marvel");
    expect("Black and White", "");
    expect("Issue Number", "121");
    expect("Story Title", "The Night Gwen Stacy Died");
    expect("Cover Date", "1973-06");
    expect("Release Date", "1973-03-13");
    expect("Page Count", "36");
    expect("Key Issue", "yes");
    expect("Key Reason", "Death of Gwen Stacy");
    expect("Synopsis", "Line one,\n\"quoted\" line two");
    expect("Writers", "Gerry Conway");
    expect("Pencillers", "Gil Kane");
    expect("Inkers", "John Romita");
    expect("Cover Artists", "John Romita");
    expect("Editors", "");
    expect("Characters", "Green Goblin; Gwen Stacy");
    expect("First Appearances of Characters", "Gwen Stacy");
    expect("Teams", "Avengers");
    expect("First Appearances of Teams", "");
    expect("Story Arcs", "Death of Gwen (part 1); Goblin War");
    expect("Collects", "");
    expect("Variant", "Newsstand");
    expect("Printing", "2");
    expect("Distribution", "newsstand");
    expect("Cover Price", "0.20");
    expect("Cover Price Currency", "USD");
    expect("Barcode", "071486024217");
    expect("Variant Cover Artists", "John Romita");
    expect("Grade", "9.4");
    expect("Grade Type", "cgc");
    expect("Certificate Number", "1234567001");
    expect("Label", "Universal");
    expect("Purchase Price", "1.0000");
    expect("Purchase Currency", "XAU");
    expect("Purchase Date", "2020-05-01");
    expect("Seller", "eBay");
    expect("Storage Box", "Box 1");
    expect("Box Position", "3");
    expect("Tags", "For sale; Key");
    expect(
        "Signatures",
        r"Gerry Conway | cgc_ss | SS-99 | 2015-10-10 | NYCC \| 2015 | sketch; Gil Kane | unverified |  |  |  | ",
    );
    expect("Notes", "Pressed; über-clean ✓");
    expect("Current Value", "2000.00");
    expect("Current Value Currency", "USD");
    expect(
        "Value History",
        r"2022-01-01 | 1800.00 | USD | GoCollect; 2023-01-01 | 2000.00 | USD | eBay sold\; best offer",
    );
    expect("Date Added", &stored.added_at);
    expect("Front Photo", "");
}

#[test]
fn collected_editions_list_what_they_collect() {
    let db = Database::open_in_memory().unwrap();
    let mut e = BookEntry::new(
        Category::CollectedEdition,
        new_series("Masterworks", 2003, None),
        Some(IssueNumber::new("1").unwrap()),
    );
    e.collects = "Amazing Spider-Man (1963) 1-10; Annual 1".into();
    e.edition.binding = Some(Binding::Hardcover);
    e.edition.has_dust_jacket = Some(true);
    e.edition.has_slipcase = Some(false);
    e.edition.isbn = Isbn::parse("9780785111818");
    e.copy.book_condition = Some(BookCondition::LikeNew);
    e.copy.dust_jacket_condition = Some(BookCondition::VeryGood);
    db.add_book(&e).unwrap();
    let row = &export(&db, None, false)[0];
    assert_eq!(cell(row, "Category"), "collected");
    assert_eq!(
        cell(row, "Collects"),
        // The text the add form shows and reads back.
        "Amazing Spider-Man (1963) 1\u{2013}10; Annual 1"
    );
    assert_eq!(cell(row, "Binding"), "hardcover");
    assert_eq!(cell(row, "Dust Jacket"), "yes");
    assert_eq!(cell(row, "Slipcase"), "no");
    assert_eq!(cell(row, "ISBN"), "9780785111818");
    assert_eq!(cell(row, "Book Condition"), "like_new");
    assert_eq!(cell(row, "Dust Jacket Condition"), "very_good");
    assert_eq!(cell(row, "Grade"), "");
}

#[test]
fn chosen_copies_export_in_their_order_and_trash_stays_out() {
    let db = Database::open_in_memory().unwrap();
    let add = |n: &str| {
        let choice = match db.list_series().unwrap().first() {
            Some(s) => SeriesChoice::Existing(s.id),
            None => new_series("Thor", 1966, None),
        };
        db.add_book(&BookEntry::new(
            Category::Comic,
            choice,
            Some(IssueNumber::new(n).unwrap()),
        ))
        .unwrap()
        .copy
    };
    let (one, two, three) = (add("1"), add("2"), add("3"));
    let numbers = |rows: Vec<Vec<String>>| -> Vec<String> {
        rows.iter()
            .map(|r| cell(r, "Issue Number").to_owned())
            .collect()
    };
    assert_eq!(numbers(export(&db, Some(&[three, one]), false)), ["3", "1"]);
    db.trash_copy(two).unwrap();
    assert_eq!(numbers(export(&db, None, false)), ["1", "3"]);
}

#[test]
fn photo_paths_name_the_files_a_full_export_holds() {
    let db = Database::open_in_memory().unwrap();
    let copy = db
        .add_book(&BookEntry::new(
            Category::Comic,
            new_series("Zot!", 1984, None),
            Some(IssueNumber::new("1").unwrap()),
        ))
        .unwrap()
        .copy;
    db.set_photo(
        copy,
        Side::Front,
        &CoverPhoto {
            full_jpeg: vec![0xFF, 0xD8, 0xFF],
            thumb_jpeg: vec![0xFF, 0xD8, 0xFF],
            width: 1,
            height: 1,
            sha256: [0; 32],
        },
    )
    .unwrap();
    let uuid = db.copy(copy).unwrap().uuid;
    let row = &export(&db, None, true)[0];
    assert_eq!(cell(row, "Front Photo"), format!("images/{uuid}_front.jpg"));
    assert_eq!(cell(row, "Back Photo"), "");
    let row = &export(&db, None, false)[0];
    assert_eq!(
        cell(row, "Front Photo"),
        "",
        "only a full export holds files"
    );
}
