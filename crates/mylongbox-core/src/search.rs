//! Quick search: one box that finds books by their series, issue
//! numbers, titles, publishers, creators, characters, teams, story arcs,
//! tags, notes, or storage boxes.

use std::collections::HashMap;

use strsim::osa_distance;

use crate::browse::{CopyRow, copy_row};
use crate::catalog::query_all;
use crate::db::Database;
use crate::error::Result;
use crate::model::CopyId;

/// The (owner, copy) pairs of `$kind`'s shelves for the owners listed in
/// `?1`, a JSON array of ids.
macro_rules! found_through {
    ($kind:ident) => {
        shelf_pairs!($kind, "UNION ALL", "IN (SELECT value FROM json_each(?1))")
    };
}

/// What a row of the search index is, as its `kind` column records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Series = 1,
    Title = 2,
    Publisher = 3,
    Creator = 4,
    CreatorAlias = 5,
    Character = 6,
    CharacterAlias = 7,
    Team = 8,
    TeamAlias = 9,
    StoryArc = 10,
    StoryArcAlias = 11,
    Tag = 12,
    StorageBox = 13,
    Notes = 14,
    /// Never stored in the index: issue numbers are read from the issues
    /// themselves, since a number matches only as a whole word.
    IssueNumber = 15,
}

impl Kind {
    const ALL: [Self; 15] = [
        Self::Series,
        Self::Title,
        Self::Publisher,
        Self::Creator,
        Self::CreatorAlias,
        Self::Character,
        Self::CharacterAlias,
        Self::Team,
        Self::TeamAlias,
        Self::StoryArc,
        Self::StoryArcAlias,
        Self::Tag,
        Self::StorageBox,
        Self::Notes,
        Self::IssueNumber,
    ];

    fn from_code(code: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|k| *k as i64 == code)
    }

    /// How much a match here counts: the names a book is known by above
    /// the people in it, above where it is kept and what was noted.
    fn weight(self) -> f64 {
        match self {
            Self::Series => 100.0,
            Self::IssueNumber => 90.0,
            Self::Title => 80.0,
            Self::Creator | Self::Character => 60.0,
            Self::CreatorAlias | Self::CharacterAlias => 55.0,
            Self::Team | Self::StoryArc => 50.0,
            Self::TeamAlias | Self::StoryArcAlias => 45.0,
            Self::Publisher | Self::Tag => 40.0,
            Self::StorageBox => 30.0,
            Self::Notes => 10.0,
        }
    }

    /// Each live copy found through the owners listed in `?1`, a JSON
    /// array of ids, with the owner that finds it.
    fn copies(self) -> &'static str {
        match self {
            Self::Series => found_through!(series),
            Self::Title | Self::IssueNumber => {
                "SELECT e.issue_id AS who, c.id AS copy FROM copy c
                 JOIN edition e ON e.id = c.edition_id
                 WHERE +c.trash_id IS NULL AND e.issue_id IN (SELECT value FROM json_each(?1))"
            }
            Self::Publisher => found_through!(publisher),
            Self::Creator | Self::CreatorAlias => found_through!(creator),
            Self::Character | Self::CharacterAlias => found_through!(character),
            Self::Team | Self::TeamAlias => found_through!(team),
            Self::StoryArc | Self::StoryArcAlias => found_through!(story_arc),
            Self::Tag => {
                "SELECT t.tag_id AS who, c.id AS copy FROM copy_tag t
                 JOIN copy c ON c.id = t.copy_id
                 WHERE +c.trash_id IS NULL AND t.tag_id IN (SELECT value FROM json_each(?1))"
            }
            Self::StorageBox => found_through!(storage_box),
            Self::Notes => {
                "SELECT id AS who, id AS copy FROM copy
                 WHERE +trash_id IS NULL AND id IN (SELECT value FROM json_each(?1))"
            }
        }
    }
}

/// Ids as a JSON array, the one form a list can be bound as.
pub(crate) fn json_ids(ids: impl IntoIterator<Item = i64>) -> String {
    let list: Vec<String> = ids.into_iter().map(|i| i.to_string()).collect();
    format!("[{}]", list.join(","))
}

