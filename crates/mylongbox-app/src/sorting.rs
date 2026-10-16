//! Sorting results, and the totals shown under them.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use mylongbox_core::{Amount, CopyRow, Currency, Date, RateTable};

use crate::i18n::{gettext_noop, ngettext_f};
/// What results can be sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortBy {
    /// Series and issue for a section, best match for a search.
    #[default]
    Usual,
    CoverDate,
    Grade,
    Value,
    PurchaseDate,
    Added,
}

/// Each way to sort, with the name its menu item's action carries.
pub const SORTS: [(SortBy, &str, &str); 6] = [
    (SortBy::Usual, "usual", gettext_noop("_Usual Order")),
    (SortBy::CoverDate, "cover-date", gettext_noop("_Cover Date")),
    (SortBy::Grade, "grade", gettext_noop("_Grade")),
    (SortBy::Value, "value", gettext_noop("Current _Value")),
    (
        SortBy::PurchaseDate,
        "purchase-date",
        gettext_noop("_Purchase Date"),
    ),
    (SortBy::Added, "added", gettext_noop("Date _Added")),
];

impl SortBy {
    pub fn from_name(name: &str) -> Self {
        SORTS
            .iter()
            .find(|(_, n, _)| *n == name)
            .map_or(Self::Usual, |s| s.0)
    }
}

/// Sorts `rows` by `by`, smallest or earliest first unless `reversed`.
/// Books without the field go last either way, and the sort is stable,
/// so books that tie keep the usual order. `worth` gives a value in one
/// currency so values in different currencies compare.
pub fn sort(
    rows: &mut [CopyRow],
    by: SortBy,
    reversed: bool,
    worth: &dyn Fn(Amount) -> Option<i64>,
) {
    fn key<T: Ord>(a: Option<T>, b: Option<T>, reversed: bool) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) if reversed => b.cmp(&a),
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
    match by {
        SortBy::Usual => {
            if reversed {
                rows.reverse();
            }
        }
        SortBy::CoverDate => rows.sort_by(|a, b| {
            let cover = |r: &CopyRow| r.cover_date.map(|c| (c.year(), c.month().unwrap_or(0)));
            key(cover(a), cover(b), reversed)
        }),
        SortBy::Grade => {
            rows.sort_by(|a, b| {
                key(
                    a.grade.map(|g| g.tenths()),
                    b.grade.map(|g| g.tenths()),
                    reversed,
                )
            });
        }
        SortBy::Value => {
            rows.sort_by(|a, b| key(a.value.and_then(worth), b.value.and_then(worth), reversed))
        }
        SortBy::PurchaseDate => {
            rows.sort_by(|a, b| key(a.purchase_date, b.purchase_date, reversed))
        }
        SortBy::Added => rows.sort_by(|a, b| {
            key(
                Some(a.added_at.as_str()),
                Some(b.added_at.as_str()),
                reversed,
            )
        }),
    }
}

