//! Tests of quick search and the index behind it.

use crate::model::*;
use crate::search::{edits_allowed, fold, quality};
use crate::{BookEntry, Database, IssueNumber, SeriesChoice, Shelf};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn entry(series: &str, year: u16, number: &str) -> BookEntry {
    BookEntry::new(
        Category::Comic,
        SeriesChoice::New {
            name: series.into(),
            start_year: Some(year),
            volume: None,
            publisher: None,
            imprint: String::new(),
            magazine_format: None,
            black_and_white: None,
        },
        Some(IssueNumber::new(number).unwrap()),
    )
}

fn add(db: &Database, e: &BookEntry) -> CopyId {
    db.add_book(e).unwrap().copy
}

/// The copies a query finds, in order.
fn found(db: &Database, query: &str) -> Vec<CopyId> {
    db.quick_search(query)
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.copy)
        .collect()
}

/// The correction offered for a query, if any.
fn suggested(db: &Database, query: &str) -> Option<String> {
    db.quick_search(query).unwrap().did_you_mean
}

/// A small collection where each book is findable by one kind of field.
struct Shelf8 {
    db: Database,
    spider: CopyId,
    hulk: CopyId,
    fantastic: CopyId,
}

fn collection() -> Shelf8 {
    let db = db();
    let mut e = entry("Amazing Spider-Man", 1963, "121");
    e.issue.title = "The Night Gwen Stacy Died".into();
    e.credits = vec![("Gerry Conway".into(), Role::Writer)];
    e.characters = vec![("Green Goblin".into(), false)];
    e.story_arcs = vec![("Death of Gwen".into(), Some(1))];
    e.tags = vec!["Key".into()];
    e.storage_box = Some("Attic Shelf".into());
    let spider = add(&db, &e);

    let mut e = entry("Incredible Hulk", 1968, "181");
    e.series = SeriesChoice::New {
        name: "Incredible Hulk".into(),
        start_year: Some(1968),
        volume: None,
        publisher: Some("Marvel".into()),
        imprint: String::new(),
        magazine_format: None,
        black_and_white: None,
    };
    e.characters = vec![("Wolverine".into(), true)];
    e.copy.notes = "Slight spine roll; bought at a convention".into();
    e.cover_artists = vec!["Herb Trimpe".into()];
    let hulk = add(&db, &e);

    let mut e = entry("Fantastic Four", 1961, "48");
    e.credits = vec![("Jack Kirby".into(), Role::Penciller)];
    e.teams = vec![("Inhumans".into(), true)];
    e.storage_box = Some("Long Box B".into());
    let fantastic = add(&db, &e);
    Shelf8 {
        db,
        spider,
        hulk,
        fantastic,
    }
}

#[test]
fn every_kind_of_field_is_searched() {
    let c = collection();
    for (query, copy) in [
        ("spider-man", c.spider),
        ("gwen stacy died", c.spider),
        ("conway", c.spider),
        ("goblin", c.spider),
        ("death of gwen", c.spider),
        ("key", c.spider),
        ("attic", c.spider),
        ("marvel", c.hulk),
        ("wolverine", c.hulk),
        ("spine roll", c.hulk),
        ("trimpe", c.hulk),
        ("kirby", c.fantastic),
        ("inhumans", c.fantastic),
    ] {
        assert_eq!(found(&c.db, query), [copy], "{query}");
    }
    assert!(found(&c.db, "nothing like this").is_empty());
    assert!(found(&c.db, "   ").is_empty());
}

