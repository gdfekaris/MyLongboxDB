//! Browsing: the books on each sidebar shelf, and the entries of the name
//! lists that lead to them.

use rusqlite::{Row, ToSql};

use crate::catalog::{amount_at, cover_date_at, query_all, series_label};
use crate::date::Date;
use crate::db::Database;
use crate::error::Result;
use crate::model::{
    Binding, BookCondition, BoxId, Category, CharacterId, CopyId, CoverDate, CreatorId, Grade,
    GradeType, IssueId, PublisherId, SeriesId, StoryArcId, TeamId,
};
use crate::money::Amount;

/// A set of books to browse: a whole section, or everything linked to
/// one entry of a name list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    Everything,
    Category(Category),
    KeyIssues,
    /// Copies graded by a grading company (CGC, CBCS, or PGX), rather
    /// than given a grade by eye.
    Graded,
    /// Copies with at least one signature.
    Signed,
    Series(SeriesId),
    Publisher(PublisherId),
    /// Credited on the issue, cover artist of the edition, signer of the
    /// copy, or credited on an issue the book reprints.
    Creator(CreatorId),
    /// In the issue, or in an issue the book reprints; likewise for teams
    /// and story arcs.
    Character(CharacterId),
    Team(TeamId),
    StoryArc(StoryArcId),
    /// In box order, with copies that have no position last.
    StorageBox(BoxId),
    /// Copies of every edition owned more than once.
    Duplicates,
    /// Copies missing a cover photo, front or back, a grade (or a book
    /// condition), a storage box, or a value: those whose
    /// [`CopyRow::needs`] is not empty.
    NeedsAttention,
}

/// A sidebar section that lists names, each leading to a [`Shelf`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShelfList {
    Series,
    Publishers,
    Creators,
    Characters,
    Teams,
    StoryArcs,
    StorageBoxes,
}

/// One entry in a [`ShelfList`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfEntry {
    pub shelf: Shelf,
    pub name: String,
    /// The publisher, for a series; otherwise empty.
    pub detail: String,
    /// How many books the shelf holds. Entries with none are listed too,
    /// such as a series known only because a collected edition reprints it.
    pub copies: u32,
}

/// One physical book, with what a results list or grid shows of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyRow {
    pub copy: CopyId,
    pub issue: IssueId,
    pub category: Category,
    /// The series name with its start year, e.g. `Amazing Spider-Man (1963)`.
    pub series: String,
    pub number: Option<String>,
    pub title: String,
    /// Empty for the standard cover.
    pub variant: String,
    pub printing: Option<u32>,
    pub cover_date: Option<CoverDate>,
    pub binding: Option<Binding>,
    pub grade: Option<Grade>,
    pub grade_type: Option<GradeType>,
    pub book_condition: Option<BookCondition>,
    pub storage_box: Option<String>,
    pub box_position: Option<u32>,
    /// The newest value entry, if any.
    pub value: Option<Amount>,
    pub is_key: bool,
    pub signed: bool,
    /// The date of the newest value entry.
    pub value_date: Option<Date>,
    pub purchase_price: Option<Amount>,
    pub purchase_date: Option<Date>,
    /// When the copy was added, as a UTC timestamp.
    pub added_at: String,
    pub has_front_photo: bool,
    pub has_back_photo: bool,
}

/// Something a copy is missing that a complete record has: what puts it
/// on the Needs Attention shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    FrontPhoto,
    BackPhoto,
    /// A grade, or for a collected edition a book condition.
    Grade,
    StorageBox,
    Value,
}

impl CopyRow {
    /// What the copy is missing, in the order a reader would fill it in.
    /// The Needs Attention shelf holds exactly the copies that miss
    /// something.
    pub fn needs(&self) -> Vec<Need> {
        let mut needs = Vec::new();
        if !self.has_front_photo {
            needs.push(Need::FrontPhoto);
        }
        if !self.has_back_photo {
            needs.push(Need::BackPhoto);
        }
        if self.grade.is_none() && self.book_condition.is_none() {
            needs.push(Need::Grade);
        }
        if self.storage_box.is_none() {
            needs.push(Need::StorageBox);
        }
        if self.value.is_none() {
            needs.push(Need::Value);
        }
        needs
    }

    /// e.g. `Amazing Spider-Man (1963) #121`, or the book's own title for a
    /// standalone collected edition.
    pub fn label(&self) -> String {
        match (&self.number, self.title.as_str()) {
            (Some(number), _) => format!("{} #{number}", self.series),
            (None, "") => self.series.clone(),
            (None, title) => format!("{}: {title}", self.series),
        }
    }
}

