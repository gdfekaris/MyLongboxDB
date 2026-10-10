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
    /// The requested row does not exist.
    NotFound,
    /// The row, or the row it would be added to, is in Trash.
    InTrash,
    /// A value was refused before writing; the text says which and why.
    Invalid(String),
    /// The series already has a live issue with this number.
    DuplicateIssueNumber {
        number: String,
    },
    /// Restoring would put something back under a parent that is still in
    /// Trash; restore the parent's group first.
    ParentInTrash {
        parent: String,
    },
    /// Another entry in the same list already has this name, either as
    /// its name or as one of its alternate names.
    NameTaken {
        name: String,
        existing: String,
    },
    /// The entry is still linked from this many places, which may include
    /// items in Trash; merge it or remove the links first.
    InUse {
        uses: u64,
    },
    /// An issue cannot be deleted for good while a collected edition that
    /// is not being deleted with it still lists it as reprinted.
    StillReprinted {
        issue: String,
        collection: String,
    },
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
            Self::NotFound => f.write_str("that item no longer exists"),
            Self::InTrash => f.write_str("that item is in Trash; restore it first"),
            Self::Invalid(why) => f.write_str(why),
            Self::DuplicateIssueNumber { number } => {
                write!(f, "this series already has an issue numbered {number}")
            }
            Self::ParentInTrash { parent } => {
                write!(f, "restore \u{201c}{parent}\u{201d} from Trash first")
            }
            Self::NameTaken { name, existing } => {
                if name.eq_ignore_ascii_case(existing) {
                    write!(f, "\u{201c}{existing}\u{201d} already exists")
                } else {
                    write!(
                        f,
                        "\u{201c}{name}\u{201d} is already a name for \u{201c}{existing}\u{201d}"
                    )
                }
            }
            Self::InUse { uses } => write!(
                f,
                "still used in {uses} place{}, which may include items in Trash; \
                 merge it into another entry instead",
                if *uses == 1 { "" } else { "s" }
            ),
            Self::StillReprinted { issue, collection } => write!(
                f,
                "{issue} cannot be deleted for good because {collection} reprints it; \
                 remove it from that book's contents or delete the book too"
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
