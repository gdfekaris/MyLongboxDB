//! CSV import: reading a file from a spreadsheet or a MyLongboxDB
//! export, checking every row in a dry run, and adding them in one
//! transaction.
//!
//! Each row is read into a [`BookEntry`] and added the way the add form
//! adds a book, so copies of one issue join it and blank cells never
//! erase anything. Rows from MyLongboxDB's own export keep their ids,
//! value history, and the time they were added.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::date::Date;
use crate::db::Database;
use crate::entry::{BookEntry, SeriesChoice};
use crate::error::{Error, Result};
use crate::export::{COLUMNS, ROLE_COLUMN_NAMES, ROLE_COLUMNS, split, split_items, split_parts};
use crate::issue_number::IssueNumber;
use crate::model::{
    Binding, BookCondition, Category, CharacterId, CopyId, CoverDate, Distribution, Grade,
    GradeType, Isbn, MagazineFormat, PageQuality, RowUuid, SeriesId, Side, SignatureFields,
    SignaturePlacement, TeamId, Verification,
};
use crate::money::{Amount, Currency};
use crate::photos::CoverPhoto;

/// The largest CSV file read.
const MAX_CSV_BYTES: u64 = 100 * 1024 * 1024;
/// The largest photo read from an archive; decoding sets its own limits.
const MAX_PHOTO_BYTES: u64 = 50 * 1024 * 1024;

/// What an archive is read from: a file, or bytes in memory.
pub trait ArchiveReader: Read + Seek + Send {}

impl<T: Read + Seek + Send> ArchiveReader for T {}

type Archive = zip::ZipArchive<Box<dyn ArchiveReader>>;

/// A file to import: a CSV, or a full export with its photos.
pub struct ImportSource {
    rows: Vec<Vec<String>>,
    /// Each row's line in the file.
    lines: Vec<usize>,
    headers: Vec<String>,
    archive: Option<Archive>,
}

impl std::fmt::Debug for ImportSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportSource")
            .field("headers", &self.headers)
            .field("rows", &self.rows.len())
            .field("archive", &self.archive.is_some())
            .finish()
    }
}

/// Text from a file in whatever encoding spreadsheets save: UTF-8 with or
/// without a byte-order mark, UTF-16 with one, or else Windows-1252.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], read: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| read(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|b| windows_1252(*b)).collect(),
    }
}

/// A byte of Windows-1252, the encoding older spreadsheets on Windows
/// save in; it is Latin-1 but for a few printing characters.
fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        b => char::from(b),
    }
}

/// The delimiter used most on the first line outside quotes: a comma,
/// semicolon, or tab.
pub(crate) fn delimiter(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or("");
    let mut counts = [(b',', 0), (b';', 0), (b'\t', 0)];
    let mut quoted = false;
    for c in first.bytes() {
        if c == b'"' {
            quoted = !quoted;
        } else if !quoted && let Some(entry) = counts.iter_mut().find(|(d, _)| *d == c) {
            entry.1 += 1;
        }
    }
    counts
        .iter()
        .rev()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
        .map_or(b',', |(d, _)| *d)
}

impl ImportSource {
    /// A CSV file's contents.
    pub fn from_csv(bytes: &[u8]) -> Result<Self> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CSV_BYTES {
            return Err(Error::Invalid("the file is too large to import".into()));
        }
        let text = decode_text(bytes);
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter(&text))
            .flexible(true)
            .has_headers(false)
            .from_reader(text.as_bytes());
        let mut records = reader.records();
        let headers = match records.next() {
            Some(Ok(record)) => record.iter().map(|h| h.trim().to_owned()).collect(),
            Some(Err(e)) => return Err(unreadable(e)),
            None => return Err(Error::Invalid("the file is empty".into())),
        };
        let (mut rows, mut lines) = (Vec::new(), Vec::new());
        let bytes = text.as_bytes();
        let (mut line, mut counted) = (1, 0);
        for record in records {
            let record = record.map_err(unreadable)?;
            // Rows a spreadsheet leaves blank are not books.
            if record.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            // Counted from the byte offset, since the reader's own line
            // count slips on Windows line endings, where the offset can
            // also fall on the end of the line before.
            let mut start = record.position().map_or(0, |p| p.byte() as usize);
            while bytes.get(start).is_some_and(|b| matches!(b, b'\r' | b'\n')) {
                start += 1;
            }
            line += bytes[counted..start]
                .iter()
                .filter(|b| **b == b'\n')
                .count();
            counted = start;
            lines.push(line);
            rows.push(record.iter().map(str::to_owned).collect());
        }
        Ok(Self {
            rows,
            lines,
            headers,
            archive: None,
        })
    }
}

fn unreadable(e: csv::Error) -> Error {
    Error::Invalid(format!("the file is not readable CSV: {e}"))
}

