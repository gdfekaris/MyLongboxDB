//! Asks what to change on every selected copy.

use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use mylongbox_core::{Amount, BoxId, BulkChange, Currency, Database, GradeType};

use crate::{autocomplete, format};

const GRADERS: [(&str, Option<Option<GradeType>>); 6] = [
    ("Leave as they are", None),
    ("Not graded", Some(None)),
    ("My own estimate", Some(Some(GradeType::Raw))),
    ("CGC", Some(Some(GradeType::Cgc))),
    ("CBCS", Some(Some(GradeType::Cbcs))),
    ("PGX", Some(Some(GradeType::Pgx))),
];

struct Fields {
    put_in: adw::EntryRow,
    take_out: adw::SwitchRow,
    tags: adw::EntryRow,
    graded_by: adw::ComboRow,
    value: adw::EntryRow,
    currency: gtk::DropDown,
    date: adw::EntryRow,
    source: adw::EntryRow,
}

impl Fields {
    /// The change the fields describe, or `None` if nothing would change
    /// or something cannot be read.
    fn change(&self) -> Option<BulkChange> {
        let storage_box = if self.take_out.is_active() {
            Some(None)
        } else {
            Some(self.put_in.text().trim().to_owned())
                .filter(|b| !b.is_empty())
                .map(Some)
        };
        let add_tags: Vec<String> = self
            .tags
            .text()
            .split(',')
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        let grade_type = GRADERS
            .get(usize::try_from(self.graded_by.selected()).unwrap_or(0))
            .and_then(|g| g.1);
        let value = match self.value.text().trim() {
            "" => None,
            text => {
                let cleaned: String = text
                    .trim_start_matches('$')
                    .chars()
                    .filter(|c| *c != ',')
                    .collect();
                let currency =
                    Currency::ALL[usize::try_from(self.currency.selected()).unwrap_or(0)];
                let amount = Amount::parse(currency, &cleaned).ok()?;
                let date = self.date.text().trim().parse().ok()?;
                Some((date, amount, self.source.text().trim().to_owned()))
            }
        };
        let change = BulkChange {
            storage_box,
            add_tags,
            grade_type,
            value,
        };
        (change != BulkChange::default()).then_some(change)
    }
}

/// Opens the dialog for `count` selected copies; `apply` gets the change
/// if the user goes ahead.
pub fn ask(
    parent: &impl IsA<gtk::Widget>,
    count: usize,
    db: &Rc<Database>,
    apply: impl Fn(BulkChange) + 'static,
) {
    let db: Weak<Database> = Rc::downgrade(db);
    let row = |title: &str| adw::EntryRow::builder().title(title).build();
    let put_in = row("Put in Box");
    {
        let db = db.clone();
        autocomplete::attach(
            &put_in,
            move |text| {
                db.upgrade()
                    .and_then(|db| db.suggest_lookups::<BoxId>(text, 9).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|l| l.name)
                    .collect()
            },
            |_| {},
        );
    }
    let take_out = adw::SwitchRow::builder()
        .title("Take Out of Their Boxes")
        .build();
    take_out
        .bind_property("active", &put_in, "sensitive")
        .invert_boolean()
        .sync_create()
        .build();
    let tags = row("Add Tags (separate with commas)");
    let graded_by = adw::ComboRow::builder()
        .title("Graded By")
        .model(&gtk::StringList::new(&GRADERS.map(|(name, _)| name)))
        .build();
    let value = row("Add a Value");
    let currency = gtk::DropDown::from_strings(&["USD", "Gold (oz)", "Bitcoin"]);
    currency.set_valign(gtk::Align::Center);
    value.add_suffix(&currency);
    let date = row("Value Date (YYYY-MM-DD)");
    date.set_text(&format::today().map(|d| d.to_string()).unwrap_or_default());
    let source = row("Value Source");

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&put_in);
    list.append(&take_out);
    list.append(&tags);
    list.append(&graded_by);
    list.append(&value);
    list.append(&date);
    list.append(&source);

    let fields = Rc::new(Fields {
        put_in,
        take_out,
        tags,
        graded_by,
        value,
        currency,
        date,
        source,
    });
    let body = match count {
        1 => "Changes apply to the selected copy. Collected editions keep their condition \
              when a grader is chosen."
            .to_owned(),
        n => format!(
            "Changes apply to all {n} selected copies. Collected editions keep their \
             condition when a grader is chosen."
        ),
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Edit Selected")
        .body(body)
        .extra_child(&list)
        .default_response("apply")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "_Cancel"), ("apply", "_Apply")]);
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("apply", false);
    {
        let weak = dialog.downgrade();
        let f = fields.clone();
        let check = Rc::new(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("apply", f.change().is_some());
            }
        });
        for entry in [&fields.put_in, &fields.tags, &fields.value, &fields.date] {
            let c = check.clone();
            entry.connect_changed(move |_| c());
        }
        let c = check.clone();
        fields.take_out.connect_active_notify(move |_| c());
        let c = check.clone();
        fields.graded_by.connect_selected_notify(move |_| c());
        fields.currency.connect_selected_notify(move |_| check());
    }
    dialog.choose(Some(parent), None::<&gio::Cancellable>, move |response| {
        if response == "apply"
            && let Some(change) = fields.change()
        {
            apply(change);
        }
    });
}
