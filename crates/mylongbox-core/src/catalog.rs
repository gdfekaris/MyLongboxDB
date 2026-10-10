//! Creating, reading, updating, and trashing series, issues, editions,
//! and copies.

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{
    Category, Copy, CopyFields, CopyId, CoverDate, Edition, EditionFields, EditionId, Issue,
    IssueFields, IssueId, RowUuid, Series, SeriesFields, SeriesId, TrashGroup, TrashId, invalid,
};
use crate::money::{Amount, Currency};

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_series {
    ($rest:literal) => {
        concat!(
            "SELECT id, uuid, trash_id, name, category, publisher_id, volume,
    start_year, known_issue_count, magazine_format, black_and_white, notes FROM series",
            " ",
            $rest
        )
    };
}

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_issue {
    ($rest:literal) => {
        concat!(
            "SELECT id, uuid, series_id, trash_id, number, title, cover_year,
    cover_month, release_date, page_count, synopsis, is_key, key_reason FROM issue",
            " ",
            $rest
        )
    };
}

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_edition {
    ($rest:literal) => {
        concat!(
            "SELECT id, uuid, issue_id, trash_id, variant, printing,
    distribution, cover_price_units, cover_price_currency, barcode, isbn, binding,
    has_dust_jacket, has_slipcase FROM edition",
            " ",
            $rest
        )
    };
}

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_copy {
    ($rest:literal) => {
        concat!(
            "SELECT id, uuid, edition_id, trash_id, added_at, grade, grade_type,
    cert_number, label_type, book_condition, dust_jacket_condition, signed_by,
    purchase_price_units, purchase_price_currency, purchase_date, seller, box_id,
    box_position, notes FROM copy",
            " ",
            $rest
        )
    };
}

impl Database {
    // Series ---------------------------------------------------------------

