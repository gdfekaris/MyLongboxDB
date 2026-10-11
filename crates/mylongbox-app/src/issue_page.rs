//! The detail page of one issue: what it is, who made it, what it
//! contains, and every copy the owner has of it.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use mylongbox_core::{
    Category, CopyDetail, EditionDetail, IssueDetail, IssueId, MagazineFormat, Role,
    format_collects,
};

use crate::books::version;
use crate::format;

/// Opens another issue's page, such as one a book reprints.
pub type OpenIssue = Rc<dyn Fn(IssueId)>;

pub fn page(d: &IssueDetail, open: OpenIssue) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    prefs.add(&summary(d));
    if !d.issue.fields.synopsis.is_empty() {
        let group = group("Synopsis");
        group.add(&paragraph(&d.issue.fields.synopsis));
        prefs.add(&group);
    }
    if let Some(group) = credits(d) {
        prefs.add(&group);
    }
    if let Some(group) = appearances(d) {
        prefs.add(&group);
    }
    if !d.collects.is_empty() {
        // The compact form first, since a book can reprint dozens of
        // issues; each can still be opened from the list below it.
        let group = group("Collects");
        let entries: Vec<_> = d.collects.iter().map(|c| c.entry.clone()).collect();
        group.add(&row(&format_collects(&entries), ""));
        let issues = expander(
            "Reprinted Issues",
            &match d.collects.len() {
                1 => "1 issue".to_owned(),
                n => format!("{n} issues"),
            },
        );
        for c in &d.collects {
            let title = format!(
                "{} #{}",
                series_title(&c.entry.series.name, c.entry.series.start_year),
                c.entry.number
            );
            issues.add_row(&link_row(&title, "", c.issue, &open));
        }
        group.add(&issues);
        prefs.add(&group);
    }
    if !d.collected_in.is_empty() {
        let group = group("Also Collected In");
        for c in &d.collected_in {
            let owned = if c.owned { "In your collection" } else { "" };
            group.add(&link_row(&c.label, owned, c.collection, &open));
        }
        prefs.add(&group);
    }
    let category = d.series.fields.category;
    for (n, edition) in d.editions.iter().enumerate() {
        prefs.add(&edition_group(edition, n, category));
    }

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&prefs));
    adw::NavigationPage::builder()
        .title(&d.label)
        .child(&toolbar)
        .build()
}

fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).build()
}

/// A row of user text, never read as markup. Markup is turned off
/// before any text is set, since the builder may set the text first.
fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row
}

/// A labelled fact: the label small, the value prominent.
fn property(label: &str, value: &str) -> adw::ActionRow {
    let row = row(label, value);
    row.add_css_class("property");
    row
}

fn expander(title: &str, subtitle: &str) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row
}

fn link_row(title: &str, subtitle: &str, issue: IssueId, open: &OpenIssue) -> adw::ActionRow {
    let row = row(title, subtitle);
    row.set_activatable(true);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let open = open.clone();
    row.connect_activated(move |_| open(issue));
    row
}

fn paragraph(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .css_classes(["body"])
        .build()
}

fn series_title(name: &str, start_year: Option<u16>) -> String {
    match start_year {
        Some(year) => format!("{name} ({year})"),
        None => name.to_owned(),
    }
}

