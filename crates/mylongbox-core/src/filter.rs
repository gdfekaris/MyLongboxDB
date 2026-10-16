//! The filter panel's model: rows of field, condition, and value, all
//! of which a book must meet.
//!
//! Each row is answered by one fixed SQL statement with bound values,
//! giving the issues or the copies that meet it; rows are combined in
//! Rust. A row about the issue (its series, number, people, cast, dates)
//! is met by issues, and the issues are then turned into books as the
//! set's [`OwnedAs`] says, so every issue row must hold for the same
//! issue. A row about the physical book (grade, price, photos, box) is
//! met by copies directly.

use std::collections::HashSet;

use rusqlite::{Params, params};
use serde::{Deserialize, Serialize};
use strsim::osa_distance;

use crate::browse::{CopyRow, copy_row};
use crate::catalog::query_all;
use crate::date::Date;
use crate::db::Database;
use crate::error::Result;
use crate::issue_number::IssueNumber;
use crate::lookup::LookupId;
use crate::model::{
    Binding, BookCondition, Category, CharacterId, CopyId, CreatorId, Distribution, Grade,
    GradeType, MagazineFormat, PageQuality, Role, SeriesId, SignaturePlacement, StoryArcId, TagId,
    TeamId, Verification,
};
use crate::money::{Amount, Currency};
use crate::search::{edits_allowed, fold, json_ids, like_escaped};

/// How the issues that issue rows find are turned into books.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedAs {
    /// Copies of the issue itself.
    #[default]
    SingleIssue,
    /// Copies of collected editions that reprint the issue.
    CollectedEdition,
    Either,
}

