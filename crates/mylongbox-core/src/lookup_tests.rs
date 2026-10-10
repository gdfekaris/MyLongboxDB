//! Tests of name lists and the links from catalog rows to them.

use crate::model::*;
use crate::{Database, Error, IssueNumber, LookupId};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

/// A live comic issue with one edition and one copy.
fn issue(db: &Database, series_name: &str, n: &str) -> (IssueId, EditionId, CopyId) {
    let series = match db
        .list_series()
        .unwrap()
        .into_iter()
        .find(|s| s.fields.name == series_name)
    {
        Some(s) => s.id,
        None => db
            .create_series(&SeriesFields::new(series_name, Category::Comic))
            .unwrap(),
    };
    let issue = db
        .create_issue(
            series,
            &IssueFields::new(Some(IssueNumber::new(n).unwrap())),
        )
        .unwrap();
    let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
    let copy = db.create_copy(edition, &CopyFields::default()).unwrap();
    (issue, edition, copy)
}

fn names<I: LookupId>(db: &Database) -> Vec<String> {
    db.list_lookups::<I>()
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

fn every_list_basics<I: LookupId + std::fmt::Debug + PartialEq>() {
    let db = db();
    let b = db.create_lookup::<I>("  beta   two ").unwrap();
    let a = db.create_lookup::<I>("Alpha").unwrap();
    assert_eq!(names::<I>(&db), ["Alpha", "beta two"]);
    assert_eq!(db.lookup(a).unwrap().name, "Alpha");
    assert_eq!(db.find_lookup::<I>("ALPHA").unwrap(), Some(a));
    assert_eq!(db.find_lookup::<I>("Gamma").unwrap(), None);
    assert!(matches!(
        db.create_lookup::<I>("alpha"),
        Err(Error::NameTaken { existing, .. }) if existing == "Alpha"
    ));
    assert!(matches!(
        db.create_lookup::<I>("   "),
        Err(Error::Invalid(_))
    ));
    db.rename_lookup(b, "Beta").unwrap();
    db.rename_lookup(b, "BETA").unwrap();
    assert!(matches!(
        db.rename_lookup(b, "alpha"),
        Err(Error::NameTaken { .. })
    ));
    db.merge_lookups(b, a).unwrap();
    assert!(matches!(db.lookup(b), Err(Error::NotFound)));
    assert!(matches!(db.merge_lookups(a, a), Err(Error::Invalid(_))));
    db.delete_lookup(a).unwrap();
    assert!(db.list_lookups::<I>().unwrap().is_empty());
}

#[test]
fn every_list_supports_the_basics() {
    every_list_basics::<PublisherId>();
    every_list_basics::<BoxId>();
    every_list_basics::<TagId>();
    every_list_basics::<CreatorId>();
    every_list_basics::<CharacterId>();
    every_list_basics::<TeamId>();
    every_list_basics::<StoryArcId>();
}

// Alternate names -------------------------------------------------------------

#[test]
fn alternate_names_find_their_entry() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    db.add_alias(kirby, "J. Kirby").unwrap();
    db.add_alias(kirby, "Jacob Kurtzberg").unwrap();
    // Repeats and the entry's own name are ignored.
    db.add_alias(kirby, "j. kirby").unwrap();
    db.add_alias(kirby, "jack kirby").unwrap();
    assert_eq!(
        db.lookup(kirby).unwrap().aliases,
        ["J. Kirby", "Jacob Kurtzberg"]
    );
    assert_eq!(
        db.find_lookup::<CreatorId>("j. KIRBY").unwrap(),
        Some(kirby)
    );
    db.remove_alias(kirby, "Jacob Kurtzberg").unwrap();
    assert_eq!(db.lookup(kirby).unwrap().aliases, ["J. Kirby"]);
}

#[test]
fn a_name_belongs_to_one_entry() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    db.add_alias(kirby, "King Kirby").unwrap();
    let other = db.create_lookup::<CreatorId>("Steve Ditko").unwrap();
    for attempt in [
        db.create_lookup::<CreatorId>("king kirby").map(|_| ()),
        db.add_alias(other, "Jack Kirby"),
        db.add_alias(other, "KING KIRBY"),
        db.rename_lookup(other, "King Kirby"),
    ] {
        assert!(matches!(
            attempt,
            Err(Error::NameTaken { existing, .. }) if existing == "Jack Kirby"
        ));
    }
}

#[test]
fn renaming_to_an_alternate_name_swaps_them() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("J. Kirby").unwrap();
    db.add_alias(kirby, "Jack Kirby").unwrap();
    db.rename_lookup(kirby, "Jack Kirby").unwrap();
    let entry = db.lookup(kirby).unwrap();
    assert_eq!(entry.name, "Jack Kirby");
    assert!(entry.aliases.is_empty());
}

