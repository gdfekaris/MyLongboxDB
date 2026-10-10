use std::fmt;

/// Errors from opening or working with a collection.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// The file is not a MyLongboxDB collection: it is not a SQLite
    /// database, or it is one that belongs to another application.
    NotACollection,
    /// The collection was written by a newer version of MyLongboxDB.
    /// Opening it with this version could damage it.
    NewerSchema {
        found: u32,
        supported: u32,
    },
    /// The schema version recorded in the `meta` table is unreadable.
    CorruptMeta,
    /// The file's tables, indexes, or triggers differ from what this
    /// version of MyLongboxDB creates, so it may have been tampered with.
    UnexpectedSchema,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "database error: {e}"),
            Self::NotACollection => f.write_str("this file is not a MyLongboxDB collection"),
            Self::NewerSchema { found, supported } => write!(
                f,
                "this collection uses schema version {found}, but this version of \
                 MyLongboxDB supports only up to {supported}; please update the app"
            ),
            Self::CorruptMeta => f.write_str("the collection's schema version is unreadable"),
            Self::UnexpectedSchema => f.write_str(
                "this collection's structure has been changed outside MyLongboxDB, \
                 so it will not be opened",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if e.sqlite_error_code() == Some(rusqlite::ErrorCode::NotADatabase) {
            Self::NotACollection
        } else {
            Self::Sqlite(e)
        }
    }
}
