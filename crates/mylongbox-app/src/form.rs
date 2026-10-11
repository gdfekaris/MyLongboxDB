//! The form for adding or editing one physical comic, in sections that
//! collapse. Saving is explicit; the header marks unsaved changes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use mylongbox_core::{
    Amount, Binding, BookCondition, BookEntry, BoxId, Category, CharacterId, CopyId, CreatorId,
    Currency, Database, Distribution, Grade, GradeType, Isbn, IssueNumber, LookupId,
    MagazineFormat, PublisherId, Role, SeriesChoice, SeriesId, StoryArcId, TagId, TeamId,
};

use crate::{autocomplete, format};

/// A field the form could not read, and why.
pub struct Invalid {
    pub widget: gtk::Widget,
    pub message: String,
}

type Check<T> = Result<T, Invalid>;

fn invalid<T>(widget: &impl IsA<gtk::Widget>, message: &str) -> Check<T> {
    Err(Invalid {
        widget: widget.clone().upcast(),
        message: message.to_owned(),
    })
}

/// Marks the form changed. Shared by every input.
#[derive(Clone)]
struct Dirty {
    changed: Rc<Cell<bool>>,
    title: adw::WindowTitle,
}

impl Dirty {
    fn mark(&self) {
        if !self.changed.replace(true) {
            self.title.set_subtitle("Unsaved changes");
        }
    }

    fn watch(&self, editable: &(impl IsA<gtk::Editable> + IsA<gtk::Widget>)) {
        let dirty = self.clone();
        editable.connect_changed(move |e| {
            e.remove_css_class("error");
            dirty.mark();
        });
    }
}

pub struct Form {
    pub page: adw::NavigationPage,
    /// The copy being edited, or `None` when adding.
    pub target: Option<CopyId>,
    category: Category,
    dirty: Dirty,

    series: adw::EntryRow,
    /// The listed series whose label the series row holds, if any.
    picked_series: Rc<RefCell<Option<SeriesId>>>,
    start_year: adw::EntryRow,
    volume: adw::EntryRow,
    publisher: adw::EntryRow,
    /// New magazine series only.
    magazine_format: adw::ComboRow,
    color: adw::ComboRow,
    number: adw::EntryRow,
    title: adw::EntryRow,
    cover_date: adw::EntryRow,
    release_date: adw::EntryRow,
    pages: adw::EntryRow,
    key: adw::SwitchRow,
    key_reason: adw::EntryRow,
    synopsis: gtk::TextView,

    credits: NameList,
    characters: NameList,
    teams: NameList,
    arcs: NameList,

    variant: adw::EntryRow,
    printing: adw::EntryRow,
    distribution: adw::ComboRow,
    cover_price: MoneyRow,
    barcode: adw::EntryRow,
    cover_artists: NameList,
    isbn: adw::EntryRow,
    binding: adw::ComboRow,
    dust_jacket: adw::SwitchRow,
    slipcase: adw::SwitchRow,
    collects: adw::EntryRow,

    grade: adw::EntryRow,
    grade_type: adw::ComboRow,
    cert_number: adw::EntryRow,
    label_type: adw::EntryRow,
    signed_by: adw::EntryRow,
    purchase_price: MoneyRow,
    purchase_date: adw::EntryRow,
    seller: adw::EntryRow,
    storage_box: adw::EntryRow,
    box_position: adw::EntryRow,
    tags: NameList,
    notes: gtk::TextView,
    book_condition: adw::ComboRow,
    jacket_condition: adw::ComboRow,
}

/// Names to offer for what has been typed so far.
type Suggest = Rc<dyn Fn(&str) -> Vec<String>>;

/// What the form needs from the app.
pub struct Hooks {
    pub save: Rc<dyn Fn()>,
    pub cancel: Rc<dyn Fn()>,
}

const DISTRIBUTIONS: [(&str, Option<Distribution>); 3] = [
    ("Not noted", None),
    ("Direct edition", Some(Distribution::Direct)),
    ("Newsstand edition", Some(Distribution::Newsstand)),
];

const FORMATS: [(&str, Option<MagazineFormat>); 4] = [
    ("Not noted", None),
    ("Standard size", Some(MagazineFormat::Standard)),
    ("Treasury size", Some(MagazineFormat::Treasury)),
    ("Digest size", Some(MagazineFormat::Digest)),
];

/// Whether a magazine is black and white.
const COLORS: [(&str, Option<bool>); 3] = [
    ("Not noted", None),
    ("Color", Some(false)),
    ("Black and white", Some(true)),
];

const BINDINGS: [(&str, Option<Binding>); 4] = [
    ("Not noted", None),
    ("Softcover", Some(Binding::Softcover)),
    ("Hardcover", Some(Binding::Hardcover)),
    ("Oversized hardcover", Some(Binding::OversizedHardcover)),
];

const CONDITIONS: [(&str, Option<BookCondition>); 8] = [
    ("Not noted", None),
    ("Sealed", Some(BookCondition::Sealed)),
    ("New", Some(BookCondition::New)),
    ("Like new", Some(BookCondition::LikeNew)),
    ("Very good", Some(BookCondition::VeryGood)),
    ("Good", Some(BookCondition::Good)),
    ("Fair", Some(BookCondition::Fair)),
    ("Poor", Some(BookCondition::Poor)),
];

const GRADE_TYPES: [(&str, Option<GradeType>); 5] = [
    ("Not graded", None),
    ("My own estimate", Some(GradeType::Raw)),
    ("CGC", Some(GradeType::Cgc)),
    ("CBCS", Some(GradeType::Cbcs)),
    ("PGX", Some(GradeType::Pgx)),
];