#[test]
fn case_accents_and_word_parts_do_not_matter() {
    let db = db();
    let mut e = entry("Pokémon Adventures", 2000, "1");
    e.credits = vec![("José Muñoz".into(), Role::Penciller)];
    let poke = add(&db, &e);
    let x = add(&db, &entry("X-Men", 1963, "1"));
    assert_eq!(found(&db, "POKEMON"), [poke]);
    assert_eq!(found(&db, "pokémon"), [poke]);
    assert_eq!(found(&db, "jose munoz"), [poke]);
    assert_eq!(found(&db, "kémo"), [poke]);
    assert_eq!(found(&db, "ADVENT"), [poke]);
    // Words shorter than three letters are looked for too, at the start
    // of a word.
    assert_eq!(found(&db, "x"), [x]);
    assert_eq!(found(&db, "x-m"), [x]);
    assert_eq!(found(&db, "Po"), [poke]);
    assert_eq!(found(&db, "mu"), [poke]);
    assert!(found(&db, "ok").is_empty(), "not inside a word");
    assert_eq!(found(&db, "oke"), [poke], "three letters may be anywhere");
}

#[test]
fn short_words_ignore_case_and_accents_like_longer_ones() {
    let db = db();
    let mut e = entry("Élan", 1990, "1");
    e.credits = vec![("Ágnes Öst".into(), Role::Writer)];
    let elan = add(&db, &e);
    let mut e = entry("Ωmega", 1991, "1");
    e.credits = vec![("Émile".into(), Role::Writer)];
    let omega = add(&db, &e);
    assert_eq!(found(&db, "el"), [elan]);
    assert_eq!(found(&db, "ÉL"), [elan]);
    assert_eq!(found(&db, "ag"), [elan]);
    assert_eq!(found(&db, "os"), [elan]);
    assert_eq!(found(&db, "ωm"), [omega], "case beyond A to Z");
    assert_eq!(found(&db, "em"), [omega]);
}

#[test]
fn every_word_must_be_found_somewhere() {
    let c = collection();
    assert_eq!(found(&c.db, "spider conway"), [c.spider]);
    assert_eq!(found(&c.db, "goblin attic"), [c.spider]);
    assert!(found(&c.db, "spider kirby").is_empty());
    assert_eq!(found(&c.db, "long box"), [c.fantastic]);
    // A short word may begin a word in a different field of each book.
    let mut every = found(&c.db, "c");
    every.sort();
    assert_eq!(every, [c.spider, c.hulk]);
}

#[test]
fn issue_numbers_are_found_as_whole_words() {
    let db = db();
    let three_hundred = add(&db, &entry("Amazing Spider-Man", 1963, "300"));
    let thirty = add(&db, &entry("Amazing Spider-Man", 1963, "30"));
    let three = add(&db, &entry("Amazing Spider-Man", 1963, "003"));
    let annual = add(&db, &entry("Amazing Spider-Man", 1963, "Annual 15"));
    let fifteen = add(&db, &entry("Amazing Spider-Man", 1963, "15"));
    let hulk = add(&db, &entry("Incredible Hulk", 1968, "300"));

    assert_eq!(found(&db, "amazing spider-man 300"), [three_hundred]);
    assert_eq!(found(&db, "spider-man #300"), [three_hundred]);
    assert_eq!(found(&db, "spider 30"), [thirty]);
    // Leading zeros are not part of the number, either way round.
    assert_eq!(found(&db, "spider 3"), [three]);
    assert_eq!(found(&db, "spider #0003"), [three]);
    let mut fifteens = found(&db, "spider 15");
    fifteens.sort();
    assert_eq!(fifteens, [annual, fifteen]);
    assert_eq!(found(&db, "spider annual"), [annual]);
    let mut every = found(&db, "300");
    every.sort();
    assert_eq!(every, [three_hundred, hulk]);
    assert!(found(&db, "spider 3000").is_empty());
    assert!(found(&db, "#").is_empty());
}

#[test]
fn imprints_find_their_series_books() {
    let db = db();
    let mut sandman = entry("Sandman", 1989, "47");
    if let SeriesChoice::New { imprint, .. } = &mut sandman.series {
        *imprint = "Vértigo".into();
    }
    let sandman = add(&db, &sandman);
    add(&db, &entry("Batman", 1940, "1"));
    assert_eq!(found(&db, "vertigo"), [sandman]);
    assert_eq!(found(&db, "VERT"), [sandman]);
    // Short words must begin a word of the imprint.
    assert_eq!(found(&db, "ve"), [sandman]);
    assert!(found(&db, "ti").is_empty());
}

