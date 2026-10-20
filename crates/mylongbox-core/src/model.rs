//! The catalog's data types: Series → Issue → Edition → Copy.

use std::fmt;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use uuid::Uuid;

use crate::date::Date;
use crate::issue_number::IssueNumber;
use crate::money::{Amount, Currency};

macro_rules! id_type {
    ($($(#[$meta:meta])* $name:ident;)+) => {$(
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) i64);

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                i64::column_result(value).map(Self)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_i64(self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                i64::deserialize(d).map(Self)
            }
        }
    )+};
}

id_type! {
    PublisherId;
    SeriesId;
    IssueId;
    EditionId;
    CopyId;
    BoxId;
    CreatorId;
    CharacterId;
    TeamId;
    StoryArcId;
    TagId;
    /// One dated valuation of a copy.
    ValueId;
    /// A group of rows moved to Trash together.
    TrashId;
    /// A named filter set in the sidebar.
    SavedShelfId;
}

/// Declares an enum stored in the database as fixed text codes.
macro_rules! code_enum {
    ($(#[$meta:meta])* pub enum $name:ident {
        $($(#[$vmeta:meta])* $variant:ident = $code:literal,)+
    }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The code stored in the database. Never change these.
            pub const fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }

            pub fn from_code(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.code()))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let text = value.as_str()?;
                Self::from_code(text).ok_or_else(|| invalid(stringify!($name), text))
            }
        }

        /// Saved filter rules keep the same codes as the database.
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.code())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let code = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::from_code(&code).ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        concat!("invalid ", stringify!($name), ": {:?}"),
                        code
                    ))
                })
            }
        }
    };
}

pub(crate) fn invalid(what: &str, value: &str) -> FromSqlError {
    FromSqlError::Other(format!("invalid {what}: {value:?}").into())
}

code_enum! {
    /// What kind of publication a series is. Forms and fields adapt to it.
    pub enum Category {
        Comic = "comic",
        Magazine = "magazine",
        CollectedEdition = "collected",
    }
}

code_enum! {
    pub enum MagazineFormat {
        Standard = "standard",
        Treasury = "treasury",
        Digest = "digest",
    }
}

code_enum! {
    pub enum Distribution {
        Direct = "direct",
        Newsstand = "newsstand",
    }
}

code_enum! {
    /// What a copy's photo shows: one of its covers, or a signature
    /// inside it.
    pub enum Side {
        Front = "front",
        Back = "back",
        Signature = "signature",
    }
}

code_enum! {
    pub enum Binding {
        Hardcover = "hardcover",
        Softcover = "softcover",
        OversizedHardcover = "oversized_hardcover",
    }
}

code_enum! {
    /// Who graded a comic or magazine, if anyone.
    pub enum GradeType {
        /// The owner's own estimate.
        Raw = "raw",
        Cgc = "cgc",
        Cbcs = "cbcs",
        Pgx = "pgx",
    }
}

code_enum! {
    /// How white a comic's or magazine's pages still are, on CGC's scale,
    /// from best to worst.
    pub enum PageQuality {
        White = "white",
        OffWhiteToWhite = "off_white_to_white",
        OffWhite = "off_white",
        CreamToOffWhite = "cream_to_off_white",
        Cream = "cream",
        LightTanToOffWhite = "light_tan_to_off_white",
        LightTanToCream = "light_tan_to_cream",
        LightTan = "light_tan",
        TanToOffWhite = "tan_to_off_white",
        TanToCream = "tan_to_cream",
        Tan = "tan",
        DarkTanToOffWhite = "dark_tan_to_off_white",
        DarkTan = "dark_tan",
        BrownToOffWhite = "brown_to_off_white",
        BrownToTan = "brown_to_tan",
        Brown = "brown",
        BrownBrittle = "brown_brittle",
    }
}

code_enum! {
    /// What a creator did on an issue. Listed in the order credits show.
    pub enum Role {
        Writer = "writer",
        Penciller = "penciller",
        Inker = "inker",
        Colorist = "colorist",
        Letterer = "letterer",
        CoverArtist = "cover_artist",
        Editor = "editor",
        /// Magazines: wrote an article.
        Article = "article",
        /// Magazines: gave or conducted an interview.
        Interview = "interview",
    }
}

