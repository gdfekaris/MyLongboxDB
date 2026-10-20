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

use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};
use crate::{covers, format};

/// How long a slice of rows may hold up the window.
const SLICE: Duration = Duration::from_millis(30);
/// The most rows of one kind listed in the report.
const LISTED: usize = 200;
/// Rows read for the examples under each column.
const SAMPLED: usize = 50;

/// What separates names in a list cell, as offered.
const SEPARATORS: [(&str, char); 4] = [
    (gettext_noop("Semicolons ( ; )"), ';'),
    (gettext_noop("Commas ( , )"), ','),
    (gettext_noop("Bars ( | )"), '|'),
    (gettext_noop("Slashes ( / )"), '/'),
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
        gettext("Empty in the first rows")
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
        return Some(gettext_f(
            "\u{201c}{field}\u{201d} is chosen for more than one column",
            &[("field", twice)],
        ));
    }
    (!seen.contains("Series"))
        .then(|| gettext("Choose the column that names each book\u{2019}s series"))
}

/// A count as shown, with thousands grouped.
fn grouped(n: usize) -> String {
    format::grouped(&n.to_string())
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
                let line = gettext_f(
                    "Line {line}: {book}",
                    &[("line", &r.line.to_string()), ("book", &r.label)],
                );
                Some((line, why))
            })
            .collect::<Vec<_>>()
    };
    let added = report.added();
    let mut parts = Vec::new();
    if added > 0 {
        let count = grouped(added);
        let values = [("count", count.as_str())];
        let title = if done {
            ngettext_f("{count} book added", "{count} books added", added, &values)
        } else {
            ngettext_f(
                "{count} book to add",
                "{count} books to add",
                added,
                &values,
            )
        };
        parts.push(part(title, Vec::new()));
    }
    let duplicates = listed(&|o| match o {
        ImportOutcome::LikelyDuplicate { note, added } => Some(match (added, done) {
            (true, true) => gettext_f("Added, though {reason}", &[("reason", note)]),
            (true, false) => gettext_f("To be added, though {reason}", &[("reason", note)]),
            (false, _) => gettext_f("Left out, as {reason}", &[("reason", note)]),
        }),
        _ => None,
    });
    if !duplicates.is_empty() {
        let n = duplicates.len();
        let title = ngettext_f(
            "{count} likely duplicate",
            "{count} likely duplicates",
            n,
            &[("count", &grouped(n))],
        );
        parts.push(part(title, duplicates));
    }
    let owned = listed(&|o| {
        (*o == ImportOutcome::AlreadyOwned)
            .then(|| gettext("This copy is already in the collection"))
    });
    if !owned.is_empty() {
        let n = owned.len();
        let title = ngettext_f(
            "{count} book already in the collection",
            "{count} books already in the collection",
            n,
            &[("count", &grouped(n))],
        );
        parts.push(part(title, owned));
    }
    let problems = listed(&|o| match o {
        ImportOutcome::Problem(why) => Some(why.clone()),
        _ => None,
    });
    if !problems.is_empty() {
        let n = problems.len();
        let title = ngettext_f(
            "{count} row cannot be added",
            "{count} rows cannot be added",
            n,
            &[("count", &grouped(n))],
        );
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
        let n = report.photo_problems.len();
        let title = ngettext_f(
            "{count} photo could not be added",
            "{count} photos could not be added",
            n,
            &[("count", &grouped(n))],
        );
        parts.push(part(title, rows));
    }
    parts
}

/// What a progress message says is being done.
#[derive(Clone, Copy)]
enum Doing {
    CheckingRows,
    AddingRows,
    AddingPhotos,
}

/// Says something went wrong, over `parent`, with a button to close it.
fn alert(parent: &adw::Dialog, heading: &str, body: &str) {
    let alert = adw::AlertDialog::new(Some(heading), Some(body));
    alert.add_response("close", &gettext("_Close"));
    alert.present(Some(parent));
}

