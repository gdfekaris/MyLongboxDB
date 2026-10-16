//! Copies of the open collection made a few pages at a time, so a window
//! can keep drawing between steps. Slow work, such as checking for
//! damage, compacting, or exporting, then runs on the copy in another
//! thread, since the open collection itself is locked to its window.

use std::path::{Path, PathBuf};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::db::Database;
use crate::error::{Error, Result};

/// The file a copy is written to.
pub struct CopyTarget {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for CopyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyTarget")
            .field("path", &self.path)
            .finish()
    }
}

impl CopyTarget {
    /// A new file at `path`, which must not exist yet.
    pub fn create(path: &Path) -> Result<Self> {
        let path = std::path::absolute(path)?;
        if path.exists() {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::AlreadyExists,
            )));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let conn = Connection::open_with_flags(&path, flags)?;
        Ok(Self { conn, path })
    }

    /// Closes the finished copy, so it can be opened as a collection of
    /// its own, and returns where it is.
    pub fn finish(self) -> Result<PathBuf> {
        self.conn.close().map_err(|(_, e)| Error::Sqlite(e))?;
        Ok(self.path)
    }

    /// Deletes an unfinished copy.
    pub fn discard(self) {
        let path = self.path.clone();
        drop(self);
        let _ = std::fs::remove_file(path);
    }
}

/// A copy under way. Changes the collection takes meanwhile are carried
/// into the copy, so it always ends up matching the collection as it is
/// when the last step finishes.
pub struct CopyJob<'a> {
    backup: Backup<'a, 'a>,
}

impl Database {
    /// Begins copying the collection into `target`.
    pub fn start_copy<'a>(&'a self, target: &'a mut CopyTarget) -> Result<CopyJob<'a>> {
        Ok(CopyJob {
            backup: Backup::new(&self.conn, &mut target.conn)?,
        })
    }
}

impl CopyJob<'_> {
    /// Copies up to `pages` more pages, returning whether the copy is
    /// complete.
    pub fn step(&mut self, pages: i32) -> Result<bool> {
        Ok(match self.backup.step(pages)? {
            StepResult::Done => true,
            // The collection is held by its own connection, so these only
            // mean "not this time"; the next step tries again.
            StepResult::More | StepResult::Busy | StepResult::Locked => false,
            _ => false,
        })
    }

    /// The share of the collection copied so far, from 0 to 1. It drops
    /// back if the collection grows meanwhile.
    pub fn fraction(&self) -> f64 {
        let progress = self.backup.progress();
        if progress.pagecount <= 0 {
            return 0.0;
        }
        let done = progress.pagecount - progress.remaining;
        f64::from(done.max(0)) / f64::from(progress.pagecount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BookEntry, Category, IssueNumber, SeriesChoice, Shelf};

    fn add(db: &Database, number: &str) {
        let e = BookEntry::new(
            Category::Comic,
            SeriesChoice::New {
                name: "Thor".into(),
                start_year: None,
                volume: None,
                publisher: None,
                imprint: String::new(),
                magazine_format: None,
                black_and_white: None,
            },
            IssueNumber::new(number).ok(),
        );
        db.add_book(&e).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mylongbox-snapshot-{}-{name}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_copy_made_in_steps_matches_the_collection() {
        let db = Database::open_in_memory().unwrap();
        for n in 1..=40 {
            add(&db, &n.to_string());
        }
        let path = scratch("steps");
        let mut target = CopyTarget::create(&path).unwrap();
        assert!(CopyTarget::create(&path).is_err(), "never over a file");
        {
            let mut job = db.start_copy(&mut target).unwrap();
            let mut steps = 0;
            let mut last = 0.0;
            while !job.step(2).unwrap() {
                steps += 1;
                // Progress only goes back when the collection grows.
                if steps <= 3 {
                    assert!(job.fraction() >= last);
                }
                last = job.fraction();
                // A change half way through still reaches the copy.
                if steps == 3 {
                    add(&db, "41");
                }
            }
            assert!(steps > 3, "{steps}");
            assert!((job.fraction() - 1.0).abs() < f64::EPSILON);
        }
        let path = target.finish().unwrap();
        let copy = Database::open(&path).unwrap();
        assert_eq!(copy.browse(Shelf::Everything).unwrap().len(), 41);
        assert!(copy.integrity_check().unwrap().is_empty());
        drop(copy);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_unfinished_copy_can_be_thrown_away() {
        let db = Database::open_in_memory().unwrap();
        add(&db, "1");
        let path = scratch("discard");
        let mut target = CopyTarget::create(&path).unwrap();
        db.start_copy(&mut target).unwrap().step(1).unwrap();
        target.discard();
        assert!(!path.exists());
    }
}