impl Form {
    /// An empty add form, or the edit form of `target` filled from `entry`.
    pub fn new(
        db: &Rc<Database>,
        category: Category,
        target: Option<CopyId>,
        entry: Option<&BookEntry>,
        hooks: Hooks,
    ) -> Self {
        let db = Rc::downgrade(db);
        let noun = match category {
            Category::Comic => "Comic",
            Category::Magazine => "Magazine",
            Category::CollectedEdition => "Collected Edition",
        };
        let window_title = adw::WindowTitle::new(
            &match target {
                None => format!("Add {noun}"),
                Some(_) => format!("Edit {noun}"),
            },
            "",
        );
        let dirty = Dirty {
            changed: Rc::default(),
            title: window_title.clone(),
        };
        let prefs = adw::PreferencesPage::new();

        // Issue ------------------------------------------------------------
        let issue = section(&prefs, "Issue", None, true);
        let series = entry_row("Series");
        let picked_series: Rc<RefCell<Option<SeriesId>>> = Rc::default();
        let start_year = entry_row("Start Year of New Series");
        let volume = entry_row("Volume of New Series");
        let publisher = entry_row("Publisher of New Series");
        let magazine_format = combo_row("Format of New Series", FORMATS.map(|(name, _)| name));
        let color = combo_row("Color", COLORS.map(|(name, _)| name));
        let number = entry_row(match category {
            Category::CollectedEdition => "Volume Number (blank for a standalone book)",
            Category::Comic | Category::Magazine => "Issue Number",
        });
        let joins = adw::ActionRow::builder()
            .title("Already in your collection")
            .subtitle("This adds another copy, and fills in any details typed here.")
            .visible(false)
            .build();
        joins.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        joins.add_css_class("property");
        let title = entry_row("Story Title");
        let cover_date = entry_row(match category {
            Category::CollectedEdition => "Publication Date (e.g. Mar 2014)",
            Category::Comic | Category::Magazine => "Cover Date (e.g. Mar 1975)",
        });
        let release_date = entry_row("Release Date (YYYY-MM-DD)");
        let pages = entry_row("Page Count");
        let key = adw::SwitchRow::builder().title("Key Issue").build();
        let key_reason = entry_row("Why It Is Key");
        key.bind_property("active", &key_reason, "visible")
            .sync_create()
            .build();
        let synopsis = text_view();
        for row in [&series, &start_year, &volume, &publisher] {
            issue.append(row);
        }
        issue.append(&magazine_format);
        issue.append(&color);
        issue.append(&number);
        issue.append(&joins);
        for row in [&title, &cover_date, &release_date, &pages] {
            issue.append(row);
        }
        issue.append(&key);
        issue.append(&key_reason);
        issue.append(&text_row("Synopsis", &synopsis));
        for row in [&start_year, &volume, &publisher] {
            row.set_visible(false);
        }
        magazine_format.set_visible(false);
        color.set_visible(false);

        {
            // The rows for a new series show only when the typed name is
            // not one already listed.
            let db = db.clone();
            let picked = picked_series.clone();
            // A book line numbers its volumes itself, so a series-level
            // volume would only confuse.
            let new_rows: Vec<adw::EntryRow> = if category == Category::CollectedEdition {
                vec![start_year.clone(), publisher.clone()]
            } else {
                vec![start_year.clone(), volume.clone(), publisher.clone()]
            };
            let magazine_rows = [magazine_format.clone(), color.clone()];
            let labels = Rc::new(move || series_labels(&db, category));
            let update = {
                let labels = labels.clone();
                move |text: &str| {
                    let found = labels()
                        .into_iter()
                        .find(|(label, _)| label.eq_ignore_ascii_case(text.trim()));
                    *picked.borrow_mut() = found.as_ref().map(|(_, id)| *id);
                    let new = found.is_none() && !text.trim().is_empty();
                    for row in &new_rows {
                        row.set_visible(new);
                    }
                    for row in &magazine_rows {
                        row.set_visible(new && category == Category::Magazine);
                    }
                }
            };
            let update = Rc::new(update);
            {
                let update = update.clone();
                series.connect_changed(move |row| update(&row.text()));
            }
            autocomplete::attach(
                &series,
                move |text| {
                    let text = text.to_lowercase();
                    labels()
                        .into_iter()
                        .map(|(label, _)| label)
                        .filter(|label| label.to_lowercase().contains(&text))
                        .collect()
                },
                move |text| update(text),
            );
        }
        lookup_completion::<PublisherId>(&publisher, &db);

        let collects = entry_row("Issues Reprinted");
        if category == Category::CollectedEdition {
            let list = section(
                &prefs,
                "Collects",
                Some(
                    "For example: Amazing Spider-Man (1963) 1\u{2013}10, Annual 1; \
                     Amazing Fantasy (1962) 15",
                ),
                true,
            );
            list.append(&collects);
            contents_preview(&collects, &list, &db);
        }

        let credits = NameList::new(
            &section(
                &prefs,
                "Credits",
                Some("Writers, artists, and editors"),
                true,
            ),
            "Add Credit",
            Extra::Role(roles(category)),
            completion::<CreatorId>(&db),
            &dirty,
        );
        let contents = section(
            &prefs,
            "Characters, Teams, and Story Arcs",
            Some("Mark a character's or team's first appearance with the star."),
            false,
        );
        let characters = NameList::new(
            &contents,
            "Add Character",
            Extra::First,
            completion::<CharacterId>(&db),
            &dirty,
        );
        let teams = NameList::new(
            &contents,
            "Add Team",
            Extra::First,
            completion::<TeamId>(&db),
            &dirty,
        );
        let arcs = NameList::new(
            &contents,
            "Add Story Arc",
            Extra::Part,
            completion::<StoryArcId>(&db),
            &dirty,
        );

        // Edition ----------------------------------------------------------
        let edition = section(
            &prefs,
            "Edition",
            Some("Shared by every copy of this cover and printing"),
            false,
        );
        let variant = entry_row("Variant (blank for the standard cover)");
        let printing = entry_row("Printing (blank for a first printing)");
        let distribution = combo_row("Distribution", DISTRIBUTIONS.map(|(name, _)| name));
        let cover_price = MoneyRow::new("Cover Price");
        let barcode = entry_row("Barcode");
        let isbn = entry_row("ISBN");
        let binding = combo_row("Binding", BINDINGS.map(|(name, _)| name));
        let dust_jacket = adw::SwitchRow::builder().title("Dust Jacket").build();
        let slipcase = adw::SwitchRow::builder().title("Slipcase").build();
        edition.append(&variant);
        edition.append(&printing);
        if category == Category::CollectedEdition {
            edition.append(&isbn);
            edition.append(&binding);
            edition.append(&dust_jacket);
            edition.append(&slipcase);
        } else {
            // Books are not sold on newsstands.
            edition.append(&distribution);
        }
        edition.append(&cover_price.row);
        edition.append(&barcode);
        let cover_artists = NameList::new(
            &edition,
            "Add Cover Artist",
            Extra::None,
            completion::<CreatorId>(&db),
            &dirty,
        );

        // Copy -------------------------------------------------------------
        let copy = section(&prefs, "This Copy", None, true);
        let grade = entry_row("Grade (0.5 to 10.0)");
        let grade_type = combo_row("Graded By", GRADE_TYPES.map(|(name, _)| name));
        let cert_number = entry_row("Certificate Number");
        let label_type = entry_row("Label");
        let signed_by = entry_row("Signed By");
        let purchase_price = MoneyRow::new("Purchase Price");
        let purchase_date = entry_row("Purchase Date (YYYY-MM-DD)");
        let seller = entry_row("Bought From");
        let storage_box = entry_row("Storage Box");
        let box_position = entry_row("Position in Box");
        lookup_completion::<BoxId>(&storage_box, &db);
        let book_condition = combo_row("Condition", CONDITIONS.map(|(name, _)| name));
        let jacket_condition = combo_row("Dust Jacket Condition", CONDITIONS.map(|(name, _)| name));
        if category == Category::CollectedEdition {
            copy.append(&book_condition);
            copy.append(&jacket_condition);
        } else {
            copy.append(&grade);
            copy.append(&grade_type);
            copy.append(&cert_number);
            copy.append(&label_type);
        }
        copy.append(&signed_by);
        copy.append(&purchase_price.row);
        for row in [&purchase_date, &seller, &storage_box, &box_position] {
            copy.append(row);
        }
        let tags = NameList::new(
            &copy,
            "Add Tag",
            Extra::None,
            completion::<TagId>(&db),
            &dirty,
        );
        let notes = text_view();
        copy.append(&text_row("Notes", &notes));

        // Joining an issue already listed --------------------------------
        if target.is_none() {
            let db = db.clone();
            let picked = picked_series.clone();
            let number_row = number.clone();
            let check = Rc::new(move || {
                let found = (|| {
                    let series = (*picked.borrow())?;
                    let number = IssueNumber::new(number_row.text().trim()).ok()?;
                    db.upgrade()?
                        .find_issue(series, Some(&number))
                        .ok()
                        .flatten()
                })();
                joins.set_visible(found.is_some());
            });
            let c = check.clone();
            series.connect_changed(move |_| c());
            number.connect_changed(move |_| check());
        }

        // Header and keys --------------------------------------------------
        let cancel = gtk::Button::with_mnemonic("_Cancel");
        let save = gtk::Button::with_mnemonic("_Save");
        save.add_css_class("suggested-action");
        {
            let hook = hooks.cancel.clone();
            cancel.connect_clicked(move |_| hook());
        }
        {
            let hook = hooks.save.clone();
            save.connect_clicked(move |_| hook());
        }
        let header = adw::HeaderBar::builder()
            .title_widget(&window_title)
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .show_back_button(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&prefs));
        let page = adw::NavigationPage::builder()
            .title(window_title.title())
            .child(&toolbar)
            // Leaving goes through Cancel, which asks before discarding.
            .can_pop(false)
            .build();
        let keys = gtk::ShortcutController::new();
        keys.set_scope(gtk::ShortcutScope::Managed);
        let shortcut = |trigger: &str, hook: Rc<dyn Fn()>| {
            gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    hook();
                    glib::Propagation::Stop
                })),
            )
        };
        keys.add_shortcut(shortcut("<Control>s", hooks.save.clone()));
        page.add_controller(keys);
        // Text fields swallow Escape, so it is caught on its way down to
        // them, unless it is meant to close a list of suggestions.
        let escape = gtk::EventControllerKey::new();
        escape.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let cancel = hooks.cancel.clone();
            escape.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape && !autocomplete::showing() {
                    cancel();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        page.add_controller(escape);

        let form = Self {
            page,
            target,
            category,
            dirty,
            series,
            picked_series,
            start_year,
            volume,
            publisher,
            magazine_format,
            color,
            number,
            title,
            cover_date,
            release_date,
            pages,
            key,
            key_reason,
            synopsis,
            credits,
            characters,
            teams,
            arcs,
            variant,
            printing,
            distribution,
            cover_price,
            barcode,
            cover_artists,
            isbn,
            binding,
            dust_jacket,
            slipcase,
            collects,
            grade,
            grade_type,
            cert_number,
            label_type,
            signed_by,
            purchase_price,
            purchase_date,
            seller,
            storage_box,
            box_position,
            tags,
            notes,
            book_condition,
            jacket_condition,
        };
        if let Some(entry) = entry {
            form.fill(entry, &db);
        }
        form.watch_all();
        form.dirty.changed.set(false);
        form.dirty.title.set_subtitle("");
        form
    }

    /// Puts the cursor in the series field of a new book, once the page
    /// is showing. An edit form leaves it alone, so typing cannot replace
    /// the series by accident.
    pub fn focus_first(&self) {
        if self.target.is_none() {
            self.series.grab_focus();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.changed.get()
    }

    fn watch_all(&self) {
        let d = &self.dirty;
        for row in [
            &self.series,
            &self.start_year,
            &self.volume,
            &self.publisher,
            &self.number,
            &self.title,
            &self.cover_date,
            &self.release_date,
            &self.pages,
            &self.key_reason,
            &self.variant,
            &self.printing,
            &self.barcode,
            &self.grade,
            &self.cert_number,
            &self.label_type,
            &self.signed_by,
            &self.purchase_date,
            &self.seller,
            &self.storage_box,
            &self.box_position,
            &self.cover_price.row,
            &self.purchase_price.row,
            &self.isbn,
            &self.collects,
        ] {
            d.watch(row);
        }
        for view in [&self.synopsis, &self.notes] {
            let d = d.clone();
            view.buffer().connect_changed(move |_| d.mark());
        }
        for combo in [
            &self.distribution,
            &self.grade_type,
            &self.magazine_format,
            &self.color,
            &self.binding,
            &self.book_condition,
            &self.jacket_condition,
        ] {
            let d = d.clone();
            combo.connect_selected_notify(move |_| d.mark());
        }
        for money in [&self.cover_price, &self.purchase_price] {
            let d = d.clone();
            money.currency.connect_selected_notify(move |_| d.mark());
        }
        for switch in [&self.key, &self.dust_jacket, &self.slipcase] {
            let d = d.clone();
            switch.connect_active_notify(move |_| d.mark());
        }
    }

    fn fill(&self, e: &BookEntry, db: &Weak<Database>) {
        if let SeriesChoice::Existing(id) = e.series
            && let Some(db) = db.upgrade()
            && let Ok(series) = db.series(id)
        {
            self.series
                .set_text(&series_label(&series.fields.name, series.fields.start_year));
        }
        let i = &e.issue;
        self.number
            .set_text(i.number.as_ref().map_or("", |n| n.as_str()));
        self.title.set_text(&i.title);
        self.cover_date.set_text(&format::cover_date(i.cover_date));
        self.release_date
            .set_text(&i.release_date.map(|d| d.to_string()).unwrap_or_default());
        self.pages
            .set_text(&i.page_count.map(|p| p.to_string()).unwrap_or_default());
        self.key.set_active(i.is_key);
        self.key_reason.set_text(&i.key_reason);
        self.synopsis.buffer().set_text(&i.synopsis);
        for (name, role) in &e.credits {
            self.credits.add(name, Some(*role), false, None);
        }
        for (name, first) in &e.characters {
            self.characters.add(name, None, *first, None);
        }
        for (name, first) in &e.teams {
            self.teams.add(name, None, *first, None);
        }
        for (name, part) in &e.story_arcs {
            self.arcs.add(name, None, false, *part);
        }

        let ed = &e.edition;
        self.variant.set_text(&ed.variant);
        self.printing
            .set_text(&ed.printing.map(|p| p.to_string()).unwrap_or_default());
        select(&self.distribution, &DISTRIBUTIONS, ed.distribution);
        self.cover_price.set(ed.cover_price);
        self.barcode.set_text(ed.barcode.as_deref().unwrap_or(""));
        self.isbn
            .set_text(ed.isbn.as_ref().map_or("", |i| i.as_str()));
        select(&self.binding, &BINDINGS, ed.binding);
        self.dust_jacket
            .set_active(ed.has_dust_jacket == Some(true));
        self.slipcase.set_active(ed.has_slipcase == Some(true));
        self.collects.set_text(&e.collects);
        for name in &e.cover_artists {
            self.cover_artists.add(name, None, false, None);
        }

        let c = &e.copy;
        self.grade
            .set_text(&c.grade.map(|g| g.to_string()).unwrap_or_default());
        select(&self.grade_type, &GRADE_TYPES, c.grade_type);
        select(&self.book_condition, &CONDITIONS, c.book_condition);
        select(&self.jacket_condition, &CONDITIONS, c.dust_jacket_condition);
        self.cert_number
            .set_text(c.cert_number.as_deref().unwrap_or(""));
        self.label_type
            .set_text(c.label_type.as_deref().unwrap_or(""));
        self.signed_by.set_text(&c.signed_by);
        self.purchase_price.set(c.purchase_price);
        self.purchase_date
            .set_text(&c.purchase_date.map(|d| d.to_string()).unwrap_or_default());
        self.seller.set_text(&c.seller);
        self.storage_box
            .set_text(e.storage_box.as_deref().unwrap_or(""));
        self.box_position
            .set_text(&c.box_position.map(|p| p.to_string()).unwrap_or_default());
        for name in &e.tags {
            self.tags.add(name, None, false, None);
        }
        self.notes.buffer().set_text(&c.notes);
    }

    /// Reads the form, or names the first field that needs fixing.
    pub fn entry(&self) -> Check<BookEntry> {
        let series_text = self.series.text().trim().to_owned();
        if series_text.is_empty() {
            return invalid(&self.series, "Enter a series");
        }
        let series = match *self.picked_series.borrow() {
            Some(id) => SeriesChoice::Existing(id),
            None => {
                let (name, year_in_name) = split_year(&series_text);
                SeriesChoice::New {
                    name,
                    start_year: optional(&self.start_year, "Enter the year as four digits")?
                        .or(year_in_name),
                    volume: optional(&self.volume, "Enter the volume as a number")?,
                    publisher: Some(self.publisher.text().trim().to_owned())
                        .filter(|p| !p.is_empty()),
                    magazine_format: FORMATS[selected(&self.magazine_format)]
                        .1
                        .filter(|_| self.category == Category::Magazine),
                    black_and_white: COLORS[selected(&self.color)]
                        .1
                        .filter(|_| self.category == Category::Magazine),
                }
            }
        };
        let number_text = self.number.text();
        let number = match number_text.trim() {
            // A standalone book has no volume number.
            "" if self.category == Category::CollectedEdition => None,
            text => match IssueNumber::new(text) {
                Ok(number) => Some(number),
                Err(why) => return invalid(&self.number, &capitalized(&why.to_string())),
            },
        };
        let mut e = BookEntry::new(self.category, series, number);

        let i = &mut e.issue;
        i.title = self.title.text().trim().to_owned();
        i.cover_date = match format::parse_cover_date(&self.cover_date.text()) {
            Ok(date) => date,
            Err(()) => {
                return invalid(
                    &self.cover_date,
                    "Enter the cover date like \u{201c}Mar 1975\u{201d} or \u{201c}1975\u{201d}",
                );
            }
        };
        i.release_date = date(&self.release_date)?;
        i.page_count = optional(&self.pages, "Enter the page count as a number")?;
        i.is_key = self.key.is_active();
        i.key_reason = if i.is_key {
            self.key_reason.text().trim().to_owned()
        } else {
            String::new()
        };
        i.synopsis = text(&self.synopsis);

        e.credits = self
            .credits
            .values()
            .into_iter()
            .map(|v| (v.name, v.role.unwrap_or(Role::Writer)))
            .collect();
        e.characters = self
            .characters
            .values()
            .into_iter()
            .map(|v| (v.name, v.first))
            .collect();
        e.teams = self
            .teams
            .values()
            .into_iter()
            .map(|v| (v.name, v.first))
            .collect();
        e.story_arcs = self
            .arcs
            .values()
            .into_iter()
            .map(|v| (v.name, v.part))
            .collect();

        let ed = &mut e.edition;
        ed.variant = self.variant.text().trim().to_owned();
        ed.printing = optional(&self.printing, "Enter the printing as a number, such as 2")?;
        let book = self.category == Category::CollectedEdition;
        ed.distribution = DISTRIBUTIONS[selected(&self.distribution)]
            .1
            .filter(|_| !book);
        if book {
            let isbn_text = self.isbn.text();
            ed.isbn = match isbn_text.trim() {
                "" => None,
                text => match Isbn::parse(text) {
                    Some(isbn) => Some(isbn),
                    None => {
                        return invalid(&self.isbn, "That ISBN is not valid; check its 13 digits");
                    }
                },
            };
            ed.binding = BINDINGS[selected(&self.binding)].1;
            ed.has_dust_jacket = self.dust_jacket.is_active().then_some(true);
            ed.has_slipcase = self.slipcase.is_active().then_some(true);
        }
        ed.cover_price = self.cover_price.amount()?;
        ed.barcode = Some(self.barcode.text().trim().to_owned()).filter(|b| !b.is_empty());
        e.cover_artists = self
            .cover_artists
            .values()
            .into_iter()
            .map(|v| v.name)
            .collect();

        if book {
            e.collects = self.collects.text().trim().to_owned();
        }
        let c = &mut e.copy;
        if book {
            c.book_condition = CONDITIONS[selected(&self.book_condition)].1;
            c.dust_jacket_condition = CONDITIONS[selected(&self.jacket_condition)].1;
        }
        let grade_text = if book {
            glib::GString::from("")
        } else {
            self.grade.text()
        };
        c.grade = match grade_text.trim() {
            "" => None,
            text => match Grade::parse(text) {
                Some(grade) => Some(grade),
                None => {
                    return invalid(
                        &self.grade,
                        "Grades run from 0.5 to 10.0, in tenths, such as 9.4",
                    );
                }
            },
        };
        if !book {
            c.grade_type = GRADE_TYPES[selected(&self.grade_type)].1;
            c.cert_number =
                Some(self.cert_number.text().trim().to_owned()).filter(|t| !t.is_empty());
            c.label_type = Some(self.label_type.text().trim().to_owned()).filter(|t| !t.is_empty());
        }
        c.signed_by = self.signed_by.text().trim().to_owned();
        c.purchase_price = self.purchase_price.amount()?;
        c.purchase_date = date(&self.purchase_date)?;
        c.seller = self.seller.text().trim().to_owned();
        e.storage_box = Some(self.storage_box.text().trim().to_owned()).filter(|b| !b.is_empty());
        c.box_position = optional(&self.box_position, "Enter the position as a number")?;
        if c.box_position.is_some() && e.storage_box.is_none() {
            return invalid(&self.storage_box, "Name the box this position is in");
        }
        c.notes = text(&self.notes);
        e.tags = self.tags.values().into_iter().map(|v| v.name).collect();
        Ok(e)
    }

    /// Points at a field that needs fixing.
    pub fn show_invalid(&self, invalid: &Invalid) {
        invalid.widget.add_css_class("error");
        invalid.widget.grab_focus();
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `Amazing Spider-Man (1963)` → name and year.
fn split_year(text: &str) -> (String, Option<u16>) {
    if let Some(open) = text.rfind(" (")
        && let Some(year) = text[open + 2..].strip_suffix(')')
        && year.len() == 4
        && let Ok(year) = year.parse()
    {
        return (text[..open].trim().to_owned(), Some(year));
    }
    (text.to_owned(), None)
}

fn series_label(name: &str, start_year: Option<u16>) -> String {
    match start_year {
        Some(year) => format!("{name} ({year})"),
        None => name.to_owned(),
    }
}

fn series_labels(db: &Weak<Database>, category: Category) -> Vec<(String, SeriesId)> {
    let Some(db) = db.upgrade() else {
        return Vec::new();
    };
    db.list_series()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.fields.category == category)
        .map(|s| (series_label(&s.fields.name, s.fields.start_year), s.id))
        .collect()
}

/// Most issues listed in the contents preview; the rest are counted.
const PREVIEW_MAX: usize = 40;

/// Completes the series being typed at the end of a contents list, and
/// lists under it the issues the text names, marking those not yet in
/// the catalog, which saving will add.
fn contents_preview(row: &adw::EntryRow, list: &gtk::ListBox, db: &Weak<Database>) {
    {
        let db = db.clone();
        autocomplete::attach_with(
            row,
            move |text| {
                let (head, tail) = match text.rfind(';') {
                    Some(at) => (&text[..=at], &text[at + 1..]),
                    None => ("", text),
                };
                let typed = tail.trim_start().to_lowercase();
                // Once a year is typed the series is chosen.
                if typed.is_empty() || typed.contains(')') {
                    return Vec::new();
                }
                let space = if head.is_empty() { "" } else { " " };
                reprintable_series(&db)
                    .into_iter()
                    .filter(|label| label.to_lowercase().contains(&typed))
                    .map(|label| {
                        let text = format!("{head}{space}{label} ");
                        (label, text)
                    })
                    .collect()
            },
            |_| {},
        );
    }

    let shown: Rc<RefCell<Vec<gtk::Widget>>> = Rc::default();
    // Each change bumps this; a preview runs only if nothing newer came
    // in during the pause.
    let latest = Rc::new(Cell::new(0u64));
    let db = db.clone();
    let list = list.clone();
    row.connect_changed(move |row| {
        let ticket = latest.get() + 1;
        latest.set(ticket);
        let (row, list, db, shown, latest) = (
            row.clone(),
            list.clone(),
            db.clone(),
            shown.clone(),
            latest.clone(),
        );
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            if latest.get() != ticket {
                return;
            }
            for old in shown.borrow_mut().drain(..) {
                list.remove(&old);
            }
            let text = row.text();
            let Some(db) = db.upgrade() else {
                return;
            };
            if text.trim().is_empty() {
                return;
            }
            let mut rows: Vec<adw::ActionRow> = Vec::new();
            match db.preview_collects(&text) {
                Ok(entries) => {
                    let new = entries.iter().filter(|e| e.issue.is_none()).count();
                    let count = match entries.len() {
                        1 => "1 issue".to_owned(),
                        n => format!("{n} issues"),
                    };
                    let detail = match new {
                        0 => "All are in the catalog already".to_owned(),
                        1 => "1 is not in the catalog yet; saving adds it".to_owned(),
                        n => format!("{n} are not in the catalog yet; saving adds them"),
                    };
                    let summary = plain_row(&count, &detail);
                    summary.add_css_class("property");
                    rows.push(summary);
                    for e in entries.iter().take(PREVIEW_MAX) {
                        let title = format!(
                            "{} #{}",
                            series_label(&e.entry.series.name, e.entry.series.start_year),
                            e.entry.number
                        );
                        let item = plain_row(&title, "");
                        if e.issue.is_none() {
                            let tag = gtk::Label::builder()
                                .label("New")
                                .css_classes(["accent", "caption-heading"])
                                .build();
                            item.add_suffix(&tag);
                        }
                        rows.push(item);
                    }
                    if entries.len() > PREVIEW_MAX {
                        let more = entries.len() - PREVIEW_MAX;
                        rows.push(plain_row(&format!("and {more} more"), ""));
                    }
                }
                Err(why) => {
                    let error =
                        plain_row("This list cannot be read", &capitalized(&why.to_string()));
                    error.add_css_class("error");
                    rows.push(error);
                }
            }
            for item in rows {
                list.append(&item);
                shown.borrow_mut().push(item.upcast());
            }
        });
    });
}