code_enum! {
    /// How a signature is known to be genuine.
    pub enum Verification {
        /// No proof beyond the signature itself.
        Unverified = "unverified",
        /// The owner saw it signed.
        Witnessed = "witnessed",
        /// A certificate of authenticity from an issuer other than JSA.
        Certificate = "coa",
        /// A JSA letter or sticker on a book that is not slabbed.
        Jsa = "jsa",
        /// CGC Signature Series: signed in front of a CGC witness.
        CgcSignatureSeries = "cgc_ss",
        /// CGC Signature Verified: authenticated through JSA and slabbed.
        CgcJsa = "cgc_jsa",
        /// CBCS's Authentic Signature program: signed in front of a CBCS
        /// witness.
        CbcsWitnessed = "cbcs_witnessed",
        /// CBCS's Verified Signature program: signed without a CBCS
        /// witness and authenticated afterwards.
        CbcsVerified = "cbcs",
        PgxVerified = "pgx",
    }
}

code_enum! {
    /// Where on a book a signature is.
    pub enum SignaturePlacement {
        FrontCover = "front_cover",
        InsideFrontCover = "inside_front_cover",
        FirstPage = "first_page",
        InteriorPage = "interior_page",
        InsideBackCover = "inside_back_cover",
        BackCover = "back_cover",
    }
}

/// One signature on a copy, apart from who signed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureFields {
    pub verification: Verification,
    /// The number of its certificate, when it has one.
    pub cert_number: Option<String>,
    pub signed_on: Option<Date>,
    /// Where it was signed, e.g. `NYCC 2015`.
    pub place: String,
    /// A sketch (remark) drawn with the signature.
    pub sketch: bool,
    /// Where on the book it is.
    pub placement: Option<SignaturePlacement>,
}

impl SignatureFields {
    pub fn new(verification: Verification) -> Self {
        Self {
            verification,
            cert_number: None,
            signed_on: None,
            place: String::new(),
            sketch: false,
            placement: None,
        }
    }
}

code_enum! {
    /// Condition of a collected edition, or of its dust jacket.
    pub enum BookCondition {
        Sealed = "sealed",
        New = "new",
        LikeNew = "like_new",
        VeryGood = "very_good",
        Good = "good",
        Fair = "fair",
        Poor = "poor",
    }
}

impl ToSql for Currency {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.code()))
    }
}

impl FromSql for Currency {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Self::from_code(text).ok_or_else(|| invalid("currency", text))
    }
}

impl ToSql for Date {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for Date {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        text.parse().map_err(|_| invalid("date", text))
    }
}

/// A row's permanent identity across exports and re-imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowUuid(pub Uuid);

impl RowUuid {
    pub(crate) fn new_random() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for RowUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl ToSql for RowUuid {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for RowUuid {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Uuid::try_parse(text)
            .map(Self)
            .map_err(|_| invalid("uuid", text))
    }
}

/// A grade from 0.5 to 10.0 in steps of 0.1, held in tenths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Grade(u8);

impl Grade {
    pub const MIN: Self = Self(5);
    pub const MAX: Self = Self(100);

    pub fn from_tenths(tenths: u8) -> Option<Self> {
        (Self::MIN.0..=Self::MAX.0)
            .contains(&tenths)
            .then_some(Self(tenths))
    }

    pub fn tenths(self) -> u8 {
        self.0
    }

    /// The letters graders use for this grade, such as `NM` for 9.4, or
    /// `None` for a grade between the standard steps.
    pub fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            100 => "GM",
            99 => "MT",
            98 => "NM/M",
            96 => "NM+",
            94 => "NM",
            92 => "NM-",
            90 => "VF/NM",
            85 => "VF+",
            80 => "VF",
            75 => "VF-",
            70 => "FN/VF",
            65 => "FN+",
            60 => "FN",
            55 => "FN-",
            50 => "VG/FN",
            45 => "VG+",
            40 => "VG",
            35 => "VG-",
            30 => "G/VG",
            25 => "G+",
            20 => "GD",
            18 => "GD-",
            15 => "FR/GD",
            10 => "FR",
            5 => "PR",
            _ => return None,
        })
    }

    /// Reads `9.8`, `10`, or `10.0`. Anything finer than tenths is refused.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (whole, tenth) = text.split_once('.').unwrap_or((text, "0"));
        if whole.is_empty()
            || whole.len() > 2
            || tenth.len() != 1
            || !whole
                .bytes()
                .chain(tenth.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        // Two digits can pass for a whole grade, and 26 tenfold already
        // overflows a u8, so the sum is checked rather than assumed small.
        let tenths = whole
            .parse::<u8>()
            .ok()?
            .checked_mul(10)?
            .checked_add(tenth.parse::<u8>().ok()?)?;
        Self::from_tenths(tenths)
    }
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.0 / 10, self.0 % 10)
    }
}

