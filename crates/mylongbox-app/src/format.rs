//! How catalog values read on screen.

use std::cell::RefCell;

use gtk::glib;
use mylongbox_core::{
    Amount, Binding, BookCondition, CoverDate, Date, Distribution, Grade, GradeType,
    MagazineFormat, RateTable, Role, Verification,
};

use crate::settings::Settings;

thread_local! {
    /// How amounts and grades are shown; the app updates it when the
    /// preferences or the exchange rates change. GTK runs on one thread.
    static DISPLAY: RefCell<Display> = RefCell::new(Display::default());
}

#[derive(Clone, Debug, Default)]
pub struct Display {
    pub settings: Settings,
    pub rates: RateTable,
}

pub fn set_display(display: Display) {
    DISPLAY.with(|d| *d.borrow_mut() = display);
}

pub fn display_settings() -> Settings {
    DISPLAY.with(|d| d.borrow().settings)
}

/// An amount in the currency chosen in the preferences, converted at the
/// rate for `on` (or the newest rate when the date is not known), with
/// thousands grouped. "—" means no rate covers it.
pub fn money(amount: Amount, on: Option<Date>) -> String {
    DISPLAY.with(|d| {
        let d = d.borrow();
        let shown = match d.settings.currency {
            None => Ok(amount),
            Some(target) => match on {
                Some(date) => d.rates.convert(amount, date, target),
                None => d.rates.convert_at_latest(amount, target),
            },
        };
        shown.map_or_else(|_| "\u{2014}".to_owned(), |a| grouped(&a.to_string()))
    })
}

/// Adds commas between thousands in the whole part of a formatted
/// amount: `$1234567.89` becomes `$1,234,567.89`.
pub fn grouped(text: &str) -> String {
    let start = text
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(text.len());
    let end = text[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(text.len(), |i| start + i);
    let digits = &text[start..end];
    let mut whole = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            whole.push(',');
        }
        whole.push(c);
    }
    format!("{}{whole}{}", &text[..start], &text[end..])
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// e.g. `CGC 9.8`, or `9.4` for the owner's own estimate, with the
/// grade's letters added when the preferences ask for them.
pub fn grade(grade: Option<Grade>, grade_type: Option<GradeType>) -> String {
    let Some(grade) = grade else {
        return String::new();
    };
    let shown = match grade.name() {
        Some(name) if display_settings().grade_names => format!("{grade} {name}"),
        _ => grade.to_string(),
    };
    let grade = shown;
    match grade_type {
        Some(GradeType::Cgc) => format!("CGC {grade}"),
        Some(GradeType::Cbcs) => format!("CBCS {grade}"),
        Some(GradeType::Pgx) => format!("PGX {grade}"),
        Some(GradeType::Raw) | None => grade.to_string(),
    }
}

pub fn condition(condition: BookCondition) -> &'static str {
    match condition {
        BookCondition::Sealed => "Sealed",
        BookCondition::New => "New",
        BookCondition::LikeNew => "Like New",
        BookCondition::VeryGood => "Very Good",
        BookCondition::Good => "Good",
        BookCondition::Fair => "Fair",
        BookCondition::Poor => "Poor",
    }
}

pub fn verification(verification: Verification) -> &'static str {
    match verification {
        Verification::Unverified => "Unverified",
        Verification::Witnessed => "Witnessed by me",
        Verification::Certificate => "Certificate of authenticity",
        Verification::Jsa => "JSA authenticated",
        Verification::CgcSignatureSeries => "CGC Signature Series",
        Verification::CgcJsa => "CGC Signature Verified (JSA)",
        Verification::CbcsVerified => "CBCS Verified Signature",
        Verification::PgxVerified => "PGX Signature Verified",
    }
}

/// The short form printed on spines and in listings.
pub fn binding_badge(binding: Binding) -> &'static str {
    match binding {
        Binding::Hardcover => "HC",
        Binding::Softcover => "TPB",
        Binding::OversizedHardcover => "OHC",
    }
}

pub fn binding(binding: Binding) -> &'static str {
    match binding {
        Binding::Hardcover => "Hardcover",
        Binding::Softcover => "Softcover",
        Binding::OversizedHardcover => "Oversized hardcover",
    }
}

