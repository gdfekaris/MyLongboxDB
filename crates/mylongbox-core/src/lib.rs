//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod date;
mod db;
mod error;
mod migrate;
mod money;

pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use error::{Error, Result};
pub use money::{
    Amount, ConversionError, Currency, ExchangeRate, ParseAmountError, RateError, RateTable,
    STALE_AFTER_DAYS,
};
