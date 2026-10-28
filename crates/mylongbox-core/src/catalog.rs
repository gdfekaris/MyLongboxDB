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
    start_year, known_issue_count, magazine_format, black_and_white, notes, imprint
    FROM series",
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
    cover_month, release_date, page_count, synopsis, is_key, key_reason, legacy_number
    FROM issue",
            " ",
            $rest
        )
    };
}

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_edition {
    ($rest:expr) => {
        concat!(
            "SELECT id, uuid, issue_id, trash_id, variant, printing,
    distribution, cover_price_units, cover_price_currency, barcode, isbn, binding,
    has_dust_jacket, has_slipcase, canadian_cover_price_cents, published FROM edition",
            " ",
            $rest
        )
    };
}

/// Expands to one complete SQL literal, so no query is assembled at runtime.
macro_rules! select_copy {
    ($rest:expr) => {
        concat!(
            "SELECT id, uuid, edition_id, trash_id, added_at, grade, grade_type,
    cert_number, label_type, book_condition, dust_jacket_condition,
    purchase_price_units, purchase_price_currency, purchase_date, seller, box_id,
    box_position, notes, page_quality, pedigree FROM copy",
            " ",
            $rest
        )
    };
}

/// Deletes the Trash groups left with nothing in them, once what they
/// held went with something deleted above it.
/// What deleting for good leaves behind, deleted in turn until none is
/// left: editions outside Trash with no copy, then issues outside Trash
/// with no edition that no collected edition reprints. The collection
/// holds the books owned, so an issue goes with its last copy, and
/// series stay. Migration 13 runs the same statements once.
const DROP_LEFTOVERS: [&str; 2] = [
    "DELETE FROM edition WHERE trash_id IS NULL
         AND NOT EXISTS (SELECT 1 FROM copy WHERE copy.edition_id = edition.id)",
    "DELETE FROM issue WHERE trash_id IS NULL
         AND NOT EXISTS (SELECT 1 FROM edition WHERE edition.issue_id = issue.id)
         AND NOT EXISTS (SELECT 1 FROM collects WHERE collects.collected_issue_id = issue.id)",
];

/// Deletes what [`DROP_LEFTOVERS`] finds, again until nothing changes,
/// since an issue that goes may have been all that reprinted another.
pub(crate) fn drop_leftovers(conn: &Connection) -> Result<()> {
    loop {
        let mut gone = 0;
        for step in DROP_LEFTOVERS {
            gone += conn.execute(step, [])?;
        }
        if gone == 0 {
            return Ok(());
        }
    }
}

pub(crate) const DROP_EMPTY_TRASH_GROUPS: &str = "DELETE FROM trash WHERE id NOT IN (
    SELECT trash_id FROM series WHERE trash_id IS NOT NULL
    UNION SELECT trash_id FROM issue WHERE trash_id IS NOT NULL
    UNION SELECT trash_id FROM edition WHERE trash_id IS NOT NULL
    UNION SELECT trash_id FROM copy WHERE trash_id IS NOT NULL)";

impl Database {
    // Series ---------------------------------------------------------------

    pub fn create_series(&self, fields: &SeriesFields) -> Result<SeriesId> {
        let f = clean_series(fields)?;
        self.conn.execute(
            "INSERT INTO series (uuid, name, category, publisher_id, volume, start_year,
                 known_issue_count, magazine_format, black_and_white, notes, imprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                f.imprint,
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
        let tx = self.tx()?;
        let before = self.series(id)?;
        live(before.trash)?;
        check_category_fits_children(&tx, id, f.category)?;
        tx.execute(
            "UPDATE series SET name = ?2, category = ?3, publisher_id = ?4, volume = ?5,
                 start_year = ?6, known_issue_count = ?7, magazine_format = ?8,
                 black_and_white = ?9, notes = ?10, imprint = ?11
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
                f.imprint,
            ],
        )?;
        self.series_rules_follow(&before.fields.name, &f.name)?;
        tx.commit()?;
        Ok(())
    }