#[test]
fn legacy_numbers_are_found_like_issue_numbers() {
    let db = db();
    let mut relaunched = entry("Amazing Spider-Man", 2018, "45");
    relaunched.issue.legacy_number = Some(846);
    let relaunched = add(&db, &relaunched);
    let original = add(&db, &entry("Amazing Spider-Man", 1963, "84"));
    assert_eq!(found(&db, "spider-man 846"), [relaunched]);
    assert_eq!(found(&db, "#846"), [relaunched]);
    assert_eq!(found(&db, "spider 45"), [relaunched]);
    // Only the whole number, as with issue numbers.
    assert_eq!(found(&db, "spider 84"), [original]);
}

#[test]
fn names_rank_above_notes_and_whole_matches_above_parts() {
    let db = db();
    let mut noted = entry("Avengers", 1963, "4");
    noted.copy.notes = "Thor cameo".into();
    let noted = add(&db, &noted);
    let series = add(&db, &entry("Thor", 1966, "126"));
    let mut part = entry("Journey into Mystery", 1952, "83");
    part.issue.title = "The Mighty Thor arrives".into();
    let part = add(&db, &part);
    assert_eq!(found(&db, "thor"), [series, part, noted]);
    // A book found through several fields ranks by its best one.
    let mut both = entry("Thor", 1966, "127");
    both.copy.notes = "thor and loki".into();
    let both = add(&db, &both);
    assert_eq!(found(&db, "thor"), [series, both, part, noted]);

    assert_eq!(quality(&fold("Thor"), "thor"), 1.0);
    assert_eq!(
        fold("Pokémon Café ÅNGSTRÖM Ñandú"),
        "pokemon cafe angstrom nandu"
    );
    assert!(quality("thor annual", "thor") > quality("mighty thor", "thor"));
    assert!(quality("mighty thor", "thor") > quality("panthor", "thor"));
    assert!(quality("panthor", "thor") > quality("thór", "thor"));
}

#[test]
fn query_syntax_is_taken_literally() {
    let db = db();
    let mut e = entry("Weird Science", 1950, "1");
    e.copy.notes = "100% complete; \"near mint\" said the_seller".into();
    let weird = add(&db, &e);
    add(&db, &entry("Other", 1990, "1"));
    for query in ["100%", "\"near", "the_seller", "_s"] {
        assert_eq!(found(&db, query), [weird], "{query}");
    }
    // Wildcards and FTS5 operators are plain characters: they find only
    // text that has them, and raise no error.
    for (query, wanted) in [
        ("%", vec![weird]),
        ("_", vec![weird]),
        ("\"", vec![weird]),
        ("*", vec![]),
        ("sci*", vec![]),
        ("AND", vec![]),
        ("NEAR(a b)", vec![]),
        ("-x", vec![]),
        ("^we", vec![]),
    ] {
        assert_eq!(found(&db, query), wanted, "{query}");
    }
}

#[test]
fn trash_hides_books_until_restored() {
    let c = collection();
    let trash = c.db.trash_copy(c.hulk).unwrap();
    assert!(found(&c.db, "wolverine").is_empty());
    c.db.restore(trash).unwrap();
    assert_eq!(found(&c.db, "wolverine"), [c.hulk]);
}

