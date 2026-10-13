//! A generated, made-up collection for tests and performance checks.
//!
//! Every name is invented, so no real collection data is ever needed.
//! The same seed always produces the same collection.

use rusqlite::params;

use crate::model::*;
use crate::{Amount, Currency, Database, Date, IssueNumber, Result};

/// A small, fast pseudo-random generator (SplitMix64). Not for anything
/// that needs real randomness; only for repeatable test data.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`; `n` must be above zero.
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// True `percent` times in a hundred.
    pub(crate) fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub(crate) fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

const ADJECTIVES: &[&str] = &[
    "Astonishing",
    "Uncanny",
    "Mighty",
    "Savage",
    "Cosmic",
    "Haunted",
    "Daring",
    "Silent",
    "Electric",
    "Forgotten",
    "Radiant",
    "Midnight",
    "Iron",
    "Crimson",
    "Wandering",
];
const NOUNS: &[&str] = &[
    "Widget-Man",
    "Comet Patrol",
    "Night Owlet",
    "Tin Sentinel",
    "Glass Falcon",
    "Star Rangers",
    "Moth Queen",
    "Clockwork Kid",
    "Thunder Fox",
    "Paper Knight",
    "Velvet Ghost",
    "Copper Lion",
    "Nova Scouts",
    "Ember Witch",
    "Quartz Golem",
];
const MAGAZINES: &[&str] = &[
    "Weird Worlds Illustrated",
    "Shadow Tales Quarterly",
    "Galaxy Gazette",
    "Creature Feature",
];
const BOOK_LINES: &[&str] = &["Archive Edition", "Grand Omnibus", "Essential Library"];
const FIRST_NAMES: &[&str] = &[
    "Ada", "Bram", "Cleo", "Dov", "Esme", "Finn", "Greta", "Hugo", "Ines", "Jules", "Kofi", "Lena",
    "Milo", "Nia", "Otto", "Pia", "Quinn", "Rosa", "Sven", "Tess", "Zoë", "Åsa",
];
const LAST_NAMES: &[&str] = &[
    "Abernathy",
    "Brightwater",
    "Castellan",
    "Dunmore",
    "Everly",
    "Fairweather",
    "Grimsby",
    "Holloway",
    "Ivers",
    "Juniper",
    "Kettering",
    "Lockhart",
    "Mayfield",
    "Northcott",
    "Oakes",
    "Pemberton",
    "Quill",
    "Ravensworth",
    "Stroud",
    "Thistle",
    "Underhill",
    "Vance",
    "Wexley",
];
const PUBLISHERS: &[&str] = &[
    "Lantern Press",
    "Starling Comics",
    "Bellwether Books",
    "Kestrel House",
    "Orbit Line",
];
const TAGS: &[&str] = &["Key", "For sale", "Signed", "Wishlist upgrade", "Gift"];

/// How big a collection to generate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FixtureSpec {
    pub copies: usize,
    pub seed: u64,
}

/// Counts of what was generated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FixtureSummary {
    pub series: usize,
    pub issues: usize,
    pub copies: usize,
    pub trashed: usize,
}

