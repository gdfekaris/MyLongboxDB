//! Tests of CSV import.

use std::io::Cursor;

use crate::export::{COLUMNS, split, split_items, split_parts};
use crate::fixtures::{FixtureSpec, generate};
use crate::import::{
    ImportReport, ImportSettings, ImportSource, Outcome, auto_mapping, decode_text, delimiter,
};
use crate::model::*;
use crate::{Amount, BookEntry, CoverPhoto, Currency, Database, Date, IssueNumber, SeriesChoice};

fn date(t: &str) -> Date {
    t.parse().unwrap()
}

fn settings(source: &ImportSource) -> ImportSettings {
    ImportSettings {
        mapping: auto_mapping(source.headers()),
        separator: ';',
        today: date("2026-10-11"),
        skip_likely_duplicates: false,
    }
}

fn csv_of(db: &Database) -> String {
    let mut out = Vec::new();
    db.export_csv(None, &mut out, false).unwrap();
    String::from_utf8(out).unwrap()
}

/// Imports `text` into `db`, with no photos.
fn import(db: &Database, text: &str) -> ImportReport {
    let mut source = ImportSource::from_csv(text.as_bytes()).unwrap();
    let settings = settings(&source);
    db.import(&mut source, &settings, &|_| Err("no photos".into()))
        .unwrap()
}

fn outcomes(report: &ImportReport) -> Vec<&Outcome> {
    report.rows.iter().map(|r| &r.outcome).collect()
}

fn photo(tag: u8) -> CoverPhoto {
    CoverPhoto {
        full_jpeg: vec![0xFF, 0xD8, 0xFF, tag],
        thumb_jpeg: vec![0xFF, 0xD8, 0xFF, 0],
        width: 1,
        height: 1,
        sha256: [tag; 32],
    }
}

