//! Saving a book from the add and edit forms: one physical copy together
//! with its series, issue, edition, and the names it mentions, as one
//! unit of work.

use std::collections::HashMap;

use rusqlite::OptionalExtension;

use crate::catalog::{check_number_free, clean_edition, live};
use crate::db::Database;
use crate::error::{Error, Result};
use crate::issue_number::IssueNumber;
use crate::lookup::LookupId;
use crate::model::{
    BoxId, Category, CharacterId, CopyFields, CopyId, CreatorId, EditionFields, EditionId,
    IssueFields, IssueId, MagazineFormat, PublisherId, Role, SeriesFields, SeriesId,
    SignatureFields, StoryArcId, TagId, TeamId,
};

/// The series a book belongs to: one picked from the list, or one typed
/// in, which is created unless a series of that name, year, and volume
/// already exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeriesChoice {
    Existing(SeriesId),
    New {
        name: String,
        start_year: Option<u16>,
        volume: Option<u32>,
        publisher: Option<String>,
        /// Such as Vertigo; empty when none.
        imprint: String,
        /// Magazines only.
        magazine_format: Option<MagazineFormat>,
        /// Magazines only.
        black_and_white: Option<bool>,
    },
}

/// Everything the form holds about one physical book. Names (creators,
/// characters, boxes, tags...) are text as typed: a name or alternate name
/// that is already listed is reused, and any other is added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookEntry {
    pub category: Category,
    pub series: SeriesChoice,
    pub issue: IssueFields,
    pub credits: Vec<(String, Role)>,
    /// Name and whether this is the first appearance.
    pub characters: Vec<(String, bool)>,
    pub teams: Vec<(String, bool)>,
    /// Name and part number.
    pub story_arcs: Vec<(String, Option<u32>)>,
    pub edition: EditionFields,
    pub cover_artists: Vec<String>,
    /// `storage_box` is ignored; the box is named in `storage_box` below.
    pub copy: CopyFields,
    pub storage_box: Option<String>,
    pub tags: Vec<String>,
    /// Signer's name and the signature's details, in the order signed.
    pub signatures: Vec<(String, SignatureFields)>,
    /// For a collected edition, the issues it reprints in the form
    /// `parse_collects` reads, such as `Amazing Spider-Man (1963) 1-10`.
    /// Empty for none.
    pub collects: String,
}

impl BookEntry {
    pub fn new(category: Category, series: SeriesChoice, number: Option<IssueNumber>) -> Self {
        Self {
            category,
            series,
            issue: IssueFields::new(number),
            credits: Vec::new(),
            characters: Vec::new(),
            teams: Vec::new(),
            story_arcs: Vec::new(),
            edition: EditionFields::default(),
            cover_artists: Vec::new(),
            copy: CopyFields::default(),
            storage_box: None,
            tags: Vec::new(),
            signatures: Vec::new(),
            collects: String::new(),
        }
    }
}

/// A new book to start from an existing copy, to save retyping what it
/// shares with that copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefill {
    /// The next issue of the series, or the next volume of a book line.
    NextIssue,
    /// Another physical copy of the same edition.
    AnotherCopy,
    /// Another edition of the same issue, such as a cover variant.
    Variant,
}

/// Where a saved book ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedBook {
    pub series: SeriesId,
    pub issue: IssueId,
    pub edition: EditionId,
    pub copy: CopyId,
}

