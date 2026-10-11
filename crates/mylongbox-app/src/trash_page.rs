//! The Trash section: what has been moved to Trash, each group with a
//! way back and a way out for good.

use std::rc::Rc;

use adw::prelude::*;
use mylongbox_core::{TrashGroup, TrashId};

use crate::format;

/// What the page's buttons do.
pub struct Hooks {
    pub restore: Rc<dyn Fn(TrashId)>,
    /// Asks, then deletes one group for good.
    pub purge: Rc<dyn Fn(TrashId, String)>,
    /// Asks, then deletes everything in Trash for good.
    pub empty: Rc<dyn Fn()>,
}

pub fn page(groups: &[TrashGroup], hooks: &Hooks) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::builder()
        .title("In Trash")
        .description("Restore what you need; the rest stays here until you empty Trash.")
        .build();
    let empty = gtk::Button::builder()
        .label("_Empty Trash")
        .use_underline(true)
        .valign(gtk::Align::Center)
        .css_classes(["destructive-action"])
        .build();
    {
        let hook = hooks.empty.clone();
        empty.connect_clicked(move |_| hook());
    }
    group.set_header_suffix(Some(&empty));

    for g in groups {
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(&g.label);
        row.set_subtitle(
            &format::local_date(&g.trashed_at)
                .map(|d| format!("Moved to Trash {}", format::date(d)))
                .unwrap_or_default(),
        );
        let restore = gtk::Button::builder()
            .label("_Restore")
            .use_underline(true)
            .valign(gtk::Align::Center)
            .build();
        let purge = gtk::Button::builder()
            .icon_name("user-trash-full-symbolic")
            .tooltip_text("Delete Forever")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let id = g.id;
        {
            let hook = hooks.restore.clone();
            restore.connect_clicked(move |_| hook(id));
        }
        {
            let hook = hooks.purge.clone();
            let label = g.label.clone();
            purge.connect_clicked(move |_| hook(id, label.clone()));
        }
        row.add_suffix(&restore);
        row.add_suffix(&purge);
        group.add(&row);
    }

    let page = adw::PreferencesPage::new();
    page.add(&group);
    page
}
