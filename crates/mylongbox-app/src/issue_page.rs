//! The detail page of one issue: what it is, who made it, what it
//! contains, and every copy the owner has of it.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{
    Category, CopyDetail, CopyId, EditionDetail, EditionId, IssueDetail, IssueId, MagazineFormat,
    Prefill, Role, SeriesId, Side, ValueEntry, format_collects,
};

use crate::books::version;
use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};
use crate::{chart, covers, format};

/// What the page's rows do when chosen.
#[derive(Clone)]
pub struct Links {
    /// Opens another issue's page, such as one a book reprints.
    pub open: Rc<dyn Fn(IssueId)>,
    /// Shows the page again with another copy as the one being viewed.
    pub select: Rc<dyn Fn(CopyId)>,
    pub edit: Rc<dyn Fn(CopyId)>,
    /// Starts a new book from a copy.
    pub prefill: Rc<dyn Fn(CopyId, Prefill)>,
    pub trash_copy: Rc<dyn Fn(CopyId)>,
    pub trash_edition: Rc<dyn Fn(EditionId)>,
    pub trash_issue: Rc<dyn Fn(IssueId)>,
    pub add_value: Rc<dyn Fn(CopyId)>,
    /// Deletes a value of a copy; the value is passed whole so it can be
    /// put back.
    pub delete_value: Rc<dyn Fn(CopyId, ValueEntry)>,
    /// Asks for a photo of one side of a copy, new or replacing one.
    pub add_photo: Rc<dyn Fn(CopyId, Side)>,
    pub remove_photo: Rc<dyn Fn(CopyId, Side)>,
    /// Opens the editor on a photo already saved.
    pub edit_photo: Rc<dyn Fn(CopyId, Side)>,
    pub swap_photos: Rc<dyn Fn(CopyId)>,
    /// Opens the full-size view of a copy's photos at one side.
    pub view_photo: Rc<dyn Fn(CopyId, Side)>,
    /// Change the series' details, from the page of this copy if any.
    pub edit_series: Rc<dyn Fn(SeriesId, Option<CopyId>)>,
}

/// The copy the page is about: `open_copy` if the issue has it, else
/// its first copy, so there is always one copy to edit when any exist.
pub fn current_copy(d: &IssueDetail, open_copy: Option<CopyId>) -> Option<CopyId> {
    let mut copies = d.editions.iter().flat_map(|e| &e.copies).map(|c| c.copy.id);
    match open_copy {
        Some(open) if copies.clone().any(|c| c == open) => Some(open),
        _ => copies.next(),
    }
}