    pub fn create_series(&self, fields: &SeriesFields) -> Result<SeriesId> {
        let f = clean_series(fields)?;
        self.conn.execute(
            "INSERT INTO series (uuid, name, category, publisher_id, volume, start_year,
                 known_issue_count, magazine_format, black_and_white, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                RowUuid::new_random(),
                f.name,
                f.category,
                f.publisher,
                f.volume,
                f.start_year,
                f.known_issue_count,
                f.magazine_format,
                f.black_and_white,
                f.notes,
            ],
        )?;
        Ok(SeriesId(self.conn.last_insert_rowid()))
    }

    /// A series, whether live or in Trash.
    pub fn series(&self, id: SeriesId) -> Result<Series> {
        query_one(
            &self.conn,
            select_series!("WHERE id = ?1"),
            [id],
            series_from_row,
        )
    }

    pub fn update_series(&self, id: SeriesId, fields: &SeriesFields) -> Result<()> {
        let f = clean_series(fields)?;
        let tx = self.conn.unchecked_transaction()?;
        live(self.series(id)?.trash)?;
        check_category_fits_children(&tx, id, f.category)?;
        tx.execute(
            "UPDATE series SET name = ?2, category = ?3, publisher_id = ?4, volume = ?5,
                 start_year = ?6, known_issue_count = ?7, magazine_format = ?8,
                 black_and_white = ?9, notes = ?10
             WHERE id = ?1",
            params![
                id,
                f.name,
                f.category,
                f.publisher,
                f.volume,
                f.start_year,
                f.known_issue_count,
                f.magazine_format,
                f.black_and_white,
                f.notes,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Every series not in Trash, by name.
    pub fn list_series(&self) -> Result<Vec<Series>> {
        query_all(
            &self.conn,
            select_series!(
                "WHERE trash_id IS NULL
                 ORDER BY name COLLATE NOCASE, start_year, volume, id"
            ),
            (),
            series_from_row,
        )
    }

    // Issues ---------------------------------------------------------------

    pub fn create_issue(&self, series: SeriesId, fields: &IssueFields) -> Result<IssueId> {
        let tx = self.conn.unchecked_transaction()?;
        let parent = self.series(series)?;
        live(parent.trash)?;
        let f = clean_issue(fields, parent.fields.category)?;
        check_number_free(&tx, series, &f, None)?;
        tx.execute(
            "INSERT INTO issue (uuid, series_id, number, number_sort, title, cover_year,
                 cover_month, release_date, page_count, synopsis, is_key, key_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                RowUuid::new_random(),
                series,
                f.number,
                number_sort(&f),
                f.title,
                f.cover_date.map(CoverDate::year),
                f.cover_date.and_then(CoverDate::month),
                f.release_date,
                f.page_count,
                f.synopsis,
                f.is_key,
                f.key_reason,
            ],
        )?;
        let id = IssueId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }

    /// An issue, whether live or in Trash.
    pub fn issue(&self, id: IssueId) -> Result<Issue> {
        query_one(
            &self.conn,
            select_issue!("WHERE id = ?1"),
            [id],
            issue_from_row,
        )
    }

    pub fn update_issue(&self, id: IssueId, fields: &IssueFields) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let current = self.issue(id)?;
        live(current.trash)?;
        let f = clean_issue(fields, self.series(current.series)?.fields.category)?;
        check_number_free(&tx, current.series, &f, Some(id))?;
        tx.execute(
            "UPDATE issue SET number = ?2, number_sort = ?3, title = ?4, cover_year = ?5,
                 cover_month = ?6, release_date = ?7, page_count = ?8, synopsis = ?9,
                 is_key = ?10, key_reason = ?11
             WHERE id = ?1",
            params![
                id,
                f.number,
                number_sort(&f),
                f.title,
                f.cover_date.map(CoverDate::year),
                f.cover_date.and_then(CoverDate::month),
                f.release_date,
                f.page_count,
                f.synopsis,
                f.is_key,
                f.key_reason,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The live issues of a series in natural number order.
    pub fn issues_in_series(&self, series: SeriesId) -> Result<Vec<Issue>> {
        query_all(
            &self.conn,
            select_issue!(
                "WHERE series_id = ?1 AND trash_id IS NULL
                 ORDER BY number_sort, id"
            ),
            [series],
            issue_from_row,
        )
    }

    // Editions -------------------------------------------------------------

    pub fn create_edition(&self, issue: IssueId, fields: &EditionFields) -> Result<EditionId> {
        let parent = self.issue(issue)?;
        live(parent.trash)?;
        let f = clean_edition(fields, self.series(parent.series)?.fields.category)?;
        self.conn.execute(
            "INSERT INTO edition (uuid, issue_id, variant, printing, distribution,
                 cover_price_units, cover_price_currency, barcode, isbn, binding,
                 has_dust_jacket, has_slipcase)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                RowUuid::new_random(),
                issue,
                f.variant,
                f.printing,
                f.distribution,
                f.cover_price.map(Amount::units),
                f.cover_price.map(Amount::currency),
                f.barcode,
                f.isbn,
                f.binding,
                f.has_dust_jacket,
                f.has_slipcase,
            ],
        )?;
        Ok(EditionId(self.conn.last_insert_rowid()))
    }

    /// An edition, whether live or in Trash.
    pub fn edition(&self, id: EditionId) -> Result<Edition> {
        query_one(
            &self.conn,
            select_edition!("WHERE id = ?1"),
            [id],
            edition_from_row,
        )
    }

    pub fn update_edition(&self, id: EditionId, fields: &EditionFields) -> Result<()> {
        let current = self.edition(id)?;
        live(current.trash)?;
        let f = clean_edition(fields, self.category_of_issue(current.issue)?)?;
        self.conn.execute(
            "UPDATE edition SET variant = ?2, printing = ?3, distribution = ?4,
                 cover_price_units = ?5, cover_price_currency = ?6, barcode = ?7, isbn = ?8,
                 binding = ?9, has_dust_jacket = ?10, has_slipcase = ?11
             WHERE id = ?1",
            params![
                id,
                f.variant,
                f.printing,
                f.distribution,
                f.cover_price.map(Amount::units),
                f.cover_price.map(Amount::currency),
                f.barcode,
                f.isbn,
                f.binding,
                f.has_dust_jacket,
                f.has_slipcase,
            ],
        )?;
        Ok(())
    }

    /// The live editions of an issue, oldest first.
    pub fn editions_of_issue(&self, issue: IssueId) -> Result<Vec<Edition>> {
        query_all(
            &self.conn,
            select_edition!("WHERE issue_id = ?1 AND trash_id IS NULL ORDER BY id"),
            [issue],
            edition_from_row,
        )
    }

    // Copies ---------------------------------------------------------------

    pub fn create_copy(&self, edition: EditionId, fields: &CopyFields) -> Result<CopyId> {
        let parent = self.edition(edition)?;
        live(parent.trash)?;
        let f = clean_copy(fields, self.category_of_issue(parent.issue)?)?;
        self.conn.execute(
            "INSERT INTO copy (uuid, edition_id, grade, grade_type, cert_number, label_type,
                 book_condition, dust_jacket_condition, signed_by, purchase_price_units,
                 purchase_price_currency, purchase_date, seller, box_id, box_position, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                RowUuid::new_random(),
                edition,
                f.grade,
                f.grade_type,
                f.cert_number,
                f.label_type,
                f.book_condition,
                f.dust_jacket_condition,
                f.signed_by,
                f.purchase_price.map(Amount::units),
                f.purchase_price.map(Amount::currency),
                f.purchase_date,
                f.seller,
                f.storage_box,
                f.box_position,
                f.notes,
            ],
        )?;
        Ok(CopyId(self.conn.last_insert_rowid()))
    }

    /// A copy, whether live or in Trash.
    pub fn copy(&self, id: CopyId) -> Result<Copy> {
        query_one(
            &self.conn,
            select_copy!("WHERE id = ?1"),
            [id],
            copy_from_row,
        )
    }

    pub fn update_copy(&self, id: CopyId, fields: &CopyFields) -> Result<()> {
        let current = self.copy(id)?;
        live(current.trash)?;
        let issue = self.edition(current.edition)?.issue;
        let f = clean_copy(fields, self.category_of_issue(issue)?)?;
        self.conn.execute(
            "UPDATE copy SET grade = ?2, grade_type = ?3, cert_number = ?4, label_type = ?5,
                 book_condition = ?6, dust_jacket_condition = ?7, signed_by = ?8,
                 purchase_price_units = ?9, purchase_price_currency = ?10, purchase_date = ?11,
                 seller = ?12, box_id = ?13, box_position = ?14, notes = ?15
             WHERE id = ?1",
            params![
                id,
                f.grade,
                f.grade_type,
                f.cert_number,
                f.label_type,
                f.book_condition,
                f.dust_jacket_condition,
                f.signed_by,
                f.purchase_price.map(Amount::units),
                f.purchase_price.map(Amount::currency),
                f.purchase_date,
                f.seller,
                f.storage_box,
                f.box_position,
                f.notes,
            ],
        )?;
        Ok(())
    }

    /// The live copies of an edition, in the order they were added.
    pub fn copies_of_edition(&self, edition: EditionId) -> Result<Vec<Copy>> {
        query_all(
            &self.conn,
            select_copy!("WHERE edition_id = ?1 AND trash_id IS NULL ORDER BY id"),
            [edition],
            copy_from_row,
        )
    }

    // Trash ----------------------------------------------------------------

    /// Moves a series and everything in it to Trash as one group.
    pub fn trash_series(&self, id: SeriesId) -> Result<TrashId> {
        let tx = self.conn.unchecked_transaction()?;
        let series = self.series(id)?;
        live(series.trash)?;
        let trash = new_trash_group(&tx, &series_label(&series))?;
        tx.execute(
            "UPDATE copy SET trash_id = ?1 WHERE trash_id IS NULL AND edition_id IN (
                 SELECT edition.id FROM edition JOIN issue ON issue.id = edition.issue_id
                 WHERE issue.series_id = ?2)",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE edition SET trash_id = ?1 WHERE trash_id IS NULL AND issue_id IN (
                 SELECT id FROM issue WHERE series_id = ?2)",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE issue SET trash_id = ?1 WHERE trash_id IS NULL AND series_id = ?2",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE series SET trash_id = ?1 WHERE id = ?2",
            params![trash, id],
        )?;
        tx.commit()?;
        Ok(trash)
    }

    /// Moves an issue, its editions, and their copies to Trash as one group.
    pub fn trash_issue(&self, id: IssueId) -> Result<TrashId> {
        let tx = self.conn.unchecked_transaction()?;
        let issue = self.issue(id)?;
        live(issue.trash)?;
        let label = issue_label(&self.series(issue.series)?, &issue);
        let trash = new_trash_group(&tx, &label)?;
        tx.execute(
            "UPDATE copy SET trash_id = ?1 WHERE trash_id IS NULL AND edition_id IN (
                 SELECT id FROM edition WHERE issue_id = ?2)",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE edition SET trash_id = ?1 WHERE trash_id IS NULL AND issue_id = ?2",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE issue SET trash_id = ?1 WHERE id = ?2",
            params![trash, id],
        )?;
        tx.commit()?;
        Ok(trash)
    }

    /// Moves an edition and its copies to Trash as one group.
    pub fn trash_edition(&self, id: EditionId) -> Result<TrashId> {
        let tx = self.conn.unchecked_transaction()?;
        let edition = self.edition(id)?;
        live(edition.trash)?;
        let label = self.edition_label(&edition)?;
        let trash = new_trash_group(&tx, &label)?;
        tx.execute(
            "UPDATE copy SET trash_id = ?1 WHERE trash_id IS NULL AND edition_id = ?2",
            params![trash, id],
        )?;
        tx.execute(
            "UPDATE edition SET trash_id = ?1 WHERE id = ?2",
            params![trash, id],
        )?;
        tx.commit()?;
        Ok(trash)
    }

    /// Moves one copy to Trash.
    pub fn trash_copy(&self, id: CopyId) -> Result<TrashId> {
        let tx = self.conn.unchecked_transaction()?;
        let copy = self.copy(id)?;
        live(copy.trash)?;
        let label = self.edition_label(&self.edition(copy.edition)?)?;
        let trash = new_trash_group(&tx, &label)?;
        tx.execute(
            "UPDATE copy SET trash_id = ?1 WHERE id = ?2",
            params![trash, id],
        )?;
        tx.commit()?;
        Ok(trash)
    }

    /// Everything in Trash, most recently deleted first.
    pub fn trash_groups(&self) -> Result<Vec<TrashGroup>> {
        query_all(
            &self.conn,
            "SELECT id, trashed_at, label FROM trash ORDER BY trashed_at DESC, id DESC",
            (),
            |row| {
                Ok(TrashGroup {
                    id: row.get(0)?,
                    trashed_at: row.get(1)?,
                    label: row.get(2)?,
                })
            },
        )
    }

    /// Puts a group back exactly where it was.
    pub fn restore(&self, trash: TrashId) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        require_trash_group(&tx, trash)?;
        let parent: Option<String> = tx
            .query_row(
                "SELECT t.label FROM issue i
                     JOIN series s ON s.id = i.series_id JOIN trash t ON t.id = s.trash_id
                     WHERE i.trash_id = ?1 AND s.trash_id <> ?1
                 UNION ALL
                 SELECT t.label FROM edition e
                     JOIN issue i ON i.id = e.issue_id JOIN trash t ON t.id = i.trash_id
                     WHERE e.trash_id = ?1 AND i.trash_id <> ?1
                 UNION ALL
                 SELECT t.label FROM copy c
                     JOIN edition e ON e.id = c.edition_id JOIN trash t ON t.id = e.trash_id
                     WHERE c.trash_id = ?1 AND e.trash_id <> ?1
                 LIMIT 1",
                [trash],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(parent) = parent {
            return Err(Error::ParentInTrash { parent });
        }
        let taken: Option<String> = tx
            .query_row(
                "SELECT i.number FROM issue i
                 WHERE i.trash_id = ?1 AND i.number IS NOT NULL AND EXISTS (
                     SELECT 1 FROM issue o
                     WHERE o.series_id = i.series_id AND o.number_sort = i.number_sort
                         AND o.trash_id IS NULL)
                 LIMIT 1",
                [trash],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(number) = taken {
            return Err(Error::DuplicateIssueNumber { number });
        }
        // trash_id must be cleared before the trash row goes, because
        // deleting a trash row deletes every row still pointing at it.
        for sql in [
            "UPDATE series SET trash_id = NULL WHERE trash_id = ?1",
            "UPDATE issue SET trash_id = NULL WHERE trash_id = ?1",
            "UPDATE edition SET trash_id = NULL WHERE trash_id = ?1",
            "UPDATE copy SET trash_id = NULL WHERE trash_id = ?1",
        ] {
            tx.execute(sql, [trash])?;
        }
        tx.execute("DELETE FROM trash WHERE id = ?1", [trash])?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes one group for good, with everything beneath it.
    pub fn purge(&self, trash: TrashId) -> Result<()> {
        require_trash_group(&self.conn, trash)?;
        self.delete_trash(Some(trash))
    }

    /// Deletes everything in Trash for good.
    pub fn empty_trash(&self) -> Result<()> {
        self.delete_trash(None)
    }

    fn delete_trash(&self, only: Option<TrashId>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let deleted = tx.execute("DELETE FROM trash WHERE ?1 IS NULL OR id = ?1", [only]);
        if let Err(e) = deleted {
            if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                drop(tx);
                return Err(self.explain_reprint_block(only)?);
            }
            return Err(e.into());
        }
        // A group whose rows all sat beneath a purged parent is now empty.
        tx.execute(
            "DELETE FROM trash WHERE id NOT IN (
                 SELECT trash_id FROM series WHERE trash_id IS NOT NULL
                 UNION SELECT trash_id FROM issue WHERE trash_id IS NOT NULL
                 UNION SELECT trash_id FROM edition WHERE trash_id IS NOT NULL
                 UNION SELECT trash_id FROM copy WHERE trash_id IS NOT NULL)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Names an issue that a surviving collected edition still reprints.
    fn explain_reprint_block(&self, only: Option<TrashId>) -> Result<Error> {
        let pair: Option<(IssueId, IssueId)> = self
            .conn
            .query_row(
                "WITH doomed AS (
                     SELECT i.id FROM issue i JOIN series s ON s.id = i.series_id
                     WHERE (?1 IS NULL AND (i.trash_id IS NOT NULL OR s.trash_id IS NOT NULL))
                         OR i.trash_id = ?1 OR s.trash_id = ?1)
                 SELECT collected_issue_id, collection_issue_id FROM collects
                 WHERE collected_issue_id IN doomed AND collection_issue_id NOT IN doomed
                 LIMIT 1",
                [only],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((issue, collection)) = pair else {
            return Ok(Error::Invalid(
                "Trash could not be emptied because something still refers to it".into(),
            ));
        };
        let label = |id| -> Result<String> {
            let issue = self.issue(id)?;
            Ok(issue_label(&self.series(issue.series)?, &issue))
        };
        Ok(Error::StillReprinted {
            issue: label(issue)?,
            collection: label(collection)?,
        })
    }

    // Helpers --------------------------------------------------------------

    fn category_of_issue(&self, issue: IssueId) -> Result<Category> {
        Ok(self.series(self.issue(issue)?.series)?.fields.category)
    }

    fn edition_label(&self, edition: &Edition) -> Result<String> {
        let issue = self.issue(edition.issue)?;
        let label = issue_label(&self.series(issue.series)?, &issue);
        Ok(if edition.fields.variant.is_empty() {
            label
        } else {
            format!("{label} ({})", edition.fields.variant)
        })
    }
}

pub(crate) fn live(trash: Option<TrashId>) -> Result<()> {
    match trash {
        None => Ok(()),
        Some(_) => Err(Error::InTrash),
    }
}

fn require_trash_group(conn: &Connection, trash: TrashId) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM trash WHERE id = ?1)",
        [trash],
        |row| row.get(0),
    )?;
    if exists { Ok(()) } else { Err(Error::NotFound) }
}