#[test]
fn lists_split_back_into_their_items() {
    assert_eq!(split(r"A\;B; C;;  D ", ';'), ["A;B", "C", "D"]);
    assert_eq!(split(r"a\\; b", ';'), [r"a\", "b"]);
    assert_eq!(split("Wolverine, Hulk", ','), ["Wolverine", "Hulk"]);
    assert!(split("  ", ';').is_empty());
    let items = split_items(r"Stan Lee | coa |  | NYCC \| 2015; Kirby", ';');
    assert_eq!(items, [r"Stan Lee | coa |  | NYCC \| 2015", "Kirby"]);
    assert_eq!(
        split_parts(&items[0]),
        ["Stan Lee", "coa", "", "NYCC | 2015"]
    );
}

#[test]
fn text_is_read_in_the_encoding_spreadsheets_save() {
    assert_eq!(decode_text(b"\xEF\xBB\xBFCaf\xC3\xA9"), "Café");
    assert_eq!(decode_text(b"Caf\xC3\xA9"), "Café");
    assert_eq!(decode_text(b"\xFF\xFEC\0a\0f\0\xE9\0"), "Café");
    assert_eq!(decode_text(b"\xFE\xFF\0C\0a\0f\0\xE9"), "Café");
    // Not UTF-8, so Windows-1252, with its curly quotes.
    assert_eq!(
        decode_text(b"Caf\xE9 \x93hi\x94"),
        "Café \u{201c}hi\u{201d}"
    );
}

#[test]
fn the_delimiter_is_the_one_the_header_uses() {
    assert_eq!(delimiter("Series,Issue\n"), b',');
    assert_eq!(delimiter("Series;Issue;Grade\n"), b';');
    assert_eq!(delimiter("Series\tIssue\n"), b'\t');
    // Separators inside quotes do not count.
    assert_eq!(delimiter("\"a;b;c\",Issue,Grade\n"), b',');
    assert_eq!(delimiter("Series\n"), b',');
}

#[test]
fn headers_are_matched_by_name_or_a_common_other_name() {
    let headers: Vec<String> = [
        "series",
        "Issue #",
        "CONDITION",
        "Price Paid",
        "Mystery",
        "Title",
        "Artist",
        "Penciler",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(
        auto_mapping(&headers),
        [
            Some("Series"),
            Some("Issue Number"),
            Some("Grade"),
            Some("Purchase Price"),
            None,
            // "Title" would mean the series, but that is taken.
            None,
            // An artist is not assumed to have pencilled.
            Some("Artists"),
            Some("Pencillers"),
        ]
    );
    let own: Vec<String> = COLUMNS.iter().map(|c| (*c).to_owned()).collect();
    let mapping = auto_mapping(&own);
    assert!(mapping.iter().zip(COLUMNS).all(|(m, c)| *m == Some(*c)));
}

#[test]
fn a_collection_survives_export_and_import() {
    let db = Database::open_in_memory().unwrap();
    generate(
        &db,
        FixtureSpec {
            copies: 1000,
            seed: 7,
        },
    )
    .unwrap();
    let first = csv_of(&db);

    let fresh = Database::open_in_memory().unwrap();
    let report = import(&fresh, &first);
    let problems: Vec<_> = report
        .rows
        .iter()
        .filter(|r| r.outcome != Outcome::Added)
        .take(5)
        .collect();
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(report.added(), 1000);
    let second = csv_of(&fresh);
    for (a, b) in first.lines().zip(second.lines()) {
        assert_eq!(a, b);
    }
    assert_eq!(first, second);

    // Importing the same file again adds nothing.
    let again = import(&fresh, &first);
    assert_eq!(again.added(), 0);
    assert!(
        outcomes(&again)
            .iter()
            .all(|o| **o == Outcome::AlreadyOwned)
    );
}

#[test]
fn a_full_export_brings_its_photos() {
    let db = Database::open_in_memory().unwrap();
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: "Thor".into(),
            start_year: Some(1966),
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        IssueNumber::new("337").ok(),
    );
    let one = db.add_book(&e).unwrap().copy;
    e.issue.number = IssueNumber::new("338").ok();
    db.add_book(&e).unwrap();
    db.set_photo(one, Side::Front, &photo(1)).unwrap();
    db.set_photo(one, Side::Back, &photo(2)).unwrap();
    let mut out = Cursor::new(Vec::new());
    db.export_zip(None, &mut out).unwrap();

    let fresh = Database::open_in_memory().unwrap();
    let mut source = ImportSource::from_zip(Cursor::new(out.into_inner())).unwrap();
    assert!(source.is_own_format());
    let settings = settings(&source);
    let dry = fresh.check_import(&source, &settings).unwrap();
    assert_eq!(dry.added(), 2);
    assert!(
        fresh.list_series().unwrap().is_empty(),
        "a check changes nothing"
    );

    // The photos reach `prepare` as the files the archive holds.
    let report = fresh
        .import(&mut source, &settings, &|bytes| {
            if bytes[3] == 2 {
                return Err("unreadable".into());
            }
            Ok(CoverPhoto {
                full_jpeg: bytes,
                ..photo(0)
            })
        })
        .unwrap();
    assert_eq!(report.added(), 2);
    assert_eq!(report.photo_problems.len(), 1);
    assert!(report.photo_problems[0].ends_with("_back.jpg: unreadable"));
    let copy = fresh.browse(crate::Shelf::Everything).unwrap()[0].copy;
    assert_eq!(
        fresh.photo(copy, Side::Front).unwrap().unwrap().full_jpeg,
        photo(1).full_jpeg
    );
    assert_eq!(fresh.photo_sides(copy).unwrap(), [Side::Front]);
}

#[test]
fn a_spreadsheet_from_elsewhere_is_read_leniently() {
    let db = Database::open_in_memory().unwrap();
    let text = "Title;Year;Issue #;Condition;Price Paid;Key;Writer;Value\r\n\
                Swamp Thing;1972;#1;VF/NM 9.0;\"$1,250.00\";Yes;Len Wein, Bernie Wrightson;$2000\r\n\
                ;;;;;;;\r\n\
                Swamp Thing;1972;2;8.5;12;no;;\r\n";
    let mut source = ImportSource::from_csv(text.as_bytes()).unwrap();
    assert_eq!(source.len(), 2, "blank rows are left out");
    let mut settings = settings(&source);
    settings.separator = ',';
    let report = db
        .import(&mut source, &settings, &|_| unreachable!())
        .unwrap();
    assert_eq!(report.rows[0].label, "Swamp Thing (1972 series) #1");
    assert_eq!(report.rows[1].line, 4);
    assert_eq!(report.added(), 2);
    let rows = db.browse(crate::Shelf::Everything).unwrap();
    assert_eq!(rows.len(), 2, "both join one series");
    let first = db.book_entry(rows[0].copy).unwrap();
    assert_eq!(first.copy.grade, Grade::parse("9.0"));
    assert_eq!(
        first.copy.purchase_price,
        Some(Amount::parse(Currency::Usd, "1250").unwrap())
    );
    assert!(first.issue.is_key);
    assert_eq!(first.credits.len(), 2);
    let values = db.values(rows[0].copy).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0].date,
        date("2026-10-11"),
        "dated the day of import"
    );
}

#[test]
fn page_quality_is_read_as_cgc_writes_it_or_abbreviated() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Pages\n\
                Thor,1,White Pages\n\
                Thor,2,Off-White to White\n\
                Thor,3,OW/W\n\
                Thor,4,Brown / Brittle\n\
                Thor,5,cream_to_off_white\n\
                Thor,6,\n\
                Thor,7,pinkish\n";
    let report = import(&db, text);
    assert_eq!(outcomes(&report)[..6], [&Outcome::Added; 6]);
    let pages: Vec<Option<PageQuality>> = db
        .browse(crate::Shelf::Everything)
        .unwrap()
        .iter()
        .map(|row| db.copy(row.copy).unwrap().fields.page_quality)
        .collect();
    assert_eq!(
        pages,
        [
            Some(PageQuality::White),
            Some(PageQuality::OffWhiteToWhite),
            Some(PageQuality::OffWhiteToWhite),
            Some(PageQuality::BrownBrittle),
            Some(PageQuality::CreamToOffWhite),
            None,
        ]
    );
    assert_eq!(
        report.rows[6].outcome,
        Outcome::Problem("Page Quality: \u{201c}pinkish\u{201d} cannot be read".into())
    );
}

#[test]
fn page_quality_on_a_collected_edition_is_a_problem() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Category,Page Quality\n\
                Omnibus,1,collected,white\n";
    let report = import(&db, text);
    assert!(
        matches!(&report.rows[0].outcome, Outcome::Problem(_)),
        "{:?}",
        report.rows[0].outcome
    );
    assert_eq!(report.added(), 0);
}

