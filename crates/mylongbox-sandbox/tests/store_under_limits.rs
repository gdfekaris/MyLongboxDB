//! The collection's whole-file work, run under the file limits the app
//! applies to itself, so nothing it does needs to write outside the
//! app's own folders, such as a temporary file in /tmp.

use std::fs;
use std::path::{Path, PathBuf};

use mylongbox_core::{
    BookEntry, Category, CollectionStore, CopyTarget, Database, IssueNumber, SeriesChoice,
    SeriesFields, Shelf,
};
use mylongbox_sandbox::{Enforced, limit_writes};

fn copy_in_steps(db: &Database, path: &Path) {
    let mut target = CopyTarget::create(path).unwrap();
    {
        let mut job = db.start_copy(&mut target).unwrap();
        while !job.step(8).unwrap() {}
    }
    target.finish().unwrap();
}

#[test]
fn exporting_checking_and_compacting_stay_in_the_app_folders() {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-under-limits");
    let _ = fs::remove_dir_all(&base);
    let data = base.join("data");
    let exports = base.join("exports");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&exports).unwrap();
    if limit_writes(&[&data, &exports]).unwrap() == Enforced::Not {
        eprintln!("skipped: Landlock is not available on this kernel");
        return;
    }

    let store = CollectionStore::new(data.join("collections")).unwrap();
    store.create("Comics").unwrap();
    let db = store.open("Comics").unwrap();
    for n in 1..=50 {
        let e = BookEntry::new(
            Category::Comic,
            SeriesChoice::New {
                name: "Thor".into(),
                start_year: None,
                volume: None,
                publisher: None,
                magazine_format: None,
                black_and_white: None,
            },
            IssueNumber::new(&n.to_string()).ok(),
        );
        db.add_book(&e).unwrap();
    }
    let mut fields = SeriesFields::new("Hulk", Category::Comic);
    fields.notes = "x".repeat(4_000_000);
    let hulk = db.create_series(&fields).unwrap();
    fields.notes.clear();
    db.update_series(hulk, &fields).unwrap();
    // Sorting every book at once, as the views and exports do.
    assert_eq!(db.browse(Shelf::Everything).unwrap().len(), 50);

    let staging = store.staging("check").unwrap();
    copy_in_steps(&db, &staging);
    assert!(store.check_copy(&staging).unwrap().is_empty());

    let staging = store.staging("export").unwrap();
    copy_in_steps(&db, &staging);
    store
        .finish_export(&staging, &exports.join("b.sqlite"))
        .unwrap();

    let staging = store.safety_staging("Comics").unwrap();
    copy_in_steps(&db, &staging);
    store
        .adopt_safety_copy("Comics", &staging, "before emptying Trash")
        .unwrap();

    let staging = store.staging("export").unwrap();
    copy_in_steps(&db, &staging);
    let mut csv = Vec::new();
    Database::open(&staging)
        .unwrap()
        .export_csv(None, &mut csv, false)
        .unwrap();
    store.discard(&staging);

    let staging = store.staging("compact").unwrap();
    copy_in_steps(&db, &staging);
    let (compacted, before, after) = store.compact_copy(&staging).unwrap();
    assert!(before > after + 3_000_000, "{before} {after}");
    drop(db);
    store.replace_with("Comics", &compacted).unwrap();
    let db = store.open("Comics").unwrap();
    assert_eq!(db.browse(Shelf::Everything).unwrap().len(), 50);
}