impl ImportSource {
    /// The file at `path`: a full export if it is a zip archive, and
    /// otherwise a CSV. A collection file is refused by name, as it is
    /// the file most often picked here by mistake.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut magic = Vec::new();
        (&mut file).take(16).read_to_end(&mut magic)?;
        file.rewind()?;
        if magic.starts_with(crate::store::ZIP_HEADER) {
            return Self::from_zip(std::io::BufReader::new(file));
        }
        if magic == crate::store::SQLITE_HEADER {
            return Err(Error::CollectionFile);
        }
        let mut bytes = Vec::new();
        // A file too large is refused without reading it all.
        file.take(MAX_CSV_BYTES + 1).read_to_end(&mut bytes)?;
        Self::from_csv(&bytes)
    }

    /// A full export: its `collection.csv`, with its photos read from
    /// the archive as they are needed.
    pub fn from_zip(reader: impl ArchiveReader + 'static) -> Result<Self> {
        let not_export = || Error::Invalid("the archive is not a MyLongboxDB full export".into());
        let reader: Box<dyn ArchiveReader> = Box::new(reader);
        let mut archive = zip::ZipArchive::new(reader).map_err(|_| not_export())?;
        let csv =
            read_entry(&mut archive, "collection.csv", MAX_CSV_BYTES).ok_or_else(not_export)?;
        let mut source = Self::from_csv(&csv)?;
        source.archive = Some(archive);
        Ok(source)
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    /// How many rows hold books.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The first `n` rows, for a preview.
    pub fn preview(&self, n: usize) -> &[Vec<String>] {
        &self.rows[..n.min(self.rows.len())]
    }

    /// Whether every column is one MyLongboxDB exports, so nothing needs
    /// matching by hand.
    pub fn is_own_format(&self) -> bool {
        self.headers.iter().all(|h| COLUMNS.contains(&h.as_str()))
            && self.headers.iter().any(|h| h == "Copy ID")
    }

    /// A photo's file from the archive, by the path its row names.
    pub fn read_photo(&mut self, path: &str) -> std::result::Result<Vec<u8>, String> {
        self.archive
            .as_mut()
            .and_then(|archive| read_entry(archive, path, MAX_PHOTO_BYTES))
            .ok_or_else(|| "the file is missing from the archive or too large".to_owned())
    }
}

/// An archive entry's bytes, if it exists and is no larger than `limit`.
/// Only entries named in the CSV are read, and nothing is unpacked to
/// disk, so names inside the archive cannot reach outside it.
fn read_entry(archive: &mut Archive, name: &str, limit: u64) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    // The stated size may lie; reading stops at the limit regardless.
    entry.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? <= limit).then_some(bytes)
}

/// Common headers of other apps' spreadsheets, and the column each
/// means.
const ALIASES: &[(&str, &str)] = &[
    ("title", "Series"),
    ("series name", "Series"),
    ("comic", "Series"),
    ("issue", "Issue Number"),
    ("issue #", "Issue Number"),
    ("issue no", "Issue Number"),
    ("number", "Issue Number"),
    ("#", "Issue Number"),
    ("story", "Story Title"),
    ("year", "Start Year"),
    ("volume number", "Volume"),
    ("cover year", "Cover Date"),
    ("writer", "Writers"),
    ("artist", "Pencillers"),
    ("penciller", "Pencillers"),
    ("penciler", "Pencillers"),
    ("pencilers", "Pencillers"),
    ("inker", "Inkers"),
    ("cover artist", "Cover Artists"),
    ("character", "Characters"),
    ("team", "Teams"),
    ("story arc", "Story Arcs"),
    ("arc", "Story Arcs"),
    ("tag", "Tags"),
    ("condition", "Grade"),
    ("cgc", "Certificate Number"),
    ("cert", "Certificate Number"),
    ("cert #", "Certificate Number"),
    ("pages", "Page Quality"),
    ("page color", "Page Quality"),
    ("page colour", "Page Quality"),
    ("price paid", "Purchase Price"),
    ("paid", "Purchase Price"),
    ("cost", "Purchase Price"),
    ("purchased", "Purchase Date"),
    ("date purchased", "Purchase Date"),
    ("value", "Current Value"),
    ("price", "Cover Price"),
    ("box", "Storage Box"),
    ("location", "Storage Box"),
    ("comments", "Notes"),
    ("note", "Notes"),
    ("upc", "Barcode"),
    ("isbn13", "ISBN"),
    ("key", "Key Issue"),
    ("canadian price", "Canadian Cover Price"),
    ("cad price", "Canadian Cover Price"),
    ("cover price cad", "Canadian Cover Price"),
    ("legacy", "Legacy Number"),
    ("legacy issue number", "Legacy Number"),
    ("lgy", "Legacy Number"),
];

/// The column each header most likely means, by its name or a common
/// other name, ignoring case; `None` for headers left out. No column is
/// chosen twice.
pub fn auto_mapping(headers: &[String]) -> Vec<Option<&'static str>> {
    let mut taken = HashSet::new();
    headers
        .iter()
        .map(|header| {
            let header = header.trim().to_lowercase();
            let found = COLUMNS
                .iter()
                .copied()
                .find(|c| c.to_lowercase() == header)
                .or_else(|| {
                    ALIASES
                        .iter()
                        .find(|(alias, _)| *alias == header)
                        .map(|(_, column)| *column)
                })?;
            taken.insert(found).then_some(found)
        })
        .collect()
}

/// How to read the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSettings {
    /// For each column of the file, the field it fills, or `None` to
    /// leave it out.
    pub mapping: Vec<Option<&'static str>>,
    /// What separates names in a list cell. MyLongboxDB's own files use
    /// `;`.
    pub separator: char,
    /// The day of the import, for a current value with no date.
    pub today: Date,
    /// Whether to leave out books that look like ones already owned.
    pub skip_likely_duplicates: bool,
}