#[test]
fn signatures_are_read_with_or_without_their_placement() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Signatures\n\
                Thor,1,Stan Lee | cbcs_witnessed | B-1 | 2010-07-22 | SDCC | | first_page\n\
                Thor,2,Stan Lee | cgc_ss |  |  |  | \n\
                Thor,3,Stan Lee | cgc_ss |  |  |  |  | upside_down\n";
    let report = import(&db, text);
    assert_eq!(outcomes(&report)[..2], [&Outcome::Added; 2]);
    assert_eq!(
        report.rows[2].outcome,
        Outcome::Problem(
            "Signatures: \u{201c}Stan Lee | cgc_ss |  |  |  |  | upside_down\u{201d} cannot be read"
                .into()
        )
    );
    let rows = db.browse(crate::Shelf::Everything).unwrap();
    let first = &db.signatures(rows[0].copy).unwrap()[0].fields;
    assert_eq!(first.verification, Verification::CbcsWitnessed);
    assert_eq!(first.placement, Some(SignaturePlacement::FirstPage));
    let second = &db.signatures(rows[1].copy).unwrap()[0].fields;
    assert_eq!(
        second.placement, None,
        "an export made before placement was kept"
    );
}

#[test]
fn a_sketch_is_read_as_yes_or_no_in_any_case() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Signatures\n\
                Thor,1,Stan Lee | | | | | sketch\n\
                Thor,2,Stan Lee | | | | | Yes\n\
                Thor,3,Stan Lee | | | | | No\n\
                Thor,4,Stan Lee | | | | | false\n\
                Thor,5,Stan Lee | | | | | \n\
                Thor,6,Stan Lee | | | | | maybe\n";
    let report = import(&db, text);
    assert_eq!(outcomes(&report)[..5], [&Outcome::Added; 5]);
    assert!(
        matches!(&report.rows[5].outcome, Outcome::Problem(_)),
        "{:?}",
        report.rows[5].outcome
    );
    let sketched: Vec<bool> = db
        .browse(crate::Shelf::Everything)
        .unwrap()
        .iter()
        .map(|row| db.signatures(row.copy).unwrap()[0].fields.sketch)
        .collect();
    assert_eq!(sketched, [true, true, false, false, false]);
}

#[test]
fn a_canadian_price_may_carry_any_usual_mark() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Canadian Cover Price\n\
                Thor,1,CA$0.75\n\
                Thor,2,C$0.75\n\
                Thor,3,CAD 0.75\n\
                Thor,4,cad0.75\n\
                Thor,5,$0.75\n\
                Thor,6,0.75\n\
                Thor,7,CADX\n";
    let report = import(&db, text);
    assert_eq!(outcomes(&report)[..6], [&Outcome::Added; 6]);
    assert!(
        matches!(&report.rows[6].outcome, Outcome::Problem(_)),
        "{:?}",
        report.rows[6].outcome
    );
    for row in db.browse(crate::Shelf::Everything).unwrap() {
        let entry = db.book_entry(row.copy).unwrap();
        assert_eq!(entry.edition.canadian_cover_price_cents, Some(75));
    }
}

#[test]
fn each_problem_names_its_column() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Grade,Purchase Date,Category,Signatures\n\
                ,1,,,,\n\
                Thor,1,9.85,,,\n\
                Thor,2,,31/12/2020,,\n\
                Thor,3,,,newspaper,\n\
                Thor,4,,,,Stan Lee | forged\n\
                Thor,5,,,,\n";
    let report = import(&db, text);
    let problems: Vec<String> = report
        .rows
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Problem(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        problems,
        [
            "Series: every book needs a series",
            "Grade: \u{201c}9.85\u{201d} cannot be read",
            "Purchase Date: \u{201c}31/12/2020\u{201d} cannot be read",
            "Category: \u{201c}newspaper\u{201d} cannot be read",
            "Signatures: \u{201c}Stan Lee | forged\u{201d} cannot be read",
        ]
    );
    assert_eq!(report.added(), 1);
}

#[test]
fn a_row_the_database_refuses_leaves_no_trace() {
    let db = Database::open_in_memory().unwrap();
    let mut e = BookEntry::new(
        Category::Magazine,
        SeriesChoice::New {
            name: "Heavy Metal".into(),
            start_year: Some(1977),
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        IssueNumber::new("1").ok(),
    );
    e.copy.notes = "mine".into();
    db.add_book(&e).unwrap();
    let text = "Series,Start Year,Issue Number,Category,Tags\n\
                Heavy Metal,1977,2,comic,New tag\n";
    let report = import(&db, text);
    assert_eq!(
        outcomes(&report),
        [&Outcome::Problem(
            "\u{201c}Heavy Metal\u{201d} is a magazine series".into()
        )]
    );
    assert_eq!(db.browse(crate::Shelf::Everything).unwrap().len(), 1);
    assert!(db.list_lookups::<TagId>().unwrap().is_empty());
}

#[test]
fn likely_duplicates_are_flagged_and_can_be_left_out() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Start Year,Issue Number,Variant\n\
                Thor,1966,337,\n\
                Thor,1966,337,Cover B\n";
    assert_eq!(import(&db, text).added(), 2);
    // The file's two copies were not owned before it, so neither is
    // flagged against the other.
    let text = "Series,Start Year,Issue Number,Variant,Printing\n\
                thor,1966,337,,\n\
                Thor,1966,337,,2\n\
                Thor,1966,338,,\n\
                Thor,1966,338,,\n";
    let mut source = ImportSource::from_csv(text.as_bytes()).unwrap();
    let mut settings = settings(&source);
    let check = db.check_import(&source, &settings).unwrap();
    let note =
        "you already own a copy of this issue, variant, printing, and distribution".to_owned();
    assert_eq!(
        outcomes(&check),
        [
            &Outcome::LikelyDuplicate {
                note: note.clone(),
                added: true
            },
            &Outcome::Added,
            &Outcome::Added,
            &Outcome::Added,
        ]
    );
    settings.skip_likely_duplicates = true;
    let report = db
        .import(&mut source, &settings, &|_| unreachable!())
        .unwrap();
    assert_eq!(
        report.rows[0].outcome,
        Outcome::LikelyDuplicate { note, added: false }
    );
    assert_eq!(report.added(), 3);
    assert_eq!(db.browse(crate::Shelf::Everything).unwrap().len(), 5);
}

