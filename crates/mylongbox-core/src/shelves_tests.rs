//! Tests of saved shelves and the sidebar's hidden entries.

use crate::filter::*;
use crate::model::*;
use crate::{
    Amount, BookEntry, Currency, Database, Date, Error, IssueNumber, MAX_SHELF_NAME, SeriesChoice,
};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn set(rules: Vec<Filter>) -> FilterSet {
    FilterSet {
        rules,
        owned_as: OwnedAs::SingleIssue,
    }
}

fn signed() -> FilterSet {
    set(vec![Filter::Flag {
        flag: Flag::Signed,
        value: true,
    }])
}

fn names(db: &Database) -> Vec<String> {
    db.saved_shelves()
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect()
}

/// A book in `series` with one writer and one character.
fn book(db: &Database, series: &str, number: &str, writer: &str, character: &str) -> CopyId {
    let mut e = BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: series.into(),
            start_year: None,
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new(number).unwrap()),
    );
    e.credits = vec![(writer.into(), Role::Writer)];
    e.characters = vec![(character.into(), false)];
    db.add_book(&e).unwrap().copy
}

/// The first rule of the shelf's filters.
fn first_rule(db: &Database, shelf: SavedShelfId) -> Filter {
    db.saved_shelf(shelf).unwrap().filters.unwrap().rules[0].clone()
}

#[test]
fn shelves_are_saved_named_and_ordered() {
    let db = db();
    let graded = set(vec![Filter::Choice(Choice::GradeType(vec![
        GradeType::Cgc,
        GradeType::Cbcs,
        GradeType::Pgx,
    ]))]);
    let a = db.add_saved_shelf("  Graded   books ", &graded).unwrap();
    let b = db.add_saved_shelf("Signed", &signed()).unwrap();
    let c = db.add_saved_shelf("Keys", &FilterSet::default()).unwrap();
    assert_eq!(names(&db), ["Graded books", "Signed", "Keys"]);
    assert_eq!(db.saved_shelf(a).unwrap().filters, Some(graded));

    // Names are unique, ignoring case, and never blank or too long.
    assert!(matches!(
        db.add_saved_shelf("signed", &signed()),
        Err(Error::NameTaken { .. })
    ));
    assert!(matches!(
        db.rename_saved_shelf(c, "SIGNED"),
        Err(Error::NameTaken { .. })
    ));
    assert!(db.add_saved_shelf("   ", &signed()).is_err());
    assert!(
        db.add_saved_shelf(&"x".repeat(MAX_SHELF_NAME + 1), &signed())
            .is_err()
    );
    // A shelf may change only the case of its own name.
    db.rename_saved_shelf(b, "SIGNED").unwrap();
    db.rename_saved_shelf(b, "Signed copies").unwrap();

    db.move_saved_shelf(c, true).unwrap();
    assert_eq!(names(&db), ["Graded books", "Keys", "Signed copies"]);
    db.move_saved_shelf(a, true).unwrap();
    db.move_saved_shelf(b, false).unwrap();
    assert_eq!(
        names(&db),
        ["Graded books", "Keys", "Signed copies"],
        "the ends stay put"
    );
    db.move_saved_shelf(a, false).unwrap();
    assert_eq!(names(&db), ["Keys", "Graded books", "Signed copies"]);

    db.set_saved_shelf_filters(c, &signed()).unwrap();
    assert_eq!(db.saved_shelf(c).unwrap().filters, Some(signed()));
    db.delete_saved_shelf(a).unwrap();
    assert_eq!(names(&db), ["Keys", "Signed copies"]);
    // A new shelf goes last, after a gap left by the one removed.
    db.add_saved_shelf("New", &signed()).unwrap();
    assert_eq!(names(&db), ["Keys", "Signed copies", "New"]);
    assert!(matches!(db.delete_saved_shelf(a), Err(Error::NotFound)));
    assert!(matches!(
        db.rename_saved_shelf(a, "Gone"),
        Err(Error::NotFound)
    ));
}

