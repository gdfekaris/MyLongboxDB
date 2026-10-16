//! CSV export: one row per copy with every field, in a format the
//! importer reads back without loss.
//!
//! Lists in a cell are joined with `; `. Signatures and values have parts,
//! joined with ` | `. A backslash keeps either separator, or itself, as
//! part of a name: `\;`, `\|`, `\\`.

use std::io::{Seek, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{Category, CopyId, CoverDate, PublisherId, Role, Side};
use crate::money::Amount;

/// The columns, in order. The importer recognizes them by these names.
pub const COLUMNS: &[&str] = &[
    "Copy ID",
    "Edition ID",
    "Issue ID",
    "Series ID",
    "Category",
    "Series",
    "Start Year",
    "Volume",
    "Publisher",
    "Imprint",
    "Known Issue Count",
    "Magazine Format",
    "Black and White",
    "Series Notes",
    "Issue Number",
    "Legacy Number",
    "Story Title",
    "Cover Date",
    "Release Date",
    "Page Count",
    "Key Issue",
    "Key Reason",
    "Synopsis",
    "Writers",
    "Pencillers",
    "Inkers",
    "Colorists",
    "Letterers",
    "Cover Artists",
    "Editors",
    "Article Writers",
    "Interviews",
    "Characters",
    "First Appearances of Characters",
    "Teams",
    "First Appearances of Teams",
    "Story Arcs",
    "Collects",
    "Variant",
    "Printing",
    "Distribution",
    "Cover Price",
    "Cover Price Currency",
    "Barcode",
    "ISBN",
    "Binding",
    "Dust Jacket",
    "Slipcase",
    "Variant Cover Artists",
    "Grade",
    "Grade Type",
    "Certificate Number",
    "Label",
    "Page Quality",
    "Book Condition",
    "Dust Jacket Condition",
    "Purchase Price",
    "Purchase Currency",
    "Purchase Date",
    "Seller",
    "Storage Box",
    "Box Position",
    "Tags",
    "Signatures",
    "Notes",
    "Current Value",
    "Current Value Currency",
    "Value History",
    "Date Added",
    "Front Photo",
    "Back Photo",
    "Signature Photo",
];

/// The columns of the credit roles, in the order of [`ROLE_COLUMNS`].
pub(crate) const ROLE_COLUMN_NAMES: [&str; 9] = [
    "Writers",
    "Pencillers",
    "Inkers",
    "Colorists",
    "Letterers",
    "Cover Artists",
    "Editors",
    "Article Writers",
    "Interviews",
];

/// The credit roles in the order of their columns.
pub(crate) const ROLE_COLUMNS: [Role; 9] = [
    Role::Writer,
    Role::Penciller,
    Role::Inker,
    Role::Colorist,
    Role::Letterer,
    Role::CoverArtist,
    Role::Editor,
    Role::Article,
    Role::Interview,
];

/// One item of a list, with the separators in it kept literal.
pub(crate) fn escape(item: &str) -> String {
    let mut out = String::with_capacity(item.len());
    for c in item.chars() {
        if matches!(c, '\\' | ';' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Items joined as a list cell.
pub(crate) fn join<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> String {
    items
        .into_iter()
        .map(|i| escape(i.as_ref()))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Items already escaped, as [`join_parts`] makes them, joined as a list
/// cell.
fn join_escaped(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().collect::<Vec<_>>().join("; ")
}

/// Parts joined as one item, e.g. a signature.
pub(crate) fn join_parts<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
    parts
        .into_iter()
        .map(|p| escape(p.as_ref()))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// A cell split at each `separator` not kept by a backslash, with the
/// backslashes left in, so items made of parts can be split again.
fn split_escaped(text: &str, separator: char) -> Vec<String> {
    let mut items = vec![String::new()];
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        let item = items.last_mut().expect("never empty");
        if c == '\\' {
            item.push(c);
            if let Some(next) = chars.next() {
                item.push(next);
            }
        } else if c == separator {
            items.push(String::new());
        } else {
            item.push(c);
        }
    }
    items
        .into_iter()
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty())
        .collect()
}

/// An item with its backslashes taken out.
fn unescape(item: &str) -> String {
    let mut out = String::with_capacity(item.len());
    let mut chars = item.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    out
}

/// A list cell's items, undoing [`join`]; empty items are dropped.
pub(crate) fn split(text: &str, separator: char) -> Vec<String> {
    split_escaped(text, separator)
        .iter()
        .map(|i| unescape(i))
        .collect()
}

/// A list cell's items with their parts still joined, undoing
/// `join_escaped`; split each with [`split_parts`].
pub(crate) fn split_items(text: &str, separator: char) -> Vec<String> {
    split_escaped(text, separator)
}

/// An item's parts, undoing [`join_parts`]. Empty parts are kept, since
/// parts are known by their place.
pub(crate) fn split_parts(item: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = item.chars();
    while let Some(c) = chars.next() {
        let part = parts.last_mut().expect("never empty");
        if c == '\\' {
            part.push(c);
            part.extend(chars.next());
        } else if c == '|' {
            parts.push(String::new());
        } else {
            part.push(c);
        }
    }
    parts.iter().map(|p| unescape(p.trim())).collect()
}

fn yes_no(flag: Option<bool>) -> String {
    match flag {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => String::new(),
    }
}

/// An amount as its number and its currency's code.
fn amount(a: Option<Amount>) -> (String, String) {
    a.map_or((String::new(), String::new()), |a| {
        (a.to_decimal_string(), a.currency().code().to_owned())
    })
}

fn text<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// A cover date as `YYYY-MM`, or `YYYY` when only the year is known.
pub(crate) fn cover_date(date: CoverDate) -> String {
    match date.month() {
        Some(month) => format!("{:04}-{month:02}", date.year()),
        None => format!("{:04}", date.year()),
    }
}

fn csv_error(e: csv::Error) -> Error {
    Error::Io(std::io::Error::other(e))
}

impl Database {
    /// Writes the copies (or every live copy, in natural order) as CSV
    /// to `out`, returning how many rows were written. With
    /// `photo_paths`, each row names its photos' files as a full export
    /// stores them: `images/<copy id>_front.jpg`.
    pub fn export_csv(
        &self,
        copies: Option<&[CopyId]>,
        out: impl Write,
        photo_paths: bool,
    ) -> Result<usize> {
        let tx = self.tx()?;
        let ids: Vec<CopyId> = match copies {
            Some(ids) => ids.to_vec(),
            None => self
                .browse(crate::Shelf::Everything)?
                .into_iter()
                .map(|r| r.copy)
                .collect(),
        };
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(out);
        writer.write_record(COLUMNS).map_err(csv_error)?;
        for id in &ids {
            writer
                .write_record(self.csv_row(*id, photo_paths)?)
                .map_err(csv_error)?;
        }
        writer.flush()?;
        tx.commit()?;
        Ok(ids.len())
    }

    fn csv_row(&self, id: CopyId, photo_paths: bool) -> Result<Vec<String>> {
        let entry = self.book_entry(id)?;
        let copy = self.copy(id)?;
        let edition = self.edition(copy.edition)?;
        let issue = self.issue(edition.issue)?;
        let series = self.series(issue.series)?;
        let (s, i, e, c) = (&series.fields, &issue.fields, &edition.fields, &copy.fields);
        let publisher = s
            .publisher
            .map(|p| self.lookup::<PublisherId>(p).map(|l| l.name))
            .transpose()?;
        let credits = |role: Role| {
            join(
                entry
                    .credits
                    .iter()
                    .filter(|(_, r)| *r == role)
                    .map(|(name, _)| name),
            )
        };
        let names = |list: &[(String, bool)], first_only: bool| {
            join(
                list.iter()
                    .filter(|(_, first)| !first_only || *first)
                    .map(|(name, _)| name),
            )
        };
        let arcs = join(entry.story_arcs.iter().map(|(name, part)| match part {
            Some(n) => format!("{name} (part {n})"),
            None => name.clone(),
        }));
        let signatures = join_escaped(entry.signatures.iter().map(|(name, f)| {
            join_parts([
                name.as_str(),
                f.verification.code(),
                f.cert_number.as_deref().unwrap_or(""),
                &text(f.signed_on),
                &f.place,
                if f.sketch { "sketch" } else { "" },
                f.placement.map_or("", |p| p.code()),
            ])
        }));
        let values = self.values(id)?;
        let history = join_escaped(values.iter().map(|v| {
            join_parts([
                v.date.to_string(),
                v.value.to_decimal_string(),
                v.value.currency().code().to_owned(),
                v.source.clone(),
            ])
        }));
        let (cover_price, cover_currency) = amount(e.cover_price);
        let (paid, paid_currency) = amount(c.purchase_price);
        let (worth, worth_currency) = amount(values.last().map(|v| v.value));
        let photo = |side: Side| -> Result<String> {
            let has = photo_paths && self.photo_sides(id)?.contains(&side);
            Ok(if has {
                format!("images/{}_{}.jpg", copy.uuid, side.code())
            } else {
                String::new()
            })
        };
        let mut row = vec![
            copy.uuid.to_string(),
            edition.uuid.to_string(),
            issue.uuid.to_string(),
            series.uuid.to_string(),
            s.category.code().to_owned(),
            s.name.clone(),
            text(s.start_year),
            text(s.volume),
            publisher.unwrap_or_default(),
            s.imprint.clone(),
            text(s.known_issue_count),
            text(s.magazine_format.map(|f| f.code())),
            yes_no(s.black_and_white),
            s.notes.clone(),
            text(i.number.as_ref().map(|n| n.as_str().to_owned())),
            text(i.legacy_number),
            i.title.clone(),
            i.cover_date.map(cover_date).unwrap_or_default(),
            text(i.release_date),
            text(i.page_count),
            yes_no(Some(i.is_key)),
            i.key_reason.clone(),
            i.synopsis.clone(),
        ];
        row.extend(ROLE_COLUMNS.map(credits));
        row.extend([
            names(&entry.characters, false),
            names(&entry.characters, true),
            names(&entry.teams, false),
            names(&entry.teams, true),
            arcs,
            if s.category == Category::CollectedEdition {
                entry.collects.clone()
            } else {
                String::new()
            },
            e.variant.clone(),
            text(e.printing),
            text(e.distribution.map(|d| d.code())),
            cover_price,
            cover_currency,
            e.barcode.clone().unwrap_or_default(),
            text(e.isbn.as_ref().map(|i| i.as_str().to_owned())),
            text(e.binding.map(|b| b.code())),
            yes_no(e.has_dust_jacket),
            yes_no(e.has_slipcase),
            join(&entry.cover_artists),
            text(c.grade),
            text(c.grade_type.map(|g| g.code())),
            c.cert_number.clone().unwrap_or_default(),
            c.label_type.clone().unwrap_or_default(),
            text(c.page_quality.map(|p| p.code())),
            text(c.book_condition.map(|b| b.code())),
            text(c.dust_jacket_condition.map(|b| b.code())),
            paid,
            paid_currency,
            text(c.purchase_date),
            c.seller.clone(),
            entry.storage_box.clone().unwrap_or_default(),
            text(c.box_position),
            join(&entry.tags),
            signatures,
            c.notes.clone(),
            worth,
            worth_currency,
            history,
            copy.added_at.clone(),
            photo(Side::Front)?,
            photo(Side::Back)?,
            photo(Side::Signature)?,
        ]);
        debug_assert_eq!(row.len(), COLUMNS.len());
        Ok(row)
    }
}

/// What a full export's `README.txt` says, so the archive explains
/// itself to anyone opening it without MyLongboxDB.
pub const README: &str = "\
MyLongboxDB full export
=======================

This archive holds a comic collection exported from MyLongboxDB, in
plain formats any spreadsheet or image viewer can open.

collection.csv
  One row per physical copy, UTF-8, comma-separated (RFC 4180), with a
  header row naming each column. Every field the app keeps for a copy,
  its edition, its issue, and its series is there.

  IDs: the Copy, Edition, Issue, and Series ID columns are permanent
  identifiers. Rows with the same Issue ID are copies of one issue.

  Lists: a cell holding several names separates them with \"; \", for
  example \"Wolverine; Hulk\". Signatures and value history entries have
  parts separated by \" | \":
    Signatures:     name | verification | certificate | date | place |
                    sketch | placement
    Value History:  date | amount | currency | source
  A backslash before ; or | or \\ means that character is part of a
  name rather than a separator.

  Story arcs give their part as \"Name (part 3)\". Characters and teams
  that make their first appearance are also listed in the First
  Appearances columns.

  Amounts are a number and a currency code in separate columns:
    USD  US dollars
    XAU  troy ounces of gold
    BTC  bitcoin
  Each amount is exactly as it was entered.

  Dates are YYYY-MM-DD. A cover date is YYYY-MM, or YYYY when only the
  year is known. Date Added is a UTC time.

  Fixed choices are written as codes:
    Category: comic, magazine, collected
    Grade Type: raw, cgc, cbcs, pgx
    Page Quality, best to worst: white, off_white_to_white, off_white,
      cream_to_off_white, cream, light_tan_to_off_white,
      light_tan_to_cream, light_tan, tan_to_off_white, tan_to_cream,
      tan, dark_tan_to_off_white, dark_tan, brown_to_off_white,
      brown_to_tan, brown, brown_brittle
    Book and dust jacket condition: sealed, new, like_new, very_good,
      good, fair, poor
    Binding: hardcover, softcover, oversized_hardcover
    Distribution: direct, newsstand
    Magazine Format: standard, treasury, digest
    Signature verification: unverified, witnessed, coa, jsa, cgc_ss,
      cgc_jsa, cbcs_witnessed, cbcs, pgx
    Signature placement: front_cover, inside_front_cover, first_page,
      interior_page, inside_back_cover, back_cover
  Yes or no columns are \"yes\", \"no\", or empty when not recorded.

  Collects, for collected editions, lists the issues the book reprints,
  for example \"Amazing Spider-Man (1963) 1-10; Annual 1\".

images/
  The photos, as JPEG files named after the copy:
    <Copy ID>_front.jpg
    <Copy ID>_back.jpg
    <Copy ID>_signature.jpg   a signature inside the book
  The Front Photo, Back Photo, and Signature Photo columns name each
  copy's files.

MyLongboxDB can import this archive, or collection.csv on its own,
with nothing lost.
";

impl Database {
    /// Writes a full export to `out`: `collection.csv`, each copy's cover
    /// photos under `images/`, and a `README.txt` explaining the format.
    /// Returns how many copies it holds.
    pub fn export_zip(&self, copies: Option<&[CopyId]>, out: impl Write + Seek) -> Result<usize> {
        let tx = self.tx()?;
        let ids: Vec<CopyId> = match copies {
            Some(ids) => ids.to_vec(),
            None => self
                .browse(crate::Shelf::Everything)?
                .into_iter()
                .map(|r| r.copy)
                .collect(),
        };
        let mut zip = ZipWriter::new(out);
        let text = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        // Photos are JPEGs already; squeezing them again gains nothing.
        let photo = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);
        zip.start_file("README.txt", text).map_err(zip_error)?;
        zip.write_all(README.as_bytes())?;
        zip.start_file("collection.csv", text).map_err(zip_error)?;
        self.export_csv(Some(&ids), &mut zip, true)?;
        for id in &ids {
            let uuid = self.copy(*id)?.uuid;
            for side in self.photo_sides(*id)? {
                let Some(stored) = self.photo(*id, side)? else {
                    continue;
                };
                zip.start_file(format!("images/{uuid}_{}.jpg", side.code()), photo)
                    .map_err(zip_error)?;
                zip.write_all(&stored.full_jpeg)?;
            }
        }
        zip.finish().map_err(zip_error)?;
        tx.commit()?;
        Ok(ids.len())
    }
}

fn zip_error(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(e) => Error::Io(e),
        other => Error::Io(std::io::Error::other(other)),
    }
}