#[test]
fn lists_without_alternate_names_say_so() {
    let db = db();
    let tag = db.create_lookup::<TagId>("Key").unwrap();
    assert!(matches!(db.add_alias(tag, "Keys"), Err(Error::Invalid(_))));
}

#[test]
fn suggestions_match_names_and_alternate_names() {
    let db = db();
    let kirby = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    db.add_alias(kirby, "King Kirby").unwrap();
    let ditko = db.create_lookup::<CreatorId>("Steve Ditko").unwrap();
    let king = db.create_lookup::<CreatorId>("Kingsley Smith").unwrap();
    let percent = db.create_lookup::<CreatorId>("100% Studio").unwrap();
    let ids = |text: &str, limit| -> Vec<CreatorId> {
        db.suggest_lookups::<CreatorId>(text, limit)
            .unwrap()
            .into_iter()
            .map(|l| l.id)
            .collect()
    };
    assert_eq!(ids("ditko", 10), [ditko]);
    // Prefix matches on the name come first.
    assert_eq!(ids("king", 10), [king, kirby]);
    assert_eq!(ids("king", 1), [king]);
    // Typed wildcards match themselves.
    assert_eq!(ids("0%", 10), [percent]);
    assert_eq!(ids("_", 10), Vec::<CreatorId>::new());
}

// Merging ----------------------------------------------------------------

#[test]
fn merging_creators_moves_credits_and_keeps_the_old_name() {
    let db = db();
    let (one, edition, _) = issue(&db, "Fantastic Four", "1");
    let (two, _, _) = issue(&db, "Fantastic Four", "2");
    let jack = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    let j = db.create_lookup::<CreatorId>("J. Kirby").unwrap();
    db.add_alias(j, "Kirby, J.").unwrap();
    db.set_credits(
        one,
        &[
            (jack, Role::Penciller),
            (j, Role::Penciller),
            (j, Role::Inker),
        ],
    )
    .unwrap();
    db.set_credits(two, &[(j, Role::Penciller)]).unwrap();
    db.set_cover_artists(edition, &[j]).unwrap();

    db.merge_lookups(j, jack).unwrap();

    let roles = |issue| -> Vec<(String, Role)> {
        db.credits(issue)
            .unwrap()
            .into_iter()
            .map(|c| (c.name, c.role))
            .collect()
    };
    let jack_name = "Jack Kirby".to_owned();
    assert_eq!(
        roles(one),
        [
            (jack_name.clone(), Role::Penciller),
            (jack_name.clone(), Role::Inker)
        ]
    );
    assert_eq!(roles(two), [(jack_name, Role::Penciller)]);
    assert_eq!(db.cover_artists(edition).unwrap()[0].id, jack);
    assert_eq!(db.lookup(jack).unwrap().aliases, ["J. Kirby", "Kirby, J."]);
    assert_eq!(db.find_lookup::<CreatorId>("J. Kirby").unwrap(), Some(jack));
    assert!(matches!(db.lookup(j), Err(Error::NotFound)));
}

#[test]
fn merging_keeps_first_appearances_and_arc_parts() {
    let db = db();
    let (one, _, _) = issue(&db, "Amazing Fantasy", "15");
    let peter = db.create_lookup::<CharacterId>("Peter Parker").unwrap();
    let spidey = db.create_lookup::<CharacterId>("Spider-Man").unwrap();
    db.set_characters(one, &[(peter, false), (spidey, true)])
        .unwrap();
    db.merge_lookups(spidey, peter).unwrap();
    let characters = db.characters(one).unwrap();
    assert_eq!(characters.len(), 1);
    assert!(characters[0].first_appearance);

    let origin = db.create_lookup::<StoryArcId>("Origin").unwrap();
    let origins = db.create_lookup::<StoryArcId>("Origins").unwrap();
    db.set_story_arcs(one, &[(origin, None), (origins, Some(2))])
        .unwrap();
    db.merge_lookups(origins, origin).unwrap();
    assert_eq!(db.story_arcs(one).unwrap()[0].part, Some(2));
}

#[test]
fn merging_tags_publishers_and_boxes_moves_their_links() {
    let db = db();
    let (_, _, copy) = issue(&db, "Batman", "1");
    let key = db.create_lookup::<TagId>("Key").unwrap();
    let keys = db.create_lookup::<TagId>("Keys").unwrap();
    db.set_tags(copy, &[key, keys]).unwrap();
    db.merge_lookups(keys, key).unwrap();
    assert_eq!(
        db.tags(copy)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        [key]
    );

    let dc = db.create_lookup::<PublisherId>("DC").unwrap();
    let dc_comics = db.create_lookup::<PublisherId>("DC Comics").unwrap();
    let mut fields = SeriesFields::new("Detective Comics", Category::Comic);
    fields.publisher = Some(dc_comics);
    let series = db.create_series(&fields).unwrap();
    db.merge_lookups(dc_comics, dc).unwrap();
    assert_eq!(db.series(series).unwrap().fields.publisher, Some(dc));

    let box_a = db.create_lookup::<BoxId>("Box A").unwrap();
    let box_1 = db.create_lookup::<BoxId>("Box 1").unwrap();
    let stored = CopyFields {
        storage_box: Some(box_a),
        box_position: Some(3),
        ..CopyFields::default()
    };
    db.update_copy(copy, &stored).unwrap();
    db.merge_lookups(box_a, box_1).unwrap();
    assert_eq!(db.copy(copy).unwrap().fields.storage_box, Some(box_1));
}