/// A word as an FTS5 phrase, so its characters are matched as they are
/// rather than read as query syntax.
fn phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// A word with LIKE's wildcards made literal.
pub(crate) fn like_escaped(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for c in term.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The words of a query. Quoting is not offered; each word must be found
/// somewhere in the book.
fn terms(query: &str) -> Vec<&str> {
    terms_of(query).collect()
}

fn terms_of(query: &str) -> impl Iterator<Item = &str> {
    query.split_whitespace()
}

/// What a quick search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    /// The books found, best first.
    pub rows: Vec<CopyRow>,
    /// The query with mistyped words corrected, when correcting them is
    /// what found the books; for a "Did you mean" line.
    pub did_you_mean: Option<String>,
}

/// Folded words, each with how it is written and how many names use it.
type Vocabulary = HashMap<String, (String, u32)>;

/// The words of a name: split at spaces with punctuation trimmed off,
/// and hyphenated words also in their parts, so "Spider-Man" offers
/// "Spider-Man", "Spider", and "Man". Words shorter than three letters
/// are left out.
fn words_of(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .flat_map(|w| {
            let parts = w.contains('-').then(|| w.split('-'));
            std::iter::once(w).chain(parts.into_iter().flatten())
        })
        .filter(|w| w.chars().count() >= 3)
}

/// How many edits a mistyped word of `len` letters may be from the word
/// meant: none below four letters, where nearly every word is near
/// another; then one, two, and at most three.
pub(crate) fn edits_allowed(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=5 => 1,
        6..=9 => 2,
        _ => 3,
    }
}

/// The known word nearest to `term`, as it is usually written, if one is
/// near enough. Swapping two letters counts as one edit, so "Kriby"
/// finds "Kirby". Ties go to the word more names use.
fn correction(vocabulary: &Vocabulary, term: &str) -> Option<String> {
    let typed = fold(term.trim_matches(|c: char| !c.is_alphanumeric()));
    let len = typed.chars().count();
    let allowed = edits_allowed(len);
    if allowed == 0 {
        return None;
    }
    vocabulary
        .iter()
        .filter(|(word, _)| word.chars().count().abs_diff(len) <= allowed)
        .map(|(word, (written, uses))| (osa_distance(&typed, word), *uses, word, written))
        .filter(|(edits, ..)| (1..=allowed).contains(edits))
        .min_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2)))
        .map(|(.., written)| written.clone())
}

/// A word of an issue number as compared with a searched word: folded,
/// and without leading zeros if it is all digits, so "003" and "3" are
/// the same number.
fn number_word(word: &str) -> String {
    let folded = fold(word);
    if !folded.is_empty() && folded.chars().all(|c| c.is_ascii_digit()) {
        let trimmed = folded.trim_start_matches('0');
        return if trimmed.is_empty() { "0" } else { trimmed }.to_owned();
    }
    folded
}

/// Lower case with accents taken off, for judging how well text matches.
/// The index does the same on its own; this only ranks what it finds.
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
            'ś' | 'ŝ' | 'ş' | 'š' => 's',
            'ţ' | 'ť' => 't',
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

/// The quality of a match at the start of a word, the least a short
/// word must reach.
const WORD_START: f64 = 0.6;

/// How well `text` matches `term`, both folded: the whole of it, its
/// start, the start of a word in it, or just somewhere inside.
pub(crate) fn quality(text: &str, term: &str) -> f64 {
    if text == term {
        1.0
    } else if text.starts_with(term) {
        0.8
    } else if text
        .match_indices(term)
        .any(|(at, _)| text[..at].ends_with(|c: char| !c.is_alphanumeric()))
    {
        WORD_START
    } else if text.contains(term) {
        0.4
    } else {
        // The index found it, perhaps through an accent this fold does
        // not know; it still counts, least of all.
        0.2
    }
}

