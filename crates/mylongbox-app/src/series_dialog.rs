//! Changes the details of a series: its name, when it started, its
//! volume and publisher, the characters and teams in every issue, and
//! the like.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use mylongbox_core::{
    Category, CharacterId, Database, LookupId, PublisherId, Series, SeriesFields, TeamId,
};

use crate::autocomplete;
use crate::form::{COLORS, FORMATS};
use crate::i18n::{gettext, pgettext};

/// What the dialog saves: the series' details, and its publisher and the
/// characters and teams in every issue by name.
#[derive(Debug)]
pub struct SeriesEdit {
    pub fields: SeriesFields,
    pub publisher: String,
    pub characters: Vec<String>,
    pub teams: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Character,
    Team,
}

impl Kind {
    fn title(self) -> String {
        match self {
            Self::Character => gettext("Character"),
            Self::Team => gettext("Team"),
        }
    }
}

/// The characters and teams in every issue: rows that can be added and
/// removed, and offers of those the series' name holds.
struct Cast {
    list: gtk::ListBox,
    /// Name rows go in above this one.
    add_character: adw::ButtonRow,
    rows: RefCell<Vec<(Kind, adw::EntryRow)>>,
    offers: RefCell<Vec<adw::ActionRow>>,
    /// The series' name as last offered from.
    title: RefCell<String>,
    db: Weak<Database>,
}

impl Cast {
    fn new(db: &Weak<Database>) -> Rc<Self> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let button = |label: &str| {
            let row = adw::ButtonRow::builder()
                .title(label)
                .start_icon_name("list-add-symbolic")
                .build();
            list.append(&row);
            row
        };
        let add_character = button(&gettext("Add Character"));
        let add_team = button(&gettext("Add Team"));
        let cast = Rc::new(Self {
            list,
            add_character,
            rows: RefCell::default(),
            offers: RefCell::default(),
            title: RefCell::default(),
            db: db.clone(),
        });
        for (button, kind) in [
            (&cast.add_character, Kind::Character),
            (&add_team, Kind::Team),
        ] {
            let weak = Rc::downgrade(&cast);
            button.connect_activated(move |_| {
                if let Some(cast) = weak.upgrade() {
                    cast.add(kind, "").grab_focus();
                }
            });
        }
        cast
    }

    fn add(self: &Rc<Self>, kind: Kind, name: &str) -> adw::EntryRow {
        let row = adw::EntryRow::builder().title(kind.title()).build();
        row.set_text(name);
        let db = self.db.clone();
        autocomplete::attach(
            &row,
            move |text| {
                let Some(db) = db.upgrade() else {
                    return Vec::new();
                };
                let limit = autocomplete::LIMIT as u32 + 1;
                match kind {
                    Kind::Character => names(db.suggest_lookups::<CharacterId>(text, limit)),
                    Kind::Team => names(db.suggest_lookups::<TeamId>(text, limit)),
                }
            },
            |_| {},
        );
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Remove"))
            .css_classes(["flat"])
            .build();
        {
            let weak = Rc::downgrade(self);
            let row = row.clone();
            remove.connect_clicked(move |_| {
                if let Some(cast) = weak.upgrade() {
                    cast.list.remove(&row);
                    cast.rows.borrow_mut().retain(|(_, r)| *r != row);
                    let title = cast.title.borrow().clone();
                    cast.offer(&title);
                }
            });
        }
        row.add_suffix(&remove);
        self.list.insert(&row, self.add_character.index());
        self.rows.borrow_mut().push((kind, row.clone()));
        row
    }

    /// The names given for `kind`, without blanks.
    fn names(&self, kind: Kind) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, row)| row.text().trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// Offers, below the buttons, each character and team that `title`
    /// names and that is not already listed.
    fn offer(self: &Rc<Self>, title: &str) {
        title.clone_into(&mut self.title.borrow_mut());
        for row in self.offers.borrow_mut().drain(..) {
            self.list.remove(&row);
        }
        let Some((characters, teams)) = self
            .db
            .upgrade()
            .and_then(|db| db.cast_in_title(title).ok())
        else {
            return;
        };
        for (kind, found) in [(Kind::Character, characters), (Kind::Team, teams)] {
            let listed: Vec<String> = self.names(kind).iter().map(|n| n.to_lowercase()).collect();
            for name in found {
                if listed.contains(&name.to_lowercase()) {
                    continue;
                }
                let subtitle = match kind {
                    Kind::Character => gettext("A character in the name of the series"),
                    Kind::Team => gettext("A team in the name of the series"),
                };
                let row = adw::ActionRow::builder()
                    .title(&name)
                    .subtitle(subtitle)
                    .build();
                let add = gtk::Button::builder()
                    .label(gettext("Add"))
                    .valign(gtk::Align::Center)
                    .build();
                let weak = Rc::downgrade(self);
                add.connect_clicked(move |_| {
                    if let Some(cast) = weak.upgrade() {
                        cast.add(kind, &name);
                        let title = cast.title.borrow().clone();
                        cast.offer(&title);
                    }
                });
                row.add_suffix(&add);
                self.list.append(&row);
                self.offers.borrow_mut().push(row);
            }
        }
    }
}

fn names<I: LookupId>(
    found: mylongbox_core::Result<Vec<mylongbox_core::Lookup<I>>>,
) -> Vec<String> {
    found
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

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

/// Opens the dialog on `series`, whose publisher is called `publisher`
/// and whose every issue has `characters` and `teams`; `save` gets what
/// the dialog then holds.
pub fn ask(
    parent: &impl IsA<gtk::Widget>,
    series: &Series,
    publisher: &str,
    (characters, teams): (&[String], &[String]),
    db: &Rc<Database>,
    save: impl Fn(SeriesEdit) + 'static,
) {
    let db: Weak<Database> = Rc::downgrade(db);
    let db_weak = db.clone();
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

    let cast = Cast::new(&db_weak);
    for name in characters {
        cast.add(Kind::Character, name);
    }
    for name in teams {
        cast.add(Kind::Team, name);
    }
    cast.offer(&f.name);
    {
        let cast = cast.clone();
        name.connect_changed(move |row| cast.offer(row.text().trim()));
    }
    let heading = gtk::Label::builder()
        .label(gettext("In Every Issue"))
        .css_classes(["heading"])
        .xalign(0.0)
        .build();
    let explained = gtk::Label::builder()
        .label(gettext(
            "These count for every book in the series, without adding them to each one.",
        ))
        .css_classes(["dim-label"])
        .wrap(true)
        .xalign(0.0)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&list);
    heading.set_margin_top(18);
    content.append(&heading);
    content.append(&explained);
    explained.set_margin_bottom(6);
    content.append(&cast.list);

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
        .extra_child(&content)
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
            save(SeriesEdit {
                fields,
                publisher,
                characters: cast.names(Kind::Character),
                teams: cast.names(Kind::Team),
            });
        }
    });
}