/// One rule of every kind, with every value type inside them.
fn every_rule() -> FilterSet {
    FilterSet {
        rules: vec![
            Filter::Text {
                field: TextField::KeyReason,
                condition: TextCondition::SimilarTo,
                value: "first \"Venom\"".into(),
            },
            Filter::Person {
                name: "Todd McFarlane".into(),
                role: Some(Role::Penciller),
            },
            Filter::List {
                field: ListField::SignedBy,
                condition: ListCondition::NoneOf,
                names: vec!["Stan Lee".into()],
            },
            Filter::IssueNumber(Compare::Between(
                IssueNumber::new("Annual 3").unwrap(),
                IssueNumber::new("½").unwrap(),
            )),
            Filter::Grade(Compare::AtLeast(Grade::parse("9.4").unwrap())),
            Filter::Number {
                field: NumberField::StartYear,
                compare: Compare::AtMost(1979),
            },
            Filter::Money {
                field: MoneyField::CurrentValue,
                compare: Compare::Equal(Amount::new(Currency::Btc, 1_500)),
            },
            Filter::Date {
                field: DateField::PurchaseDate,
                condition: DateCondition::Between(
                    "2024-01-31".parse::<Date>().unwrap(),
                    "2026-10-12".parse::<Date>().unwrap(),
                ),
            },
            Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::InYear(1988),
            },
            Filter::Choice(Choice::Verification(vec![Verification::CgcSignatureSeries])),
            Filter::Has {
                what: Presence::FrontPhoto,
                has: false,
            },
            Filter::Collects {
                series: SeriesId(7),
                from: Some(IssueNumber::new("1").unwrap()),
                to: None,
            },
        ],
        owned_as: OwnedAs::Either,
    }
}

/// The JSON a shelf of [`every_rule`] is saved as. Collections keep it,
/// so a change here breaks shelves people have saved: add new forms,
/// never change these.
const EVERY_RULE_JSON: &str = r#"{"rules":[{"text":{"field":"key_reason","condition":"similar_to","value":"first \"Venom\""}},{"person":{"name":"Todd McFarlane","role":"penciller"}},{"list":{"field":"signed_by","condition":"none_of","names":["Stan Lee"]}},{"issue_number":{"between":["Annual 3","½"]}},{"grade":{"at_least":"9.4"}},{"number":{"field":"start_year","compare":{"at_most":1979}}},{"money":{"field":"current_value","compare":{"equal":{"currency":"BTC","units":1500}}}},{"date":{"field":"purchase_date","condition":{"between":["2024-01-31","2026-10-12"]}}},{"date":{"field":"cover_date","condition":{"in_year":1988}}},{"choice":{"verification":["cgc_ss"]}},{"has":{"what":"front_photo","has":false}},{"collects":{"series":7,"from":"1","to":null}}],"owned_as":"either"}"#;

#[test]
fn rules_are_saved_in_a_form_that_never_changes() {
    let db = db();
    let shelf = db.add_saved_shelf("Everything", &every_rule()).unwrap();
    let stored: String = db
        .conn
        .query_row(
            "SELECT rules FROM saved_shelf WHERE id = ?1",
            [shelf],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, EVERY_RULE_JSON);
    assert_eq!(db.saved_shelf(shelf).unwrap().filters, Some(every_rule()));
}

#[test]
fn page_quality_rules_are_saved_by_their_codes() {
    let rules = set(vec![Filter::Choice(Choice::PageQuality(vec![
        PageQuality::White,
        PageQuality::OffWhiteToWhite,
    ]))]);
    let json = r#"{"rules":[{"choice":{"page_quality":["white","off_white_to_white"]}}],"owned_as":"single_issue"}"#;
    assert_eq!(serde_json::to_string(&rules).unwrap(), json);
    assert_eq!(serde_json::from_str::<FilterSet>(json).unwrap(), rules);
}

