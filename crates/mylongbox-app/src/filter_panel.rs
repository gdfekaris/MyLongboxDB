//! The filter panel: rows of field, condition, and value, added with a
//! button, each changing the results as soon as it reads as a filter.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use mylongbox_core::{FilterSet, OwnedAs, SeriesId};

use crate::filter_fields::{FIELDS, Inputs, Kind, Shape, build, unbuild};
use crate::i18n::{gettext, gettext_noop};

/// How long typing must pause before the books are filtered again.
const TYPING_PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

const OWNED_AS: [(&str, OwnedAs); 3] = [
    (gettext_noop("Owned as Single Issues"), OwnedAs::SingleIssue),
    (
        gettext_noop("Owned in Collected Editions"),
        OwnedAs::CollectedEdition,
    ),
    (gettext_noop("Owned Either Way"), OwnedAs::Either),
];

/// `labels` in the reader's language, for a menu or the like.
fn translated(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|l| gettext(l)).collect()
}

/// A menu, searchable if asked, of `labels` in the reader's language.
fn menu(labels: &[&str], searchable: bool) -> gtk::DropDown {
    let shown = translated(labels);
    let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
    shown_menu(&shown, searchable)
}

/// A menu, searchable if asked, of text already in the reader's
/// language.
fn shown_menu(shown: &[&str], searchable: bool) -> gtk::DropDown {
    let dropdown = gtk::DropDown::from_strings(shown);
    if searchable {
        dropdown.set_enable_search(true);
        dropdown.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        )));
    }
    dropdown.set_valign(gtk::Align::Center);
    dropdown
}

fn selected(dropdown: &gtk::DropDown) -> usize {
    usize::try_from(dropdown.selected()).unwrap_or(0)
}

/// One row: field, condition, its values, and a button to remove it.
struct Row {
    widget: gtk::Box,
    field: gtk::DropDown,
    condition: gtk::DropDown,
    values: gtk::Box,
    entries: RefCell<Vec<gtk::Entry>>,
    menu: RefCell<Option<gtk::DropDown>>,
    checks: RefCell<Vec<gtk::CheckButton>>,
    series: SeriesLookup,
}

/// Finds a series by the name it is listed under.
pub type SeriesLookup = Rc<dyn Fn(&str) -> Option<SeriesId>>;

impl Row {
    fn kind(&self) -> Kind {
        FIELDS
            .get(selected(&self.field))
            .map_or(FIELDS[0].kind, |f| f.kind)
    }

    fn inputs(&self) -> Inputs {
        Inputs {
            texts: self
                .entries
                .borrow()
                .iter()
                .map(|e| e.text().into())
                .collect(),
            menu: self.menu.borrow().as_ref().map_or(0, selected),
            checks: self
                .checks
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_active())
                .map(|(i, _)| i)
                .collect(),
        }
    }
}

pub struct FilterPanel {
    /// Itself, for the buttons of rows to reach it without keeping it
    /// alive.
    myself: RefCell<Weak<FilterPanel>>,
    root: gtk::Box,
    list: gtk::Box,
    rows: RefCell<Vec<Rc<Row>>>,
    owned_as: gtk::DropDown,
    changed: Rc<dyn Fn()>,
    /// Holds back `changed` while rows are filled in all at once.
    quiet: Rc<Cell<bool>>,
    series: SeriesLookup,
    save: gtk::Button,
}