/// The page for `d` as seen from the copy `current`: what the issue's
/// copies share, then only that copy and its edition, and at the bottom
/// the issue's other copies in brief, each opening the page as seen
/// from it.
pub fn page(d: &IssueDetail, links: &Links, current: Option<CopyId>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    prefs.add(&summary(d, current, links));
    if !d.issue.fields.synopsis.is_empty() {
        let group = group(&gettext("Notes About This Issue"));
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
        // Translators: the heading of the issues a collected edition
        // reprints.
        let group = group(&gettext("Collects"));
        let entries: Vec<_> = d.collects.iter().map(|c| c.entry.clone()).collect();
        group.add(&row(&format_collects(&entries), ""));
        let count = d.collects.len();
        let issues = expander(
            &gettext("Reprinted Issues"),
            &ngettext_f(
                "{count} issue",
                "{count} issues",
                count,
                &[("count", &count.to_string())],
            ),
        );
        for c in &d.collects {
            let title = format!(
                "{} #{}",
                mylongbox_core::series_label(&c.entry.series.name, c.entry.series.start_year, None),
                c.entry.number
            );
            issues.add_row(&link_row(&title, "", c.issue, links));
        }
        group.add(&issues);
        prefs.add(&group);
    }
    if !d.collected_in.is_empty() {
        let group = group(&gettext("Also Collected In"));
        for c in &d.collected_in {
            let owned = if c.owned {
                gettext("In your collection")
            } else {
                String::new()
            };
            group.add(&link_row(&c.label, &owned, c.collection, links));
        }
        prefs.add(&group);
    }
    let category = d.series.fields.category;
    match current {
        // Only the copy being viewed and its edition; the others wait at
        // the bottom, so nothing about them is mistaken for this copy.
        Some(current) => {
            let mine = d.editions.iter().find_map(|e| {
                e.copies
                    .iter()
                    .find(|c| c.copy.id == current)
                    .map(|c| (e, c))
            });
            if let Some((edition, copy)) = mine {
                prefs.add(&this_copy(edition, copy, category, links));
            }
            if let Some(group) = other_copies(d, current, category, links) {
                prefs.add(&group);
            }
        }
        // An issue known only from the catalog, such as one a book
        // reprints, has editions but no copies to choose between.
        None => {
            for edition in &d.editions {
                prefs.add(&edition_group(edition, links));
            }
        }
    }

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    header.pack_end(&issue_menu(&toolbar, d, current, links));
    toolbar.add_top_bar(&header);
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

fn link_row(title: &str, subtitle: &str, issue: IssueId, links: &Links) -> adw::ActionRow {
    let row = row(title, subtitle);
    row.set_activatable(true);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let open = links.open.clone();
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

/// A photo that opens the full-size viewer when chosen, or the
/// placeholder naming the missing side.
fn photo_or_placeholder(
    copy: CopyId,
    side: Side,
    has_photo: bool,
    missing: &str,
    small: bool,
    links: &Links,
) -> gtk::Widget {
    let texture = has_photo
        .then(|| covers::thumbnail_of(copy, side))
        .flatten();
    let Some(texture) = texture else {
        return covers::placeholder(missing, small).upcast();
    };
    let button = gtk::Button::builder()
        .child(&covers::picture(&texture, gtk::ContentFit::Contain))
        .tooltip_text(gettext("View Full Size"))
        .hexpand(true)
        .vexpand(true)
        .css_classes(["flat", "cover-button"])
        .build();
    // The tooltip does not say which side; the name read out does.
    button.update_property(&[gtk::accessible::Property::Label(&match side {
        Side::Front => gettext("View the Front Cover Full Size"),
        Side::Back => gettext("View the Back Cover Full Size"),
        Side::Signature => gettext("View the Signature Photo Full Size"),
    })]);
    let hook = links.view_photo.clone();
    button.connect_clicked(move |_| hook(copy, side));
    button.upcast()
}

/// The cover of the copy being viewed, with a flip to the back. Other
/// copies' photos are never shown for it, since they may be of another
/// edition or a copy in a different state.
fn summary_cover(d: &IssueDetail, current: Option<CopyId>, links: &Links) -> gtk::Widget {
    let frame = gtk::Box::builder()
        .width_request(184)
        .height_request(278)
        // What is inside expands; the cover keeps its size.
        .hexpand(false)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["card"])
        .build();
    let missing = |side| match side {
        Side::Front => gettext("No Front Cover Image for This Copy"),
        Side::Back => gettext("No Back Cover Image for This Copy"),
        Side::Signature => gettext("No Signature Photo for This Copy"),
    };
    let found = d
        .editions
        .iter()
        .flat_map(|e| &e.copies)
        .find(|c| Some(c.copy.id) == current);
    let Some(c) = found else {
        frame.set_valign(gtk::Align::Start);
        frame.append(&covers::placeholder(&missing(Side::Front), false));
        return frame.upcast();
    };
    let (copy, sides, links) = (c.copy.id, c.photos.clone(), links.clone());
    let show = move |frame: &gtk::Box, side: Side| {
        while let Some(child) = frame.first_child() {
            frame.remove(&child);
        }
        frame.append(&photo_or_placeholder(
            copy,
            side,
            sides.contains(&side),
            &missing(side),
            false,
            &links,
        ));
    };
    show(&frame, Side::Front);
    let flip = gtk::Button::builder()
        .icon_name("object-flip-horizontal-symbolic")
        .tooltip_text(gettext("Show the Back Cover"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(8)
        .margin_bottom(8)
        .css_classes(["circular", "osd"])
        .build();
    let side = Cell::new(Side::Front);
    {
        let frame = frame.clone();
        flip.connect_clicked(move |b| {
            // The flip turns the book over; the signature has its own slot.
            let next = match side.get() {
                Side::Front => Side::Back,
                Side::Back | Side::Signature => Side::Front,
            };
            side.set(next);
            show(&frame, next);
            b.set_tooltip_text(Some(&match next {
                Side::Front | Side::Signature => gettext("Show the Back Cover"),
                Side::Back => gettext("Show the Front Cover"),
            }));
        });
    }
    let overlay = gtk::Overlay::builder()
        .child(&frame)
        .valign(gtk::Align::Start)
        .hexpand(false)
        .build();
    overlay.add_overlay(&flip);
    overlay.upcast()
}

/// The cover, the issue's name, and the facts about it at a glance.
fn summary(d: &IssueDetail, current: Option<CopyId>, links: &Links) -> adw::PreferencesGroup {
    let f = &d.issue.fields;
    let s = &d.series.fields;
    let cover = summary_cover(d, current, links);

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
            gettext("Magazine"),
            s.magazine_format
                .filter(|m| *m != MagazineFormat::Standard)
                .map(format::magazine_format)
                .unwrap_or_default()
                .to_owned(),
            match s.black_and_white {
                Some(true) => gettext("Black and white"),
                _ => String::new(),
            },
        ]),
        Category::CollectedEdition => gettext("Collected edition"),
    };
    let volume = s
        .volume
        .filter(|_| !d.volume_in_label)
        // Translators: a series' volume number, as in "Vol. 2".
        .map(|v| gettext_f("Vol. {number}", &[("number", &v.to_string())]))
        .unwrap_or_default();
    let legacy = f
        .legacy_number
        // Translators: the issue's number across a title's relaunches,
        // as in "Legacy #846".
        .map(|n| gettext_f("Legacy #{number}", &[("number", &n.to_string())]))
        .unwrap_or_default();
    let facts = [
        format::joined([
            d.publisher.clone().unwrap_or_default(),
            s.imprint.clone(),
            volume,
            legacy,
            kind,
        ]),
        format::joined([
            f.cover_date
                .map(|c| {
                    gettext_f(
                        "Cover date {date}",
                        &[("date", &format::cover_date(Some(c)))],
                    )
                })
                .unwrap_or_default(),
            f.release_date
                .map(|r| gettext_f("Released {date}", &[("date", &format::date(r))]))
                .unwrap_or_default(),
            f.page_count
                .map(|p| {
                    ngettext_f(
                        "{count} page",
                        "{count} pages",
                        usize::try_from(p).unwrap_or(usize::MAX),
                        &[("count", &p.to_string())],
                    )
                })
                .unwrap_or_default(),
        ]),
        ownership(d),
    ];
    for fact in facts.iter().filter(|f| !f.is_empty()) {
        text.append(&label(fact, &["dim-label"]));
    }
    if f.is_key {
        let key = match f.key_reason.as_str() {
            "" => gettext("Key issue"),
            // Translators: {reason} says why the issue is a key one, such
            // as "first appearance of Wolverine".
            why => gettext_f("Key issue: {reason}", &[("reason", why)]),
        };
        text.append(&label(&key, &["accent", "heading"]));
    }

    // The placeholder expands to fill the cover; that must not make the
    // whole summary take up the page.
    let top = gtk::Box::builder().spacing(24).vexpand(false).build();
    top.append(&cover);
    top.append(&text);
    let group = adw::PreferencesGroup::new();
    group.add(&top);
    group
}

