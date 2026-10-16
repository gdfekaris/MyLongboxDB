//! Tests of the filter model: every field type, every condition, AND
//! combination, and how issue rules become books.

use std::collections::BTreeSet;

use crate::filter::similar;
use crate::model::*;
use crate::{
    Amount, BookEntry, Choice, Compare, Currency, Database, Date, DateCondition, DateField,
    ExchangeRate, Filter, FilterSet, Flag, IssueNumber, ListCondition, ListField, MoneyField,
    NumberField, OwnedAs, Presence, SeriesChoice, TextCondition, TextField,
};

/// Five books that between them use every field.
struct Sample {
    db: Database,
    /// Amazing Spider-Man #121: graded, signed, photographed, valued.
    a: CopyId,
    /// Amazing Spider-Man #122: a raw newsstand copy bought with gold.
    b: CopyId,
    /// Incredible Hulk #181: first Wolverine.
    c: CopyId,
    /// Savage Sword of Conan #1: a black-and-white magazine.
    d: CopyId,
    /// A Masterworks volume reprinting Amazing Spider-Man #119–123.
    e: CopyId,
}

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn usd(text: &str) -> Amount {
    Amount::parse(Currency::Usd, text).unwrap()
}

fn series(name: &str, year: u16, publisher: Option<&str>) -> SeriesChoice {
    SeriesChoice::New {
        name: name.into(),
        start_year: Some(year),
        volume: None,
        publisher: publisher.map(Into::into),
        imprint: String::new(),
        magazine_format: None,
        black_and_white: None,
    }
}

fn number(n: &str) -> Option<IssueNumber> {
    Some(IssueNumber::new(n).unwrap())
}

