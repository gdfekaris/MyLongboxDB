//! What a collected edition reprints, and where an issue is reprinted.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::{issue_label, live};
use crate::collects_text::{CollectsEntry, SeriesRef, format_collects, parse_collects};
use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{Category, IssueId, RowUuid, SeriesId};

/// One line of a contents preview. `None` means saving will create it as
/// a catalog entry with no copies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewEntry {
    pub entry: CollectsEntry,
    pub series: Option<SeriesId>,
    pub issue: Option<IssueId>,
}

/// An issue a collected edition reprints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedIssue {
    pub issue: IssueId,
    pub series: SeriesId,
    pub entry: CollectsEntry,
}

/// A collected edition that reprints a given issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedIn {
    pub collection: IssueId,
    /// e.g. `Epic Collection: Amazing Spider-Man (2014) #1`.
    pub label: String,
    /// Whether the owner has a copy of the collection.
    pub owned: bool,
}

/// How the owner has an issue: as single copies, inside collected
/// editions, both, or neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ownership {
    pub single_copies: u32,
    pub collections_owned: u32,
}

impl Database {
    /// Reads a contents list and matches it to the catalog without
    /// changing anything, so the user can see what saving will create.
    pub fn preview_collects(&self, text: &str) -> Result<Vec<PreviewEntry>> {
        let entries = parse_collects(text).map_err(Error::Invalid)?;
        let mut series_cache = HashMap::new();
        entries
            .into_iter()
            .map(|entry| {
                let series = find_series(&self.conn, &entry.series, &mut series_cache)?;
                let issue = match series {
                    Some(s) => find_issue(&self.conn, s, &entry)?,
                    None => None,
                };
                Ok(PreviewEntry {
                    entry,
                    series,
                    issue,
                })
            })
            .collect()
    }