fn ownership(d: &IssueDetail) -> String {
    let o = d.ownership;
    let singles = match (o.single_copies, d.series.fields.category) {
        (0, Category::CollectedEdition) => gettext("Not in your collection"),
        (0, _) => gettext("No single copies"),
        (n, _) => ngettext_f(
            "You own {count} copy",
            "You own {count} copies",
            usize::try_from(n).unwrap_or(usize::MAX),
            &[("count", &n.to_string())],
        ),
    };
    let collected = match o.collections_owned {
        0 => String::new(),
        n => ngettext_f(
            "Reprinted in {count} book you own",
            "Reprinted in {count} books you own",
            usize::try_from(n).unwrap_or(usize::MAX),
            &[("count", &n.to_string())],
        ),
    };
    format::joined([singles, collected])
}

/// One row per role, naming everyone credited in it.
fn credits(d: &IssueDetail) -> Option<adw::PreferencesGroup> {
    if d.credits.is_empty() {
        return None;
    }
    let group = group(&gettext("Credits"));
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
            gettext_f("{name} (first appearance)", &[("name", name)])
        } else {
            name.to_owned()
        }
    };
    // Those in every issue of the series join the issue's own, once each.
    let mut characters: Vec<_> = d
        .characters
        .iter()
        .map(|c| (c.name.as_str(), c.first_appearance))
        .collect();
    for c in &d.series_characters {
        if !d.characters.iter().any(|own| own.id == c.id) {
            characters.push((&c.name, false));
        }
    }
    let mut teams: Vec<_> = d
        .teams
        .iter()
        .map(|t| (t.name.as_str(), t.first_appearance))
        .collect();
    for t in &d.series_teams {
        if !d.teams.iter().any(|own| own.id == t.id) {
            teams.push((&t.name, false));
        }
    }
    let by_name = |a: &(&str, bool), b: &(&str, bool)| a.0.to_lowercase().cmp(&b.0.to_lowercase());
    characters.sort_by(by_name);
    teams.sort_by(by_name);
    let characters: Vec<_> = characters.into_iter().map(|(n, f)| first(n, f)).collect();
    let teams: Vec<_> = teams.into_iter().map(|(n, f)| first(n, f)).collect();
    let arcs: Vec<_> = d
        .story_arcs
        .iter()
        .map(|a| match a.part {
            Some(part) => gettext_f(
                "{arc} (part {part})",
                &[("arc", &a.name), ("part", &part.to_string())],
            ),
            None => a.name.clone(),
        })
        .collect();
    let rows = [
        (gettext_noop("Characters"), characters),
        (gettext_noop("Teams"), teams),
        (gettext_noop("Story Arcs"), arcs),
    ];
    if rows.iter().all(|(_, names)| names.is_empty()) {
        return None;
    }
    let group = group(&gettext("In This Issue"));
    for (label, names) in rows {
        if !names.is_empty() {
            group.add(&property(&gettext(label), &names.join(", ")));
        }
    }
    Some(group)
}