#[test]
fn ids_already_taken_are_not_reused() {
    let db = Database::open_in_memory().unwrap();
    let e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: "Thor".into(),
            start_year: Some(1966),
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        IssueNumber::new("1").ok(),
    );
    let copy = db.add_book(&e).unwrap().copy;
    let csv = csv_of(&db);
    // The row with only the copy's id changed is a second copy, of the
    // same edition.
    let other = db.copy(copy).unwrap().uuid.to_string();
    let changed = csv.replacen(&other, "00000000-0000-4000-8000-000000000001", 1);
    let report = import(&db, &changed);
    assert_eq!(report.added(), 1);
    let rows = db.browse(crate::Shelf::Everything).unwrap();
    assert_eq!(rows.len(), 2);
    let editions: Vec<_> = rows
        .iter()
        .map(|r| db.copy(r.copy).unwrap().edition)
        .collect();
    assert_eq!(
        editions[0], editions[1],
        "it joins the edition with that id"
    );

    // A copy in Trash is not imported again beside itself.
    db.trash_copy(copy).unwrap();
    assert_eq!(
        outcomes(&import(&db, &csv)),
        [&Outcome::Problem(
            "this copy is in Trash; restore it from there".into()
        )]
    );
}

#[test]
fn files_that_are_not_imports_are_refused() {
    assert!(ImportSource::from_csv(b"").is_err());
    assert!(ImportSource::from_zip(Cursor::new(b"PK not a zip".to_vec())).is_err());
    let mut out = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    zip.start_file("other.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    assert!(ImportSource::from_zip(Cursor::new(out.into_inner())).is_err());
}

#[test]
fn money_fields_keep_their_currency() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Issue Number,Purchase Price,Purchase Currency,Value History\n\
                Thor,1,0.25,XAU,2020-01-01 | 0.001 | BTC | auction; 2021-01-01 | 5 | USD | \n";
    assert_eq!(import(&db, text).added(), 1);
    let copy = db.browse(crate::Shelf::Everything).unwrap()[0].copy;
    assert_eq!(
        db.copy(copy).unwrap().fields.purchase_price,
        Some(Amount::parse(Currency::Gold, "0.25").unwrap())
    );
    let values = db.values(copy).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(
        values[0].value,
        Amount::parse(Currency::Btc, "0.001").unwrap()
    );
    assert_eq!(values[0].source, "auction");
}

#[test]
fn rows_are_reported_by_their_line_in_the_file() {
    for ending in ["\n", "\r\n"] {
        let text = [
            "Series,Notes",
            "Thor,",
            "",
            ",",
            "Thor,\"two",
            "lines\"",
            "Hulk,",
        ]
        .join(ending);
        let source = ImportSource::from_csv(text.as_bytes()).unwrap();
        let db = Database::open_in_memory().unwrap();
        let report = db.check_import(&source, &settings(&source)).unwrap();
        let lines: Vec<usize> = report.rows.iter().map(|r| r.line).collect();
        assert_eq!(lines, [2, 5, 7], "{ending:?}");
    }
}

#[test]
fn amounts_and_grades_are_read_as_people_write_them() {
    use crate::import::{grade, money};
    let usd = |t: &str| Some(Amount::parse(Currency::Usd, t).unwrap());
    assert_eq!(money("$1,250.00", ""), usd("1250"));
    assert_eq!(money("1,250", ""), usd("1250"));
    assert_eq!(money("12,50", ""), usd("12.50"));
    assert_eq!(money(" $ 3 ", "usd"), usd("3"));
    assert_eq!(money("1,2,3", ""), usd("123"));
    assert_eq!(money("5", "EUR"), None, "only currencies the app keeps");
    assert_eq!(money("-5", ""), None);
    assert_eq!(grade("9.4"), Grade::parse("9.4"));
    assert_eq!(grade("NM 9.4"), Grade::parse("9.4"));
    assert_eq!(grade("vf/nm"), Grade::parse("9.0"));
    assert_eq!(grade("great"), None);
}