impl FilterPanel {
    /// A panel that calls `changed` whenever what it filters by may have
    /// changed, finds series for the Collects field with `series`, and
    /// calls `save` when its filters are to be kept as a shelf.
    pub fn new(
        changed: impl Fn() + 'static,
        series: SeriesLookup,
        save: impl Fn() + 'static,
    ) -> Rc<Self> {
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .min_content_height(0)
            .max_content_height(240)
            .vexpand(false)
            .build();
        let add = gtk::Button::builder()
            .label(gettext("_Add Filter"))
            .use_underline(true)
            .build();
        let owned_as = menu(&OWNED_AS.map(|(label, _)| label), false);
        owned_as.set_tooltip_text(Some(&gettext(
            "Whether issue fields find the issues themselves or books reprinting them",
        )));
        let clear = gtk::Button::builder()
            .label(gettext("_Clear"))
            .use_underline(true)
            .css_classes(["flat"])
            .build();
        let save_button = gtk::Button::builder()
            .label(gettext("Add to _Sidebar\u{2026}"))
            .use_underline(true)
            .tooltip_text(gettext("Keep These Filters as a Shelf in the Sidebar"))
            .sensitive(false)
            .build();
        save_button.connect_clicked(move |_| save());
        let bar = gtk::Box::builder().spacing(6).build();
        bar.append(&add);
        bar.append(&owned_as);
        let spacer = gtk::Box::builder().hexpand(true).build();
        bar.append(&spacer);
        bar.append(&clear);
        bar.append(&save_button);
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        root.append(&scroller);
        root.append(&bar);
        let quiet = Rc::new(Cell::new(false));
        let changed: Rc<dyn Fn()> = {
            let quiet = quiet.clone();
            Rc::new(move || {
                if !quiet.get() {
                    changed();
                }
            })
        };
        let panel = Rc::new(Self {
            myself: RefCell::new(Weak::new()),
            root,
            list,
            rows: RefCell::new(Vec::new()),
            owned_as: owned_as.clone(),
            changed,
            quiet,
            series,
            save: save_button,
        });
        panel.myself.replace(Rc::downgrade(&panel));
        {
            let weak = Rc::downgrade(&panel);
            add.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.add_row();
                }
            });
        }
        {
            let weak = Rc::downgrade(&panel);
            clear.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.clear();
                }
            });
        }
        {
            let changed = panel.changed.clone();
            owned_as.connect_selected_notify(move |_| changed());
        }
        panel.add_row();
        panel
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The filters the panel's complete rows stand for.
    pub fn filter_set(&self) -> FilterSet {
        FilterSet {
            rules: self
                .rows
                .borrow()
                .iter()
                .filter_map(|row| {
                    build(
                        row.kind(),
                        selected(&row.condition),
                        &row.inputs(),
                        &*self.series,
                    )
                })
                .collect(),
            owned_as: OWNED_AS
                .get(selected(&self.owned_as))
                .map_or(OwnedAs::SingleIssue, |o| o.1),
        }
    }

    /// Fills the panel with `filters`, one row for each rule, as for
    /// editing a shelf. `series_label` names a series as the Collects
    /// field lists it; a rule that cannot be shown is left out.
    pub fn set_filter_set(
        &self,
        filters: &FilterSet,
        series_label: &dyn Fn(SeriesId) -> Option<String>,
    ) {
        self.quiet.set(true);
        for row in self.rows.take() {
            self.list.remove(&row.widget);
        }
        for rule in &filters.rules {
            let Some((field, condition, inputs)) = unbuild(rule, series_label) else {
                continue;
            };
            let row = self.add_row_quietly();
            // Each choice rebuilds what follows it, so they go in order.
            row.field.set_selected(u32::try_from(field).unwrap_or(0));
            row.condition
                .set_selected(u32::try_from(condition).unwrap_or(0));
            for (entry, text) in row.entries.borrow().iter().zip(&inputs.texts) {
                entry.set_text(text);
            }
            if let Some(menu) = row.menu.borrow().as_ref() {
                menu.set_selected(u32::try_from(inputs.menu).unwrap_or(0));
            }
            for (i, check) in row.checks.borrow().iter().enumerate() {
                check.set_active(inputs.checks.contains(&i));
            }
        }
        if self.rows.borrow().is_empty() {
            self.add_row_quietly();
        }
        let owned_as = OWNED_AS
            .iter()
            .position(|(_, o)| *o == filters.owned_as)
            .unwrap_or(0);
        self.owned_as
            .set_selected(u32::try_from(owned_as).unwrap_or(0));
        self.quiet.set(false);
        (self.changed)();
    }

    /// Words the save button: adding a new shelf, or updating the one
    /// being edited.
    pub fn set_save(&self, label: &str, tooltip: &str, sensitive: bool) {
        self.save.set_label(label);
        self.save.set_tooltip_text(Some(tooltip));
        self.save.set_sensitive(sensitive);
    }

    /// Back to one empty row, owned as single issues.
    pub fn clear(&self) {
        for row in self.rows.take() {
            self.list.remove(&row.widget);
        }
        self.owned_as.set_selected(0);
        self.add_row_quietly();
        (self.changed)();
    }

    fn add_row(&self) {
        let row = self.add_row_quietly();
        row.field.grab_focus();
    }

    fn add_row_quietly(&self) -> Rc<Row> {
        let labels: Vec<&str> = FIELDS.iter().map(|f| f.label).collect();
        let field = menu(&labels, true);
        let condition = menu(FIELDS[0].kind.conditions(), false);
        let values = gtk::Box::builder().spacing(6).hexpand(true).build();
        let remove = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text(gettext("Remove This Filter"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let widget = gtk::Box::builder().spacing(6).build();
        widget.append(&field);
        widget.append(&condition);
        widget.append(&values);
        widget.append(&remove);
        self.list.append(&widget);
        let row = Rc::new(Row {
            widget,
            field: field.clone(),
            condition: condition.clone(),
            values,
            entries: RefCell::new(Vec::new()),
            menu: RefCell::new(None),
            checks: RefCell::new(Vec::new()),
            series: self.series.clone(),
        });
        rebuild_values(&row, &self.changed);
        self.rows.borrow_mut().push(row.clone());

        let changed = self.changed.clone();
        let weak: Weak<Row> = Rc::downgrade(&row);
        field.connect_selected_notify(move |field| {
            let Some(row) = weak.upgrade() else {
                return;
            };
            let kind = FIELDS
                .get(selected(field))
                .map_or(FIELDS[0].kind, |f| f.kind);
            let conditions = translated(kind.conditions());
            let conditions: Vec<&str> = conditions.iter().map(String::as_str).collect();
            row.condition
                .set_model(Some(&gtk::StringList::new(&conditions)));
            row.condition.set_selected(0);
            rebuild_values(&row, &changed);
            changed();
        });
        let changed = self.changed.clone();
        let weak = Rc::downgrade(&row);
        condition.connect_selected_notify(move |_| {
            if let Some(row) = weak.upgrade() {
                rebuild_values(&row, &changed);
                changed();
            }
        });
        let panel = self.myself.borrow().clone();
        let removed = Rc::downgrade(&row);
        remove.connect_clicked(move |_| {
            if let (Some(panel), Some(row)) = (panel.upgrade(), removed.upgrade()) {
                panel.list.remove(&row.widget);
                panel.rows.borrow_mut().retain(|r| !Rc::ptr_eq(r, &row));
                if panel.rows.borrow().is_empty() {
                    panel.add_row_quietly();
                }
                (panel.changed)();
            }
        });
        row
    }
}

/// Fills a row's value area with what its field and condition ask for.
fn rebuild_values(row: &Rc<Row>, changed: &Rc<dyn Fn()>) {
    while let Some(child) = row.values.first_child() {
        row.values.remove(&child);
    }
    row.entries.borrow_mut().clear();
    row.menu.replace(None);
    row.checks.borrow_mut().clear();
    let entry = |placeholder: &str| {
        let entry = gtk::Entry::builder()
            .placeholder_text(gettext(placeholder))
            .hexpand(true)
            .width_chars(8)
            .valign(gtk::Align::Center)
            .build();
        let (changed, weak) = (changed.clone(), Rc::downgrade(row));
        // Each change bumps this; the books are filtered again only if
        // nothing newer was typed during the pause.
        let latest = Rc::new(Cell::new(0u64));
        entry.connect_changed(move |_| {
            if let Some(row) = weak.upgrade() {
                mark_unreadable(&row);
            }
            let ticket = latest.get() + 1;
            latest.set(ticket);
            let (changed, latest) = (changed.clone(), latest.clone());
            glib::timeout_add_local_once(TYPING_PAUSE, move || {
                if latest.get() == ticket {
                    changed();
                }
            });
        });
        row.values.append(&entry);
        row.entries.borrow_mut().push(entry);
    };
    let dropdown = |options: &[&str]| {
        let dropdown = shown_menu(options, false);
        let changed = changed.clone();
        dropdown.connect_selected_notify(move |_| changed());
        row.values.append(&dropdown);
        row.menu.replace(Some(dropdown));
    };
    match row.kind().shape(selected(&row.condition)) {
        Shape::Nothing => {}
        Shape::Entries(placeholders) => placeholders.iter().for_each(|p| entry(p)),
        Shape::EntriesAndMenu(placeholders, options) => {
            placeholders.iter().for_each(|p| entry(p));
            dropdown(&options);
        }
        Shape::Menu(options) => dropdown(&options),
        Shape::Checks(options) => {
            let choices = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(2)
                .build();
            let button = gtk::MenuButton::builder()
                .label(gettext("Choose\u{2026}"))
                .popover(&gtk::Popover::builder().child(&choices).build())
                .valign(gtk::Align::Center)
                .build();
            for option in options {
                let check = gtk::CheckButton::with_label(option);
                let (changed, weak, button) = (changed.clone(), Rc::downgrade(row), button.clone());
                check.connect_toggled(move |_| {
                    if let Some(row) = weak.upgrade() {
                        button.set_label(&checked_labels(&row));
                    }
                    changed();
                });
                choices.append(&check);
                row.checks.borrow_mut().push(check);
            }
            row.values.append(&button);
        }
    }
}

/// The ticked options, for the button that shows them.
fn checked_labels(row: &Row) -> String {
    let picked: Vec<String> = row
        .checks
        .borrow()
        .iter()
        .filter(|c| c.is_active())
        .filter_map(|c| c.label().map(String::from))
        .collect();
    if picked.is_empty() {
        gettext("Choose\u{2026}")
    } else {
        picked.join(", ")
    }
}

/// Marks a row's boxes when something is typed but does not read, so a
/// row that changes nothing says why.
fn mark_unreadable(row: &Row) {
    let inputs = row.inputs();
    let typed = inputs.texts.iter().any(|t| !t.trim().is_empty());
    let reads = build(row.kind(), selected(&row.condition), &inputs, &*row.series).is_some();
    for entry in row.entries.borrow().iter() {
        if typed && !reads {
            entry.add_css_class("warning");
        } else {
            entry.remove_css_class("warning");
        }
    }
}