/// The edition's name and facts, with its cover art and codes. Trashing
/// an edition trashes its copies, so that is offered only where it
/// touches no copy but `current`, if any.
fn edition_rows(e: &EditionDetail, links: &Links, current: Option<CopyId>) -> Vec<adw::ActionRow> {
    let f = &e.edition.fields;
    let name = edition_name(e);
    let artists: Vec<_> = e.cover_artists.iter().map(|a| a.name.as_str()).collect();
    let flag = |value: Option<bool>, yes: String| match value {
        Some(true) => yes,
        _ => String::new(),
    };
    let facts = format::joined([
        f.published
            .map(|d| gettext_f("Published {date}", &[("date", &format::partial_date(d))]))
            .unwrap_or_default(),
        f.binding
            .map(format::binding)
            .unwrap_or_default()
            .to_owned(),
        flag(f.has_dust_jacket, gettext("Dust jacket")),
        flag(f.has_slipcase, gettext("Slipcase")),
        f.cover_price
            .map(|p| {
                gettext_f(
                    "Cover price {amount}",
                    &[("amount", &format::money(p, None))],
                )
            })
            .unwrap_or_default(),
        f.canadian_cover_price_cents
            .map(|c| {
                // Translators: CA$ marks Canadian dollars.
                gettext_f(
                    "Canadian price CA${amount}",
                    &[("amount", &mylongbox_core::cents_string(c))],
                )
            })
            .unwrap_or_default(),
    ]);

    let mut rows = Vec::new();
    // A header row rather than the group title, so the name can be bold
    // while the variant, which the user typed, is escaped.
    let header = row("", &facts);
    header.set_use_markup(true);
    header.set_title(&format!("<b>{}</b>", glib::markup_escape_text(&name)));
    header.set_subtitle(&glib::markup_escape_text(&facts));
    let only_this = e.copies.iter().all(|c| Some(c.copy.id) == current);
    if only_this {
        let trash = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Move Edition to Trash"))
            .css_classes(["flat"])
            .build();
        let hook = links.trash_edition.clone();
        let edition = e.edition.id;
        trash.connect_clicked(move |_| hook(edition));
        header.add_suffix(&trash);
    }
    rows.push(header);
    if !artists.is_empty() {
        rows.push(property(&gettext("Cover Art"), &artists.join(", ")));
    }
    if let Some(isbn) = &f.isbn {
        rows.push(property(&gettext("ISBN"), isbn.as_str()));
    }
    if let Some(barcode) = &f.barcode {
        rows.push(property(&gettext("Barcode"), barcode));
    }
    rows
}

