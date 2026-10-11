//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod browse;
#[cfg(test)]
mod browse_tests;
mod catalog;
#[cfg(test)]
mod catalog_tests;
mod collects;
#[cfg(test)]
mod collects_tests;
mod collects_text;
mod date;
mod db;
mod detail;
#[cfg(test)]
mod detail_tests;
mod entry;
#[cfg(test)]
mod entry_tests;
mod error;
#[cfg(test)]
mod fixtures;
mod issue_number;
mod links;
mod lookup;
#[cfg(test)]
mod lookup_tests;
mod merge_series;
#[cfg(test)]
mod merge_series_tests;
mod migrate;
mod model;
mod money;
#[cfg(test)]
mod schema_tests;
mod store;

pub use browse::{CopyRow, Shelf, ShelfEntry, ShelfList};
pub use collects::{CollectedIn, CollectedIssue, Ownership, PreviewEntry};
pub use collects_text::{
    CollectsEntry, MAX_ENTRIES, MAX_RANGE, SeriesRef, format_collects, parse_collects,
};
pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use detail::{CopyDetail, EditionDetail, IssueDetail, ValueEntry};
pub use entry::{BookEntry, Prefill, SavedBook, SeriesChoice};
pub use error::{Error, Result};
pub use issue_number::{IssueNumber, IssueNumberError};
pub use links::{Appearance, ArcPart, Credit, Named, Signature};
pub use lookup::{Lookup, LookupId};
pub use merge_series::SeriesMerge;
pub use model::{
    Binding, BookCondition, BoxId, Category, CharacterId, Copy, CopyFields, CopyId, CoverDate,
    CreatorId, Distribution, Edition, EditionFields, EditionId, Grade, GradeType, Isbn, Issue,
    IssueFields, IssueId, MagazineFormat, PublisherId, Role, RowUuid, Series, SeriesFields,
    SeriesId, SignatureFields, StoryArcId, TagId, TeamId, TrashGroup, TrashId, Verification,
};
pub use money::{
    Amount, ConversionError, Currency, ExchangeRate, ParseAmountError, RateError, RateTable,
    STALE_AFTER_DAYS,
};
pub use store::{CollectionInfo, CollectionStore, MAX_NAME};