#[test]
fn the_index_follows_every_change() {
    let c = collection();
    let kirby =
        c.db.find_lookup::<CreatorId>("Jack Kirby")
            .unwrap()
            .unwrap();
    c.db.rename_lookup(kirby, "Jacob Kurtzberg").unwrap();
    assert!(found(&c.db, "kirby").is_empty());
    assert_eq!(found(&c.db, "kurtzberg"), [c.fantastic]);
    c.db.add_alias(kirby, "King Kirby").unwrap();
    assert_eq!(found(&c.db, "king"), [c.fantastic]);
    c.db.remove_alias(kirby, "King Kirby").unwrap();
    assert!(found(&c.db, "king").is_empty());

    let mut fields = c.db.copy(c.hulk).unwrap().fields;
    fields.notes = "Signed in silver".into();
    c.db.update_copy(c.hulk, &fields).unwrap();
    assert!(found(&c.db, "spine").is_empty());
    assert_eq!(found(&c.db, "silver"), [c.hulk]);

    let series = c.db.find_issue_series(c.spider);
    c.db.rename_shelf(Shelf::Series(series), "Spectacular Spider-Man")
        .unwrap();
    assert_eq!(found(&c.db, "spectacular"), [c.spider]);
    assert!(found(&c.db, "amazing").is_empty());

    // After all that, the index agrees with its source, and is exactly
    // what a rebuild would make.
    assert_consistent(&c.db);
    let before = index(&c.db);
    c.db.rebuild_search_index().unwrap();
    assert_eq!(index(&c.db), before);
}

#[test]
fn merging_entries_keeps_their_names_findable() {
    let db = db();
    let mut a = entry("Fantastic Four", 1961, "1");
    a.credits = vec![("J. Kirby".into(), Role::Penciller)];
    let first = add(&db, &a);
    let mut b = entry("Fantastic Four", 1961, "2");
    b.credits = vec![("Jack Kirby".into(), Role::Penciller)];
    let second = add(&db, &b);
    let alias = db.find_lookup::<CreatorId>("J. Kirby").unwrap().unwrap();
    db.merge_shelf(Shelf::Creator(alias), "Jack Kirby").unwrap();
    // The merged spelling became an alternate name of the kept entry.
    assert_eq!(found(&db, "j. kirby"), [first, second]);
    assert_consistent(&db);
    let before = index(&db);
    db.rebuild_search_index().unwrap();
    assert_eq!(index(&db), before);
}

#[test]
fn a_rebuilt_index_replaces_whatever_was_there() {
    let c = collection();
    let before = index(&c.db);
    // A file from elsewhere could carry an index that does not match.
    c.db.conn
        .execute(
            "INSERT INTO search_text (rowid, text) VALUES (99999, 'planted')",
            [],
        )
        .unwrap();
    c.db.conn
        .execute(
            "INSERT INTO search_text (search_text) VALUES ('delete-all')",
            [],
        )
        .unwrap();
    assert!(found(&c.db, "wolverine").is_empty());
    c.db.rebuild_search_index().unwrap();
    assert_eq!(index(&c.db), before);
    assert_eq!(found(&c.db, "wolverine"), [c.hulk]);
    assert!(found(&c.db, "planted").is_empty());
}

#[test]
fn mistyped_words_find_what_was_meant() {
    let c = collection();
    assert_eq!(found(&c.db, "kriby"), [c.fantastic]);
    assert_eq!(suggested(&c.db, "kriby").as_deref(), Some("Kirby"));
    assert_eq!(found(&c.db, "wolverien"), [c.hulk]);
    assert_eq!(suggested(&c.db, "wolverien").as_deref(), Some("Wolverine"));
    // Other words stay as typed.
    assert_eq!(found(&c.db, "fantastic kriby"), [c.fantastic]);
    assert_eq!(
        suggested(&c.db, "fantastic kriby").as_deref(),
        Some("fantastic Kirby")
    );
    // Hyphenated names are known whole and in parts.
    assert_eq!(found(&c.db, "spiderman"), [c.spider]);
    assert_eq!(suggested(&c.db, "spiderman").as_deref(), Some("Spider-Man"));
    assert_eq!(found(&c.db, "spidr"), [c.spider]);
    // Capitals, accents, and punctuation around a word are no obstacle.
    assert_eq!(suggested(&c.db, "KRIBY!").as_deref(), Some("Kirby"));
    assert_eq!(suggested(&c.db, "Wolvérien").as_deref(), Some("Wolverine"));
    // A word that is found needs no correction.
    assert_eq!(suggested(&c.db, "kirby"), None);
}

