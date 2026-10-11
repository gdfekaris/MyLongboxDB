//! Changing many copies at once, as one unit of work.

use rusqlite::{OptionalExtension, params};

use crate::catalog::{live, new_trash_group};
use crate::date::Date;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{BoxId, Category, CopyId, GradeType, TagId, TrashId};
use crate::money::Amount;

/// What to change on every selected copy. Each part left `None` or
/// empty leaves that detail as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkChange {
    /// Moves the copies into this box after whatever is there, numbered
    /// in the order given. `Some(None)` takes them out of their boxes.
    pub storage_box: Option<Option<String>>,
    pub add_tags: Vec<String>,
    /// Who graded them. Collected editions have no grade, so they keep
    /// theirs.
    pub grade_type: Option<Option<GradeType>>,
    /// A value to record for each copy: date, amount, and source.
    pub value: Option<(Date, Amount, String)>,
}

impl Database {
    /// Applies `change` to every copy in `copies`; if any copy cannot take
    /// it, none of them change.
    pub fn bulk_edit(&self, copies: &[CopyId], change: &BulkChange) -> Result<()> {
        let tx = self.tx()?;
        let target_box = match &change.storage_box {
            Some(Some(name)) if !name.trim().is_empty() => Some(Some(self.name::<BoxId>(name)?)),
            Some(_) => Some(None),
            None => None,
        };
        let mut next_position = match target_box {
            Some(Some(b)) => {
                let last: Option<u32> = tx
                    .query_row(
                        "SELECT max(box_position) FROM copy WHERE box_id = ?1",
                        [b],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                last.unwrap_or(0)
            }
            _ => 0,
        };
        let tags = change
            .add_tags
            .iter()
            .filter(|t| !t.trim().is_empty())
            .map(|t| self.name::<TagId>(t))
            .collect::<Result<Vec<_>>>()?;

        for &copy in copies {
            let current = self.copy(copy)?;
            live(current.trash)?;
            let mut fields = current.fields.clone();
            match target_box {
                Some(Some(b)) if fields.storage_box == Some(b) => {}
                Some(Some(b)) => {
                    next_position = next_position
                        .checked_add(1)
                        .ok_or_else(|| Error::Invalid("that box is full".into()))?;
                    fields.storage_box = Some(b);
                    fields.box_position = Some(next_position);
                }
                Some(None) => {
                    fields.storage_box = None;
                    fields.box_position = None;
                }
                None => {}
            }
            if let Some(grade_type) = change.grade_type {
                let issue = self.edition(current.edition)?.issue;
                if self.category_of_issue(issue)? != Category::CollectedEdition {
                    fields.grade_type = grade_type;
                }
            }
            if fields != current.fields {
                self.update_copy(copy, &fields)?;
            }
            if !tags.is_empty() {
                let mut all: Vec<TagId> = self.tags(copy)?.into_iter().map(|t| t.id).collect();
                all.extend(&tags);
                self.set_tags(copy, &all)?;
            }
            if let Some((date, value, source)) = &change.value {
                self.add_value(copy, *date, *value, source)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves several copies to Trash as one group, so they come back
    /// together.
    pub fn trash_copies(&self, copies: &[CopyId]) -> Result<TrashId> {
        let tx = self.tx()?;
        let label = match copies.len() {
            0 => return Err(Error::Invalid("no copies were chosen".into())),
            1 => return self.trash_copy(copies[0]),
            n => format!("{n} copies"),
        };
        let trash = new_trash_group(&tx, &label)?;
        for &copy in copies {
            live(self.copy(copy)?.trash)?;
            tx.execute(
                "UPDATE copy SET trash_id = ?1 WHERE id = ?2",
                params![trash, copy],
            )?;
        }
        tx.commit()?;
        Ok(trash)
    }
}