#[test]
fn rules_from_a_newer_version_are_kept_but_not_read() {
    let db = db();
    db.conn
        .execute(
            "INSERT INTO saved_shelf (name, position, rules) VALUES ('Future', 1, ?1)",
            [r#"{"rules":[{"moon_phase":"full"}],"owned_as":"either"}"#],
        )
        .unwrap();
    let shelf = db.saved_shelves().unwrap().remove(0);
    assert_eq!(shelf.filters, None);
    // Changes elsewhere leave it untouched, and it can still be managed.
    book(&db, "Thor", "1", "Walt Simonson", "Beta Ray Bill");
    let simonson = db
        .find_lookup::<CreatorId>("Walt Simonson")
        .unwrap()
        .unwrap();
    db.rename_lookup(simonson, "Walter Simonson").unwrap();
    db.rename_saved_shelf(shelf.id, "From the future").unwrap();
    let stored: String = db
        .conn
        .query_row("SELECT rules FROM saved_shelf", [], |row| row.get(0))
        .unwrap();
    assert!(stored.contains("moon_phase"));
    // Values that are not what they claim are refused rather than read.
    for bad in [
        r#"{"rules":[{"grade":{"at_least":"11.0"}}],"owned_as":"either"}"#,
        r#"{"rules":[{"grade":{"at_least":"99.9"}}],"owned_as":"either"}"#,
        r#"{"rules":[{"money":{"field":"cover_price","compare":{"equal":{"currency":"USD","units":-1}}}}],"owned_as":"either"}"#,
        r#"{"rules":[{"choice":{"grade_type":["slabbed"]}}],"owned_as":"either"}"#,
        r#"{"rules":[{"issue_number":{"equal":""}}],"owned_as":"either"}"#,
    ] {
        assert!(serde_json::from_str::<FilterSet>(bad).is_err(), "{bad}");
    }
}

#[test]
fn rules_follow_renames_and_merges() {
    let db = db();
    book(&db, "Thor", "337", "Walt Simonson", "Beta Ray Bill");
    book(&db, "Thor", "338", "Walter Simonson", "Bill");
    let by_name = db
        .add_saved_shelf(
            "Simonson",
            &set(vec![Filter::Person {
                name: "walt simonson".into(),
                role: None,
            }]),
        )
        .unwrap();
    let by_list = db
        .add_saved_shelf(
            "Bill",
            &set(vec![Filter::List {
                field: ListField::Characters,
                condition: ListCondition::AnyOf,
                names: vec!["Bill".into(), "Thor".into()],
            }]),
        )
        .unwrap();

    let walt = db
        .find_lookup::<CreatorId>("Walt Simonson")
        .unwrap()
        .unwrap();
    db.rename_lookup(walt, "W. Simonson").unwrap();
    assert_eq!(
        first_rule(&db, by_name),
        Filter::Person {
            name: "W. Simonson".into(),
            role: None
        }
    );
    // A character merged away is now meant by the one it went into; a
    // creator of the same name is a different thing and is left alone.
    let bill = db.find_lookup::<CharacterId>("Bill").unwrap().unwrap();
    let beta = db
        .find_lookup::<CharacterId>("Beta Ray Bill")
        .unwrap()
        .unwrap();
    db.merge_lookups(bill, beta).unwrap();
    assert_eq!(
        first_rule(&db, by_list),
        Filter::List {
            field: ListField::Characters,
            condition: ListCondition::AnyOf,
            names: vec!["Beta Ray Bill".into(), "Thor".into()],
        }
    );
    assert_eq!(
        first_rule(&db, by_name),
        Filter::Person {
            name: "W. Simonson".into(),
            role: None
        }
    );
}

#[test]
fn series_rules_follow_a_series_when_it_is_the_only_one_so_named() {
    let db = db();
    book(&db, "Thor", "1", "Stan Lee", "Thor");
    book(&db, "Mighty Thor", "1", "Stan Lee", "Thor");
    let series = |name: &str| {
        db.list_series()
            .unwrap()
            .into_iter()
            .find(|s| s.fields.name == name)
            .unwrap()
    };
    let (thor, mighty) = (series("Thor"), series("Mighty Thor"));
    let named = db
        .add_saved_shelf(
            "Thor",
            &set(vec![Filter::Text {
                field: TextField::SeriesName,
                condition: TextCondition::Is,
                value: "Thor".into(),
            }]),
        )
        .unwrap();
    let reprinted = db
        .add_saved_shelf(
            "Thor collected",
            &set(vec![Filter::Collects {
                series: thor.id,
                from: None,
                to: None,
            }]),
        )
        .unwrap();
    let mut fields = thor.fields.clone();
    fields.name = "The Mighty Thor (1966)".into();
    db.update_series(thor.id, &fields).unwrap();
    let Filter::Text { value, .. } = first_rule(&db, named) else {
        panic!("still a text rule");
    };
    assert_eq!(value, "The Mighty Thor (1966)");

    db.merge_series(thor.id, mighty.id).unwrap();
    assert_eq!(
        first_rule(&db, reprinted),
        Filter::Collects {
            series: mighty.id,
            from: None,
            to: None
        }
    );
    let Filter::Text { value, .. } = first_rule(&db, named) else {
        panic!("still a text rule");
    };
    assert_eq!(value, "Mighty Thor");

    // With two series of one name, renaming one cannot say which a rule
    // meant, so the rule is left as it was.
    book(&db, "Hulk", "1", "Stan Lee", "Hulk");
    let mut fields = SeriesFields::new("Hulk", Category::Comic);
    fields.start_year = Some(2008);
    let other = db.create_series(&fields).unwrap();
    let hulk = db
        .add_saved_shelf(
            "Hulk",
            &set(vec![Filter::Text {
                field: TextField::SeriesName,
                condition: TextCondition::Is,
                value: "Hulk".into(),
            }]),
        )
        .unwrap();
    let mut fields = db.series(other).unwrap().fields;
    fields.name = "Incredible Hulk".into();
    db.update_series(other, &fields).unwrap();
    let Filter::Text { value, .. } = first_rule(&db, hulk) else {
        panic!("still a text rule");
    };
    assert_eq!(value, "Hulk");
}

#[test]
fn rules_say_what_they_mention_that_is_gone() {
    let db = db();
    book(&db, "Thor", "1", "Stan Lee", "Thor");
    let thor = db.list_series().unwrap()[0].id;
    let filters = set(vec![
        Filter::Person {
            name: "Stan Lee".into(),
            role: None,
        },
        Filter::Person {
            name: "Jack Kirby".into(),
            role: None,
        },
        Filter::List {
            field: ListField::Tags,
            condition: ListCondition::AnyOf,
            names: vec!["Keys".into(), "keys".into()],
        },
        Filter::Collects {
            series: thor,
            from: None,
            to: None,
        },
    ]);
    assert_eq!(db.missing_from(&filters).unwrap(), ["Jack Kirby", "Keys"]);
    db.trash_series(thor).unwrap();
    assert_eq!(
        db.missing_from(&filters).unwrap(),
        ["Jack Kirby", "Keys", "a series that is no longer listed"]
    );
}

#[test]
fn hidden_sections_are_kept_by_code() {
    let db = db();
    assert!(db.hidden_sections().unwrap().is_empty());
    db.set_section_hidden("teams", true).unwrap();
    db.set_section_hidden("magazines", true).unwrap();
    db.set_section_hidden("teams", true).unwrap();
    assert_eq!(db.hidden_sections().unwrap(), ["magazines", "teams"]);
    db.set_section_hidden("teams", false).unwrap();
    db.set_section_hidden("never-hidden", false).unwrap();
    assert_eq!(db.hidden_sections().unwrap(), ["magazines"]);
    assert!(db.set_section_hidden("", true).is_err());
}

#[test]
fn rules_follow_a_rename_in_every_list_of_names() {
    use crate::LookupId;
    /// The field a rule names an entry by, and how to rename one.
    type Kind = (Option<TextField>, Option<ListField>, fn(&Database));
    fn rename<I: LookupId>(db: &Database) {
        let id = db.create_lookup::<I>("Gold").unwrap();
        db.rename_lookup(id, "Gold 2").unwrap();
    }
    let rule = |field: Option<TextField>, list: Option<ListField>, name: &str| match (field, list) {
        (Some(field), _) => Filter::Text {
            field,
            condition: TextCondition::Is,
            value: name.into(),
        },
        (_, Some(field)) => Filter::List {
            field,
            condition: ListCondition::AnyOf,
            names: vec![name.into()],
        },
        _ => unreachable!(),
    };
    let db = db();
    // Every rule names "Gold", so a rename reaching the wrong kind of
    // rule shows up as a change to the wrong shelf.
    let kinds: [Kind; 5] = [
        (Some(TextField::Publisher), None, rename::<PublisherId>),
        (Some(TextField::StorageBox), None, rename::<BoxId>),
        (None, Some(ListField::Tags), rename::<TagId>),
        (None, Some(ListField::Teams), rename::<TeamId>),
        (None, Some(ListField::StoryArcs), rename::<StoryArcId>),
    ];
    let shelves: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(i, (field, list, _))| {
            db.add_saved_shelf(
                &format!("Shelf {i}"),
                &set(vec![rule(*field, *list, "Gold")]),
            )
            .unwrap()
        })
        .collect();
    for (done, (_, _, rename)) in kinds.iter().enumerate() {
        rename(&db);
        for (i, ((field, list, _), shelf)) in kinds.iter().zip(&shelves).enumerate() {
            let name = if i <= done { "Gold 2" } else { "Gold" };
            assert_eq!(first_rule(&db, *shelf), rule(*field, *list, name));
        }
    }
}