/// An edition of an issue known only from the catalog, which has no
/// copies to show.
fn edition_group(e: &EditionDetail, links: &Links) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    for row in edition_rows(e, links, None) {
        group.add(&row);
    }
    if e.copies.is_empty() {
        let none = row(
            &gettext("No copies"),
            &gettext("Listed in the catalog only"),
        );
        none.add_css_class("dim-label");
        group.add(&none);
    }
    group
}

/// The copy being viewed: what it is, its edition, its photos,
/// everything about it, and the ways to change it.
fn this_copy(
    e: &EditionDetail,
    c: &CopyDetail,
    category: Category,
    links: &Links,
) -> adw::PreferencesGroup {
    let group = group(&gettext("This Copy"));
    let copy = c.copy.id;
    let edit = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Edit This Copy"))
        .css_classes(["flat"])
        .build();
    let hook = links.edit.clone();
    edit.connect_clicked(move |_| hook(copy));
    let buttons = gtk::Box::builder().valign(gtk::Align::Center).build();
    buttons.append(&edit);
    buttons.append(&more_menu(&group, copy, category, links));
    group.set_header_suffix(Some(&buttons));
    group.add(&row(&copy_summary(c, category), ""));
    for row in edition_rows(e, links, Some(copy)) {
        group.add(&row);
    }
    group.add(&photo_row(c, links));
    for row in copy_details(c, links) {
        group.add(&row);
    }
    group
}

/// What sets an edition apart: its cover (`Cover B`, or `Standard
/// Cover` when it has no variant), its printing after the first, and how
/// it was sold, as in `Standard Cover · Direct`.
fn edition_name(e: &EditionDetail) -> String {
    let f = &e.edition.fields;
    let cover = match f.variant.as_str() {
        "" => gettext("Standard Cover"),
        variant => variant.to_owned(),
    };
    format::joined([
        version(&cover, f.printing),
        f.distribution
            .map(format::distribution)
            .unwrap_or_default()
            .to_owned(),
    ])
}

/// Every copy of the issue but `current`, in brief, each opening the
/// page as seen from it.
fn other_copies(
    d: &IssueDetail,
    current: CopyId,
    category: Category,
    links: &Links,
) -> Option<adw::PreferencesGroup> {
    let group = group(&gettext("My Other Copies"));
    let mut any = false;
    for e in &d.editions {
        for copy in &e.copies {
            if copy.copy.id != current {
                group.add(&other_copy_row(copy, &edition_name(e), category, links));
                any = true;
            }
        }
    }
    any.then_some(group)
}