    /// Carries saved "series is" rules from `old` to `new`, unless another
    /// series is still called `old`, when the rule may mean that one.
    pub(crate) fn series_rules_follow(&self, old: &str, new: &str) -> Result<()> {
        let still_named: i64 = self.conn.query_row(
            "SELECT count(*) FROM series WHERE name = ?1 COLLATE NOCASE AND trash_id IS NULL",
            [old],
            |row| row.get(0),
        )?;
        if still_named == 0 {
            self.rules_follow_rename(crate::shelves::Named::Series, old, new)?;
        }
        Ok(())
    }

    /// Imprints already given to a series that contain `text`, ignoring
    /// case, those of `publisher`'s series first, then by name.
    pub fn suggest_imprints(&self, publisher: &str, text: &str, limit: u32) -> Result<Vec<String>> {
        query_all(
            &self.conn,
            "SELECT s.imprint FROM series s LEFT JOIN publisher p ON p.id = s.publisher_id
             WHERE s.imprint <> '' AND s.trash_id IS NULL
                 AND s.imprint LIKE ?1 ESCAPE '\\'
             GROUP BY s.imprint COLLATE NOCASE
             ORDER BY max(coalesce(p.name = ?2 COLLATE NOCASE, 0)) DESC,
                 s.imprint COLLATE NOCASE
             LIMIT ?3",
            params![
                format!("%{}%", crate::search::like_escaped(text.trim())),
                publisher.trim(),
                limit
            ],
            |row| row.get(0),
        )
    }