#[test]
#[ignore = "slow in debug builds; run in release"]
fn importing_ten_thousand_copies_is_quick() {
    let db = Database::open_in_memory().unwrap();
    generate(
        &db,
        FixtureSpec {
            copies: 10_000,
            seed: 42,
        },
    )
    .unwrap();
    let text = csv_of(&db);
    let fresh = Database::open_in_memory().unwrap();
    let source = ImportSource::from_csv(text.as_bytes()).unwrap();
    let start = std::time::Instant::now();
    let check = fresh.check_import(&source, &settings(&source)).unwrap();
    let checked = start.elapsed();
    assert_eq!(check.added(), 10_000);
    let start = std::time::Instant::now();
    assert_eq!(import(&fresh, &text).added(), 10_000);
    let imported = start.elapsed();
    eprintln!("check {checked:?}, import {imported:?}");
    for took in [checked, imported] {
        assert!(took.as_secs() < 20, "{took:?}");
    }
}

#[test]
fn what_the_collection_already_has_is_kept() {
    let db = Database::open_in_memory().unwrap();
    let mut thor = SeriesFields::new("Thor", Category::Comic);
    thor.start_year = Some(1966);
    thor.notes = "Mine".into();
    thor.known_issue_count = Some(400);
    let thor = db.create_series(&thor).unwrap();
    let mut hulk = SeriesFields::new("Hulk", Category::Comic);
    hulk.start_year = Some(1962);
    let hulk = db.create_series(&hulk).unwrap();
    let ids = [thor, hulk].map(|s| db.series(s).unwrap().uuid);
    let text = "Series ID,Series,Start Year,Issue Number,Series Notes,Known Issue Count\n\
                00000000-0000-4000-8000-000000000001,Thor,1966,1,Theirs,500\n\
                00000000-0000-4000-8000-000000000002,Hulk,1962,1,Theirs,6\n";
    assert_eq!(import(&db, text).added(), 2);
    // Each book joins the series by its name, which keeps its own id.
    assert_eq!([thor, hulk].map(|s| db.series(s).unwrap().uuid), ids);
    assert_eq!(db.list_series().unwrap().len(), 2);
    // Blank fields are filled in; filled ones are not replaced.
    let thor = db.series(thor).unwrap().fields;
    assert_eq!(
        (thor.notes.as_str(), thor.known_issue_count),
        ("Mine", Some(400))
    );
    let hulk = db.series(hulk).unwrap().fields;
    assert_eq!(
        (hulk.notes.as_str(), hulk.known_issue_count),
        ("Theirs", Some(6))
    );
}

#[test]
fn an_import_can_go_a_few_rows_at_a_time() {
    let db = Database::open_in_memory().unwrap();
    generate(
        &db,
        FixtureSpec {
            copies: 40,
            seed: 3,
        },
    )
    .unwrap();
    let text = csv_of(&db);
    let source = ImportSource::from_csv(text.as_bytes()).unwrap();
    let settings = settings(&source);
    let fresh = Database::open_in_memory().unwrap();
    let whole = fresh.check_import(&source, &settings).unwrap();

    let mut run = fresh.start_import(&source).unwrap();
    let mut slices = 0;
    while !fresh.import_rows(&mut run, &source, &settings, 7).unwrap() {
        slices += 1;
        assert_eq!(run.rows_done(), slices * 7);
    }
    assert_eq!(slices, 5);
    assert_eq!(run.report(), &whole);
    // Abandoned, it leaves nothing behind; finished, it keeps it all.
    run.abandon().unwrap();
    assert!(fresh.browse(crate::Shelf::Everything).unwrap().is_empty());
    let mut run = fresh.start_import(&source).unwrap();
    while !fresh.import_rows(&mut run, &source, &settings, 7).unwrap() {}
    assert_eq!(run.finish().unwrap(), whole);
    assert_eq!(csv_of(&fresh), text);
}