/// A copy's condition, and whether it is signed, in one line.
fn copy_summary(c: &CopyDetail, category: Category) -> String {
    let f = &c.copy.fields;
    let condition = match category {
        Category::CollectedEdition => f
            .book_condition
            .map(format::condition)
            .map_or_else(|| gettext("No condition noted"), str::to_owned),
        Category::Comic | Category::Magazine => match format::grade(f.grade, f.grade_type) {
            g if g.is_empty() => gettext("Ungraded"),
            g => g,
        },
    };
    format::joined([
        condition,
        match c.signatures.len() {
            0 => String::new(),
            1 => gettext("Signed"),
            n => ngettext_f(
                "{count} signature",
                "{count} signatures",
                n,
                &[("count", &n.to_string())],
            ),
        },
    ])
}

/// Where a copy is kept, if that was noted.
fn location(c: &CopyDetail) -> String {
    match (&c.storage_box, c.copy.fields.box_position) {
        (Some(name), Some(position)) => gettext_f(
            "{box}, position {position}",
            &[("box", name), ("position", &position.to_string())],
        ),
        (Some(name), None) => name.clone(),
        (None, _) => String::new(),
    }
}

/// Another copy of the issue, in brief; choosing it views the page from
/// that copy.
fn other_copy_row(
    c: &CopyDetail,
    edition: &str,
    category: Category,
    links: &Links,
) -> adw::ActionRow {
    let title = format::joined([edition.to_owned(), copy_summary(c, category)]);
    let row = row(&title, &location(c));
    row.set_activatable(true);
    row.set_tooltip_text(Some(&gettext("View This Copy")));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let select = links.select.clone();
    let copy = c.copy.id;
    row.connect_activated(move |_| select(copy));
    row
}

