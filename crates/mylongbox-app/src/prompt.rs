//! Small dialogs that ask for a name, or manage a list of names.

use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use crate::autocomplete;
use crate::i18n::{gettext, gettext_f};

/// Names to offer for what has been typed so far.
pub type Suggest = Rc<dyn Fn(&str) -> Vec<String>>;

/// What a text prompt says and offers.
pub struct Ask<'a> {
    pub heading: &'a str,
    pub body: &'a str,
    pub field: &'a str,
    pub initial: &'a str,
    pub action: &'a str,
    pub suggest: Option<Suggest>,
}

/// Asks for one line of text; `done` gets it, trimmed, if the user goes
/// ahead. The action is enabled only once something is typed.
pub fn ask_text(parent: &impl IsA<gtk::Widget>, ask: Ask<'_>, done: impl Fn(String) + 'static) {
    let entry = adw::EntryRow::builder()
        .title(ask.field)
        .activates_default(true)
        .build();
    entry.set_text(ask.initial);
    if let Some(suggest) = ask.suggest {
        autocomplete::attach(&entry, move |text| suggest(text), |_| {});
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading(ask.heading)
        .body(ask.body)
        .extra_child(&list)
        .focus_widget(&entry)
        .default_response("go")
        .close_response("cancel")
        .build();
    let cancel = gettext("_Cancel");
    dialog.add_responses(&[("cancel", &cancel), ("go", ask.action)]);
    dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("go", !ask.initial.trim().is_empty());
    {
        let weak = dialog.downgrade();
        entry.connect_changed(move |e| {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("go", !e.text().trim().is_empty());
            }
        });
    }
    dialog.choose(Some(parent), None::<&gio::Cancellable>, move |response| {
        if response == "go" {
            done(entry.text().trim().to_owned());
        }
    });
}

/// Changes one alternate name, or says why it could not.
pub type AliasChange = Box<dyn Fn(&str) -> Result<(), String>>;

/// Ways to change a list of alternate names.
pub struct AliasHooks {
    pub add: AliasChange,
    pub remove: AliasChange,
}

/// Shows an entry's alternate names, each removable, with a field to add
/// one. Changes take effect at once.
pub fn alternate_names(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    current: Vec<String>,
    hooks: AliasHooks,
) {
    let hooks = Rc::new(hooks);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .css_classes(["error", "caption"])
        .visible(false)
        .build();
    let add = adw::EntryRow::builder()
        .title(gettext("Add an Alternate Name"))
        .show_apply_button(true)
        .build();
    list.append(&add);

    let insert: Rc<dyn Fn(&str)> = {
        let (list, hooks, error) = (list.clone(), hooks.clone(), error.clone());
        Rc::new(move |alias: &str| {
            let row = adw::ActionRow::new();
            row.set_use_markup(false);
            row.set_title(alias);
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(gettext("Remove This Name"))
                .css_classes(["flat"])
                .build();
            let (owner, hooks, error, alias) =
                (list.clone(), hooks.clone(), error.clone(), alias.to_owned());
            let shown = row.clone();
            remove.connect_clicked(move |_| match (hooks.remove)(&alias) {
                Ok(()) => {
                    owner.remove(&shown);
                    error.set_visible(false);
                }
                Err(why) => {
                    error.set_label(&why);
                    error.set_visible(true);
                }
            });
            row.add_suffix(&remove);
            list.insert(&row, 0);
        })
    };
    for alias in current.iter().rev() {
        insert(alias);
    }
    {
        let (hooks, error, insert) = (hooks.clone(), error.clone(), insert.clone());
        add.connect_apply(move |entry| {
            let alias = entry.text().trim().to_owned();
            if alias.is_empty() {
                return;
            }
            match (hooks.add)(&alias) {
                Ok(()) => {
                    insert(&alias);
                    entry.set_text("");
                    error.set_visible(false);
                }
                Err(why) => {
                    error.set_label(&why);
                    error.set_visible(true);
                }
            }
        });
    }

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    content.append(&list);
    content.append(&error);
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Alternate Names"))
        .body(gettext_f(
            "Other spellings that find \u{201c}{name}\u{201d} when typed, such as \
             initials or a pen name.",
            &[("name", name)],
        ))
        .extra_child(&content)
        .default_response("close")
        .close_response("close")
        .build();
    let close = gettext("_Close");
    dialog.add_responses(&[("close", &close)]);
    dialog.present(Some(parent));
}
