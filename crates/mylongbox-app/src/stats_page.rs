//! Collection Stats: how big the collection is, what it is worth, and
//! how its books break down by condition and age.

use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use mylongbox_core::{Category, CopyId, CopyRow, GradeType, IssueId};

use crate::about;
use crate::format;
use crate::i18n::{gettext, gettext_f, ngettext_f};

/// What the page shows beyond the books themselves.
pub struct Counts {
    /// Series, publishers, and creators with at least one book.
    pub series: usize,
    pub publishers: usize,
    pub creators: usize,
    /// The collection file's size and its photos' share, when known.
    pub size: Option<(u64, u64)>,
}

pub struct Hooks {
    /// Opens a book's page as seen from one of its copies.
    pub open: Rc<dyn Fn(IssueId, CopyId)>,
}

/// How many of the most valuable books are listed.
const TOP: usize = 5;

/// The page for the books in `rows`, every book in the collection.
pub fn page(rows: &[CopyRow], counts: &Counts, hooks: &Hooks) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.add(&size(rows, counts));
    page.add(&value(rows));
    if let Some(group) = most_valuable(rows, hooks) {
        page.add(&group);
    }
    page.add(&condition(rows));
    if let Some(group) = cover_dates(rows) {
        page.add(&group);
    }
    if let Some((total, photos)) = counts.size {
        let group = group(&gettext("Storage"));
        group.add(&property(
            &gettext("Collection File"),
            &about::size_text(total, photos),
        ));
        page.add(&group);
    }
    page
}

fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).build()
}

/// A labelled figure. Markup is off, since names in it are typed by the
/// owner.
fn property(label: &str, value: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(label);
    row.set_subtitle(value);
    row.add_css_class("property");
    row
}

fn books(n: usize) -> String {
    ngettext_f(
        "{count} book",
        "{count} books",
        n,
        &[("count", &format::grouped(&n.to_string()))],
    )
}

fn count(n: usize) -> String {
    format::grouped(&n.to_string())
}

fn size(rows: &[CopyRow], counts: &Counts) -> adw::PreferencesGroup {
    let group = group(&gettext("Size"));
    let of = |category| rows.iter().filter(|r| r.category == category).count();
    group.add(&property(&gettext("Books"), &count(rows.len())));
    for (label, category) in [
        (gettext("Comics"), Category::Comic),
        (gettext("Magazines"), Category::Magazine),
        (gettext("Collected Editions"), Category::CollectedEdition),
    ] {
        group.add(&property(&label, &count(of(category))));
    }
    group.add(&property(&gettext("Series"), &count(counts.series)));
    group.add(&property(&gettext("Publishers"), &count(counts.publishers)));
    group.add(&property(&gettext("Creators"), &count(counts.creators)));
    group
}

fn value(rows: &[CopyRow]) -> adw::PreferencesGroup {
    let group = group(&gettext("Value"));
    let valued: Vec<_> = rows
        .iter()
        .filter_map(|r| r.value.map(|v| (v, r.value_date)))
        .collect();
    let paid: Vec<_> = rows
        .iter()
        .filter_map(|r| r.purchase_price.map(|p| (p, r.purchase_date)))
        .collect();
    let worth = match format::money_total(valued.iter().copied(), true) {
        Some(amount) => gettext_f(
            "{amount}, from the {books} with a value",
            &[("amount", &amount), ("books", &books(valued.len()))],
        ),
        None => gettext("No values recorded"),
    };
    group.add(&property(&gettext("Worth Now"), &worth));
    let cost = match format::money_total(paid.iter().copied(), false) {
        Some(amount) => gettext_f(
            "{amount}, for the {books} with a price paid",
            &[("amount", &amount), ("books", &books(paid.len()))],
        ),
        None => gettext("No prices paid recorded"),
    };
    group.add(&property(&gettext("Paid"), &cost));
    let both: Vec<_> = rows
        .iter()
        .filter_map(|r| {
            Some((
                (r.value?, r.value_date),
                (r.purchase_price?, r.purchase_date),
            ))
        })
        .collect();
    if let Some(gain) = format::gain(both.iter().copied()) {
        group.add(&property(
            &gettext("Gain or Loss"),
            &gettext_f(
                "{amount}, on the {books} with both",
                &[("amount", &gain), ("books", &books(both.len()))],
            ),
        ));
    }
    group
}

fn most_valuable(rows: &[CopyRow], hooks: &Hooks) -> Option<adw::PreferencesGroup> {
    let mut valued: Vec<(&CopyRow, i64)> = rows
        .iter()
        .filter_map(|r| Some((r, format::in_cents(r.value?)?)))
        .collect();
    if valued.is_empty() {
        return None;
    }
    valued.sort_by_key(|v| std::cmp::Reverse(v.1));
    let group = group(&gettext("Most Valuable"));
    for (r, _) in valued.into_iter().take(TOP) {
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(&r.label());
        // What it is worth now, so at the newest rate.
        row.set_subtitle(&r.value.map(|v| format::money(v, None)).unwrap_or_default());
        row.set_activatable(true);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let (open, issue, copy) = (hooks.open.clone(), r.issue, r.copy);
        row.connect_activated(move |_| open(issue, copy));
        group.add(&row);
    }
    Some(group)
}

fn condition(rows: &[CopyRow]) -> adw::PreferencesGroup {
    let group = group(&gettext("Condition"));
    // Collected editions have a book condition rather than a grade.
    let graded = |kind: Option<GradeType>| {
        rows.iter()
            .filter(|r| r.category != Category::CollectedEdition)
            .filter(|r| r.grade_type.filter(|t| *t != GradeType::Raw) == kind)
            .count()
    };
    group.add(&property(&gettext("Raw or Ungraded"), &count(graded(None))));
    for (label, kind) in [
        ("CGC", GradeType::Cgc),
        ("CBCS", GradeType::Cbcs),
        ("PGX", GradeType::Pgx),
    ] {
        group.add(&property(
            &gettext_f("Graded by {company}", &[("company", label)]),
            &count(graded(Some(kind))),
        ));
    }
    group.add(&property(
        &gettext("Signed"),
        &count(rows.iter().filter(|r| r.signed).count()),
    ));
    group.add(&property(
        &gettext("Key Issues"),
        &count(rows.iter().filter(|r| r.is_key).count()),
    ));
    group
}

fn cover_dates(rows: &[CopyRow]) -> Option<adw::PreferencesGroup> {
    let dates: Vec<_> = rows.iter().filter_map(|r| r.cover_date).collect();
    let key = |d: &mylongbox_core::CoverDate| (d.year(), d.month().unwrap_or(0));
    let oldest = dates.iter().min_by_key(|d| key(d))?;
    let newest = dates.iter().max_by_key(|d| key(d))?;
    let group = group(&gettext("Cover Dates"));
    group.add(&property(
        &gettext("Oldest"),
        &format::cover_date(Some(*oldest)),
    ));
    group.add(&property(
        &gettext("Newest"),
        &format::cover_date(Some(*newest)),
    ));
    let mut decades: BTreeMap<u16, usize> = BTreeMap::new();
    for d in &dates {
        *decades.entry(d.year() / 10 * 10).or_default() += 1;
    }
    for (decade, n) in decades {
        group.add(&property(
            // Translators: a decade, as in "1960s".
            &gettext_f("{decade}s", &[("decade", &decade.to_string())]),
            &books(n),
        ));
    }
    if dates.len() < rows.len() {
        let undated = rows.len() - dates.len();
        group.add(&property(&gettext("No Cover Date"), &books(undated)));
    }
    Some(group)
}