/// Empties the index without reading it, refills its source from the
/// tables it mirrors, and indexes that afresh.
const REBUILD: &str = "
    INSERT INTO search_text (search_text) VALUES ('delete-all');
    DELETE FROM search_source;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 1, name, 1, id FROM series;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 2, title, 2, id FROM issue;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 3, name, 3, id FROM publisher;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 4, name, 4, id FROM creator;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT rowid * 16 + 5, alias, 5, creator_id FROM creator_alias;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 6, name, 6, id FROM character;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT rowid * 16 + 7, alias, 7, character_id FROM character_alias;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 8, name, 8, id FROM team;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT rowid * 16 + 9, alias, 9, team_id FROM team_alias;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 10, name, 10, id FROM story_arc;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT rowid * 16 + 11, alias, 11, story_arc_id FROM story_arc_alias;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 12, name, 12, id FROM tag;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 13, name, 13, id FROM storage_box;
    INSERT INTO search_source (id, text, kind, owner)
        SELECT id * 16 + 14, notes, 14, id FROM copy;
    INSERT INTO search_text (search_text) VALUES ('rebuild');
";

impl Database {
    /// Builds the search index afresh from the collection's own tables,
    /// for a file from elsewhere, whose index cannot be trusted to match
    /// them or to be well formed.
    pub(crate) fn rebuild_search_index(&self) -> Result<()> {
        let tx = self.tx()?;
        self.conn.execute_batch(REBUILD)?;
        tx.commit()?;
        Ok(())
    }

    /// The live copies matching every word of `query`, best first. Each
    /// word is found anywhere in a name, case and accents aside; a book
    /// ranks by where its words matched and how well. A word that finds
    /// nothing is taken to be mistyped and replaced by the nearest word
    /// the collection knows, if one is near enough.
    pub fn quick_search(&self, query: &str) -> Result<SearchResults> {
        let terms = terms(query);
        if terms.is_empty() {
            return Ok(SearchResults::default());
        }
        let tx = self.tx()?;
        let mut vocabulary = None;
        let mut corrections = Vec::new();
        let mut scores: Option<HashMap<CopyId, f64>> = None;
        for term in terms {
            let mut hits = self.term_hits(term)?;
            if hits.is_empty() {
                if vocabulary.is_none() {
                    vocabulary = Some(self.vocabulary()?);
                }
                if let Some(word) = vocabulary.as_ref().and_then(|v| correction(v, term)) {
                    hits = self.term_hits(&word)?;
                    corrections.push((term, word));
                }
            }
            let found = self.copies_for(hits)?;
            scores = Some(match scores {
                None => found,
                // A book must match every word; its score adds up.
                Some(so_far) => so_far
                    .into_iter()
                    .filter_map(|(copy, s)| found.get(&copy).map(|f| (copy, s + f)))
                    .collect(),
            });
            if scores.as_ref().is_some_and(HashMap::is_empty) {
                break;
            }
        }
        let scores = scores.unwrap_or_default();
        let ids: Vec<i64> = scores.keys().map(|c| c.0).collect();
        let mut rows = query_all(
            &self.conn,
            copy_rows!("c.id IN (SELECT value FROM json_each(?1))"),
            [json_ids(ids.iter().copied())],
            copy_row,
        )?;
        tx.commit()?;
        // Rows come in their natural order, which a stable sort keeps
        // among books that score the same.
        rows.sort_by(|a, b| {
            let score = |r: &CopyRow| scores.get(&r.copy).copied().unwrap_or(0.0);
            score(b).total_cmp(&score(a))
        });
        let did_you_mean = (!corrections.is_empty() && !rows.is_empty()).then(|| {
            terms_of(query)
                .map(|t| {
                    corrections
                        .iter()
                        .find(|(typed, _)| *typed == t)
                        .map_or(t, |(_, word)| word.as_str())
                })
                .collect::<Vec<_>>()
                .join(" ")
        });
        Ok(SearchResults { rows, did_you_mean })
    }

