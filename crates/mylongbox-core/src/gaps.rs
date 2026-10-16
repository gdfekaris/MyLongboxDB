//! Gaps in a run: the issue numbers missing from each series between
//! those owned, or up to the number of issues the series is known to
//! have.

use std::collections::{BTreeSet, HashMap};

use crate::catalog::query_all;
use crate::db::Database;
use crate::error::Result;
use crate::model::{Category, SeriesId};

/// The gaps in one series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesGaps {
    pub series: SeriesId,
    pub name: String,
    pub start_year: Option<u16>,
    pub volume: Option<u32>,
    pub category: Category,
    /// How many of the numbers in the run are owned.
    pub owned: u32,
    /// The first and last number of the run.
    pub first: u32,
    pub last: u32,
    /// The missing numbers, as inclusive ranges in order.
    pub missing: Vec<(u32, u32)>,
}

impl SeriesGaps {
    /// How many numbers are missing.
    pub fn missing_count(&self) -> u32 {
        self.missing.iter().map(|(a, b)| b - a + 1).sum()
    }
}

/// A plain whole number such as `7` or `007`; annuals, halves, and the
/// like are not part of a numbered run.
pub(crate) fn whole_number(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The numbers from `first` to `last` not in `owned`, as ranges.
pub(crate) fn missing_ranges(owned: &BTreeSet<u32>, first: u32, last: u32) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for n in first..=last {
        if owned.contains(&n) {
            continue;
        }
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == n => *end = n,
            _ => ranges.push((n, n)),
        }
    }
    ranges
}

impl Database {
    /// Every live series with a gap in its run, by name. A run goes from
    /// the lowest number owned to the highest, or, when the series' issue
    /// count is known, from #1 (or #0, if owned) to that count. With
    /// `count_collected`, an issue reprinted in a collected edition that
    /// is owned counts as owned.
    pub fn gaps(&self, count_collected: bool) -> Result<Vec<SeriesGaps>> {
        let tx = self.tx()?;
        let mut owned: HashMap<SeriesId, BTreeSet<u32>> = HashMap::new();
        let mut add = |rows: Vec<(SeriesId, String)>| {
            for (series, number) in rows {
                if let Some(n) = whole_number(&number) {
                    owned.entry(series).or_default().insert(n);
                }
            }
        };
        add(query_all(
            &self.conn,
            "SELECT i.series_id, i.number FROM copy c
             JOIN edition e ON e.id = c.edition_id
             JOIN issue i ON i.id = e.issue_id
             WHERE c.trash_id IS NULL AND i.number IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?);
        if count_collected {
            add(query_all(
                &self.conn,
                "SELECT r.series_id, r.number FROM collects k
                 JOIN issue r ON r.id = k.collected_issue_id
                 JOIN edition e ON e.issue_id = k.collection_issue_id
                 JOIN copy c ON c.edition_id = e.id
                 WHERE c.trash_id IS NULL AND r.trash_id IS NULL AND r.number IS NOT NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?);
        }
        let series = query_all(
            &self.conn,
            "SELECT id, name, start_year, category, known_issue_count, volume FROM series
             WHERE trash_id IS NULL
             ORDER BY name COLLATE NOCASE, start_year, id",
            [],
            |row| {
                Ok((
                    row.get::<_, SeriesId>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<u16>>(2)?,
                    row.get::<_, Category>(3)?,
                    row.get::<_, Option<u32>>(4)?,
                    row.get::<_, Option<u32>>(5)?,
                ))
            },
        )?;
        tx.commit()?;
        Ok(series
            .into_iter()
            .filter_map(|(id, name, start_year, category, known, volume)| {
                let numbers = owned.get(&id)?;
                let (lowest, highest) = (*numbers.first()?, *numbers.last()?);
                let (first, last) = match known {
                    Some(count) => (lowest.min(1), highest.max(count)),
                    None => (lowest, highest),
                };
                let missing = missing_ranges(numbers, first, last);
                if missing.is_empty() {
                    return None;
                }
                let owned = u32::try_from(numbers.range(first..=last).count()).unwrap_or(u32::MAX);
                Some(SeriesGaps {
                    series: id,
                    name,
                    start_year,
                    volume,
                    category,
                    owned,
                    first,
                    last,
                    missing,
                })
            })
            .collect())
    }
}
