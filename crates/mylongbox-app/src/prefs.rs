//! The Preferences window: how amounts and grades are shown, the
//! collections kept by the app, and the exchange-rate table.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{Amount, CollectionInfo, Currency, Database, ExchangeRate};
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
}

pub struct State<'a> {
    pub settings: Settings,
    pub collections: &'a [CollectionInfo],
    /// The collection open now.
    pub current: Option<&'a str>,
    pub db: Option<&'a Rc<Database>>,
    /// Opens on the exchange-rate page, as from the reminder banner.
    pub show_rates: bool,
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
