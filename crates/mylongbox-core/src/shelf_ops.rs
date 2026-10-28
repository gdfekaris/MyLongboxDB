//! Renaming, merging, and deleting the entry behind a shelf, whatever
//! list it belongs to, for the management menu on a shelf's page.

use std::collections::HashSet;

use crate::browse::{Shelf, ShelfEntry, ShelfList};
use crate::catalog::{DROP_EMPTY_TRASH_GROUPS, LabelMatch, match_series_label, query_all};
use crate::db::Database;
use crate::error::{Error, Result};
use crate::lookup::LookupId;
use crate::merge_series::SeriesMerge;
use crate::model::{SeriesId, TrashId};
use crate::search::json_ids;

/// Runs `$body` with `$id` bound to the shelf's typed id, for every shelf
/// that names an entry of a name list. Series and whole sections are
/// handled by `$other`.
macro_rules! for_lookup {
    ($shelf:expr, $id:ident => $body:expr, $other:pat => $else:expr) => {
        match $shelf {
            Shelf::Publisher($id) => $body,
            Shelf::Creator($id) => $body,
            Shelf::Character($id) => $body,
            Shelf::Team($id) => $body,
            Shelf::StoryArc($id) => $body,
            Shelf::StorageBox($id) => $body,
            $other => $else,
        }
    };
}

fn not_an_entry<T>() -> Result<T> {
    Err(Error::Invalid("this section is not a single entry".into()))
}

/// The ids of the series among `entries`, as a JSON array.
fn series_ids(entries: &[ShelfEntry]) -> String {
    json_ids(entries.iter().filter_map(|e| match e.shelf {
        Shelf::Series(id) => Some(id.0),
        _ => None,
    }))
}

impl Database {
    /// Gives the entry behind a shelf a new name.
    pub fn rename_shelf(&self, shelf: Shelf, name: &str) -> Result<()> {
        for_lookup!(shelf, id => self.rename_lookup(id, name), other => match other {
            Shelf::Series(id) => {
                let mut fields = self.series(id)?.fields;
                fields.name = name.to_owned();
                self.update_series(id, &fields)
            }
            _ => not_an_entry(),
        })
    }

    /// Merges the entry behind `from` into the entry of the same list
    /// named `into` (a name or alternate name; for a series, its name and
    /// year as listed), and returns the shelf of the entry that remains.
    pub fn merge_shelf(&self, from: Shelf, into: &str) -> Result<Shelf> {
        fn target<I: LookupId>(db: &Database, from: I, into: &str) -> Result<I> {
            let target = db.find_lookup::<I>(into)?.ok_or_else(|| {
                Error::Invalid(format!(
                    "nothing is listed as \u{201c}{}\u{201d}",
                    into.trim()
                ))
            })?;
            db.merge_lookups(from, target)?;
            Ok(target)
        }
        Ok(match from {
            Shelf::Publisher(id) => Shelf::Publisher(target(self, id, into)?),
            Shelf::Creator(id) => Shelf::Creator(target(self, id, into)?),
            Shelf::Character(id) => Shelf::Character(target(self, id, into)?),
            Shelf::Team(id) => Shelf::Team(target(self, id, into)?),
            Shelf::StoryArc(id) => Shelf::StoryArc(target(self, id, into)?),
            Shelf::StorageBox(id) => Shelf::StorageBox(target(self, id, into)?),
            Shelf::Series(id) => Shelf::Series(self.merge_series_named(id, into)?.0),
            _ => return not_an_entry(),
        })
    }

    /// Merges series `from` into the one of the same kind listed as
    /// `into` (see [`Database::shelf_suggestions`]), returning that series
    /// and what the merge did.
    pub fn merge_series_named(
        &self,
        from: SeriesId,
        into: &str,
    ) -> Result<(SeriesId, SeriesMerge)> {
        let category = self.series(from)?.fields.category;
        let typed = into.trim();
        let labels = self.series_labels()?;
        let others = labels
            .iter()
            .filter(|s| s.id != from && s.category == category);
        let into = match match_series_label(others, typed) {
            LabelMatch::One(id) => id,
            LabelMatch::None => {
                return Err(Error::Invalid(format!(
                    "no series of the same kind is listed as \u{201c}{typed}\u{201d}"
                )));
            }
            LabelMatch::Several => {
                return Err(Error::Invalid(format!(
                    "more than one series is listed as \u{201c}{typed}\u{201d}; \
                     choose one from the suggestions"
                )));
            }
        };
        Ok((into, self.merge_series(from, into)?))
    }

    /// Deletes the entry behind a shelf. A series goes to Trash with
    /// everything in it, so its group is returned; a name in a list is
    /// deleted outright and only while nothing uses it.
    pub fn delete_shelf(&self, shelf: Shelf) -> Result<Option<TrashId>> {
        for_lookup!(shelf, id => self.delete_lookup(id).map(|()| None), other => match other {
            Shelf::Series(id) => self.trash_series(id).map(Some),
            _ => not_an_entry(),
        })
    }

    /// Adds a name to a list before any book uses it, and returns its
    /// shelf. Series have more to them; see [`Database::add_series`].
    pub fn add_to_list(&self, list: ShelfList, name: &str) -> Result<Shelf> {
        Ok(match list {
            ShelfList::Publishers => Shelf::Publisher(self.create_lookup(name)?),
            ShelfList::Creators => Shelf::Creator(self.create_lookup(name)?),
            ShelfList::Characters => Shelf::Character(self.create_lookup(name)?),
            ShelfList::Teams => Shelf::Team(self.create_lookup(name)?),
            ShelfList::StoryArcs => Shelf::StoryArc(self.create_lookup(name)?),
            ShelfList::StorageBoxes => Shelf::StorageBox(self.create_lookup(name)?),
            ShelfList::Series => return not_an_entry(),
        })
    }