pub fn distribution(distribution: Distribution) -> &'static str {
    match distribution {
        Distribution::Direct => "Direct edition",
        Distribution::Newsstand => "Newsstand edition",
    }
}

pub fn magazine_format(format: MagazineFormat) -> &'static str {
    match format {
        MagazineFormat::Standard => "Standard size",
        MagazineFormat::Treasury => "Treasury size",
        MagazineFormat::Digest => "Digest size",
    }
}

/// Plural, as a heading over the people credited.
pub fn role(role: Role) -> &'static str {
    match role {
        Role::Writer => "Writers",
        Role::Penciller => "Pencillers",
        Role::Inker => "Inkers",
        Role::Colorist => "Colorists",
        Role::Letterer => "Letterers",
        Role::CoverArtist => "Cover Artists",
        Role::Editor => "Editors",
        Role::Article => "Articles",
        Role::Interview => "Interviews",
    }
}

/// e.g. `2nd printing`.
pub fn printing(printing: u32) -> String {
    let suffix = match (printing % 10, printing % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{printing}{suffix} printing")
}

/// e.g. `Mar 1975`, or `1975` when the month is unknown.
pub fn cover_date(date: Option<CoverDate>) -> String {
    match date {
        None => String::new(),
        Some(d) => match d.month() {
            Some(m) => format!("{} {}", MONTHS[usize::from(m - 1)], d.year()),
            None => d.year().to_string(),
        },
    }
}

/// Reads a cover date as people write it: `1975`, `1975-03`, `3/1975`,
/// `Mar 1975`, or `March 1975`. Empty text is no date; `Err` means the
/// text is not a date.
pub fn parse_cover_date(text: &str) -> Result<Option<CoverDate>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let year = |t: &str| -> Result<u16, ()> {
        if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
            t.parse().map_err(|_| ())
        } else {
            Err(())
        }
    };
    let month = |t: &str| -> Result<u8, ()> {
        if let Ok(m) = t.parse::<u8>() {
            return Ok(m);
        }
        let t = t.to_lowercase();
        let found = MONTHS
            .iter()
            .position(|m| t.len() >= 3 && m.to_lowercase().starts_with(&t[..3.min(t.len())]))
            .ok_or(())?;
        let full = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        if !full[found].starts_with(t.trim_end_matches('.')) {
            return Err(());
        }
        u8::try_from(found + 1).map_err(|_| ())
    };
    let date = if let Some((y, m)) = text.split_once('-') {
        CoverDate::new(year(y)?, Some(month(m)?))
    } else if let Some((m, y)) = text.split_once('/') {
        CoverDate::new(year(y)?, Some(month(m)?))
    } else if let Some((m, y)) = text.split_once(' ') {
        CoverDate::new(year(y.trim())?, Some(month(m)?))
    } else {
        CoverDate::new(year(text)?, None)
    };
    date.map(Some).ok_or(())
}

/// e.g. `Mar 5, 1975`.
pub fn date(date: Date) -> String {
    format!(
        "{} {}, {}",
        MONTHS[usize::from(date.month() - 1)],
        date.day(),
        date.year()
    )
}

/// The local calendar date of a stored UTC timestamp, so a book added
/// late in the evening is not shown as added tomorrow.
pub fn local_date(timestamp: &str) -> Option<Date> {
    let local = glib::DateTime::from_iso8601(timestamp, None)
        .and_then(|t| t.to_local())
        .ok()?;
    Date::new(
        u16::try_from(local.year()).ok()?,
        u8::try_from(local.month()).ok()?,
        u8::try_from(local.day_of_month()).ok()?,
    )
    .ok()
}

/// Today's date on this computer.
pub fn today() -> Option<Date> {
    let now = glib::DateTime::now_local().ok()?;
    Date::new(
        u16::try_from(now.year()).ok()?,
        u8::try_from(now.month()).ok()?,
        u8::try_from(now.day_of_month()).ok()?,
    )
    .ok()
}

