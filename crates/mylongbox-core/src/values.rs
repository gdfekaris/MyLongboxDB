//! A copy's value over time: dated valuations the owner enters, since
//! the app cannot look prices up.

use rusqlite::params;

use crate::catalog::{live, query_all};
use crate::date::Date;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{CopyId, RowUuid, ValueId};
use crate::money::{Amount, Currency};

/// A dated valuation of a copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueEntry {
    pub id: ValueId,
    pub date: Date,
    pub value: Amount,
    /// Free text, e.g. `GoCollect` or `eBay sold`.
    pub source: String,
}

impl Database {
    /// Records what a copy was worth on `date`.
    pub fn add_value(
        &self,
        copy: CopyId,
        date: Date,
        value: Amount,
        source: &str,
    ) -> Result<ValueId> {
        live(self.copy(copy)?.trash)?;
        if value.units() < 0 {
            return Err(Error::Invalid("a value cannot be negative".into()));
        }
        self.conn.execute(
            "INSERT INTO value_entry (uuid, copy_id, date, value_units, value_currency, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                RowUuid::new_random(),
                copy,
                date,
                value.units(),
                value.currency(),
                source.trim()
            ],
        )?;
        Ok(ValueId(self.conn.last_insert_rowid()))
    }

    /// A copy's valuations, oldest first; entries on the same day keep
    /// the order they were added.
    pub fn values(&self, copy: CopyId) -> Result<Vec<ValueEntry>> {
        query_all(
            &self.conn,
            "SELECT id, date, value_units, value_currency, source FROM value_entry
             WHERE copy_id = ?1 ORDER BY date, id",
            [copy],
            |row| {
                Ok(ValueEntry {
                    id: row.get(0)?,
                    date: row.get(1)?,
                    value: Amount::new(row.get::<_, Currency>(3)?, row.get(2)?),
                    source: row.get(4)?,
                })
            },
        )
    }

    pub fn delete_value(&self, id: ValueId) -> Result<()> {
        match self
            .conn
            .execute("DELETE FROM value_entry WHERE id = ?1", [id])?
        {
            0 => Err(Error::NotFound),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IssueNumber;
    use crate::model::*;

    fn copy(db: &Database) -> CopyId {
        let series = db
            .create_series(&SeriesFields::new("X-Men", Category::Comic))
            .unwrap();
        let issue = db
            .create_issue(
                series,
                &IssueFields::new(Some(IssueNumber::new("1").unwrap())),
            )
            .unwrap();
        let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
        db.create_copy(edition, &CopyFields::default()).unwrap()
    }

    fn day(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn values_come_back_oldest_first() {
        let db = Database::open_in_memory().unwrap();
        let c = copy(&db);
        db.add_value(
            c,
            day("2024-01-01"),
            Amount::new(Currency::Usd, 900),
            " GoCollect ",
        )
        .unwrap();
        db.add_value(c, day("2020-01-01"), Amount::new(Currency::Gold, 12), "")
            .unwrap();
        db.add_value(
            c,
            day("2024-01-01"),
            Amount::new(Currency::Usd, 950),
            "eBay sold",
        )
        .unwrap();
        let got: Vec<_> = db
            .values(c)
            .unwrap()
            .into_iter()
            .map(|v| (v.date.to_string(), v.value.units(), v.source))
            .collect();
        assert_eq!(
            got,
            [
                ("2020-01-01".to_owned(), 12, String::new()),
                ("2024-01-01".to_owned(), 900, "GoCollect".to_owned()),
                ("2024-01-01".to_owned(), 950, "eBay sold".to_owned()),
            ]
        );
    }

    #[test]
    fn values_can_be_deleted_once() {
        let db = Database::open_in_memory().unwrap();
        let c = copy(&db);
        let id = db
            .add_value(c, day("2024-01-01"), Amount::new(Currency::Usd, 900), "")
            .unwrap();
        db.delete_value(id).unwrap();
        assert!(db.values(c).unwrap().is_empty());
        assert!(matches!(db.delete_value(id), Err(Error::NotFound)));
    }

    #[test]
    fn trashed_copies_take_no_values() {
        let db = Database::open_in_memory().unwrap();
        let c = copy(&db);
        db.trash_copy(c).unwrap();
        assert!(matches!(
            db.add_value(c, day("2024-01-01"), Amount::new(Currency::Usd, 1), ""),
            Err(Error::InTrash)
        ));
    }

    #[test]
    fn negative_values_are_refused() {
        let db = Database::open_in_memory().unwrap();
        let c = copy(&db);
        assert!(matches!(
            db.add_value(c, day("2024-01-01"), Amount::new(Currency::Usd, -1), ""),
            Err(Error::Invalid(_))
        ));
    }
}