/// Fills an empty collection with a made-up one of about `spec.copies`
/// copies. Exactly that many live copies exist afterwards, plus a few in
/// Trash.
pub(crate) fn generate(db: &Database, spec: FixtureSpec) -> Result<FixtureSummary> {
    let mut rng = Rng::new(spec.seed);
    let mut summary = FixtureSummary::default();

    let publishers = PUBLISHERS
        .iter()
        .map(|name| db.create_lookup::<PublisherId>(name))
        .collect::<Result<Vec<_>>>()?;
    let tags = TAGS
        .iter()
        .map(|name| db.create_lookup::<TagId>(name))
        .collect::<Result<Vec<_>>>()?;
    let mut creators = Vec::new();
    for first in FIRST_NAMES {
        for last in LAST_NAMES.iter().take(8) {
            creators.push(db.create_lookup::<CreatorId>(&format!("{first} {last}"))?);
        }
    }
    let characters = names(&mut rng, 120)
        .iter()
        .map(|name| db.create_lookup::<CharacterId>(name))
        .collect::<Result<Vec<_>>>()?;
    let teams = NOUNS
        .iter()
        .map(|noun| db.create_lookup::<TeamId>(&format!("The {noun} Brigade")))
        .collect::<Result<Vec<_>>>()?;
    let arcs = ADJECTIVES
        .iter()
        .map(|adjective| db.create_lookup::<StoryArcId>(&format!("The {adjective} Saga")))
        .collect::<Result<Vec<_>>>()?;
    let boxes = (1..=spec.copies.div_ceil(150).max(1))
        .map(|n| db.create_lookup::<BoxId>(&format!("Box {n}")))
        .collect::<Result<Vec<_>>>()?;

    db.conn.execute(
        "INSERT INTO exchange_rate (date, usd_cents_per_ounce, usd_cents_per_bitcoin)
         VALUES ('1985-01-01', 30000, 1), ('2015-01-01', 120000, 30000000),
                ('2026-09-01', 400000, 10000000)",
        [],
    )?;

    let mut comic_series = Vec::new();
    let mut copies = 0;
    let mut box_fill = vec![0u32; boxes.len()];
    let mut series_number = 0;
    while copies < spec.copies {
        series_number += 1;
        let roll = rng.below(10);
        let category = match roll {
            0 => Category::Magazine,
            1 => Category::CollectedEdition,
            _ => Category::Comic,
        };
        let mut fields =
            SeriesFields::new(series_name(&mut rng, category, series_number), category);
        fields.start_year = Some(1960 + rng.below(66) as u16);
        fields.volume = Some(1 + rng.below(3) as u32);
        fields.publisher = Some(*rng.pick(&publishers));
        if category == Category::Magazine {
            fields.magazine_format = Some(*rng.pick(MagazineFormat::ALL));
            fields.black_and_white = Some(rng.chance(60));
        }
        let run_length = match category {
            Category::CollectedEdition => 1 + rng.below(8),
            _ => 5 + rng.below(60),
        } as u32;
        if rng.chance(50) {
            fields.known_issue_count = Some(run_length + rng.below(20) as u32);
        }
        let series = db.create_series(&fields)?;
        summary.series += 1;
        if category == Category::Comic {
            comic_series.push((series, fields.name.clone(), fields.start_year, run_length));
        }

        let start_year = fields.start_year.unwrap_or(1980);
        for n in 1..=run_length {
            if copies >= spec.copies {
                break;
            }
            // Gaps in the run, for the gaps view.
            if rng.chance(15) {
                continue;
            }
            let number = if category == Category::Comic && rng.chance(3) {
                format!("Annual {}", 1 + n / 12)
            } else {
                n.to_string()
            };
            let Ok(number) = IssueNumber::new(&number) else {
                continue;
            };
            let mut issue_fields = IssueFields::new(Some(number));
            issue_fields.title = if rng.chance(70) {
                format!("{} {}", rng.pick(ADJECTIVES), rng.pick(NOUNS))
            } else {
                String::new()
            };
            let month = 1 + (n - 1) % 12;
            issue_fields.cover_date =
                CoverDate::new(start_year + ((n - 1) / 12) as u16, Some(month as u8));
            issue_fields.page_count = Some(if category == Category::CollectedEdition {
                200 + rng.below(1000) as u32
            } else {
                32 + rng.below(40) as u32
            });
            issue_fields.is_key = rng.chance(4);
            if issue_fields.is_key {
                issue_fields.key_reason = "First appearance".into();
            }
            let issue = match db.create_issue(series, &issue_fields) {
                Ok(issue) => issue,
                // Two annuals can land on one number; skip the repeat.
                Err(crate::Error::DuplicateIssueNumber { .. }) => continue,
                Err(e) => return Err(e),
            };
            summary.issues += 1;

            let writer = *rng.pick(&creators);
            let artist = *rng.pick(&creators);
            db.set_credits(issue, &[(writer, Role::Writer), (artist, Role::Penciller)])?;
            if category != Category::CollectedEdition {
                let cast: Vec<_> = (0..1 + rng.below(4))
                    .map(|_| (*rng.pick(&characters), rng.chance(3)))
                    .collect();
                db.set_characters(issue, &cast)?;
                if rng.chance(30) {
                    db.set_teams(issue, &[(*rng.pick(&teams), false)])?;
                }
                if rng.chance(25) {
                    db.set_story_arcs(issue, &[(*rng.pick(&arcs), Some(1 + rng.below(6) as u32))])?;
                }
            } else if !comic_series.is_empty() {
                let (_, name, year, length) = rng.pick(&comic_series).clone();
                let from = 1 + rng.below(u64::from(length));
                let to = (from + rng.below(12)).min(u64::from(length));
                let year = year.map(|y| format!(" ({y})")).unwrap_or_default();
                db.set_collects(issue, &format!("{name}{year} {from}-{to}"))?;
            }

            let editions = if rng.chance(12) { 2 } else { 1 };
            for variant in 0..editions {
                if copies >= spec.copies {
                    break;
                }
                let edition =
                    db.create_edition(issue, &edition_fields(&mut rng, category, variant))?;
                let wanted = if rng.chance(8) { 2 } else { 1 };
                for _ in 0..wanted {
                    if copies >= spec.copies {
                        break;
                    }
                    let slot = rng.below(boxes.len() as u64) as usize;
                    let fields = copy_fields(&mut rng, category, boxes[slot], &mut box_fill[slot]);
                    let copy = db.create_copy(edition, &fields)?;
                    if rng.chance(10) {
                        db.set_tags(copy, &[*rng.pick(&tags)])?;
                    }
                    if rng.chance(6) {
                        let signatures: Vec<_> = (0..=rng.below(3))
                            .map(|_| {
                                let mut f = SignatureFields::new(*rng.pick(Verification::ALL));
                                f.sketch = rng.chance(10);
                                (*rng.pick(&creators), f)
                            })
                            .collect();
                        db.set_signatures(copy, &signatures)?;
                    }
                    if rng.chance(80) {
                        add_values(db, &mut rng, copy)?;
                    }
                    copies += 1;
                }
            }
        }
    }
    summary.copies = copies;

    // A few deleted items, so views can be checked for leaving Trash out.
    // Fresh copies are added for them, so the live count stays exact.
    let trash_count = if comic_series.is_empty() {
        0
    } else {
        (spec.copies / 200).max(1)
    };
    for _ in 0..trash_count {
        let series = &comic_series[rng.below(comic_series.len() as u64) as usize];
        let number = IssueNumber::new(&format!("Trashed {}", summary.trashed + 1))
            .map_err(|e| crate::Error::Invalid(e.to_string()))?;
        let issue = db.create_issue(series.0, &IssueFields::new(Some(number)))?;
        let edition = db.create_edition(issue, &EditionFields::default())?;
        db.create_copy(edition, &CopyFields::default())?;
        db.trash_issue(issue)?;
        summary.trashed += 1;
    }
    Ok(summary)
}

