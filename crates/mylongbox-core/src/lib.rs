//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod catalog;
#[cfg(test)]
mod catalog_tests;
mod date;
mod db;
mod error;
mod issue_number;
mod migrate;
mod model;
mod money;
#[cfg(test)]
mod schema_tests;

pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use error::{Error, Result};
pub use issue_number::{IssueNumber, IssueNumberError};
pub use model::{
    Binding, BookCondition, BoxId, Category, Copy, CopyFields, CopyId, CoverDate, Distribution,
    Edition, EditionFields, EditionId, Grade, GradeType, Isbn, Issue, IssueFields, IssueId,
    MagazineFormat, PublisherId, RowUuid, Series, SeriesFields, SeriesId, TrashGroup, TrashId,
};
pub use money::{
    Amount, ConversionError, Currency, ExchangeRate, ParseAmountError, RateError, RateTable,
    STALE_AFTER_DAYS,
};
