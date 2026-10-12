//! The import wizard: match the file's columns to fields, check every
//! row, then add them, showing progress all the way.
//!
//! The rows are added a slice at a time between frames, so the window
//! keeps drawing, and photos are decoded on worker threads. Everything
//! happens in one transaction, so cancelling leaves the collection as it
//! was.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{
    CSV_COLUMNS, Database, ImportOutcome, ImportReport, ImportRun, ImportSettings, ImportSource,
    auto_mapping,
};
use mylongbox_media::Output;

use crate::{covers, format};

/// How long a slice of rows may hold up the window.
const SLICE: Duration = Duration::from_millis(30);
/// The most rows of one kind listed in the report.
const LISTED: usize = 200;
/// Rows read for the examples under each column.
const SAMPLED: usize = 50;

/// What separates names in a list cell, as offered.
const SEPARATORS: [(&str, char); 4] = [
    ("Semicolons ( ; )", ';'),
    ("Commas ( , )", ','),
    ("Bars ( | )", '|'),
    ("Slashes ( / )", '/'),
];

/// Columns that hold lists of names.
const LIST_COLUMNS: &[&str] = &[
    "Writers",
    "Pencillers",
    "Inkers",
    "Colorists",
    "Letterers",
    "Cover Artists",
    "Editors",
    "Article Writers",
    "Interviews",
    "Characters",
    "First Appearances of Characters",
    "Teams",
    "First Appearances of Teams",
    "Story Arcs",
    "Variant Cover Artists",
    "Tags",
];

/// A few of a column's values, to show what it holds.
pub fn examples(rows: &[Vec<String>], column: usize) -> String {
    let mut seen = Vec::new();
    for value in rows.iter().filter_map(|r| r.get(column)).map(|v| v.trim()) {
        if !value.is_empty() && !seen.contains(&value) {
            seen.push(value);
            if seen.len() == 3 {
                break;
            }
        }
    }
    let shortened: Vec<String> = seen
        .iter()
        .map(|v| match v.char_indices().nth(30) {
            Some((at, _)) => format!("{}\u{2026}", &v[..at]),
            None => (*v).to_owned(),
        })
        .collect();
    if shortened.is_empty() {
        "Empty in the first rows".to_owned()
    } else {
        shortened.join(" \u{b7} ")
    }
}

/// What separates names in the file's list cells: semicolons in
/// MyLongboxDB's own files, and otherwise whichever of the offered
/// separators its list columns use most.
pub fn guess_separator(rows: &[Vec<String>], mapping: &[Option<&'static str>]) -> char {
    let lists: Vec<usize> = mapping
        .iter()
        .enumerate()
        .filter(|(_, m)| m.is_some_and(|f| LIST_COLUMNS.contains(&f)))
        .map(|(i, _)| i)
        .collect();
    let used = |separator: char| {
        rows.iter()
            .flat_map(|r| lists.iter().filter_map(|i| r.get(*i)))
            .filter(|cell| cell.contains(separator))
            .count()
    };
    SEPARATORS
        .iter()
        .map(|(_, s)| (*s, used(*s)))
        .rev()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
        .map_or(';', |(s, _)| s)
}

/// Why the columns as chosen cannot be imported, if they cannot.
pub fn mapping_problem(mapping: &[Option<&'static str>]) -> Option<String> {
    let mut seen = HashSet::new();
    if let Some(twice) = mapping.iter().flatten().find(|f| !seen.insert(**f)) {
        return Some(format!(
            "\u{201c}{twice}\u{201d} is chosen for more than one column"
        ));
    }
    (!seen.contains("Series"))
        .then(|| "Choose the column that names each book\u{2019}s series".into())
}

fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{} {many}", format::grouped(&n.to_string())),
    }
}

/// One part of the report: a heading and the rows it lists.
#[derive(Debug, PartialEq, Eq)]
pub struct Part {
    pub title: String,
    /// Each listed row's title and what happens to it.
    pub rows: Vec<(String, String)>,
    /// How many more there are than are listed.
    pub more: usize,
}