impl Database {
    /// Adds a new copy. If its issue is already in the catalog, the copy
    /// joins it, and so does its edition when the variant, printing, and
    /// distribution match one already there. Details typed for an
    /// existing issue or edition fill in or replace what it has, but a
    /// blank field never erases anything, and names are added to its lists
    /// rather than replacing them.
    pub fn add_book(&self, entry: &BookEntry) -> Result<SavedBook> {
        let tx = self.tx()?;
        let series = self.resolve_series(entry)?;
        let issue = match self.find_issue(series, entry.issue.number.as_ref())? {
            Some(issue) => {
                let current = self.issue(issue)?.fields;
                self.update_issue(issue, &merge_issue(&current, &entry.issue))?;
                self.add_issue_links(issue, entry)?;
                issue
            }
            None => {
                let issue = self.create_issue(series, &entry.issue)?;
                self.add_issue_links(issue, entry)?;
                issue
            }
        };
        let edition = match self.find_edition(issue, &entry.edition)? {
            Some(edition) => {
                let current = self.edition(edition)?.fields;
                self.update_edition(edition, &merge_edition(&current, &entry.edition))?;
                let mut artists: Vec<_> = self
                    .cover_artists(edition)?
                    .into_iter()
                    .map(|a| a.id)
                    .collect();
                artists.extend(self.names::<CreatorId>(&entry.cover_artists)?);
                self.set_cover_artists(edition, &artists)?;
                edition
            }
            None => {
                let edition = self.create_edition(issue, &entry.edition)?;
                let artists = self.names::<CreatorId>(&entry.cover_artists)?;
                self.set_cover_artists(edition, &artists)?;
                edition
            }
        };
        let copy = self.create_copy(edition, &self.copy_fields(entry)?)?;
        self.set_tags(copy, &self.names::<TagId>(&entry.tags)?)?;
        self.set_signatures(copy, &self.signatures_of(entry)?)?;
        if !entry.collects.trim().is_empty() {
            self.set_collects(issue, &entry.collects)?;
        }
        tx.commit()?;
        Ok(SavedBook {
            series,
            issue,
            edition,
            copy,
        })
    }

    /// Saves the edit form of `copy`. Its issue is shared with any other
    /// copies of it, so changes to that show on every copy, and picking
    /// another series moves the whole issue there. Its edition is not, so
    /// changing that changes this copy alone.
    pub fn update_book(&self, copy: CopyId, entry: &BookEntry) -> Result<SavedBook> {
        let tx = self.tx()?;
        let current = self.copy(copy)?;
        live(current.trash)?;
        let issue = self.edition(current.edition)?.issue;
        let mut series = self.issue(issue)?.series;
        let target = self.resolve_series(entry)?;
        if target != series {
            self.move_issue(issue, target, &entry.issue)?;
            series = target;
        }
        self.update_issue(issue, &entry.issue)?;
        self.set_credits(issue, &self.credits_of(entry)?)?;
        self.set_characters(issue, &self.appearances::<CharacterId>(&entry.characters)?)?;
        self.set_teams(issue, &self.appearances::<TeamId>(&entry.teams)?)?;
        self.set_story_arcs(issue, &self.arcs_of(entry)?)?;
        if entry.category == Category::CollectedEdition {
            self.set_collects(issue, &entry.collects)?;
        }
        let edition = self.edition_for(copy, current.edition, issue, entry)?;
        self.update_copy(copy, &self.copy_fields(entry)?)?;
        self.set_tags(copy, &self.names::<TagId>(&entry.tags)?)?;
        self.set_signatures(copy, &self.signatures_of(entry)?)?;
        tx.commit()?;
        Ok(SavedBook {
            series,
            issue,
            edition,
            copy,
        })
    }

    /// The edit form's contents for a copy outside Trash.
    pub fn book_entry(&self, copy: CopyId) -> Result<BookEntry> {
        let tx = self.tx()?;
        let c = self.copy(copy)?;
        live(c.trash)?;
        let edition = self.edition(c.edition)?;
        let issue = self.issue(edition.issue)?;
        let series = self.series(issue.series)?;
        let entry = BookEntry {
            category: series.fields.category,
            series: SeriesChoice::Existing(series.id),
            credits: self
                .credits(issue.id)?
                .into_iter()
                .map(|c| (c.name, c.role))
                .collect(),
            characters: self
                .characters(issue.id)?
                .into_iter()
                .map(|a| (a.name, a.first_appearance))
                .collect(),
            teams: self
                .teams(issue.id)?
                .into_iter()
                .map(|a| (a.name, a.first_appearance))
                .collect(),
            story_arcs: self
                .story_arcs(issue.id)?
                .into_iter()
                .map(|a| (a.name, a.part))
                .collect(),
            cover_artists: self
                .cover_artists(edition.id)?
                .into_iter()
                .map(|a| a.name)
                .collect(),
            storage_box: c
                .fields
                .storage_box
                .map(|b| self.lookup::<BoxId>(b).map(|l| l.name))
                .transpose()?,
            tags: self.tags(copy)?.into_iter().map(|t| t.name).collect(),
            signatures: self
                .signatures(copy)?
                .into_iter()
                .map(|s| (s.name, s.fields))
                .collect(),
            collects: if series.fields.category == Category::CollectedEdition {
                self.collects_text(issue.id)?
            } else {
                String::new()
            },
            issue: issue.fields,
            edition: edition.fields,
            copy: c.fields,
        };
        tx.commit()?;
        Ok(entry)
    }