    /// Pedigrees already given to a copy that contain `text`, ignoring
    /// case, by name.
    pub fn suggest_pedigrees(&self, text: &str, limit: u32) -> Result<Vec<String>> {
        query_all(
            &self.conn,
            "SELECT pedigree FROM copy
             WHERE pedigree <> '' AND trash_id IS NULL AND pedigree LIKE ?1 ESCAPE '\\'
             GROUP BY pedigree COLLATE NOCASE
             ORDER BY pedigree COLLATE NOCASE
             LIMIT ?2",
            params![
                format!("%{}%", crate::search::like_escaped(text.trim())),
                limit
            ],
            |row| row.get(0),
        )
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
        let tx = self.tx()?;
        let parent = self.series(series)?;
        live(parent.trash)?;
        let f = clean_issue(fields, parent.fields.category)?;
        check_number_free(&tx, series, &f, None)?;
        tx.execute(
            "INSERT INTO issue (uuid, series_id, number, number_sort, title, cover_year,
                 cover_month, release_date, page_count, synopsis, is_key, key_reason,
                 legacy_number)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                f.legacy_number,
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
        let tx = self.tx()?;
        let current = self.issue(id)?;
        live(current.trash)?;
        let f = clean_issue(fields, self.series(current.series)?.fields.category)?;
        check_number_free(&tx, current.series, &f, Some(id))?;
        tx.execute(
            "UPDATE issue SET number = ?2, number_sort = ?3, title = ?4, cover_year = ?5,
                 cover_month = ?6, release_date = ?7, page_count = ?8, synopsis = ?9,
                 is_key = ?10, key_reason = ?11, legacy_number = ?12
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
                f.legacy_number,
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
        let tx = self.tx()?;
        let parent = self.issue(issue)?;
        live(parent.trash)?;
        let f = clean_edition(fields, self.series(parent.series)?.fields.category)?;
        tx.execute(
            "INSERT INTO edition (uuid, issue_id, variant, printing, distribution,
                 cover_price_units, cover_price_currency, barcode, isbn, binding,
                 has_dust_jacket, has_slipcase, canadian_cover_price_cents, published)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                f.canadian_cover_price_cents,
                f.published,
            ],
        )?;
        let id = EditionId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
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
        let tx = self.tx()?;
        let current = self.edition(id)?;
        live(current.trash)?;
        let f = clean_edition(fields, self.category_of_issue(current.issue)?)?;
        tx.execute(
            "UPDATE edition SET variant = ?2, printing = ?3, distribution = ?4,
                 cover_price_units = ?5, cover_price_currency = ?6, barcode = ?7, isbn = ?8,
                 binding = ?9, has_dust_jacket = ?10, has_slipcase = ?11,
                 canadian_cover_price_cents = ?12, published = ?13
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
                f.canadian_cover_price_cents,
                f.published,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The live editions of an issue, in the order the book list shows
    /// their copies.
    pub fn editions_of_issue(&self, issue: IssueId) -> Result<Vec<Edition>> {
        query_all(
            &self.conn,
            select_edition!(concat!(
                "e WHERE issue_id = ?1 AND trash_id IS NULL ORDER BY ",
                edition_order!()
            )),
            [issue],
            edition_from_row,
        )
    }

    // Copies ---------------------------------------------------------------

    pub fn create_copy(&self, edition: EditionId, fields: &CopyFields) -> Result<CopyId> {
        let tx = self.tx()?;
        let parent = self.edition(edition)?;
        live(parent.trash)?;
        let f = clean_copy(fields, self.category_of_issue(parent.issue)?)?;
        tx.execute(
            "INSERT INTO copy (uuid, edition_id, grade, grade_type, cert_number, label_type,
                 book_condition, dust_jacket_condition, purchase_price_units,
                 purchase_price_currency, purchase_date, seller, box_id, box_position, notes,
                 page_quality, pedigree)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17)",
            params![
                RowUuid::new_random(),
                edition,
                f.grade,
                f.grade_type,
                f.cert_number,
                f.label_type,
                f.book_condition,
                f.dust_jacket_condition,
                f.purchase_price.map(Amount::units),
                f.purchase_price.map(Amount::currency),
                f.purchase_date,
                f.seller,
                f.storage_box,
                f.box_position,
                f.notes,
                f.page_quality,
                f.pedigree,
            ],
        )?;
        let id = CopyId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
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
        let tx = self.tx()?;
        let current = self.copy(id)?;
        live(current.trash)?;
        let issue = self.edition(current.edition)?.issue;
        let f = clean_copy(fields, self.category_of_issue(issue)?)?;
        tx.execute(
            "UPDATE copy SET grade = ?2, grade_type = ?3, cert_number = ?4, label_type = ?5,
                 book_condition = ?6, dust_jacket_condition = ?7,
                 purchase_price_units = ?8, purchase_price_currency = ?9, purchase_date = ?10,
                 seller = ?11, box_id = ?12, box_position = ?13, notes = ?14,
                 page_quality = ?15, pedigree = ?16
             WHERE id = ?1",
            params![
                id,
                f.grade,
                f.grade_type,
                f.cert_number,
                f.label_type,
                f.book_condition,
                f.dust_jacket_condition,
                f.purchase_price.map(Amount::units),
                f.purchase_price.map(Amount::currency),
                f.purchase_date,
                f.seller,
                f.storage_box,
                f.box_position,
                f.notes,
                f.page_quality,
                f.pedigree,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The live copies of an edition, in the order the book list shows
    /// them.
    pub fn copies_of_edition(&self, edition: EditionId) -> Result<Vec<Copy>> {
        query_all(
            &self.conn,
            select_copy!(concat!(
                "c WHERE edition_id = ?1 AND trash_id IS NULL ORDER BY ",
                copy_order!()
            )),
            [edition],
            copy_from_row,
        )
    }

    // Trash ----------------------------------------------------------------

    /// Moves a series and everything in it to Trash as one group.
    pub fn trash_series(&self, id: SeriesId) -> Result<TrashId> {
        let tx = self.tx()?;
        let series = self.series(id)?;
        live(series.trash)?;
        let trash = new_trash_group(&tx, &self.shown_series_label(&series)?)?;
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
        let tx = self.tx()?;
        let issue = self.issue(id)?;
        live(issue.trash)?;
        let label = self.issue_label(&self.series(issue.series)?, &issue)?;
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
        let tx = self.tx()?;
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
        let tx = self.tx()?;
        let copy = self.copy(id)?;
        live(copy.trash)?;
        // Named apart from its issue and edition, which Trash may also hold.
        let label = format!(
            "Copy of {}",
            self.issue_and_variant(&self.edition(copy.edition)?)?
        );
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
        let tx = self.tx()?;
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
        let tx = self.tx()?;
        let deleted = tx.execute("DELETE FROM trash WHERE ?1 IS NULL OR id = ?1", [only]);
        if let Err(e) = deleted {
            if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                drop(tx);
                return Err(self.explain_reprint_block(only)?);
            }
            return Err(e.into());
        }
        drop_leftovers(&tx)?;
        // A group whose rows all sat beneath a purged parent is now empty.
        tx.execute(DROP_EMPTY_TRASH_GROUPS, [])?;
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
            self.issue_label(&self.series(issue.series)?, &issue)
        };
        Ok(Error::StillReprinted {
            issue: label(issue)?,
            collection: label(collection)?,
        })
    }

