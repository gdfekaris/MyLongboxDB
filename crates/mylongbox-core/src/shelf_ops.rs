//! Renaming, merging, and deleting the entry behind a shelf, whatever
//! list it belongs to, for the management menu on a shelf's page.

use crate::browse::Shelf;
use crate::catalog::series_title;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::lookup::LookupId;
use crate::model::TrashId;

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
            Shelf::Series(id) => {
                let category = self.series(id)?.fields.category;
                let into = self
                    .list_series()?
                    .into_iter()
                    .find(|s| {
                        s.fields.category == category
                            && series_title(&s.fields.name, s.fields.start_year)
                                .eq_ignore_ascii_case(into.trim())
                    })
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "no series of the same kind is listed as \u{201c}{}\u{201d}",
                            into.trim()
                        ))
                    })?;
                self.merge_series(id, into.id)?;
                Shelf::Series(into.id)
            }
            _ => return not_an_entry(),
        })
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
                    .list_series()?
                    .into_iter()
                    .filter(|s| s.id != own && s.fields.category == category)
                    .map(|s| series_title(&s.fields.name, s.fields.start_year))
                    .filter(|label| label.to_lowercase().contains(&text))
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