    /// An add form's contents, filled in from `copy` for `kind`.
    ///
    /// The next issue keeps the series, its creators and teams, the kind
    /// of edition, and the box, and moves the number, cover date, and box
    /// position on by one; what belongs to one issue (title, characters,
    /// arcs, variant) and to one copy (grade, price, notes) starts blank.
    /// When the number has no digits to advance, it is left blank.
    pub fn prefilled_entry(&self, copy: CopyId, kind: Prefill) -> Result<BookEntry> {
        let mut e = self.book_entry(copy)?;
        let next_position = e.copy.box_position.and_then(|p| p.checked_add(1));
        let blank_copy = CopyFields::default();
        // Signatures belong to one physical copy.
        e.signatures.clear();
        match kind {
            Prefill::NextIssue => {
                let mut issue = IssueFields::new(e.issue.number.as_ref().and_then(|n| n.next()));
                issue.legacy_number = e.issue.legacy_number.and_then(|n| n.checked_add(1));
                issue.cover_date = e.issue.cover_date.and_then(|d| d.next_month());
                e.issue = issue;
                e.characters.clear();
                e.story_arcs.clear();
                e.collects.clear();
                e.edition = EditionFields {
                    distribution: e.edition.distribution,
                    cover_price: e.edition.cover_price,
                    canadian_cover_price_cents: e.edition.canadian_cover_price_cents,
                    binding: e.edition.binding,
                    has_dust_jacket: e.edition.has_dust_jacket,
                    has_slipcase: e.edition.has_slipcase,
                    ..EditionFields::default()
                };
                e.cover_artists.clear();
                e.copy = CopyFields {
                    purchase_date: e.copy.purchase_date,
                    seller: e.copy.seller.clone(),
                    pedigree: e.copy.pedigree.clone(),
                    box_position: next_position,
                    ..blank_copy
                };
                e.tags.clear();
            }
            Prefill::AnotherCopy => {
                e.copy = CopyFields {
                    box_position: next_position,
                    ..blank_copy
                };
                e.tags.clear();
            }
            Prefill::Variant => {
                e.edition = EditionFields::default();
                e.cover_artists.clear();
                e.copy = CopyFields {
                    box_position: next_position,
                    ..blank_copy
                };
                e.tags.clear();
            }
        }
        Ok(e)
    }

