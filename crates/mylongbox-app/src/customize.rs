//! The Customize Sidebar dialog: which built-in entries the sidebar
//! shows, and the order of the owner's shelves.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use mylongbox_core::SavedShelfId;

use crate::i18n::{gettext, gettext_f};

/// A built-in sidebar entry, as the dialog lists it.
pub struct Entry {
    pub code: &'static str,
    /// Already in the reader's language.
    pub name: String,
    /// Always shown, so the way back to the books is never hidden.
    pub locked: bool,
    pub hidden: bool,
}

/// What the dialog's controls ask the app to do.
pub struct Hooks {
    pub set_hidden: Box<dyn Fn(&'static str, bool)>,
    /// Moves a shelf one place earlier, or later.
    pub move_shelf: Box<dyn Fn(SavedShelfId, bool)>,
}

pub fn dialog(
    entries: Vec<Entry>,
    shelves: Vec<(SavedShelfId, String)>,
    hooks: Hooks,
) -> adw::PreferencesDialog {
    let hooks = Rc::new(hooks);
    let page = adw::PreferencesPage::new();
    let shown = adw::PreferencesGroup::builder()
        .title(gettext("Shown in the Sidebar"))
        .description(gettext(
            "Hidden entries are only out of sight; everything in them stays in the collection.",
        ))
        .build();
    for entry in entries {
        let row = adw::SwitchRow::builder()
            .title(&entry.name)
            .active(!entry.hidden || entry.locked)
            .sensitive(!entry.locked)
            .build();
        if entry.locked {
            row.set_subtitle(&gettext("Always shown"));
        }
        let (hooks, code) = (hooks.clone(), entry.code);
        row.connect_active_notify(move |row| (hooks.set_hidden)(code, !row.is_active()));
        shown.add(&row);
    }
    page.add(&shown);

    if !shelves.is_empty() {
        let group = adw::PreferencesGroup::builder()
            .title(gettext("My Shelves"))
            .description(gettext("Shelves are listed in this order, above Trash."))
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        group.add(&list);
        let order = Rc::new(RefCell::new(shelves));
        fill_shelves(&list, &order, &hooks);
        page.add(&group);
    }

    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Customize Sidebar"))
        .build();
    dialog.add(&page);
    dialog
}

/// Lists the shelves in `order`, each with buttons to move it, refilling
/// the list after every move so the buttons at the ends are right.
fn fill_shelves(
    list: &gtk::ListBox,
    order: &Rc<RefCell<Vec<(SavedShelfId, String)>>>,
    hooks: &Rc<Hooks>,
) {
    list.remove_all();
    let count = order.borrow().len();
    for (at, (id, name)) in order.borrow().iter().cloned().enumerate() {
        let row = adw::ActionRow::builder()
            .title(&name)
            .use_markup(false)
            .build();
        let named = [("name", name.as_str())];
        for (earlier, icon, tip, spoken, possible) in [
            (
                true,
                "go-up-symbolic",
                gettext("Move Up"),
                gettext_f("Move \u{201c}{name}\u{201d} Up", &named),
                at > 0,
            ),
            (
                false,
                "go-down-symbolic",
                gettext("Move Down"),
                gettext_f("Move \u{201c}{name}\u{201d} Down", &named),
                at + 1 < count,
            ),
        ] {
            let button = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(&tip)
                .valign(gtk::Align::Center)
                .sensitive(possible)
                .css_classes(["flat"])
                .build();
            // The name read out says which shelf the button moves.
            button.update_property(&[gtk::accessible::Property::Label(&spoken)]);
            let (list, order, hooks) = (list.clone(), order.clone(), hooks.clone());
            button.connect_clicked(move |_| {
                {
                    let mut shelves = order.borrow_mut();
                    let to = if earlier { at - 1 } else { at + 1 };
                    shelves.swap(at, to);
                }
                (hooks.move_shelf)(id, earlier);
                fill_shelves(&list, &order, &hooks);
            });
            row.add_suffix(&button);
        }
        list.append(&row);
    }
}