fn sample() -> Sample {
    let db = Database::open_in_memory().unwrap();
    for (day, ounce, bitcoin) in [
        ("2020-01-01", "1500", "7000"),
        ("2021-01-01", "1900", "30000"),
        ("2022-01-01", "2000", "45000"),
    ] {
        db.set_exchange_rate(ExchangeRate::new(date(day), usd(ounce), usd(bitcoin)).unwrap())
            .unwrap();
    }

    let mut e = BookEntry::new(
        Category::Comic,
        series("Amazing Spider-Man", 1963, Some("Marvel")),
        number("121"),
    );
    e.issue.title = "The Night Gwen Stacy Died".into();
    e.issue.cover_date = CoverDate::new(1973, Some(6));
    e.issue.release_date = Some(date("1973-03-13"));
    e.issue.page_count = Some(36);
    e.issue.legacy_number = Some(121);
    if let SeriesChoice::New { imprint, .. } = &mut e.series {
        *imprint = "Marvel Comics Group".into();
    }
    e.issue.is_key = true;
    e.issue.key_reason = "Death of Gwen Stacy".into();
    e.credits = vec![
        ("Gerry Conway".into(), Role::Writer),
        ("Gil Kane".into(), Role::Penciller),
    ];
    e.characters = vec![("Green Goblin".into(), false), ("Spider-Man".into(), false)];
    e.story_arcs = vec![("Death of Gwen".into(), Some(1))];
    e.copy.grade = Grade::parse("9.4");
    e.copy.grade_type = Some(GradeType::Cgc);
    e.copy.cert_number = Some("1234567001".into());
    e.copy.label_type = Some("Universal".into());
    e.copy.page_quality = Some(PageQuality::White);
    e.copy.purchase_price = Some(usd("1500.00"));
    e.copy.purchase_date = Some(date("2020-09-15"));
    e.copy.seller = "eBay".into();
    e.copy.box_position = Some(3);
    e.copy.notes = "Pressed; 100% original".into();
    e.storage_box = Some("Box 1".into());
    e.tags = vec!["Key".into()];
    let mut signed = SignatureFields::new(Verification::CgcSignatureSeries);
    signed.placement = Some(SignaturePlacement::FrontCover);
    e.signatures = vec![("Gerry Conway".into(), signed)];
    let a = db.add_book(&e).unwrap().copy;
    db.add_value(a, date("2023-01-01"), usd("2000"), "GoCollect")
        .unwrap();
    db.set_photo(
        a,
        Side::Front,
        &crate::CoverPhoto {
            full_jpeg: vec![0xFF, 0xD8, 0xFF, 1],
            thumb_jpeg: vec![0xFF, 0xD8, 0xFF, 2],
            width: 10,
            height: 15,
            sha256: [0; 32],
        },
    )
    .unwrap();

    let asm = db.find_series_named("Amazing Spider-Man");
    let mut e = BookEntry::new(Category::Comic, SeriesChoice::Existing(asm), number("122"));
    e.issue.cover_date = CoverDate::new(1973, Some(7));
    e.credits = vec![
        ("Gerry Conway".into(), Role::Writer),
        ("Gil Kane".into(), Role::Penciller),
    ];
    e.edition.variant = "Newsstand".into();
    e.edition.distribution = Some(Distribution::Newsstand);
    e.copy.grade = Grade::parse("8.0");
    e.copy.grade_type = Some(GradeType::Raw);
    // A twentieth of an ounce, worth $95 at the 2021 rate.
    e.copy.purchase_price = Some(Amount::parse(Currency::Gold, "0.05").unwrap());
    e.copy.purchase_date = Some(date("2021-01-01"));
    let b = db.add_book(&e).unwrap().copy;

    let mut e = BookEntry::new(
        Category::Comic,
        series("Incredible Hulk", 1968, Some("Marvel")),
        number("181"),
    );
    e.issue.cover_date = CoverDate::new(1974, Some(11));
    e.issue.is_key = true;
    e.issue.key_reason = "First full Wolverine".into();
    e.credits = vec![
        ("Len Wein".into(), Role::Writer),
        ("Herb Trimpe".into(), Role::Penciller),
    ];
    e.characters = vec![("Wolverine".into(), true), ("Hulk".into(), false)];
    e.cover_artists = vec!["Herb Trimpe".into()];
    e.copy.grade = Grade::parse("6.0");
    e.copy.grade_type = Some(GradeType::Cbcs);
    e.copy.page_quality = Some(PageQuality::CreamToOffWhite);
    let c = db.add_book(&e).unwrap().copy;

    let mut e = BookEntry::new(
        Category::Magazine,
        SeriesChoice::New {
            name: "Savage Sword of Conan".into(),
            start_year: Some(1974),
            volume: None,
            publisher: Some("Curtis".into()),
            imprint: String::new(),
            magazine_format: Some(MagazineFormat::Standard),
            black_and_white: Some(true),
        },
        number("1"),
    );
    e.issue.cover_date = CoverDate::new(1974, None);
    e.teams = vec![("Barbarians".into(), false)];
    let d = db.add_book(&e).unwrap().copy;

    let mut e = BookEntry::new(
        Category::CollectedEdition,
        series(
            "Marvel Masterworks: Amazing Spider-Man",
            2003,
            Some("Marvel"),
        ),
        number("12"),
    );
    e.issue.page_count = Some(300);
    e.collects = "Amazing Spider-Man (1963) 119-123".into();
    e.edition.binding = Some(Binding::Hardcover);
    e.edition.has_dust_jacket = Some(true);
    e.edition.isbn = Isbn::parse("9780785111818");
    e.edition.cover_price = Some(usd("54.99"));
    e.copy.book_condition = Some(BookCondition::LikeNew);
    e.copy.dust_jacket_condition = Some(BookCondition::VeryGood);
    let e = db.add_book(&e).unwrap().copy;

    Sample { db, a, b, c, d, e }
}

impl Database {
    fn find_series_named(&self, name: &str) -> SeriesId {
        self.list_series()
            .unwrap()
            .into_iter()
            .find(|s| s.fields.name == name)
            .unwrap()
            .id
    }
}

impl Sample {
    /// The books meeting all of `rules`, as single issues.
    fn find(&self, rules: Vec<Filter>) -> BTreeSet<CopyId> {
        self.find_owned(rules, OwnedAs::SingleIssue)
    }

    fn find_owned(&self, rules: Vec<Filter>, owned_as: OwnedAs) -> BTreeSet<CopyId> {
        self.db
            .filter_books(&FilterSet { rules, owned_as })
            .unwrap()
            .into_iter()
            .map(|r| r.copy)
            .collect()
    }

    fn all(&self) -> BTreeSet<CopyId> {
        [self.a, self.b, self.c, self.d, self.e].into()
    }
}

fn set(copies: &[CopyId]) -> BTreeSet<CopyId> {
    copies.iter().copied().collect()
}

fn text(field: TextField, condition: TextCondition, value: &str) -> Filter {
    Filter::Text {
        field,
        condition,
        value: value.into(),
    }
}

