//! Data model, database access, migrations, and search for MyLongboxDB.

#![forbid(unsafe_code)]

mod db;
mod error;
mod migrate;

pub use db::{APPLICATION_ID, Database};
pub use error::{Error, Result};