#[test]
fn files_are_opened_by_what_they_hold() {
    let dir = std::env::temp_dir().join(format!("mlbx-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::open_in_memory().unwrap();
    let e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: "Thor".into(),
            start_year: None,
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        IssueNumber::new("1").ok(),
    );
    db.add_book(&e).unwrap();
    // Named .csv, but a zip all the same.
    let zip_path = dir.join("export.csv");
    db.export_zip(None, std::fs::File::create(&zip_path).unwrap())
        .unwrap();
    let csv_path = dir.join("books.txt");
    std::fs::write(&csv_path, csv_of(&db)).unwrap();
    let mut from_zip = ImportSource::open(&zip_path).unwrap();
    let from_csv = ImportSource::open(&csv_path).unwrap();
    assert_eq!(from_zip.headers(), from_csv.headers());
    assert_eq!(from_zip.len(), 1);
    assert!(from_zip.read_photo("images/none.jpg").is_err());
    assert!(ImportSource::open(&dir.join("missing.csv")).is_err());
    // A collection file is named as one, so the message can say where it
    // goes instead.
    let collection = dir.join("Comics.sqlite");
    rusqlite::Connection::open(&collection)
        .unwrap()
        .execute_batch("CREATE TABLE t (x)")
        .unwrap();
    assert!(matches!(
        ImportSource::open(&collection),
        Err(crate::Error::CollectionFile)
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cells_a_spreadsheet_would_run_are_guarded_and_come_back_as_they_were() {
    let db = Database::open_in_memory().unwrap();
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: "=HYPERLINK(\"http://example.com\")".into(),
            start_year: None,
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new("-1").unwrap()),
    );
    e.issue.title = "'Til Death".into();
    e.copy.notes = "@SUM(A1)".into();
    e.edition.variant = "+1 Variant".into();
    db.add_book(&e).unwrap();
    let text = csv_of(&db);
    for guarded in [
        "'=HYPERLINK",
        ",'-1,",
        "''Til Death",
        "'@SUM(A1)",
        "'+1 Variant",
    ] {
        assert!(text.contains(guarded), "{guarded} in {text}");
    }

    let fresh = Database::open_in_memory().unwrap();
    assert_eq!(import(&fresh, &text).added(), 1);
    assert_eq!(csv_of(&fresh), text, "exact on the way back");
    let row = &fresh.browse(crate::Shelf::Everything).unwrap()[0];
    let back = fresh.book_entry(row.copy).unwrap();
    assert_eq!(back.issue.number.unwrap().as_str(), "-1");
    assert_eq!(back.issue.title, "'Til Death");
    assert_eq!(back.copy.notes, "@SUM(A1)");
    assert_eq!(back.edition.variant, "+1 Variant");

    // A file from elsewhere keeps a ' that guards nothing.
    let other = Database::open_in_memory().unwrap();
    import(
        &other,
        "Series,Issue Number,Story Title\nZot!,1,'Til Death\n",
    );
    let row = &other.browse(crate::Shelf::Everything).unwrap()[0];
    assert_eq!(
        other.book_entry(row.copy).unwrap().issue.title,
        "'Til Death"
    );
}

#[test]
fn a_run_dropped_unfinished_is_undone_and_later_edits_stay() {
    let db = Database::open_in_memory().unwrap();
    let source =
        ImportSource::from_csv("Series,Issue Number\nThor,1\nThor,2\n".as_bytes()).unwrap();
    let mut run = db.start_import(&source).unwrap();
    db.import_rows(&mut run, &source, &settings(&source), 1)
        .unwrap();
    // Neither finished nor abandoned, as an early return could leave it.
    drop(run);
    assert!(
        db.conn.is_autocommit(),
        "no savepoint is left for later edits to join"
    );
    assert!(db.browse(crate::Shelf::Everything).unwrap().is_empty());
    db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    assert!(db.conn.is_autocommit());
}

#[test]
fn a_copy_of_another_distribution_is_not_a_likely_duplicate() {
    let db = Database::open_in_memory().unwrap();
    let text = "Series,Start Year,Issue Number,Distribution\n\
                Thor,1966,337,direct\n";
    assert_eq!(import(&db, text).added(), 1);
    // The newsstand copy goes to an edition of its own, so it is not
    // flagged; another direct copy joins the one owned, so it is.
    let text = "Series,Start Year,Issue Number,Distribution\n\
                Thor,1966,337,newsstand\n\
                Thor,1966,337,direct\n";
    let source = ImportSource::from_csv(text.as_bytes()).unwrap();
    let check = db.check_import(&source, &settings(&source)).unwrap();
    assert_eq!(
        outcomes(&check),
        [
            &Outcome::Added,
            &Outcome::LikelyDuplicate {
                note: "you already own a copy of this issue, variant, printing, and distribution"
                    .into(),
                added: true
            },
        ]
    );
}

#[test]
fn series_characters_and_teams_are_added_to_the_series() {
    let db = Database::open_in_memory().unwrap();
    let mut thor = SeriesFields::new("Thor", Category::Comic);
    thor.start_year = Some(1966);
    let thor = db.create_series(&thor).unwrap();
    let odin = db.create_lookup::<CharacterId>("Odin").unwrap();
    db.set_series_characters(thor, &[odin]).unwrap();
    let text = "Series,Start Year,Issue Number,Series Characters,Series Teams\n\
                Thor,1966,1,Thor; Loki,Asgardians\n\
                Thor,1966,2,,\n";
    assert_eq!(import(&db, text).added(), 2);
    let names = |list: Vec<crate::Named<CharacterId>>| -> Vec<String> {
        list.into_iter().map(|n| n.name).collect()
    };
    // Those it had are kept, and a row with none takes none away.
    assert_eq!(
        names(db.series_characters(thor).unwrap()),
        ["Loki", "Odin", "Thor"]
    );
    assert_eq!(db.series_teams(thor).unwrap()[0].name, "Asgardians");

    // And they come back from an export.
    let fresh = Database::open_in_memory().unwrap();
    assert_eq!(import(&fresh, &csv_of(&db)).added(), 2);
    let series = fresh.list_series().unwrap()[0].id;
    assert_eq!(
        names(fresh.series_characters(series).unwrap()),
        ["Loki", "Odin", "Thor"]
    );
}

/// A live series made the way the add form makes one.
fn series(
    db: &Database,
    name: &str,
    category: Category,
    start_year: Option<u16>,
    volume: Option<u32>,
) -> SeriesId {
    let mut fields = SeriesFields::new(name, category);
    fields.start_year = start_year;
    fields.volume = volume;
    db.create_series(&fields).unwrap()
}

/// Each live series as (name, start year, volume, issues).
fn series_list(db: &Database) -> Vec<(String, Option<u16>, Option<u32>, usize)> {
    db.list_series()
        .unwrap()
        .into_iter()
        .map(|s| {
            let issues = db.issues_in_series(s.id).unwrap().len();
            (s.fields.name, s.fields.start_year, s.fields.volume, issues)
        })
        .collect()
}

fn ff(volume: Option<u32>, issues: usize) -> (String, Option<u16>, Option<u32>, usize) {
    ("Fantastic Four".into(), Some(1961), volume, issues)
}

#[test]
fn a_blank_volume_in_the_row_joins_the_series_of_that_name_and_year() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "Fantastic Four", Category::Comic, Some(1961), Some(1));
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,,29\n\
         fantastic  four,1961,,57\n",
    );
    assert_eq!(outcomes(&report), [&Outcome::Added, &Outcome::Added]);
    // The series keeps the volume it had.
    assert_eq!(series_list(&db), [ff(Some(1), 2)]);
    assert!(report.rows.iter().all(|r| !r.new_series));
}