// Deleting ---------------------------------------------------------------

#[test]
fn an_entry_in_use_cannot_be_deleted() {
    let db = db();
    let (one, _, copy) = issue(&db, "Batman", "1");
    let kane = db.create_lookup::<CreatorId>("Bob Kane").unwrap();
    db.set_credits(one, &[(kane, Role::Writer), (kane, Role::Penciller)])
        .unwrap();
    assert!(matches!(
        db.delete_lookup(kane),
        Err(Error::InUse { uses: 2 })
    ));
    let tag = db.create_lookup::<TagId>("Key").unwrap();
    db.set_tags(copy, &[tag]).unwrap();
    assert!(matches!(
        db.delete_lookup(tag),
        Err(Error::InUse { uses: 1 })
    ));
    // Links from items in Trash still count.
    db.trash_issue(one).unwrap();
    assert!(matches!(db.delete_lookup(kane), Err(Error::InUse { .. })));
}

// Links ------------------------------------------------------------------

#[test]
fn credits_list_in_role_order_then_name() {
    let db = db();
    let (one, _, _) = issue(&db, "Fantastic Four", "1");
    let stan = db.create_lookup::<CreatorId>("Stan Lee").unwrap();
    let jack = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    let george = db.create_lookup::<CreatorId>("George Klein").unwrap();
    db.set_credits(
        one,
        &[
            (george, Role::Inker),
            (stan, Role::Editor),
            (jack, Role::Penciller),
            (stan, Role::Writer),
        ],
    )
    .unwrap();
    let order: Vec<_> = db
        .credits(one)
        .unwrap()
        .into_iter()
        .map(|c| (c.name, c.role))
        .collect();
    assert_eq!(
        order,
        [
            ("Stan Lee".to_owned(), Role::Writer),
            ("Jack Kirby".to_owned(), Role::Penciller),
            ("George Klein".to_owned(), Role::Inker),
            ("Stan Lee".to_owned(), Role::Editor),
        ]
    );
}

#[test]
fn setting_a_list_replaces_it_and_drops_repeats() {
    let db = db();
    let (one, edition, copy) = issue(&db, "X-Men", "1");
    let xavier = db.create_lookup::<CharacterId>("Professor X").unwrap();
    let cyclops = db.create_lookup::<CharacterId>("Cyclops").unwrap();
    db.set_characters(one, &[(xavier, true), (cyclops, true), (xavier, false)])
        .unwrap();
    let got: Vec<_> = db
        .characters(one)
        .unwrap()
        .into_iter()
        .map(|a| (a.name, a.first_appearance))
        .collect();
    assert_eq!(
        got,
        [
            ("Cyclops".to_owned(), true),
            ("Professor X".to_owned(), true)
        ]
    );
    db.set_characters(one, &[(cyclops, false)]).unwrap();
    assert_eq!(db.characters(one).unwrap().len(), 1);

    let team = db.create_lookup::<TeamId>("X-Men").unwrap();
    db.set_teams(one, &[(team, true)]).unwrap();
    assert!(db.teams(one).unwrap()[0].first_appearance);

    let jack = db.create_lookup::<CreatorId>("Jack Kirby").unwrap();
    db.set_cover_artists(edition, &[jack, jack]).unwrap();
    assert_eq!(db.cover_artists(edition).unwrap().len(), 1);

    db.set_tags(copy, &[]).unwrap();
    assert!(db.tags(copy).unwrap().is_empty());
}

#[test]
fn links_cannot_be_changed_in_trash_or_given_bad_parts() {
    let db = db();
    let (one, _, copy) = issue(&db, "X-Men", "1");
    let arc = db.create_lookup::<StoryArcId>("Dark Phoenix").unwrap();
    assert!(matches!(
        db.set_story_arcs(one, &[(arc, Some(0))]),
        Err(Error::Invalid(_))
    ));
    db.trash_issue(one).unwrap();
    assert!(matches!(db.set_story_arcs(one, &[]), Err(Error::InTrash)));
    assert!(matches!(db.set_tags(copy, &[]), Err(Error::InTrash)));
}
