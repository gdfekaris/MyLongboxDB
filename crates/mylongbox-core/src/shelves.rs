//! Saved shelves, filter sets the owner named and keeps in the sidebar,
//! and the built-in sidebar entries the owner chose to hide. Both belong
//! to the collection, so they travel with a copy of it.

use rusqlite::{OptionalExtension, params};

use crate::catalog::query_all;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::filter::{Filter, FilterSet, ListField, TextCondition, TextField};
use crate::model::{CharacterId, CreatorId, SavedShelfId, SeriesId, StoryArcId, TagId, TeamId};

/// The longest name a shelf may have, so it fits the sidebar.
pub const MAX_SHELF_NAME: usize = 60;

/// A named filter set.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedShelf {
    pub id: SavedShelfId,
    pub name: String,
    /// `None` when the rules were saved by a newer version of the app in
    /// a form this one cannot read; the shelf can still be renamed,
    /// moved, or removed.
    pub filters: Option<FilterSet>,
}

/// What a saved rule may mention by name, so a rename or merge can carry
/// the rule along with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    Series,
    Publisher,
    Creator,
    Character,
    Team,
    StoryArc,
    Tag,
    StorageBox,
}

impl Named {
    /// The names in `rule` that mean an entry of this kind.
    fn names_in(self, rule: &mut Filter) -> Vec<&mut String> {
        match (self, rule) {
            (Self::Creator, Filter::Person { name, .. }) => vec![name],
            (
                _,
                Filter::List {
                    field, names: list, ..
                },
            ) if self.lists().contains(field) => list.iter_mut().collect(),
            (
                _,
                Filter::Text {
                    field,
                    condition: TextCondition::Is,
                    value,
                },
            ) if self.text() == Some(*field) => vec![value],
            _ => Vec::new(),
        }
    }

    fn lists(self) -> &'static [ListField] {
        match self {
            Self::Creator => &[ListField::CoverArtists, ListField::SignedBy],
            Self::Character => &[ListField::Characters],
            Self::Team => &[ListField::Teams],
            Self::StoryArc => &[ListField::StoryArcs],
            Self::Tag => &[ListField::Tags],
            Self::Series | Self::Publisher | Self::StorageBox => &[],
        }
    }

    /// The text field whose "is" rules name an entry of this kind.
    fn text(self) -> Option<TextField> {
        match self {
            Self::Series => Some(TextField::SeriesName),
            Self::Publisher => Some(TextField::Publisher),
            Self::StorageBox => Some(TextField::StorageBox),
            _ => None,
        }
    }
}

fn clean_shelf_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(Error::Invalid("enter a name for the shelf".into()));
    }
    if name.chars().count() > MAX_SHELF_NAME {
        return Err(Error::Invalid(format!(
            "a shelf name can be at most {MAX_SHELF_NAME} characters"
        )));
    }
    Ok(name)
}

fn rules_json(filters: &FilterSet) -> Result<String> {
    serde_json::to_string(filters)
        .map_err(|e| Error::Invalid(format!("the filters could not be saved: {e}")))
}

impl Database {
    /// Every saved shelf, in the order the sidebar lists them.
    pub fn saved_shelves(&self) -> Result<Vec<SavedShelf>> {
        query_all(
            &self.conn,
            "SELECT id, name, rules FROM saved_shelf ORDER BY position, id",
            [],
            |row| {
                let rules: String = row.get(2)?;
                Ok(SavedShelf {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    filters: serde_json::from_str(&rules).ok(),
                })
            },
        )
    }

    pub fn saved_shelf(&self, id: SavedShelfId) -> Result<SavedShelf> {
        self.saved_shelves()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or(Error::NotFound)
    }

    /// Saves `filters` as a shelf named `name`, listed after the others.
    pub fn add_saved_shelf(&self, name: &str, filters: &FilterSet) -> Result<SavedShelfId> {
        let name = clean_shelf_name(name)?;
        let rules = rules_json(filters)?;
        let tx = self.tx()?;
        self.check_shelf_name_free(&name, None)?;
        tx.execute(
            "INSERT INTO saved_shelf (name, position, rules)
             VALUES (?1, (SELECT coalesce(max(position), 0) + 1 FROM saved_shelf), ?2)",
            params![name, rules],
        )?;
        let id = SavedShelfId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }

