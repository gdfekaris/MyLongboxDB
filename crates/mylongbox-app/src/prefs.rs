//! The Preferences window: how amounts and grades are shown, the
//! collections kept by the app, backups and upkeep, and the
//! exchange-rate table.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{Amount, CollectionInfo, Currency, Database, ExchangeRate, SafetyCopy};
use mylongbox_media::{LONG_EDGES, QUALITIES};

use crate::format;
use crate::settings::Settings;

const CURRENCIES: [(&str, Option<Currency>); 4] = [
    ("As entered", None),
    ("US dollars", Some(Currency::Usd)),
    ("Gold (troy ounces)", Some(Currency::Gold)),
    ("Bitcoin", Some(Currency::Btc)),
];

/// What the window's controls ask the app to do.
pub struct Hooks {
    pub settings_changed: Box<dyn Fn(Settings)>,
    pub rates_changed: Rc<dyn Fn()>,
    pub open: Box<dyn Fn(String)>,
    pub rename: Box<dyn Fn(String)>,
    pub delete: Box<dyn Fn(String)>,
    /// Runs a window action, such as importing a collection.
    pub action: Box<dyn Fn(&str)>,
    pub backup_folder_chosen: Box<dyn Fn(PathBuf)>,
    /// Brings back the named collection's safety copy.
    pub restore_safety: Box<dyn Fn(String)>,
}

pub struct State<'a> {
    pub settings: Settings,
    pub collections: &'a [CollectionInfo],
    /// The collection open now.
    pub current: Option<&'a str>,
    pub db: Option<&'a Rc<Database>>,
    /// Opens on the exchange-rate page, as from the reminder banner.
    pub show_rates: bool,
    pub backup_folder: Option<PathBuf>,
    /// The open collection's safety copy, if it has one.
    pub safety: Option<SafetyCopy>,
}

pub fn dialog(state: &State<'_>, hooks: Hooks) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    let hooks = Rc::new(hooks);
    let general = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();
    // Each group changes its own part of the settings and reports them
    // whole.
    let settings = Rc::new(Cell::new(state.settings));
    general.add(&display_group(&settings, &hooks));
    general.add(&images_group(&settings, &hooks));
    general.add(&collections_group(state, &hooks, &dialog));
    dialog.add(&general);

    let backups = adw::PreferencesPage::builder()
        .title("Backups")
        .icon_name("drive-harddisk-symbolic")
        .build();
    backups.add(&backup_group(state, &hooks, &dialog));
    backups.add(&safety_group(state, &settings, &hooks, &dialog));
    if let Some(db) = state.db {
        backups.add(&upkeep_group(db, &dialog));
    }
    dialog.add(&backups);

    let rates = adw::PreferencesPage::builder()
        .title("Exchange Rates")
        .icon_name("view-list-symbolic")
        .build();
    rates.add(&rates_group(state.db, &hooks));
    dialog.add(&rates);
    if state.show_rates {
        dialog.set_visible_page(&rates);
    }
    dialog
}

fn display_group(current: &Rc<Cell<Settings>>, hooks: &Rc<Hooks>) -> adw::PreferencesGroup {
    let settings = current.get();
    let group = adw::PreferencesGroup::builder().title("Display").build();
    let currency = adw::ComboRow::builder()
        .title("Show Amounts In")
        .subtitle("Amounts in another currency are converted at your exchange rates")
        .model(&gtk::StringList::new(&CURRENCIES.map(|(name, _)| name)))
        .build();
    let index = CURRENCIES
        .iter()
        .position(|(_, c)| *c == settings.currency)
        .unwrap_or(0);
    currency.set_selected(u32::try_from(index).unwrap_or(0));
    let names = adw::SwitchRow::builder()
        .title("Show Grade Names")
        .subtitle("For example 9.4 NM rather than 9.4")
        .active(settings.grade_names)
        .build();
    let changed = {
        let (currency, names, hooks) = (currency.clone(), names.clone(), hooks.clone());
        let current = current.clone();
        Rc::new(move || {
            let index = usize::try_from(currency.selected()).unwrap_or(0);
            let settings = Settings {
                currency: CURRENCIES.get(index).and_then(|c| c.1),
                grade_names: names.is_active(),
                ..current.get()
            };
            current.set(settings);
            (hooks.settings_changed)(settings);
        })
    };
    let c = changed.clone();
    currency.connect_selected_notify(move |_| c());
    names.connect_active_notify(move |_| changed());
    group.add(&currency);
    group.add(&names);
    group
}