/// The report as the wizard shows it, in parts with nothing to say left
/// out.
pub fn report_parts(report: &ImportReport, done: bool) -> Vec<Part> {
    let part = |title: String, rows: Vec<(String, String)>| {
        let more = rows.len().saturating_sub(LISTED);
        Part {
            title,
            rows: rows.into_iter().take(LISTED).collect(),
            more,
        }
    };
    let listed = |matches: &dyn Fn(&ImportOutcome) -> Option<String>| {
        report
            .rows
            .iter()
            .filter_map(|r| {
                let why = matches(&r.outcome)?;
                Some((format!("Line {}: {}", r.line, r.label), why))
            })
            .collect::<Vec<_>>()
    };
    let added = report.added();
    let mut parts = Vec::new();
    if added > 0 {
        let title = if done {
            format!("{} added", count(added, "book", "books"))
        } else {
            format!("{} to add", count(added, "book", "books"))
        };
        parts.push(part(title, Vec::new()));
    }
    let duplicates = listed(&|o| match o {
        ImportOutcome::LikelyDuplicate { note, added } => Some(match (added, done) {
            (true, true) => format!("Added, though {note}"),
            (true, false) => format!("To be added, though {note}"),
            (false, _) => format!("Left out, as {note}"),
        }),
        _ => None,
    });
    if !duplicates.is_empty() {
        let title = count(duplicates.len(), "likely duplicate", "likely duplicates");
        parts.push(part(title, duplicates));
    }
    let owned = listed(&|o| {
        (*o == ImportOutcome::AlreadyOwned).then(|| "This copy is already in the collection".into())
    });
    if !owned.is_empty() {
        let title = format!(
            "{} already in the collection",
            count(owned.len(), "book", "books")
        );
        parts.push(part(title, owned));
    }
    let problems = listed(&|o| match o {
        ImportOutcome::Problem(why) => Some(why.clone()),
        _ => None,
    });
    if !problems.is_empty() {
        let title = format!("{} cannot be added", count(problems.len(), "row", "rows"));
        parts.push(part(title, problems));
    }
    if !report.photo_problems.is_empty() {
        let rows = report
            .photo_problems
            .iter()
            .map(|p| match p.split_once(": ") {
                Some((path, why)) => (path.to_owned(), why.to_owned()),
                None => (p.clone(), String::new()),
            })
            .collect();
        let title = format!(
            "{} could not be added",
            count(report.photo_problems.len(), "photo", "photos")
        );
        parts.push(part(title, rows));
    }
    parts
}

/// Where the wizard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Columns,
    Checking,
    /// The check is done; this many books would be added.
    Checked(usize),
    Importing,
    /// The import is kept; the report is shown for its photos' sake.
    Done,
}

struct Wizard {
    db: Weak<Database>,
    source: RefCell<ImportSource>,
    output: Output,
    dialog: adw::Dialog,
    stack: gtk::Stack,
    back: gtk::Button,
    next: gtk::Button,
    columns: Vec<adw::ComboRow>,
    columns_group: adw::PreferencesGroup,
    separator: adw::ComboRow,
    skip: adw::SwitchRow,
    progress: gtk::ProgressBar,
    progress_label: gtk::Label,
    report: adw::Bin,
    stage: Cell<Stage>,
    cancelled: Cell<bool>,
    /// Keeps a safety copy before an import starts, or says why not.
    before_import: Box<dyn Fn() -> Result<(), String>>,
    /// Told how many books were added, once an import is kept.
    imported: Box<dyn Fn(usize)>,
}