/// What would happen, or happened, to one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Added,
    /// The copy itself is already in the collection, by its id.
    AlreadyOwned,
    /// A copy of the same issue, variant, printing, and distribution was
    /// already owned; the text says how many. Added unless the settings say to
    /// leave these out.
    LikelyDuplicate {
        note: String,
        added: bool,
    },
    /// The row cannot be added; the text says why.
    Problem(String),
}

/// One row's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowReport {
    /// The row's line in the file, counting the header as line 1.
    pub line: usize,
    /// e.g. `Amazing Spider-Man (1963 series) #121`.
    pub label: String,
    pub outcome: Outcome,
}

/// Every row's result, and the photos that could not be added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub rows: Vec<RowReport>,
    pub photo_problems: Vec<String>,
}

impl ImportReport {
    pub fn count(&self, matches: impl Fn(&Outcome) -> bool) -> usize {
        self.rows.iter().filter(|r| matches(&r.outcome)).count()
    }

    pub fn added(&self) -> usize {
        self.count(|o| {
            matches!(
                o,
                Outcome::Added | Outcome::LikelyDuplicate { added: true, .. }
            )
        })
    }
}

/// A row read into what adding it takes.
#[derive(Debug)]
struct Planned {
    entry: BookEntry,
    copy_id: Option<RowUuid>,
    edition_id: Option<RowUuid>,
    issue_id: Option<RowUuid>,
    series_id: Option<RowUuid>,
    known_issue_count: Option<u32>,
    series_notes: String,
    series_characters: Vec<String>,
    series_teams: Vec<String>,
    values: Vec<(Date, Amount, String)>,
    added_at: Option<String>,
    photos: Vec<(Side, String)>,
}

/// A cell as it was before export guarded it against being read as a
/// formula. Only a `'` before what export guards is taken off, so text
/// from elsewhere that starts with one, such as `'Til Death`, is kept.
fn unguarded(cell: &str) -> &str {
    match cell.strip_prefix('\'') {
        Some(rest) if rest.starts_with(crate::export::FORMULA_STARTS) => rest.trim(),
        _ => cell,
    }
}

/// A row's cells by the field each fills.
struct Cells<'a> {
    by_field: HashMap<&'static str, &'a str>,
    separator: char,
}

impl Cells<'_> {
    fn get(&self, field: &str) -> &str {
        self.by_field.get(field).map_or("", |c| unguarded(c.trim()))
    }

    fn list(&self, field: &str) -> Vec<String> {
        split(self.get(field), self.separator)
    }

    /// A cell read by `parse`, with the field named in the problem if it
    /// does not read; empty cells are `None`.
    fn read<T>(
        &self,
        field: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> std::result::Result<Option<T>, String> {
        let text = self.get(field);
        if text.is_empty() {
            return Ok(None);
        }
        parse(text)
            .map(Some)
            .ok_or_else(|| format!("{field}: \u{201c}{text}\u{201d} cannot be read"))
    }
}

/// A code, as stored or as a reader writes it: `like_new`, `Like New`,
/// `like-new`.
fn code<T>(text: &str, from_code: fn(&str) -> Option<T>) -> Option<T> {
    let normal: String = text
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();
    from_code(&normal)
}

fn category(text: &str) -> Option<Category> {
    match text.trim().to_lowercase().as_str() {
        "collected edition" | "collected editions" | "trade" | "tpb" | "book" => {
            Some(Category::CollectedEdition)
        }
        _ => code(text, Category::from_code),
    }
}

/// A page quality as its code, as CGC writes it ("Off-White to White
/// Pages", "Brown / Brittle"), or abbreviated as collectors do ("OW/W").
fn page_quality(text: &str) -> Option<PageQuality> {
    let words = text.trim().to_lowercase();
    let words = words.strip_suffix("pages").unwrap_or(&words);
    let mut normal = String::new();
    for word in words
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !normal.is_empty() {
            normal.push('_');
        }
        normal.push_str(word);
    }
    let abbreviated = match normal.as_str() {
        "w" => "white",
        "ow_w" => "off_white_to_white",
        "ow" => "off_white",
        "c_ow" => "cream_to_off_white",
        "c" => "cream",
        "lt_ow" => "light_tan_to_off_white",
        "lt_c" => "light_tan_to_cream",
        "lt" => "light_tan",
        "t_ow" => "tan_to_off_white",
        "t_c" => "tan_to_cream",
        "t" => "tan",
        "dt_ow" => "dark_tan_to_off_white",
        "dt" => "dark_tan",
        "b_ow" => "brown_to_off_white",
        "b_t" => "brown_to_tan",
        "b" => "brown",
        other => other,
    };
    PageQuality::from_code(abbreviated)
}

