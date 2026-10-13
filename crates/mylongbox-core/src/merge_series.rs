//! Folding a duplicate series into the one it duplicates.

use crate::catalog::live;
use crate::db::{Database, run_steps};
use crate::error::{Error, Result};
use crate::model::{IssueId, SeriesId};

/// What a series merge did, for the confirmation message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeriesMerge {
    /// Issues that moved across unchanged.
    pub moved: u32,
    /// Issues that matched one already there and were combined with it.
    pub combined: u32,
}

/// Combines issue ?1 into issue ?2: every link of ?1 is added to ?2
/// without duplicates, and ?2's blank fields are filled from ?1.
const COMBINE_ISSUE: &[&str] = &[
    "UPDATE edition SET issue_id = ?2 WHERE issue_id = ?1",
    "INSERT INTO credit (issue_id, creator_id, role)
     SELECT ?2, creator_id, role FROM credit AS old WHERE issue_id = ?1
         AND NOT EXISTS (SELECT 1 FROM credit
             WHERE issue_id = ?2 AND creator_id = old.creator_id AND role = old.role)",
    "UPDATE issue_character SET first_appearance = 1
     WHERE issue_id = ?2 AND character_id IN (
         SELECT character_id FROM issue_character WHERE issue_id = ?1 AND first_appearance = 1)",
    "INSERT INTO issue_character (issue_id, character_id, first_appearance)
     SELECT ?2, character_id, first_appearance FROM issue_character AS old WHERE issue_id = ?1
         AND NOT EXISTS (SELECT 1 FROM issue_character
             WHERE issue_id = ?2 AND character_id = old.character_id)",
    "UPDATE issue_team SET first_appearance = 1
     WHERE issue_id = ?2 AND team_id IN (
         SELECT team_id FROM issue_team WHERE issue_id = ?1 AND first_appearance = 1)",
    "INSERT INTO issue_team (issue_id, team_id, first_appearance)
     SELECT ?2, team_id, first_appearance FROM issue_team AS old WHERE issue_id = ?1
         AND NOT EXISTS (SELECT 1 FROM issue_team WHERE issue_id = ?2 AND team_id = old.team_id)",
    "UPDATE issue_story_arc SET part = (
         SELECT old.part FROM issue_story_arc AS old
         WHERE old.issue_id = ?1 AND old.story_arc_id = issue_story_arc.story_arc_id)
     WHERE issue_id = ?2 AND part IS NULL",
    "INSERT INTO issue_story_arc (issue_id, story_arc_id, part)
     SELECT ?2, story_arc_id, part FROM issue_story_arc AS old WHERE issue_id = ?1
         AND NOT EXISTS (SELECT 1 FROM issue_story_arc
             WHERE issue_id = ?2 AND story_arc_id = old.story_arc_id)",
    // Books that reprint ?1 now reprint ?2, once each.
    "DELETE FROM collects WHERE collected_issue_id = ?1 AND collection_issue_id IN (
         SELECT collection_issue_id FROM collects WHERE collected_issue_id = ?2)",
    "UPDATE collects SET collected_issue_id = ?2 WHERE collected_issue_id = ?1",
    // When both are collected editions, ?1's contents follow ?2's.
    "INSERT INTO collects (collection_issue_id, collected_issue_id, position)
     SELECT ?2, collected_issue_id,
         (SELECT coalesce(max(position), 0) FROM collects WHERE collection_issue_id = ?2)
             + position
     FROM collects AS old WHERE collection_issue_id = ?1
         AND NOT EXISTS (SELECT 1 FROM collects
             WHERE collection_issue_id = ?2 AND collected_issue_id = old.collected_issue_id)",
    "UPDATE issue SET
         title = CASE WHEN title = '' THEN (SELECT title FROM issue WHERE id = ?1) ELSE title END,
         cover_month = CASE WHEN cover_year IS NULL
             THEN (SELECT cover_month FROM issue WHERE id = ?1) ELSE cover_month END,
         cover_year = coalesce(cover_year, (SELECT cover_year FROM issue WHERE id = ?1)),
         release_date = coalesce(release_date, (SELECT release_date FROM issue WHERE id = ?1)),
         page_count = coalesce(page_count, (SELECT page_count FROM issue WHERE id = ?1)),
         synopsis = CASE WHEN synopsis = ''
             THEN (SELECT synopsis FROM issue WHERE id = ?1) ELSE synopsis END,
         key_reason = CASE WHEN key_reason = ''
             THEN (SELECT key_reason FROM issue WHERE id = ?1) ELSE key_reason END,
         is_key = max(is_key, (SELECT is_key FROM issue WHERE id = ?1))
     WHERE id = ?2",
    "DELETE FROM issue WHERE id = ?1",
];