/// The joins every shelf starts from, keeping only books outside Trash.
/// Trashing a series, issue, or edition marks every copy beneath it too,
/// so the copy's own mark is enough.
macro_rules! live_copies {
    () => {
        "copy c
         JOIN edition e ON e.id = c.edition_id
         JOIN issue i ON i.id = e.issue_id
         JOIN series s ON s.id = i.series_id
         LEFT JOIN storage_box b ON b.id = c.box_id
         WHERE c.trash_id IS NULL"
    };
}

/// The live copies of every issue linked through `$table`, and of the
/// collected editions that reprint those issues, so a character's shelf
/// also holds the books they appear in. The arms are joined by `$union`,
/// and `$owners` is a condition on the linked entry's id, as in
/// [`shelf_pairs`].
macro_rules! linked_copies {
    ($table:literal, $column:literal, $union:literal, $owners:literal) => {
        concat!(
            "SELECT l.",
            $column,
            " AS who, c.id AS copy FROM ",
            $table,
            " l
             JOIN edition e ON e.issue_id = l.issue_id
             JOIN copy c ON c.edition_id = e.id
             WHERE +c.trash_id IS NULL AND l.",
            $column,
            " ",
            $owners,
            " ",
            $union,
            " SELECT l.",
            $column,
            ", c.id FROM ",
            $table,
            " l
             JOIN collects k ON k.collected_issue_id = l.issue_id
             JOIN edition e ON e.issue_id = k.collection_issue_id
             JOIN copy c ON c.edition_id = e.id
             WHERE +c.trash_id IS NULL AND l.",
            $column,
            " ",
            $owners
        )
    };
}

/// Pairs each entry of a name list (`who`) with a book on its shelf
/// (`copy`), once per book. Both a shelf and its count in the list read
/// these pairs, so an entry never promises more books than its shelf shows.
///
/// Search passes two more arguments: `UNION ALL`, since it keeps only
/// the best score for each book and need not pay to remove repeats, and a
/// condition picking the entries it found, which then applies inside
/// every arm rather than after them all.
///
/// The `+` before `trash_id` keeps SQLite from starting at the index of
/// trashed copies, which matches nearly every copy, and makes it start
/// from the entry's links instead.
macro_rules! shelf_pairs {
    ($kind:ident) => {
        shelf_pairs!($kind, "UNION", "IS NOT NULL")
    };
    (series, $union:literal, $owners:literal) => {
        concat!(
            "SELECT i.series_id AS who, c.id AS copy FROM copy c
             JOIN edition e ON e.id = c.edition_id
             JOIN issue i ON i.id = e.issue_id
             WHERE +c.trash_id IS NULL AND i.series_id ",
            $owners
        )
    };
    (publisher, $union:literal, $owners:literal) => {
        concat!(
            "SELECT s.publisher_id AS who, c.id AS copy FROM copy c
             JOIN edition e ON e.id = c.edition_id
             JOIN issue i ON i.id = e.issue_id
             JOIN series s ON s.id = i.series_id
             WHERE +c.trash_id IS NULL AND s.publisher_id ",
            $owners
        )
    };
    (creator, $union:literal, $owners:literal) => {
        concat!(
            linked_copies!("credit", "creator_id", $union, $owners),
            " ",
            $union,
            " SELECT a.creator_id, c.id FROM edition_cover_artist a
             JOIN copy c ON c.edition_id = a.edition_id
             WHERE +c.trash_id IS NULL AND a.creator_id ",
            $owners,
            " ",
            $union,
            " SELECT s.creator_id, c.id FROM signature s
             JOIN copy c ON c.id = s.copy_id
             WHERE +c.trash_id IS NULL AND s.creator_id ",
            $owners
        )
    };
    (character, $union:literal, $owners:literal) => {
        linked_copies!("issue_character", "character_id", $union, $owners)
    };
    (team, $union:literal, $owners:literal) => {
        linked_copies!("issue_team", "team_id", $union, $owners)
    };
    (story_arc, $union:literal, $owners:literal) => {
        linked_copies!("issue_story_arc", "story_arc_id", $union, $owners)
    };
    (storage_box, $union:literal, $owners:literal) => {
        concat!(
            "SELECT box_id AS who, id AS copy FROM copy
             WHERE +trash_id IS NULL AND box_id ",
            $owners
        )
    };
}

/// Keeps the books on the shelf of entry `?1`.
macro_rules! on_shelf {
    ($kind:ident) => {
        concat!(
            "c.id IN (SELECT copy FROM (",
            shelf_pairs!($kind),
            ") WHERE who = ?1)"
        )
    };
}