impl Wizard {
    fn mapping(&self) -> Vec<Option<&'static str>> {
        self.columns
            .iter()
            .map(|row| {
                let chosen = usize::try_from(row.selected()).unwrap_or(0);
                chosen
                    .checked_sub(1)
                    .and_then(|i| CSV_COLUMNS.get(i).copied())
            })
            .collect()
    }

    fn settings(&self) -> Option<ImportSettings> {
        let mapping = self.mapping();
        if mapping_problem(&mapping).is_some() {
            return None;
        }
        let separator = SEPARATORS
            .get(usize::try_from(self.separator.selected()).unwrap_or(0))
            .map_or(';', |s| s.1);
        Some(ImportSettings {
            mapping,
            separator,
            today: format::today()?,
            skip_likely_duplicates: self.skip.is_active(),
        })
    }

    /// Shows a stage's page and buttons.
    fn show(&self, stage: Stage) {
        self.stage.set(stage);
        let (page, back, next): (&str, Option<&str>, Option<String>) = match stage {
            Stage::Columns => ("columns", Some("_Cancel"), Some("_Check".into())),
            Stage::Checking | Stage::Importing => ("progress", Some("_Cancel"), None),
            Stage::Checked(0) => ("report", Some("_Back"), Some("_Import".into())),
            Stage::Checked(n) => (
                "report",
                Some("_Back"),
                Some(format!("_Import {}", count(n, "Book", "Books"))),
            ),
            Stage::Done => ("report", None, Some("_Done".into())),
        };
        self.stack.set_visible_child_name(page);
        self.back.set_visible(back.is_some());
        if let Some(label) = back {
            self.back.set_label(label);
        }
        self.next.set_visible(next.is_some());
        if let Some(label) = next {
            self.next.set_label(&label);
        }
        self.update_next();
    }

    /// Enables the forward button when going forward can do something.
    fn update_next(&self) {
        let problem = mapping_problem(&self.mapping());
        let description = problem.clone().unwrap_or_else(|| {
            "Choose what each column holds. Columns left out are not imported.".into()
        });
        self.columns_group.set_description(Some(&description));
        self.next.set_sensitive(match self.stage.get() {
            Stage::Columns => problem.is_none(),
            Stage::Checked(n) => n > 0,
            _ => true,
        });
    }

    fn set_progress(&self, doing: &str, done: usize, total: usize) {
        self.progress_label.set_label(&format!(
            "{doing} {} of {}\u{2026}",
            format::grouped(&done.min(total).to_string()),
            format::grouped(&total.to_string())
        ));
        self.progress.set_fraction(if total == 0 {
            1.0
        } else {
            done as f64 / total as f64
        });
    }

    fn show_report(&self, report: &ImportReport, done: bool) {
        let page = adw::PreferencesPage::new();
        let parts = report_parts(report, done);
        if parts.is_empty() {
            let group = adw::PreferencesGroup::new();
            group.add(
                &adw::ActionRow::builder()
                    .title("The file has no books")
                    .build(),
            );
            page.add(&group);
        }
        for part in parts {
            let group = adw::PreferencesGroup::builder().title(&part.title).build();
            if !part.rows.is_empty() {
                let expander = adw::ExpanderRow::builder()
                    .title("Show Each Row")
                    .expanded(part.rows.len() <= 5)
                    .build();
                for (title, why) in &part.rows {
                    let row = adw::ActionRow::builder()
                        .title(title)
                        .subtitle(why)
                        .use_markup(false)
                        .subtitle_lines(3)
                        .build();
                    expander.add_row(&row);
                }
                if part.more > 0 {
                    let more = adw::ActionRow::builder()
                        .title(format!(
                            "And {} more",
                            format::grouped(&part.more.to_string())
                        ))
                        .build();
                    expander.add_row(&more);
                }
                group.add(&expander);
            }
            page.add(&group);
        }
        self.report.set_child(Some(&page));
    }

    fn cancel(&self) {
        match self.stage.get() {
            Stage::Checking | Stage::Importing => self.cancelled.set(true),
            Stage::Checked(_) => self.show(Stage::Columns),
            Stage::Columns | Stage::Done => {
                self.dialog.force_close();
            }
        }
    }

    fn fail(&self, run: ImportRun, db: &Database, why: &str) {
        let _ = db.abandon_import(run);
        let alert = adw::AlertDialog::new(Some("Nothing Was Imported"), Some(why));
        alert.add_response("close", "_Close");
        alert.present(Some(&self.dialog));
        self.show(Stage::Columns);
    }
}