/// What runs as an import starts, such as keeping a safety copy; the
/// import stops if it fails.
pub type BeforeImport =
    Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>>;

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
    before_import: BeforeImport,
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
        let (page, back, next): (&str, Option<String>, Option<String>) = match stage {
            Stage::Columns => (
                "columns",
                Some(gettext("_Cancel")),
                // Translators: checks every row of the file without
                // saving anything.
                Some(gettext("_Check")),
            ),
            Stage::Checking | Stage::Importing => ("progress", Some(gettext("_Cancel")), None),
            Stage::Checked(0) => ("report", Some(gettext("_Back")), Some(gettext("_Import"))),
            Stage::Checked(n) => (
                "report",
                Some(gettext("_Back")),
                Some(ngettext_f(
                    "_Import {count} Book",
                    "_Import {count} Books",
                    n,
                    &[("count", &grouped(n))],
                )),
            ),
            Stage::Done => ("report", None, Some(gettext("_Done"))),
        };
        self.stack.set_visible_child_name(page);
        self.back.set_visible(back.is_some());
        if let Some(label) = back {
            self.back.set_label(&label);
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
            gettext("Choose what each column holds. Columns left out are not imported.")
        });
        self.columns_group.set_description(Some(&description));
        self.next.set_sensitive(match self.stage.get() {
            Stage::Columns => problem.is_none(),
            Stage::Checked(n) => n > 0,
            _ => true,
        });
    }

    fn set_progress(&self, doing: Doing, done: usize, total: usize) {
        let (done_text, total_text) = (grouped(done.min(total)), grouped(total));
        let values = [("done", done_text.as_str()), ("total", total_text.as_str())];
        let text = match doing {
            Doing::CheckingRows => gettext_f("Checking row {done} of {total}\u{2026}", &values),
            Doing::AddingRows => gettext_f("Adding row {done} of {total}\u{2026}", &values),
            Doing::AddingPhotos => gettext_f("Adding photo {done} of {total}\u{2026}", &values),
        };
        self.progress_label.set_label(&text);
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
                    .title(gettext("The file has no books"))
                    .build(),
            );
            page.add(&group);
        }
        for part in parts {
            let group = adw::PreferencesGroup::builder().title(&part.title).build();
            if !part.rows.is_empty() {
                let expander = adw::ExpanderRow::builder()
                    .title(gettext("Show Each Row"))
                    .expanded(part.rows.len() <= 5)
                    .build();
                for (title, why) in &part.rows {
                    let row = adw::ActionRow::builder()
                        .use_markup(false)
                        .title(title)
                        .subtitle(why)
                        .subtitle_lines(3)
                        .build();
                    expander.add_row(&row);
                }
                if part.more > 0 {
                    let more = adw::ActionRow::builder()
                        .title(ngettext_f(
                            "And {count} more",
                            "And {count} more",
                            part.more,
                            &[("count", &grouped(part.more))],
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

    fn fail(&self, run: ImportRun<'_>, why: &str) {
        let _ = run.abandon();
        alert(&self.dialog, &gettext("Nothing Was Imported"), why);
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
    let doing = if keep {
        Doing::AddingRows
    } else {
        Doing::CheckingRows
    };
    if keep && let Err(why) = (wizard.before_import)().await {
        wizard.show(Stage::Columns);
        alert(&wizard.dialog, &gettext("Nothing Was Imported"), &why);
        return;
    }
    let total = wizard.source.borrow().len();
    wizard.set_progress(doing, 0, total);
    let mut run = match db.start_import(&wizard.source.borrow()) {
        Ok(run) => run,
        Err(e) => {
            wizard.show(Stage::Columns);
            alert(
                &wizard.dialog,
                &gettext("Nothing Was Imported"),
                &e.to_string(),
            );
            return;
        }
    };
    loop {
        if wizard.cancelled.get() {
            let _ = run.abandon();
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
                    wizard.fail(run, &e.to_string());
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
        match run.abandon() {
            Ok(report) => {
                wizard.show_report(&report, false);
                wizard.show(Stage::Checked(report.added()));
            }
            Err(e) => {
                wizard.show(Stage::Columns);
                alert(&wizard.dialog, &gettext("The Check Failed"), &e.to_string());
            }
        }
        return;
    }

    let photos = run.photos().to_vec();
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4));
    for (done, batch) in photos.chunks(workers).enumerate() {
        wizard.set_progress(Doing::AddingPhotos, done * workers, photos.len());
        if wizard.cancelled.get() {
            let _ = run.abandon();
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
                .unwrap_or_else(|_| Err(gettext("the photo could not be read")));
            if let Err(e) = db.import_photo(&mut run, photo, prepared) {
                wizard.fail(run, &e.to_string());
                return;
            }
        }
    }
    match run.finish() {
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
            alert(
                &wizard.dialog,
                &gettext("Nothing Was Imported"),
                &e.to_string(),
            );
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
    before_import: BeforeImport,
    imported: impl Fn(usize) + 'static,
) {
    let mapping = auto_mapping(source.headers());
    let samples = source.preview(SAMPLED).to_vec();

    // A group's title is read as markup, and a file's name is anyone's.
    let file = adw::PreferencesGroup::builder()
        .title(glib::markup_escape_text(file_name))
        .description({
            let n = source.len();
            let count = grouped(n);
            let values = [("count", count.as_str())];
            if source.is_own_format() {
                ngettext_f(
                    "{count} book from MyLongboxDB, so every column is matched already.",
                    "{count} books from MyLongboxDB, so every column is matched already.",
                    n,
                    &values,
                )
            } else {
                ngettext_f("{count} book", "{count} books", n, &values)
            }
        })
        .build();
    let separator = adw::ComboRow::builder()
        .title(gettext("Names in a Cell Are Separated By"))
        .subtitle(gettext("As in \u{201c}Len Wein; Bernie Wrightson\u{201d}"))
        .model(&gtk::StringList::new(
            &SEPARATORS
                .map(|s| gettext(s.0))
                .each_ref()
                .map(String::as_str),
        ))
        .build();
    let guessed = guess_separator(&samples, &mapping);
    let at = SEPARATORS.iter().position(|s| s.1 == guessed).unwrap_or(0);
    separator.set_selected(u32::try_from(at).unwrap_or(0));
    let skip = adw::SwitchRow::builder()
        .title(gettext("Leave Out Likely Duplicates"))
        .subtitle(gettext(
            "Books whose issue, variant, printing, and distribution you already own",
        ))
        .build();
    file.add(&separator);
    file.add(&skip);

    let columns_group = adw::PreferencesGroup::builder()
        .title(gettext("Columns"))
        .build();
    // Field names stay as the columns of MyLongboxDB's own files are
    // named, so they match what an export writes.
    let leave_out = gettext("Leave Out");
    let mut choices = vec![leave_out.as_str()];
    choices.extend(CSV_COLUMNS.iter().copied());
    let model = gtk::StringList::new(&choices);
    let columns: Vec<adw::ComboRow> = source
        .headers()
        .iter()
        .enumerate()
        .map(|(i, header)| {
            let row = adw::ComboRow::builder()
                .use_markup(false)
                .title(if header.is_empty() {
                    gettext("(no heading)")
                } else {
                    header.clone()
                })
                .subtitle(examples(&samples, i))
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
            .label(gettext(
                "Nothing is saved until the end, so cancelling leaves the collection as it was.",
            ))
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

    let back = gtk::Button::with_mnemonic(&gettext("_Cancel"));
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
        .title(gettext("Import Books"))
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
        before_import,
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
        let note = "you already own a copy of this issue, variant, printing, and distribution";
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
