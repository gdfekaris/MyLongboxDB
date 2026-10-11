//! The owner's exchange-rate table, kept in the collection.

use rusqlite::params;

use crate::catalog::query_all;
use crate::date::Date;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::money::{Amount, Currency, ExchangeRate, RateTable};

impl Database {
    /// Every exchange rate entered, newest first.
    pub fn exchange_rates(&self) -> Result<Vec<ExchangeRate>> {
        let rows: Vec<(Date, i64, i64)> = query_all(
            &self.conn,
            "SELECT date, usd_cents_per_ounce, usd_cents_per_bitcoin FROM exchange_rate
             ORDER BY date DESC",
            (),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        rows.into_iter()
            .map(|(date, gold, btc)| {
                ExchangeRate::new(
                    date,
                    Amount::new(Currency::Usd, gold),
                    Amount::new(Currency::Usd, btc),
                )
                .map_err(|e| Error::Invalid(e.to_string()))
            })
            .collect()
    }

    /// The rates as a table for converting amounts.
    pub fn rate_table(&self) -> Result<RateTable> {
        Ok(RateTable::new(self.exchange_rates()?))
    }

    /// Records a rate, replacing any rate already entered for its date.
    pub fn set_exchange_rate(&self, rate: ExchangeRate) -> Result<()> {
        self.conn.execute(
            "INSERT INTO exchange_rate (date, usd_cents_per_ounce, usd_cents_per_bitcoin)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (date) DO UPDATE SET
                 usd_cents_per_ounce = excluded.usd_cents_per_ounce,
                 usd_cents_per_bitcoin = excluded.usd_cents_per_bitcoin",
            params![
                rate.date(),
                rate.usd_per_ounce().units(),
                rate.usd_per_bitcoin().units()
            ],
        )?;
        Ok(())
    }

    pub fn delete_exchange_rate(&self, date: Date) -> Result<()> {
        match self
            .conn
            .execute("DELETE FROM exchange_rate WHERE date = ?1", [date])?
        {
            0 => Err(Error::NotFound),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(date: &str, gold_dollars: i64, btc_dollars: i64) -> ExchangeRate {
        ExchangeRate::new(
            date.parse().unwrap(),
            Amount::new(Currency::Usd, gold_dollars * 100),
            Amount::new(Currency::Usd, btc_dollars * 100),
        )
        .unwrap()
    }

    #[test]
    fn rates_are_kept_one_per_date_newest_first() {
        let db = Database::open_in_memory().unwrap();
        db.set_exchange_rate(rate("2020-01-01", 1500, 7000))
            .unwrap();
        db.set_exchange_rate(rate("2024-01-01", 2000, 40000))
            .unwrap();
        db.set_exchange_rate(rate("2020-01-01", 1550, 7100))
            .unwrap();
        let rates = db.exchange_rates().unwrap();
        assert_eq!(
            rates,
            [
                rate("2024-01-01", 2000, 40000),
                rate("2020-01-01", 1550, 7100)
            ]
        );

        let table = db.rate_table().unwrap();
        let ounce = Amount::new(Currency::Gold, 10_000);
        assert_eq!(
            table
                .convert(ounce, "2021-06-01".parse().unwrap(), Currency::Usd)
                .unwrap(),
            Amount::new(Currency::Usd, 155_000)
        );
    }

    #[test]
    fn rates_can_be_deleted_once() {
        let db = Database::open_in_memory().unwrap();
        db.set_exchange_rate(rate("2020-01-01", 1500, 7000))
            .unwrap();
        let date = "2020-01-01".parse().unwrap();
        db.delete_exchange_rate(date).unwrap();
        assert!(db.exchange_rates().unwrap().is_empty());
        assert!(matches!(
            db.delete_exchange_rate(date),
            Err(Error::NotFound)
        ));
    }
}