fn plain_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row
}

/// Labels of every series a collected edition can reprint from.
fn reprintable_series(db: &Weak<Database>) -> Vec<String> {
    let Some(db) = db.upgrade() else {
        return Vec::new();
    };
    db.list_series()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.fields.category != Category::CollectedEdition)
        .map(|s| series_label(&s.fields.name, s.fields.start_year))
        .collect()
}

fn completion<I: LookupId + 'static>(db: &Weak<Database>) -> Suggest {
    let db = db.clone();
    Rc::new(move |text: &str| {
        let Some(db) = db.upgrade() else {
            return Vec::new();
        };
        // Lists are small enough that one more than shown is plenty.
        db.suggest_lookups::<I>(text, autocomplete::LIMIT as u32 + 1)
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.name)
            .collect()
    })
}

fn lookup_completion<I: LookupId + 'static>(row: &adw::EntryRow, db: &Weak<Database>) {
    let suggest = completion::<I>(db);
    autocomplete::attach(row, move |text| suggest(text), |_| {});
}

fn optional<T: std::str::FromStr>(row: &adw::EntryRow, message: &str) -> Check<Option<T>> {
    let text = row.text();
    match text.trim() {
        "" => Ok(None),
        text => match text.parse() {
            Ok(value) => Ok(Some(value)),
            Err(_) => invalid(row, message),
        },
    }
}

