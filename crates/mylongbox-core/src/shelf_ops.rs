//! Renaming, merging, and deleting the entry behind a shelf, whatever
//! list it belongs to, for the management menu on a shelf's page.

use std::collections::HashSet;

use crate::browse::{Shelf, ShelfList};
use crate::catalog::{LabelMatch, match_series_label, query_all};
use crate::db::Database;
use crate::error::{Error, Result};
use crate::lookup::LookupId;
use crate::merge_series::SeriesMerge;
use crate::model::{
    BoxId, CharacterId, CreatorId, PublisherId, SeriesId, StoryArcId, TeamId, TrashId,
};

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

    /// The names in a list that nothing uses, not even items in Trash,
    /// as the list shows them. A series is unused while it has no issues.
    pub fn unused_in_list(&self, list: ShelfList) -> Result<Vec<String>> {
        fn names<I: LookupId>(db: &Database) -> Result<Vec<String>> {
            Ok(db
                .unused_lookups::<I>()?
                .into_iter()
                .map(|l| l.name)
                .collect())
        }
        match list {
            ShelfList::Series => Ok(self
                .unused_series()?
                .into_iter()
                .map(|(_, label)| label)
                .collect()),
            ShelfList::Publishers => names::<PublisherId>(self),
            ShelfList::Creators => names::<CreatorId>(self),
            ShelfList::Characters => names::<CharacterId>(self),
            ShelfList::Teams => names::<TeamId>(self),
            ShelfList::StoryArcs => names::<StoryArcId>(self),
            ShelfList::StorageBoxes => names::<BoxId>(self),
        }
    }

    /// Deletes every name [`Database::unused_in_list`] gives, as one
    /// unit, and returns how many went.
    pub fn remove_unused(&self, list: ShelfList) -> Result<usize> {
        fn remove<I: LookupId>(db: &Database) -> Result<usize> {
            let unused = db.unused_lookups::<I>()?;
            for entry in &unused {
                db.delete_lookup(entry.id)?;
            }
            Ok(unused.len())
        }
        let tx = self.tx()?;
        let removed = match list {
            ShelfList::Series => {
                let unused = self.unused_series()?;
                for (id, _) in &unused {
                    self.conn.execute(
                        "DELETE FROM series WHERE id = ?1
                         AND NOT EXISTS (SELECT 1 FROM issue WHERE series_id = ?1)",
                        [id],
                    )?;
                }
                unused.len()
            }
            ShelfList::Publishers => remove::<PublisherId>(self)?,
            ShelfList::Creators => remove::<CreatorId>(self)?,
            ShelfList::Characters => remove::<CharacterId>(self)?,
            ShelfList::Teams => remove::<TeamId>(self)?,
            ShelfList::StoryArcs => remove::<StoryArcId>(self)?,
            ShelfList::StorageBoxes => remove::<BoxId>(self)?,
        };
        tx.commit()?;
        Ok(removed)
    }

    /// The series outside Trash with no issues, not even in Trash, with
    /// their labels.
    fn unused_series(&self) -> Result<Vec<(SeriesId, String)>> {
        let empty: HashSet<SeriesId> = query_all(
            &self.conn,
            "SELECT id FROM series s
             WHERE trash_id IS NULL AND NOT EXISTS (SELECT 1 FROM issue WHERE series_id = s.id)",
            [],
            |row| row.get(0),
        )?
        .into_iter()
        .collect();
        Ok(self
            .series_labels()?
            .into_iter()
            .filter(|s| empty.contains(&s.id))
            .map(|s| (s.id, s.label))
            .collect())
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