fn yes_no(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "yes" | "y" | "true" | "1" | "x" => Some(true),
        "no" | "n" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// A Canadian price in cents, from `0.75` or with a mark before it as
/// stores and spreadsheets write one: `CA$0.75`, `C$0.75`, `CAD 0.75`,
/// or `$0.75`.
fn canadian_cents(text: &str) -> Option<i64> {
    let text = text.trim();
    let amount = ["CA$", "C$", "CAD", "$"]
        .iter()
        .find_map(|mark| {
            text.get(..mark.len())
                .filter(|start| start.eq_ignore_ascii_case(mark))
                .map(|_| &text[mark.len()..])
        })
        .unwrap_or(text);
    crate::money::parse_cents(amount.trim()).ok()
}

fn number<T: std::str::FromStr>(text: &str) -> Option<T> {
    text.trim().parse().ok()
}

/// A cover date as `YYYY`, `YYYY-MM`, or a full date whose day is let
/// go.
fn cover_date(text: &str) -> Option<CoverDate> {
    let mut parts = text.trim().split('-');
    let year = parts.next()?.parse().ok()?;
    let month = match parts.next() {
        Some(m) => Some(m.parse().ok()?),
        None => None,
    };
    CoverDate::new(year, month)
}

/// A grade as a number, its letters (`VF/NM`), or both (`VF/NM 9.0`).
pub(crate) fn grade(text: &str) -> Option<Grade> {
    let by_name = |word: &str| {
        (Grade::MIN.tenths()..=Grade::MAX.tenths())
            .filter_map(Grade::from_tenths)
            .find(|g| g.name().is_some_and(|n| n.eq_ignore_ascii_case(word)))
    };
    Grade::parse(text)
        .or_else(|| text.split_whitespace().find_map(Grade::parse))
        .or_else(|| by_name(text.trim()))
}

/// An amount with its currency taken from `currency`, US dollars when
/// that is empty. A `$` is allowed, and commas: `1,250.00` and `1,250`
/// group thousands, while `12,50` is how much of Europe writes 12.50.
pub(crate) fn money(text: &str, currency: &str) -> Option<Amount> {
    let currency = if currency.trim().is_empty() {
        Currency::Usd
    } else {
        Currency::from_code(&currency.trim().to_uppercase())?
    };
    let text = text.trim().trim_start_matches('$').trim();
    let decimal_comma = !text.contains('.')
        && text.matches(',').count() == 1
        && text
            .rsplit(',')
            .next()
            .is_some_and(|after| after.len() != 3);
    let cleaned = if decimal_comma {
        text.replace(',', ".")
    } else {
        text.replace(',', "")
    };
    Amount::parse(currency, &cleaned).ok()
}

/// A story arc as `Name (part 3)`, or just its name.
fn arc(item: &str) -> (String, Option<u32>) {
    if let Some(rest) = item.strip_suffix(')')
        && let Some((name, part)) = rest.rsplit_once(" (part ")
        && let Ok(part) = part.trim().parse()
    {
        return (name.trim().to_owned(), Some(part));
    }
    (item.to_owned(), None)
}

fn uuid(text: &str) -> Option<RowUuid> {
    Uuid::parse_str(text.trim()).ok().map(RowUuid)
}

/// A UTC time as MyLongboxDB writes it, `YYYY-MM-DDTHH:MM:SSZ`.
fn timestamp(text: &str) -> Option<String> {
    let t = text.trim();
    let digits = |r: std::ops::Range<usize>| {
        t.get(r)
            .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()))
    };
    let shape = t.len() == 20
        && digits(0..4)
        && &t[4..5] == "-"
        && digits(5..7)
        && &t[7..8] == "-"
        && digits(8..10)
        && &t[10..11] == "T"
        && digits(11..13)
        && &t[13..14] == ":"
        && digits(14..16)
        && &t[16..17] == ":"
        && digits(17..19)
        && &t[19..20] == "Z";
    (shape && t[..10].parse::<Date>().is_ok()).then(|| t.to_owned())
}