/// The cover, the issue's name, and the facts about it at a glance.
fn summary(d: &IssueDetail) -> adw::PreferencesGroup {
    let f = &d.issue.fields;
    let s = &d.series.fields;
    let placeholder = gtk::Image::builder()
        .icon_name("image-x-generic-symbolic")
        .pixel_size(64)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["dim-label"])
        .build();
    let cover = gtk::Box::builder()
        .width_request(160)
        .height_request(242)
        .valign(gtk::Align::Start)
        .css_classes(["card"])
        .build();
    cover.append(&placeholder);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .build();
    let label = |text: &str, classes: &[&str]| {
        gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .css_classes(classes)
            .build()
    };
    text.append(&label(&d.label, &["title-1"]));
    if !f.title.is_empty() {
        text.append(&label(&f.title, &["title-3"]));
    }
    let kind = match s.category {
        Category::Comic => String::new(),
        Category::Magazine => format::joined([
            "Magazine",
            s.magazine_format
                .filter(|m| *m != MagazineFormat::Standard)
                .map(format::magazine_format)
                .unwrap_or_default(),
            match s.black_and_white {
                Some(true) => "Black and white",
                _ => "",
            },
        ]),
        Category::CollectedEdition => "Collected edition".to_owned(),
    };
    let volume = s.volume.map(|v| format!("Vol. {v}")).unwrap_or_default();
    let facts = [
        format::joined([d.publisher.clone().unwrap_or_default(), volume, kind]),
        format::joined([
            f.cover_date
                .map(|c| format!("Cover date {}", format::cover_date(Some(c))))
                .unwrap_or_default(),
            f.release_date
                .map(|r| format!("Released {}", format::date(r)))
                .unwrap_or_default(),
            f.page_count
                .map(|p| format!("{p} pages"))
                .unwrap_or_default(),
        ]),
        ownership(d),
    ];
    for fact in facts.iter().filter(|f| !f.is_empty()) {
        text.append(&label(fact, &["dim-label"]));
    }
    if f.is_key {
        let key = match f.key_reason.as_str() {
            "" => "Key issue".to_owned(),
            why => format!("Key issue: {why}"),
        };
        text.append(&label(&key, &["accent", "heading"]));
    }

    let top = gtk::Box::builder().spacing(24).build();
    top.append(&cover);
    top.append(&text);
    let group = adw::PreferencesGroup::new();
    group.add(&top);
    group
}

fn ownership(d: &IssueDetail) -> String {
    let o = d.ownership;
    let singles = match (o.single_copies, d.series.fields.category) {
        (0, Category::CollectedEdition) => "Not in your collection".to_owned(),
        (0, _) => "No single copies".to_owned(),
        (1, _) => "You own 1 copy".to_owned(),
        (n, _) => format!("You own {n} copies"),
    };
    let collected = match o.collections_owned {
        0 => String::new(),
        1 => "Reprinted in 1 book you own".to_owned(),
        n => format!("Reprinted in {n} books you own"),
    };
    format::joined([singles, collected])
}

/// One row per role, naming everyone credited in it.
fn credits(d: &IssueDetail) -> Option<adw::PreferencesGroup> {
    if d.credits.is_empty() {
        return None;
    }
    let group = group("Credits");
    for role in Role::ALL {
        let names: Vec<_> = d
            .credits
            .iter()
            .filter(|c| c.role == *role)
            .map(|c| c.name.as_str())
            .collect();
        if !names.is_empty() {
            group.add(&property(format::role(*role), &names.join(", ")));
        }
    }
    Some(group)
}

fn appearances(d: &IssueDetail) -> Option<adw::PreferencesGroup> {
    let first = |name: &str, first: bool| {
        if first {
            format!("{name} (first appearance)")
        } else {
            name.to_owned()
        }
    };
    let characters: Vec<_> = d
        .characters
        .iter()
        .map(|c| first(&c.name, c.first_appearance))
        .collect();
    let teams: Vec<_> = d
        .teams
        .iter()
        .map(|t| first(&t.name, t.first_appearance))
        .collect();
    let arcs: Vec<_> = d
        .story_arcs
        .iter()
        .map(|a| match a.part {
            Some(part) => format!("{} (part {part})", a.name),
            None => a.name.clone(),
        })
        .collect();
    let rows = [
        ("Characters", characters),
        ("Teams", teams),
        ("Story Arcs", arcs),
    ];
    if rows.iter().all(|(_, names)| names.is_empty()) {
        return None;
    }
    let group = group("In This Issue");
    for (label, names) in rows {
        if !names.is_empty() {
            group.add(&property(label, &names.join(", ")));
        }
    }
    Some(group)
}