    pub fn rename_saved_shelf(&self, id: SavedShelfId, name: &str) -> Result<()> {
        let name = clean_shelf_name(name)?;
        let tx = self.tx()?;
        self.check_shelf_name_free(&name, Some(id))?;
        let changed = tx.execute(
            "UPDATE saved_shelf SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        tx.commit()?;
        Ok(())
    }

    /// Replaces a shelf's filters, keeping its name and place.
    pub fn set_saved_shelf_filters(&self, id: SavedShelfId, filters: &FilterSet) -> Result<()> {
        let rules = rules_json(filters)?;
        let changed = self.conn.execute(
            "UPDATE saved_shelf SET rules = ?2 WHERE id = ?1",
            params![id, rules],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    pub fn delete_saved_shelf(&self, id: SavedShelfId) -> Result<()> {
        let changed = self
            .conn
            .execute("DELETE FROM saved_shelf WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    /// Moves a shelf one place earlier in the sidebar, or one later; at
    /// either end it stays where it is.
    pub fn move_saved_shelf(&self, id: SavedShelfId, earlier: bool) -> Result<()> {
        let tx = self.tx()?;
        let mut order: Vec<SavedShelfId> =
            self.saved_shelves()?.into_iter().map(|s| s.id).collect();
        let at = order.iter().position(|s| *s == id).ok_or(Error::NotFound)?;
        let to = if earlier {
            at.checked_sub(1)
        } else {
            Some(at + 1).filter(|n| *n < order.len())
        };
        if let Some(to) = to {
            order.swap(at, to);
            for (position, shelf) in (1i64..).zip(&order) {
                tx.execute(
                    "UPDATE saved_shelf SET position = ?2 WHERE id = ?1",
                    params![shelf, position],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn check_shelf_name_free(&self, name: &str, except: Option<SavedShelfId>) -> Result<()> {
        let existing: Option<(SavedShelfId, String)> = self
            .conn
            .query_row(
                "SELECT id, name FROM saved_shelf WHERE name = ?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((id, existing)) if Some(id) != except => Err(Error::NameTaken {
                name: name.to_owned(),
                existing,
            }),
            _ => Ok(()),
        }
    }

    /// The codes of the built-in sidebar entries the owner hid.
    pub fn hidden_sections(&self) -> Result<Vec<String>> {
        query_all(
            &self.conn,
            "SELECT code FROM hidden_section ORDER BY code",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_section_hidden(&self, code: &str, hidden: bool) -> Result<()> {
        if code.is_empty() {
            return Err(Error::Invalid("a sidebar entry needs a code".into()));
        }
        if hidden {
            self.conn.execute(
                "INSERT INTO hidden_section (code) VALUES (?1) ON CONFLICT DO NOTHING",
                [code],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM hidden_section WHERE code = ?1", [code])?;
        }
        Ok(())
    }

    /// The names and series the rules mention that the collection no
    /// longer lists, so a shelf can say why part of it finds nothing.
    pub fn missing_from(&self, filters: &FilterSet) -> Result<Vec<String>> {
        let mut missing = Vec::new();
        let mut note = |name: &str| {
            if !missing
                .iter()
                .any(|m: &String| m.eq_ignore_ascii_case(name))
            {
                missing.push(name.to_owned());
            }
        };
        for rule in &filters.rules {
            match rule {
                Filter::Person { name, .. } => {
                    if self.find_lookup::<CreatorId>(name)?.is_none() {
                        note(name);
                    }
                }
                Filter::List { field, names, .. } => {
                    for name in names {
                        let found = match field {
                            ListField::Characters => {
                                self.find_lookup::<CharacterId>(name)?.is_some()
                            }
                            ListField::Teams => self.find_lookup::<TeamId>(name)?.is_some(),
                            ListField::StoryArcs => self.find_lookup::<StoryArcId>(name)?.is_some(),
                            ListField::Tags => self.find_lookup::<TagId>(name)?.is_some(),
                            ListField::CoverArtists | ListField::SignedBy => {
                                self.find_lookup::<CreatorId>(name)?.is_some()
                            }
                        };
                        if !found {
                            note(name);
                        }
                    }
                }
                Filter::Collects { series, .. } => {
                    let live: Option<i64> = self
                        .conn
                        .query_row(
                            "SELECT 1 FROM series WHERE id = ?1 AND trash_id IS NULL",
                            [series],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if live.is_none() {
                        note("a series that is no longer listed");
                    }
                }
                _ => {}
            }
        }
        Ok(missing)
    }

    /// Carries saved rules that name `old` over to `new` after an entry
    /// of kind `what` was renamed or merged away. Runs inside the change's
    /// own transaction, so both happen or neither does.
    pub(crate) fn rules_follow_rename(&self, what: Named, old: &str, new: &str) -> Result<()> {
        if old == new {
            return Ok(());
        }
        self.rewrite_rules(|rule| {
            let mut changed = false;
            for name in what.names_in(rule) {
                if name.eq_ignore_ascii_case(old) {
                    new.clone_into(name);
                    changed = true;
                }
            }
            changed
        })
    }

    /// Points rules about the collected editions of `from` at `into`, the
    /// series it was merged into.
    pub(crate) fn rules_follow_series_merge(&self, from: SeriesId, into: SeriesId) -> Result<()> {
        self.rewrite_rules(|rule| match rule {
            Filter::Collects { series, .. } if *series == from => {
                *series = into;
                true
            }
            _ => false,
        })
    }

    /// Applies `change` to every readable rule, saving the shelves whose
    /// rules it changed. Rules this version cannot read are left alone.
    fn rewrite_rules(&self, change: impl Fn(&mut Filter) -> bool) -> Result<()> {
        for shelf in self.saved_shelves()? {
            let Some(mut filters) = shelf.filters else {
                continue;
            };
            let mut changed = false;
            for rule in &mut filters.rules {
                changed |= change(rule);
            }
            if changed {
                self.set_saved_shelf_filters(shelf.id, &filters)?;
            }
        }
        Ok(())
    }
}