fn plan(cells: &Cells<'_>, today: Date) -> std::result::Result<Planned, String> {
    let name = cells
        .get("Series")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        return Err("Series: every book needs a series".into());
    }
    let category = cells.read("Category", category)?.unwrap_or(Category::Comic);
    let start_year = cells.read("Start Year", number)?;
    let volume = cells.read("Volume", number)?;
    let number_text = cells.get("Issue Number").trim_start_matches('#');
    let issue_number = if number_text.is_empty() {
        None
    } else {
        Some(IssueNumber::new(number_text).map_err(|e| format!("Issue Number: {e}"))?)
    };
    let mut entry = BookEntry::new(
        category,
        SeriesChoice::New {
            name: name.clone(),
            start_year,
            volume,
            publisher: Some(cells.get("Publisher").to_owned()).filter(|p| !p.is_empty()),
            imprint: cells.get("Imprint").to_owned(),
            magazine_format: cells
                .read("Magazine Format", |t| code(t, MagazineFormat::from_code))?,
            black_and_white: cells.read("Black and White", yes_no)?,
        },
        issue_number,
    );
    entry.issue.title = cells.get("Story Title").to_owned();
    entry.issue.cover_date = cells.read("Cover Date", cover_date)?;
    entry.issue.release_date = cells.read("Release Date", |t| t.parse().ok())?;
    entry.issue.page_count = cells.read("Page Count", number)?;
    entry.issue.legacy_number = cells.read("Legacy Number", |t| {
        number(t.trim_start_matches('#').trim())
    })?;
    entry.issue.is_key = cells.read("Key Issue", yes_no)?.unwrap_or(false);
    entry.issue.key_reason = cells.get("Key Reason").to_owned();
    entry.issue.synopsis = cells.get("Synopsis").to_owned();
    for (role, column) in ROLE_COLUMNS.iter().zip(ROLE_COLUMN_NAMES) {
        for name in cells.list(column) {
            entry.credits.push((name, *role));
        }
    }
    let cast = |all: &str, first: &str| {
        let firsts = cells.list(first);
        let mut names: Vec<(String, bool)> = cells
            .list(all)
            .into_iter()
            .map(|n| {
                let is_first = firsts.iter().any(|f| f.eq_ignore_ascii_case(&n));
                (n, is_first)
            })
            .collect();
        for f in firsts {
            if !names.iter().any(|(n, _)| n.eq_ignore_ascii_case(&f)) {
                names.push((f, true));
            }
        }
        names
    };
    entry.characters = cast("Characters", "First Appearances of Characters");
    entry.teams = cast("Teams", "First Appearances of Teams");
    entry.story_arcs = cells.list("Story Arcs").iter().map(|a| arc(a)).collect();
    entry.collects = cells.get("Collects").to_owned();
    entry.edition.variant = cells.get("Variant").to_owned();
    entry.edition.printing = cells.read("Printing", number)?;
    entry.edition.distribution =
        cells.read("Distribution", |t| code(t, Distribution::from_code))?;
    entry.edition.published = cells.read("Published", |t| t.parse().ok())?;
    entry.edition.cover_price = cells.read("Cover Price", |t| {
        money(t, cells.get("Cover Price Currency"))
    })?;
    entry.edition.canadian_cover_price_cents =
        cells.read("Canadian Cover Price", canadian_cents)?;
    entry.edition.barcode = Some(cells.get("Barcode").to_owned()).filter(|b| !b.is_empty());
    entry.edition.isbn = cells.read("ISBN", Isbn::parse)?;
    entry.edition.binding = cells.read("Binding", |t| code(t, Binding::from_code))?;
    entry.edition.has_dust_jacket = cells.read("Dust Jacket", yes_no)?;
    entry.edition.has_slipcase = cells.read("Slipcase", yes_no)?;
    entry.cover_artists = cells.list("Variant Cover Artists");
    entry.copy.grade = cells.read("Grade", grade)?;
    entry.copy.grade_type = cells.read("Grade Type", |t| code(t, GradeType::from_code))?;
    entry.copy.cert_number =
        Some(cells.get("Certificate Number").to_owned()).filter(|c| !c.is_empty());
    entry.copy.label_type = Some(cells.get("Label").to_owned()).filter(|l| !l.is_empty());
    entry.copy.page_quality = cells.read("Page Quality", page_quality)?;
    entry.copy.book_condition =
        cells.read("Book Condition", |t| code(t, BookCondition::from_code))?;
    entry.copy.dust_jacket_condition = cells.read("Dust Jacket Condition", |t| {
        code(t, BookCondition::from_code)
    })?;
    entry.copy.purchase_price = cells.read("Purchase Price", |t| {
        money(t, cells.get("Purchase Currency"))
    })?;
    entry.copy.purchase_date = cells.read("Purchase Date", |t| t.parse().ok())?;
    entry.copy.seller = cells.get("Seller").to_owned();
    entry.copy.pedigree = cells.get("Pedigree").to_owned();
    entry.copy.box_position = cells.read("Box Position", number)?;
    entry.copy.notes = cells.get("Notes").to_owned();
    entry.storage_box = Some(cells.get("Storage Box").to_owned()).filter(|b| !b.is_empty());
    entry.tags = cells.list("Tags");
    entry.signatures = split_items(cells.get("Signatures"), cells.separator)
        .iter()
        .map(|item| signature(item))
        .collect::<std::result::Result<_, _>>()?;

    let mut values = split_items(cells.get("Value History"), cells.separator)
        .iter()
        .map(|item| value(item))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values.is_empty()
        && let Some(worth) = cells.read("Current Value", |t| {
            money(t, cells.get("Current Value Currency"))
        })?
    {
        values.push((today, worth, String::new()));
    }
    let photos = [
        (Side::Front, "Front Photo"),
        (Side::Back, "Back Photo"),
        (Side::Signature, "Signature Photo"),
    ]
    .into_iter()
    .filter(|(_, column)| !cells.get(column).is_empty())
    .map(|(side, column)| (side, cells.get(column).to_owned()))
    .collect();
    Ok(Planned {
        copy_id: cells.read("Copy ID", uuid)?,
        edition_id: cells.read("Edition ID", uuid)?,
        issue_id: cells.read("Issue ID", uuid)?,
        series_id: cells.read("Series ID", uuid)?,
        known_issue_count: cells.read("Known Issue Count", number)?,
        series_notes: cells.get("Series Notes").to_owned(),
        series_characters: cells.list("Series Characters"),
        series_teams: cells.list("Series Teams"),
        values,
        added_at: cells.read("Date Added", timestamp)?,
        photos,
        entry,
    })
}

/// A signature as `name | verification | certificate | date | place |
/// sketch | placement`, of which only the name is needed; exports made
/// before placement was kept have no last part.
fn signature(item: &str) -> std::result::Result<(String, SignatureFields), String> {
    let parts = split_parts(item);
    let part = |n: usize| parts.get(n).map_or("", String::as_str);
    let problem = || format!("Signatures: \u{201c}{item}\u{201d} cannot be read");
    let verification = if part(1).is_empty() {
        Verification::Unverified
    } else {
        code(part(1), Verification::from_code).ok_or_else(problem)?
    };
    let mut fields = SignatureFields::new(verification);
    fields.cert_number = Some(part(2).to_owned()).filter(|c| !c.is_empty());
    fields.signed_on = if part(3).is_empty() {
        None
    } else {
        Some(part(3).parse().map_err(|_| problem())?)
    };
    fields.place = part(4).to_owned();
    // Written as "sketch" or left empty; other files may say yes or no.
    fields.sketch = match part(5) {
        "" => false,
        t if t.eq_ignore_ascii_case("sketch") => true,
        t => yes_no(t).ok_or_else(problem)?,
    };
    fields.placement = if part(6).is_empty() {
        None
    } else {
        Some(code(part(6), SignaturePlacement::from_code).ok_or_else(problem)?)
    };
    if part(0).is_empty() {
        return Err(problem());
    }
    Ok((part(0).to_owned(), fields))
}

