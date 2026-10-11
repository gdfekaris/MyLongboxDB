//! The detail page of one issue: what it is, who made it, what it
//! contains, and every copy the owner has of it.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{
    Category, CopyDetail, CopyId, EditionDetail, EditionId, IssueDetail, IssueId, MagazineFormat,
    Prefill, Role, Side, ValueEntry, format_collects,
};

use crate::books::version;
use crate::{chart, covers, format};

/// What the page's rows do when chosen.
#[derive(Clone)]
pub struct Links {
    /// Opens another issue's page, such as one a book reprints.
    pub open: Rc<dyn Fn(IssueId)>,
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
    pub swap_photos: Rc<dyn Fn(CopyId)>,
    /// Opens the full-size view of a copy's photos at one side.
    pub view_photo: Rc<dyn Fn(CopyId, Side)>,
}

/// The page for `d`, with the row of `open_copy` unfolded, as after a
/// change to that copy.
pub fn page(d: &IssueDetail, links: &Links, open_copy: Option<CopyId>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    prefs.add(&summary(d, open_copy, links));
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
            issues.add_row(&link_row(&title, "", c.issue, links));
        }
        group.add(&issues);
        prefs.add(&group);
    }
    if !d.collected_in.is_empty() {
        let group = group("Also Collected In");
        for c in &d.collected_in {
            let owned = if c.owned { "In your collection" } else { "" };
            group.add(&link_row(&c.label, owned, c.collection, links));
        }
        prefs.add(&group);
    }
    let category = d.series.fields.category;
    for (n, edition) in d.editions.iter().enumerate() {
        prefs.add(&edition_group(edition, n, category, links, open_copy));
    }

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    header.pack_end(&issue_menu(&toolbar, d.issue.id, links));
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

fn series_title(name: &str, start_year: Option<u16>) -> String {
    match start_year {
        Some(year) => format!("{name} ({year})"),
        None => name.to_owned(),
    }
}

/// The copy whose photos stand for the issue: the one asked for if it
/// has any, else the first with a front, else the first with any, else
/// simply the first.
fn cover_copy(d: &IssueDetail, open_copy: Option<CopyId>) -> Option<&CopyDetail> {
    let copies = || d.editions.iter().flat_map(|e| &e.copies);
    copies()
        .find(|c| Some(c.copy.id) == open_copy && !c.photos.is_empty())
        .or_else(|| copies().find(|c| c.photos.contains(&Side::Front)))
        .or_else(|| copies().find(|c| !c.photos.is_empty()))
        .or_else(|| copies().next())
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
        .tooltip_text("View Full Size")
        .hexpand(true)
        .vexpand(true)
        .css_classes(["flat", "cover-button"])
        .build();
    let hook = links.view_photo.clone();
    button.connect_clicked(move |_| hook(copy, side));
    button.upcast()
}

