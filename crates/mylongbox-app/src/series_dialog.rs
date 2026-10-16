//! Changes the details of a series: its name, when it started, its
//! volume and publisher, and the like.

use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use mylongbox_core::{Category, Database, PublisherId, Series, SeriesFields};

use crate::autocomplete;
use crate::form::{COLORS, FORMATS};
use crate::i18n::{gettext, pgettext};

struct Fields {
    current: SeriesFields,
    name: adw::EntryRow,
    start_year: adw::EntryRow,
    volume: adw::EntryRow,
    publisher: adw::EntryRow,
    imprint: adw::EntryRow,
    known_issue_count: adw::EntryRow,
    format: adw::ComboRow,
    color: adw::ComboRow,
    notes: adw::EntryRow,
}

impl Fields {
    /// The series as the fields describe it with its publisher's name, or
    /// `None` while something cannot be read. Rows that cannot be read
    /// are marked.
    fn read(&self) -> Option<(SeriesFields, String)> {
        fn number<T: std::str::FromStr>(row: &adw::EntryRow) -> Result<Option<T>, ()> {
            let parsed = match row.text().trim() {
                "" => Ok(None),
                text => text.parse().map(Some).map_err(|_| ()),
            };
            if parsed.is_ok() {
                row.remove_css_class("error");
            } else {
                row.add_css_class("error");
            }
            parsed
        }
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            self.name.add_css_class("error");
        } else {
            self.name.remove_css_class("error");
        }
        let (start_year, volume, count) = (
            number(&self.start_year),
            number(&self.volume),
            number(&self.known_issue_count),
        );
        let (Ok(start_year), Ok(volume), Ok(known_issue_count)) = (start_year, volume, count)
        else {
            return None;
        };
        if name.is_empty() {
            return None;
        }
        let magazine = self.current.category == Category::Magazine;
        let pick = |combo: &adw::ComboRow| usize::try_from(combo.selected()).unwrap_or(0);
        let fields = SeriesFields {
            name,
            start_year,
            volume,
            known_issue_count,
            magazine_format: FORMATS[pick(&self.format)].1.filter(|_| magazine),
            black_and_white: COLORS[pick(&self.color)].1.filter(|_| magazine),
            notes: self.notes.text().trim().to_owned(),
            imprint: self.imprint.text().trim().to_owned(),
            ..self.current.clone()
        };
        Some((fields, self.publisher.text().trim().to_owned()))
    }
}

/// Opens the dialog on `series`, whose publisher is called `publisher`;
/// `save` gets the changed details and the publisher's name.
pub fn ask(
    parent: &impl IsA<gtk::Widget>,
    series: &Series,
    publisher: &str,
    db: &Rc<Database>,
    save: impl Fn(SeriesFields, String) + 'static,
) {
    let db: Weak<Database> = Rc::downgrade(db);
    let f = &series.fields;
    let row = |title: &str, text: String| {
        let row = adw::EntryRow::builder().title(title).build();
        row.set_text(&text);
        row
    };
    let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
    let name = row(&gettext("Name"), f.name.clone());
    let start_year = row(
        &gettext("Start Year"),
        f.start_year.map(|y| y.to_string()).unwrap_or_default(),
    );
    let volume = row(&gettext("Volume"), number(f.volume));
    let publisher = row(&gettext("Publisher"), publisher.to_owned());
    let imprint = row(&gettext("Imprint"), f.imprint.clone());
    crate::form::imprint_completion(&imprint, &publisher, &db);
    autocomplete::attach(
        &publisher,
        move |text| {
            db.upgrade()
                .and_then(|db| db.suggest_lookups::<PublisherId>(text, 9).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|l| l.name)
                .collect()
        },
        |_| {},
    );
    let known_issue_count = row(
        &gettext("Known Issue Count (for finding gaps)"),
        number(f.known_issue_count),
    );
    let combo = |title: &str, names: Vec<String>, at: usize| {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let combo = adw::ComboRow::builder()
            .title(title)
            .model(&gtk::StringList::new(&names))
            .build();
        combo.set_selected(u32::try_from(at).unwrap_or(0));
        combo
    };
    let format = combo(
        &gettext("Format"),
        FORMATS.iter().map(|(n, _)| gettext(n)).collect(),
        FORMATS
            .iter()
            .position(|(_, v)| *v == f.magazine_format)
            .unwrap_or(0),
    );
    let color = combo(
        &pgettext("whether a magazine is printed in color", "Color"),
        COLORS.iter().map(|(n, _)| gettext(n)).collect(),
        COLORS
            .iter()
            .position(|(_, v)| *v == f.black_and_white)
            .unwrap_or(0),
    );
    let notes = row(&gettext("Notes"), f.notes.clone());

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for row in [
        &name,
        &start_year,
        &volume,
        &publisher,
        &imprint,
        &known_issue_count,
    ] {
        list.append(row);
    }
    if f.category == Category::Magazine {
        list.append(&format);
        list.append(&color);
    }
    list.append(&notes);

    let fields = Rc::new(Fields {
        current: f.clone(),
        name,
        start_year,
        volume,
        publisher,
        imprint,
        known_issue_count,
        format,
        color,
        notes,
    });
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Edit Series"))
        .body(gettext("Changes show on every book in the series."))
        .extra_child(&list)
        .default_response("save")
        .close_response("cancel")
        .build();
    let (cancel_label, save_label) = (gettext("_Cancel"), gettext("_Save"));
    dialog.add_responses(&[("cancel", &cancel_label), ("save", &save_label)]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    {
        let weak = dialog.downgrade();
        let f = fields.clone();
        let check = Rc::new(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("save", f.read().is_some());
            }
        });
        for entry in [
            &fields.name,
            &fields.start_year,
            &fields.volume,
            &fields.known_issue_count,
        ] {
            let c = check.clone();
            entry.connect_changed(move |_| c());
        }
    }
    dialog.choose(Some(parent), None::<&gio::Cancellable>, move |response| {
        if response == "save"
            && let Some((fields, publisher)) = fields.read()
        {
            save(fields, publisher);
        }
    });
}
