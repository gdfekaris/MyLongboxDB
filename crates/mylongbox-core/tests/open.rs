//! Opening collection files on disk.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use mylongbox_core::{Database, Error};

/// A fresh, empty directory removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("mylongbox-core-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn creates_and_reopens_a_collection() {
    let dir = TempDir::new();
    let path = dir.file("collection.sqlite");
    let version = Database::open(&path).unwrap().schema_version().unwrap();
    assert!(path.exists());
    assert_eq!(
        Database::open(&path).unwrap().schema_version().unwrap(),
        version
    );
}

#[test]
fn refuses_a_file_that_is_not_sqlite() {
    let dir = TempDir::new();
    let path = dir.file("notes.sqlite");
    fs::write(
        &path,
        b"this is plainly not a database, just some text padding it out".repeat(20),
    )
    .unwrap();
    assert!(matches!(Database::open(&path), Err(Error::NotACollection)));
}

#[test]
fn refuses_another_applications_database() {
    let dir = TempDir::new();
    let path = dir.file("other.sqlite");
    let other = rusqlite::Connection::open(&path).unwrap();
    other
        .execute_batch("CREATE TABLE bookmarks (url TEXT)")
        .unwrap();
    drop(other);
    assert!(matches!(Database::open(&path), Err(Error::NotACollection)));
    let other = rusqlite::Connection::open(&path).unwrap();
    let tables: i64 = other
        .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tables, 1, "the foreign database must be left untouched");
}

#[test]
fn uri_style_paths_are_plain_file_names() {
    // Read as a URI, this would open a throwaway in-memory database and
    // succeed. Read as a plain path, it names a file inside a directory
    // called "file:" that does not exist, so opening must fail.
    let dir = TempDir::new();
    let target = dir.file("collection.sqlite");
    let uri = PathBuf::from(format!("file:{}?mode=memory", target.display()));
    assert!(Database::open(&uri).is_err());
}

#[test]
fn refuses_to_follow_symlinks() {
    let dir = TempDir::new();
    let real = dir.file("real.sqlite");
    let link = dir.file("link.sqlite");
    Database::open(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(Database::open(&link).is_err());
}