    /// The live issue numbered `number` in a series, if there is one: the
    /// form uses it to say a book will join an issue already listed.
    pub fn find_issue(
        &self,
        series: SeriesId,
        number: Option<&IssueNumber>,
    ) -> Result<Option<IssueId>> {
        let Some(number) = number else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM issue
                 WHERE series_id = ?1 AND number_sort = ?2 AND trash_id IS NULL",
                rusqlite::params![series, number.sort_key()],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn resolve_series(&self, entry: &BookEntry) -> Result<SeriesId> {
        let id = match &entry.series {
            SeriesChoice::Existing(id) => *id,
            SeriesChoice::New {
                name,
                start_year,
                volume,
                publisher,
                imprint,
                magazine_format,
                black_and_white,
            } => {
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                let found: Option<SeriesId> = self
                    .conn
                    .query_row(
                        "SELECT id FROM series
                         WHERE name = ?1 COLLATE NOCASE AND start_year IS ?2
                             AND volume IS ?3 AND trash_id IS NULL
                         ORDER BY id LIMIT 1",
                        rusqlite::params![name, start_year, volume],
                        |row| row.get(0),
                    )
                    .optional()?;
                match found {
                    Some(id) => id,
                    None => {
                        let mut fields = SeriesFields::new(name, entry.category);
                        fields.start_year = *start_year;
                        fields.volume = *volume;
                        fields.magazine_format = *magazine_format;
                        fields.black_and_white = *black_and_white;
                        fields.imprint.clone_from(imprint);
                        fields.publisher = publisher
                            .as_deref()
                            .filter(|p| !p.trim().is_empty())
                            .map(|p| self.name::<PublisherId>(p))
                            .transpose()?;
                        return self.create_series(&fields);
                    }
                }
            }
        };
        let series = self.series(id)?;
        live(series.trash)?;
        if series.fields.category != entry.category {
            let kind = match series.fields.category {
                Category::Comic => "comic",
                Category::Magazine => "magazine",
                Category::CollectedEdition => "collected edition",
            };
            return Err(Error::Invalid(format!(
                "\u{201c}{}\u{201d} is a {kind} series",
                series.fields.name
            )));
        }
        Ok(id)
    }

    /// Moves an issue to another series under its new number, in one
    /// statement so its old number never collides with one there.
    fn move_issue(&self, issue: IssueId, series: SeriesId, fields: &IssueFields) -> Result<()> {
        check_number_free(&self.conn, series, fields, Some(issue))?;
        let number = fields.number.as_ref();
        self.conn.execute(
            "UPDATE issue SET series_id = ?2, number = ?3, number_sort = ?4 WHERE id = ?1",
            rusqlite::params![
                issue,
                series,
                number.map(IssueNumber::as_str),
                number.map_or("", |n| n.sort_key())
            ],
        )?;
        Ok(())
    }

    /// The edition `copy` is in once the edit form is saved. Changing it
    /// never changes another copy, since copies of one issue can be
    /// different editions: a copy whose edition details change leaves an
    /// edition it shares as it was, and joins the edition of the issue
    /// that already has those details (cover artists too), or one of its
    /// own. An edition left with no copy at all goes, as the collection
    /// holds only books owned.
    fn edition_for(
        &self,
        copy: CopyId,
        edition: EditionId,
        issue: IssueId,
        entry: &BookEntry,
    ) -> Result<EditionId> {
        let fields = clean_edition(&entry.edition, entry.category)?;
        let mut artists = self.names::<CreatorId>(&entry.cover_artists)?;
        artists.sort_unstable();
        artists.dedup();
        // Variants match as find_edition matches them, except that a
        // change of case alone to the copy's own still counts.
        let same = |id: EditionId, have: &EditionFields, own: bool| -> Result<bool> {
            let variant = if own {
                have.variant == fields.variant
            } else {
                have.variant.eq_ignore_ascii_case(&fields.variant)
            };
            let have = EditionFields {
                variant: fields.variant.clone(),
                ..have.clone()
            };
            let mut theirs: Vec<_> = self.cover_artists(id)?.into_iter().map(|a| a.id).collect();
            theirs.sort_unstable();
            Ok(variant && have == fields && theirs == artists)
        };
        if same(edition, &self.edition(edition)?.fields, true)? {
            return Ok(edition);
        }
        let mut target = None;
        for other in self.editions_of_issue(issue)? {
            if other.id != edition && same(other.id, &other.fields, false)? {
                target = Some(other.id);
                break;
            }
        }
        // Copies in Trash count, so one restored comes back as it was.
        let shared: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM copy WHERE edition_id = ?1 AND id <> ?2)",
            rusqlite::params![edition, copy],
            |row| row.get(0),
        )?;
        let target = match target {
            Some(other) => other,
            None if !shared => {
                self.update_edition(edition, &fields)?;
                self.set_cover_artists(edition, &artists)?;
                return Ok(edition);
            }
            None => {
                let new = self.create_edition(issue, &fields)?;
                self.set_cover_artists(new, &artists)?;
                new
            }
        };
        self.conn.execute(
            "UPDATE copy SET edition_id = ?2 WHERE id = ?1",
            rusqlite::params![copy, target],
        )?;
        if !shared {
            self.conn
                .execute("DELETE FROM edition WHERE id = ?1", [edition])?;
        }
        Ok(target)
    }

    /// The live edition of `issue` with the same variant, printing, and
    /// distribution, if there is one.
    fn find_edition(&self, issue: IssueId, f: &EditionFields) -> Result<Option<EditionId>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM edition
                 WHERE issue_id = ?1 AND variant = ?2 COLLATE NOCASE AND printing IS ?3
                     AND distribution IS ?4 AND trash_id IS NULL
                 ORDER BY id LIMIT 1",
                rusqlite::params![issue, f.variant.trim(), f.printing, f.distribution],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Adds the entry's names to an issue's lists, keeping what is there.
    fn add_issue_links(&self, issue: IssueId, entry: &BookEntry) -> Result<()> {
        let mut credits: Vec<_> = self
            .credits(issue)?
            .into_iter()
            .map(|c| (c.creator, c.role))
            .collect();
        credits.extend(self.credits_of(entry)?);
        self.set_credits(issue, &credits)?;

        let characters = merge_flags(
            self.characters(issue)?
                .into_iter()
                .map(|a| (a.id, a.first_appearance)),
            self.appearances::<CharacterId>(&entry.characters)?,
        );
        self.set_characters(issue, &characters)?;
        let teams = merge_flags(
            self.teams(issue)?
                .into_iter()
                .map(|a| (a.id, a.first_appearance)),
            self.appearances::<TeamId>(&entry.teams)?,
        );
        self.set_teams(issue, &teams)?;

        let mut arcs: Vec<(StoryArcId, Option<u32>)> = self
            .story_arcs(issue)?
            .into_iter()
            .map(|a| (a.arc, a.part))
            .collect();
        for (arc, part) in self.arcs_of(entry)? {
            match arcs.iter_mut().find(|(a, _)| *a == arc) {
                Some((_, existing)) => *existing = part.or(*existing),
                None => arcs.push((arc, part)),
            }
        }
        self.set_story_arcs(issue, &arcs)
    }

    fn copy_fields(&self, entry: &BookEntry) -> Result<CopyFields> {
        let mut fields = entry.copy.clone();
        fields.storage_box = entry
            .storage_box
            .as_deref()
            .filter(|b| !b.trim().is_empty())
            .map(|b| self.name::<BoxId>(b))
            .transpose()?;
        Ok(fields)
    }

    fn credits_of(&self, entry: &BookEntry) -> Result<Vec<(CreatorId, Role)>> {
        entry
            .credits
            .iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, role)| Ok((self.name::<CreatorId>(name)?, *role)))
            .collect()
    }

    fn signatures_of(&self, entry: &BookEntry) -> Result<Vec<(CreatorId, SignatureFields)>> {
        entry
            .signatures
            .iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, fields)| Ok((self.name::<CreatorId>(name)?, fields.clone())))
            .collect()
    }

    fn arcs_of(&self, entry: &BookEntry) -> Result<Vec<(StoryArcId, Option<u32>)>> {
        entry
            .story_arcs
            .iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, part)| Ok((self.name::<StoryArcId>(name)?, *part)))
            .collect()
    }

    fn appearances<I: LookupId>(&self, names: &[(String, bool)]) -> Result<Vec<(I, bool)>> {
        names
            .iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, first)| Ok((self.name::<I>(name)?, *first)))
            .collect()
    }

    pub(crate) fn names<I: LookupId>(&self, names: &[String]) -> Result<Vec<I>> {
        names
            .iter()
            .filter(|name| !name.trim().is_empty())
            .map(|name| self.name::<I>(name))
            .collect()
    }

    /// Saves a series' details as the Edit Series dialog has them, with
    /// its publisher and the characters and teams in every issue given
    /// by name: each found by name or alternate name, or added if there
    /// is none; a blank publisher clears it. One unit, so a refused
    /// change leaves no new names behind.
    pub fn edit_series(
        &self,
        id: SeriesId,
        fields: &SeriesFields,
        publisher: &str,
        characters: &[String],
        teams: &[String],
    ) -> Result<()> {
        let tx = self.tx()?;
        let mut f = fields.clone();
        f.publisher = match publisher.trim() {
            "" => None,
            name => Some(self.name::<PublisherId>(name)?),
        };
        self.update_series(id, &f)?;
        self.set_series_characters(id, &self.names::<CharacterId>(characters)?)?;
        self.set_series_teams(id, &self.names::<TeamId>(teams)?)?;
        tx.commit()
    }

    /// Adds a series as the New Series dialog has it, with its publisher
    /// and the characters and teams in every issue given by name, as in
    /// [`Database::edit_series`].
    pub fn add_series(
        &self,
        fields: &SeriesFields,
        publisher: &str,
        characters: &[String],
        teams: &[String],
    ) -> Result<SeriesId> {
        let tx = self.tx()?;
        let mut f = fields.clone();
        f.publisher = match publisher.trim() {
            "" => None,
            name => Some(self.name::<PublisherId>(name)?),
        };
        let id = self.create_series(&f)?;
        self.set_series_characters(id, &self.names::<CharacterId>(characters)?)?;
        self.set_series_teams(id, &self.names::<TeamId>(teams)?)?;
        tx.commit()?;
        Ok(id)
    }

    /// The entry with this name or alternate name, added if there is none.
    pub(crate) fn name<I: LookupId>(&self, name: &str) -> Result<I> {
        match self.find_lookup::<I>(name)? {
            Some(id) => Ok(id),
            None => self.create_lookup::<I>(name),
        }
    }
}