/// How cover photos are saved. Photos already in a collection keep the
/// size and quality they were saved with.
fn images_group(current: &Rc<Cell<Settings>>, hooks: &Rc<Hooks>) -> adw::PreferencesGroup {
    let photos = current.get().photos;
    let group = adw::PreferencesGroup::builder()
        .title("Cover Photos")
        .description("Applies to photos added from now on.")
        .build();
    let spin = |title: &str, subtitle: &str, range: (f64, f64), step: f64, value: f64| {
        adw::SpinRow::builder()
            .title(title)
            .subtitle(subtitle)
            .adjustment(&gtk::Adjustment::new(
                value, range.0, range.1, step, step, 0.0,
            ))
            .build()
    };
    let edge = spin(
        "Longest Edge",
        "In pixels; larger photos show more detail but make the collection bigger",
        (f64::from(*LONG_EDGES.start()), f64::from(*LONG_EDGES.end())),
        100.0,
        f64::from(photos.long_edge),
    );
    let quality = spin(
        "JPEG Quality",
        "Higher keeps more detail in larger files",
        (f64::from(*QUALITIES.start()), f64::from(*QUALITIES.end())),
        5.0,
        f64::from(photos.quality),
    );
    let changed = {
        let (edge, quality, hooks) = (edge.clone(), quality.clone(), hooks.clone());
        let current = current.clone();
        Rc::new(move || {
            let mut settings = current.get();
            // The rows keep their values within the ranges given above.
            settings.photos.long_edge = edge.value().round() as u32;
            settings.photos.quality = quality.value().round() as u8;
            current.set(settings);
            (hooks.settings_changed)(settings);
        })
    };
    let c = changed.clone();
    edge.connect_value_notify(move |_| c());
    quality.connect_value_notify(move |_| changed());
    group.add(&edge);
    group.add(&quality);
    group
}

fn collections_group(
    state: &State<'_>,
    hooks: &Rc<Hooks>,
    dialog: &adw::PreferencesDialog,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Collections")
        .description(
            "Collections are kept in this app\u{2019}s own folder. Import and export move \
             copies in and out.",
        )
        .build();
    for c in state.collections {
        let current = state.current == Some(c.name.as_str());
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(&c.name);
        let size = glib::format_size(c.size_bytes);
        row.set_subtitle(&if current {
            format!("Open now \u{b7} {size}")
        } else {
            size.to_string()
        });
        let button = |icon: &str, tip: &str| {
            gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tip)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build()
        };
        // Each choice closes the window first, since it changes what the
        // window lists.
        let wire = |b: &gtk::Button, hook: fn(&Hooks, String)| {
            let (hooks, dialog, name) = (hooks.clone(), dialog.clone(), c.name.clone());
            b.connect_clicked(move |_| {
                dialog.close();
                hook(&hooks, name.clone());
            });
        };
        if !current {
            let open = gtk::Button::builder()
                .label("_Open")
                .use_underline(true)
                .valign(gtk::Align::Center)
                .build();
            wire(&open, |h, n| (h.open)(n));
            row.add_suffix(&open);
        }
        let rename = button("document-edit-symbolic", "Rename");
        wire(&rename, |h, n| (h.rename)(n));
        row.add_suffix(&rename);
        if !current {
            let delete = button("user-trash-symbolic", "Delete");
            wire(&delete, |h, n| (h.delete)(n));
            row.add_suffix(&delete);
        }
        group.add(&row);
    }
    for (title, action) in [
        ("New Collection\u{2026}", "new-collection"),
        ("Import Collection File\u{2026}", "import-collection"),
        ("Export a Copy\u{2026}", "export-collection"),
    ] {
        let row = adw::ButtonRow::builder().title(title).build();
        let (hooks, dialog) = (hooks.clone(), dialog.clone());
        row.connect_activated(move |_| {
            dialog.close();
            (hooks.action)(action);
        });
        if action == "export-collection" && state.current.is_none() {
            row.set_sensitive(false);
        }
        group.add(&row);
    }
    group
}