#[test]
fn a_volume_in_the_row_joins_the_series_that_has_none() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "Fantastic Four", Category::Comic, Some(1961), None);
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,1,29\n",
    );
    assert_eq!(outcomes(&report), [&Outcome::Added]);
    // The series is not given the row's volume.
    assert_eq!(series_list(&db), [ff(None, 1)]);
}

#[test]
fn a_series_matching_the_volume_exactly_is_chosen_first() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "Fantastic Four", Category::Comic, Some(1961), Some(1));
    series(&db, "Fantastic Four", Category::Comic, Some(1961), None);
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,1,29\n\
         Fantastic Four,1961,1,30\n\
         Fantastic Four,1961,,57\n",
    );
    assert_eq!(report.added(), 3);
    assert_eq!(series_list(&db), [ff(None, 1), ff(Some(1), 2)]);
}

#[test]
fn lookalike_series_a_file_keeps_apart_stay_apart() {
    // An export of a collection with "Fantastic Four (1961)" twice, once
    // without a volume, into one that has only vol 1: the row without a
    // volume would otherwise join vol 1, whichever comes first.
    let text = "Series ID,Series,Start Year,Volume,Issue Number\n\
                11111111-1111-4111-8111-111111111111,Fantastic Four,1961,,2\n\
                22222222-2222-4222-8222-222222222222,Fantastic Four,1961,1,1\n";
    let db = Database::open_in_memory().unwrap();
    series(&db, "Fantastic Four", Category::Comic, Some(1961), Some(1));
    let report = import(&db, text);
    assert_eq!(report.added(), 2);
    assert_eq!(series_list(&db), [ff(None, 1), ff(Some(1), 1)]);
    // A file naming each series only once still joins loosely.
    let db = Database::open_in_memory().unwrap();
    series(&db, "Fantastic Four", Category::Comic, Some(1961), Some(1));
    import(
        &db,
        text.lines().take(2).collect::<Vec<_>>().join("\n").as_str(),
    );
    assert_eq!(series_list(&db), [ff(Some(1), 1)]);
}

#[test]
fn series_of_different_stated_volumes_stay_apart() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "Superman", Category::Comic, Some(1987), Some(2));
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Superman,1987,3,1\n\
         Superman,1987,3,2\n",
    );
    assert_eq!(report.added(), 2);
    assert_eq!(
        series_list(&db),
        [
            ("Superman".into(), Some(1987), Some(2), 0),
            ("Superman".into(), Some(1987), Some(3), 2),
        ]
    );
    // Only the row that made the series says so.
    let made: Vec<bool> = report.rows.iter().map(|r| r.new_series).collect();
    assert_eq!(made, [true, false]);
}

#[test]
fn a_row_that_could_join_several_series_is_a_problem() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "X", Category::Comic, Some(1990), Some(1));
    series(&db, "X", Category::Comic, Some(1990), Some(2));
    // Old duplicates with no volume: the first is joined, as an exact
    // match joins the first of several.
    let y = series(&db, "Y", Category::Comic, None, None);
    series(&db, "Y", Category::Comic, None, None);
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         X,1990,,1\n\
         Y,,4,1\n\
         X,1990,2,1\n",
    );
    assert_eq!(
        outcomes(&report),
        [
            &Outcome::Problem(
                "several series match \u{201c}X (1990 series)\u{201d}: Vol. 1 and Vol. 2; \
                 give the row a Volume to choose one"
                    .into()
            ),
            &Outcome::Added,
            &Outcome::Added,
        ]
    );
    assert_eq!(db.list_series().unwrap().len(), 4);
    assert_eq!(db.issues_in_series(y).unwrap().len(), 1);
}

#[test]
fn the_start_year_must_still_match() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "Flash", Category::Comic, Some(1959), Some(1));
    series(&db, "Y", Category::Comic, None, Some(2));
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Flash,2016,,1\n\
         Y,,,1\n",
    );
    assert_eq!(report.added(), 2);
    assert_eq!(
        series_list(&db),
        [
            ("Flash".into(), Some(1959), Some(1), 0),
            ("Flash".into(), Some(2016), None, 1),
            ("Y".into(), None, Some(2), 1),
        ]
    );
    let made: Vec<bool> = report.rows.iter().map(|r| r.new_series).collect();
    assert_eq!(made, [true, false]);
}

#[test]
fn a_series_in_trash_is_not_joined() {
    let db = Database::open_in_memory().unwrap();
    let z = series(&db, "Z", Category::Comic, Some(1970), Some(1));
    db.trash_series(z).unwrap();
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Z,1970,,1\n",
    );
    assert_eq!(report.added(), 1);
    assert_eq!(series_list(&db), [("Z".into(), Some(1970), None, 1)]);
}