    // Labels ---------------------------------------------------------------

    /// A series' label as the app shows it; see [`shown_label`].
    pub fn shown_series_label(&self, series: &Series) -> Result<String> {
        let f = &series.fields;
        let lookalike = self.is_lookalike(series.id)?;
        Ok(shown_label(&f.name, f.start_year, f.volume, lookalike))
    }

    /// Whether another live series shares this one's name and start
    /// year; see [`shown_label`].
    pub(crate) fn is_lookalike(&self, series: SeriesId) -> Result<bool> {
        Ok(self.conn.query_row(
            concat!("SELECT ", lookalike!("s"), " FROM series s WHERE s.id = ?1"),
            [series],
            |row| row.get(0),
        )?)
    }

    /// e.g. `Amazing Spider-Man (1963 series) #121`, or a standalone
    /// book's own title.
    pub(crate) fn issue_label(&self, series: &Series, issue: &Issue) -> Result<String> {
        let series = self.shown_series_label(series)?;
        Ok(match (&issue.fields.number, issue.fields.title.as_str()) {
            (Some(number), _) => format!("{series} #{number}"),
            (None, "") => series,
            (None, title) => format!("{series}: {title}"),
        })
    }

    /// Every series not in Trash as the app lists it, by name.
    pub fn series_labels(&self) -> Result<Vec<SeriesLabel>> {
        query_all(
            &self.conn,
            concat!(
                "SELECT s.id, s.category, s.name, s.start_year, s.volume, ",
                lookalike!("s"),
                " FROM series s WHERE s.trash_id IS NULL
                 ORDER BY s.name COLLATE NOCASE, s.start_year, s.volume, s.id"
            ),
            (),
            |row| {
                let (name, start_year, volume): (String, _, _) =
                    (row.get(2)?, row.get(3)?, row.get(4)?);
                Ok(SeriesLabel {
                    id: row.get(0)?,
                    category: row.get(1)?,
                    label: shown_label(&name, start_year, volume, row.get(5)?),
                    usual: series_label(&name, start_year, volume),
                })
            },
        )
    }

    // Helpers --------------------------------------------------------------

    pub(crate) fn category_of_issue(&self, issue: IssueId) -> Result<Category> {
        Ok(self.series(self.issue(issue)?.series)?.fields.category)
    }

    /// e.g. `X-Men (1963 series) #1, standard edition`, so Trash tells an
    /// edition apart from its issue.
    fn edition_label(&self, edition: &Edition) -> Result<String> {
        let label = self.issue_and_variant(edition)?;
        Ok(if edition.fields.variant.is_empty() {
            format!("{label}, standard edition")
        } else {
            label
        })
    }

