//! Everything the detail page shows about one issue: its series, credits,
//! contents, and every edition and copy the owner has of it.

use crate::catalog::{issue_label, live};
use crate::collects::{CollectedIn, CollectedIssue, Ownership};
use crate::db::Database;
use crate::error::Result;
use crate::links::{Appearance, ArcPart, Credit, Named, Signature};
use crate::model::{
    BoxId, CharacterId, Copy, CreatorId, Edition, Issue, IssueId, PublisherId, Series, Side, TagId,
    TeamId,
};
use crate::values::ValueEntry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyDetail {
    pub copy: Copy,
    pub storage_box: Option<String>,
    pub tags: Vec<Named<TagId>>,
    /// In the order signed.
    pub signatures: Vec<Signature>,
    /// Valuations, oldest first; the last is what it is worth now.
    pub values: Vec<ValueEntry>,
    /// The sides that have photos, front first.
    pub photos: Vec<Side>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditionDetail {
    pub edition: Edition,
    pub cover_artists: Vec<Named<CreatorId>>,
    /// Empty when the edition is only a catalog entry.
    pub copies: Vec<CopyDetail>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueDetail {
    pub issue: Issue,
    pub series: Series,
    /// e.g. `Amazing Spider-Man (1963) #121`.
    pub label: String,
    pub publisher: Option<String>,
    pub credits: Vec<Credit>,
    pub characters: Vec<Appearance<CharacterId>>,
    pub teams: Vec<Appearance<TeamId>>,
    pub story_arcs: Vec<ArcPart>,
    /// Editions outside Trash, oldest first.
    pub editions: Vec<EditionDetail>,
    /// What a collected edition reprints; empty for other issues.
    pub collects: Vec<CollectedIssue>,
    /// Collected editions that reprint this issue.
    pub collected_in: Vec<CollectedIn>,
    pub ownership: Ownership,
}

impl Database {
    /// The detail page of an issue that is not in Trash, read as one
    /// consistent snapshot.
    pub fn issue_detail(&self, id: IssueId) -> Result<IssueDetail> {
        let tx = self.tx()?;
        let issue = self.issue(id)?;
        // Trashing a series marks its issues too.
        live(issue.trash)?;
        let series = self.series(issue.series)?;
        let publisher = series
            .fields
            .publisher
            .map(|p| self.lookup::<PublisherId>(p).map(|l| l.name))
            .transpose()?;
        let editions = self
            .editions_of_issue(id)?
            .into_iter()
            .map(|edition| {
                let copies = self
                    .copies_of_edition(edition.id)?
                    .into_iter()
                    .map(|copy| self.copy_detail(copy))
                    .collect::<Result<_>>()?;
                Ok(EditionDetail {
                    cover_artists: self.cover_artists(edition.id)?,
                    edition,
                    copies,
                })
            })
            .collect::<Result<_>>()?;
        let detail = IssueDetail {
            label: issue_label(&series, &issue),
            publisher,
            credits: self.credits(id)?,
            characters: self.characters(id)?,
            teams: self.teams(id)?,
            story_arcs: self.story_arcs(id)?,
            editions,
            collects: self.collects(id)?,
            collected_in: self.collected_in(id)?,
            ownership: self.ownership(id)?,
            issue,
            series,
        };
        tx.commit()?;
        Ok(detail)
    }

    fn copy_detail(&self, copy: Copy) -> Result<CopyDetail> {
        let storage_box = copy
            .fields
            .storage_box
            .map(|b| self.lookup::<BoxId>(b).map(|l| l.name))
            .transpose()?;
        Ok(CopyDetail {
            storage_box,
            tags: self.tags(copy.id)?,
            signatures: self.signatures(copy.id)?,
            values: self.values(copy.id)?,
            photos: self.photo_sides(copy.id)?,
            copy,
        })
    }
}