impl ToSql for Grade {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(i64::from(self.0)))
    }
}

impl FromSql for Grade {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let n = value.as_i64()?;
        u8::try_from(n)
            .ok()
            .and_then(Self::from_tenths)
            .ok_or_else(|| invalid("grade", &n.to_string()))
    }
}

/// The month and year printed on a cover; the month is often unknown for
/// annuals and books.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverDate {
    year: u16,
    month: Option<u8>,
}

impl CoverDate {
    pub fn new(year: u16, month: Option<u8>) -> Option<Self> {
        let valid = (1..=9999).contains(&year) && month.is_none_or(|m| (1..=12).contains(&m));
        valid.then_some(Self { year, month })
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> Option<u8> {
        self.month
    }

    /// One month later, for "add next issue"; a year-only date stays put.
    pub fn next_month(self) -> Option<Self> {
        match self.month {
            None => Some(self),
            Some(12) => Self::new(self.year.checked_add(1)?, Some(1)),
            Some(m) => Self::new(self.year, Some(m + 1)),
        }
    }
}

/// A date known to the year, the month, or the day, such as when an
/// edition came out. Held and stored as `YYYY`, `YYYY-MM`, or
/// `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartialDate {
    year: u16,
    month: Option<u8>,
    day: Option<u8>,
}

impl PartialDate {
    /// A day needs its month, and must be a real day of it.
    pub fn new(year: u16, month: Option<u8>, day: Option<u8>) -> Option<Self> {
        match (month, day) {
            (None, Some(_)) => None,
            (Some(m), Some(d)) => Date::new(year, m, d).ok().map(Self::from),
            (month, None) => CoverDate::new(year, month).map(|_| Self {
                year,
                month,
                day: None,
            }),
        }
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> Option<u8> {
        self.month
    }

    /// The whole date, when the day is known.
    pub fn date(self) -> Option<Date> {
        Date::new(self.year, self.month?, self.day?).ok()
    }
}

impl From<Date> for PartialDate {
    fn from(date: Date) -> Self {
        Self {
            year: date.year(),
            month: Some(date.month()),
            day: Some(date.day()),
        }
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(m) = self.month {
            write!(f, "-{m:02}")?;
        }
        if let Some(d) = self.day {
            write!(f, "-{d:02}")?;
        }
        Ok(())
    }
}

impl std::str::FromStr for PartialDate {
    type Err = ();

    /// Reads `YYYY`, `YYYY-MM`, or `YYYY-MM-DD` exactly.
    fn from_str(text: &str) -> Result<Self, ()> {
        let parts: Vec<&str> = text.split('-').collect();
        let widths = [4, 2, 2];
        if parts.len() > 3
            || parts
                .iter()
                .zip(widths)
                .any(|(p, w)| p.len() != w || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(());
        }
        let year = parts[0].parse().map_err(|_| ())?;
        let small = |i: usize| parts.get(i).map(|p| p.parse::<u8>().map_err(|_| ()));
        let month = small(1).transpose()?;
        let day = small(2).transpose()?;
        Self::new(year, month, day).ok_or(())
    }
}

impl ToSql for PartialDate {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for PartialDate {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        text.parse().map_err(|()| invalid("date", text))
    }
}

/// A 13-digit ISBN with a valid check digit, held without hyphens.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Isbn(String);

impl Isbn {
    /// Accepts hyphens and spaces between digits, as printed on books.
    pub fn parse(text: &str) -> Option<Self> {
        let digits: String = text.chars().filter(|c| !matches!(c, '-' | ' ')).collect();
        if digits.len() != 13 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let sum: u32 = digits
            .bytes()
            .enumerate()
            .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
            .sum();
        sum.is_multiple_of(10).then_some(Self(digits))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ToSql for Isbn {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.as_str()))
    }
}

