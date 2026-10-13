//! Links from catalog rows to name lists: credits, characters, teams,
//! story arcs, cover artists, and tags. Each setter replaces the whole
//! list at once, the way a form saves.

use std::collections::HashSet;

use rusqlite::params;

use crate::db::Database;
use crate::error::Result;
use crate::model::{
    CharacterId, CopyId, CreatorId, EditionId, IssueId, Role, SignatureFields, StoryArcId, TagId,
    TeamId,
};

/// A creator's credit on an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credit {
    pub creator: CreatorId,
    pub name: String,
    pub role: Role,
}

/// A character or team in an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance<I> {
    pub id: I,
    pub name: String,
    pub first_appearance: bool,
}

/// An issue's place in a story arc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArcPart {
    pub arc: StoryArcId,
    pub name: String,
    pub part: Option<u32>,
}

/// A signature on a copy and who signed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub signer: CreatorId,
    pub name: String,
    pub fields: SignatureFields,
}

/// A named entry linked without further detail: a tag or cover artist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named<I> {
    pub id: I,
    pub name: String,
}

impl Database {
    /// Credits in role order (writer, penciller, …), then by name.
    pub fn credits(&self, issue: IssueId) -> Result<Vec<Credit>> {
        let mut stmt = self.conn.prepare(
            "SELECT credit.creator_id, creator.name, credit.role FROM credit
             JOIN creator ON creator.id = credit.creator_id
             WHERE credit.issue_id = ?1
             ORDER BY creator.name COLLATE NOCASE",
        )?;
        let mut credits = stmt
            .query_map([issue], |row| {
                Ok(Credit {
                    creator: row.get(0)?,
                    name: row.get(1)?,
                    role: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Stable, so names stay alphabetical within each role.
        credits.sort_by_key(|c| Role::ALL.iter().position(|r| *r == c.role));
        Ok(credits)
    }

    pub fn set_credits(&self, issue: IssueId, credits: &[(CreatorId, Role)]) -> Result<()> {
        self.require_live_issue(issue)?;
        let tx = self.tx()?;
        tx.execute("DELETE FROM credit WHERE issue_id = ?1", [issue])?;
        for (creator, role) in unique(credits) {
            tx.execute(
                "INSERT INTO credit (issue_id, creator_id, role) VALUES (?1, ?2, ?3)",
                params![issue, creator, role],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn characters(&self, issue: IssueId) -> Result<Vec<Appearance<CharacterId>>> {
        let mut stmt = self.conn.prepare(
            "SELECT character.id, character.name, issue_character.first_appearance
             FROM issue_character JOIN character ON character.id = issue_character.character_id
             WHERE issue_character.issue_id = ?1
             ORDER BY character.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([issue], |row| {
            Ok(Appearance {
                id: row.get(0)?,
                name: row.get(1)?,
                first_appearance: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Characters with their first-appearance flags.
    pub fn set_characters(&self, issue: IssueId, characters: &[(CharacterId, bool)]) -> Result<()> {
        self.require_live_issue(issue)?;
        let tx = self.tx()?;
        tx.execute("DELETE FROM issue_character WHERE issue_id = ?1", [issue])?;
        for (character, first) in unique_by_id(characters) {
            tx.execute(
                "INSERT INTO issue_character (issue_id, character_id, first_appearance)
                 VALUES (?1, ?2, ?3)",
                params![issue, character, first],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn teams(&self, issue: IssueId) -> Result<Vec<Appearance<TeamId>>> {
        let mut stmt = self.conn.prepare(
            "SELECT team.id, team.name, issue_team.first_appearance
             FROM issue_team JOIN team ON team.id = issue_team.team_id
             WHERE issue_team.issue_id = ?1
             ORDER BY team.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([issue], |row| {
            Ok(Appearance {
                id: row.get(0)?,
                name: row.get(1)?,
                first_appearance: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Teams with their first-appearance flags.
    pub fn set_teams(&self, issue: IssueId, teams: &[(TeamId, bool)]) -> Result<()> {
        self.require_live_issue(issue)?;
        let tx = self.tx()?;
        tx.execute("DELETE FROM issue_team WHERE issue_id = ?1", [issue])?;
        for (team, first) in unique_by_id(teams) {
            tx.execute(
                "INSERT INTO issue_team (issue_id, team_id, first_appearance) VALUES (?1, ?2, ?3)",
                params![issue, team, first],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn story_arcs(&self, issue: IssueId) -> Result<Vec<ArcPart>> {
        let mut stmt = self.conn.prepare(
            "SELECT story_arc.id, story_arc.name, issue_story_arc.part
             FROM issue_story_arc JOIN story_arc ON story_arc.id = issue_story_arc.story_arc_id
             WHERE issue_story_arc.issue_id = ?1
             ORDER BY story_arc.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([issue], |row| {
            Ok(ArcPart {
                arc: row.get(0)?,
                name: row.get(1)?,
                part: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Story arcs with the issue's part number in each, when known.
    pub fn set_story_arcs(&self, issue: IssueId, arcs: &[(StoryArcId, Option<u32>)]) -> Result<()> {
        self.require_live_issue(issue)?;
        if arcs.iter().any(|(_, part)| *part == Some(0)) {
            return Err(crate::Error::Invalid("story arc parts start at 1".into()));
        }
        let tx = self.tx()?;
        tx.execute("DELETE FROM issue_story_arc WHERE issue_id = ?1", [issue])?;
        for (arc, part) in unique_by_id(arcs) {
            tx.execute(
                "INSERT INTO issue_story_arc (issue_id, story_arc_id, part) VALUES (?1, ?2, ?3)",
                params![issue, arc, part],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn cover_artists(&self, edition: EditionId) -> Result<Vec<Named<CreatorId>>> {
        let mut stmt = self.conn.prepare(
            "SELECT creator.id, creator.name FROM edition_cover_artist
             JOIN creator ON creator.id = edition_cover_artist.creator_id
             WHERE edition_cover_artist.edition_id = ?1
             ORDER BY creator.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([edition], |row| {
            Ok(Named {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_cover_artists(&self, edition: EditionId, artists: &[CreatorId]) -> Result<()> {
        let current = self.edition(edition)?;
        crate::catalog::live(current.trash)?;
        let tx = self.tx()?;
        tx.execute(
            "DELETE FROM edition_cover_artist WHERE edition_id = ?1",
            [edition],
        )?;
        for artist in unique(artists) {
            tx.execute(
                "INSERT INTO edition_cover_artist (edition_id, creator_id) VALUES (?1, ?2)",
                params![edition, artist],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A copy's signatures in the order signed.
    pub fn signatures(&self, copy: CopyId) -> Result<Vec<Signature>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.creator_id, creator.name, s.verification, s.cert_number, s.signed_on,
                 s.place, s.sketch, s.placement
             FROM signature s JOIN creator ON creator.id = s.creator_id
             WHERE s.copy_id = ?1
             ORDER BY s.position",
        )?;
        let rows = stmt.query_map([copy], |row| {
            Ok(Signature {
                signer: row.get(0)?,
                name: row.get(1)?,
                fields: SignatureFields {
                    verification: row.get(2)?,
                    cert_number: row.get(3)?,
                    signed_on: row.get(4)?,
                    place: row.get(5)?,
                    sketch: row.get(6)?,
                    placement: row.get(7)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces a copy's signatures. The same person may sign more than
    /// once, so repeats are kept, in the order given.
    pub fn set_signatures(
        &self,
        copy: CopyId,
        signatures: &[(CreatorId, SignatureFields)],
    ) -> Result<()> {
        let current = self.copy(copy)?;
        crate::catalog::live(current.trash)?;
        let tx = self.tx()?;
        tx.execute("DELETE FROM signature WHERE copy_id = ?1", [copy])?;
        for (position, (signer, f)) in (1u32..).zip(signatures) {
            let cert = f
                .cert_number
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty());
            tx.execute(
                "INSERT INTO signature (copy_id, position, creator_id, verification,
                     cert_number, signed_on, place, sketch, placement)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    copy,
                    position,
                    signer,
                    f.verification,
                    cert,
                    f.signed_on,
                    f.place.trim(),
                    f.sketch,
                    f.placement
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn tags(&self, copy: CopyId) -> Result<Vec<Named<TagId>>> {
        let mut stmt = self.conn.prepare(
            "SELECT tag.id, tag.name FROM copy_tag JOIN tag ON tag.id = copy_tag.tag_id
             WHERE copy_tag.copy_id = ?1
             ORDER BY tag.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([copy], |row| {
            Ok(Named {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_tags(&self, copy: CopyId, tags: &[TagId]) -> Result<()> {
        let current = self.copy(copy)?;
        crate::catalog::live(current.trash)?;
        let tx = self.tx()?;
        tx.execute("DELETE FROM copy_tag WHERE copy_id = ?1", [copy])?;
        for tag in unique(tags) {
            tx.execute(
                "INSERT INTO copy_tag (copy_id, tag_id) VALUES (?1, ?2)",
                params![copy, tag],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn require_live_issue(&self, issue: IssueId) -> Result<()> {
        crate::catalog::live(self.issue(issue)?.trash)
    }
}

/// Drops exact repeats, keeping the first of each.
fn unique<T: Copy + Eq + std::hash::Hash>(items: &[T]) -> Vec<T> {
    let mut seen = HashSet::new();
    items.iter().copied().filter(|i| seen.insert(*i)).collect()
}

/// Keeps the first entry for each id, so a list naming the same character
/// twice cannot fail on the table's primary key.
fn unique_by_id<I: Copy + Eq + std::hash::Hash, V: Copy>(items: &[(I, V)]) -> Vec<(I, V)> {
    let mut seen = HashSet::new();
    items
        .iter()
        .copied()
        .filter(|(id, _)| seen.insert(*id))
        .collect()
}
