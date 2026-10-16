//! The form for adding or editing one physical comic, in sections that
//! collapse. Saving is explicit; the header marks unsaved changes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use mylongbox_core::{
    Amount, Binding, BookCondition, BookEntry, BoxId, Category, CharacterId, CopyId, CreatorId,
    Currency, Database, Distribution, Grade, GradeType, Isbn, IssueNumber, LookupId,
    MagazineFormat, PageQuality, PublisherId, Role, SeriesChoice, SeriesId, SignatureFields,
    SignaturePlacement, StoryArcId, TagId, TeamId, Verification,
};

use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f, pgettext};
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
            self.title.set_subtitle(&gettext("Unsaved changes"));
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
    focus: Focus,
    category: Category,
    dirty: Dirty,

    series: adw::EntryRow,
    /// The listed series whose label the series row holds, if any.
    picked_series: Rc<RefCell<Option<SeriesId>>>,
    start_year: adw::EntryRow,
    volume: adw::EntryRow,
    publisher: adw::EntryRow,
    imprint: adw::EntryRow,
    /// New magazine series only.
    magazine_format: adw::ComboRow,
    color: adw::ComboRow,
    number: adw::EntryRow,
    /// Comics and magazines only.
    legacy: adw::EntryRow,
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
    canadian_price: adw::EntryRow,
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
    page_quality: adw::ComboRow,
    signatures: SignatureList,
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

/// Where the cursor starts: in the field the user most likely types
/// first for this kind of form.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// A blank form starts at the series.
    Series,
    /// The next issue starts at its story title.
    Title,
    /// A variant starts at its variant name, with the edition open.
    Variant,
    /// Another copy starts at its grade or condition.
    Copy,
    /// An edit form leaves the cursor alone, so typing cannot replace
    /// anything by accident.
    Nowhere,
}

/// What the form needs from the app.
pub struct Hooks {
    pub save: Rc<dyn Fn()>,
    pub cancel: Rc<dyn Fn()>,
}

const DISTRIBUTIONS: [(&str, Option<Distribution>); 3] = [
    (gettext_noop("Not noted"), None),
    (gettext_noop("Direct"), Some(Distribution::Direct)),
    (gettext_noop("Newsstand"), Some(Distribution::Newsstand)),
];

pub(crate) const FORMATS: [(&str, Option<MagazineFormat>); 4] = [
    (gettext_noop("Not noted"), None),
    (
        gettext_noop("Standard size"),
        Some(MagazineFormat::Standard),
    ),
    (
        gettext_noop("Treasury size"),
        Some(MagazineFormat::Treasury),
    ),
    (gettext_noop("Digest size"), Some(MagazineFormat::Digest)),
];

/// Whether a magazine is black and white.
pub(crate) const COLORS: [(&str, Option<bool>); 3] = [
    (gettext_noop("Not noted"), None),
    // Translators: printed in color, as opposed to black and white.
    (gettext_noop("Color"), Some(false)),
    (gettext_noop("Black and white"), Some(true)),
];

const BINDINGS: [(&str, Option<Binding>); 4] = [
    (gettext_noop("Not noted"), None),
    (gettext_noop("Softcover"), Some(Binding::Softcover)),
    (gettext_noop("Hardcover"), Some(Binding::Hardcover)),
    (
        gettext_noop("Oversized hardcover"),
        Some(Binding::OversizedHardcover),
    ),
];

const CONDITIONS: [(&str, Option<BookCondition>); 8] = [
    (gettext_noop("Not noted"), None),
    (gettext_noop("Sealed"), Some(BookCondition::Sealed)),
    // Translators: the condition of a book, never read.
    (gettext_noop("New"), Some(BookCondition::New)),
    (gettext_noop("Like new"), Some(BookCondition::LikeNew)),
    (gettext_noop("Very good"), Some(BookCondition::VeryGood)),
    (gettext_noop("Good"), Some(BookCondition::Good)),
    (gettext_noop("Fair"), Some(BookCondition::Fair)),
    (gettext_noop("Poor"), Some(BookCondition::Poor)),
];