fn date(row: &adw::EntryRow) -> Check<Option<mylongbox_core::Date>> {
    optional(row, "Enter the date as YYYY-MM-DD, such as 1975-03-05")
}

fn text(view: &gtk::TextView) -> String {
    let buffer = view.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .trim()
        .to_owned()
}

fn entry_row(title: &str) -> adw::EntryRow {
    adw::EntryRow::builder().title(title).build()
}

fn combo_row<const N: usize>(title: &str, names: [&str; N]) -> adw::ComboRow {
    adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&names))
        .build()
}

fn selected(combo: &adw::ComboRow) -> usize {
    usize::try_from(combo.selected()).unwrap_or(0)
}

fn select<T: PartialEq, const N: usize>(
    combo: &adw::ComboRow,
    choices: &[(&str, Option<T>); N],
    value: Option<T>,
) {
    let index = choices.iter().position(|(_, v)| *v == value).unwrap_or(0);
    combo.set_selected(u32::try_from(index).unwrap_or(0));
}

fn text_view() -> gtk::TextView {
    gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .accepts_tab(false)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .height_request(96)
        .build()
}

/// A labelled box for several lines of text.
fn text_row(title: &str, view: &gtk::TextView) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    let frame = gtk::Frame::builder().child(view).build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&label);
    content.append(&frame);
    gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(false)
        .selectable(false)
        .build()
}

