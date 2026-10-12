//! Work on the whole collection, such as backing it up, without holding
//! up the window.
//!
//! The open collection belongs to the window's thread, so it is first
//! copied a few pages at a time between frames; the slow part then runs
//! on that copy in another thread. A progress dialog appears if the work
//! takes more than a moment, and keeps the collection from being changed
//! meanwhile.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{CollectionStore, CopyTarget, Database};

use crate::i18n::{gettext, gettext_f};

/// How long a slice of copying may hold up the window.
const SLICE: Duration = Duration::from_millis(30);
/// Pages copied between checks of the time: 4 MB at SQLite's usual page.
const PAGES: i32 = 1024;
/// How long work may run before its progress dialog appears.
const SHOW_AFTER: Duration = Duration::from_millis(300);

/// A modal dialog showing how work is going, which appears only once
/// the work has taken longer than a moment, or straight away when asked.
pub struct Progress {
    dialog: adw::Dialog,
    label: gtk::Label,
    bar: gtk::ProgressBar,
    finished: Cell<bool>,
}

impl Progress {
    pub fn start(parent: &impl IsA<gtk::Widget>, title: &str, at_once: bool) -> Rc<Self> {
        let label = gtk::Label::builder().wrap(true).build();
        let bar = gtk::ProgressBar::builder().width_request(300).build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(&label);
        content.append(&bar);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(
            &adw::HeaderBar::builder()
                .show_start_title_buttons(false)
                .show_end_title_buttons(false)
                .build(),
        );
        toolbar.set_content(Some(&content));
        let dialog = adw::Dialog::builder()
            .title(title)
            .child(&toolbar)
            .can_close(false)
            .build();
        let progress = Rc::new(Self {
            dialog,
            label,
            bar,
            finished: Cell::new(false),
        });
        let parent = parent.clone().upcast::<gtk::Widget>();
        let show = {
            let progress = Rc::downgrade(&progress);
            move || {
                if let Some(p) = progress.upgrade()
                    && !p.finished.get()
                {
                    p.dialog.present(Some(&parent));
                }
            }
        };
        if at_once {
            show();
        } else {
            glib::timeout_add_local_once(SHOW_AFTER, show);
        }
        progress
    }

    /// Says what is being done, and how far along it is, or that the
    /// share done is not known.
    pub fn set(&self, doing: &str, fraction: Option<f64>) {
        self.label.set_label(doing);
        match fraction {
            Some(f) => self.bar.set_fraction(f.clamp(0.0, 1.0)),
            None => self.bar.pulse(),
        }
    }

    pub fn finish(&self) {
        self.finished.set(true);
        self.dialog.force_close();
    }
}

/// Lets the window draw and handle input before the next slice of work.
pub async fn breathe() {
    // Below the priority of drawing, so each slice gets shown.
    glib::timeout_future_with_priority(glib::Priority::DEFAULT_IDLE, Duration::ZERO).await;
}

/// Copies the open collection to `path`, a slice at a time, showing how
/// far it has got.
pub async fn copy(db: &Database, path: &Path, progress: &Progress) -> Result<PathBuf, String> {
    let doing = gettext("Copying the collection\u{2026}");
    let mut target = CopyTarget::create(path).map_err(|e| e.to_string())?;
    let copied = async {
        let mut job = db.start_copy(&mut target).map_err(|e| e.to_string())?;
        loop {
            let started = Instant::now();
            loop {
                if job.step(PAGES).map_err(|e| e.to_string())? {
                    return Ok::<(), String>(());
                }
                if started.elapsed() >= SLICE {
                    break;
                }
            }
            progress.set(&doing, Some(job.fraction()));
            breathe().await;
        }
    }
    .await;
    match copied {
        Ok(()) => target.finish().map_err(|e| e.to_string()),
        Err(why) => {
            target.discard();
            Err(why)
        }
    }
}

/// Runs `work` on a worker thread, pulsing `progress` with `doing` until
/// it is done.
pub async fn in_background<T: Send + 'static>(
    progress: &Progress,
    doing: &str,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    progress.set(doing, None);
    let job = gio::spawn_blocking(work);
    let pulse = {
        let bar = progress.bar.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            bar.pulse();
            glib::ControlFlow::Continue
        })
    };
    let result = job
        .await
        .unwrap_or_else(|_| Err(gettext("the work stopped unexpectedly")));
    pulse.remove();
    result
}

/// Keeps a copy of collection `name` as it is now, before a change that
/// would be hard to undo.
pub async fn safety_copy(
    store: &CollectionStore,
    name: &str,
    db: &Database,
    reason: &str,
    progress: &Progress,
) -> Result<(), String> {
    let failed = |e: String| {
        gettext_f(
            "the safety copy could not be made: {reason}",
            &[("reason", &e)],
        )
    };
    let staging = store
        .safety_staging(name)
        .map_err(|e| failed(e.to_string()))?;
    let copied = copy(db, &staging, progress).await.map_err(failed)?;
    store
        .adopt_safety_copy(name, &copied, reason)
        .map_err(|e| failed(e.to_string()))
}

/// Whether some work is under way, so a second waits its turn rather
/// than racing the first.
#[derive(Clone, Default)]
pub struct Busy(Rc<RefCell<bool>>);

impl Busy {
    /// Marks work as started, or returns `None` if some already is; the
    /// mark is cleared when the returned guard is dropped.
    pub fn begin(&self) -> Option<BusyGuard> {
        if *self.0.borrow() {
            return None;
        }
        *self.0.borrow_mut() = true;
        Some(BusyGuard(self.0.clone()))
    }
}

pub struct BusyGuard(Rc<RefCell<bool>>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        *self.0.borrow_mut() = false;
    }
}
