//! How catalog values read on screen.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk::glib;
use mylongbox_core::{
    Amount, Binding, BookCondition, CoverDate, Date, Distribution, Grade, GradeType,
    MagazineFormat, PageQuality, RateTable, Role, Verification,
};

use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f, pgettext};
use crate::settings::Settings;

thread_local! {
    /// How amounts and grades are shown; the app updates it when the
    /// preferences or the exchange rates change. GTK runs on one thread.
    static DISPLAY: RefCell<Display> = RefCell::new(Display::default());

    static LABELS: RefCell<HashMap<&'static str, &'static str>> = RefCell::new(HashMap::new());
}

/// A fixed label, such as a condition's name, in the reader's language.
/// Each is looked up once and kept for the life of the app, so callers
/// can hold it like the English text; there are only a few dozen.
fn label(msgid: &'static str) -> &'static str {
    LABELS.with(|labels| {
        *labels
            .borrow_mut()
            .entry(msgid)
            .or_insert_with(|| Box::leak(gettext(msgid).into_boxed_str()))
    })
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

/// What a list of results adds up to, for the bar under them: how many
/// books, what they are worth today, and what they cost when bought,
/// in the currency chosen in the preferences.
pub fn totals(rows: &[mylongbox_core::CopyRow]) -> String {
    DISPLAY.with(|d| {
        let d = d.borrow();
        let target = d.settings.currency;
        let show = |a: Amount| grouped(&a.to_string());
        let worth = crate::sorting::total(
            rows.iter()
                .filter_map(|r| r.value.map(|v| (v, r.value_date))),
            target,
            &d.rates,
            true,
            show,
        );
        let cost = crate::sorting::total(
            rows.iter()
                .filter_map(|r| r.purchase_price.map(|p| (p, r.purchase_date))),
            target,
            &d.rates,
            false,
            show,
        );
        let n = rows.len();
        let count = ngettext_f(
            "{count} book",
            "{count} books",
            n,
            &[("count", &grouped(&n.to_string()))],
        );
        joined([
            count,
            worth
                .map(|w| gettext_f("Worth {amount}", &[("amount", &w)]))
                .unwrap_or_default(),
            cost.map(|c| gettext_f("Cost {amount}", &[("amount", &c)]))
                .unwrap_or_default(),
        ])
    })
}

/// A value in US cents at the newest rate, so values in different
/// currencies sort together.
pub fn in_cents(amount: Amount) -> Option<i64> {
    DISPLAY.with(|d| {
        d.borrow()
            .rates
            .convert_at_latest(amount, mylongbox_core::Currency::Usd)
            .ok()
            .map(Amount::units)
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

/// The English month abbreviations, which cover dates are read in.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A month's short name, as shown, for months numbered 1 to 12.
fn month_name(month: u8) -> String {
    let context = "month abbreviation";
    match month {
        1 => pgettext(context, "Jan"),
        2 => pgettext(context, "Feb"),
        3 => pgettext(context, "Mar"),
        4 => pgettext(context, "Apr"),
        5 => pgettext(context, "May"),
        6 => pgettext(context, "Jun"),
        7 => pgettext(context, "Jul"),
        8 => pgettext(context, "Aug"),
        9 => pgettext(context, "Sep"),
        10 => pgettext(context, "Oct"),
        11 => pgettext(context, "Nov"),
        _ => pgettext(context, "Dec"),
    }
}

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
        BookCondition::Sealed => label(gettext_noop("Sealed")),
        BookCondition::New => label(gettext_noop("New")),
        BookCondition::LikeNew => label(gettext_noop("Like New")),
        BookCondition::VeryGood => label(gettext_noop("Very Good")),
        BookCondition::Good => label(gettext_noop("Good")),
        BookCondition::Fair => label(gettext_noop("Fair")),
        BookCondition::Poor => label(gettext_noop("Poor")),
    }
}

/// The colour of a book's pages as CGC names it, e.g. `Off-White to
/// White`.
pub fn page_quality(quality: PageQuality) -> &'static str {
    match quality {
        // Translators: this and the names after it are the colours of a
        // comic's pages on CGC's page quality scale, from best to worst.
        PageQuality::White => label(gettext_noop("White")),
        PageQuality::OffWhiteToWhite => label(gettext_noop("Off-White to White")),
        PageQuality::OffWhite => label(gettext_noop("Off-White")),
        PageQuality::CreamToOffWhite => label(gettext_noop("Cream to Off-White")),
        PageQuality::Cream => label(gettext_noop("Cream")),
        PageQuality::LightTanToOffWhite => label(gettext_noop("Light Tan to Off-White")),
        PageQuality::LightTanToCream => label(gettext_noop("Light Tan to Cream")),
        PageQuality::LightTan => label(gettext_noop("Light Tan")),
        PageQuality::TanToOffWhite => label(gettext_noop("Tan to Off-White")),
        PageQuality::TanToCream => label(gettext_noop("Tan to Cream")),
        PageQuality::Tan => label(gettext_noop("Tan")),
        PageQuality::DarkTanToOffWhite => label(gettext_noop("Dark Tan to Off-White")),
        PageQuality::DarkTan => label(gettext_noop("Dark Tan")),
        PageQuality::BrownToOffWhite => label(gettext_noop("Brown to Off-White")),
        PageQuality::BrownToTan => label(gettext_noop("Brown to Tan")),
        PageQuality::Brown => label(gettext_noop("Brown")),
        PageQuality::BrownBrittle => label(gettext_noop("Brown / Brittle")),
    }
}

pub fn verification(verification: Verification) -> &'static str {
    match verification {
        Verification::Unverified => label(gettext_noop("Unverified")),
        Verification::Witnessed => label(gettext_noop("Witnessed by me")),
        Verification::Certificate => label(gettext_noop("Certificate of authenticity")),
        Verification::Jsa => label(gettext_noop("JSA authenticated")),
        Verification::CgcSignatureSeries => label(gettext_noop("CGC Signature Series")),
        Verification::CgcJsa => label(gettext_noop("CGC Signature Verified (JSA)")),
        Verification::CbcsVerified => label(gettext_noop("CBCS Verified Signature")),
        Verification::PgxVerified => label(gettext_noop("PGX Signature Verified")),
    }
}

/// The short form printed on spines and in listings.
pub fn binding_badge(binding: Binding) -> &'static str {
    match binding {
        // Translators: short forms of hardcover, trade paperback, and
        // oversized hardcover, as collectors abbreviate them.
        Binding::Hardcover => label(gettext_noop("HC")),
        Binding::Softcover => label(gettext_noop("TPB")),
        Binding::OversizedHardcover => label(gettext_noop("OHC")),
    }
}