/// A folder's name as the file chooser showed it; folders reached
/// through the document portal have long made-up paths above it.
fn folder_name(folder: &std::path::Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn backup_group(
    state: &State<'_>,
    hooks: &Rc<Hooks>,
    dialog: &adw::PreferencesDialog,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Back Up")
        .description(
            "A backup is a complete copy of the open collection, checked before it is \
             written. Import it to use it. Old backups are never removed.",
        )
        .build();
    let folder = adw::ActionRow::builder()
        .title("Backup Folder")
        .subtitle(state.backup_folder.as_deref().map_or_else(
            || "Asked for the first time you back up".to_owned(),
            folder_name,
        ))
        .use_markup(false)
        .build();
    let choose = gtk::Button::builder()
        .label("_Choose\u{2026}")
        .use_underline(true)
        .valign(gtk::Align::Center)
        .build();
    {
        let (hooks, dialog, row) = (hooks.clone(), dialog.clone(), folder.clone());
        choose.connect_clicked(move |_| {
            let chooser = gtk::FileDialog::builder()
                .title("Choose a Folder for Backups")
                .modal(true)
                .build();
            let (hooks, row) = (hooks.clone(), row.clone());
            let parent = dialog.root().and_downcast::<gtk::Window>();
            chooser.select_folder(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
                if let Ok(file) = result
                    && let Some(path) = file.path()
                {
                    row.set_subtitle(&folder_name(&path));
                    (hooks.backup_folder_chosen)(path);
                }
            });
        });
    }
    folder.add_suffix(&choose);
    group.add(&folder);
    let now = adw::ButtonRow::builder()
        .title("Back _Up Now")
        .use_underline(true)
        .build();
    now.set_sensitive(state.current.is_some());
    {
        let (hooks, dialog) = (hooks.clone(), dialog.clone());
        now.connect_activated(move |_| {
            dialog.close();
            (hooks.action)("back-up");
        });
    }
    group.add(&now);
    group
}

/// When a file was last changed, in local time.
fn made_at(time: std::time::SystemTime) -> String {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| glib::DateTime::from_unix_local(i64::try_from(d.as_secs()).ok()?).ok())
        .and_then(|t| t.format("%-d %B %Y, %H:%M").ok())
        .map_or_else(String::new, |t| t.to_string())
}

fn safety_group(
    state: &State<'_>,
    current: &Rc<Cell<Settings>>,
    hooks: &Rc<Hooks>,
    dialog: &adw::PreferencesDialog,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Safety Copies")
        .description(
            "Kept in this app\u{2019}s own folder, one for each collection, each replacing \
             the last.",
        )
        .build();
    let switch = adw::SwitchRow::builder()
        .title("Keep a Safety Copy")
        .subtitle(
            "Before importing books, upgrading a collection, changing several books at \
             once, and deleting from Trash",
        )
        .active(current.get().safety_copies)
        .build();
    {
        let (current, hooks) = (current.clone(), hooks.clone());
        switch.connect_active_notify(move |row| {
            let settings = Settings {
                safety_copies: row.is_active(),
                ..current.get()
            };
            current.set(settings);
            (hooks.settings_changed)(settings);
        });
    }
    group.add(&switch);
    if let Some(name) = state.current {
        let row = adw::ActionRow::builder()
            .title(format!("Safety Copy of \u{201c}{name}\u{201d}"))
            .use_markup(false)
            .build();
        match &state.safety {
            Some(copy) => {
                row.set_subtitle(&format!(
                    "Made {} {} \u{b7} {}",
                    made_at(copy.made),
                    copy.reason,
                    glib::format_size(copy.size_bytes)
                ));
                let open = gtk::Button::builder()
                    .label("_Open")
                    .use_underline(true)
                    .tooltip_text("Open it as a collection of its own")
                    .valign(gtk::Align::Center)
                    .build();
                let (hooks, dialog, name) = (hooks.clone(), dialog.clone(), name.to_owned());
                open.connect_clicked(move |_| {
                    dialog.close();
                    (hooks.restore_safety)(name.clone());
                });
                row.add_suffix(&open);
            }
            None => row.set_subtitle("None made yet"),
        }
        group.add(&row);
    }
    group
}