const GRADE_TYPES: [(&str, Option<GradeType>); 5] = [
    (gettext_noop("Not graded"), None),
    (gettext_noop("My own estimate"), Some(GradeType::Raw)),
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
        heading: Option<&str>,
        focus: Focus,
        hooks: Hooks,
    ) -> Self {
        let db = Rc::downgrade(db);
        let window_title = adw::WindowTitle::new(
            &match (heading, target, category) {
                (Some(heading), _, _) => heading.to_owned(),
                (None, None, Category::Comic) => gettext("Add Comic"),
                (None, None, Category::Magazine) => gettext("Add Magazine"),
                (None, None, Category::CollectedEdition) => gettext("Add Collected Edition"),
                (None, Some(_), Category::Comic) => gettext("Edit Comic"),
                (None, Some(_), Category::Magazine) => gettext("Edit Magazine"),
                (None, Some(_), Category::CollectedEdition) => gettext("Edit Collected Edition"),
            },
            "",
        );
        let dirty = Dirty {
            changed: Rc::default(),
            title: window_title.clone(),
        };
        let prefs = adw::PreferencesPage::new();

        // Issue ------------------------------------------------------------
        // Translators: the section about one issue of a series.
        let issue = section(&prefs, &gettext("Issue"), None, true);
        let series = entry_row(&gettext("Series"));
        let picked_series: Rc<RefCell<Option<SeriesId>>> = Rc::default();
        let start_year = entry_row(&gettext("Start Year of New Series"));
        let volume = entry_row(&gettext("Volume of New Series"));
        let publisher = entry_row(&gettext("Publisher of New Series"));
        let imprint = entry_row(&gettext("Imprint of New Series"));
        let magazine_format = combo_row(
            &gettext("Format of New Series"),
            FORMATS.map(|(name, _)| name),
        );
        let color = combo_row(
            &pgettext("whether a magazine is printed in color", "Color"),
            COLORS.map(|(name, _)| name),
        );
        let number = entry_row(&match category {
            Category::CollectedEdition => gettext("Volume Number (blank for a standalone book)"),
            Category::Comic | Category::Magazine => gettext("Issue Number"),
        });
        let joins = adw::ActionRow::builder()
            .title(gettext("Already in your collection"))
            .subtitle(gettext(
                "This adds another copy, and fills in any details typed here.",
            ))
            .visible(false)
            .build();
        joins.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        joins.add_css_class("property");
        let legacy = entry_row(&gettext("Legacy Number"));
        legacy.set_visible(category != Category::CollectedEdition);
        let title = entry_row(&gettext("Story Title"));
        let cover_date = entry_row(&match category {
            Category::CollectedEdition => gettext("Publication Date (e.g. Mar 2014)"),
            Category::Comic | Category::Magazine => gettext("Cover Date (e.g. Mar 1975)"),
        });
        let release_date = entry_row(&gettext("Release Date (YYYY-MM-DD)"));
        let pages = entry_row(&gettext("Page Count"));
        let key = adw::SwitchRow::builder()
            .title(gettext("Key Issue"))
            .build();
        let key_reason = entry_row(&gettext("Why It Is Key"));
        key.bind_property("active", &key_reason, "visible")
            .sync_create()
            .build();
        let synopsis = text_view();
        for row in [&series, &start_year, &volume, &publisher, &imprint] {
            issue.append(row);
        }
        issue.append(&magazine_format);
        issue.append(&color);
        issue.append(&number);
        issue.append(&joins);
        issue.append(&legacy);
        for row in [&title, &cover_date, &release_date, &pages] {
            issue.append(row);
        }
        issue.append(&key);
        issue.append(&key_reason);
        issue.append(&text_row(&gettext("Notes About This Issue"), &synopsis));
        for row in [&start_year, &volume, &publisher, &imprint] {
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
                vec![start_year.clone(), publisher.clone(), imprint.clone()]
            } else {
                vec![
                    start_year.clone(),
                    volume.clone(),
                    publisher.clone(),
                    imprint.clone(),
                ]
            };
            let magazine_rows = [magazine_format.clone(), color.clone()];
            let labels = Rc::new(move || series_labels(&db, category));
            let update = {
                let labels = labels.clone();
                move |text: &str| {
                    let found = labels()
                        .into_iter()
                        .find(|(label, _)| mylongbox_core::same_series_label(text, label));
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
        imprint_completion(&imprint, &publisher, &db);

        let collects = entry_row(&gettext("Issues Reprinted"));
        if category == Category::CollectedEdition {
            let list = section(
                &prefs,
                &gettext("Collects"),
                Some(&gettext(
                    "For example: Amazing Spider-Man (1963) 1\u{2013}10, Annual 1; \
                     Amazing Fantasy (1962) 15",
                )),
                true,
            );
            list.append(&collects);
            contents_preview(&collects, &list, &db);
        }

        let credits = NameList::new(
            &section(
                &prefs,
                &gettext("Credits"),
                Some(&gettext("Writers, artists, and editors")),
                true,
            ),
            gettext_noop("Add Credit"),
            Extra::Role(roles(category)),
            completion::<CreatorId>(&db),
            &dirty,
        );
        let contents = section(
            &prefs,
            &gettext("Characters, Teams, and Story Arcs"),
            Some(&gettext(
                "Mark a character's or team's first appearance with the star.",
            )),
            false,
        );
        let characters = NameList::new(
            &contents,
            gettext_noop("Add Character"),
            Extra::First,
            completion::<CharacterId>(&db),
            &dirty,
        );
        let teams = NameList::new(
            &contents,
            gettext_noop("Add Team"),
            Extra::First,
            completion::<TeamId>(&db),
            &dirty,
        );
        let arcs = NameList::new(
            &contents,
            gettext_noop("Add Story Arc"),
            Extra::Part,
            completion::<StoryArcId>(&db),
            &dirty,
        );

        // Edition ----------------------------------------------------------
        let edition = section(
            &prefs,
            &gettext("Edition"),
            Some(&gettext("Shared by every copy of this cover and printing")),
            focus == Focus::Variant,
        );
        let variant = entry_row(&gettext("Variant (blank for the standard cover)"));
        let printing = entry_row(&gettext("Printing (blank for a first printing)"));
        let distribution = combo_row(
            &gettext("Distribution"),
            DISTRIBUTIONS.map(|(name, _)| name),
        );
        let cover_price = MoneyRow::new(&gettext("Cover Price"));
        // Translators: CA$ marks Canadian dollars.
        let canadian_price = entry_row(&gettext("Canadian Cover Price (CA$)"));
        let barcode = entry_row(&gettext("Barcode"));
        let isbn = entry_row(&gettext("ISBN"));
        let binding = combo_row(&gettext("Binding"), BINDINGS.map(|(name, _)| name));
        let dust_jacket = adw::SwitchRow::builder()
            .title(gettext("Dust Jacket"))
            .build();
        let slipcase = adw::SwitchRow::builder().title(gettext("Slipcase")).build();
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
        edition.append(&canadian_price);
        edition.append(&barcode);
        let cover_artists = NameList::new(
            &edition,
            gettext_noop("Add Cover Artist"),
            Extra::None,
            completion::<CreatorId>(&db),
            &dirty,
        );

        // Copy -------------------------------------------------------------
        let copy = section(&prefs, &gettext("This Copy"), None, true);
        let grade = entry_row(&gettext("Grade (0.5 to 10.0)"));
        let grade_type = combo_row(&gettext("Graded By"), GRADE_TYPES.map(|(name, _)| name));
        let cert_number = entry_row(&gettext("Certificate Number"));
        // Translators: the kind of label a grading company put on the slab.
        let label_type = entry_row(&gettext("Label"));
        let page_quality = page_quality_row();
        let purchase_price = MoneyRow::new(&gettext("Purchase Price"));
        let purchase_date = entry_row(&gettext("Purchase Date (YYYY-MM-DD)"));
        let seller = entry_row(&gettext("Bought From"));
        let storage_box = entry_row(&gettext("Storage Box"));
        let box_position = entry_row(&gettext("Position in Box"));
        lookup_completion::<BoxId>(&storage_box, &db);
        let book_condition = combo_row(&gettext("Condition"), CONDITIONS.map(|(name, _)| name));
        let jacket_condition = combo_row(
            &gettext("Dust Jacket Condition"),
            CONDITIONS.map(|(name, _)| name),
        );
        if category == Category::CollectedEdition {
            copy.append(&book_condition);
            copy.append(&jacket_condition);
        } else {
            copy.append(&grade);
            copy.append(&grade_type);
            copy.append(&cert_number);
            copy.append(&label_type);
            copy.append(&page_quality);
        }
        copy.append(&purchase_price.row);
        for row in [&purchase_date, &seller, &storage_box, &box_position] {
            copy.append(row);
        }
        let tags = NameList::new(
            &copy,
            gettext_noop("Add Tag"),
            Extra::None,
            completion::<TagId>(&db),
            &dirty,
        );
        let notes = text_view();
        copy.append(&text_row(&gettext("Notes on This Copy"), &notes));

        let signed = entry.is_some_and(|e| !e.signatures.is_empty());
        let signatures = SignatureList::new(
            &section(
                &prefs,
                &gettext("Signatures"),
                Some(&gettext("Each signature on this copy, in the order signed")),
                signed,
            ),
            completion::<CreatorId>(&db),
            &dirty,
        );

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
        let cancel = gtk::Button::with_mnemonic(&gettext("_Cancel"));
        let save = gtk::Button::with_mnemonic(&gettext("_Save"));
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
            focus,
            category,
            dirty,
            series,
            picked_series,
            start_year,
            volume,
            publisher,
            imprint,
            magazine_format,
            color,
            number,
            legacy,
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
            canadian_price,
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
            page_quality,
            signatures,
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
        form.focus_when_shown();
        form
    }

    /// Puts the cursor where this kind of form starts once the page has
    /// finished sliding in; before that it has no layout to scroll.
    fn focus_when_shown(&self) {
        let book = self.category == Category::CollectedEdition;
        let field: adw::PreferencesRow = match self.focus {
            Focus::Series => self.series.clone().upcast(),
            Focus::Title => self.title.clone().upcast(),
            Focus::Variant => self.variant.clone().upcast(),
            Focus::Copy if book => self.book_condition.clone().upcast(),
            Focus::Copy => self.grade.clone().upcast(),
            Focus::Nowhere => return,
        };
        self.page.connect_shown(move |_| {
            field.grab_focus();
            // Focus alone leaves a field further down out of sight.
            if let Some(viewport) = field
                .ancestor(gtk::Viewport::static_type())
                .and_downcast::<gtk::Viewport>()
            {
                viewport.scroll_to(&field, None);
            }
        });
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
            &self.imprint,
            &self.number,
            &self.legacy,
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
            &self.purchase_date,
            &self.seller,
            &self.storage_box,
            &self.box_position,
            &self.cover_price.row,
            &self.canadian_price,
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
            &self.page_quality,
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
            self.series.set_text(&mylongbox_core::series_label(
                &series.fields.name,
                series.fields.start_year,
                series.fields.volume,
            ));
        }
        let i = &e.issue;
        self.number
            .set_text(i.number.as_ref().map_or("", |n| n.as_str()));
        self.legacy
            .set_text(&i.legacy_number.map(|n| n.to_string()).unwrap_or_default());
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
        self.canadian_price.set_text(
            &ed.canadian_cover_price_cents
                .map(mylongbox_core::cents_string)
                .unwrap_or_default(),
        );
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
        let quality = c
            .page_quality
            .and_then(|q| PageQuality::ALL.iter().position(|&p| p == q))
            .map_or(0, |i| i + 1);
        self.page_quality
            .set_selected(u32::try_from(quality).unwrap_or(0));
        for (name, fields) in &e.signatures {
            self.signatures.add(name, fields);
        }
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
            return invalid(&self.series, &gettext("Enter a series"));
        }
        let series = match *self.picked_series.borrow() {
            Some(id) => SeriesChoice::Existing(id),
            None => {
                let (name, year_in_name, volume_in_name) =
                    mylongbox_core::parse_series_label(&series_text);
                SeriesChoice::New {
                    name,
                    start_year: optional(
                        &self.start_year,
                        &gettext("Enter the year as four digits"),
                    )?
                    .or(year_in_name),
                    volume: optional(&self.volume, &gettext("Enter the volume as a number"))?
                        .or(volume_in_name),
                    publisher: Some(self.publisher.text().trim().to_owned())
                        .filter(|p| !p.is_empty()),
                    imprint: self.imprint.text().trim().to_owned(),
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
                    &gettext(
                        "Enter the cover date like \u{201c}Mar 1975\u{201d} or \u{201c}1975\u{201d}",
                    ),
                );
            }
        };
        i.release_date = date(&self.release_date)?;
        i.page_count = optional(&self.pages, &gettext("Enter the page count as a number"))?;
        i.legacy_number = match self.legacy.text().trim().trim_start_matches('#').trim() {
            "" => None,
            text => match text.parse() {
                Ok(n) => Some(n),
                Err(_) => {
                    return invalid(
                        &self.legacy,
                        &gettext("Enter the legacy number as a whole number, such as 846"),
                    );
                }
            },
        };
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
        ed.printing = optional(
            &self.printing,
            &gettext("Enter the printing as a number, such as 2"),
        )?;
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
                        return invalid(
                            &self.isbn,
                            &gettext("That ISBN is not valid; check its 13 digits"),
                        );
                    }
                },
            };
            ed.binding = BINDINGS[selected(&self.binding)].1;
            ed.has_dust_jacket = self.dust_jacket.is_active().then_some(true);
            ed.has_slipcase = self.slipcase.is_active().then_some(true);
        }
        ed.cover_price = self.cover_price.amount()?;
        ed.canadian_cover_price_cents = match self
            .canadian_price
            .text()
            .trim()
            .trim_start_matches(['C', 'A', '$'])
        {
            "" => None,
            text => match mylongbox_core::parse_cents(text) {
                Ok(cents) => Some(cents),
                Err(_) => {
                    return invalid(
                        &self.canadian_price,
                        &gettext("Enter the Canadian price in dollars and cents, such as 0.75"),
                    );
                }
            },
        };
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
                        &gettext("Grades run from 0.5 to 10.0, in tenths, such as 9.4"),
                    );
                }
            },
        };
        if !book {
            c.grade_type = GRADE_TYPES[selected(&self.grade_type)].1;
            c.cert_number =
                Some(self.cert_number.text().trim().to_owned()).filter(|t| !t.is_empty());
            c.label_type = Some(self.label_type.text().trim().to_owned()).filter(|t| !t.is_empty());
            c.page_quality = selected(&self.page_quality)
                .checked_sub(1)
                .and_then(|i| PageQuality::ALL.get(i).copied());
        }
        c.purchase_price = self.purchase_price.amount()?;
        c.purchase_date = date(&self.purchase_date)?;
        c.seller = self.seller.text().trim().to_owned();
        e.storage_box = Some(self.storage_box.text().trim().to_owned()).filter(|b| !b.is_empty());
        c.box_position = optional(
            &self.box_position,
            &gettext("Enter the position as a number"),
        )?;
        if c.box_position.is_some() && e.storage_box.is_none() {
            return invalid(
                &self.storage_box,
                &gettext("Name the box this position is in"),
            );
        }
        c.notes = text(&self.notes);
        e.tags = self.tags.values().into_iter().map(|v| v.name).collect();
        e.signatures = self.signatures.values()?;
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
/// A series as a contents list names it: `Amazing Spider-Man (1963)`,
/// the year alone in brackets, which is how the list is read back.
fn contents_series(name: &str, start_year: Option<u16>) -> String {
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
        .map(|s| {
            let f = &s.fields;
            let label = mylongbox_core::series_label(&f.name, f.start_year, f.volume);
            (label, s.id)
        })
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
                    let count = ngettext_f(
                        "{count} issue",
                        "{count} issues",
                        entries.len(),
                        &[("count", &entries.len().to_string())],
                    );
                    let detail = match new {
                        0 => gettext("All are in the catalog already"),
                        n => ngettext_f(
                            "{count} is not in the catalog yet; saving adds it",
                            "{count} are not in the catalog yet; saving adds them",
                            n,
                            &[("count", &n.to_string())],
                        ),
                    };
                    let summary = plain_row(&count, &detail);
                    summary.add_css_class("property");
                    rows.push(summary);
                    for e in entries.iter().take(PREVIEW_MAX) {
                        let title = format!(
                            "{} #{}",
                            mylongbox_core::series_label(
                                &e.entry.series.name,
                                e.entry.series.start_year,
                                None
                            ),
                            e.entry.number
                        );
                        let item = plain_row(&title, "");
                        if e.issue.is_none() {
                            let tag = gtk::Label::builder()
                                .label(pgettext("an issue not yet in the catalog", "New"))
                                .css_classes(["accent", "caption-heading"])
                                .build();
                            item.add_suffix(&tag);
                        }
                        rows.push(item);
                    }
                    if entries.len() > PREVIEW_MAX {
                        let more = entries.len() - PREVIEW_MAX;
                        rows.push(plain_row(
                            &ngettext_f(
                                "and {count} more",
                                "and {count} more",
                                more,
                                &[("count", &more.to_string())],
                            ),
                            "",
                        ));
                    }
                }
                Err(why) => {
                    let error = plain_row(
                        &gettext("This list cannot be read"),
                        &capitalized(&why.to_string()),
                    );
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
        .map(|s| contents_series(&s.fields.name, s.fields.start_year))
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

/// Offers the imprints already in use, those of the publisher in
/// `publisher` first.
pub fn imprint_completion(row: &adw::EntryRow, publisher: &adw::EntryRow, db: &Weak<Database>) {
    let db = db.clone();
    let publisher = publisher.downgrade();
    autocomplete::attach(
        row,
        move |text| {
            let publisher = publisher.upgrade().map(|p| p.text()).unwrap_or_default();
            db.upgrade()
                .and_then(|db| {
                    db.suggest_imprints(&publisher, text, autocomplete::LIMIT as u32 + 1)
                        .ok()
                })
                .unwrap_or_default()
        },
        |_| {},
    );
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
    optional(
        row,
        &gettext("Enter the date as YYYY-MM-DD, such as 1975-03-05"),
    )
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

/// A row choosing among `names`, which are looked up in the reader's
/// language here.
fn combo_row<const N: usize>(title: &str, names: [&str; N]) -> adw::ComboRow {
    let names = names.map(gettext);
    adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&names.each_ref().map(String::as_str)))
        .build()
}

