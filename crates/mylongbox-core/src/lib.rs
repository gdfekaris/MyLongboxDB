//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod date;
mod db;
mod error;
mod migrate;

pub use date::{Date, DateError};
pub use db::{APPLICATION_ID, Database};
pub use error::{Error, Result};