impl FromSql for Isbn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Self::parse(text).ok_or_else(|| invalid("ISBN", text))
    }
}

impl ToSql for IssueNumber {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for IssueNumber {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Self::new(text).map_err(|_| invalid("issue number", text))
    }
}

/// A titled run: a comic's run, a magazine, or a line of collected books.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesFields {
    pub name: String,
    pub category: Category,
    pub publisher: Option<PublisherId>,
    /// Such as Vertigo for DC; empty when none.
    pub imprint: String,
    pub volume: Option<u32>,
    pub start_year: Option<u16>,
    /// For gap detection, when known.
    pub known_issue_count: Option<u32>,
    /// Magazines only.
    pub magazine_format: Option<MagazineFormat>,
    /// Magazines only.
    pub black_and_white: Option<bool>,
    pub notes: String,
}

impl SeriesFields {
    pub fn new(name: impl Into<String>, category: Category) -> Self {
        Self {
            name: name.into(),
            category,
            publisher: None,
            imprint: String::new(),
            volume: None,
            start_year: None,
            known_issue_count: None,
            magazine_format: None,
            black_and_white: None,
            notes: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    pub id: SeriesId,
    pub uuid: RowUuid,
    pub trash: Option<TrashId>,
    pub fields: SeriesFields,
}

/// One numbered issue of a series, or one volume of a book line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueFields {
    /// `None` only for a standalone collected edition.
    pub number: Option<IssueNumber>,
    /// The number across the title's relaunches, as in Amazing
    /// Spider-Man (2018) #45, legacy #846.
    pub legacy_number: Option<u32>,
    pub title: String,
    pub cover_date: Option<CoverDate>,
    pub release_date: Option<Date>,
    pub page_count: Option<u32>,
    pub synopsis: String,
    pub is_key: bool,
    pub key_reason: String,
}

impl IssueFields {
    pub fn new(number: Option<IssueNumber>) -> Self {
        Self {
            number,
            legacy_number: None,
            title: String::new(),
            cover_date: None,
            release_date: None,
            page_count: None,
            synopsis: String::new(),
            is_key: false,
            key_reason: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub id: IssueId,
    pub uuid: RowUuid,
    pub series: SeriesId,
    pub trash: Option<TrashId>,
    pub fields: IssueFields,
}

/// One published version of an issue: a cover variant or a printing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditionFields {
    /// e.g. `Cover B`; empty for the standard cover.
    pub variant: String,
    pub printing: Option<u32>,
    pub distribution: Option<Distribution>,
    pub cover_price: Option<Amount>,
    /// The Canadian price printed beside the US one, in Canadian cents.
    /// Shown as printed and never converted or totalled.
    pub canadian_cover_price_cents: Option<i64>,
    /// When this edition came out, for one published after the issue
    /// first was, such as a facsimile.
    pub published: Option<PartialDate>,
    pub barcode: Option<String>,
    /// Collected editions only.
    pub isbn: Option<Isbn>,
    /// Collected editions only.
    pub binding: Option<Binding>,
    /// Collected editions only.
    pub has_dust_jacket: Option<bool>,
    /// Collected editions only.
    pub has_slipcase: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edition {
    pub id: EditionId,
    pub uuid: RowUuid,
    pub issue: IssueId,
    pub trash: Option<TrashId>,
    pub fields: EditionFields,
}

/// One physical book the owner has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyFields {
    /// Comics and magazines only.
    pub grade: Option<Grade>,
    /// Comics and magazines only.
    pub grade_type: Option<GradeType>,
    pub cert_number: Option<String>,
    pub label_type: Option<String>,
    /// Comics and magazines only.
    pub page_quality: Option<PageQuality>,
    /// Collected editions only.
    pub book_condition: Option<BookCondition>,
    /// Collected editions only.
    pub dust_jacket_condition: Option<BookCondition>,
    pub purchase_price: Option<Amount>,
    pub purchase_date: Option<Date>,
    pub seller: String,
    /// The named collection it once belonged to, such as Mile High;
    /// empty when none.
    pub pedigree: String,
    pub storage_box: Option<BoxId>,
    /// Position within the box; requires a box.
    pub box_position: Option<u32>,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copy {
    pub id: CopyId,
    pub uuid: RowUuid,
    pub edition: EditionId,
    pub trash: Option<TrashId>,
    /// When the copy was cataloged, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub added_at: String,
    pub fields: CopyFields,
}

/// A group of rows in Trash, listed for the user to restore or purge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashGroup {
    pub id: TrashId,
    /// UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub trashed_at: String,
    /// What was deleted, e.g. `Amazing Spider-Man #121`.
    pub label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_grades_have_names() {
        let name = |text: &str| Grade::parse(text).unwrap().name();
        assert_eq!(name("9.4"), Some("NM"));
        assert_eq!(name("9.8"), Some("NM/M"));
        assert_eq!(name("10"), Some("GM"));
        assert_eq!(name("0.5"), Some("PR"));
        assert_eq!(name("9.3"), None);
        let named = (5..=100)
            .filter_map(Grade::from_tenths)
            .filter(|g| g.name().is_some())
            .count();
        assert_eq!(named, 25);
    }

    #[test]
    fn grades_parse_in_tenths() {
        assert_eq!(Grade::parse("9.8").map(Grade::tenths), Some(98));
        assert_eq!(Grade::parse("10").map(Grade::tenths), Some(100));
        assert_eq!(Grade::parse("0.5").map(Grade::tenths), Some(5));
        for bad in [
            "0.4", "10.1", "9.85", "9.", ".5", "-1", "nm", "", "100", "11", "26", "25.6", "99.9",
        ] {
            assert_eq!(Grade::parse(bad), None, "{bad}");
        }
        assert_eq!(Grade::parse("9.8").unwrap().to_string(), "9.8");
        assert_eq!(Grade::MAX.to_string(), "10.0");
    }

    #[test]
    fn cover_dates_advance_by_month() {
        let d = |y, m| CoverDate::new(y, m).unwrap();
        assert_eq!(d(1973, Some(6)).next_month(), Some(d(1973, Some(7))));
        assert_eq!(d(1973, Some(12)).next_month(), Some(d(1974, Some(1))));
        assert_eq!(d(1973, None).next_month(), Some(d(1973, None)));
        assert_eq!(CoverDate::new(1973, Some(13)), None);
        assert_eq!(CoverDate::new(0, None), None);
    }

    #[test]
    fn partial_dates_read_and_write_as_far_as_known() {
        for text in ["2019", "2019-07", "2019-07-03"] {
            let d: PartialDate = text.parse().unwrap();
            assert_eq!(d.to_string(), text);
        }
        let day: PartialDate = "2019-07-03".parse().unwrap();
        assert_eq!(day.date(), crate::Date::new(2019, 7, 3).ok());
        assert_eq!("2019-07".parse::<PartialDate>().unwrap().date(), None);
        for bad in [
            "19",
            "2019-7",
            "2019-13",
            "2019-02-30",
            "2019--03",
            "2019-07-03-01",
            "0000",
            "",
            "July 2019",
        ] {
            assert!(bad.parse::<PartialDate>().is_err(), "{bad}");
        }
        assert_eq!(PartialDate::new(2019, None, Some(3)), None);
        assert!(
            "2018-12-31".parse::<PartialDate>().unwrap() < "2019".parse::<PartialDate>().unwrap()
        );
    }

    #[test]
    fn isbns_need_a_valid_check_digit() {
        assert_eq!(
            Isbn::parse("978-1-302-90420-3").map(|i| i.as_str().to_owned()),
            Some("9781302904203".to_owned())
        );
        assert_eq!(Isbn::parse("9781302904204"), None);
        assert_eq!(Isbn::parse("978130290420"), None);
        assert_eq!(Isbn::parse("97813029042O3"), None);
    }

    #[test]
    fn codes_round_trip() {
        for c in Category::ALL {
            assert_eq!(Category::from_code(c.code()), Some(*c));
        }
        for c in BookCondition::ALL {
            assert_eq!(BookCondition::from_code(c.code()), Some(*c));
        }
        assert_eq!(Category::from_code("Comic"), None);
    }
}
