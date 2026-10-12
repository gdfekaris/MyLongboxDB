//! The Keyboard Shortcuts window.

use crate::i18n::gettext;

/// The shortcuts, by section: each a title and the keys, as GTK writes
/// accelerators.
fn sections() -> Vec<(String, Vec<(String, &'static str)>)> {
    vec![
        (
            gettext("General"),
            vec![
                (gettext("Search"), "<Control>f"),
                (gettext("Main Menu"), "F10"),
                (gettext("Preferences"), "<Control>comma"),
                (gettext("Keyboard Shortcuts"), "<Control>question"),
                (gettext("Quit"), "<Control>q"),
            ],
        ),
        (
            gettext("Books"),
            vec![
                (gettext("Add a Comic"), "<Control>n"),
                (gettext("Add the Next Issue"), "<Control><Shift>n"),
                (gettext("Cover Grid"), "<Control>1"),
                (gettext("List"), "<Control>2"),
                (gettext("Go Back"), "<Alt>Left"),
                (gettext("Select All, When Selecting"), "<Control>a"),
                (gettext("Move Selected Books to Trash"), "Delete"),
            ],
        ),
        (
            gettext("Book Form"),
            vec![
                (gettext("Save"), "<Control>s"),
                (gettext("Cancel"), "Escape"),
            ],
        ),
        (
            gettext("Cover Photos"),
            vec![
                (gettext("Zoom In"), "plus"),
                (gettext("Zoom Out"), "minus"),
                (gettext("Fit to the Window"), "0"),
                (gettext("Actual Size"), "1"),
            ],
        ),
    ]
}

pub fn dialog() -> adw::ShortcutsDialog {
    let dialog = adw::ShortcutsDialog::new();
    for (title, items) in sections() {
        let section = adw::ShortcutsSection::new(Some(&title));
        for (name, keys) in items {
            section.add(adw::ShortcutsItem::new(&name, keys));
        }
        dialog.add(section);
    }
    dialog
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checked without a display: each key is one GDK names, after
    /// modifiers GTK knows.
    #[test]
    fn every_shortcut_is_a_key_gtk_can_read() {
        for (_, items) in sections() {
            for (name, keys) in items {
                let mut rest = keys;
                while let Some(after) = rest.strip_prefix('<') {
                    let (modifier, tail) = after.split_once('>').expect(keys);
                    assert!(
                        ["Control", "Shift", "Alt"].contains(&modifier),
                        "{name}: {keys}"
                    );
                    rest = tail;
                }
                assert!(gtk::gdk::Key::from_name(rest).is_some(), "{name}: {keys}");
            }
        }
    }
}