#[test]
fn only_near_known_names_are_suggested() {
    let c = collection();
    for query in ["xyzzyqw", "zzz", "kir", "conventoin"] {
        assert_eq!(suggested(&c.db, query), None, "{query}");
    }
    // Words of notes are found as typed, but never suggested.
    assert!(found(&c.db, "conventoin").is_empty());
    assert_eq!(found(&c.db, "convention"), [c.hulk]);
    // A correction that finds nothing live is not offered.
    c.db.trash_copy(c.fantastic).unwrap();
    assert!(found(&c.db, "kriby").is_empty());
    assert_eq!(suggested(&c.db, "kriby"), None);
}

#[test]
fn the_more_used_word_wins_a_tie() {
    let db = db();
    let mut a = entry("Incredible Hulk", 1968, "1");
    a.characters = vec![("Hulk".into(), false)];
    let hulk = add(&db, &a);
    let bulk = add(&db, &entry("Bulk Comics", 1990, "1"));
    // "Gulk" is one edit from both; Hulk names two entries.
    assert_eq!(suggested(&db, "gulk").as_deref(), Some("Hulk"));
    assert_eq!(found(&db, "gulk"), [hulk]);
    assert_eq!(found(&db, "bulk"), [bulk]);
}

#[test]
fn longer_words_may_be_further_off() {
    assert_eq!(edits_allowed(3), 0);
    assert_eq!(edits_allowed(4), 1);
    assert_eq!(edits_allowed(5), 1);
    assert_eq!(edits_allowed(6), 2);
    assert_eq!(edits_allowed(9), 2);
    assert_eq!(edits_allowed(10), 3);
    assert_eq!(edits_allowed(40), 3);
}

/// Runs FTS5's own check that the index matches the text it indexes.
fn assert_consistent(db: &Database) {
    db.conn
        .execute(
            "INSERT INTO search_text (search_text, rank) VALUES ('integrity-check', 1)",
            [],
        )
        .unwrap();
}

/// Every row of the search index, in order.
fn index(db: &Database) -> Vec<(i64, String, i64, i64)> {
    crate::catalog::query_all(
        &db.conn,
        "SELECT s.id, s.text, s.kind, s.owner FROM search_text f
         JOIN search_source s ON s.id = f.rowid ORDER BY s.id",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

impl Database {
    fn find_issue_series(&self, copy: CopyId) -> SeriesId {
        let edition = self.edition(self.copy(copy).unwrap().edition).unwrap();
        self.issue(edition.issue).unwrap().series
    }
}

/// The search target: every query under 100 ms at 10,000 copies. Run with
/// `cargo test --release -- --ignored`.
#[test]
#[ignore = "slow in debug builds; run in release"]
fn searching_ten_thousand_copies_is_quick() {
    use crate::fixtures::{FixtureSpec, generate};
    let db = db();
    generate(
        &db,
        FixtureSpec {
            copies: 10_000,
            seed: 42,
        },
    )
    .unwrap();
    // Broad and narrow words, short ones, several at once, and none.
    for query in [
        "a",
        "e",
        "man",
        "uncanny",
        "widget-man",
        "ghost",
        "mighty comet",
        "astonishing nova scouts",
        "zzzz",
        "astonsihing",
        "qqqqqqqqq",
    ] {
        let start = std::time::Instant::now();
        let rows = db.quick_search(query).unwrap().rows;
        let took = start.elapsed();
        assert!(
            took.as_millis() < 100,
            "{query:?} took {took:?} for {} books",
            rows.len()
        );
    }
}