/// A choice of page quality on CGC's scale, or none, which comes first.
fn page_quality_row() -> adw::ComboRow {
    let mut names = vec![gettext("Not recorded")];
    names.extend(
        PageQuality::ALL
            .iter()
            .map(|&q| format::page_quality(q).to_owned()),
    );
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    adw::ComboRow::builder()
        .title(gettext("Page Quality"))
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
        .tooltip_text(gettext_f("Show or Hide {section}", &[("section", title)]))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    {
        let revealer = revealer.clone();
        toggle.connect_clicked(move |button| {
            let open = !revealer.reveals_child();
            revealer.set_reveal_child(open);
            button.set_icon_name(icon(open));
            button.update_state(&[gtk::accessible::State::Expanded(Some(open))]);
        });
    }
    toggle.update_state(&[gtk::accessible::State::Expanded(Some(open))]);
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
        let (gold, bitcoin) = (gettext("Gold (oz)"), gettext("Bitcoin"));
        let currency = gtk::DropDown::from_strings(&["USD", &gold, &bitcoin]);
        currency.set_valign(gtk::Align::Center);
        currency.set_tooltip_text(Some(&gettext("Currency")));
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

/// One signature in the form: who signed, how it is verified, and its
/// details folded away beneath.
struct SignatureRow {
    name: adw::EntryRow,
    verification: gtk::DropDown,
    details: adw::ExpanderRow,
    cert: adw::EntryRow,
    date: adw::EntryRow,
    place: adw::EntryRow,
    sketch: adw::SwitchRow,
    placement: adw::ComboRow,
}

/// Signatures that can be added and removed.
#[derive(Clone)]
struct SignatureList {
    list: gtk::ListBox,
    /// Signatures go in above this row.
    add_button: adw::ButtonRow,
    rows: Rc<RefCell<Vec<SignatureRow>>>,
    suggest: Suggest,
    dirty: Dirty,
}

impl SignatureList {
    fn new(list: &gtk::ListBox, suggest: Suggest, dirty: &Dirty) -> Self {
        let add_button = adw::ButtonRow::builder()
            .title(gettext("Add Signature"))
            .start_icon_name("list-add-symbolic")
            .build();
        list.append(&add_button);
        let signatures = Self {
            list: list.clone(),
            add_button: add_button.clone(),
            rows: Rc::default(),
            suggest,
            dirty: dirty.clone(),
        };
        {
            let signatures = signatures.clone();
            add_button.connect_activated(move |_| {
                let row = signatures.add("", &SignatureFields::new(Verification::Unverified));
                row.grab_focus();
                signatures.dirty.mark();
            });
        }
        signatures
    }

    fn add(&self, name: &str, f: &SignatureFields) -> adw::EntryRow {
        let row = entry_row(&gettext("Signed By"));
        row.set_text(name);
        let suggest = self.suggest.clone();
        autocomplete::attach(&row, move |text| suggest(text), |_| {});
        self.dirty.watch(&row);

        let names: Vec<&str> = Verification::ALL
            .iter()
            .map(|v| format::verification(*v))
            .collect();
        let verification = gtk::DropDown::from_strings(&names);
        verification.set_valign(gtk::Align::Center);
        verification.set_tooltip_text(Some(&gettext("How It Is Verified")));
        let index = Verification::ALL
            .iter()
            .position(|v| *v == f.verification)
            .unwrap_or(0);
        verification.set_selected(u32::try_from(index).unwrap_or(0));
        {
            let d = self.dirty.clone();
            verification.connect_selected_notify(move |_| d.mark());
        }
        row.add_suffix(&verification);

        let details = adw::ExpanderRow::builder()
            .title(gettext("Details"))
            .subtitle(gettext("Where on the book, certificate, date, sketch"))
            .build();
        let mut placements = vec![gettext("Not noted")];
        placements.extend(
            SignaturePlacement::ALL
                .iter()
                .map(|p| format::signature_placement(*p).to_owned()),
        );
        let placements: Vec<&str> = placements.iter().map(String::as_str).collect();
        let placement = adw::ComboRow::builder()
            .title(gettext("Where on the Book"))
            .model(&gtk::StringList::new(&placements))
            .build();
        let at = f
            .placement
            .and_then(|p| SignaturePlacement::ALL.iter().position(|&q| q == p))
            .map_or(0, |i| i + 1);
        placement.set_selected(u32::try_from(at).unwrap_or(0));
        {
            let d = self.dirty.clone();
            placement.connect_selected_notify(move |_| d.mark());
        }
        details.add_row(&placement);
        let cert = entry_row(&gettext("Certificate Number"));
        cert.set_text(f.cert_number.as_deref().unwrap_or(""));
        let date = entry_row(&gettext("Date Signed (YYYY-MM-DD)"));
        date.set_text(&f.signed_on.map(|d| d.to_string()).unwrap_or_default());
        let place = entry_row(&gettext("Signed At (e.g. NYCC 2015)"));
        place.set_text(&f.place);
        let sketch = adw::SwitchRow::builder()
            .title(gettext("With a Sketch"))
            .active(f.sketch)
            .build();
        for field in [&cert, &date, &place] {
            self.dirty.watch(field);
            details.add_row(field);
        }
        {
            let d = self.dirty.clone();
            sketch.connect_active_notify(move |_| d.mark());
        }
        details.add_row(&sketch);

        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Remove Signature"))
            .css_classes(["flat"])
            .build();
        {
            let signatures = self.clone();
            let row = row.clone();
            let details = details.clone();
            remove.connect_clicked(move |_| {
                signatures.list.remove(&row);
                signatures.list.remove(&details);
                signatures.rows.borrow_mut().retain(|r| r.name != row);
                signatures.dirty.mark();
            });
        }
        row.add_suffix(&remove);

        let at = self.add_button.index();
        self.list.insert(&row, at);
        self.list.insert(&details, at + 1);
        self.rows.borrow_mut().push(SignatureRow {
            name: row.clone(),
            verification,
            details,
            cert,
            date,
            place,
            sketch,
            placement,
        });
        row
    }

    fn values(&self) -> Check<Vec<(String, SignatureFields)>> {
        let mut values = Vec::new();
        for r in self.rows.borrow().iter() {
            let name = r.name.text().trim().to_owned();
            if name.is_empty() {
                continue;
            }
            let signed_on = match date(&r.date) {
                Ok(date) => date,
                Err(why) => {
                    // The field is folded away; show it before pointing at it.
                    r.details.set_expanded(true);
                    return Err(why);
                }
            };
            let index = usize::try_from(r.verification.selected()).unwrap_or(0);
            values.push((
                name,
                SignatureFields {
                    verification: Verification::ALL
                        .get(index)
                        .copied()
                        .unwrap_or(Verification::Unverified),
                    cert_number: Some(r.cert.text().trim().to_owned()).filter(|c| !c.is_empty()),
                    signed_on,
                    place: r.place.text().trim().to_owned(),
                    sketch: r.sketch.is_active(),
                    placement: usize::try_from(r.placement.selected())
                        .ok()
                        .and_then(|i| i.checked_sub(1))
                        .and_then(|i| SignaturePlacement::ALL.get(i).copied()),
                },
            ));
        }
        Ok(values)
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
    (gettext_noop("Writer"), Role::Writer),
    (gettext_noop("Penciller"), Role::Penciller),
    (gettext_noop("Inker"), Role::Inker),
    (gettext_noop("Colorist"), Role::Colorist),
    (gettext_noop("Letterer"), Role::Letterer),
    (gettext_noop("Cover Artist"), Role::CoverArtist),
    (gettext_noop("Editor"), Role::Editor),
    (gettext_noop("Article"), Role::Article),
    (gettext_noop("Interview"), Role::Interview),
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
            .title(gettext(add_label))
            .start_icon_name("list-add-symbolic")
            .build();
        list.append(&add_button);
        let placeholder = match extra {
            Extra::Role(_) => gettext_noop("Creator"),
            Extra::First => {
                if add_label.contains("Team") {
                    gettext_noop("Team")
                } else {
                    gettext_noop("Character")
                }
            }
            Extra::Part => gettext_noop("Story Arc"),
            Extra::None => {
                if add_label.contains("Tag") {
                    gettext_noop("Tag")
                } else {
                    gettext_noop("Cover Artist")
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
        let row = entry_row(&gettext(self.placeholder));
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
                let names: Vec<String> = offered.iter().map(|(name, _)| gettext(name)).collect();
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let roles = gtk::DropDown::from_strings(&names);
                roles.set_valign(gtk::Align::Center);
                roles.set_tooltip_text(Some(&gettext("Role")));
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
                    .tooltip_text(gettext("First Appearance"))
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
                spin.set_tooltip_text(Some(&gettext("Part Number (0 if none)")));
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
            .tooltip_text(gettext("Remove"))
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
    fn contents_lists_name_series_by_year_alone() {
        assert_eq!(
            contents_series("Amazing Spider-Man", Some(1963)),
            "Amazing Spider-Man (1963)"
        );
        assert_eq!(contents_series("X-Men", None), "X-Men");
    }
}