/// Joins the parts that are not empty with a middle dot.
pub fn joined<I, S>(parts: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parts
        .into_iter()
        .filter(|p| !p.as_ref().is_empty())
        .map(|p| p.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printings_get_ordinals() {
        let names = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].map(printing);
        assert_eq!(
            names,
            [
                "1st printing",
                "2nd printing",
                "3rd printing",
                "4th printing",
                "11th printing",
                "12th printing",
                "13th printing",
                "21st printing",
                "22nd printing",
                "101st printing",
                "111th printing",
            ]
        );
    }

    #[test]
    fn grades_name_their_grader() {
        let g = Grade::parse("9.8");
        assert_eq!(grade(g, Some(GradeType::Cgc)), "CGC 9.8");
        assert_eq!(grade(g, Some(GradeType::Raw)), "9.8");
        assert_eq!(grade(g, None), "9.8");
        assert_eq!(grade(None, Some(GradeType::Cgc)), "");
    }

    #[test]
    fn dates_read_naturally() {
        assert_eq!(cover_date(CoverDate::new(1975, Some(3))), "Mar 1975");
        assert_eq!(cover_date(CoverDate::new(1975, None)), "1975");
        assert_eq!(cover_date(None), "");
        assert_eq!(date(Date::new(2024, 12, 5).unwrap()), "Dec 5, 2024");
    }

    #[test]
    fn cover_dates_parse_as_people_write_them() {
        let mar = CoverDate::new(1975, Some(3));
        for text in [
            "1975-03",
            "1975-3",
            "3/1975",
            "Mar 1975",
            "march 1975",
            "Mar. 1975",
        ] {
            assert_eq!(parse_cover_date(text), Ok(mar), "{text}");
        }
        assert_eq!(parse_cover_date("1975"), Ok(CoverDate::new(1975, None)));
        assert_eq!(parse_cover_date("  "), Ok(None));
        for bad in [
            "75",
            "13/1975",
            "Marz 1975",
            "Ma 1975",
            "1975-00",
            "spring",
            "1975 03 01",
        ] {
            assert_eq!(parse_cover_date(bad), Err(()), "{bad}");
        }
        // What the form shows reads back as the same date.
        assert_eq!(parse_cover_date(&cover_date(mar)), Ok(mar));
    }

    #[test]
    fn timestamps_become_local_dates() {
        // The exact day depends on the machine's time zone, but a stored
        // timestamp always lands within a day of its UTC date.
        let date = local_date("2026-10-11T03:00:00Z").unwrap();
        let utc = Date::new(2026, 10, 11).unwrap();
        assert!(date.days_until(utc).abs() <= 1, "{date}");
        assert_eq!(local_date("yesterday"), None);
        assert_eq!(local_date(""), None);
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(grouped("$1234567.89"), "$1,234,567.89");
        assert_eq!(grouped("$999.00"), "$999.00");
        assert_eq!(grouped("1000.0000 oz"), "1,000.0000 oz");
        assert_eq!(grouped("0.00870721 BTC"), "0.00870721 BTC");
        assert_eq!(grouped("$-1234.00"), "$-1,234.00");
    }

    #[test]
    fn amounts_show_in_the_chosen_currency_or_a_dash() {
        use mylongbox_core::{Currency, ExchangeRate};
        let rate = ExchangeRate::new(
            Date::new(2020, 1, 1).unwrap(),
            Amount::new(Currency::Usd, 150_000),
            Amount::new(Currency::Usd, 700_000),
        )
        .unwrap();
        let ounce = Amount::new(Currency::Gold, 10_000);
        set_display(Display::default());
        assert_eq!(money(ounce, None), "1.0000 oz");
        set_display(Display {
            settings: Settings {
                currency: Some(Currency::Usd),
                grade_names: true,
                ..Settings::default()
            },
            rates: RateTable::new([rate]),
        });
        assert_eq!(money(ounce, Date::new(2021, 1, 1).ok()), "$1,500.00");
        assert_eq!(money(ounce, Date::new(2019, 1, 1).ok()), "\u{2014}");
        assert_eq!(
            grade(Grade::parse("9.4"), Some(GradeType::Cgc)),
            "CGC 9.4 NM"
        );
        assert_eq!(grade(Grade::parse("9.3"), None), "9.3");
        set_display(Display::default());
    }

    #[test]
    fn joined_skips_empty_parts() {
        assert_eq!(joined(["a", "", "b"]), "a · b");
        assert_eq!(joined(["", ""]), "");
    }
}