#[test]
fn no_rules_leave_every_book() {
    let s = sample();
    assert_eq!(s.find(vec![]), s.all());
    let rows = s.db.filter_books(&FilterSet::default()).unwrap();
    assert_eq!(rows.len(), 5, "in natural order, once each");
}

#[test]
fn text_fields_contain_are_and_resemble() {
    use TextCondition::*;
    use TextField::*;
    let s = sample();
    for (rule, wanted) in [
        (text(SeriesName, Contains, "spider"), set(&[s.a, s.b, s.e])),
        (text(SeriesName, Is, " incredible HULK "), set(&[s.c])),
        (
            text(SeriesName, SimilarTo, "Amazng Spider-Man"),
            set(&[s.a, s.b]),
        ),
        (text(SeriesName, SimilarTo, "Savage Sword"), set(&[])),
        (text(Publisher, Is, "curtis"), set(&[s.d])),
        (text(Imprint, Contains, "comics group"), set(&[s.a, s.b])),
        (text(Title, Contains, "gwen"), set(&[s.a])),
        (text(KeyReason, Contains, "wolverine"), set(&[s.c])),
        (text(Variant, Is, "newsstand"), set(&[s.b])),
        (text(Isbn, Contains, "0785111"), set(&[s.e])),
        (text(CertNumber, Is, "1234567001"), set(&[s.a])),
        (text(LabelType, SimilarTo, "Univeral"), set(&[s.a])),
        (text(Seller, Contains, "EBAY"), set(&[s.a])),
        (text(StorageBox, Is, "box 1"), set(&[s.a])),
        (text(Notes, Contains, "100%"), set(&[s.a])),
        // Wildcards are taken literally.
        (text(Notes, Contains, "_"), set(&[])),
        (text(Barcode, Contains, "7"), set(&[])),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn people_are_found_by_name_or_alternate_name_and_role() {
    let s = sample();
    let person = |name: &str, role| Filter::Person {
        name: name.into(),
        role,
    };
    assert_eq!(s.find(vec![person("gil kane", None)]), set(&[s.a, s.b]));
    assert_eq!(
        s.find(vec![person("Gerry Conway", Some(Role::Writer))]),
        set(&[s.a, s.b])
    );
    assert_eq!(
        s.find(vec![person("Gil Kane", Some(Role::Writer))]),
        set(&[])
    );
    assert_eq!(s.find(vec![person("Nobody", None)]), set(&[]));
    let kane = s.db.find_lookup::<CreatorId>("Gil Kane").unwrap().unwrap();
    s.db.add_alias(kane, "G. Kane").unwrap();
    assert_eq!(s.find(vec![person("G. Kane", None)]), set(&[s.a, s.b]));
}

#[test]
fn lists_include_any_all_or_none() {
    use ListCondition::*;
    use ListField::*;
    let s = sample();
    let list = |field, condition, names: &[&str]| Filter::List {
        field,
        condition,
        names: names.iter().map(|n| (*n).into()).collect(),
    };
    for (rule, wanted) in [
        (
            list(Characters, AnyOf, &["Wolverine", "green goblin"]),
            set(&[s.a, s.c]),
        ),
        (list(Characters, AllOf, &["Wolverine", "Hulk"]), set(&[s.c])),
        (
            list(Characters, AllOf, &["Wolverine", "Green Goblin"]),
            set(&[]),
        ),
        (
            list(Characters, NoneOf, &["Wolverine"]),
            set(&[s.a, s.b, s.d, s.e]),
        ),
        (list(Characters, AnyOf, &["Nobody"]), set(&[])),
        (list(Characters, AllOf, &["Hulk", "Nobody"]), set(&[])),
        (list(Characters, NoneOf, &["Nobody"]), s.all()),
        (list(Characters, AllOf, &[]), s.all()),
        (list(Characters, AnyOf, &[]), set(&[])),
        (list(Teams, AnyOf, &["Barbarians"]), set(&[s.d])),
        (list(StoryArcs, AnyOf, &["Death of Gwen"]), set(&[s.a])),
        (list(Tags, AnyOf, &["key"]), set(&[s.a])),
        (list(Tags, NoneOf, &["Key"]), set(&[s.b, s.c, s.d, s.e])),
        (list(CoverArtists, AnyOf, &["Herb Trimpe"]), set(&[s.c])),
        (list(SignedBy, AllOf, &["Gerry Conway"]), set(&[s.a])),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn numbers_grades_and_issue_numbers_compare() {
    let s = sample();
    let n = |t: &str| IssueNumber::new(t).unwrap();
    let g = |t: &str| Grade::parse(t).unwrap();
    for (rule, wanted) in [
        (Filter::IssueNumber(Compare::Equal(n("121"))), set(&[s.a])),
        (
            Filter::IssueNumber(Compare::AtLeast(n("122"))),
            set(&[s.b, s.c]),
        ),
        (
            Filter::IssueNumber(Compare::Between(n("122"), n("12"))),
            set(&[s.b, s.a, s.e]),
        ),
        (Filter::IssueNumber(Compare::AtMost(n("1"))), set(&[s.d])),
        (Filter::Grade(Compare::AtLeast(g("9.0"))), set(&[s.a])),
        (
            Filter::Grade(Compare::Between(g("8.0"), g("6.0"))),
            set(&[s.b, s.c]),
        ),
        (Filter::Grade(Compare::Equal(g("8.0"))), set(&[s.b])),
        (
            Filter::Number {
                field: NumberField::PageCount,
                compare: Compare::AtLeast(100),
            },
            set(&[s.e]),
        ),
        (
            Filter::Number {
                field: NumberField::LegacyNumber,
                compare: Compare::AtLeast(100),
            },
            set(&[s.a]),
        ),
        (
            Filter::Number {
                field: NumberField::StartYear,
                compare: Compare::AtMost(1968),
            },
            set(&[s.a, s.b, s.c]),
        ),
        (
            Filter::Number {
                field: NumberField::BoxPosition,
                compare: Compare::Equal(3),
            },
            set(&[s.a]),
        ),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn money_compares_across_currencies_at_the_rate_of_its_day() {
    let s = sample();
    let money = |field, compare| Filter::Money { field, compare };
    use MoneyField::*;
    let gold = |t: &str| Amount::parse(Currency::Gold, t).unwrap();
    for (rule, wanted) in [
        (
            money(PurchasePrice, Compare::AtLeast(usd("1000"))),
            set(&[s.a]),
        ),
        // B's twentieth of an ounce was $95 on the day it was bought.
        (
            money(PurchasePrice, Compare::AtMost(usd("100"))),
            set(&[s.b]),
        ),
        (money(PurchasePrice, Compare::Equal(usd("95"))), set(&[s.b])),
        // At today's rate it would be $100; the rate of its day counts.
        (
            money(PurchasePrice, Compare::AtMost(usd("99"))),
            set(&[s.b]),
        ),
        (
            money(PurchasePrice, Compare::AtLeast(usd("1500"))),
            set(&[s.a]),
        ),
        // A's $1,500 was an ounce at the 2020 rate.
        (
            money(PurchasePrice, Compare::AtLeast(gold("0.5"))),
            set(&[s.a]),
        ),
        (
            money(CurrentValue, Compare::AtLeast(usd("1500"))),
            set(&[s.a]),
        ),
        (money(CurrentValue, Compare::AtMost(usd("10"))), set(&[])),
        (
            money(CoverPrice, Compare::Between(usd("60"), usd("50"))),
            set(&[s.e]),
        ),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn amounts_that_cannot_be_converted_do_not_match() {
    let db = Database::open_in_memory().unwrap();
    let mut e = BookEntry::new(Category::Comic, series("Zot!", 1984, None), number("1"));
    e.copy.purchase_price = Some(Amount::parse(Currency::Btc, "0.001").unwrap());
    e.copy.purchase_date = Some(date("2015-01-01"));
    db.add_book(&e).unwrap();
    let rule = Filter::Money {
        field: MoneyField::PurchasePrice,
        compare: Compare::AtLeast(usd("0")),
    };
    let found = db
        .filter_books(&FilterSet {
            rules: vec![rule],
            owned_as: OwnedAs::SingleIssue,
        })
        .unwrap();
    assert!(found.is_empty(), "no rate to convert it at");
}

#[test]
fn dates_compare_and_cover_dates_by_month() {
    let s = sample();
    let on = |field, condition| Filter::Date { field, condition };
    use DateCondition::*;
    use DateField::*;
    for (rule, wanted) in [
        (
            on(CoverDate, Between(date("1973-12-31"), date("1973-01-01"))),
            set(&[s.a, s.b]),
        ),
        // Conan is dated only 1974, which counts as all of it.
        (on(CoverDate, InYear(1974)), set(&[s.c, s.d])),
        (on(CoverDate, Before(date("1974-01-15"))), set(&[s.a, s.b])),
        (on(CoverDate, After(date("1974-06-01"))), set(&[s.c])),
        (
            on(CoverDate, Between(date("1974-03-01"), date("1974-12-01"))),
            set(&[s.c]),
        ),
        (on(ReleaseDate, Before(date("1974-01-01"))), set(&[s.a])),
        (on(PurchaseDate, InYear(2021)), set(&[s.b])),
        (on(PurchaseDate, After(date("2020-10-01"))), set(&[s.b])),
        (on(PurchaseDate, InYear(2020)), set(&[s.a])),
        // Conan, dated only 1974, is not wholly before June 1974, nor
        // wholly within a range ending then.
        (on(CoverDate, Before(date("1974-06-01"))), set(&[s.a, s.b])),
        (
            on(CoverDate, Between(date("1973-01-01"), date("1974-06-30"))),
            set(&[s.a, s.b]),
        ),
        (on(Added, After(date("2000-01-01"))), s.all()),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn choices_flags_and_presence() {
    let s = sample();
    let flag = |flag, value| Filter::Flag { flag, value };
    let has = |what, has| Filter::Has { what, has };
    for (rule, wanted) in [
        (
            Filter::Choice(Choice::Category(vec![Category::Magazine])),
            set(&[s.d]),
        ),
        (
            Filter::Choice(Choice::Category(vec![
                Category::Comic,
                Category::CollectedEdition,
            ])),
            set(&[s.a, s.b, s.c, s.e]),
        ),
        (
            Filter::Choice(Choice::GradeType(vec![GradeType::Cgc, GradeType::Cbcs])),
            set(&[s.a, s.c]),
        ),
        (Filter::Choice(Choice::GradeType(vec![])), set(&[])),
        (
            Filter::Choice(Choice::PageQuality(vec![PageQuality::White])),
            set(&[s.a]),
        ),
        (
            Filter::Choice(Choice::SignaturePlacement(vec![
                SignaturePlacement::FrontCover,
                SignaturePlacement::FirstPage,
            ])),
            set(&[s.a]),
        ),
        (
            Filter::Choice(Choice::SignaturePlacement(vec![
                SignaturePlacement::BackCover,
            ])),
            set(&[]),
        ),
        (
            Filter::Choice(Choice::PageQuality(vec![
                PageQuality::White,
                PageQuality::CreamToOffWhite,
                PageQuality::Brown,
            ])),
            set(&[s.a, s.c]),
        ),
        (
            Filter::Choice(Choice::MagazineFormat(vec![MagazineFormat::Standard])),
            set(&[s.d]),
        ),
        (
            Filter::Choice(Choice::Distribution(vec![Distribution::Newsstand])),
            set(&[s.b]),
        ),
        (
            Filter::Choice(Choice::Binding(vec![Binding::Hardcover])),
            set(&[s.e]),
        ),
        (
            Filter::Choice(Choice::BookCondition(vec![BookCondition::LikeNew])),
            set(&[s.e]),
        ),
        (
            Filter::Choice(Choice::DustJacketCondition(vec![BookCondition::VeryGood])),
            set(&[s.e]),
        ),
        (
            Filter::Choice(Choice::Verification(vec![Verification::CgcSignatureSeries])),
            set(&[s.a]),
        ),
        (flag(Flag::KeyIssue, true), set(&[s.a, s.c])),
        (flag(Flag::KeyIssue, false), set(&[s.b, s.d, s.e])),
        (flag(Flag::FirstAppearance, true), set(&[s.c])),
        (
            flag(Flag::FirstAppearance, false),
            set(&[s.a, s.b, s.d, s.e]),
        ),
        (flag(Flag::BlackAndWhite, true), set(&[s.d])),
        (flag(Flag::BlackAndWhite, false), set(&[])),
        (flag(Flag::DustJacket, true), set(&[s.e])),
        (flag(Flag::Slipcase, true), set(&[])),
        (flag(Flag::Signed, true), set(&[s.a])),
        (flag(Flag::Signed, false), set(&[s.b, s.c, s.d, s.e])),
        (flag(Flag::Sketch, false), s.all()),
        (has(Presence::FrontPhoto, true), set(&[s.a])),
        (has(Presence::BackPhoto, false), s.all()),
        (has(Presence::Value, true), set(&[s.a])),
        (has(Presence::Grade, false), set(&[s.d])),
        (has(Presence::StorageBox, true), set(&[s.a])),
        (has(Presence::PurchasePrice, true), set(&[s.a, s.b])),
        (has(Presence::PurchaseDate, false), set(&[s.c, s.d, s.e])),
        (has(Presence::CoverPrice, true), set(&[s.e])),
        (has(Presence::CoverDate, false), set(&[s.e])),
        (has(Presence::ReleaseDate, true), set(&[s.a])),
        (has(Presence::Tags, true), set(&[s.a])),
        (has(Presence::Notes, false), set(&[s.b, s.c, s.d, s.e])),
    ] {
        assert_eq!(s.find(vec![rule.clone()]), wanted, "{rule:?}");
    }
}

#[test]
fn books_that_collect_given_issues() {
    let s = sample();
    let asm = s.db.find_series_named("Amazing Spider-Man");
    let n = |t: &str| Some(IssueNumber::new(t).unwrap());
    let collects = |from, to| Filter::Collects {
        series: asm,
        from,
        to,
    };
    assert_eq!(s.find(vec![collects(None, None)]), set(&[s.e]));
    assert_eq!(s.find(vec![collects(n("121"), n("120"))]), set(&[s.e]));
    assert_eq!(s.find(vec![collects(n("123"), None)]), set(&[s.e]));
    assert_eq!(s.find(vec![collects(None, n("118"))]), set(&[]));
    assert_eq!(s.find(vec![collects(n("200"), n("210"))]), set(&[]));
}

#[test]
fn owned_as_turns_issues_into_single_copies_or_reprints() {
    let s = sample();
    let asm_121 = || {
        vec![
            text(
                TextField::SeriesName,
                TextCondition::Is,
                "Amazing Spider-Man",
            ),
            Filter::IssueNumber(Compare::Equal(IssueNumber::new("121").unwrap())),
        ]
    };
    assert_eq!(s.find_owned(asm_121(), OwnedAs::SingleIssue), set(&[s.a]));
    assert_eq!(
        s.find_owned(asm_121(), OwnedAs::CollectedEdition),
        set(&[s.e])
    );
    assert_eq!(s.find_owned(asm_121(), OwnedAs::Either), set(&[s.a, s.e]));
    // Every issue rule must hold for the same reprinted issue.
    let hulk_121 = vec![
        text(TextField::SeriesName, TextCondition::Is, "Incredible Hulk"),
        Filter::IssueNumber(Compare::Equal(IssueNumber::new("121").unwrap())),
    ];
    assert_eq!(s.find_owned(hulk_121, OwnedAs::Either), set(&[]));
    // Books that reprint Gil Kane's work.
    let kane = vec![Filter::Person {
        name: "Gil Kane".into(),
        role: None,
    }];
    assert_eq!(s.find_owned(kane, OwnedAs::CollectedEdition), set(&[s.e]));
    // With no issue rules, only collected editions are owned as such.
    assert_eq!(s.find_owned(vec![], OwnedAs::CollectedEdition), set(&[s.e]));
    assert_eq!(s.find_owned(vec![], OwnedAs::Either), s.all());
}

#[test]
fn rules_combine_with_and_across_levels() {
    let s = sample();
    let wolverine_graded = vec![
        Filter::List {
            field: ListField::Characters,
            condition: ListCondition::AnyOf,
            names: vec!["Wolverine".into()],
        },
        Filter::Grade(Compare::AtLeast(Grade::parse("5.0").unwrap())),
    ];
    assert_eq!(s.find(wolverine_graded), set(&[s.c]));
    let contradiction = vec![
        Filter::Flag {
            flag: Flag::KeyIssue,
            value: true,
        },
        Filter::Flag {
            flag: Flag::KeyIssue,
            value: false,
        },
    ];
    assert_eq!(s.find(contradiction), set(&[]));
    let both_levels = vec![
        text(TextField::SeriesName, TextCondition::Contains, "spider"),
        Filter::Has {
            what: Presence::PurchasePrice,
            has: true,
        },
        Filter::Choice(Choice::GradeType(vec![GradeType::Raw])),
    ];
    assert_eq!(s.find(both_levels), set(&[s.b]));
}

#[test]
fn trash_is_left_out() {
    let s = sample();
    s.db.trash_copy(s.a).unwrap();
    let key = vec![Filter::Flag {
        flag: Flag::KeyIssue,
        value: true,
    }];
    assert_eq!(s.find(key), set(&[s.c]));
    assert_eq!(s.find(vec![]).len(), 4);
}

#[test]
fn similar_spellings() {
    assert!(similar("Spiderman", "Spider-Man"));
    assert!(similar("amazng spider-man", "Amazing Spider-Man"));
    assert!(similar("Pokemon", "Pokémon"));
    assert!(similar(" Hulk ", "hulk"));
    assert!(similar("Thro", "Thor"));
    assert!(
        similar("Wolevrnie", "Wolverine"),
        "two edits in nine letters"
    );
    assert!(!similar("Wolevrnei", "Wolverine"), "but not three");
    assert!(!similar("Hulk", "Thor"));
    assert!(!similar("Spider-Man", "Spider-Man 2099"));
    assert!(!similar("Spider-Man", "Spider-Woman: Origin"));
    assert!(!similar("Ant", "Art"), "short words must match exactly");
    assert!(!similar("", ""));
}

#[test]
fn books_without_a_number_never_compare() {
    let db = Database::open_in_memory().unwrap();
    let standalone = BookEntry::new(
        Category::CollectedEdition,
        series("Watchmen", 1987, None),
        None,
    );
    db.add_book(&standalone).unwrap();
    let rule = Filter::IssueNumber(Compare::AtMost(IssueNumber::new("5").unwrap()));
    let found = db
        .filter_books(&FilterSet {
            rules: vec![rule],
            owned_as: OwnedAs::SingleIssue,
        })
        .unwrap();
    assert!(found.is_empty());
}

/// The search target holds for filters too: under 100 ms at 10,000
/// copies. Run with `cargo test --release -- --ignored`.
#[test]
#[ignore = "slow in debug builds; run in release"]
fn filtering_ten_thousand_copies_is_quick() {
    use crate::fixtures::{FixtureSpec, generate};
    let db = Database::open_in_memory().unwrap();
    generate(
        &db,
        FixtureSpec {
            copies: 10_000,
            seed: 42,
        },
    )
    .unwrap();
    let year = |y| Filter::Date {
        field: DateField::CoverDate,
        condition: DateCondition::InYear(y),
    };
    let cases: Vec<(Vec<Filter>, OwnedAs)> = vec![
        (vec![], OwnedAs::SingleIssue),
        (
            vec![text(TextField::SeriesName, TextCondition::Contains, "a")],
            OwnedAs::SingleIssue,
        ),
        (
            vec![text(
                TextField::SeriesName,
                TextCondition::SimilarTo,
                "Astonshing",
            )],
            OwnedAs::Either,
        ),
        (
            vec![Filter::Grade(Compare::AtLeast(
                Grade::parse("9.0").unwrap(),
            ))],
            OwnedAs::SingleIssue,
        ),
        (
            vec![Filter::Money {
                field: MoneyField::CurrentValue,
                compare: Compare::AtLeast(usd("50")),
            }],
            OwnedAs::SingleIssue,
        ),
        (
            vec![Filter::Money {
                field: MoneyField::PurchasePrice,
                compare: Compare::AtMost(usd("20")),
            }],
            OwnedAs::SingleIssue,
        ),
        (
            vec![
                year(1975),
                Filter::Has {
                    what: Presence::Value,
                    has: false,
                },
            ],
            OwnedAs::Either,
        ),
        (
            vec![Filter::Flag {
                flag: Flag::KeyIssue,
                value: false,
            }],
            OwnedAs::CollectedEdition,
        ),
        (
            vec![Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::Between(date("1960-01-01"), date("1990-12-31")),
            }],
            OwnedAs::Either,
        ),
    ];
    for (rules, owned_as) in cases {
        let start = std::time::Instant::now();
        let rows = db
            .filter_books(&FilterSet {
                rules: rules.clone(),
                owned_as,
            })
            .unwrap();
        let took = start.elapsed();
        assert!(
            took.as_millis() < 100,
            "{rules:?} {owned_as:?} took {took:?} for {} books",
            rows.len()
        );
    }
}