/// Checking the open collection for damage, and compacting it.
fn upkeep_group(db: &Rc<Database>, dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Upkeep")
        .description("For the open collection. Both can take a while for a large one.")
        .build();
    let row = |title: &str, subtitle: &str, label: &str| {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        let button = gtk::Button::builder()
            .label(label)
            .use_underline(true)
            .valign(gtk::Align::Center)
            .build();
        row.add_suffix(&button);
        group.add(&row);
        button
    };
    let check = row(
        "Check for Damage",
        "Reads the whole collection, as a disk fault can harm files unnoticed",
        "_Check",
    );
    {
        let (db, dialog) = (Rc::downgrade(db), dialog.clone());
        check.connect_clicked(move |_| {
            let Some(db) = db.upgrade() else {
                return;
            };
            match db.integrity_check() {
                Ok(problems) if problems.is_empty() => {
                    dialog.add_toast(adw::Toast::new("No damage found"));
                }
                Ok(problems) => {
                    let mut body = String::from(
                        "Open a backup or the safety copy to get back to a sound copy. What was \
                         found:\n\n",
                    );
                    for p in problems.iter().take(10) {
                        body.push_str(&format!("\u{2022} {p}\n"));
                    }
                    if problems.len() > 10 {
                        body.push_str(&format!("and {} more", problems.len() - 10));
                    }
                    let alert =
                        adw::AlertDialog::new(Some("The Collection Is Damaged"), Some(&body));
                    alert.add_response("close", "_Close");
                    alert.present(Some(&dialog));
                }
                Err(e) => dialog.add_toast(adw::Toast::new(&format!("Could not check: {e}"))),
            }
        });
    }
    let compact = row(
        "Compact",
        "Gives back the space deleted books and photos leave behind",
        "C_ompact",
    );
    {
        let (db, dialog) = (Rc::downgrade(db), dialog.clone());
        compact.connect_clicked(move |_| {
            let Some(db) = db.upgrade() else {
                return;
            };
            let message = match db.compact() {
                Ok((before, after)) if after < before => format!(
                    "Compacted from {} to {}",
                    glib::format_size(before),
                    glib::format_size(after)
                ),
                Ok((_, after)) => format!("Already compact at {}", glib::format_size(after)),
                Err(e) => format!("Could not compact: {e}"),
            };
            dialog.add_toast(adw::Toast::new(&message));
        });
    }
    group
}

fn rates_group(db: Option<&Rc<Database>>, hooks: &Rc<Hooks>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Exchange Rates")
        .description(
            "The app cannot look prices up. Enter what an ounce of gold and one bitcoin \
             cost in US dollars on a date; each amount is converted at the newest rate on \
             or before its own date. Rates are kept with the collection.",
        )
        .build();
    let Some(db) = db else {
        let none = adw::ActionRow::builder()
            .title("Open a collection to edit its rates")
            .build();
        group.add(&none);
        return group;
    };
    let rates = Rc::new(RateList {
        rows: gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build(),
        db: Rc::downgrade(db),
        hooks: hooks.clone(),
    });
    rates.refill();
    group.add(&rates.rows);
    // The buttons hold the list weakly; the group holds it for as long
    // as the window shows it.
    group.connect_destroy(move |_| {
        let _ = &rates;
    });
    group
}