/// An edition's own facts, then one expandable row per copy.
fn edition_group(e: &EditionDetail, index: usize, category: Category) -> adw::PreferencesGroup {
    let f = &e.edition.fields;
    let name = match version(&f.variant, f.printing) {
        v if v.is_empty() && index == 0 => "Standard Edition".to_owned(),
        v if v.is_empty() => "Edition".to_owned(),
        v => v,
    };
    let artists: Vec<_> = e.cover_artists.iter().map(|a| a.name.as_str()).collect();
    let flag = |value: Option<bool>, yes: &str| match value {
        Some(true) => yes.to_owned(),
        _ => String::new(),
    };
    let facts = format::joined([
        f.distribution
            .map(format::distribution)
            .unwrap_or_default()
            .to_owned(),
        f.binding
            .map(format::binding)
            .unwrap_or_default()
            .to_owned(),
        flag(f.has_dust_jacket, "Dust jacket"),
        flag(f.has_slipcase, "Slipcase"),
        f.cover_price
            .map(|p| format!("Cover price {p}"))
            .unwrap_or_default(),
    ]);

    let group = adw::PreferencesGroup::new();
    // A header row rather than the group title, so the name can be bold
    // while the variant, which the user typed, is escaped.
    let header = row("", &facts);
    header.set_use_markup(true);
    header.set_title(&format!("<b>{}</b>", glib::markup_escape_text(&name)));
    header.set_subtitle(&glib::markup_escape_text(&facts));
    group.add(&header);
    if !artists.is_empty() {
        group.add(&property("Cover Art", &artists.join(", ")));
    }
    if let Some(isbn) = &f.isbn {
        group.add(&property("ISBN", isbn.as_str()));
    }
    if let Some(barcode) = &f.barcode {
        group.add(&property("Barcode", barcode));
    }
    if e.copies.is_empty() {
        let none = row("No copies", "Listed in the catalog only");
        none.add_css_class("dim-label");
        group.add(&none);
    }
    for (n, copy) in e.copies.iter().enumerate() {
        group.add(&copy_row(copy, n + 1, category));
    }
    group
}

fn copy_row(c: &CopyDetail, number: usize, category: Category) -> adw::ExpanderRow {
    let f = &c.copy.fields;
    let condition = match category {
        Category::CollectedEdition => f
            .book_condition
            .map(format::condition)
            .unwrap_or("No condition noted")
            .to_owned(),
        Category::Comic | Category::Magazine => match format::grade(f.grade, f.grade_type) {
            g if g.is_empty() => "Ungraded".to_owned(),
            g => g,
        },
    };
    let location = match (&c.storage_box, f.box_position) {
        (Some(name), Some(position)) => format!("{name}, position {position}"),
        (Some(name), None) => name.clone(),
        (None, _) => String::new(),
    };
    let value = c
        .value
        .as_ref()
        .map(|v| format!("Worth {}", v.value))
        .unwrap_or_default();

    let row = expander(
        &format!("Copy {number} · {condition}"),
        &format::joined([location.as_str(), value.as_str()]),
    );
    let add = |label: &str, value: String| {
        if !value.is_empty() {
            row.add_row(&property(label, &value));
        }
    };
    add(
        "Grading",
        format::joined([
            f.cert_number
                .as_ref()
                .map(|n| format!("Certificate {n}"))
                .unwrap_or_default(),
            f.label_type.clone().unwrap_or_default(),
        ]),
    );
    add(
        "Dust Jacket",
        f.dust_jacket_condition
            .map(format::condition)
            .unwrap_or_default()
            .to_owned(),
    );
    add("Signed By", f.signed_by.clone());
    add("Location", location.clone());
    add(
        "Purchased",
        format::joined([
            f.purchase_price.map(|p| p.to_string()).unwrap_or_default(),
            f.purchase_date.map(format::date).unwrap_or_default(),
            f.seller.clone(),
        ]),
    );
    add(
        "Value",
        c.value
            .as_ref()
            .map(|v| {
                format::joined([
                    v.value.to_string(),
                    format!("as of {}", format::date(v.date)),
                    v.source.clone(),
                ])
            })
            .unwrap_or_default(),
    );
    add(
        "Tags",
        c.tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    add("Notes", f.notes.clone());
    add(
        "Added",
        c.copy
            .added_at
            .get(..10)
            .and_then(|d| d.parse().ok())
            .map(format::date)
            .unwrap_or_default(),
    );
    row
}
