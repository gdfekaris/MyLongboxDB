//! Suggestions under a text row as the user types, picked with the
//! mouse or with the arrow keys and Enter.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

/// Most suggestions shown at once.
pub const LIMIT: usize = 8;

thread_local! {
    /// How many suggestion lists are showing, so Escape can close a list
    /// rather than the form beneath it.
    static SHOWING: Cell<usize> = const { Cell::new(0) };
}

/// Whether any suggestion list is showing.
pub fn showing() -> bool {
    SHOWING.with(Cell::get) > 0
}

/// Offers `suggest(text)` under `row` while it has focus. Choosing one
/// replaces the row's text and calls `picked` with it.
pub fn attach(
    row: &adw::EntryRow,
    suggest: impl Fn(&str) -> Vec<String> + 'static,
    picked: impl Fn(&str) + 'static,
) {
    attach_with(
        row,
        move |text| {
            suggest(text.trim())
                .into_iter()
                .map(|s| (s.clone(), s))
                .collect()
        },
        picked,
    );
}

/// Like [`attach`], but each suggestion is shown as its first part and
/// replaces the row's text with its second, so a suggestion can complete
/// just the end of a longer text.
pub fn attach_with(
    row: &adw::EntryRow,
    suggest: impl Fn(&str) -> Vec<(String, String)> + 'static,
    picked: impl Fn(&str) + 'static,
) {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .activate_on_single_click(true)
        .build();
    list.add_css_class("navigation-sidebar");
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(320)
        .child(&list)
        .build();
    // Never autohide: that would take keyboard focus from the row while
    // the user is still typing.
    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .halign(gtk::Align::Start)
        .child(&scroller)
        .build();
    popover.set_parent(row);
    popover.connect_visible_notify(|p| {
        SHOWING.with(|n| {
            n.set(if p.is_visible() {
                n.get() + 1
            } else {
                n.get().saturating_sub(1)
            });
        });
    });
    {
        // A popover with a parent must be unparented before the parent
        // goes away.
        let popover = popover.clone();
        row.connect_destroy(move |_| popover.unparent());
    }

    let choices: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
    // Set while the row's text is changed here, so that does not reopen
    // the suggestions.
    let quiet = Rc::new(Cell::new(false));
    let picked = Rc::new(picked);

    let accept = {
        let row = row.clone();
        let popover = popover.clone();
        let choices = choices.clone();
        let quiet = quiet.clone();
        let picked = picked.clone();
        Rc::new(move |index: usize| {
            let Some((_, choice)) = choices.borrow().get(index).cloned() else {
                return;
            };
            popover.popdown();
            quiet.set(true);
            row.set_text(&choice);
            row.set_position(-1);
            quiet.set(false);
            picked(&choice);
        })
    };
    {
        let accept = accept.clone();
        list.connect_row_activated(move |_, item| {
            if let Ok(index) = usize::try_from(item.index()) {
                accept(index);
            }
        });
    }

    {
        let list = list.clone();
        let popover = popover.clone();
        let choices = choices.clone();
        let quiet = quiet.clone();
        row.connect_changed(move |row| {
            // Only while the user types here, not when the form fills in
            // the row.
            if quiet.get() || !row.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) {
                return;
            }
            let text = row.text();
            let found: Vec<(String, String)> = if text.trim().is_empty() {
                Vec::new()
            } else {
                suggest(&text)
                    .into_iter()
                    .filter(|(_, s)| !s.trim().eq_ignore_ascii_case(text.trim()))
                    .take(LIMIT)
                    .collect()
            };
            list.remove_all();
            for (shown, _) in &found {
                let label = gtk::Label::builder()
                    .label(shown)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();
                list.append(&label);
            }
            let any = !found.is_empty();
            *choices.borrow_mut() = found;
            if any {
                popover.set_size_request(row.width(), -1);
                list.unselect_all();
                popover.popup();
            } else {
                popover.popdown();
            }
        });
    }

    // Arrow keys move through the suggestions without leaving the row.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let list = list.clone();
        let popover = popover.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if !popover.is_visible() {
                return glib::Propagation::Proceed;
            }
            let selected = list.selected_row().map(|r| r.index());
            let count = i32::try_from(choices.borrow().len()).unwrap_or(0);
            let select = |index: i32| {
                if let Some(item) = list.row_at_index(index) {
                    list.select_row(Some(&item));
                }
            };
            match key {
                gtk::gdk::Key::Down => {
                    select(selected.map_or(0, |i| (i + 1).min(count - 1)));
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    match selected {
                        Some(i) if i > 0 => select(i - 1),
                        _ => list.unselect_all(),
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::Tab => {
                    match selected.and_then(|i| usize::try_from(i).ok()) {
                        Some(index) => {
                            accept(index);
                            glib::Propagation::Stop
                        }
                        None => {
                            popover.popdown();
                            glib::Propagation::Proceed
                        }
                    }
                }
                gtk::gdk::Key::Escape => {
                    popover.popdown();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    row.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    {
        let popover = popover.clone();
        focus.connect_leave(move |_| {
            // Clicking a suggestion moves focus too; let the click land
            // before closing.
            let popover = popover.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                popover.popdown();
            });
        });
    }
    row.add_controller(focus);
}