    /// The names in a list that hold no books, as the list shows them
    /// and counts them: names used only by books in Trash, or by issues
    /// and editions with no copy left, are unused too. A series is kept
    /// while a collected edition outside the unused ones reprints one
    /// of its issues, so the edition keeps what it collects.
    pub fn unused_in_list(&self, list: ShelfList) -> Result<Vec<String>> {
        if list == ShelfList::Series {
            return Ok(self.unused_series()?.into_iter().map(|e| e.name).collect());
        }
        Ok(self
            .shelf_list(list)?
            .into_iter()
            .filter(|e| e.copies == 0)
            .map(|e| e.name)
            .collect())
    }

    /// Deletes every name [`Database::unused_in_list`] gives, as one
    /// unit, and returns how many went. Each goes with its links on
    /// books in Trash and on issues with no copy; an unused series goes
    /// with all its issues, in Trash or not.
    pub fn remove_unused(&self, list: ShelfList) -> Result<usize> {
        let tx = self.tx()?;
        let removed = if list == ShelfList::Series {
            let unused = self.unused_series()?;
            // One statement, so reprints between the series going are
            // checked only once they are all gone.
            tx.execute(
                "DELETE FROM series WHERE id IN (SELECT value FROM json_each(?1))",
                [series_ids(&unused)],
            )?;
            tx.execute(DROP_EMPTY_TRASH_GROUPS, [])?;
            unused.len()
        } else {
            let unused: Vec<ShelfEntry> = self
                .shelf_list(list)?
                .into_iter()
                .filter(|e| e.copies == 0)
                .collect();
            for entry in &unused {
                for_lookup!(entry.shelf, id => self.delete_lookup_and_links(id)?, _other => not_an_entry()?);
            }
            unused.len()
        };
        tx.commit()?;
        Ok(removed)
    }

    /// The series outside Trash with no copy of any of their issues,
    /// less those a collected edition in another series reprints from.
    fn unused_series(&self) -> Result<Vec<ShelfEntry>> {
        let mut unused: Vec<ShelfEntry> = self
            .shelf_list(ShelfList::Series)?
            .into_iter()
            .filter(|e| e.copies == 0)
            .collect();
        // Keeping one series can keep another that it reprints from.
        loop {
            let reprinted: HashSet<SeriesId> = query_all(
                &self.conn,
                "SELECT DISTINCT i.series_id FROM collects k
                 JOIN issue i ON i.id = k.collected_issue_id
                 JOIN issue o ON o.id = k.collection_issue_id
                 WHERE i.series_id IN (SELECT value FROM json_each(?1))
                     AND o.series_id NOT IN (SELECT value FROM json_each(?1))",
                [series_ids(&unused)],
                |row| row.get(0),
            )?
            .into_iter()
            .collect();
            if reprinted.is_empty() {
                return Ok(unused);
            }
            unused.retain(|e| !matches!(e.shelf, Shelf::Series(id) if reprinted.contains(&id)));
        }
    }

    /// The alternate names of the entry behind a shelf; empty for lists
    /// without them.
    pub fn shelf_aliases(&self, shelf: Shelf) -> Result<Vec<String>> {
        for_lookup!(shelf, id => Ok(self.lookup(id)?.aliases), other => match other {
            Shelf::Series(_) => Ok(Vec::new()),
            _ => not_an_entry(),
        })
    }

    pub fn add_shelf_alias(&self, shelf: Shelf, alias: &str) -> Result<()> {
        for_lookup!(shelf, id => self.add_alias(id, alias), _other => not_an_entry())
    }

    pub fn remove_shelf_alias(&self, shelf: Shelf, alias: &str) -> Result<()> {
        for_lookup!(shelf, id => self.remove_alias(id, alias), _other => not_an_entry())
    }

    /// Names in the same list as `shelf` that contain `text`, other than
    /// its own, for choosing what to merge into.
    pub fn shelf_suggestions(&self, shelf: Shelf, text: &str) -> Result<Vec<String>> {
        fn names<I: LookupId>(db: &Database, own: I, text: &str) -> Result<Vec<String>> {
            Ok(db
                .suggest_lookups::<I>(text, 9)?
                .into_iter()
                .filter(|l| l.id.raw() != own.raw())
                .map(|l| l.name)
                .collect())
        }
        match shelf {
            Shelf::Publisher(id) => names(self, id, text),
            Shelf::Creator(id) => names(self, id, text),
            Shelf::Character(id) => names(self, id, text),
            Shelf::Team(id) => names(self, id, text),
            Shelf::StoryArc(id) => names(self, id, text),
            Shelf::StorageBox(id) => names(self, id, text),
            Shelf::Series(own) => {
                let category = self.series(own)?.fields.category;
                let text = text.to_lowercase();
                Ok(self
                    .series_labels()?
                    .into_iter()
                    .filter(|s| {
                        s.id != own
                            && s.category == category
                            && s.label.to_lowercase().contains(&text)
                    })
                    .map(|s| s.label)
                    .take(9)
                    .collect())
            }
            _ => not_an_entry(),
        }
    }

    /// Whether the list behind a shelf keeps alternate names.
    pub fn shelf_has_aliases(shelf: Shelf) -> bool {
        matches!(
            shelf,
            Shelf::Creator(_) | Shelf::Character(_) | Shelf::Team(_) | Shelf::StoryArc(_)
        )
    }
}