fn new_trash_group(conn: &Connection, label: &str) -> Result<TrashId> {
    conn.execute("INSERT INTO trash (label) VALUES (?1)", [label])?;
    Ok(TrashId(conn.last_insert_rowid()))
}

fn series_label(series: &Series) -> String {
    match series.fields.start_year {
        Some(year) => format!("{} ({year})", series.fields.name),
        None => series.fields.name.clone(),
    }
}

pub(crate) fn issue_label(series: &Series, issue: &Issue) -> String {
    let series = series_label(series);
    match (&issue.fields.number, issue.fields.title.as_str()) {
        (Some(number), _) => format!("{series} #{number}"),
        (None, "") => series,
        (None, title) => format!("{series}: {title}"),
    }
}

fn query_one<P, T>(
    conn: &Connection,
    sql: &str,
    params: P,
    map: impl FnOnce(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<T>
where
    P: rusqlite::Params,
{
    conn.query_row(sql, params, map)
        .optional()?
        .ok_or(Error::NotFound)
}

fn query_all<P, T>(
    conn: &Connection,
    sql: &str,
    params: P,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, map)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// Validation -------------------------------------------------------------

fn refuse<T>(why: &str) -> Result<T> {
    Err(Error::Invalid(why.to_owned()))
}

fn trimmed(text: &str) -> String {
    text.trim().to_owned()
}

fn optional_text(text: &Option<String>) -> Option<String> {
    text.as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn check_amount(amount: Option<Amount>, what: &str) -> Result<()> {
    if amount.is_some_and(|a| a.units() < 0) {
        return refuse(&format!("{what} cannot be negative"));
    }
    Ok(())
}

fn clean_series(fields: &SeriesFields) -> Result<SeriesFields> {
    let mut f = fields.clone();
    f.name = trimmed(&f.name);
    if f.name.is_empty() {
        return refuse("enter a series name");
    }
    if f.category != Category::Magazine
        && (f.magazine_format.is_some() || f.black_and_white.is_some())
    {
        return refuse("format and black-and-white apply only to magazines");
    }
    if f.volume == Some(0) {
        return refuse("volume numbers start at 1");
    }
    if f.start_year.is_some_and(|y| !(1..=9999).contains(&y)) {
        return refuse("enter a year from 1 to 9999");
    }
    Ok(f)
}

fn clean_issue(fields: &IssueFields, category: Category) -> Result<IssueFields> {
    let mut f = fields.clone();
    f.title = trimmed(&f.title);
    f.key_reason = trimmed(&f.key_reason);
    if f.number.is_none() && category != Category::CollectedEdition {
        return refuse("enter an issue number");
    }
    Ok(f)
}

fn clean_edition(fields: &EditionFields, category: Category) -> Result<EditionFields> {
    let mut f = fields.clone();
    f.variant = trimmed(&f.variant);
    f.barcode = optional_text(&f.barcode);
    let book_fields = f.isbn.is_some()
        || f.binding.is_some()
        || f.has_dust_jacket.is_some()
        || f.has_slipcase.is_some();
    if book_fields && category != Category::CollectedEdition {
        return refuse("ISBN, binding, dust jacket, and slipcase apply only to collected editions");
    }
    if f.printing == Some(0) {
        return refuse("printings start at 1");
    }
    check_amount(f.cover_price, "a cover price")?;
    Ok(f)
}

fn clean_copy(fields: &CopyFields, category: Category) -> Result<CopyFields> {
    let mut f = fields.clone();
    f.cert_number = optional_text(&f.cert_number);
    f.label_type = optional_text(&f.label_type);
    f.signed_by = trimmed(&f.signed_by);
    f.seller = trimmed(&f.seller);
    let graded = f.grade.is_some()
        || f.grade_type.is_some()
        || f.cert_number.is_some()
        || f.label_type.is_some();
    let book = f.book_condition.is_some() || f.dust_jacket_condition.is_some();
    if category == Category::CollectedEdition && graded {
        return refuse("collected editions use book condition instead of a grade");
    }
    if category != Category::CollectedEdition && book {
        return refuse("book condition applies only to collected editions");
    }
    if f.box_position.is_some() && f.storage_box.is_none() {
        return refuse("choose a box before a position in it");
    }
    if f.box_position == Some(0) {
        return refuse("box positions start at 1");
    }
    check_amount(f.purchase_price, "a purchase price")?;
    Ok(f)
}

fn number_sort(f: &IssueFields) -> &str {
    f.number.as_ref().map_or("", |n| n.sort_key())
}

fn check_number_free(
    conn: &Connection,
    series: SeriesId,
    f: &IssueFields,
    except: Option<IssueId>,
) -> Result<()> {
    let Some(number) = &f.number else {
        return Ok(());
    };
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM issue
             WHERE series_id = ?1 AND number_sort = ?2 AND trash_id IS NULL
                 AND (?3 IS NULL OR id <> ?3))",
        params![series, number.sort_key(), except],
        |row| row.get(0),
    )?;
    if taken {
        return Err(Error::DuplicateIssueNumber {
            number: number.as_str().to_owned(),
        });
    }
    Ok(())
}

/// A category change must not strand fields its new category forbids.
fn check_category_fits_children(
    conn: &Connection,
    series: SeriesId,
    category: Category,
) -> Result<()> {
    let exists =
        |sql: &str| -> Result<bool> { Ok(conn.query_row(sql, [series], |row| row.get(0))?) };
    if category == Category::CollectedEdition {
        if exists(
            "SELECT EXISTS (SELECT 1 FROM copy c JOIN edition e ON e.id = c.edition_id
                 JOIN issue i ON i.id = e.issue_id
                 WHERE i.series_id = ?1 AND (c.grade IS NOT NULL OR c.grade_type IS NOT NULL
                     OR c.cert_number IS NOT NULL OR c.label_type IS NOT NULL))",
        )? {
            return refuse("some copies have grades; clear them before making this a book line");
        }
    } else {
        if exists("SELECT EXISTS (SELECT 1 FROM issue WHERE series_id = ?1 AND number IS NULL)")? {
            return refuse("some volumes have no number; number them first");
        }
        if exists(
            "SELECT EXISTS (SELECT 1 FROM edition e JOIN issue i ON i.id = e.issue_id
                 WHERE i.series_id = ?1 AND (e.isbn IS NOT NULL OR e.binding IS NOT NULL
                     OR e.has_dust_jacket IS NOT NULL OR e.has_slipcase IS NOT NULL))",
        )? {
            return refuse("some editions have book details; clear them first");
        }
        if exists(
            "SELECT EXISTS (SELECT 1 FROM copy c JOIN edition e ON e.id = c.edition_id
                 JOIN issue i ON i.id = e.issue_id
                 WHERE i.series_id = ?1 AND (c.book_condition IS NOT NULL
                     OR c.dust_jacket_condition IS NOT NULL))",
        )? {
            return refuse("some copies have a book condition; clear it first");
        }
    }
    Ok(())
}