#[test]
fn copies_in_the_series_joined_count_as_likely_duplicates() {
    let db = Database::open_in_memory().unwrap();
    import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,1,29\n",
    );
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,,29\n",
    );
    assert_eq!(
        outcomes(&report),
        [&Outcome::LikelyDuplicate {
            note: "you already own a copy of this issue, variant, printing, and distribution"
                .into(),
            added: true
        }]
    );
    assert_eq!(series_list(&db), [ff(Some(1), 1)]);
}

#[test]
fn only_a_series_of_the_rows_kind_is_joined_without_its_volume() {
    let db = Database::open_in_memory().unwrap();
    series(&db, "W", Category::Magazine, Some(1975), Some(1));
    series(&db, "V", Category::Magazine, Some(1975), Some(1));
    series(&db, "V", Category::Comic, Some(1975), Some(2));
    let report = import(
        &db,
        "Series,Start Year,Volume,Issue Number,Category\n\
         W,1975,,1,comic\n\
         V,1975,,1,comic\n",
    );
    // The magazine is the only W, so the row is told why it cannot join
    // it, rather than starting a comic series of the same name.
    assert_eq!(
        outcomes(&report),
        [
            &Outcome::Problem("\u{201c}W\u{201d} is a magazine series".into()),
            &Outcome::Added,
        ]
    );
    assert_eq!(
        series_list(&db),
        [
            ("V".into(), Some(1975), Some(1), 0),
            ("V".into(), Some(1975), Some(2), 1),
            ("W".into(), Some(1975), Some(1), 0),
        ]
    );
}

#[test]
fn an_export_brings_back_every_series_with_its_volume() {
    let db = Database::open_in_memory().unwrap();
    import(
        &db,
        "Series,Start Year,Volume,Issue Number\n\
         Fantastic Four,1961,1,1\n\
         Fantastic Four,1961,,2\n\
         Superman,1987,2,1\n\
         Superman,1987,3,1\n\
         Superman,,,1\n",
    );
    // A blank volume joins the 1961 series rather than starting another.
    let before = series_list(&db);
    assert_eq!(before.len(), 4);
    let fresh = Database::open_in_memory().unwrap();
    let report = import(&fresh, &csv_of(&db));
    assert_eq!(report.added(), 5);
    assert_eq!(series_list(&fresh), before);
}

mod properties {
    use proptest::prelude::*;

    use super::*;

    /// A name, start year, and volume from a few each, so they often meet.
    fn key() -> impl Strategy<Value = (usize, Option<u16>, Option<u32>)> {
        (
            0..2usize,
            prop::option::of(1990..1992u16),
            prop::option::of(1..3u32),
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn an_import_never_starts_a_series_that_one_already_could_be(
            existing in prop::collection::vec(key(), 0..4),
            rows in prop::collection::vec(key(), 1..6),
        ) {
            const NAMES: [&str; 2] = ["Thor", "Hulk"];
            let db = Database::open_in_memory().unwrap();
            for (name, year, volume) in &existing {
                series(&db, NAMES[*name], Category::Comic, *year, *volume);
            }
            let cell = |v: Option<String>| v.unwrap_or_default();
            let mut text = "Series,Start Year,Volume,Issue Number\n".to_owned();
            for (i, (name, year, volume)) in rows.iter().enumerate() {
                text.push_str(&format!(
                    "{},{},{},{}\n",
                    NAMES[*name],
                    cell(year.map(|y| y.to_string())),
                    cell(volume.map(|v| v.to_string())),
                    i + 1
                ));
            }
            let before = db.list_series().unwrap();
            import(&db, &text);
            for made in db.list_series().unwrap().iter().filter(|s| s.id > before.iter().map(|b| b.id).max().unwrap_or(SeriesId(0))) {
                let f = &made.fields;
                for old in db.list_series().unwrap().iter().filter(|s| s.id != made.id) {
                    let o = &old.fields;
                    let differ = matches!((o.volume, f.volume), (Some(a), Some(b)) if a != b);
                    prop_assert!(
                        o.name != f.name || o.start_year != f.start_year || differ,
                        "{f:?} was made beside {o:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_series_the_import_made_takes_what_its_own_rows_say() {
    let db = Database::open_in_memory().unwrap();
    // The omnibus makes "Golem (2013)" first, with only a name and year.
    let report = import(
        &db,
        "Category,Series,Start Year,Volume,Publisher,Issue Number,Collects\n\
         collected,Golem Omnibus,,,,1,Golem (2013) #1-2\n\
         comic,Golem,2013,1,Orbit Line,1,\n",
    );
    assert_eq!(report.added(), 2);
    let made: Vec<bool> = report.rows.iter().map(|r| r.new_series).collect();
    assert_eq!(made, [true, false]);
    let golem: Vec<_> = db
        .list_series()
        .unwrap()
        .into_iter()
        .filter(|s| s.fields.name == "Golem")
        .collect();
    assert_eq!(golem.len(), 1);
    assert_eq!(golem[0].fields.volume, Some(1));
    let publisher = db.lookup(golem[0].fields.publisher.unwrap()).unwrap();
    assert_eq!(publisher.name, "Orbit Line");
    // The issue the omnibus named is the one the copy went to.
    assert_eq!(db.issues_in_series(golem[0].id).unwrap().len(), 2);
}