    /// Replaces a collected edition's contents with `text`, creating any
    /// series and issues that do not exist yet. Empty text clears it.
    pub fn set_collects(&self, collection: IssueId, text: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let issue = self.issue(collection)?;
        live(issue.trash)?;
        if self.series(issue.series)?.fields.category != Category::CollectedEdition {
            return Err(Error::Invalid(
                "only collected editions list the issues they reprint".into(),
            ));
        }
        let entries = if text.trim().is_empty() {
            Vec::new()
        } else {
            parse_collects(text).map_err(Error::Invalid)?
        };

        let mut series_cache = HashMap::new();
        let mut seen = HashSet::new();
        let mut collected = Vec::new();
        for entry in &entries {
            let series = match find_series(&tx, &entry.series, &mut series_cache)? {
                Some(id) => id,
                None => {
                    let id = create_series(&tx, &entry.series)?;
                    series_cache.insert(entry.series.clone(), Some(id));
                    id
                }
            };
            let id = match find_issue(&tx, series, entry)? {
                Some(id) => id,
                None => create_issue(&tx, series, entry)?,
            };
            // A repeated issue keeps its first place in reading order.
            if seen.insert(id) {
                collected.push(id);
            }
        }

        tx.execute(
            "DELETE FROM collects WHERE collection_issue_id = ?1",
            [collection],
        )?;
        for (position, id) in (1u32..).zip(collected) {
            tx.execute(
                "INSERT INTO collects (collection_issue_id, collected_issue_id, position)
                 VALUES (?1, ?2, ?3)",
                params![collection, id, position],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The issues a collected edition reprints, in reading order.
    pub fn collects(&self, collection: IssueId) -> Result<Vec<CollectedIssue>> {
        let mut stmt = self.conn.prepare(
            "SELECT issue.id, series.id, series.name, series.start_year, issue.number
             FROM collects
             JOIN issue ON issue.id = collects.collected_issue_id
             JOIN series ON series.id = issue.series_id
             WHERE collects.collection_issue_id = ?1
             ORDER BY collects.position",
        )?;
        let rows = stmt.query_map([collection], |row| {
            Ok(CollectedIssue {
                issue: row.get(0)?,
                series: row.get(1)?,
                entry: CollectsEntry {
                    series: SeriesRef {
                        name: row.get(2)?,
                        start_year: row.get(3)?,
                    },
                    number: row.get(4)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The contents as text, in the form the contents editor reads.
    pub fn collects_text(&self, collection: IssueId) -> Result<String> {
        let entries: Vec<_> = self
            .collects(collection)?
            .into_iter()
            .map(|c| c.entry)
            .collect();
        Ok(format_collects(&entries))
    }

    /// Collected editions not in Trash that reprint `issue`, owned ones
    /// first: the detail page's "Also collected in".
    pub fn collected_in(&self, issue: IssueId) -> Result<Vec<CollectedIn>> {
        let mut stmt = self.conn.prepare(
            "SELECT collects.collection_issue_id, EXISTS (
                 SELECT 1 FROM copy JOIN edition ON edition.id = copy.edition_id
                 WHERE edition.issue_id = collects.collection_issue_id
                     AND copy.trash_id IS NULL)
             FROM collects JOIN issue ON issue.id = collects.collection_issue_id
             WHERE collects.collected_issue_id = ?1 AND issue.trash_id IS NULL",
        )?;
        let rows = stmt
            .query_map([issue], |row| Ok((row.get::<_, IssueId>(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(IssueId, bool)>>>()?;
        let mut found = rows
            .into_iter()
            .map(|(collection, owned)| {
                let issue = self.issue(collection)?;
                Ok(CollectedIn {
                    collection,
                    label: issue_label(&self.series(issue.series)?, &issue),
                    owned,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        found.sort_by(|a, b| b.owned.cmp(&a.owned).then_with(|| a.label.cmp(&b.label)));
        Ok(found)
    }

    /// How many copies of `issue` the owner has, and in how many owned
    /// collected editions it is reprinted. Items in Trash do not count.
    pub fn ownership(&self, issue: IssueId) -> Result<Ownership> {
        self.issue(issue)?;
        let (single, collected): (i64, i64) = self.conn.query_row(
            "SELECT
                 (SELECT count(*) FROM copy JOIN edition ON edition.id = copy.edition_id
                  WHERE edition.issue_id = ?1 AND copy.trash_id IS NULL),
                 (SELECT count(*) FROM collects
                  WHERE collected_issue_id = ?1 AND EXISTS (
                      SELECT 1 FROM copy JOIN edition ON edition.id = copy.edition_id
                      WHERE edition.issue_id = collects.collection_issue_id
                          AND copy.trash_id IS NULL))",
            [issue],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(Ownership {
            single_copies: u32::try_from(single).unwrap_or(u32::MAX),
            collections_owned: u32::try_from(collected).unwrap_or(u32::MAX),
        })
    }
}

/// The live series a reference names, `None` if there is none yet.
fn find_series(
    conn: &Connection,
    series: &SeriesRef,
    cache: &mut HashMap<SeriesRef, Option<SeriesId>>,
) -> Result<Option<SeriesId>> {
    if let Some(found) = cache.get(series) {
        return Ok(*found);
    }
    let mut stmt = conn.prepare(
        "SELECT id, category FROM series
         WHERE name = ?1 COLLATE NOCASE AND trash_id IS NULL
             AND (?2 IS NULL OR start_year = ?2)
         ORDER BY id",
    )?;
    let matches = stmt
        .query_map(params![series.name, series.start_year], |row| {
            Ok((row.get::<_, SeriesId>(0)?, row.get::<_, Category>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let label = match series.start_year {
        Some(year) => format!("{} ({year})", series.name),
        None => series.name.clone(),
    };
    let found = match matches.as_slice() {
        [] => None,
        [(_, Category::CollectedEdition)] => {
            return Err(Error::Invalid(format!(
                "{label} is a line of collected editions; list the single issues it reprints"
            )));
        }
        [(id, _)] => Some(*id),
        [..] if series.start_year.is_none() => {
            return Err(Error::Invalid(format!(
                "more than one series is named {label}; add its start year, like {label} (1963)"
            )));
        }
        [..] => {
            return Err(Error::Invalid(format!(
                "more than one series is named {label}; rename one to tell them apart"
            )));
        }
    };
    cache.insert(series.clone(), found);
    Ok(found)
}

fn find_issue(
    conn: &Connection,
    series: SeriesId,
    entry: &CollectsEntry,
) -> Result<Option<IssueId>> {
    Ok(conn
        .query_row(
            "SELECT id FROM issue
             WHERE series_id = ?1 AND number_sort = ?2 AND trash_id IS NULL",
            params![series, entry.number.sort_key()],
            |row| row.get(0),
        )
        .optional()?)
}

/// A new comic series for a contents list. The owner can change its
/// category later if it is really a magazine.
fn create_series(conn: &Connection, series: &SeriesRef) -> Result<SeriesId> {
    conn.execute(
        "INSERT INTO series (uuid, name, category, start_year) VALUES (?1, ?2, ?3, ?4)",
        params![
            RowUuid::new_random(),
            series.name,
            Category::Comic,
            series.start_year
        ],
    )?;
    Ok(SeriesId(conn.last_insert_rowid()))
}

/// A catalog issue with only its number: known to exist, not owned.
fn create_issue(conn: &Connection, series: SeriesId, entry: &CollectsEntry) -> Result<IssueId> {
    conn.execute(
        "INSERT INTO issue (uuid, series_id, number, number_sort) VALUES (?1, ?2, ?3, ?4)",
        params![
            RowUuid::new_random(),
            series,
            entry.number,
            entry.number.sort_key()
        ],
    )?;
    Ok(IssueId(conn.last_insert_rowid()))
}