/// The sum of `amounts` as text, each converted to `target` at the rate
/// for its date (the newest rate when undated or when `today` asks for
/// what they are worth now), or summed per currency when no target is
/// chosen. Amounts no rate covers are counted rather than guessed.
pub fn total(
    amounts: impl IntoIterator<Item = (Amount, Option<Date>)>,
    target: Option<Currency>,
    rates: &RateTable,
    today: bool,
    show: impl Fn(Amount) -> String,
) -> Option<String> {
    let mut sums: BTreeMap<&'static str, (Currency, i64)> = BTreeMap::new();
    let mut unconverted = 0;
    let mut any = false;
    for (amount, on) in amounts {
        any = true;
        let converted = match (target, on) {
            (None, _) => Ok(amount),
            (Some(t), Some(date)) if !today => rates.convert(amount, date, t),
            (Some(t), _) => rates.convert_at_latest(amount, t),
        };
        match converted {
            Ok(a) => {
                let entry = sums.entry(a.currency().code()).or_insert((a.currency(), 0));
                entry.1 = entry.1.saturating_add(a.units());
            }
            Err(_) => unconverted += 1,
        }
    }
    if !any {
        return None;
    }
    // Dollars first, then gold, then bitcoin.
    let order = [Currency::Usd, Currency::Gold, Currency::Btc];
    let mut parts: Vec<String> = order
        .iter()
        .filter_map(|c| sums.get(c.code()))
        .map(|(c, units)| show(Amount::new(*c, *units)))
        .collect();
    if parts.is_empty() {
        parts.push("\u{2014}".to_owned());
    }
    let mut text = parts.join(" + ");
    if unconverted > 0 {
        let note = ngettext_f(
            "{count} without a rate",
            "{count} without a rate",
            unconverted,
            &[("count", &unconverted.to_string())],
        );
        text.push_str(&format!(" ({note})"));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mylongbox_core::{
        BookEntry, Category, CopyFields, Database, ExchangeRate, Grade, IssueNumber, SeriesChoice,
        Shelf,
    };

    fn usd(t: &str) -> Amount {
        Amount::parse(Currency::Usd, t).unwrap()
    }

    fn date(t: &str) -> Date {
        t.parse().unwrap()
    }

    /// Four books, each with some fields left out.
    fn rows() -> Vec<CopyRow> {
        let db = Database::open_in_memory().unwrap();
        let books = [
            (
                "1",
                Some((1980, Some(5))),
                Some("9.0"),
                Some("30"),
                Some("2020-01-01"),
            ),
            ("2", Some((1975, None)), None, Some("100"), None),
            ("3", None, Some("4.0"), None, Some("2019-06-01")),
            (
                "4",
                Some((1980, Some(1))),
                Some("9.8"),
                Some("5"),
                Some("2021-03-03"),
            ),
        ];
        for (number, cover, grade, value, bought) in books {
            let choice = match db.list_series().unwrap().first() {
                Some(s) => SeriesChoice::Existing(s.id),
                None => SeriesChoice::New {
                    name: "Thor".into(),
                    start_year: Some(1966),
                    volume: None,
                    publisher: None,
                    imprint: String::new(),
                    magazine_format: None,
                    black_and_white: None,
                },
            };
            let mut e = BookEntry::new(Category::Comic, choice, IssueNumber::new(number).ok());
            e.issue.cover_date = cover.and_then(|(y, m)| mylongbox_core::CoverDate::new(y, m));
            e.copy = CopyFields {
                grade: grade.and_then(Grade::parse),
                purchase_date: bought.map(date),
                ..CopyFields::default()
            };
            let copy = db.add_book(&e).unwrap().copy;
            if let Some(v) = value {
                db.add_value(copy, date("2024-01-01"), usd(v), "").unwrap();
            }
        }
        db.browse(Shelf::Everything).unwrap()
    }

    fn numbers(rows: &[CopyRow]) -> Vec<&str> {
        rows.iter().map(|r| r.number.as_deref().unwrap()).collect()
    }

    fn sorted(by: SortBy, reversed: bool) -> Vec<String> {
        let mut rows = rows();
        sort(&mut rows, by, reversed, &|a| Some(a.units()));
        numbers(&rows).into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn each_field_sorts_with_blanks_last() {
        assert_eq!(sorted(SortBy::Usual, false), ["1", "2", "3", "4"]);
        assert_eq!(sorted(SortBy::Usual, true), ["4", "3", "2", "1"]);
        assert_eq!(sorted(SortBy::CoverDate, false), ["2", "4", "1", "3"]);
        assert_eq!(sorted(SortBy::CoverDate, true), ["1", "4", "2", "3"]);
        assert_eq!(sorted(SortBy::Grade, false), ["3", "1", "4", "2"]);
        assert_eq!(sorted(SortBy::Grade, true), ["4", "1", "3", "2"]);
        assert_eq!(sorted(SortBy::Value, true), ["2", "1", "4", "3"]);
        assert_eq!(sorted(SortBy::PurchaseDate, false), ["3", "1", "4", "2"]);
        // Added in order, so by date added they keep it.
        assert_eq!(sorted(SortBy::Added, false), ["1", "2", "3", "4"]);
    }

    #[test]
    fn values_compare_through_one_currency() {
        let mut rows = rows();
        // A worth that ranks book 4's $5 above everything.
        sort(&mut rows, SortBy::Value, true, &|a| {
            Some(if a.units() == 500 {
                i64::MAX
            } else {
                a.units()
            })
        });
        assert_eq!(numbers(&rows)[0], "4");
        // Values that cannot be compared go last, like blanks.
        sort(&mut rows, SortBy::Value, false, &|a| {
            (a.units() != 10_000).then_some(a.units())
        });
        assert_eq!(numbers(&rows), ["4", "1", "2", "3"]);
    }

    #[test]
    fn sorts_have_names_for_their_menu() {
        for (by, name, _) in SORTS {
            assert_eq!(SortBy::from_name(name), by);
        }
        assert_eq!(SortBy::from_name("nonsense"), SortBy::Usual);
    }

    fn show(a: Amount) -> String {
        a.to_string()
    }

    #[test]
    fn totals_add_up_in_the_chosen_currency() {
        let rates = RateTable::new([
            ExchangeRate::new(date("2020-01-01"), usd("1000"), usd("10000")).unwrap(),
            ExchangeRate::new(date("2024-01-01"), usd("2000"), usd("40000")).unwrap(),
        ]);
        let ounce = Amount::parse(Currency::Gold, "1").unwrap();
        let amounts = [(usd("10"), None), (ounce, Some(date("2021-06-01")))];
        let usd_total = |today| total(amounts, Some(Currency::Usd), &rates, today, show);
        // An ounce bought in 2021 cost $1,000 then and is worth $2,000 now.
        assert_eq!(usd_total(false).as_deref(), Some("$1010.00"));
        assert_eq!(usd_total(true).as_deref(), Some("$2010.00"));
        // As entered, each currency adds up on its own.
        assert_eq!(
            total(amounts, None, &rates, true, show).as_deref(),
            Some("$10.00 + 1.0000 oz")
        );
        // Nothing to add up, and amounts no rate covers.
        assert_eq!(total([], Some(Currency::Usd), &rates, true, show), None);
        let early = [(ounce, Some(date("2019-01-01"))), (usd("2"), None)];
        assert_eq!(
            total(early, Some(Currency::Usd), &rates, false, show).as_deref(),
            Some("$2.00 (1 without a rate)")
        );
        assert_eq!(
            total(
                [(ounce, None)],
                Some(Currency::Usd),
                &RateTable::default(),
                true,
                show
            )
            .as_deref(),
            Some("\u{2014} (1 without a rate)")
        );
    }
}