/// Everything recorded about a copy, as rows, ending with its value
/// history.
fn copy_details(c: &CopyDetail, links: &Links) -> Vec<gtk::Widget> {
    let f = &c.copy.fields;
    let mut rows: Vec<gtk::Widget> = Vec::new();
    let mut add = |label: &str, value: String| {
        if !value.is_empty() {
            rows.push(property(label, &value).upcast());
        }
    };
    add(
        &gettext("Grading"),
        format::joined([
            f.cert_number
                .as_ref()
                .map(|n| gettext_f("Certificate {number}", &[("number", n)]))
                .unwrap_or_default(),
            f.label_type.clone().unwrap_or_default(),
        ]),
    );
    add(&gettext("Pedigree"), f.pedigree.clone());
    add(
        &gettext("Page Quality"),
        f.page_quality
            .map(format::page_quality)
            .unwrap_or_default()
            .to_owned(),
    );
    add(
        &gettext("Dust Jacket"),
        f.dust_jacket_condition
            .map(format::condition)
            .unwrap_or_default()
            .to_owned(),
    );
    for s in &c.signatures {
        let sf = &s.fields;
        add(
            format::verification(sf.verification),
            format::joined([
                gettext_f("Signed by {name}", &[("name", &s.name)]),
                sf.placement
                    .map(format::signature_placement)
                    .unwrap_or_default()
                    .to_owned(),
                if sf.sketch {
                    gettext("with a sketch")
                } else {
                    String::new()
                },
                sf.place.clone(),
                sf.signed_on.map(format::date).unwrap_or_default(),
                sf.cert_number
                    .as_ref()
                    .map(|n| gettext_f("Certificate {number}", &[("number", n)]))
                    .unwrap_or_default(),
            ]),
        );
    }
    add(&gettext("Location"), location(c));
    add(
        &gettext("Purchased"),
        format::joined([
            f.purchase_price
                .map(|p| format::money(p, f.purchase_date))
                .unwrap_or_default(),
            f.purchase_date.map(format::date).unwrap_or_default(),
            f.seller.clone(),
        ]),
    );
    add(
        &gettext("Tags"),
        c.tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    add(&gettext("Notes on This Copy"), f.notes.clone());
    add(
        // Translators: when the copy was added to the collection.
        &gettext("Added"),
        format::local_date(&c.copy.added_at)
            .map(format::date)
            .unwrap_or_default(),
    );
    rows.extend(value_history(c, links));
    rows
}

/// The copy's front and back photos and the photo of its signature,
/// each with ways to add, replace, or remove it.
fn photo_row(c: &CopyDetail, links: &Links) -> gtk::ListBoxRow {
    let copy = c.copy.id;
    let slots = gtk::Box::builder()
        .spacing(18)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    for side in [Side::Front, Side::Back] {
        slots.append(&photo_slot(copy, side, c.photos.contains(&side), links));
    }
    let covers = c
        .photos
        .iter()
        .any(|s| matches!(s, Side::Front | Side::Back));
    if covers {
        let swap = gtk::Button::builder()
            .icon_name("object-flip-horizontal-symbolic")
            .tooltip_text(gettext("Swap Front and Back"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let hook = links.swap_photos.clone();
        swap.connect_clicked(move |_| hook(copy));
        slots.append(&swap);
    }
    slots.append(&photo_slot(
        copy,
        Side::Signature,
        c.photos.contains(&Side::Signature),
        links,
    ));
    gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .child(&slots)
        .build()
}

fn photo_slot(copy: CopyId, side: Side, has_photo: bool, links: &Links) -> gtk::Box {
    // Whole sentences for each side, since languages differ in how a
    // side's name fits into them.
    let (title, missing, replace_tip, edit_tip, remove_tip, add_tip) = match side {
        Side::Front => (
            gettext("Front Cover"),
            gettext("No Front Cover Image"),
            gettext("Replace the Front Cover Photo"),
            gettext("Edit the Front Cover Photo"),
            gettext("Remove the Front Cover Photo"),
            gettext("Add a Front Cover Photo"),
        ),
        Side::Back => (
            gettext("Back Cover"),
            gettext("No Back Cover Image"),
            gettext("Replace the Back Cover Photo"),
            gettext("Edit the Back Cover Photo"),
            gettext("Remove the Back Cover Photo"),
            gettext("Add a Back Cover Photo"),
        ),
        Side::Signature => (
            gettext("Signature"),
            gettext("No Signature Photo"),
            gettext("Replace the Signature Photo"),
            gettext("Edit the Signature Photo"),
            gettext("Remove the Signature Photo"),
            gettext("Add a Photo of the Signature"),
        ),
    };
    let frame = gtk::Box::builder()
        .width_request(120)
        .height_request(182)
        // What is inside expands; the frame keeps its size.
        .hexpand(false)
        .vexpand(false)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["card"])
        .build();
    frame.append(&photo_or_placeholder(
        copy, side, has_photo, &missing, true, links,
    ));
    let caption = gtk::Label::builder()
        .label(&title)
        .css_classes(["caption-heading"])
        .build();
    let buttons = gtk::Box::builder()
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    let button = |label: &str, tip: &str| {
        gtk::Button::builder()
            .label(label)
            .use_underline(true)
            .tooltip_text(tip)
            .css_classes(["flat"])
            .build()
    };
    if has_photo {
        let replace = button(&gettext("_Replace\u{2026}"), &replace_tip);
        let hook = links.add_photo.clone();
        replace.connect_clicked(move |_| hook(copy, side));
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(&remove_tip)
            .css_classes(["flat"])
            .build();
        let hook = links.remove_photo.clone();
        remove.connect_clicked(move |_| hook(copy, side));
        let edit = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text(&edit_tip)
            .css_classes(["flat"])
            .build();
        let hook = links.edit_photo.clone();
        edit.connect_clicked(move |_| hook(copy, side));
        buttons.append(&edit);
        buttons.append(&replace);
        buttons.append(&remove);
    } else {
        let add = button(&gettext("_Add Photo\u{2026}"), &add_tip);
        let hook = links.add_photo.clone();
        add.connect_clicked(move |_| hook(copy, side));
        buttons.append(&add);
    }
    let slot = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    slot.append(&frame);
    slot.append(&caption);
    slot.append(&buttons);
    slot
}

/// The copy's values over time: a small chart once there are two, each
/// value with a way to delete it, and a way to add one.
fn value_history(c: &CopyDetail, links: &Links) -> Vec<gtk::Widget> {
    let mut rows: Vec<gtk::Widget> = Vec::new();
    if c.values.len() >= 2 {
        let chart = chart::value_chart(&c.values);
        rows.push(
            gtk::ListBoxRow::builder()
                .child(&chart)
                .activatable(false)
                .selectable(false)
                .build()
                .upcast(),
        );
    }
    for v in c.values.iter().rev() {
        let entry = plain_row(
            &format::money(v.value, Some(v.date)),
            &format::joined([
                gettext_f("Value as of {date}", &[("date", &format::date(v.date))]),
                v.source.clone(),
            ]),
        );
        let delete = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Delete This Value"))
            .css_classes(["flat"])
            .build();
        let hook = links.delete_value.clone();
        let (copy, entry_data) = (c.copy.id, v.clone());
        delete.connect_clicked(move |_| hook(copy, entry_data.clone()));
        entry.add_suffix(&delete);
        rows.push(entry.upcast());
    }
    let add = adw::ButtonRow::builder()
        .title(gettext("Add Value"))
        .start_icon_name("list-add-symbolic")
        .build();
    let hook = links.add_value.clone();
    let copy = c.copy.id;
    add.connect_activated(move |_| hook(copy));
    rows.push(add.upcast());
    rows
}

fn plain_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row
}

/// The page's own menu, for the issue as a whole. Its actions belong to
/// `page`, which holds both the header and the content, since a menu
/// finds actions only on the widgets around it.
fn issue_menu(
    page: &adw::ToolbarView,
    d: &IssueDetail,
    current: Option<CopyId>,
    links: &Links,
) -> gtk::MenuButton {
    let (issue, series) = (d.issue.id, d.series.id);
    let actions = gio::SimpleActionGroup::new();
    let edit_series = gio::SimpleAction::new("edit-series", None);
    {
        let hook = links.edit_series.clone();
        edit_series.connect_activate(move |_, _| hook(series, current));
    }
    actions.add_action(&edit_series);
    let trash = gio::SimpleAction::new("trash", None);
    {
        let hook = links.trash_issue.clone();
        trash.connect_activate(move |_, _| hook(issue));
    }
    actions.add_action(&trash);
    page.insert_action_group("issue", Some(&actions));
    let menu = gio::Menu::new();
    menu.append(Some(&gettext("_Edit Series…")), Some("issue.edit-series"));
    menu.append(Some(&gettext("Move Issue to _Trash")), Some("issue.trash"));
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("Issue Menu"))
        .menu_model(&menu)
        .build()
}