/// The rates on show, which redraw themselves after a change.
struct RateList {
    rows: gtk::ListBox,
    db: Weak<Database>,
    hooks: Rc<Hooks>,
}

impl RateList {
    /// Lists the rates newest first, each removable, then a way to add
    /// one. Buttons hold the list weakly, so closing the window frees it.
    fn refill(self: &Rc<Self>) {
        self.rows.remove_all();
        let Some(db) = self.db.upgrade() else {
            return;
        };
        for rate in db.exchange_rates().unwrap_or_default() {
            let row = adw::ActionRow::builder()
                .title(format::date(rate.date()))
                .subtitle(format!(
                    "Gold {} an ounce \u{b7} Bitcoin {}",
                    format::grouped(&rate.usd_per_ounce().to_string()),
                    format::grouped(&rate.usd_per_bitcoin().to_string())
                ))
                .build();
            let delete = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text("Delete This Rate")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let (me, date) = (Rc::downgrade(self), rate.date());
            delete.connect_clicked(move |_| {
                if let Some(me) = me.upgrade() {
                    me.change(|db| db.delete_exchange_rate(date));
                }
            });
            row.add_suffix(&delete);
            self.rows.append(&row);
        }
        let add = adw::ButtonRow::builder()
            .title("Add Rate\u{2026}")
            .start_icon_name("list-add-symbolic")
            .build();
        let me = Rc::downgrade(self);
        add.connect_activated(move |row| {
            let me = me.clone();
            ask_rate(row, move |rate| {
                if let Some(me) = me.upgrade() {
                    me.change(|db| db.set_exchange_rate(rate));
                }
            });
        });
        self.rows.append(&add);
    }

    fn change(self: &Rc<Self>, f: impl FnOnce(&Database) -> mylongbox_core::Result<()>) {
        if let Some(db) = self.db.upgrade()
            && f(&db).is_ok()
        {
            (self.hooks.rates_changed)();
            self.refill();
        }
    }
}

/// Asks for one day's rates; Add is enabled once all three read.
fn ask_rate(parent: &impl IsA<gtk::Widget>, done: impl Fn(ExchangeRate) + 'static) {
    let entry = |title: &str| adw::EntryRow::builder().title(title).build();
    let date = entry("Date (YYYY-MM-DD)");
    date.set_text(&format::today().map(|d| d.to_string()).unwrap_or_default());
    let gold = entry("US Dollars per Ounce of Gold");
    let bitcoin = entry("US Dollars per Bitcoin");
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&date);
    list.append(&gold);
    list.append(&bitcoin);
    let read = {
        let (date, gold, bitcoin) = (date.clone(), gold.clone(), bitcoin.clone());
        move || -> Option<ExchangeRate> {
            let usd = |row: &adw::EntryRow| {
                let cleaned: String = row
                    .text()
                    .trim()
                    .trim_start_matches('$')
                    .chars()
                    .filter(|c| *c != ',')
                    .collect();
                Amount::parse(Currency::Usd, &cleaned).ok()
            };
            ExchangeRate::new(
                date.text().trim().parse().ok()?,
                usd(&gold)?,
                usd(&bitcoin)?,
            )
            .ok()
        }
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Add Exchange Rate")
        .body("A rate already entered for the same date is replaced.")
        .extra_child(&list)
        .focus_widget(&gold)
        .default_response("add")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "_Cancel"), ("add", "_Add")]);
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("add", false);
    {
        let weak = dialog.downgrade();
        let read = read.clone();
        let check = Rc::new(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("add", read().is_some());
            }
        });
        for row in [&date, &gold, &bitcoin] {
            let c = check.clone();
            row.connect_changed(move |_| c());
        }
    }
    dialog.choose(Some(parent), None::<&gio::Cancellable>, move |response| {
        if response == "add"
            && let Some(rate) = read()
        {
            done(rate);
        }
    });
}