/// A value as `date | amount | currency | source`.
fn value(item: &str) -> std::result::Result<(Date, Amount, String), String> {
    let parts = split_parts(item);
    let part = |n: usize| parts.get(n).map_or("", String::as_str);
    let problem = || format!("Value History: \u{201c}{item}\u{201d} cannot be read");
    let date = part(0).parse().map_err(|_| problem())?;
    let amount = money(part(1), part(2)).ok_or_else(problem)?;
    Ok((date, amount, part(3).to_owned()))
}

/// A row's book as a reader would name it: `Thor (1966 series) #337`,
/// with the volume in place of a missing year, as series are labelled.
fn label(cells: &Cells<'_>) -> String {
    let mut label = cells
        .get("Series")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        label.push_str("(no series)");
    }
    let (year, volume) = (cells.get("Start Year"), cells.get("Volume"));
    if !year.is_empty() {
        label.push_str(&format!(" ({year} series)"));
    } else if !volume.is_empty() {
        label.push_str(&format!(" (Vol. {volume})"));
    }
    let number = cells.get("Issue Number").trim_start_matches('#');
    if !number.is_empty() {
        label.push_str(&format!(" #{number}"));
    }
    label
}

/// The tables whose rows keep their ids across an export and import.
#[derive(Clone, Copy)]
enum Kept {
    Series,
    Issue,
    Edition,
    Copy,
}

impl Kept {
    fn find(self) -> &'static str {
        match self {
            Self::Series => "SELECT id, trash_id IS NOT NULL FROM series WHERE uuid = ?1",
            Self::Issue => "SELECT id, trash_id IS NOT NULL FROM issue WHERE uuid = ?1",
            Self::Edition => "SELECT id, trash_id IS NOT NULL FROM edition WHERE uuid = ?1",
            Self::Copy => "SELECT id, trash_id IS NOT NULL FROM copy WHERE uuid = ?1",
        }
    }

    fn set(self) -> &'static str {
        match self {
            Self::Series => "UPDATE series SET uuid = ?2 WHERE id = ?1",
            Self::Issue => "UPDATE issue SET uuid = ?2 WHERE id = ?1",
            Self::Edition => "UPDATE edition SET uuid = ?2 WHERE id = ?1",
            Self::Copy => "UPDATE copy SET uuid = ?2 WHERE id = ?1",
        }
    }

    fn largest(self) -> &'static str {
        match self {
            Self::Series => "SELECT coalesce(max(id), 0) FROM series",
            Self::Issue => "SELECT coalesce(max(id), 0) FROM issue",
            Self::Edition => "SELECT coalesce(max(id), 0) FROM edition",
            Self::Copy => "SELECT coalesce(max(id), 0) FROM copy",
        }
    }
}

/// The largest id in each table when an import starts: rows above it
/// were made by the import, so they may take the file's ids, and copies
/// at or below it were owned before.
#[derive(Debug)]
struct Before {
    series: i64,
    issue: i64,
    edition: i64,
    copy: i64,
}

/// A row's photo, to add once its copy exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPhoto {
    pub copy: CopyId,
    pub side: Side,
    /// Its file in the archive.
    pub path: String,
}

/// An import under way. Rows are added a few at a time, so a window can
/// show progress between them, inside one transaction that
/// [`ImportRun::finish`] commits or [`ImportRun::abandon`] undoes. Until
/// then, everything the collection is asked to do joins that
/// transaction, so a run dropped without either is undone, rather than
/// left to gather every later change and lose it when the app closes.
pub struct ImportRun<'db> {
    db: &'db Database,
    /// Set once the transaction is committed or undone.
    ended: bool,
    before: Before,
    next: usize,
    with_photos: bool,
    report: ImportReport,
    photos: Vec<PendingPhoto>,
}

impl ImportRun<'_> {
    /// How many rows have been read.
    pub fn rows_done(&self) -> usize {
        self.next
    }

    pub fn report(&self) -> &ImportReport {
        &self.report
    }

    /// The photos of the books added so far, for the caller to prepare
    /// and pass to [`Database::import_photo`].
    pub fn photos(&self) -> &[PendingPhoto] {
        &self.photos
    }

    /// Keeps everything the run added, returning what it found.
    pub fn finish(mut self) -> Result<ImportReport> {
        self.db.conn.execute_batch("RELEASE import")?;
        self.ended = true;
        Ok(std::mem::take(&mut self.report))
    }

    /// Undoes everything the run added, returning what it found: a dry
    /// run is a run abandoned once every row is read.
    pub fn abandon(mut self) -> Result<ImportReport> {
        self.db
            .conn
            .execute_batch("ROLLBACK TO import; RELEASE import")?;
        self.ended = true;
        Ok(std::mem::take(&mut self.report))
    }
}

impl Drop for ImportRun<'_> {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self
                .db
                .conn
                .execute_batch("ROLLBACK TO import; RELEASE import");
        }
    }
}