/// The ways to start a new book from this copy, as a menu of actions
/// that belong to the copy's row.
fn more_menu(
    row: &impl IsA<gtk::Widget>,
    copy: CopyId,
    category: Category,
    links: &Links,
) -> gtk::MenuButton {
    let next = match category {
        Category::CollectedEdition => gettext("Add _Next Volume"),
        Category::Comic | Category::Magazine => gettext("Add _Next Issue"),
    };
    let actions = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    for (name, label, kind) in [
        (
            "another",
            gettext("Add Another _Copy"),
            Prefill::AnotherCopy,
        ),
        // Translators: a cover variant or another printing of the issue.
        ("variant", gettext("Add _Variant"), Prefill::Variant),
        ("next", next, Prefill::NextIssue),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let hook = links.prefill.clone();
        action.connect_activate(move |_, _| hook(copy, kind));
        actions.add_action(&action);
        menu.append(Some(&label), Some(&format!("copy.{name}")));
    }
    let trash = gio::SimpleAction::new("trash", None);
    {
        let hook = links.trash_copy.clone();
        trash.connect_activate(move |_, _| hook(copy));
    }
    actions.add_action(&trash);
    let last = gio::Menu::new();
    last.append(Some(&gettext("Move to _Trash")), Some("copy.trash"));
    menu.append_section(None, &last);
    row.insert_action_group("copy", Some(&actions));
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("More Ways to Add"))
        .css_classes(["flat"])
        .menu_model(&menu)
        .build()
}