/// A titled section whose rows can be hidden with the button beside the
/// title. Returns the list to put rows in.
fn section(
    prefs: &adw::PreferencesPage,
    title: &str,
    description: Option<&str>,
    open: bool,
) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let revealer = gtk::Revealer::builder()
        .child(&list)
        .reveal_child(open)
        .build();
    // A plain button rather than a toggle, which would look pressed
    // whenever the section is open.
    let icon = |open: bool| {
        if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }
    };
    let toggle = gtk::Button::builder()
        .icon_name(icon(open))
        .tooltip_text(format!("Show or Hide {title}"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    {
        let revealer = revealer.clone();
        toggle.connect_clicked(move |button| {
            let open = !revealer.reveals_child();
            revealer.set_reveal_child(open);
            button.set_icon_name(icon(open));
        });
    }
    let group = adw::PreferencesGroup::builder().title(title).build();
    if let Some(description) = description {
        group.set_description(Some(description));
    }
    group.set_header_suffix(Some(&toggle));
    group.add(&revealer);
    prefs.add(&group);
    list
}

/// An amount with its currency.
struct MoneyRow {
    row: adw::EntryRow,
    currency: gtk::DropDown,
}

impl MoneyRow {
    fn new(title: &str) -> Self {
        let row = entry_row(title);
        let currency = gtk::DropDown::from_strings(&["USD", "Gold (oz)", "Bitcoin"]);
        currency.set_valign(gtk::Align::Center);
        currency.set_tooltip_text(Some("Currency"));
        row.add_suffix(&currency);
        Self { row, currency }
    }

    fn currency(&self) -> Currency {
        Currency::ALL[usize::try_from(self.currency.selected()).unwrap_or(0)]
    }

    fn set(&self, amount: Option<Amount>) {
        let Some(amount) = amount else {
            return;
        };
        let index = Currency::ALL
            .iter()
            .position(|c| *c == amount.currency())
            .unwrap_or(0);
        self.currency
            .set_selected(u32::try_from(index).unwrap_or(0));
        self.row.set_text(&amount.to_decimal_string());
    }

    fn amount(&self) -> Check<Option<Amount>> {
        let text = self.row.text();
        if text.trim().is_empty() {
            return Ok(None);
        }
        // People paste "$1.25" and "1,000.00"; neither symbol nor
        // grouping changes the amount.
        let cleaned: String = text
            .trim()
            .trim_start_matches('$')
            .chars()
            .filter(|c| *c != ',')
            .collect();
        match Amount::parse(self.currency(), &cleaned) {
            Ok(amount) => Ok(Some(amount)),
            Err(why) => invalid(&self.row, &capitalized(&why.to_string())),
        }
    }
}

/// What each row of a name list holds besides the name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Extra {
    None,
    /// The roles offered.
    Role(&'static [(&'static str, Role)]),
    First,
    Part,
}