fn names(rng: &mut Rng, count: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(count);
    let mut n = 0;
    while out.len() < count {
        n += 1;
        let name = format!("{} {} {n}", rng.pick(ADJECTIVES), rng.pick(NOUNS));
        out.push(name);
    }
    out
}

fn series_name(rng: &mut Rng, category: Category, n: usize) -> String {
    match category {
        Category::Magazine => format!("{} {n}", rng.pick(MAGAZINES)),
        Category::CollectedEdition => format!("{}: {} {n}", rng.pick(BOOK_LINES), rng.pick(NOUNS)),
        Category::Comic => format!("The {} {} {n}", rng.pick(ADJECTIVES), rng.pick(NOUNS)),
    }
}

fn price(rng: &mut Rng) -> Amount {
    match rng.below(20) {
        0 => Amount::new(Currency::Gold, 1 + rng.below(500) as i64),
        1 => Amount::new(Currency::Btc, 1_000 + rng.below(5_000_000) as i64),
        _ => Amount::new(Currency::Usd, 25 + rng.below(200_000) as i64),
    }
}

fn edition_fields(rng: &mut Rng, category: Category, variant: u32) -> EditionFields {
    let mut f = EditionFields {
        variant: if variant == 0 {
            String::new()
        } else {
            format!("Cover {}", (b'A' + variant as u8) as char)
        },
        printing: Some(1),
        distribution: if category == Category::CollectedEdition {
            None
        } else {
            Some(*rng.pick(Distribution::ALL))
        },
        cover_price: Some(Amount::new(Currency::Usd, 12 + rng.below(500) as i64)),
        ..EditionFields::default()
    };
    if category == Category::CollectedEdition {
        f.binding = Some(*rng.pick(Binding::ALL));
        f.has_dust_jacket = Some(f.binding != Some(Binding::Softcover));
        f.has_slipcase = Some(rng.chance(10));
        f.isbn = Some(isbn(rng));
    }
    f
}