/// The issue's cover, with a flip to the back.
fn summary_cover(d: &IssueDetail, open_copy: Option<CopyId>, links: &Links) -> gtk::Widget {
    let frame = gtk::Box::builder()
        .width_request(160)
        .height_request(242)
        // What is inside expands; the cover keeps its size.
        .hexpand(false)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["card"])
        .build();
    let missing = |side| match side {
        Side::Front => "No Front Cover Image for This Issue",
        Side::Back => "No Back Cover Image for This Issue",
    };
    let Some(c) = cover_copy(d, open_copy) else {
        frame.set_valign(gtk::Align::Start);
        frame.append(&covers::placeholder(missing(Side::Front), false));
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
            missing(side),
            false,
            &links,
        ));
    };
    show(&frame, Side::Front);
    let flip = gtk::Button::builder()
        .icon_name("object-flip-horizontal-symbolic")
        .tooltip_text("Show the Back Cover")
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
            let next = match side.get() {
                Side::Front => Side::Back,
                Side::Back => Side::Front,
            };
            side.set(next);
            show(&frame, next);
            b.set_tooltip_text(Some(match next {
                Side::Front => "Show the Back Cover",
                Side::Back => "Show the Front Cover",
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
fn summary(d: &IssueDetail, open_copy: Option<CopyId>, links: &Links) -> adw::PreferencesGroup {
    let f = &d.issue.fields;
    let s = &d.series.fields;
    let cover = summary_cover(d, open_copy, links);

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
fn edition_group(
    e: &EditionDetail,
    index: usize,
    category: Category,
    links: &Links,
    open_copy: Option<CopyId>,
) -> adw::PreferencesGroup {
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
            .map(|p| format!("Cover price {}", format::money(p, None)))
            .unwrap_or_default(),
    ]);

    let group = adw::PreferencesGroup::new();
    // A header row rather than the group title, so the name can be bold
    // while the variant, which the user typed, is escaped.
    let header = row("", &facts);
    header.set_use_markup(true);
    header.set_title(&format!("<b>{}</b>", glib::markup_escape_text(&name)));
    header.set_subtitle(&glib::markup_escape_text(&facts));
    let trash = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Move Edition to Trash")
        .css_classes(["flat"])
        .build();
    {
        let hook = links.trash_edition.clone();
        let edition = e.edition.id;
        trash.connect_clicked(move |_| hook(edition));
    }
    header.add_suffix(&trash);
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
        let row = copy_row(copy, n + 1, category, links);
        row.set_expanded(open_copy == Some(copy.copy.id));
        group.add(&row);
    }
    group
}

fn copy_row(c: &CopyDetail, number: usize, category: Category, links: &Links) -> adw::ExpanderRow {
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
        .values
        .last()
        .map(|v| format!("Worth {}", format::money(v.value, None)))
        .unwrap_or_default();

    let row = expander(
        &format::joined([
            format!("Copy {number}"),
            condition,
            match c.signatures.len() {
                0 => String::new(),
                1 => "Signed".to_owned(),
                n => format!("{n} signatures"),
            },
        ]),
        &format::joined([location.as_str(), value.as_str()]),
    );
    let edit = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Edit This Copy")
        .css_classes(["flat"])
        .build();
    let hook = links.edit.clone();
    let copy = c.copy.id;
    edit.connect_clicked(move |_| hook(copy));
    row.add_suffix(&edit);
    row.add_suffix(&more_menu(&row, copy, category, links));
    row.add_row(&photo_row(c, links));
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
    for s in &c.signatures {
        let sf = &s.fields;
        add(
            format::verification(sf.verification),
            format::joined([
                format!("Signed by {}", s.name),
                if sf.sketch {
                    "with a sketch".to_owned()
                } else {
                    String::new()
                },
                sf.place.clone(),
                sf.signed_on.map(format::date).unwrap_or_default(),
                sf.cert_number
                    .as_ref()
                    .map(|n| format!("Certificate {n}"))
                    .unwrap_or_default(),
            ]),
        );
    }
    add("Location", location.clone());
    add(
        "Purchased",
        format::joined([
            f.purchase_price
                .map(|p| format::money(p, f.purchase_date))
                .unwrap_or_default(),
            f.purchase_date.map(format::date).unwrap_or_default(),
            f.seller.clone(),
        ]),
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
        format::local_date(&c.copy.added_at)
            .map(format::date)
            .unwrap_or_default(),
    );
    value_history(&row, c, links);
    row
}

/// The copy's front and back photos, each with ways to add, replace, or
/// remove it.
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
    if !c.photos.is_empty() {
        let swap = gtk::Button::builder()
            .icon_name("object-flip-horizontal-symbolic")
            .tooltip_text("Swap Front and Back")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let hook = links.swap_photos.clone();
        swap.connect_clicked(move |_| hook(copy));
        slots.append(&swap);
    }
    gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .child(&slots)
        .build()
}

