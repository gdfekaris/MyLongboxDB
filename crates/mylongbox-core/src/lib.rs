//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

// Its SQL fragments are shared with search.
#[macro_use]
mod browse;
#[cfg(test)]
mod browse_tests;
mod bulk;
#[cfg(test)]
mod bulk_tests;
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
mod export;
#[cfg(test)]
mod export_tests;
mod filter;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod fixtures;
mod gaps;
#[cfg(test)]
mod gaps_tests;
mod import;
#[cfg(test)]
mod import_tests;
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
mod photos;
#[cfg(test)]
mod photos_tests;
mod rates;
#[cfg(test)]
mod schema_tests;
mod search;
#[cfg(test)]
mod search_tests;
mod shelf_ops;
#[cfg(test)]
mod shelf_ops_tests;
mod store;
mod values;

pub use browse::{CopyRow, Need, Shelf, ShelfEntry, ShelfList};
pub use bulk::BulkChange;
pub use collects::{CollectedIn, CollectedIssue, Ownership, PreviewEntry};
pub use collects_text::{
    CollectsEntry, MAX_ENTRIES, MAX_RANGE, SeriesRef, format_collects, parse_collects,
};
pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use detail::{CopyDetail, EditionDetail, IssueDetail};
pub use entry::{BookEntry, Prefill, SavedBook, SeriesChoice};
pub use error::{Error, Result};
pub use export::COLUMNS as CSV_COLUMNS;
pub use filter::{
    Choice, Compare, DateCondition, DateField, Filter, FilterSet, Flag, ListCondition, ListField,
    MoneyField, NumberField, OwnedAs, Presence, TextCondition, TextField,
};
pub use gaps::SeriesGaps;
pub use import::{
    ArchiveReader, ImportReport, ImportRun, ImportSettings, ImportSource, Outcome as ImportOutcome,
    PendingPhoto, RowReport, auto_mapping,
};
pub use issue_number::{IssueNumber, IssueNumberError};
pub use links::{Appearance, ArcPart, Credit, Named, Signature};
pub use lookup::{Lookup, LookupId};
pub use merge_series::SeriesMerge;
pub use model::{
    Binding, BookCondition, BoxId, Category, CharacterId, Copy, CopyFields, CopyId, CoverDate,
    CreatorId, Distribution, Edition, EditionFields, EditionId, Grade, GradeType, Isbn, Issue,
    IssueFields, IssueId, MagazineFormat, PublisherId, Role, RowUuid, Series, SeriesFields,
    SeriesId, Side, SignatureFields, StoryArcId, TagId, TeamId, TrashGroup, TrashId, ValueId,
    Verification,
};
pub use money::{
    Amount, ConversionError, Currency, ExchangeRate, ParseAmountError, RateError, RateTable,
    STALE_AFTER_DAYS,
};
pub use photos::{CoverPhoto, MAX_PHOTO_SIDE};
pub use search::SearchResults;
pub use store::{CollectionInfo, CollectionStore, MAX_NAME, SafetyCopy};
pub use values::ValueEntry;