/// The editions of one issue, standard ones first and each variant's
/// together after them, then by printing (none recorded counts as the
/// first) and distribution: direct, newsstand, then unrecorded.
macro_rules! edition_order {
    () => {
        "e.variant <> '', e.variant COLLATE NOCASE, coalesce(e.printing, 1),
         CASE e.distribution WHEN 'direct' THEN 0 WHEN 'newsstand' THEN 1 ELSE 2 END, e.id"
    };
}

/// The copies of one edition: raw before professionally graded, then
/// unsigned before signed, then in the order they were added.
macro_rules! copy_order {
    () => {
        "coalesce(c.grade_type IN ('cgc', 'cbcs', 'pgx'), 0),
         EXISTS (SELECT 1 FROM signature g WHERE g.copy_id = c.id), c.id"
    };
}

/// Natural order: by series, then issue number (so 2 comes before 10),
/// then each issue's editions and copies in their own order.
macro_rules! natural_order {
    () => {
        concat!(
            "s.name COLLATE NOCASE, s.start_year, s.volume, s.id, i.number_sort,
             i.title COLLATE NOCASE, i.id, ",
            edition_order!(),
            ", ",
            copy_order!()
        )
    };
}

macro_rules! copy_rows {
    ($filter:expr) => {
        copy_rows!($filter, natural_order!())
    };
    ($filter:expr, $order:expr) => {
        concat!(
            "SELECT c.id, i.id, s.category, s.name, s.start_year, i.number, i.title,
                 e.variant, e.printing, i.cover_year, i.cover_month, e.binding, c.grade,
                 c.grade_type, c.book_condition, b.name, c.box_position,
                 (SELECT value_units FROM value_entry v WHERE v.copy_id = c.id
                  ORDER BY v.date DESC, v.id DESC LIMIT 1),
                 (SELECT value_currency FROM value_entry v WHERE v.copy_id = c.id
                  ORDER BY v.date DESC, v.id DESC LIMIT 1),
                 i.is_key,
                 EXISTS (SELECT 1 FROM signature g WHERE g.copy_id = c.id),
                 (SELECT date FROM value_entry v WHERE v.copy_id = c.id
                  ORDER BY v.date DESC, v.id DESC LIMIT 1),
                 c.purchase_price_units, c.purchase_price_currency, c.purchase_date,
                 c.added_at,
                 EXISTS (SELECT 1 FROM cover_photo p WHERE p.copy_id = c.id AND p.side = 'front'),
                 EXISTS (SELECT 1 FROM cover_photo p WHERE p.copy_id = c.id AND p.side = 'back'),
                 s.volume
             FROM ",
            live_copies!(),
            " AND ",
            $filter,
            " ORDER BY ",
            $order
        )
    };
}

/// Lists `$table` by name, counting each entry's books.
macro_rules! shelf_counts {
    ($kind:ident, $table:literal) => {
        concat!(
            "SELECT x.id, x.name, '', count(m.copy) FROM ",
            $table,
            " x LEFT JOIN (",
            shelf_pairs!($kind),
            ") m ON m.who = x.id GROUP BY x.id ORDER BY x.name COLLATE NOCASE, x.id"
        )
    };
}