/// A made-up ISBN-13 with a valid check digit.
fn isbn(rng: &mut Rng) -> Isbn {
    let mut digits: Vec<u32> = vec![9, 7, 8];
    digits.extend((0..9).map(|_| rng.below(10) as u32));
    let sum: u32 = digits
        .iter()
        .enumerate()
        .map(|(i, d)| d * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    digits.push((10 - sum % 10) % 10);
    let text: String = digits
        .iter()
        .map(|d| char::from_digit(*d, 10).unwrap_or('0'))
        .collect();
    Isbn::parse(&text).unwrap_or_else(|| Isbn::parse("9780000000002").expect("valid ISBN"))
}

fn copy_fields(
    rng: &mut Rng,
    category: Category,
    storage_box: BoxId,
    fill: &mut u32,
) -> CopyFields {
    let mut f = CopyFields::default();
    if category == Category::CollectedEdition {
        f.book_condition = Some(*rng.pick(BookCondition::ALL));
    } else if rng.chance(90) {
        f.grade = Grade::from_tenths(5 + rng.below(96) as u8);
        f.grade_type = Some(if rng.chance(15) {
            GradeType::Cgc
        } else {
            GradeType::Raw
        });
        if f.grade_type == Some(GradeType::Cgc) {
            f.cert_number = Some(format!("{:010}", rng.below(10_000_000_000)));
            f.page_quality = Some(*rng.pick(PageQuality::ALL));
        }
    }
    if rng.chance(85) {
        f.purchase_price = Some(price(rng));
        f.purchase_date = Date::new(
            1985 + rng.below(41) as u16,
            1 + rng.below(12) as u8,
            1 + rng.below(28) as u8,
        )
        .ok();
        f.seller = rng
            .pick(&["Local shop", "Convention", "Online auction", "Garage sale"])
            .to_string();
    }
    if rng.chance(92) {
        *fill += 1;
        f.storage_box = Some(storage_box);
        f.box_position = Some(*fill);
    }
    f
}

/// Value history has no API yet, so rows are written directly.
fn add_values(db: &Database, rng: &mut Rng, copy: CopyId) -> Result<()> {
    for year in 0..1 + rng.below(3) {
        let value = price(rng);
        db.conn.execute(
            "INSERT INTO value_entry (uuid, copy_id, date, value_units, value_currency, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                RowUuid::new_random(),
                copy,
                Date::new(2020 + year as u16, 6, 1).ok(),
                value.units(),
                value.currency(),
                *rng.pick(&["Price guide", "Recent sale", "Appraisal"]),
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(db: &Database) -> Vec<String> {
        db.list_series()
            .unwrap()
            .into_iter()
            .flat_map(|s| {
                db.issues_in_series(s.id)
                    .unwrap()
                    .into_iter()
                    .map(move |i| format!("{} #{}", s.fields.name, i.fields.number.unwrap()))
            })
            .collect()
    }

    fn live_copies(db: &Database) -> i64 {
        db.conn
            .query_row(
                "SELECT count(*) FROM copy WHERE trash_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn the_same_seed_gives_the_same_collection() {
        let a = Database::open_in_memory().unwrap();
        let b = Database::open_in_memory().unwrap();
        let spec = FixtureSpec {
            copies: 300,
            seed: 7,
        };
        assert_eq!(generate(&a, spec).unwrap(), generate(&b, spec).unwrap());
        assert_eq!(snapshot(&a), snapshot(&b));
        let c = Database::open_in_memory().unwrap();
        generate(
            &c,
            FixtureSpec {
                copies: 300,
                seed: 8,
            },
        )
        .unwrap();
        assert_ne!(snapshot(&a), snapshot(&c));
    }

    #[test]
    fn makes_exactly_the_requested_live_copies_and_a_consistent_file() {
        let db = Database::open_in_memory().unwrap();
        let summary = generate(
            &db,
            FixtureSpec {
                copies: 1000,
                seed: 1,
            },
        )
        .unwrap();
        assert_eq!(summary.copies, 1000);
        assert_eq!(live_copies(&db), 1000);
        assert!(summary.trashed >= 1);
        assert_eq!(db.trash_groups().unwrap().len(), summary.trashed);
        let violations: i64 = db
            .conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
        let integrity: String = db
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    #[test]
    fn covers_every_kind_of_row_the_views_need() {
        let db = Database::open_in_memory().unwrap();
        generate(
            &db,
            FixtureSpec {
                copies: 1000,
                seed: 3,
            },
        )
        .unwrap();
        let count = |sql: &str| -> i64 { db.conn.query_row(sql, [], |row| row.get(0)).unwrap() };
        for (what, sql) in [
            (
                "magazines",
                "SELECT count(*) FROM series WHERE category = 'magazine'",
            ),
            (
                "collected editions",
                "SELECT count(*) FROM series WHERE category = 'collected'",
            ),
            ("contents lists", "SELECT count(*) FROM collects"),
            (
                "variants",
                "SELECT count(*) FROM edition WHERE variant <> ''",
            ),
            (
                "duplicate copies",
                "SELECT count(*) FROM (SELECT edition_id FROM copy GROUP BY edition_id HAVING count(*) > 1)",
            ),
            (
                "ungraded comics",
                "SELECT count(*) FROM copy WHERE grade IS NULL AND book_condition IS NULL",
            ),
            (
                "unboxed copies",
                "SELECT count(*) FROM copy WHERE box_id IS NULL",
            ),
            (
                "gold prices",
                "SELECT count(*) FROM copy WHERE purchase_price_currency = 'XAU'",
            ),
            (
                "bitcoin prices",
                "SELECT count(*) FROM copy WHERE purchase_price_currency = 'BTC'",
            ),
            ("value history", "SELECT count(*) FROM value_entry"),
            (
                "copies without values",
                "SELECT count(*) FROM copy WHERE id NOT IN (SELECT copy_id FROM value_entry)",
            ),
            ("key issues", "SELECT count(*) FROM issue WHERE is_key = 1"),
            (
                "annuals",
                "SELECT count(*) FROM issue WHERE number LIKE 'Annual %'",
            ),
            (
                "first appearances",
                "SELECT count(*) FROM issue_character WHERE first_appearance = 1",
            ),
            ("tags", "SELECT count(*) FROM copy_tag"),
            ("signatures", "SELECT count(*) FROM signature"),
            (
                "accented names",
                "SELECT count(*) FROM creator WHERE name LIKE 'Zo%' OR name LIKE '%sa %'",
            ),
            ("exchange rates", "SELECT count(*) FROM exchange_rate"),
        ] {
            assert!(count(sql) > 0, "no {what}");
        }
    }

    #[test]
    fn tiny_collections_work_too() {
        for copies in [0, 1, 2, 5] {
            let db = Database::open_in_memory().unwrap();
            let summary = generate(&db, FixtureSpec { copies, seed: 9 }).unwrap();
            assert_eq!(summary.copies, copies);
            assert_eq!(live_copies(&db), copies as i64);
        }
    }

    /// The size the search performance target is set for. Run with
    /// `cargo test --release -- --ignored`.
    #[test]
    #[ignore = "slow in debug builds; run in release"]
    fn generates_ten_thousand_copies() {
        let db = Database::open_in_memory().unwrap();
        let summary = generate(
            &db,
            FixtureSpec {
                copies: 10_000,
                seed: 42,
            },
        )
        .unwrap();
        assert_eq!(live_copies(&db), 10_000);
        assert!(summary.series > 100);
    }
}