// Reading rows -----------------------------------------------------------

fn bad(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(invalid(what, "")))
}

fn amount_at(row: &Row<'_>, units: usize, currency: usize) -> rusqlite::Result<Option<Amount>> {
    let u: Option<i64> = row.get(units)?;
    let c: Option<Currency> = row.get(currency)?;
    match (u, c) {
        (None, None) => Ok(None),
        (Some(u), Some(c)) if u >= 0 => Ok(Some(Amount::new(c, u))),
        _ => Err(bad(units, "amount")),
    }
}

fn series_from_row(row: &Row<'_>) -> rusqlite::Result<Series> {
    Ok(Series {
        id: row.get(0)?,
        uuid: row.get(1)?,
        trash: row.get(2)?,
        fields: SeriesFields {
            name: row.get(3)?,
            category: row.get(4)?,
            publisher: row.get(5)?,
            volume: row.get(6)?,
            start_year: row.get(7)?,
            known_issue_count: row.get(8)?,
            magazine_format: row.get(9)?,
            black_and_white: row.get(10)?,
            notes: row.get(11)?,
        },
    })
}

fn issue_from_row(row: &Row<'_>) -> rusqlite::Result<Issue> {
    let year: Option<u16> = row.get(6)?;
    let month: Option<u8> = row.get(7)?;
    let cover_date = match (year, month) {
        (None, None) => None,
        (Some(y), m) => Some(CoverDate::new(y, m).ok_or_else(|| bad(6, "cover date"))?),
        (None, Some(_)) => return Err(bad(7, "cover date")),
    };
    Ok(Issue {
        id: row.get(0)?,
        uuid: row.get(1)?,
        series: row.get(2)?,
        trash: row.get(3)?,
        fields: IssueFields {
            number: row.get(4)?,
            title: row.get(5)?,
            cover_date,
            release_date: row.get(8)?,
            page_count: row.get(9)?,
            synopsis: row.get(10)?,
            is_key: row.get(11)?,
            key_reason: row.get(12)?,
        },
    })
}