struct NameValue {
    name: String,
    role: Option<Role>,
    first: bool,
    part: Option<u32>,
}

struct NameRow {
    row: adw::EntryRow,
    role: Option<gtk::DropDown>,
    first: Option<gtk::ToggleButton>,
    part: Option<gtk::SpinButton>,
}

/// Roles in the order offered. Only magazines credit articles and
/// interviews.
const ROLES: [(&str, Role); 9] = [
    ("Writer", Role::Writer),
    ("Penciller", Role::Penciller),
    ("Inker", Role::Inker),
    ("Colorist", Role::Colorist),
    ("Letterer", Role::Letterer),
    ("Cover Artist", Role::CoverArtist),
    ("Editor", Role::Editor),
    ("Article", Role::Article),
    ("Interview", Role::Interview),
];

fn roles(category: Category) -> &'static [(&'static str, Role)] {
    match category {
        Category::Magazine => &ROLES,
        Category::Comic | Category::CollectedEdition => &ROLES[..7],
    }
}

/// Rows of names that can be added and removed, each with autocomplete.
#[derive(Clone)]
struct NameList {
    list: gtk::ListBox,
    /// Rows go in above this one.
    add_button: adw::ButtonRow,
    rows: Rc<RefCell<Vec<NameRow>>>,
    extra: Extra,
    suggest: Suggest,
    dirty: Dirty,
    placeholder: &'static str,
}