/// A comparison for numbers, grades, money, and issue numbers. Between
/// includes both ends, in either order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compare<T> {
    Equal(T),
    AtLeast(T),
    AtMost(T),
    Between(T, T),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextField {
    SeriesName,
    Publisher,
    Title,
    Synopsis,
    KeyReason,
    Variant,
    Barcode,
    Isbn,
    CertNumber,
    LabelType,
    Seller,
    StorageBox,
    Notes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextCondition {
    Contains,
    Is,
    /// Close in spelling, for names remembered roughly.
    SimilarTo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberField {
    PageCount,
    LegacyNumber,
    Printing,
    Volume,
    StartYear,
    BoxPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoneyField {
    CoverPrice,
    PurchasePrice,
    /// The newest value entry.
    CurrentValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateField {
    /// By month; an issue dated only by year counts as the whole year.
    CoverDate,
    ReleaseDate,
    PurchaseDate,
    Added,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateCondition {
    Before(Date),
    After(Date),
    Between(Date, Date),
    InYear(u16),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Category(Vec<Category>),
    MagazineFormat(Vec<MagazineFormat>),
    Distribution(Vec<Distribution>),
    Binding(Vec<Binding>),
    GradeType(Vec<GradeType>),
    PageQuality(Vec<PageQuality>),
    BookCondition(Vec<BookCondition>),
    DustJacketCondition(Vec<BookCondition>),
    /// How any of the book's signatures is verified.
    Verification(Vec<Verification>),
    /// Where on the book any of its signatures is.
    SignaturePlacement(Vec<SignaturePlacement>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    KeyIssue,
    /// A first appearance of a character or team.
    FirstAppearance,
    BlackAndWhite,
    DustJacket,
    Slipcase,
    Signed,
    /// A signature with a sketch.
    Sketch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    FrontPhoto,
    BackPhoto,
    /// A photo of a signature inside the book.
    SignaturePhoto,
    Value,
    /// A grade, or a book condition for a collected edition.
    Grade,
    StorageBox,
    PurchasePrice,
    PurchaseDate,
    CoverPrice,
    CoverDate,
    ReleaseDate,
    Tags,
    Notes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListField {
    Characters,
    Teams,
    StoryArcs,
    Tags,
    CoverArtists,
    SignedBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListCondition {
    AnyOf,
    AllOf,
    NoneOf,
}

/// One row of the filter panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Text {
        field: TextField,
        condition: TextCondition,
        value: String,
    },
    /// Credited on the issue, in `role` if one is given.
    Person {
        name: String,
        role: Option<Role>,
    },
    List {
        field: ListField,
        condition: ListCondition,
        names: Vec<String>,
    },
    IssueNumber(Compare<IssueNumber>),
    Grade(Compare<Grade>),
    Number {
        field: NumberField,
        compare: Compare<u32>,
    },
    /// Amounts in another currency are converted at the exchange rate
    /// for their date; those that cannot be converted do not match.
    Money {
        field: MoneyField,
        compare: Compare<Amount>,
    },
    Date {
        field: DateField,
        condition: DateCondition,
    },
    Choice(Choice),
    Flag {
        flag: Flag,
        value: bool,
    },
    Has {
        what: Presence,
        has: bool,
    },
    /// Collected editions that reprint issues of `series`, numbered
    /// within the range if one is given.
    Collects {
        series: SeriesId,
        from: Option<IssueNumber>,
        to: Option<IssueNumber>,
    },
}

/// Every row of the panel, all of which a book must meet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FilterSet {
    pub rules: Vec<Filter>,
    pub owned_as: OwnedAs,
}

/// What a rule is met by.
enum Met {
    Issues(HashSet<i64>),
    Copies(HashSet<i64>),
}

impl Met {
    fn of(issue_level: bool, found: HashSet<i64>) -> Self {
        if issue_level {
            Self::Issues(found)
        } else {
            Self::Copies(found)
        }
    }
}

/// `SELECT $id FROM $from WHERE $column$test`, as one fixed string.
macro_rules! select_where {
    ($id:literal, $from:literal, $column:literal, $test:literal) => {
        concat!("SELECT ", $id, " FROM ", $from, " WHERE ", $column, $test)
    };
}

/// A statement for each condition on one text field, and whether the
/// field belongs to the issue or the copy.
struct TextSql {
    issue_level: bool,
    contains: &'static str,
    is: &'static str,
    /// Every id with its value, for judging similarity in Rust.
    values: &'static str,
}

macro_rules! text_sql {
    ($issue_level:literal, $id:literal, $from:literal, $column:literal) => {
        TextSql {
            issue_level: $issue_level,
            contains: select_where!($id, $from, $column, " LIKE ?1 ESCAPE '\\'"),
            is: select_where!($id, $from, $column, " = ?1 COLLATE NOCASE"),
            values: concat!(
                "SELECT ", $id, ", ", $column, " FROM ", $from, " WHERE ", $column, " <> ''"
            ),
        }
    };
}

impl TextField {
    fn sql(self) -> TextSql {
        match self {
            Self::SeriesName => {
                text_sql!(
                    true,
                    "i.id",
                    "issue i JOIN series s ON s.id = i.series_id",
                    "s.name"
                )
            }
            Self::Publisher => text_sql!(
                true,
                "i.id",
                "issue i JOIN series s ON s.id = i.series_id
                 JOIN publisher p ON p.id = s.publisher_id",
                "p.name"
            ),
            Self::Title => text_sql!(true, "id", "issue", "title"),
            Self::Synopsis => text_sql!(true, "id", "issue", "synopsis"),
            Self::KeyReason => text_sql!(true, "id", "issue", "key_reason"),
            Self::Variant => text_sql!(
                false,
                "c.id",
                "copy c JOIN edition e ON e.id = c.edition_id",
                "e.variant"
            ),
            Self::Barcode => text_sql!(
                false,
                "c.id",
                "copy c JOIN edition e ON e.id = c.edition_id",
                "e.barcode"
            ),
            Self::Isbn => {
                text_sql!(
                    false,
                    "c.id",
                    "copy c JOIN edition e ON e.id = c.edition_id",
                    "e.isbn"
                )
            }
            Self::CertNumber => text_sql!(false, "id", "copy", "cert_number"),
            Self::LabelType => text_sql!(false, "id", "copy", "label_type"),
            Self::Seller => text_sql!(false, "id", "copy", "seller"),
            Self::StorageBox => text_sql!(
                false,
                "c.id",
                "copy c JOIN storage_box b ON b.id = c.box_id",
                "b.name"
            ),
            Self::Notes => text_sql!(false, "id", "copy", "notes"),
        }
    }
}

/// A statement for each comparison on one field.
struct CompareSql {
    issue_level: bool,
    equal: &'static str,
    at_least: &'static str,
    at_most: &'static str,
    between: &'static str,
}

macro_rules! compare_sql {
    ($issue_level:literal, $id:literal, $from:literal, $column:literal) => {
        CompareSql {
            issue_level: $issue_level,
            equal: select_where!($id, $from, $column, " = ?1"),
            at_least: select_where!($id, $from, $column, " >= ?1"),
            at_most: select_where!($id, $from, $column, " <= ?1"),
            between: select_where!($id, $from, $column, " BETWEEN ?1 AND ?2"),
        }
    };
}

impl NumberField {
    fn sql(self) -> CompareSql {
        match self {
            Self::PageCount => compare_sql!(true, "id", "issue", "page_count"),
            Self::LegacyNumber => compare_sql!(true, "id", "issue", "legacy_number"),
            Self::Printing => compare_sql!(
                false,
                "c.id",
                "copy c JOIN edition e ON e.id = c.edition_id",
                "e.printing"
            ),
            Self::Volume => compare_sql!(
                true,
                "i.id",
                "issue i JOIN series s ON s.id = i.series_id",
                "s.volume"
            ),
            Self::StartYear => compare_sql!(
                true,
                "i.id",
                "issue i JOIN series s ON s.id = i.series_id",
                "s.start_year"
            ),
            Self::BoxPosition => compare_sql!(false, "id", "copy", "box_position"),
        }
    }
}

const GRADE_SQL: CompareSql = compare_sql!(false, "id", "copy", "grade");
// Standalone books have no number, and so an empty sort key, which
// would otherwise sort before every number.
const NUMBER_SQL: CompareSql =
    compare_sql!(true, "id", "issue", "number IS NOT NULL AND number_sort");

/// Each copy's amount for a money field, with the date to convert it at:
/// a cover price at the issue's release or cover date, and anything
/// undated at the newest rate.
fn money_sql(field: MoneyField) -> &'static str {
    match field {
        MoneyField::CoverPrice => {
            "SELECT c.id, e.cover_price_units, e.cover_price_currency,
                 coalesce(i.release_date,
                     CASE WHEN i.cover_year IS NOT NULL
                     THEN printf('%04d-%02d-01', i.cover_year, coalesce(i.cover_month, 1)) END)
             FROM copy c JOIN edition e ON e.id = c.edition_id
             JOIN issue i ON i.id = e.issue_id
             WHERE e.cover_price_units IS NOT NULL"
        }
        MoneyField::PurchasePrice => {
            "SELECT id, purchase_price_units, purchase_price_currency, purchase_date
             FROM copy WHERE purchase_price_units IS NOT NULL"
        }
        MoneyField::CurrentValue => {
            "SELECT v.copy_id, v.value_units, v.value_currency, v.date FROM value_entry v
             WHERE v.id = (SELECT w.id FROM value_entry w WHERE w.copy_id = v.copy_id
                           ORDER BY w.date DESC, w.id DESC LIMIT 1)"
        }
    }
}

/// Statements for before, after, and between on a date field.
struct DateSql {
    issue_level: bool,
    before: &'static str,
    after: &'static str,
    between: &'static str,
}

macro_rules! date_sql {
    ($issue_level:literal, $id:literal, $from:literal, $column:literal) => {
        DateSql {
            issue_level: $issue_level,
            before: select_where!($id, $from, $column, " < ?1"),
            after: select_where!($id, $from, $column, " > ?1"),
            between: select_where!($id, $from, $column, " BETWEEN ?1 AND ?2"),
        }
    };
}

impl DateField {
    fn sql(self) -> DateSql {
        match self {
            // Months as year × 100 + month. An issue dated by year only
            // spans its whole year: it is before a date only if all of
            // the year is, and within a range only if all of it is.
            Self::CoverDate => DateSql {
                issue_level: true,
                before: "SELECT id FROM issue
                         WHERE cover_year * 100 + coalesce(cover_month, 12) < ?1",
                after: "SELECT id FROM issue
                        WHERE cover_year * 100 + coalesce(cover_month, 1) > ?1",
                between: "SELECT id FROM issue
                          WHERE cover_year * 100 + coalesce(cover_month, 1) >= ?1
                          AND cover_year * 100 + coalesce(cover_month, 12) <= ?2",
            },
            Self::ReleaseDate => date_sql!(true, "id", "issue", "release_date"),
            Self::PurchaseDate => date_sql!(false, "id", "copy", "purchase_date"),
            Self::Added => date_sql!(false, "id", "copy", "substr(added_at, 1, 10)"),
        }
    }
}

/// The statement for a choice field, its values as a JSON array in `?1`.
fn choice_sql(choice: &Choice) -> (&'static str, bool) {
    macro_rules! within {
        ($id:literal, $from:literal, $column:literal) => {
            select_where!($id, $from, $column, " IN (SELECT value FROM json_each(?1))")
        };
    }
    match choice {
        Choice::Category(_) => (
            within!(
                "i.id",
                "issue i JOIN series s ON s.id = i.series_id",
                "s.category"
            ),
            true,
        ),
        Choice::MagazineFormat(_) => (
            within!(
                "i.id",
                "issue i JOIN series s ON s.id = i.series_id",
                "s.magazine_format"
            ),
            true,
        ),
        Choice::Distribution(_) => (
            within!(
                "c.id",
                "copy c JOIN edition e ON e.id = c.edition_id",
                "e.distribution"
            ),
            false,
        ),
        Choice::Binding(_) => (
            within!(
                "c.id",
                "copy c JOIN edition e ON e.id = c.edition_id",
                "e.binding"
            ),
            false,
        ),
        Choice::GradeType(_) => (within!("id", "copy", "grade_type"), false),
        Choice::PageQuality(_) => (within!("id", "copy", "page_quality"), false),
        Choice::BookCondition(_) => (within!("id", "copy", "book_condition"), false),
        Choice::DustJacketCondition(_) => (within!("id", "copy", "dust_jacket_condition"), false),
        Choice::Verification(_) => (within!("copy_id", "signature", "verification"), false),
        Choice::SignaturePlacement(_) => (within!("copy_id", "signature", "placement"), false),
    }
}

/// The codes chosen, as a JSON array of strings.
fn choice_codes(choice: &Choice) -> String {
    fn codes<T: Copy>(items: &[T], code: fn(T) -> &'static str) -> Vec<&'static str> {
        items.iter().map(|i| code(*i)).collect()
    }
    let list = match choice {
        Choice::Category(v) => codes(v, Category::code),
        Choice::MagazineFormat(v) => codes(v, MagazineFormat::code),
        Choice::Distribution(v) => codes(v, Distribution::code),
        Choice::Binding(v) => codes(v, Binding::code),
        Choice::GradeType(v) => codes(v, GradeType::code),
        Choice::PageQuality(v) => codes(v, PageQuality::code),
        Choice::BookCondition(v) | Choice::DustJacketCondition(v) => codes(v, BookCondition::code),
        Choice::Verification(v) => codes(v, Verification::code),
        Choice::SignaturePlacement(v) => codes(v, SignaturePlacement::code),
    };
    json_strings(&list)
}

impl Flag {
    /// Statements for yes and for no, and whether the flag belongs to
    /// the issue.
    fn sql(self) -> (&'static str, &'static str, bool) {
        match self {
            Self::KeyIssue => (
                "SELECT id FROM issue WHERE is_key = 1",
                "SELECT id FROM issue WHERE is_key = 0",
                true,
            ),
            Self::FirstAppearance => (
                "SELECT issue_id FROM issue_character WHERE first_appearance = 1
                 UNION SELECT issue_id FROM issue_team WHERE first_appearance = 1",
                "SELECT id FROM issue WHERE id NOT IN (
                     SELECT issue_id FROM issue_character WHERE first_appearance = 1
                     UNION SELECT issue_id FROM issue_team WHERE first_appearance = 1)",
                true,
            ),
            // These are recorded only for magazines and collected
            // editions; elsewhere they are neither yes nor no.
            Self::BlackAndWhite => (
                "SELECT i.id FROM issue i JOIN series s ON s.id = i.series_id
                 WHERE s.black_and_white = 1",
                "SELECT i.id FROM issue i JOIN series s ON s.id = i.series_id
                 WHERE s.black_and_white = 0",
                true,
            ),
            Self::DustJacket => (
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.has_dust_jacket = 1",
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.has_dust_jacket = 0",
                false,
            ),
            Self::Slipcase => (
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.has_slipcase = 1",
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.has_slipcase = 0",
                false,
            ),
            Self::Signed => (
                "SELECT copy_id FROM signature",
                "SELECT id FROM copy WHERE id NOT IN (SELECT copy_id FROM signature)",
                false,
            ),
            Self::Sketch => (
                "SELECT copy_id FROM signature WHERE sketch = 1",
                "SELECT id FROM copy
                 WHERE id NOT IN (SELECT copy_id FROM signature WHERE sketch = 1)",
                false,
            ),
        }
    }
}

impl Presence {
    /// Statements for has and for has none, and whether it belongs to
    /// the issue.
    fn sql(self) -> (&'static str, &'static str, bool) {
        macro_rules! of_copy {
            ($has:literal) => {
                (
                    $has,
                    concat!("SELECT id FROM copy WHERE id NOT IN (", $has, ")"),
                    false,
                )
            };
        }
        macro_rules! of_issue {
            ($has:literal) => {
                (
                    $has,
                    concat!("SELECT id FROM issue WHERE id NOT IN (", $has, ")"),
                    true,
                )
            };
        }
        match self {
            Self::FrontPhoto => of_copy!("SELECT copy_id FROM cover_photo WHERE side = 'front'"),
            Self::BackPhoto => of_copy!("SELECT copy_id FROM cover_photo WHERE side = 'back'"),
            Self::SignaturePhoto => {
                of_copy!("SELECT copy_id FROM cover_photo WHERE side = 'signature'")
            }
            Self::Value => of_copy!("SELECT copy_id FROM value_entry"),
            Self::Grade => of_copy!(
                "SELECT id FROM copy WHERE grade IS NOT NULL OR book_condition IS NOT NULL"
            ),
            Self::StorageBox => of_copy!("SELECT id FROM copy WHERE box_id IS NOT NULL"),
            Self::PurchasePrice => {
                of_copy!("SELECT id FROM copy WHERE purchase_price_units IS NOT NULL")
            }
            Self::PurchaseDate => of_copy!("SELECT id FROM copy WHERE purchase_date IS NOT NULL"),
            Self::CoverPrice => of_copy!(
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.cover_price_units IS NOT NULL"
            ),
            Self::Tags => of_copy!("SELECT copy_id FROM copy_tag"),
            Self::Notes => of_copy!("SELECT id FROM copy WHERE notes <> ''"),
            Self::CoverDate => of_issue!("SELECT id FROM issue WHERE cover_year IS NOT NULL"),
            Self::ReleaseDate => of_issue!("SELECT id FROM issue WHERE release_date IS NOT NULL"),
        }
    }
}

/// Statements for a list field: the owners linked to any of the ids in
/// `?1`, and those linked to all of them (`?2` being how many).
struct ListSql {
    issue_level: bool,
    any: &'static str,
    all: &'static str,
}

macro_rules! list_sql {
    ($issue_level:literal, $owner:literal, $from:literal, $id:literal) => {
        ListSql {
            issue_level: $issue_level,
            any: select_where!($owner, $from, $id, " IN (SELECT value FROM json_each(?1))"),
            all: concat!(
                select_where!($owner, $from, $id, " IN (SELECT value FROM json_each(?1))"),
                " GROUP BY ",
                $owner,
                " HAVING count(DISTINCT ",
                $id,
                ") = ?2"
            ),
        }
    };
}

impl ListField {
    fn sql(self) -> ListSql {
        match self {
            Self::Characters => list_sql!(true, "issue_id", "issue_character", "character_id"),
            Self::Teams => list_sql!(true, "issue_id", "issue_team", "team_id"),
            Self::StoryArcs => list_sql!(true, "issue_id", "issue_story_arc", "story_arc_id"),
            Self::Tags => list_sql!(false, "copy_id", "copy_tag", "tag_id"),
            Self::SignedBy => list_sql!(false, "copy_id", "signature", "creator_id"),
            Self::CoverArtists => list_sql!(
                false,
                "c.id",
                "edition_cover_artist a JOIN copy c ON c.edition_id = a.edition_id",
                "a.creator_id"
            ),
        }
    }
}

/// Strings as a JSON array.
fn json_strings(items: &[&str]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|s| {
            let mut out = String::from('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        })
        .collect();
    format!("[{}]", quoted.join(","))
}

/// Whether two values are close enough in spelling to be "similar":
/// "Amazng Spider-Man" to "Amazing Spider-Man", "Spiderman" to
/// "Spider-Man", but not "Spider-Man" to "Spider-Man 2099". As many
/// edits are allowed as search allows a mistyped word of that length.
pub(crate) fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (fold(a.trim()), fold(b.trim()));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let longer = a.chars().count().max(b.chars().count());
    osa_distance(&a, &b) <= edits_allowed(longer)
}

/// Orders a range's ends.
fn ordered<T: PartialOrd>(a: T, b: T) -> (T, T) {
    if b < a { (b, a) } else { (a, b) }
}

/// The last sort key any issue number can have.
const AFTER_EVERY_NUMBER: &str = "\u{10FFFF}";

impl Database {
    /// The live copies meeting every rule, in natural order.
    pub fn filter_books(&self, set: &FilterSet) -> Result<Vec<CopyRow>> {
        let tx = self.tx()?;
        let rows = match self.matching_copies(set)? {
            None => query_all(&self.conn, copy_rows!("1"), [], copy_row)?,
            Some(copies) => query_all(
                &self.conn,
                copy_rows!("c.id IN (SELECT value FROM json_each(?1))"),
                [json_ids(copies.iter().map(|c| c.0))],
                copy_row,
            )?,
        };
        tx.commit()?;
        Ok(rows)
    }

    /// The copies meeting every rule, or `None` when the rules leave
    /// every book in. Copies in Trash may be among them; showing them is
    /// left to the caller.
    pub fn matching_copies(&self, set: &FilterSet) -> Result<Option<HashSet<CopyId>>> {
        let mut issues: Option<HashSet<i64>> = None;
        let mut copies: Option<HashSet<i64>> = None;
        let narrow = |kept: &mut Option<HashSet<i64>>, found: HashSet<i64>| {
            *kept = Some(match kept.take() {
                None => found,
                Some(k) => k.intersection(&found).copied().collect(),
            });
        };
        for rule in &set.rules {
            match self.met_by(rule)? {
                Met::Issues(found) => narrow(&mut issues, found),
                Met::Copies(found) => narrow(&mut copies, found),
            }
        }
        let from_issues = match (issues, set.owned_as) {
            (None, OwnedAs::SingleIssue | OwnedAs::Either) => None,
            (None, OwnedAs::CollectedEdition) => Some(self.ids(
                "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
                 WHERE e.issue_id IN (SELECT collection_issue_id FROM collects)",
                [],
            )?),
            (Some(issues), owned_as) => {
                let list = json_ids(issues);
                Some(self.ids(
                    match owned_as {
                        OwnedAs::SingleIssue => SINGLE,
                        OwnedAs::CollectedEdition => COLLECTED,
                        OwnedAs::Either => EITHER,
                    },
                    [list],
                )?)
            }
        };
        if let Some(found) = from_issues {
            narrow(&mut copies, found);
        }
        Ok(copies.map(|c| c.into_iter().map(CopyId).collect()))
    }

    fn ids(&self, sql: &'static str, params: impl Params) -> Result<HashSet<i64>> {
        Ok(
            query_all(&self.conn, sql, params, |row| row.get::<_, i64>(0))?
                .into_iter()
                .collect(),
        )
    }

    fn met(&self, issue_level: bool, sql: &'static str, params: impl Params) -> Result<Met> {
        let found = self.ids(sql, params)?;
        Ok(Met::of(issue_level, found))
    }

    fn met_by(&self, rule: &Filter) -> Result<Met> {
        match rule {
            Filter::Text {
                field,
                condition,
                value,
            } => self.text(*field, *condition, value.trim()),
            Filter::Person { name, role } => {
                let Some(creator) = self.find_lookup::<CreatorId>(name)? else {
                    return Ok(Met::Issues(HashSet::new()));
                };
                match role {
                    None => self.met(
                        true,
                        "SELECT issue_id FROM credit WHERE creator_id = ?1",
                        [creator.raw()],
                    ),
                    Some(role) => self.met(
                        true,
                        "SELECT issue_id FROM credit WHERE creator_id = ?1 AND role = ?2",
                        params![creator.raw(), role],
                    ),
                }
            }
            Filter::List {
                field,
                condition,
                names,
            } => self.list(*field, *condition, names),
            Filter::IssueNumber(compare) => {
                let key = |n: &IssueNumber| n.sort_key().to_owned();
                self.compare(&NUMBER_SQL, compare, key)
            }
            Filter::Grade(compare) => self.compare(&GRADE_SQL, compare, |g| g.tenths()),
            Filter::Number { field, compare } => self.compare(&field.sql(), compare, |n| *n),
            Filter::Money { field, compare } => self.money(*field, compare),
            Filter::Date { field, condition } => self.date(*field, *condition),
            Filter::Choice(choice) => {
                let (sql, issue_level) = choice_sql(choice);
                self.met(issue_level, sql, [choice_codes(choice)])
            }
            Filter::Flag { flag, value } => {
                let (yes, no, issue_level) = flag.sql();
                self.met(issue_level, if *value { yes } else { no }, [])
            }
            Filter::Has { what, has } => {
                let (with, without, issue_level) = what.sql();
                self.met(issue_level, if *has { with } else { without }, [])
            }
            Filter::Collects { series, from, to } => {
                let from = from
                    .as_ref()
                    .map_or(String::new(), |n| n.sort_key().to_owned());
                let to = to
                    .as_ref()
                    .map_or(AFTER_EVERY_NUMBER.to_owned(), |n| n.sort_key().to_owned());
                let (from, to) = ordered(from, to);
                self.met(
                    false,
                    "SELECT c.id FROM collects k
                     JOIN issue r ON r.id = k.collected_issue_id
                     JOIN edition e ON e.issue_id = k.collection_issue_id
                     JOIN copy c ON c.edition_id = e.id
                     WHERE r.series_id = ?1 AND r.number IS NOT NULL
                     AND r.number_sort BETWEEN ?2 AND ?3",
                    params![series, from, to],
                )
            }
        }
    }

    fn text(&self, field: TextField, condition: TextCondition, value: &str) -> Result<Met> {
        let sql = field.sql();
        match condition {
            TextCondition::Contains => self.met(
                sql.issue_level,
                sql.contains,
                [format!("%{}%", like_escaped(value))],
            ),
            TextCondition::Is => self.met(sql.issue_level, sql.is, [value]),
            TextCondition::SimilarTo => {
                let found: HashSet<i64> = query_all(&self.conn, sql.values, [], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .into_iter()
                .filter(|(_, text)| similar(text, value))
                .map(|(id, _)| id)
                .collect();
                Ok(Met::of(sql.issue_level, found))
            }
        }
    }

    fn compare<T, V: rusqlite::ToSql + PartialOrd>(
        &self,
        sql: &CompareSql,
        compare: &Compare<T>,
        value: impl Fn(&T) -> V,
    ) -> Result<Met> {
        let level = sql.issue_level;
        match compare {
            Compare::Equal(v) => self.met(level, sql.equal, [value(v)]),
            Compare::AtLeast(v) => self.met(level, sql.at_least, [value(v)]),
            Compare::AtMost(v) => self.met(level, sql.at_most, [value(v)]),
            Compare::Between(a, b) => {
                let (low, high) = ordered(value(a), value(b));
                self.met(level, sql.between, params![low, high])
            }
        }
    }

    fn money(&self, field: MoneyField, compare: &Compare<Amount>) -> Result<Met> {
        let rates = self.rate_table()?;
        // Every bound in the first bound's currency.
        let target = match compare {
            Compare::Equal(a)
            | Compare::AtLeast(a)
            | Compare::AtMost(a)
            | Compare::Between(a, _) => a.currency(),
        };
        let in_target = |amount: Amount, on: Option<Date>| -> Option<i64> {
            let converted = match on {
                Some(date) => rates.convert(amount, date, target),
                None => rates.convert_at_latest(amount, target),
            };
            converted.ok().map(Amount::units)
        };
        // The bounds as an inclusive range of the target's units.
        let bound = |a: &Amount| in_target(*a, None);
        let range = match compare {
            Compare::Equal(a) => bound(a).map(|v| (v, v)),
            Compare::AtLeast(a) => bound(a).map(|v| (v, i64::MAX)),
            Compare::AtMost(a) => bound(a).map(|v| (i64::MIN, v)),
            Compare::Between(a, b) => bound(a).zip(bound(b)).map(|(a, b)| ordered(a, b)),
        };
        let Some((low, high)) = range else {
            return Ok(Met::Copies(HashSet::new()));
        };
        let rows = query_all(&self.conn, money_sql(field), [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Currency>(2)?,
                row.get::<_, Option<Date>>(3)?,
            ))
        })?;
        Ok(Met::Copies(
            rows.into_iter()
                .filter(|(_, units, currency, on)| {
                    in_target(Amount::new(*currency, *units), *on)
                        .is_some_and(|v| (low..=high).contains(&v))
                })
                .map(|(id, ..)| id)
                .collect(),
        ))
    }

    fn date(&self, field: DateField, condition: DateCondition) -> Result<Met> {
        let sql = field.sql();
        let level = sql.issue_level;
        // Cover dates are compared by month.
        let month = |d: Date| i64::from(d.year()) * 100 + i64::from(d.month());
        let cover = field == DateField::CoverDate;
        match (condition, cover) {
            (DateCondition::Before(d), false) => self.met(level, sql.before, [d]),
            (DateCondition::After(d), false) => self.met(level, sql.after, [d]),
            (DateCondition::Between(a, b), false) => {
                let (a, b) = ordered(a, b);
                self.met(level, sql.between, params![a, b])
            }
            (DateCondition::InYear(y), false) => self.met(
                level,
                sql.between,
                params![format!("{y:04}-01-01"), format!("{y:04}-12-31")],
            ),
            (DateCondition::Before(d), true) => self.met(level, sql.before, [month(d)]),
            (DateCondition::After(d), true) => self.met(level, sql.after, [month(d)]),
            (DateCondition::Between(a, b), true) => {
                let (a, b) = ordered(a, b);
                self.met(level, sql.between, params![month(a), month(b)])
            }
            (DateCondition::InYear(y), true) => {
                let y = i64::from(y);
                self.met(level, sql.between, params![y * 100 + 1, y * 100 + 12])
            }
        }
    }

    fn list(&self, field: ListField, condition: ListCondition, names: &[String]) -> Result<Met> {
        let ids = match field {
            ListField::Characters => self.lookup_ids::<CharacterId>(names)?,
            ListField::Teams => self.lookup_ids::<TeamId>(names)?,
            ListField::StoryArcs => self.lookup_ids::<StoryArcId>(names)?,
            ListField::Tags => self.lookup_ids::<TagId>(names)?,
            ListField::CoverArtists | ListField::SignedBy => self.lookup_ids::<CreatorId>(names)?,
        };
        let sql = field.sql();
        let level = sql.issue_level;
        let known: Vec<i64> = ids.iter().flatten().copied().collect();
        match condition {
            ListCondition::AnyOf => self.met(level, sql.any, [json_ids(known)]),
            ListCondition::AllOf => {
                // A name the collection does not know is met by nothing.
                if ids.iter().any(Option::is_none) {
                    return Ok(Met::of(level, HashSet::new()));
                }
                let wanted = known.iter().collect::<HashSet<_>>().len();
                if wanted == 0 {
                    return self.everything(level);
                }
                self.met(
                    level,
                    sql.all,
                    params![json_ids(known), i64::try_from(wanted).unwrap_or(i64::MAX)],
                )
            }
            ListCondition::NoneOf => {
                let excluded = self.ids(sql.any, [json_ids(known)])?;
                let all = self.all_ids(level)?;
                let kept = all.difference(&excluded).copied().collect();
                Ok(Met::of(level, kept))
            }
        }
    }

    fn everything(&self, issue_level: bool) -> Result<Met> {
        let all = self.all_ids(issue_level)?;
        Ok(Met::of(issue_level, all))
    }

    /// Every issue, or every copy.
    fn all_ids(&self, issue_level: bool) -> Result<HashSet<i64>> {
        if issue_level {
            self.ids("SELECT id FROM issue", [])
        } else {
            self.ids("SELECT id FROM copy", [])
        }
    }

    /// Each name's entry, by its name or an alternate name, or `None`
    /// where the collection has none.
    fn lookup_ids<I: LookupId>(&self, names: &[String]) -> Result<Vec<Option<i64>>> {
        names
            .iter()
            .filter(|n| !n.trim().is_empty())
            .map(|n| Ok(self.find_lookup::<I>(n)?.map(|id| id.raw())))
            .collect()
    }
}

/// Issues to their own copies.
const SINGLE: &str = "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
     WHERE e.issue_id IN (SELECT value FROM json_each(?1))";
/// Issues to the copies of collected editions reprinting them.
const COLLECTED: &str = "SELECT c.id FROM collects k
     JOIN edition e ON e.issue_id = k.collection_issue_id
     JOIN copy c ON c.edition_id = e.id
     WHERE k.collected_issue_id IN (SELECT value FROM json_each(?1))";
/// Both.
const EITHER: &str = "SELECT c.id FROM copy c JOIN edition e ON e.id = c.edition_id
     WHERE e.issue_id IN (SELECT value FROM json_each(?1))
     UNION SELECT c.id FROM collects k
     JOIN edition e ON e.issue_id = k.collection_issue_id
     JOIN copy c ON c.edition_id = e.id
     WHERE k.collected_issue_id IN (SELECT value FROM json_each(?1))";