impl Database {
    /// Folds `from` into `into`. Issues only in `from` move across; an
    /// issue whose number `into` already has is combined with it, so its
    /// editions and copies sit under one issue, each copy still its own
    /// entry. Items in Trash move too. `into` keeps its own details and
    /// fills its blanks from `from`, which is then gone.
    pub fn merge_series(&self, from: SeriesId, into: SeriesId) -> Result<SeriesMerge> {
        if from == into {
            return Err(Error::Invalid(
                "choose a different series to merge into".into(),
            ));
        }
        let tx = self.tx()?;
        let (source, target) = (self.series(from)?, self.series(into)?);
        live(source.trash)?;
        live(target.trash)?;
        if source.fields.category != target.fields.category {
            return Err(Error::Invalid(
                "only series of the same kind can be merged; change one's category first".into(),
            ));
        }

        let pairs: Vec<(IssueId, IssueId)> = {
            let mut stmt = tx.prepare(
                "SELECT old.id, new.id FROM issue AS old
                 JOIN issue AS new ON new.series_id = ?2 AND new.number_sort = old.number_sort
                     AND new.trash_id IS NULL AND new.number IS NOT NULL
                 WHERE old.series_id = ?1 AND old.trash_id IS NULL AND old.number IS NOT NULL",
            )?;
            stmt.query_map([from, into], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (old, new) in &pairs {
            run_steps(&tx, COMBINE_ISSUE, [old.0, new.0])?;
        }

        let moved_live: i64 = tx.query_row(
            "SELECT count(*) FROM issue WHERE series_id = ?1 AND trash_id IS NULL",
            [from],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE issue SET series_id = ?2 WHERE series_id = ?1",
            [from, into],
        )?;
        tx.execute(
            "UPDATE series SET
                 publisher_id = coalesce(publisher_id, (SELECT publisher_id FROM series WHERE id = ?1)),
                 volume = coalesce(volume, (SELECT volume FROM series WHERE id = ?1)),
                 start_year = coalesce(start_year, (SELECT start_year FROM series WHERE id = ?1)),
                 known_issue_count = coalesce(known_issue_count,
                     (SELECT known_issue_count FROM series WHERE id = ?1)),
                 magazine_format = coalesce(magazine_format,
                     (SELECT magazine_format FROM series WHERE id = ?1)),
                 black_and_white = coalesce(black_and_white,
                     (SELECT black_and_white FROM series WHERE id = ?1)),
                 notes = CASE WHEN notes = '' THEN (SELECT notes FROM series WHERE id = ?1)
                     ELSE notes END
             WHERE id = ?2",
            [from, into],
        )?;
        tx.execute("DELETE FROM series WHERE id = ?1", [from])?;
        self.rules_follow_series_merge(from, into)?;
        self.series_rules_follow(&source.fields.name, &target.fields.name)?;
        tx.commit()?;
        Ok(SeriesMerge {
            moved: u32::try_from(moved_live).unwrap_or(u32::MAX),
            combined: u32::try_from(pairs.len()).unwrap_or(u32::MAX),
        })
    }
}