    /// Every word of the names in the index, folded, with how it is
    /// written and how many names use it. Notes are left out, as their
    /// words would make odd suggestions.
    fn vocabulary(&self) -> Result<Vocabulary> {
        let texts: Vec<String> = query_all(
            &self.conn,
            "SELECT text FROM search_source WHERE kind <> 14 ORDER BY id",
            [],
            |row| row.get(0),
        )?;
        let mut words = Vocabulary::new();
        for text in &texts {
            for word in words_of(text) {
                let entry = words
                    .entry(fold(word))
                    .or_insert_with(|| (word.to_owned(), 0));
                entry.1 += 1;
            }
        }
        Ok(words)
    }

    /// The entries one word finds, each with the best score it finds it
    /// by.
    fn term_hits(&self, term: &str) -> Result<HashMap<Kind, HashMap<i64, f64>>> {
        let folded = fold(term);
        // Trigrams need three characters; shorter words are looked for
        // by scanning the indexed text instead, and must begin a word, as
        // "x" means X-Men rather than every name with an x in it. One that
        // starts with a symbol, such as "%", may appear anywhere.
        let short = term.chars().count() < 3;
        let word_start_only = short && term.chars().next().is_some_and(char::is_alphanumeric);
        let hits = if !short {
            query_all(
                &self.conn,
                "SELECT s.kind, s.owner, s.text FROM search_text f
                 JOIN search_source s ON s.id = f.rowid
                 WHERE search_text MATCH ?1",
                [phrase(term)],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )?
        } else {
            query_all(
                &self.conn,
                "SELECT kind, owner, text FROM search_source WHERE text LIKE ?1 ESCAPE '\\'",
                [format!("%{}%", like_escaped(term))],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )?
        };
        // The best score for each owner of each kind.
        let mut owners: HashMap<Kind, HashMap<i64, f64>> = HashMap::new();
        for (code, owner, text) in hits {
            let Some(kind) = Kind::from_code(code) else {
                continue;
            };
            let quality = quality(&fold(&text), &folded);
            if word_start_only && quality < WORD_START {
                continue;
            }
            let score = kind.weight() * quality;
            let best = owners.entry(kind).or_default().entry(owner).or_insert(0.0);
            *best = best.max(score);
        }
        // A word finds an issue when it is the number or a word of it,
        // so "300" finds #300 but not #30 or #3000, "#300" works as
        // typed, and "annual" finds Annual 15.
        let wanted = number_word(term.strip_prefix('#').unwrap_or(term));
        if !wanted.is_empty() {
            let numbers = query_all(
                &self.conn,
                "SELECT id, number FROM issue WHERE instr(lower(number), ?1) > 0",
                [&wanted],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let issues = owners.entry(Kind::IssueNumber).or_default();
            for (issue, number) in numbers {
                if number_word(&number) == wanted
                    || number.split_whitespace().any(|w| number_word(w) == wanted)
                {
                    issues.insert(issue, Kind::IssueNumber.weight());
                }
            }
            if issues.is_empty() {
                owners.remove(&Kind::IssueNumber);
            }
        }
        Ok(owners)
    }

    /// Each live copy the entries found by a word lead to, with the best
    /// score it is found by.
    fn copies_for(
        &self,
        mut owners: HashMap<Kind, HashMap<i64, f64>>,
    ) -> Result<HashMap<CopyId, f64>> {
        let mut copies: HashMap<CopyId, f64> = HashMap::new();
        // In a fixed order, so the same search always runs the same way.
        for kind in Kind::ALL {
            let Some(scored) = owners.remove(&kind) else {
                continue;
            };
            let ids: Vec<i64> = scored.keys().copied().collect();
            let pairs = query_all(
                &self.conn,
                kind.copies(),
                [json_ids(ids.iter().copied())],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, CopyId>(1)?)),
            )?;
            for (owner, copy) in pairs {
                let score = scored.get(&owner).copied().unwrap_or(0.0);
                let best = copies.entry(copy).or_insert(0.0);
                *best = best.max(score);
            }
        }
        Ok(copies)
    }
}