impl Database {
    /// Begins an import of `source`.
    pub fn start_import(&self, source: &ImportSource) -> Result<ImportRun<'_>> {
        let largest = |kept: Kept| -> Result<i64> {
            Ok(self.conn.query_row(kept.largest(), [], |r| r.get(0))?)
        };
        let before = Before {
            series: largest(Kept::Series)?,
            issue: largest(Kept::Issue)?,
            edition: largest(Kept::Edition)?,
            copy: largest(Kept::Copy)?,
        };
        self.conn.execute_batch("SAVEPOINT import")?;
        Ok(ImportRun {
            db: self,
            ended: false,
            before,
            next: 0,
            with_photos: source.archive.is_some(),
            report: ImportReport::default(),
            photos: Vec::new(),
        })
    }

    /// Adds up to `limit` more rows, returning whether every row has now
    /// been read. Rows that cannot be added are reported, not fatal; an
    /// error means the collection itself failed, and the run should be
    /// abandoned.
    pub fn import_rows(
        &self,
        run: &mut ImportRun<'_>,
        source: &ImportSource,
        settings: &ImportSettings,
        limit: usize,
    ) -> Result<bool> {
        let end = run.next.saturating_add(limit).min(source.rows.len());
        while run.next < end {
            let index = run.next;
            let cells = Cells {
                by_field: settings
                    .mapping
                    .iter()
                    .zip(&source.rows[index])
                    .filter_map(|(field, cell)| Some(((*field)?, cell.as_str())))
                    .collect(),
                separator: settings.separator,
            };
            let outcome = match plan(&cells, settings.today) {
                Err(problem) => Outcome::Problem(problem),
                Ok(planned) => {
                    let unit = self.tx()?;
                    match self.import_row(planned, settings, &run.before, run.with_photos) {
                        Ok((outcome, pending)) => {
                            unit.commit()?;
                            run.photos.extend(pending);
                            outcome
                        }
                        Err(Error::Sqlite(e)) if !is_constraint(&e) => {
                            return Err(Error::Sqlite(e));
                        }
                        Err(e) => Outcome::Problem(e.to_string()),
                    }
                }
            };
            run.report.rows.push(RowReport {
                line: source.lines[index],
                label: label(&cells),
                outcome,
            });
            run.next += 1;
        }
        Ok(run.next == source.rows.len())
    }

    /// Stores a photo of the run's, once the caller has made it from the
    /// file, or notes why it could not be.
    pub fn import_photo(
        &self,
        run: &mut ImportRun<'_>,
        photo: &PendingPhoto,
        prepared: std::result::Result<CoverPhoto, String>,
    ) -> Result<()> {
        let why = match prepared {
            Ok(stored) => match self.set_photo(photo.copy, photo.side, &stored) {
                Ok(()) => return Ok(()),
                Err(Error::Sqlite(e)) if !is_constraint(&e) => return Err(Error::Sqlite(e)),
                Err(e) => e.to_string(),
            },
            Err(why) => why,
        };
        run.report
            .photo_problems
            .push(format!("{}: {why}", photo.path));
        Ok(())
    }

    /// Runs the whole import and undoes it, reporting what each row
    /// would do.
    pub fn check_import(
        &self,
        source: &ImportSource,
        settings: &ImportSettings,
    ) -> Result<ImportReport> {
        let mut run = self.start_import(source)?;
        match self.import_rows(&mut run, source, settings, usize::MAX) {
            Ok(_) => run.abandon(),
            Err(e) => {
                run.abandon()?;
                Err(e)
            }
        }
    }

    /// Runs the whole import at once and keeps it. `prepare` turns a
    /// photo's file into what is stored, or says why it cannot.
    pub fn import(
        &self,
        source: &mut ImportSource,
        settings: &ImportSettings,
        prepare: &dyn Fn(Vec<u8>) -> std::result::Result<CoverPhoto, String>,
    ) -> Result<ImportReport> {
        let mut run = self.start_import(source)?;
        let added = self
            .import_rows(&mut run, source, settings, usize::MAX)
            .and_then(|_| {
                for photo in run.photos.clone() {
                    let prepared = source.read_photo(&photo.path).and_then(prepare);
                    self.import_photo(&mut run, &photo, prepared)?;
                }
                Ok(())
            });
        match added {
            Ok(()) => run.finish(),
            Err(e) => {
                run.abandon()?;
                Err(e)
            }
        }
    }

    /// The live row with the file's id, if any; an error if it is in
    /// Trash.
    fn kept_row(&self, kept: Kept, uuid: Option<RowUuid>) -> Result<Option<i64>> {
        let Some(uuid) = uuid else {
            return Ok(None);
        };
        let found: Option<(i64, bool)> = self
            .conn
            .query_row(kept.find(), [uuid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        match found {
            Some((_, true)) => Err(Error::InTrash),
            Some((id, false)) => Ok(Some(id)),
            None => Ok(None),
        }
    }

    /// Gives a row made by this import the file's id for it, unless that
    /// id is already taken.
    fn keep_uuid(&self, kept: Kept, id: i64, made_after: i64, uuid: Option<RowUuid>) -> Result<()> {
        if let Some(uuid) = uuid
            && id > made_after
        {
            let taken: Option<i64> = self
                .conn
                .query_row(kept.find(), [uuid], |r| r.get(0))
                .optional()?;
            if taken.is_none() {
                self.conn.execute(kept.set(), params![id, uuid])?;
            }
        }
        Ok(())
    }

    /// How many copies owned before the import share the planned book's
    /// issue, variant, printing, and distribution: the edition it would
    /// be added to.
    fn owned_before(&self, entry: &BookEntry, before: &Before) -> Result<i64> {
        let series: Option<SeriesId> = match &entry.series {
            SeriesChoice::Existing(id) => Some(*id),
            SeriesChoice::New {
                name,
                start_year,
                volume,
                ..
            } => self
                .conn
                .query_row(
                    "SELECT id FROM series
                     WHERE name = ?1 COLLATE NOCASE AND start_year IS ?2
                         AND volume IS ?3 AND trash_id IS NULL
                     ORDER BY id LIMIT 1",
                    params![name, start_year, volume],
                    |r| r.get(0),
                )
                .optional()?,
        };
        let Some(series) = series else {
            return Ok(0);
        };
        let Some(issue) = self.find_issue(series, entry.issue.number.as_ref())? else {
            return Ok(0);
        };
        Ok(self.conn.query_row(
            "SELECT count(*) FROM copy JOIN edition ON edition.id = copy.edition_id
             WHERE edition.issue_id = ?1 AND edition.variant = ?2 COLLATE NOCASE
                 AND edition.printing IS ?3 AND edition.distribution IS ?4
                 AND edition.trash_id IS NULL AND copy.trash_id IS NULL AND copy.id <= ?5",
            params![
                issue,
                entry.edition.variant.trim(),
                entry.edition.printing,
                entry.edition.distribution,
                before.copy
            ],
            |r| r.get(0),
        )?)
    }

    fn import_row(
        &self,
        mut planned: Planned,
        settings: &ImportSettings,
        before: &Before,
        with_photos: bool,
    ) -> Result<(Outcome, Vec<PendingPhoto>)> {
        match self.kept_row(Kept::Copy, planned.copy_id) {
            Ok(Some(_)) => return Ok((Outcome::AlreadyOwned, Vec::new())),
            Err(Error::InTrash) => {
                return Ok((
                    Outcome::Problem("this copy is in Trash; restore it from there".into()),
                    Vec::new(),
                ));
            }
            other => {
                other?;
            }
        }
        let mut entry = planned.entry;
        // The series the book was exported from, when it is still here,
        // even if it has since been renamed.
        if let Ok(Some(id)) = self.kept_row(Kept::Series, planned.series_id) {
            entry.series = SeriesChoice::Existing(SeriesId(id));
        }
        let owned = self.owned_before(&entry, before)?;
        let duplicate = (owned > 0).then(|| {
            if owned == 1 {
                "you already own a copy of this issue, variant, printing, and distribution".to_owned()
            } else {
                format!("you already own {owned} copies of this issue, variant, printing, and distribution")
            }
        });
        if let Some(note) = &duplicate
            && settings.skip_likely_duplicates
        {
            let outcome = Outcome::LikelyDuplicate {
                note: note.clone(),
                added: false,
            };
            return Ok((outcome, Vec::new()));
        }

        let saved = self.add_book(&entry)?;
        self.keep_uuid(
            Kept::Series,
            saved.series.0,
            before.series,
            planned.series_id,
        )?;
        self.keep_uuid(Kept::Issue, saved.issue.0, before.issue, planned.issue_id)?;
        self.keep_uuid(
            Kept::Edition,
            saved.edition.0,
            before.edition,
            planned.edition_id,
        )?;
        self.keep_uuid(Kept::Copy, saved.copy.0, before.copy, planned.copy_id)?;
        if planned.known_issue_count.is_some() || !planned.series_notes.is_empty() {
            let mut fields = self.series(saved.series)?.fields;
            // What a series already says is kept, as in the add form.
            if fields.known_issue_count.is_none() {
                fields.known_issue_count = planned.known_issue_count;
            }
            if fields.notes.is_empty() {
                fields.notes = std::mem::take(&mut planned.series_notes);
            }
            self.update_series(saved.series, &fields)?;
        }
        // Added to those the series already has, never taking any away.
        if !planned.series_characters.is_empty() {
            let mut ids: Vec<CharacterId> = self
                .series_characters(saved.series)?
                .into_iter()
                .map(|n| n.id)
                .collect();
            ids.extend(self.names::<CharacterId>(&planned.series_characters)?);
            self.set_series_characters(saved.series, &ids)?;
        }
        if !planned.series_teams.is_empty() {
            let mut ids: Vec<TeamId> = self
                .series_teams(saved.series)?
                .into_iter()
                .map(|n| n.id)
                .collect();
            ids.extend(self.names::<TeamId>(&planned.series_teams)?);
            self.set_series_teams(saved.series, &ids)?;
        }
        if let Some(added_at) = &planned.added_at {
            self.conn.execute(
                "UPDATE copy SET added_at = ?2 WHERE id = ?1",
                params![saved.copy, added_at],
            )?;
        }
        for (date, amount, source) in &planned.values {
            self.add_value(saved.copy, *date, *amount, source)?;
        }
        let photos = if with_photos {
            planned
                .photos
                .into_iter()
                .map(|(side, path)| PendingPhoto {
                    copy: saved.copy,
                    side,
                    path,
                })
                .collect()
        } else {
            Vec::new()
        };
        let outcome = match duplicate {
            Some(note) => Outcome::LikelyDuplicate { note, added: true },
            None => Outcome::Added,
        };
        Ok((outcome, photos))
    }
}

/// Whether a database error is about the row's data, such as a value a
/// check refuses, rather than the file or disk failing.
fn is_constraint(e: &rusqlite::Error) -> bool {
    e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation)
}
