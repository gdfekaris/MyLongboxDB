//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod catalog;
#[cfg(test)]
mod catalog_tests;
mod date;
mod db;
mod error;
mod issue_number;
mod links;
mod lookup;
#[cfg(test)]
mod lookup_tests;
mod migrate;
mod model;
mod money;
#[cfg(test)]
mod schema_tests;

pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use error::{Error, Result};
pub use issue_number::{IssueNumber, IssueNumberError};
pub use links::{Appearance, ArcPart, Credit, Named};
pub use lookup::{Lookup, LookupId};
pub use model::{
    Binding, BookCondition, BoxId, Category, CharacterId, Copy, CopyFields, CopyId, CoverDate,
    CreatorId, Distribution, Edition, EditionFields, EditionId, Grade, GradeType, Isbn, Issue,
    IssueFields, IssueId, MagazineFormat, PublisherId, Role, RowUuid, Series, SeriesFields,
    SeriesId, StoryArcId, TagId, TeamId, TrashGroup, TrashId,
};
pub use money::{
    Amount, ConversionError, Currency, ExchangeRate, ParseAmountError, RateError, RateTable,
    STALE_AFTER_DAYS,
};