    /// e.g. `X-Men (1963 series) #1 (Cover B)`, or just the issue for the
    /// standard cover.
    fn issue_and_variant(&self, edition: &Edition) -> Result<String> {
        let issue = self.issue(edition.issue)?;
        let label = self.issue_label(&self.series(issue.series)?, &issue)?;
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

pub(crate) fn new_trash_group(conn: &Connection, label: &str) -> Result<TrashId> {
    conn.execute("INSERT INTO trash (label) VALUES (?1)", [label])?;
    Ok(TrashId(conn.last_insert_rowid()))
}

/// A series' name with what tells apart relaunches that share it: its
/// start year, as in `Amazing Spider-Man (1963 series)`, or failing
/// that its volume, as in `Black Panther (Vol. 3)`.
pub fn series_label(name: &str, start_year: Option<u16>, volume: Option<u32>) -> String {
    match (start_year, volume) {
        (Some(year), _) => format!("{name} ({year} series)"),
        (None, Some(volume)) => format!("{name} (Vol. {volume})"),
        (None, None) => name.to_owned(),
    }
}

/// A series' label as the app shows it: [`series_label`], unless another
/// series shares its name and start year (`lookalike`), when the volume
/// tells them apart too: `Fantastic Four (1961 series, Vol. 1)` and
/// `Fantastic Four (1961 series, no volume)`.
pub(crate) fn shown_label(
    name: &str,
    start_year: Option<u16>,
    volume: Option<u32>,
    lookalike: bool,
) -> String {
    match start_year {
        Some(year) if lookalike => {
            let volume = volume.map_or_else(|| "no volume".to_owned(), |v| format!("Vol. {v}"));
            format!("{name} ({year} series, {volume})")
        }
        _ => series_label(name, start_year, volume),
    }
}

/// Whether [`shown_label`] names the series' volume: when it has no
/// start year to go by, or shares that year with a lookalike.
pub(crate) fn label_names_volume(
    start_year: Option<u16>,
    volume: Option<u32>,
    lookalike: bool,
) -> bool {
    volume.is_some() && (start_year.is_none() || lookalike)
}

/// A live series as the app lists it, for finding one by its label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesLabel {
    pub id: SeriesId,
    pub category: Category,
    /// As [`shown_label`] writes it.
    pub label: String,
    /// As [`series_label`] writes it, which lookalikes share.
    usual: String,
}

/// Which series typed text names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelMatch {
    One(SeriesId),
    None,
    /// The text names lookalikes together.
    Several,
}

/// The series of `labels` that `text` names: by the label it is shown
/// with, or failing that as series are usually written, which a
/// lookalike's own label never needs.
pub fn match_series_label<'a>(
    labels: impl IntoIterator<Item = &'a SeriesLabel>,
    text: &str,
) -> LabelMatch {
    let labels: Vec<&SeriesLabel> = labels.into_iter().collect();
    let text = text.trim();
    let shown: Vec<&&SeriesLabel> = labels
        .iter()
        .filter(|s| s.label.eq_ignore_ascii_case(text))
        .collect();
    if let [one] = shown.as_slice() {
        return LabelMatch::One(one.id);
    }
    let usual: Vec<&&SeriesLabel> = labels
        .iter()
        .filter(|s| same_series_label(text, &s.usual))
        .collect();
    match usual.as_slice() {
        [] => LabelMatch::None,
        [one] => LabelMatch::One(one.id),
        _ => LabelMatch::Several,
    }
}