/// Adds the rows a slice at a time, then, when keeping them, the
/// photos; a check is abandoned once every row is read.
async fn run(wizard: Rc<Wizard>, keep: bool) {
    let (Some(settings), Some(db)) = (wizard.settings(), wizard.db.upgrade()) else {
        return;
    };
    wizard.cancelled.set(false);
    wizard.show(if keep {
        Stage::Importing
    } else {
        Stage::Checking
    });
    let doing = if keep { "Adding row" } else { "Checking row" };
    if keep && let Err(why) = (wizard.before_import)() {
        wizard.show(Stage::Columns);
        let alert = adw::AlertDialog::new(Some("Nothing Was Imported"), Some(&why));
        alert.add_response("close", "_Close");
        alert.present(Some(&wizard.dialog));
        return;
    }
    let total = wizard.source.borrow().len();
    wizard.set_progress(doing, 0, total);
    let mut run = match db.start_import(&wizard.source.borrow()) {
        Ok(run) => run,
        Err(e) => {
            wizard.show(Stage::Columns);
            let alert = adw::AlertDialog::new(Some("Nothing Was Imported"), Some(&e.to_string()));
            alert.add_response("close", "_Close");
            alert.present(Some(&wizard.dialog));
            return;
        }
    };
    loop {
        if wizard.cancelled.get() {
            let _ = db.abandon_import(run);
            wizard.show(Stage::Columns);
            return;
        }
        let started = Instant::now();
        let finished = loop {
            match db.import_rows(&mut run, &wizard.source.borrow(), &settings, 10) {
                Ok(true) => break true,
                Ok(false) if started.elapsed() < SLICE => {}
                Ok(false) => break false,
                Err(e) => {
                    wizard.fail(run, &db, &e.to_string());
                    return;
                }
            }
        };
        wizard.set_progress(doing, run.rows_done(), total);
        if finished {
            break;
        }
        // Below the priority of drawing, so each slice gets shown.
        glib::timeout_future_with_priority(glib::Priority::DEFAULT_IDLE, Duration::ZERO).await;
    }

    if !keep {
        match db.abandon_import(run) {
            Ok(report) => {
                wizard.show_report(&report, false);
                wizard.show(Stage::Checked(report.added()));
            }
            Err(e) => {
                wizard.show(Stage::Columns);
                let alert = adw::AlertDialog::new(Some("The Check Failed"), Some(&e.to_string()));
                alert.add_response("close", "_Close");
                alert.present(Some(&wizard.dialog));
            }
        }
        return;
    }

    let photos = run.photos().to_vec();
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4));
    for (done, batch) in photos.chunks(workers).enumerate() {
        wizard.set_progress("Adding photo", done * workers, photos.len());
        if wizard.cancelled.get() {
            let _ = db.abandon_import(run);
            wizard.show(Stage::Columns);
            return;
        }
        let output = wizard.output;
        let jobs: Vec<_> = batch
            .iter()
            .map(|photo| {
                let bytes = wizard.source.borrow_mut().read_photo(&photo.path);
                let job =
                    gio::spawn_blocking(move || bytes.and_then(|b| covers::prepare(&b, output)));
                (photo, job)
            })
            .collect();
        for (photo, job) in jobs {
            let prepared = job
                .await
                .unwrap_or_else(|_| Err("the photo could not be read".into()));
            if let Err(e) = db.import_photo(&mut run, photo, prepared) {
                wizard.fail(run, &db, &e.to_string());
                return;
            }
        }
    }
    match db.finish_import(run) {
        Ok(report) => {
            (wizard.imported)(report.added());
            if report.photo_problems.is_empty() {
                wizard.dialog.force_close();
            } else {
                wizard.show_report(&report, true);
                wizard.show(Stage::Done);
            }
        }
        Err(e) => {
            wizard.show(Stage::Columns);
            let alert = adw::AlertDialog::new(Some("Nothing Was Imported"), Some(&e.to_string()));
            alert.add_response("close", "_Close");
            alert.present(Some(&wizard.dialog));
        }
    }
}