fn photo_slot(copy: CopyId, side: Side, has_photo: bool, links: &Links) -> gtk::Box {
    let (title, missing) = match side {
        Side::Front => ("Front Cover", "No Front Cover Image"),
        Side::Back => ("Back Cover", "No Back Cover Image"),
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
        copy, side, has_photo, missing, true, links,
    ));
    let caption = gtk::Label::builder()
        .label(title)
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
        let replace = button("_Replace\u{2026}", &format!("Replace the {title} Photo"));
        let hook = links.add_photo.clone();
        replace.connect_clicked(move |_| hook(copy, side));
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(format!("Remove the {title} Photo"))
            .css_classes(["flat"])
            .build();
        let hook = links.remove_photo.clone();
        remove.connect_clicked(move |_| hook(copy, side));
        buttons.append(&replace);
        buttons.append(&remove);
    } else {
        let add = button("_Add Photo\u{2026}", &format!("Add a {title} Photo"));
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
fn value_history(row: &adw::ExpanderRow, c: &CopyDetail, links: &Links) {
    if c.values.len() >= 2 {
        let chart = chart::value_chart(&c.values);
        row.add_row(
            &gtk::ListBoxRow::builder()
                .child(&chart)
                .activatable(false)
                .selectable(false)
                .build(),
        );
    }
    for v in c.values.iter().rev() {
        let entry = plain_row(
            &format::money(v.value, Some(v.date)),
            &format::joined([
                format!("Value as of {}", format::date(v.date)),
                v.source.clone(),
            ]),
        );
        let delete = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Delete This Value")
            .css_classes(["flat"])
            .build();
        let hook = links.delete_value.clone();
        let (copy, entry_data) = (c.copy.id, v.clone());
        delete.connect_clicked(move |_| hook(copy, entry_data.clone()));
        entry.add_suffix(&delete);
        row.add_row(&entry);
    }
    let add = adw::ButtonRow::builder()
        .title("Add Value")
        .start_icon_name("list-add-symbolic")
        .build();
    let hook = links.add_value.clone();
    let copy = c.copy.id;
    add.connect_activated(move |_| hook(copy));
    row.add_row(&add);
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
fn issue_menu(page: &adw::ToolbarView, issue: IssueId, links: &Links) -> gtk::MenuButton {
    let actions = gio::SimpleActionGroup::new();
    let trash = gio::SimpleAction::new("trash", None);
    {
        let hook = links.trash_issue.clone();
        trash.connect_activate(move |_, _| hook(issue));
    }
    actions.add_action(&trash);
    page.insert_action_group("issue", Some(&actions));
    let menu = gio::Menu::new();
    menu.append(Some("Move Issue to _Trash"), Some("issue.trash"));
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Issue Menu")
        .menu_model(&menu)
        .build()
}

/// The ways to start a new book from this copy, as a menu of actions
/// that belong to the copy's row.
fn more_menu(
    row: &adw::ExpanderRow,
    copy: CopyId,
    category: Category,
    links: &Links,
) -> gtk::MenuButton {
    let next = match category {
        Category::CollectedEdition => "Add _Next Volume",
        Category::Comic | Category::Magazine => "Add _Next Issue",
    };
    let actions = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    for (name, label, kind) in [
        ("another", "Add Another _Copy", Prefill::AnotherCopy),
        ("variant", "Add _Variant", Prefill::Variant),
        ("next", next, Prefill::NextIssue),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let hook = links.prefill.clone();
        action.connect_activate(move |_, _| hook(copy, kind));
        actions.add_action(&action);
        menu.append(Some(label), Some(&format!("copy.{name}")));
    }
    let trash = gio::SimpleAction::new("trash", None);
    {
        let hook = links.trash_copy.clone();
        trash.connect_activate(move |_, _| hook(copy));
    }
    actions.add_action(&trash);
    let last = gio::Menu::new();
    last.append(Some("Move to _Trash"), Some("copy.trash"));
    menu.append_section(None, &last);
    row.insert_action_group("copy", Some(&actions));
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("More Ways to Add")
        .css_classes(["flat"])
        .menu_model(&menu)
        .build()
}