/// Keeps every name from both lists once; a first appearance marked in
/// either stays marked.
fn merge_flags<I: Copy + Eq + std::hash::Hash>(
    current: impl Iterator<Item = (I, bool)>,
    added: Vec<(I, bool)>,
) -> Vec<(I, bool)> {
    let mut order = Vec::new();
    let mut flags = HashMap::new();
    for (id, first) in current.chain(added) {
        let flag = flags.entry(id).or_insert_with(|| {
            order.push(id);
            false
        });
        *flag |= first;
    }
    order.into_iter().map(|id| (id, flags[&id])).collect()
}

fn text_or(new: &str, old: &str) -> String {
    if new.trim().is_empty() { old } else { new }.to_owned()
}

fn merge_issue(old: &IssueFields, new: &IssueFields) -> IssueFields {
    IssueFields {
        number: old.number.clone(),
        legacy_number: new.legacy_number.or(old.legacy_number),
        title: text_or(&new.title, &old.title),
        cover_date: new.cover_date.or(old.cover_date),
        release_date: new.release_date.or(old.release_date),
        page_count: new.page_count.or(old.page_count),
        synopsis: text_or(&new.synopsis, &old.synopsis),
        is_key: old.is_key || new.is_key,
        key_reason: text_or(&new.key_reason, &old.key_reason),
    }
}

fn merge_edition(old: &EditionFields, new: &EditionFields) -> EditionFields {
    EditionFields {
        variant: old.variant.clone(),
        printing: old.printing,
        distribution: old.distribution,
        cover_price: new.cover_price.or(old.cover_price),
        canadian_cover_price_cents: new
            .canadian_cover_price_cents
            .or(old.canadian_cover_price_cents),
        // Spaces alone would be trimmed away on saving, erasing the old.
        barcode: new
            .barcode
            .clone()
            .filter(|b| !b.trim().is_empty())
            .or_else(|| old.barcode.clone()),
        isbn: new.isbn.clone().or_else(|| old.isbn.clone()),
        binding: new.binding.or(old.binding),
        has_dust_jacket: new.has_dust_jacket.or(old.has_dust_jacket),
        has_slipcase: new.has_slipcase.or(old.has_slipcase),
        published: new.published.or(old.published),
    }
}
