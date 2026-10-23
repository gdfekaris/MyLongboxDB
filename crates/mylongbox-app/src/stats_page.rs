//! Collection Stats: how big the collection is, what it is worth, and
//! how its books break down by condition and age.

use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use mylongbox_core::{Category, CopyId, CopyRow, GradeType, IssueId};

use crate::about;
use crate::format;
use crate::i18n::{gettext, gettext_f, ngettext_f};
use crate::style::{self, Style};

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
    if style::current() == Style::Pulp {
        page.add(&observatory(rows));
    }
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

/// Deco Pulp's instrument panel, after Griffith Observatory: the
/// collection's size and worth in big figures, a dial of how much of it
/// is graded, and a sunburst of its decades. The groups below give the
/// same figures in words, so the drawings are passed over by screen
/// readers.
fn observatory(rows: &[CopyRow]) -> adw::PreferencesGroup {
    let labelled = |top: &gtk::Widget, label: &str| {
        let figure = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        figure.append(top);
        figure.append(
            &gtk::Label::builder()
                .label(label)
                .css_classes(["observatory-label"])
                .build(),
        );
        figure
    };
    let figure = |value: &str, label: &str| {
        let value = gtk::Label::builder()
            .label(value)
            .css_classes(["observatory-figure"])
            .build();
        labelled(value.upcast_ref(), label)
    };
    let worth = format::money_total(
        rows.iter()
            .filter_map(|r| r.value.map(|v| (v, r.value_date))),
        true,
    )
    .unwrap_or_else(|| "\u{2014}".to_owned());
    let gradable: Vec<_> = rows
        .iter()
        .filter(|r| r.category != Category::CollectedEdition)
        .collect();
    let graded = gradable
        .iter()
        .filter(|r| r.grade_type.is_some_and(|t| t != GradeType::Raw))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let share = if gradable.is_empty() {
        0.0
    } else {
        graded as f64 / gradable.len() as f64
    };
    let mut decades: BTreeMap<u16, usize> = BTreeMap::new();
    for d in rows.iter().filter_map(|r| r.cover_date) {
        *decades.entry(d.year() / 10 * 10).or_default() += 1;
    }

    let panel = gtk::Box::builder()
        .spacing(12)
        .homogeneous(true)
        .css_classes(["observatory"])
        .build();
    panel.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Collection at a glance",
    ))]);
    panel.append(&figure(&count(rows.len()), &gettext("Books")));
    panel.append(&figure(&worth, &gettext("Worth Now")));
    panel.append(&labelled(
        dial(share).upcast_ref(),
        // Translators: the share of comics graded and sealed in a
        // plastic case by a grading company.
        &gettext("Slabbed"),
    ));
    panel.append(&labelled(
        sunburst(decades.into_values().collect()).upcast_ref(),
        &gettext("Cover Dates"),
    ));
    let group = adw::PreferencesGroup::new();
    group.add(&panel);
    group
}

/// The observatory's palette, which Deco Pulp's stylesheet shares.
const GOLD: (f64, f64, f64) = (0.788, 0.635, 0.153);
const TEAL: (f64, f64, f64) = (0.122, 0.541, 0.541);

fn instrument_area() -> gtk::DrawingArea {
    gtk::DrawingArea::builder()
        .content_width(120)
        .content_height(72)
        .halign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
}

/// A half dial with gold ticks, its needle at `share` of the way round.
fn dial(share: f64) -> gtk::DrawingArea {
    use std::f64::consts::PI;
    let area = instrument_area();
    area.set_draw_func(move |area, cr, width, height| {
        let ink = area.color();
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) - 6.0);
        let r = (cx - 4.0).min(cy - 2.0);
        cr.set_line_width(6.0);
        cr.set_source_rgba(TEAL.0, TEAL.1, TEAL.2, 0.25);
        cr.arc(cx, cy, r - 3.0, PI, 2.0 * PI);
        // Drawing errors only leave the dial blank.
        let _ = cr.stroke();
        cr.set_source_rgb(TEAL.0, TEAL.1, TEAL.2);
        cr.arc(cx, cy, r - 3.0, PI, PI + PI * share);
        let _ = cr.stroke();
        cr.set_source_rgb(GOLD.0, GOLD.1, GOLD.2);
        for tick in 0..=10 {
            let a = PI + PI * f64::from(tick) / 10.0;
            let inner = if tick % 5 == 0 { r - 16.0 } else { r - 11.0 };
            cr.set_line_width(if tick % 5 == 0 { 2.0 } else { 1.0 });
            cr.move_to(cx + inner * a.cos(), cy + inner * a.sin());
            cr.line_to(cx + (r - 7.0) * a.cos(), cy + (r - 7.0) * a.sin());
            let _ = cr.stroke();
        }
        let a = PI + PI * share;
        cr.set_source_rgba(
            f64::from(ink.red()),
            f64::from(ink.green()),
            f64::from(ink.blue()),
            1.0,
        );
        cr.set_line_width(2.5);
        cr.move_to(cx, cy);
        cr.line_to(cx + (r - 12.0) * a.cos(), cy + (r - 12.0) * a.sin());
        let _ = cr.stroke();
        cr.set_source_rgb(GOLD.0, GOLD.1, GOLD.2);
        cr.arc(cx, cy, 4.0, 0.0, 2.0 * PI);
        let _ = cr.fill();
    });
    area
}

/// Rays fanning up from the horizon, one per decade from the oldest,
/// each as long as its share of the books.
fn sunburst(counts: Vec<usize>) -> gtk::DrawingArea {
    use std::f64::consts::PI;
    let area = instrument_area();
    area.set_draw_func(move |area, cr, width, height| {
        let ink = area.color();
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) - 6.0);
        let r = (cx - 4.0).min(cy - 2.0);
        let most = counts.iter().copied().max().unwrap_or(0);
        if most == 0 {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let (n, most) = (counts.len() as f64, most as f64);
        let step = PI / n;
        for (at, &c) in counts.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let (i, c) = (at as f64, c as f64);
            let length = 8.0 + (r - 8.0) * c / most;
            let a = PI + step * i + step * 0.12;
            let b = PI + step * (i + 1.0) - step * 0.12;
            cr.move_to(cx, cy);
            cr.arc(cx, cy, length, a, b);
            cr.close_path();
            if at.is_multiple_of(2) {
                cr.set_source_rgb(GOLD.0, GOLD.1, GOLD.2);
            } else {
                cr.set_source_rgb(TEAL.0, TEAL.1, TEAL.2);
            }
            // Drawing errors only leave the rays blank.
            let _ = cr.fill();
        }
        cr.set_source_rgba(
            f64::from(ink.red()),
            f64::from(ink.green()),
            f64::from(ink.blue()),
            1.0,
        );
        cr.set_line_width(1.5);
        cr.move_to(cx - r, cy);
        cr.line_to(cx + r, cy);
        let _ = cr.stroke();
    });
    area
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