/// Opens the wizard for a file read into `source`. `before_import` runs
/// as an import starts, which stops if it fails; `imported` is told how
/// many books were added once an import is kept.
pub fn open(
    parent: &impl IsA<gtk::Widget>,
    file_name: &str,
    source: ImportSource,
    db: &Rc<Database>,
    output: Output,
    before_import: impl Fn() -> Result<(), String> + 'static,
    imported: impl Fn(usize) + 'static,
) {
    let mapping = auto_mapping(source.headers());
    let samples = source.preview(SAMPLED).to_vec();

    let file = adw::PreferencesGroup::builder()
        .title(file_name)
        .description(if source.is_own_format() {
            format!(
                "{} from MyLongboxDB, so every column is matched already.",
                count(source.len(), "book", "books")
            )
        } else {
            count(source.len(), "book", "books")
        })
        .build();
    let separator = adw::ComboRow::builder()
        .title("Names in a Cell Are Separated By")
        .subtitle("As in \u{201c}Len Wein; Bernie Wrightson\u{201d}")
        .model(&gtk::StringList::new(&SEPARATORS.map(|s| s.0)))
        .build();
    let guessed = guess_separator(&samples, &mapping);
    let at = SEPARATORS.iter().position(|s| s.1 == guessed).unwrap_or(0);
    separator.set_selected(u32::try_from(at).unwrap_or(0));
    let skip = adw::SwitchRow::builder()
        .title("Leave Out Likely Duplicates")
        .subtitle("Books whose issue, variant, and printing you already own")
        .build();
    file.add(&separator);
    file.add(&skip);

    let columns_group = adw::PreferencesGroup::builder().title("Columns").build();
    let mut choices = vec!["Leave Out"];
    choices.extend(CSV_COLUMNS.iter().copied());
    let model = gtk::StringList::new(&choices);
    let columns: Vec<adw::ComboRow> = source
        .headers()
        .iter()
        .enumerate()
        .map(|(i, header)| {
            let row = adw::ComboRow::builder()
                .title(if header.is_empty() {
                    "(no heading)"
                } else {
                    header
                })
                .subtitle(examples(&samples, i))
                .use_markup(false)
                .model(&model)
                .enable_search(true)
                .expression(gtk::PropertyExpression::new(
                    gtk::StringObject::static_type(),
                    None::<&gtk::Expression>,
                    "string",
                ))
                .build();
            let chosen = mapping[i]
                .and_then(|f| CSV_COLUMNS.iter().position(|c| *c == f))
                .map_or(0, |p| p + 1);
            row.set_selected(u32::try_from(chosen).unwrap_or(0));
            columns_group.add(&row);
            row
        })
        .collect();

    let columns_page = adw::PreferencesPage::new();
    columns_page.add(&file);
    columns_page.add(&columns_group);

    let progress = gtk::ProgressBar::builder().width_request(320).build();
    let progress_label = gtk::Label::new(None);
    let progress_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();
    progress_box.append(&progress_label);
    progress_box.append(&progress);
    progress_box.append(
        &gtk::Label::builder()
            .label("Nothing is saved until the end, so cancelling leaves the collection as it was.")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build(),
    );

    let report = adw::Bin::new();
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_named(&columns_page, Some("columns"));
    stack.add_named(&progress_box, Some("progress"));
    stack.add_named(&report, Some("report"));

    let back = gtk::Button::with_mnemonic("_Cancel");
    let next = gtk::Button::builder()
        .use_underline(true)
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&back);
    header.pack_end(&next);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    let dialog = adw::Dialog::builder()
        .title("Import Books")
        .content_width(640)
        .content_height(720)
        .child(&toolbar)
        .can_close(false)
        .build();

    let wizard = Rc::new(Wizard {
        db: Rc::downgrade(db),
        source: RefCell::new(source),
        output,
        dialog: dialog.clone(),
        stack,
        back: back.clone(),
        next: next.clone(),
        columns,
        columns_group,
        separator,
        skip,
        progress,
        progress_label,
        report,
        stage: Cell::new(Stage::Columns),
        cancelled: Cell::new(false),
        before_import: Box::new(before_import),
        imported: Box::new(imported),
    });
    for row in &wizard.columns {
        let weak = Rc::downgrade(&wizard);
        row.connect_selected_notify(move |_| {
            if let Some(w) = weak.upgrade() {
                w.update_next();
            }
        });
    }
    {
        let weak = Rc::downgrade(&wizard);
        dialog.connect_close_attempt(move |_| {
            if let Some(w) = weak.upgrade() {
                w.cancel();
            }
        });
    }
    {
        let weak = Rc::downgrade(&wizard);
        back.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.cancel();
            }
        });
    }
    {
        let weak = Rc::downgrade(&wizard);
        next.connect_clicked(move |_| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            match w.stage.get() {
                Stage::Columns => {
                    glib::spawn_future_local(run(w, false));
                }
                Stage::Checked(_) => {
                    glib::spawn_future_local(run(w, true));
                }
                Stage::Done => {
                    w.dialog.force_close();
                }
                Stage::Checking | Stage::Importing => {}
            }
        });
    }
    wizard.show(Stage::Columns);
    // The wizard lives as long as its dialog.
    let keep = RefCell::new(Some(wizard));
    dialog.connect_closed(move |_| {
        keep.borrow_mut().take();
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
        cells
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect()
    }

    #[test]
    fn examples_show_what_a_column_holds() {
        let rows = rows(&[
            &["Thor", ""],
            &["Thor", ""],
            &["Hulk", ""],
            &["An exceedingly long series name, going on", ""],
            &["X-Men", ""],
        ]);
        assert_eq!(
            examples(&rows, 0),
            "Thor \u{b7} Hulk \u{b7} An exceedingly long series nam\u{2026}"
        );
        assert_eq!(examples(&rows, 1), "Empty in the first rows");
        assert_eq!(examples(&rows, 7), "Empty in the first rows");
    }

    #[test]
    fn the_separator_is_guessed_from_list_columns() {
        let mapping = [Some("Series"), Some("Writers"), Some("Tags")];
        let commas = rows(&[&["Thor, the", "Len Wein, Bernie Wrightson", "Key"]]);
        assert_eq!(guess_separator(&commas, &mapping), ',');
        let semicolons = rows(&[&["A, B", "Len Wein; Bernie Wrightson", "Key; Gift"]]);
        assert_eq!(guess_separator(&semicolons, &mapping), ';');
        // Only the series has commas, and it is not a list.
        let none = rows(&[&["Thor, the", "Len Wein", ""]]);
        assert_eq!(guess_separator(&none, &mapping), ';');
    }

    #[test]
    fn columns_must_name_the_series_once_each() {
        assert_eq!(
            mapping_problem(&[Some("Series"), None, Some("Grade")]),
            None
        );
        assert_eq!(
            mapping_problem(&[None, Some("Grade")]).as_deref(),
            Some("Choose the column that names each book\u{2019}s series")
        );
        assert_eq!(
            mapping_problem(&[Some("Series"), Some("Grade"), Some("Grade")]).as_deref(),
            Some("\u{201c}Grade\u{201d} is chosen for more than one column")
        );
    }

    fn report(outcomes: Vec<ImportOutcome>, photo_problems: Vec<String>) -> ImportReport {
        ImportReport {
            rows: outcomes
                .into_iter()
                .enumerate()
                .map(|(i, outcome)| mylongbox_core::RowReport {
                    line: i + 2,
                    label: format!("Thor #{i}"),
                    outcome,
                })
                .collect(),
            photo_problems,
        }
    }

    #[test]
    fn the_report_says_what_happens_to_each_row() {
        let note = "you already own a copy of this issue, variant, and printing";
        let r = report(
            vec![
                ImportOutcome::Added,
                ImportOutcome::Added,
                ImportOutcome::LikelyDuplicate {
                    note: note.into(),
                    added: true,
                },
                ImportOutcome::AlreadyOwned,
                ImportOutcome::Problem("Grade: \u{201c}11\u{201d} cannot be read".into()),
            ],
            Vec::new(),
        );
        let parts = report_parts(&r, false);
        // Likely duplicates being added count among the books to add.
        let titles: Vec<&str> = parts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "3 books to add",
                "1 likely duplicate",
                "1 book already in the collection",
                "1 row cannot be added",
            ]
        );
        assert!(parts[0].rows.is_empty());
        assert_eq!(
            parts[1].rows,
            [(
                "Line 4: Thor #2".into(),
                format!("To be added, though {note}")
            )]
        );
        assert_eq!(
            parts[3].rows[0].1,
            "Grade: \u{201c}11\u{201d} cannot be read"
        );
        assert_eq!(report_parts(&r, true)[0].title, "3 books added");
        assert!(report_parts(&report(Vec::new(), Vec::new()), false).is_empty());
    }

    #[test]
    fn long_lists_are_cut_short_and_photos_reported() {
        let r = report(
            (0..LISTED + 5)
                .map(|_| ImportOutcome::Problem("Series: every book needs a series".into()))
                .collect(),
            vec!["images/a_back.jpg: unreadable".into()],
        );
        let parts = report_parts(&r, true);
        assert_eq!(parts[0].title, "205 rows cannot be added");
        assert_eq!((parts[0].rows.len(), parts[0].more), (LISTED, 5));
        assert_eq!(parts[1].title, "1 photo could not be added");
        assert_eq!(
            parts[1].rows,
            [("images/a_back.jpg".into(), "unreadable".into())]
        );
    }
}