impl NameList {
    fn new(
        list: &gtk::ListBox,
        add_label: &'static str,
        extra: Extra,
        suggest: Suggest,
        dirty: &Dirty,
    ) -> Self {
        let add_button = adw::ButtonRow::builder()
            .title(add_label)
            .start_icon_name("list-add-symbolic")
            .build();
        list.append(&add_button);
        let placeholder = match extra {
            Extra::Role(_) => "Creator",
            Extra::First => {
                if add_label.contains("Team") {
                    "Team"
                } else {
                    "Character"
                }
            }
            Extra::Part => "Story Arc",
            Extra::None => {
                if add_label.contains("Tag") {
                    "Tag"
                } else {
                    "Cover Artist"
                }
            }
        };
        let names = Self {
            list: list.clone(),
            add_button: add_button.clone(),
            rows: Rc::default(),
            extra,
            suggest,
            dirty: dirty.clone(),
            placeholder,
        };
        {
            let names = names.clone();
            add_button.connect_activated(move |_| {
                let row = names.add("", None, false, None);
                row.grab_focus();
                names.dirty.mark();
            });
        }
        names
    }

    fn add(&self, name: &str, role: Option<Role>, first: bool, part: Option<u32>) -> adw::EntryRow {
        let row = entry_row(self.placeholder);
        row.set_text(name);
        let suggest = self.suggest.clone();
        autocomplete::attach(&row, move |text| suggest(text), |_| {});
        self.dirty.watch(&row);

        let mut item = NameRow {
            row: row.clone(),
            role: None,
            first: None,
            part: None,
        };
        match self.extra {
            Extra::Role(offered) => {
                let names: Vec<&str> = offered.iter().map(|(name, _)| *name).collect();
                let roles = gtk::DropDown::from_strings(&names);
                roles.set_valign(gtk::Align::Center);
                roles.set_tooltip_text(Some("Role"));
                let index = offered
                    .iter()
                    .position(|(_, r)| Some(*r) == role)
                    .unwrap_or(0);
                roles.set_selected(u32::try_from(index).unwrap_or(0));
                let d = self.dirty.clone();
                roles.connect_selected_notify(move |_| d.mark());
                row.add_suffix(&roles);
                item.role = Some(roles);
            }
            Extra::First => {
                let star = gtk::ToggleButton::builder()
                    .icon_name("starred-symbolic")
                    .active(first)
                    .valign(gtk::Align::Center)
                    .tooltip_text("First Appearance")
                    .css_classes(["flat"])
                    .build();
                let d = self.dirty.clone();
                star.connect_toggled(move |_| d.mark());
                row.add_suffix(&star);
                item.first = Some(star);
            }
            Extra::Part => {
                let spin = gtk::SpinButton::with_range(0.0, 999.0, 1.0);
                spin.set_value(f64::from(part.unwrap_or(0)));
                spin.set_valign(gtk::Align::Center);
                spin.set_tooltip_text(Some("Part Number (0 if none)"));
                let d = self.dirty.clone();
                spin.connect_value_changed(move |_| d.mark());
                row.add_suffix(&spin);
                item.part = Some(spin);
            }
            Extra::None => {}
        }
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Remove")
            .css_classes(["flat"])
            .build();
        {
            let names = self.clone();
            let row = row.clone();
            remove.connect_clicked(move |_| {
                names.list.remove(&row);
                names.rows.borrow_mut().retain(|r| r.row != row);
                names.dirty.mark();
            });
        }
        row.add_suffix(&remove);

        let at = self.add_button.index();
        self.list.insert(&row, at);
        self.rows.borrow_mut().push(item);
        row
    }

    fn values(&self) -> Vec<NameValue> {
        let offered = match self.extra {
            Extra::Role(offered) => offered,
            Extra::None | Extra::First | Extra::Part => &[],
        };
        self.rows
            .borrow()
            .iter()
            .filter(|r| !r.row.text().trim().is_empty())
            .map(|r| NameValue {
                name: r.row.text().trim().to_owned(),
                role: r
                    .role
                    .as_ref()
                    .and_then(|d| offered.get(usize::try_from(d.selected()).ok()?))
                    .map(|(_, role)| *role),
                first: r.first.as_ref().is_some_and(|t| t.is_active()),
                part: r
                    .part
                    .as_ref()
                    .map(|s| s.value_as_int())
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|&v| v > 0),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_series_names_keep_their_year() {
        assert_eq!(
            split_year("Amazing Spider-Man (1963)"),
            ("Amazing Spider-Man".to_owned(), Some(1963))
        );
        assert_eq!(split_year("X-Men"), ("X-Men".to_owned(), None));
        assert_eq!(split_year("Year (One)"), ("Year (One)".to_owned(), None));
    }
}