/// Reads a series as [`shown_label`] writes it, or with the year alone
/// in brackets as earlier versions wrote it: the name, then the start
/// year or volume the brackets name, if any.
pub fn parse_series_label(text: &str) -> (String, Option<u16>, Option<u32>) {
    let text = text.trim();
    if let Some(open) = text.rfind(" (")
        && let Some(inside) = text[open + 2..].strip_suffix(')')
    {
        let name = text[..open].trim().to_owned();
        let inside = inside.trim();
        // A lookalike's volume follows its year.
        let (inside, volume) = match inside.rsplit_once(',') {
            Some((year, volume)) if volume.trim().eq_ignore_ascii_case("no volume") => {
                (year.trim(), None)
            }
            Some((year, volume)) => match volume_of(volume) {
                Some(volume) => (year.trim(), Some(volume)),
                None => (inside, None),
            },
            None => (inside, None),
        };
        let year = inside.strip_suffix(" series").unwrap_or(inside).trim();
        if year.len() == 4
            && let Ok(year) = year.parse::<u16>()
            && year > 0
        {
            return (name, Some(year), volume);
        }
        if volume.is_none()
            && let Some(volume) = volume_of(inside)
        {
            return (name, None, Some(volume));
        }
    }
    (text.to_owned(), None, None)
}

/// `Vol. 3` as 3.
fn volume_of(text: &str) -> Option<u32> {
    let text = text.trim();
    text.strip_prefix("Vol.")
        .or_else(|| text.strip_prefix("vol."))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
}

/// Whether `text` names the series labelled `label`, written either
/// way [`parse_series_label`] reads, ignoring case.
pub fn same_series_label(text: &str, label: &str) -> bool {
    let (name, year, volume) = parse_series_label(text);
    series_label(&name, year, volume).eq_ignore_ascii_case(label)
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

pub(crate) fn query_all<P, T>(
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
    f.imprint = trimmed(&f.imprint);
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
    if f.canadian_cover_price_cents.is_some_and(|c| c < 0) {
        return refuse("a Canadian cover price cannot be negative");
    }
    Ok(f)
}

fn clean_copy(fields: &CopyFields, category: Category) -> Result<CopyFields> {
    let mut f = fields.clone();
    f.cert_number = optional_text(&f.cert_number);
    f.label_type = optional_text(&f.label_type);
    f.seller = trimmed(&f.seller);
    f.pedigree = trimmed(&f.pedigree);
    let graded = f.grade.is_some()
        || f.grade_type.is_some()
        || f.cert_number.is_some()
        || f.label_type.is_some()
        || f.page_quality.is_some();
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

pub(crate) fn check_number_free(
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
                     OR c.cert_number IS NOT NULL OR c.label_type IS NOT NULL
                     OR c.page_quality IS NOT NULL))",
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

pub(crate) fn amount_at(
    row: &Row<'_>,
    units: usize,
    currency: usize,
) -> rusqlite::Result<Option<Amount>> {
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
            imprint: row.get(12)?,
        },
    })
}

pub(crate) fn cover_date_at(
    row: &Row<'_>,
    year: usize,
    month: usize,
) -> rusqlite::Result<Option<CoverDate>> {
    let y: Option<u16> = row.get(year)?;
    let m: Option<u8> = row.get(month)?;
    match (y, m) {
        (None, None) => Ok(None),
        (Some(y), m) => Ok(Some(
            CoverDate::new(y, m).ok_or_else(|| bad(year, "cover date"))?,
        )),
        (None, Some(_)) => Err(bad(month, "cover date")),
    }
}

fn issue_from_row(row: &Row<'_>) -> rusqlite::Result<Issue> {
    let cover_date = cover_date_at(row, 6, 7)?;
    Ok(Issue {
        id: row.get(0)?,
        uuid: row.get(1)?,
        series: row.get(2)?,
        trash: row.get(3)?,
        fields: IssueFields {
            number: row.get(4)?,
            legacy_number: row.get(13)?,
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
            canadian_cover_price_cents: row.get(14)?,
            published: row.get(15)?,
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
            page_quality: row.get(18)?,
            book_condition: row.get(9)?,
            dust_jacket_condition: row.get(10)?,
            purchase_price: amount_at(row, 11, 12)?,
            purchase_date: row.get(13)?,
            seller: row.get(14)?,
            pedigree: row.get(19)?,
            storage_box: row.get(15)?,
            box_position: row.get(16)?,
            notes: row.get(17)?,
        },
    })
}