impl Database {
    /// The books on a shelf, in natural order.
    pub fn browse(&self, shelf: Shelf) -> Result<Vec<CopyRow>> {
        let conn = &self.conn;
        let query = |sql: &str, id: &dyn ToSql| query_all(conn, sql, [id], copy_row);
        match shelf {
            Shelf::Everything => query_all(conn, copy_rows!("1"), (), copy_row),
            Shelf::Category(category) => query(copy_rows!("s.category = ?1"), &category),
            Shelf::KeyIssues => query_all(conn, copy_rows!("i.is_key = 1"), (), copy_row),
            Shelf::Graded => query_all(
                conn,
                copy_rows!("c.grade_type IN ('cgc', 'cbcs', 'pgx')"),
                (),
                copy_row,
            ),
            Shelf::Signed => query_all(
                conn,
                copy_rows!("EXISTS (SELECT 1 FROM signature g WHERE g.copy_id = c.id)"),
                (),
                copy_row,
            ),
            Shelf::Duplicates => query_all(
                conn,
                copy_rows!(
                    "c.edition_id IN (SELECT edition_id FROM copy WHERE trash_id IS NULL
                     GROUP BY edition_id HAVING count(*) > 1)"
                ),
                (),
                copy_row,
            ),
            Shelf::NeedsAttention => query_all(
                conn,
                copy_rows!(
                    "(NOT EXISTS (SELECT 1 FROM cover_photo p
                                  WHERE p.copy_id = c.id AND p.side = 'front')
                      OR NOT EXISTS (SELECT 1 FROM cover_photo p
                                     WHERE p.copy_id = c.id AND p.side = 'back')
                      OR (c.grade IS NULL AND c.book_condition IS NULL)
                      OR c.box_id IS NULL
                      OR NOT EXISTS (SELECT 1 FROM value_entry v WHERE v.copy_id = c.id))"
                ),
                (),
                copy_row,
            ),
            Shelf::Series(id) => query(copy_rows!(on_shelf!(series)), &id),
            Shelf::Publisher(id) => query(copy_rows!(on_shelf!(publisher)), &id),
            Shelf::Creator(id) => query(copy_rows!(on_shelf!(creator)), &id),
            Shelf::Character(id) => query(copy_rows!(on_shelf!(character)), &id),
            Shelf::Team(id) => query(copy_rows!(on_shelf!(team)), &id),
            Shelf::StoryArc(id) => query(copy_rows!(on_shelf!(story_arc)), &id),
            Shelf::StorageBox(id) => query(
                copy_rows!(
                    on_shelf!(storage_box),
                    concat!("c.box_position IS NULL, c.box_position, ", natural_order!())
                ),
                &id,
            ),
        }
    }

    /// Every entry of a name list, by name, with how many books each holds.
    pub fn shelf_list(&self, list: ShelfList) -> Result<Vec<ShelfEntry>> {
        let conn = &self.conn;
        let entries =
            |sql: &str, shelf: fn(i64) -> Shelf| query_all(conn, sql, (), |row| entry(row, shelf));
        match list {
            ShelfList::Series => query_all(
                conn,
                concat!(
                    "SELECT x.id, x.name, coalesce(p.name, ''), count(m.copy), x.start_year,
                         x.volume
                     FROM series x LEFT JOIN publisher p ON p.id = x.publisher_id
                     LEFT JOIN (",
                    shelf_pairs!(series),
                    ") m ON m.who = x.id
                     WHERE x.trash_id IS NULL
                     GROUP BY x.id
                     ORDER BY x.name COLLATE NOCASE, x.start_year, x.volume, x.id"
                ),
                (),
                |row| {
                    let mut e = entry(row, |id| Shelf::Series(SeriesId(id)))?;
                    e.name = series_label(&e.name, row.get(4)?, row.get(5)?);
                    Ok(e)
                },
            ),
            ShelfList::Publishers => entries(shelf_counts!(publisher, "publisher"), |id| {
                Shelf::Publisher(PublisherId(id))
            }),
            ShelfList::Creators => entries(shelf_counts!(creator, "creator"), |id| {
                Shelf::Creator(CreatorId(id))
            }),
            ShelfList::Characters => entries(shelf_counts!(character, "character"), |id| {
                Shelf::Character(CharacterId(id))
            }),
            ShelfList::Teams => entries(shelf_counts!(team, "team"), |id| Shelf::Team(TeamId(id))),
            ShelfList::StoryArcs => entries(shelf_counts!(story_arc, "story_arc"), |id| {
                Shelf::StoryArc(StoryArcId(id))
            }),
            ShelfList::StorageBoxes => entries(shelf_counts!(storage_box, "storage_box"), |id| {
                Shelf::StorageBox(BoxId(id))
            }),
        }
    }
}

fn entry(row: &Row<'_>, shelf: fn(i64) -> Shelf) -> rusqlite::Result<ShelfEntry> {
    Ok(ShelfEntry {
        shelf: shelf(row.get(0)?),
        name: row.get(1)?,
        detail: row.get(2)?,
        copies: row.get(3)?,
    })
}

pub(crate) fn copy_row(row: &Row<'_>) -> rusqlite::Result<CopyRow> {
    Ok(CopyRow {
        copy: row.get(0)?,
        issue: row.get(1)?,
        category: row.get(2)?,
        series: series_label(&row.get::<_, String>(3)?, row.get(4)?, row.get(28)?),
        number: row.get(5)?,
        title: row.get(6)?,
        variant: row.get(7)?,
        printing: row.get(8)?,
        cover_date: cover_date_at(row, 9, 10)?,
        binding: row.get(11)?,
        grade: row.get(12)?,
        grade_type: row.get(13)?,
        book_condition: row.get(14)?,
        storage_box: row.get(15)?,
        box_position: row.get(16)?,
        value: amount_at(row, 17, 18)?,
        is_key: row.get(19)?,
        signed: row.get(20)?,
        value_date: row.get(21)?,
        purchase_price: amount_at(row, 22, 23)?,
        purchase_date: row.get(24)?,
        added_at: row.get(25)?,
        has_front_photo: row.get(26)?,
        has_back_photo: row.get(27)?,
    })
}