fn edition_from_row(row: &Row<'_>) -> rusqlite::Result<Edition> {
    Ok(Edition {
        id: row.get(0)?,
        uuid: row.get(1)?,
        issue: row.get(2)?,
        trash: row.get(3)?,
        fields: EditionFields {
            variant: row.get(4)?,
            printing: row.get(5)?,
            distribution: row.get(6)?,
            cover_price: amount_at(row, 7, 8)?,
            barcode: row.get(9)?,
            isbn: row.get(10)?,
            binding: row.get(11)?,
            has_dust_jacket: row.get(12)?,
            has_slipcase: row.get(13)?,
        },
    })
}

fn copy_from_row(row: &Row<'_>) -> rusqlite::Result<Copy> {
    Ok(Copy {
        id: row.get(0)?,
        uuid: row.get(1)?,
        edition: row.get(2)?,
        trash: row.get(3)?,
        added_at: row.get(4)?,
        fields: CopyFields {
            grade: row.get(5)?,
            grade_type: row.get(6)?,
            cert_number: row.get(7)?,
            label_type: row.get(8)?,
            book_condition: row.get(9)?,
            dust_jacket_condition: row.get(10)?,
            signed_by: row.get(11)?,
            purchase_price: amount_at(row, 12, 13)?,
            purchase_date: row.get(14)?,
            seller: row.get(15)?,
            storage_box: row.get(16)?,
            box_position: row.get(17)?,
            notes: row.get(18)?,
        },
    })
}