pub fn binding(binding: Binding) -> &'static str {
    match binding {
        Binding::Hardcover => label(gettext_noop("Hardcover")),
        Binding::Softcover => label(gettext_noop("Softcover")),
        Binding::OversizedHardcover => label(gettext_noop("Oversized hardcover")),
    }
}

pub fn distribution(distribution: Distribution) -> &'static str {
    match distribution {
        Distribution::Direct => label(gettext_noop("Direct edition")),
        Distribution::Newsstand => label(gettext_noop("Newsstand edition")),
    }
}

pub fn magazine_format(format: MagazineFormat) -> &'static str {
    match format {
        MagazineFormat::Standard => label(gettext_noop("Standard size")),
        MagazineFormat::Treasury => label(gettext_noop("Treasury size")),
        MagazineFormat::Digest => label(gettext_noop("Digest size")),
    }
}

/// Plural, as a heading over the people credited.
pub fn role(role: Role) -> &'static str {
    match role {
        Role::Writer => label(gettext_noop("Writers")),
        Role::Penciller => label(gettext_noop("Pencillers")),
        Role::Inker => label(gettext_noop("Inkers")),
        Role::Colorist => label(gettext_noop("Colorists")),
        Role::Letterer => label(gettext_noop("Letterers")),
        Role::CoverArtist => label(gettext_noop("Cover Artists")),
        Role::Editor => label(gettext_noop("Editors")),
        Role::Article => label(gettext_noop("Articles")),
        Role::Interview => label(gettext_noop("Interviews")),
    }
}

/// e.g. `2nd printing`.
pub fn printing(printing: u32) -> String {
    let number = printing.to_string();
    let values = [("number", number.as_str())];
    match (printing % 10, printing % 100) {
        // Translators: English ordinals differ by their last digit; use
        // the same form for all four if your language needs no difference.
        (_, 11..=13) => gettext_f("{number}th printing", &values),
        (1, _) => gettext_f("{number}st printing", &values),
        (2, _) => gettext_f("{number}nd printing", &values),
        (3, _) => gettext_f("{number}rd printing", &values),
        _ => gettext_f("{number}th printing", &values),
    }
}

/// e.g. `Mar 1975`, or `1975` when the month is unknown.
pub fn cover_date(date: Option<CoverDate>) -> String {
    match date {
        None => String::new(),
        Some(d) => match d.month() {
            // Translators: a cover date such as "Mar 1975".
            Some(m) => gettext_f(
                "{month} {year}",
                &[("month", &month_name(m)), ("year", &d.year().to_string())],
            ),
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
    // Translators: a date such as "Mar 5, 1975".
    gettext_f(
        "{month} {day}, {year}",
        &[
            ("month", &month_name(date.month())),
            ("day", &date.day().to_string()),
            ("year", &date.year().to_string()),
        ],
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

/// What a copy of a collection made at `now` is called, e.g.
/// `mylongboxdb-Comics-2026-10-10-0930.sqlite`, so copies made over time
/// sort in the order they were made.
pub fn copy_name(collection: &str, now: &glib::DateTime) -> String {
    format!(
        "mylongboxdb-{collection}-{:04}-{:02}-{:02}-{:02}{:02}.sqlite",
        now.year(),
        now.month(),
        now.day_of_month(),
        now.hour(),
        now.minute()
    )
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
    fn copies_are_named_by_collection_and_time() {
        let at = glib::DateTime::from_utc(2026, 3, 7, 9, 5, 30.0).unwrap();
        assert_eq!(
            copy_name("Main Comics", &at),
            "mylongboxdb-Main Comics-2026-03-07-0905.sqlite"
        );
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
