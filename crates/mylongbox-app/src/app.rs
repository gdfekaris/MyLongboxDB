use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{
    Amount, BulkChange, Category, CollectionInfo, CollectionStore, CopyId, CopyRow, CoverPhoto,
    Currency, Database, Date, EditionId, FilterSet, ImportSource, IssueId, Prefill, SavedShelf,
    SavedShelfId, SeriesFields, SeriesId, Shelf, ShelfEntry, ShelfList, Side, TrashId, ValueEntry,
};
use mylongbox_media::Edit;
use relm4::typed_view::column::TypedColumnView;
use relm4::typed_view::grid::TypedGridView;
use relm4::typed_view::list::TypedListView;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::books::{self, Book, Manage, Name};
use crate::filter_panel::FilterPanel;
use crate::form::{Focus, Form, Hooks};
use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};
use crate::issue_page::{self, Links};
use crate::settings::{self, Settings};
use crate::sorting::SortBy;
use crate::window::MainWindow;
use crate::{
    about, bulk_dialog, covers, customize, format, gaps_page, import_dialog, photo_editor, prefs,
    prompt, series_dialog, shortcuts, sorting, stats_page, trash_page, viewer, work,
};

/// What a sidebar section shows.
#[derive(Clone, Copy)]
enum Content {
    /// Books, or what to say when there are none.
    Books(Shelf, &'static str, &'static str),
    /// A list of names, each leading to its books.
    Names(ShelfList, &'static str, &'static str),
    Trash,
    /// The numbers missing from each run.
    Gaps,
    /// What the collection holds and is worth, in figures.
    Stats,
}

struct Section {
    /// Names the section in the collection's list of hidden ones. Never
    /// change these.
    code: &'static str,
    name: &'static str,
    content: Content,
}

impl Section {
    /// Everything and Trash are always shown, so the way back to the
    /// books, and to what was thrown away, is never hidden.
    fn locked(&self) -> bool {
        matches!(self.code, "everything" | "trash")
    }
}

const fn books(
    code: &'static str,
    name: &'static str,
    shelf: Shelf,
    empty: &'static str,
    hint: &'static str,
) -> Section {
    Section {
        code,
        name,
        content: Content::Books(shelf, empty, hint),
    }
}

const fn names(
    code: &'static str,
    name: &'static str,
    list: ShelfList,
    empty: &'static str,
    hint: &'static str,
) -> Section {
    Section {
        code,
        name,
        content: Content::Names(list, empty, hint),
    }
}

/// A row of the sidebar.
#[derive(Clone, Copy)]
enum Place {
    Section(&'static Section),
    Saved(SavedShelfId),
}

/// The sidebar, in groups separated by a line.
const SECTIONS: &[&[Section]] = &[
    &[
        books(
            "everything",
            gettext_noop("Everything"),
            Shelf::Everything,
            gettext_noop("Nothing Here Yet"),
            gettext_noop("Comics, magazines, and collected editions you add will appear here."),
        ),
        books(
            "comics",
            gettext_noop("Comics"),
            Shelf::Category(Category::Comic),
            gettext_noop("No Comics"),
            gettext_noop("Comics you add will appear here."),
        ),
        books(
            "magazines",
            gettext_noop("Magazines"),
            Shelf::Category(Category::Magazine),
            gettext_noop("No Magazines"),
            gettext_noop("Magazines you add will appear here."),
        ),
        books(
            "collected",
            gettext_noop("Collected Editions"),
            Shelf::Category(Category::CollectedEdition),
            gettext_noop("No Collected Editions"),
            gettext_noop("Trades, omnibuses, and other books you add will appear here."),
        ),
    ],
    &[
        books(
            "key_issues",
            gettext_noop("Key Issues"),
            Shelf::KeyIssues,
            gettext_noop("No Key Issues"),
            gettext_noop("Issues you mark as key will appear here."),
        ),
        books(
            "graded",
            gettext_noop("Graded Books"),
            Shelf::Graded,
            gettext_noop("No Graded Books"),
            gettext_noop("Books graded by CGC, CBCS, or PGX appear here."),
        ),
        books(
            "signed",
            gettext_noop("Signed Books"),
            Shelf::Signed,
            gettext_noop("No Signed Books"),
            gettext_noop("Books you record a signature on appear here."),
        ),
    ],
    &[
        names(
            "series",
            gettext_noop("Series"),
            ShelfList::Series,
            gettext_noop("No Series"),
            gettext_noop("Series appear here as you add books."),
        ),
        names(
            "publishers",
            gettext_noop("Publishers"),
            ShelfList::Publishers,
            gettext_noop("No Publishers"),
            gettext_noop("Publishers appear here as you add them to series."),
        ),
        names(
            "creators",
            gettext_noop("Creators"),
            ShelfList::Creators,
            gettext_noop("No Creators"),
            gettext_noop("Writers, artists, and editors appear here as you credit them."),
        ),
        names(
            "characters",
            gettext_noop("Characters"),
            ShelfList::Characters,
            gettext_noop("No Characters"),
            gettext_noop("Characters appear here as you add them to issues."),
        ),
        names(
            "teams",
            gettext_noop("Teams"),
            ShelfList::Teams,
            gettext_noop("No Teams"),
            gettext_noop("Teams appear here as you add them to issues."),
        ),
        names(
            "story_arcs",
            gettext_noop("Story Arcs"),
            ShelfList::StoryArcs,
            gettext_noop("No Story Arcs"),
            gettext_noop("Story arcs appear here as you add them to issues."),
        ),
        names(
            "storage",
            gettext_noop("Storage Locations"),
            ShelfList::StorageBoxes,
            gettext_noop("No Storage Locations"),
            gettext_noop("Boxes appear here as you file books in them."),
        ),
    ],
    &[
        Section {
            code: "stats",
            name: gettext_noop("Collection Stats"),
            content: Content::Stats,
        },
        Section {
            code: "gaps",
            name: gettext_noop("Gaps"),
            content: Content::Gaps,
        },
        books(
            "duplicates",
            gettext_noop("Duplicates"),
            Shelf::Duplicates,
            gettext_noop("No Duplicates"),
            gettext_noop("Editions you own more than one copy of appear here."),
        ),
        books(
            "needs_attention",
            gettext_noop("Needs Attention"),
            Shelf::NeedsAttention,
            gettext_noop("Nothing Needs Attention"),
            gettext_noop(
                "Books missing a front cover photo, a grade, a storage box, or a value appear here.",
            ),
        ),
    ],
    &[Section {
        code: "trash",
        name: gettext_noop("Trash"),
        content: Content::Trash,
    }],
];

fn sections() -> impl Iterator<Item = &'static Section> {
    SECTIONS.iter().flat_map(|group| group.iter())
}

/// Which form to open.
#[derive(Clone, Copy)]
enum FormKind {
    New(Category),
    Edit(CopyId),
    From(CopyId, Prefill),
}

pub struct App {
    /// The folder of collections, or why it could not be used.
    store: Result<CollectionStore, String>,
    /// The open collection and its name. Pages hold only weak references,
    /// so closing it here releases its lock.
    open: Option<(String, Rc<Database>)>,
    /// Counts collections opened, so work finishing in the background
    /// can tell whether the one it was for is still open.
    opened: u64,
    /// The add or edit form, while one is open.
    form: Option<Form>,
    /// The copy saved or shown most recently, which "add next issue"
    /// follows on from.
    recent_copy: Option<CopyId>,
    /// The copy to show unfolded on the next book page opened.
    open_copy: Option<CopyId>,
    /// How amounts and grades are shown.
    settings: Settings,
    /// For views built while loading, whose buttons send messages.
    sender: ComponentSender<App>,
    section: &'static Section,
    /// The saved shelf shown in place of `section`, if one is.
    saved: Option<SavedShelfId>,
    /// The saved shelf whose filters the filter panel is changing.
    editing: Option<SavedShelfId>,
    /// What each row of the sidebar opens, in order.
    places: Vec<Place>,
    /// The name-list entry whose books are shown, if any.
    shelf: Option<ShelfEntry>,
    /// The entry being renamed, merged, or deleted: the one shown, or
    /// one picked from its list's menu.
    managed: Option<ShelfEntry>,
    /// What the books last shown were, from `view_key`.
    books_view: Option<String>,
    as_list: bool,
    grid: TypedGridView<Book, gtk::MultiSelection>,
    table: TypedColumnView<Book, gtk::MultiSelection>,
    /// Whether clicks select books, for changing several at once.
    selecting: bool,
    names: TypedListView<Name, gtk::SingleSelection>,
    /// What is typed in the search box.
    query: String,
    filters: Rc<FilterPanel>,
    /// Whether the filter panel is open; its rows apply only then.
    filters_shown: bool,
    /// The spelling the last search was corrected to, if any.
    correction: Option<String>,
    /// How many books the last search found.
    found: usize,
    /// The open collection, for the filter panel to find series in.
    current_db: Rc<RefCell<Weak<Database>>>,
    /// Whether issues in collected editions fill gaps.
    gaps_collected: bool,
    sort: SortBy,
    sort_reversed: bool,
    /// Whether work on the whole collection is under way.
    busy: work::Busy,
    /// Set once quitting has been confirmed, so the window may close.
    quitting: Rc<Cell<bool>>,
}

#[derive(Debug)]
pub enum AppMsg {
    NewCollection,
    CreateCollection(String),
    ImportCollection,
    ImportPicked(PathBuf),
    ExportCollection,
    ExportPicked(PathBuf),
    /// A checked copy was saved, this many bytes in size.
    CopyExported(Result<u64, String>),
    /// Ask where to save a CSV of the collection, or of the books shown.
    ExportCsv(bool),
    /// Write the CSV, of these copies or of every one.
    CsvPicked(PathBuf, Option<Vec<CopyId>>),
    /// Ask where to save a full export.
    ExportZip,
    ZipPicked(PathBuf),
    /// An export finished, of books with their photos or without.
    ExportDone(Result<usize, String>, bool),
    /// Ask for a CSV file or full export to add books from.
    ImportBooks,
    BooksFilePicked(PathBuf),
    /// The import wizard added this many books.
    BooksImported(usize),
    /// Check the open collection for damage.
    CheckDamage,
    CheckDone(Result<Vec<String>, String>),
    /// Compact the open collection.
    Compact,
    /// A compacted copy is ready to take the collection's place.
    CompactReady(Result<(PathBuf, u64, u64), String>),
    /// Show a message, from work that finished in the background.
    Notify(String),
    ShowShortcuts,
    ShowAbout,
    /// Close the window, asking first if a form has unsaved changes.
    Quit,
    QuitNow,
    /// Bring back a collection's safety copy as a collection of its own.
    RestoreSafetyCopy(String),
    SectionSelected(usize),
    NameActivated(u32),
    /// A book in the grid or list, by position.
    BookActivated(u32),
    OpenIssue(IssueId),
    /// Opens a book's page as seen from one of its copies.
    OpenCopy(IssueId, CopyId),
    /// View the open issue's page from another of its copies.
    SelectCopy(CopyId),
    Add(Category),
    EditCopy(CopyId),
    /// Start a new book from a copy.
    AddFrom(CopyId, Prefill),
    /// Start the issue after the copy saved or opened most recently.
    AddNext,
    /// Ask for a new value of a copy.
    AddValue(CopyId),
    SaveValue(CopyId, Date, Amount, String),
    DeleteValue(CopyId, ValueEntry),
    /// Ask for a photo file for one side of a copy.
    AddPhoto(CopyId, Side),
    PhotoPicked(CopyId, Side, PathBuf),
    /// The editor closed with an edit for a photo loaded earlier.
    PhotoEdited {
        opened: u64,
        copy: CopyId,
        side: Side,
        loaded: Box<covers::Loaded>,
        edit: Edit,
    },
    RemovePhoto(CopyId, Side),
    /// Put back a removed photo.
    RestorePhoto(CopyId, Side, Box<CoverPhoto>),
    SwapPhotos(CopyId),
    ViewPhoto(CopyId, Side),
    /// Open Preferences, on the exchange rates if asked.
    ShowPreferences(bool),
    SettingsChanged(Settings),
    RatesChanged,
    OpenCollectionNamed(String),
    RenameCollection(String),
    DoRenameCollection(String, String),
    ConfirmDeleteCollection(String),
    DoDeleteCollection(String),
    /// Do something to an entry of a name list: the one whose books are
    /// shown, or the one at this position in the list shown.
    Manage(Manage, Option<u32>),
    /// Ask for a series' new details, from the page of this copy if any.
    EditSeries(SeriesId, Option<CopyId>),
    SaveSeries(SeriesId, Box<SeriesFields>, String, Option<CopyId>),
    RenameEntry,
    EntryAliases,
    MergeEntry,
    ConfirmDeleteEntry,
    DoRename(String),
    DoMerge(String),
    DoDeleteEntry,
    Selecting(bool),
    SelectionChanged,
    /// Ask what to change on the selected copies.
    BulkEdit,
    ApplyBulk(Vec<CopyId>, BulkChange),
    /// Make the change, once a safety copy is kept.
    DoBulk(Vec<CopyId>, BulkChange),
    TrashSelected,
    TrashCopy(CopyId),
    TrashEdition(EditionId),
    TrashIssue(IssueId),
    Restore(TrashId),
    /// Ask before deleting a group in Trash for good.
    ConfirmPurge(TrashId, String),
    Purge(TrashId),
    DoPurge(TrashId),
    ConfirmEmpty,
    EmptyTrash,
    DoEmptyTrash,
    FormSave,
    /// Leave the form, asking first if it has unsaved changes.
    FormCancel,
    FormClose,
    ShelfBack,
    ViewChanged(bool),
    SearchChanged(String),
    ShowFilters(bool),
    /// Keep the filters shown as a new shelf, or as the one being edited.
    SaveShelf,
    AddShelf(String),
    RenameShelf(SavedShelfId),
    ShelfRenamed(SavedShelfId, String),
    /// Open the filter panel on a shelf's filters, to change them.
    EditShelf(SavedShelfId),
    ConfirmRemoveShelf(SavedShelfId),
    RemoveShelf(SavedShelfId),
    CustomizeSidebar,
    SectionHidden(&'static str, bool),
    /// Move a shelf one place earlier in the sidebar, or later.
    MoveShelf(SavedShelfId, bool),
    FiltersChanged,
    /// Put the corrected spelling in the search box.
    UseCorrection,
    /// Whether issues in collected editions fill gaps.
    GapsCountCollected(bool),
    /// Show a series' books, as from its gaps.
    OpenSeries(SeriesId, String),
    Sort(SortBy, bool),
}

#[derive(Debug)]
pub enum CommandMsg {
    Imported(Result<CollectionInfo, String>),
    /// A file to add books from, read for the collection opened as
    /// number `opened`.
    BooksFileRead {
        opened: u64,
        name: String,
        source: Result<Box<ImportSource>, String>,
    },
    /// A photo file read and ready for the editor, for the collection
    /// opened as number `opened`.
    PhotoLoaded {
        opened: u64,
        copy: CopyId,
        side: Side,
        loaded: Result<Box<covers::Loaded>, String>,
    },
    /// A photo edited and saved as JPEGs, ready to store in the
    /// collection opened as number `opened`.
    PhotoReady {
        opened: u64,
        copy: CopyId,
        side: Side,
        photo: Result<Box<CoverPhoto>, String>,
    },
}

impl Component for App {
    type CommandOutput = CommandMsg;
    type Input = AppMsg;
    type Output = ();
    type Init = ();
    type Root = MainWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        MainWindow::new()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // Leave no trace of which files were picked in GTK's recently
        // used list.
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_recent_files_enabled(false);
        }

        add_action(&root, "new-collection", &sender, || AppMsg::NewCollection);
        add_action(&root, "import-collection", &sender, || {
            AppMsg::ImportCollection
        });
        add_action(&root, "export-csv", &sender, || AppMsg::ExportCsv(false));
        add_action(&root, "export-zip", &sender, || AppMsg::ExportZip);
        add_action(&root, "import-books", &sender, || AppMsg::ImportBooks);
        add_action(&root, "export-shown-csv", &sender, || {
            AppMsg::ExportCsv(true)
        });
        add_action(&root, "export-collection", &sender, || {
            AppMsg::ExportCollection
        });
        add_action(&root, "shelf-back", &sender, || AppMsg::ShelfBack);
        add_action(&root, "add-comic", &sender, || AppMsg::Add(Category::Comic));
        add_action(&root, "add-magazine", &sender, || {
            AppMsg::Add(Category::Magazine)
        });
        add_action(&root, "add-collected", &sender, || {
            AppMsg::Add(Category::CollectedEdition)
        });
        add_action(&root, "add-next", &sender, || AppMsg::AddNext);
        relm4::main_application().set_accels_for_action("win.add-next", &["<Control><Shift>n"]);
        relm4::main_application().set_accels_for_action("win.add-comic", &["<Control>n"]);
        relm4::main_application().set_accels_for_action("win.shelf-back", &["<Alt>Left"]);
        {
            let sender = sender.clone();
            root.sidebar().connect_row_activated(move |_, row| {
                if let Ok(index) = usize::try_from(row.index()) {
                    sender.input(AppMsg::SectionSelected(index));
                }
            });
        }
        add_action(&root, "customize-sidebar", &sender, || {
            AppMsg::CustomizeSidebar
        });

        let css = gtk::CssProvider::new();
        css.load_from_string(books::CSS);
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(&root),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let grid = TypedGridView::<Book, gtk::MultiSelection>::new();
        grid.view.set_max_columns(24);
        let table = books::book_table();
        for model in [&grid.selection_model, &table.selection_model] {
            let sender = sender.clone();
            model.connect_selection_changed(move |_, _, _| sender.input(AppMsg::SelectionChanged));
        }
        {
            let sender = sender.clone();
            root.select_toggle()
                .connect_active_notify(move |t| sender.input(AppMsg::Selecting(t.is_active())));
        }
        add_action(&root, "bulk-edit", &sender, || AppMsg::BulkEdit);
        add_action(&root, "preferences", &sender, || {
            AppMsg::ShowPreferences(false)
        });
        add_action(&root, "edit-rates", &sender, || {
            AppMsg::ShowPreferences(true)
        });
        relm4::main_application().set_accels_for_action("win.preferences", &["<Control>comma"]);
        add_action(&root, "rename-entry", &sender, || {
            AppMsg::Manage(Manage::Rename, None)
        });
        add_action(&root, "entry-aliases", &sender, || {
            AppMsg::Manage(Manage::Aliases, None)
        });
        add_action(&root, "merge-entry", &sender, || {
            AppMsg::Manage(Manage::Merge, None)
        });
        add_action(&root, "edit-series-entry", &sender, || {
            AppMsg::Manage(Manage::EditSeries, None)
        });
        {
            let manage = gio::SimpleAction::new("manage-name", Some(glib::VariantTy::STRING));
            let s = sender.clone();
            manage.connect_activate(move |_, target| {
                if let Some((what, position)) = target
                    .and_then(|t| t.get::<String>())
                    .and_then(|t| books::parse_manage_target(&t))
                {
                    s.input(AppMsg::Manage(what, Some(position)));
                }
            });
            root.add_action(&manage);
        }
        add_action(&root, "delete-entry", &sender, || {
            AppMsg::Manage(Manage::Delete, None)
        });
        add_action(&root, "trash-selected", &sender, || AppMsg::TrashSelected);
        relm4::main_application().set_accels_for_action("win.trash-selected", &["Delete"]);
        table.view.add_css_class("data-table");
        table.view.set_reorderable(false);
        let names = TypedListView::<Name, gtk::SingleSelection>::new();
        names.selection_model.set_autoselect(false);
        names.view.set_single_click_activate(true);
        names.view.add_css_class("navigation-sidebar");
        {
            let sender = sender.clone();
            names
                .view
                .connect_activate(move |_, position| sender.input(AppMsg::NameActivated(position)));
        }
        grid.view.set_single_click_activate(true);
        table.view.set_single_click_activate(true);
        {
            let sender = sender.clone();
            grid.view
                .connect_activate(move |_, position| sender.input(AppMsg::BookActivated(position)));
        }
        {
            let sender = sender.clone();
            table
                .view
                .connect_activate(move |_, position| sender.input(AppMsg::BookActivated(position)));
        }
        root.set_views(&grid.view, &table.view, &names.view);
        for (name, view, keys) in [
            ("view-grid", "grid", "<Control>1"),
            ("view-list", "list", "<Control>2"),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let toggle = root.view_toggle();
            action.connect_activate(move |_, _| toggle.set_active_name(Some(view)));
            root.add_action(&action);
            relm4::main_application().set_accels_for_action(&format!("win.{name}"), &[keys]);
        }
        add_action(&root, "shortcuts", &sender, || AppMsg::ShowShortcuts);
        add_action(&root, "about", &sender, || AppMsg::ShowAbout);
        relm4::main_application().set_accels_for_action("win.shortcuts", &["<Control>question"]);
        add_action(&root, "quit", &sender, || AppMsg::Quit);
        relm4::main_application().set_accels_for_action("win.quit", &["<Control>q"]);
        // Closing the window asks about unsaved changes, as Cancel does.
        let quitting: Rc<Cell<bool>> = Rc::default();
        {
            let (sender, quitting) = (sender.clone(), quitting.clone());
            root.connect_close_request(move |_| {
                if quitting.get() {
                    return glib::Propagation::Proceed;
                }
                sender.input(AppMsg::Quit);
                glib::Propagation::Stop
            });
        }
        {
            let sender = sender.clone();
            root.view_toggle()
                .connect_active_name_notify(move |toggle| {
                    sender.input(AppMsg::ViewChanged(
                        toggle.active_name().as_deref() == Some("list"),
                    ));
                });
        }

        let store = CollectionStore::new(glib::user_data_dir().join("collections"))
            .map_err(|e| e.to_string());
        let current_db: Rc<RefCell<Weak<Database>>> = Rc::default();
        let filters = {
            let (changed, db) = (sender.clone(), current_db.clone());
            let save = sender.clone();
            FilterPanel::new(
                move || changed.input(AppMsg::FiltersChanged),
                Rc::new(move |label: &str| {
                    let db = db.borrow().upgrade()?;
                    let label = label.trim().to_lowercase();
                    db.list_series().ok()?.into_iter().find_map(|s| {
                        let f = &s.fields;
                        let listed = mylongbox_core::series_label(&f.name, f.start_year, f.volume);
                        mylongbox_core::same_series_label(&label, &listed).then_some(s.id)
                    })
                }),
                move || save.input(AppMsg::SaveShelf),
            )
        };
        root.set_filter_panel(filters.widget());
        {
            let sender = sender.clone();
            root.search_entry().connect_search_changed(move |entry| {
                sender.input(AppMsg::SearchChanged(entry.text().into()));
            });
        }
        {
            let sender = sender.clone();
            root.filter_toggle()
                .connect_toggled(move |b| sender.input(AppMsg::ShowFilters(b.is_active())));
        }
        {
            let sender = sender.clone();
            root.correction_banner()
                .connect_button_clicked(move |_| sender.input(AppMsg::UseCorrection));
        }
        {
            // The sort menu's choices: what to sort by, and which way.
            let sort = gio::SimpleAction::new_stateful(
                "sort",
                Some(glib::VariantTy::STRING),
                &"usual".to_variant(),
            );
            let reversed =
                gio::SimpleAction::new_stateful("sort-reversed", None, &false.to_variant());
            let send = {
                let (sender, sort, reversed) = (sender.clone(), sort.clone(), reversed.clone());
                move || {
                    let by = sort
                        .state()
                        .and_then(|s| s.get::<String>())
                        .map_or(SortBy::Usual, |n| SortBy::from_name(&n));
                    let back = reversed
                        .state()
                        .and_then(|s| s.get::<bool>())
                        .unwrap_or(false);
                    sender.input(AppMsg::Sort(by, back));
                }
            };
            let send = Rc::new(send);
            {
                let send = send.clone();
                sort.connect_change_state(move |action, value| {
                    if let Some(value) = value {
                        action.set_state(value);
                        send();
                    }
                });
            }
            reversed.connect_activate(move |action, _| {
                let on = action
                    .state()
                    .and_then(|s| s.get::<bool>())
                    .unwrap_or(false);
                action.set_state(&(!on).to_variant());
                send();
            });
            root.add_action(&sort);
            root.add_action(&reversed);
        }
        {
            // Ctrl+F opens the search box, or closes it.
            let search = gio::SimpleAction::new("search", None);
            let toggle = root.search_toggle();
            search.connect_activate(move |_, _| toggle.set_active(!toggle.is_active()));
            root.add_action(&search);
            relm4::main_application().set_accels_for_action("win.search", &["<Control>f"]);
        }

        let mut model = App {
            store,
            open: None,
            opened: 0,
            form: None,
            recent_copy: None,
            open_copy: None,
            settings: Settings::load(&settings::path()),
            sender: sender.clone(),
            section: &SECTIONS[0][0],
            saved: None,
            editing: None,
            places: Vec::new(),
            shelf: None,
            managed: None,
            books_view: None,
            as_list: false,
            grid,
            table,
            selecting: false,
            names,
            query: String::new(),
            filters,
            filters_shown: false,
            correction: None,
            found: 0,
            current_db,
            gaps_collected: false,
            sort: SortBy::Usual,
            sort_reversed: false,
            busy: work::Busy::default(),
            quitting: quitting.clone(),
        };

        let startup = match &model.store {
            Ok(store) => store
                .last_opened()
                .or_else(|| store.list().ok()?.into_iter().next().map(|c| c.name)),
            Err(why) => {
                root.toast(&gettext_f(
                    "Collections cannot be stored: {reason}",
                    &[("reason", why.as_str())],
                ));
                None
            }
        };
        match startup {
            Some(name) => model.open_collection(&name, &root),
            None => {
                model.refresh_sidebar(&root);
                model.show(&root);
            }
        }
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: AppMsg, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::NewCollection => ask_for_name(root, &sender),
            AppMsg::CreateCollection(name) => match self
                .store()
                .and_then(|s| s.create(&name).map_err(|e| e.to_string()))
            {
                Ok(info) => self.open_collection(&info.name, root),
                Err(why) => root.toast(&why),
            },
            AppMsg::ImportCollection => pick_file_to_import(root, &sender),
            AppMsg::ImportPicked(path) => match self.store() {
                Ok(store) => {
                    let store = store.clone();
                    root.toast_working(&gettext("Importing…"));
                    sender.spawn_oneshot_command(move || {
                        CommandMsg::Imported(store.import(&path).map_err(|e| e.to_string()))
                    });
                }
                Err(why) => root.toast(&why),
            },
            AppMsg::ExportCollection => match (&self.open, glib::DateTime::now_local()) {
                (Some((name, _)), Ok(now)) => {
                    pick_export_file(root, &sender, &format::copy_name(name, &now));
                }
                (Some((name, _)), Err(_)) => {
                    pick_export_file(root, &sender, &format!("{name}.sqlite"));
                }
                (None, _) => root.toast(&gettext("Open a collection first")),
            },
            AppMsg::ExportPicked(path) => self.export_copy(path, root, &sender),
            AppMsg::CopyExported(result) => match result {
                Ok(size) => root.toast(&gettext_f(
                    "Saved a checked copy ({size})",
                    &[("size", &glib::format_size(size))],
                )),
                Err(why) => root.toast(&gettext_f(
                    "Could not save a copy: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ExportCsv(shown) => {
                let Some((name, _)) = &self.open else {
                    root.toast(&gettext("Open a collection first"));
                    return;
                };
                let (copies, file) = if shown {
                    let ids: Vec<CopyId> = (0..self.grid.len())
                        .filter_map(|i| self.grid.get(i).map(|b| b.borrow().0.copy))
                        .collect();
                    if ids.is_empty() || !root.is_showing_books() {
                        root.toast(&gettext("No books are shown to export"));
                        return;
                    }
                    (
                        Some(ids),
                        format!(
                            "{name} \u{2013} {}.csv",
                            format::file_name_part(&self.shown_title())
                        ),
                    )
                } else {
                    (None, format!("{name}.csv"))
                };
                pick_csv_file(root, &sender, &file, copies);
            }
            AppMsg::CsvPicked(path, copies) => {
                self.export(root, &sender, false, move |db| {
                    let file = std::fs::File::create(&path)?;
                    let mut out = std::io::BufWriter::new(file);
                    let count = db.export_csv(copies.as_deref(), &mut out, false)?;
                    std::io::Write::flush(&mut out)?;
                    Ok(count)
                });
            }
            AppMsg::ExportZip => match &self.open {
                Some((name, _)) => pick_zip_file(root, &sender, &format!("{name}.zip")),
                None => root.toast(&gettext("Open a collection first")),
            },
            AppMsg::ZipPicked(path) => {
                // Written whole in the app's own cache first, as a zip
                // file is finished by going back to its start, then copied
                // to where it was asked for.
                let staging = glib::user_cache_dir().join("mylongboxdb-export.zip");
                self.export(root, &sender, true, move |db| {
                    if let Some(dir) = staging.parent() {
                        std::fs::create_dir_all(dir)?;
                    }
                    let written = (|| {
                        let file = std::fs::File::create(&staging)?;
                        let count = db.export_zip(None, std::io::BufWriter::new(file))?;
                        std::fs::copy(&staging, &path)?;
                        Ok(count)
                    })();
                    let _ = std::fs::remove_file(&staging);
                    written
                });
            }
            AppMsg::ExportDone(result, photos) => match result {
                Ok(n) if photos => root.toast(&ngettext_f(
                    "Exported {count} book with its photos",
                    "Exported {count} books with their photos",
                    n,
                    &[("count", &n.to_string())],
                )),
                Ok(n) => root.toast(&ngettext_f(
                    "Exported {count} book",
                    "Exported {count} books",
                    n,
                    &[("count", &n.to_string())],
                )),
                Err(why) => root.toast(&gettext_f(
                    "Could not export: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ImportBooks => match &self.open {
                Some(_) => pick_books_file(root, &sender),
                None => root.toast(&gettext("Open a collection first")),
            },
            AppMsg::BooksFilePicked(path) => {
                root.toast_working(&gettext("Reading the file\u{2026}"));
                let opened = self.opened;
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                sender.spawn_oneshot_command(move || CommandMsg::BooksFileRead {
                    opened,
                    name,
                    source: ImportSource::open(&path)
                        .map(Box::new)
                        .map_err(|e| e.to_string()),
                });
            }
            AppMsg::BooksImported(added) => {
                self.show(root);
                root.toast(&match added {
                    0 => gettext("No books were added"),
                    n => ngettext_f(
                        "Imported {count} book",
                        "Imported {count} books",
                        n,
                        &[("count", &format::grouped(&n.to_string()))],
                    ),
                });
            }
            AppMsg::CheckDamage => self.check_damage(root, &sender),
            AppMsg::CheckDone(Ok(problems)) if problems.is_empty() => {
                root.toast(&gettext("No damage found"));
            }
            AppMsg::CheckDone(Ok(problems)) => {
                let mut body = gettext(
                    "Open a backup or the safety copy to get back to a sound copy. What was \
                     found:\n\n",
                );
                for p in problems.iter().take(10) {
                    body.push_str(&format!("\u{2022} {p}\n"));
                }
                if problems.len() > 10 {
                    let more = problems.len() - 10;
                    body.push_str(&ngettext_f(
                        "and {count} more",
                        "and {count} more",
                        more,
                        &[("count", &more.to_string())],
                    ));
                }
                let heading = gettext("The Collection Is Damaged");
                let alert = adw::AlertDialog::new(Some(&heading), Some(&body));
                alert.add_response("close", &gettext("_Close"));
                alert.present(Some(root));
            }
            AppMsg::CheckDone(Err(why)) => {
                root.toast(&gettext_f("Could not check: {reason}", &[("reason", &why)]))
            }
            AppMsg::Compact => self.compact(root, &sender),
            AppMsg::CompactReady(result) => self.finish_compact(result, root),
            AppMsg::Notify(message) => root.toast(&message),
            AppMsg::ShowAbout => {
                let size = self.open.as_ref().and_then(|(name, db)| {
                    let total = self
                        .store()
                        .ok()?
                        .list()
                        .ok()?
                        .into_iter()
                        .find(|c| &c.name == name)?
                        .size_bytes;
                    Some(about::Size {
                        collection: name.clone(),
                        total,
                        photos: db.photo_bytes().ok()?,
                    })
                });
                about::show(root, size);
            }
            AppMsg::ShowShortcuts => shortcuts::dialog().present(Some(root)),
            AppMsg::Quit => match &self.form {
                Some(form) if form.is_dirty() => {
                    confirm_discard(root, &sender, Some(AppMsg::QuitNow));
                }
                _ => sender.input(AppMsg::QuitNow),
            },
            AppMsg::QuitNow => {
                self.quitting.set(true);
                root.close();
            }
            AppMsg::RestoreSafetyCopy(name) => {
                match self
                    .store()
                    .and_then(|s| s.restore_safety_copy(&name).map_err(|e| e.to_string()))
                {
                    Ok(info) => {
                        self.open_collection(&info.name, root);
                        root.toast(&gettext_f(
                            "Opened the safety copy as \u{201c}{name}\u{201d}",
                            &[("name", &info.name)],
                        ));
                    }
                    Err(why) => root.toast(&gettext_f(
                        "Could not open the safety copy: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::SectionSelected(index) => {
                let Some(place) = self.places.get(index).copied() else {
                    return;
                };
                match place {
                    Place::Section(section) => {
                        self.section = section;
                        self.saved = None;
                    }
                    Place::Saved(id) => self.saved = Some(id),
                }
                self.shelf = None;
                // Choosing a section starts it at the top, even the one shown.
                self.books_view = None;
                self.end_search(root);
                root.pop_to_section();
                self.show(root);
                root.show_content();
            }
            AppMsg::NameActivated(position) => {
                if let Some(entry) = self.names.get_visible(position) {
                    self.shelf = Some(entry.borrow().0.clone());
                    self.show(root);
                }
            }
            AppMsg::BookActivated(position) => {
                // Both views hold the same rows in the same order.
                if let Some(book) = self.grid.get_visible(position) {
                    let (issue, copy) = {
                        let row = &book.borrow().0;
                        (row.issue, row.copy)
                    };
                    self.open_copy = Some(copy);
                    self.open_issue(issue, root, &sender);
                }
            }
            AppMsg::OpenIssue(issue) => self.open_issue(issue, root, &sender),
            AppMsg::OpenCopy(issue, copy) => {
                self.open_copy = Some(copy);
                self.open_issue(issue, root, &sender);
            }
            AppMsg::SelectCopy(copy) => self.refresh_copy(copy, root, &sender),
            AppMsg::Add(category) => self.open_form(FormKind::New(category), root, &sender),
            AppMsg::EditCopy(copy) => self.open_form(FormKind::Edit(copy), root, &sender),
            AppMsg::AddFrom(copy, kind) => {
                self.open_form(FormKind::From(copy, kind), root, &sender);
            }
            AppMsg::AddNext => match self.recent_copy {
                Some(copy) => {
                    self.open_form(FormKind::From(copy, Prefill::NextIssue), root, &sender);
                }
                None => root.toast(&gettext(
                    "Open or add a book first, then add the issue after it",
                )),
            },
            AppMsg::FormSave => self.save_form(root, &sender),
            AppMsg::FormCancel => match &self.form {
                Some(form) if form.is_dirty() => confirm_discard(root, &sender, None),
                Some(_) => sender.input(AppMsg::FormClose),
                None => {}
            },
            AppMsg::FormClose => {
                if self.form.take().is_some() {
                    root.pop_page();
                }
            }
            AppMsg::AddValue(copy) => ask_value(root, &sender, copy),
            AppMsg::SaveValue(copy, date, value, source) => {
                match self.with_db(|db| db.add_value(copy, date, value, &source)) {
                    Ok(_) => {
                        self.refresh_copy(copy, root, &sender);
                        root.toast(&gettext("Value added"));
                    }
                    Err(why) => root.toast(&gettext_f(
                        "Could not add the value: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::DeleteValue(copy, entry) => {
                match self.with_db(|db| db.delete_value(entry.id)) {
                    Ok(()) => {
                        self.refresh_copy(copy, root, &sender);
                        let undo = sender.clone();
                        root.toast_undo(&gettext("Value deleted"), move || {
                            undo.input(AppMsg::SaveValue(
                                copy,
                                entry.date,
                                entry.value,
                                entry.source.clone(),
                            ));
                        });
                    }
                    Err(why) => root.toast(&gettext_f(
                        "Could not delete the value: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::AddPhoto(copy, side) => pick_photo(root, &sender, copy, side),
            AppMsg::PhotoPicked(copy, side, path) => {
                root.toast_working(&gettext("Opening the photo\u{2026}"));
                let opened = self.opened;
                sender.spawn_oneshot_command(move || CommandMsg::PhotoLoaded {
                    opened,
                    copy,
                    side,
                    loaded: covers::load(&path).map(Box::new),
                });
            }
            AppMsg::PhotoEdited {
                opened,
                copy,
                side,
                loaded,
                edit,
            } => {
                root.toast_working(&gettext("Saving the photo\u{2026}"));
                let output = self.settings.photos;
                sender.spawn_oneshot_command(move || CommandMsg::PhotoReady {
                    opened,
                    copy,
                    side,
                    photo: covers::finish(&loaded.full, &edit, output).map(Box::new),
                });
            }
            AppMsg::RemovePhoto(copy, side) => match self.with_db(|db| db.remove_photo(copy, side))
            {
                Ok(photo) => {
                    covers::forget(copy);
                    self.refresh_copy(copy, root, &sender);
                    let undo = sender.clone();
                    let photo = Box::new(photo);
                    root.toast_undo(&photo_message(side, PhotoDone::Removed), move || {
                        undo.input(AppMsg::RestorePhoto(copy, side, photo.clone()));
                    });
                }
                Err(why) => root.toast(&gettext_f(
                    "Could not remove the photo: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::RestorePhoto(copy, side, photo) => {
                self.store_photo(copy, side, &photo, PhotoDone::PutBack, root, &sender);
            }
            AppMsg::SwapPhotos(copy) => match self.with_db(|db| db.swap_photos(copy)) {
                Ok(()) => {
                    covers::forget(copy);
                    self.refresh_copy(copy, root, &sender);
                    root.toast(&gettext("Front and back swapped"));
                }
                Err(why) => root.toast(&gettext_f(
                    "Could not swap the photos: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ViewPhoto(copy, side) => self.view_photo(copy, side, root),
            AppMsg::SearchChanged(query) => {
                if query != self.query {
                    self.query = query;
                    root.pop_to_section();
                    self.show(root);
                }
            }
            AppMsg::ShowFilters(shown) => {
                self.filters_shown = shown;
                // Closing the panel ends editing a shelf, unsaved.
                if !shown && self.editing.take().is_some() {
                    self.filters.clear();
                }
                self.update_save_button();
                root.pop_to_section();
                self.show(root);
            }
            AppMsg::FiltersChanged => {
                self.update_save_button();
                if self.filters_shown {
                    root.pop_to_section();
                    self.show(root);
                }
            }
            AppMsg::SaveShelf => self.save_shelf(root),
            AppMsg::AddShelf(name) => {
                let Some((_, db)) = &self.open else {
                    return;
                };
                match db.add_saved_shelf(&name, &self.filters.filter_set()) {
                    Ok(id) => {
                        self.go_to_shelf(id, root);
                        root.toast(&gettext_f(
                            "Added \u{201c}{name}\u{201d} to the sidebar",
                            &[("name", name.trim())],
                        ));
                    }
                    Err(why) => root.toast(&why.to_string()),
                }
            }
            AppMsg::RenameShelf(id) => self.ask_shelf_name(id, root),
            AppMsg::ShelfRenamed(id, name) => {
                if let Some((_, db)) = &self.open {
                    match db.rename_saved_shelf(id, &name) {
                        Ok(()) => {
                            self.refresh_sidebar(root);
                            self.show_header(root);
                        }
                        Err(why) => root.toast(&why.to_string()),
                    }
                }
            }
            AppMsg::EditShelf(id) => self.edit_shelf(id, root),
            AppMsg::ConfirmRemoveShelf(id) => {
                let Some(shelf) = self.with_db(|db| db.saved_shelf(id)).ok() else {
                    return;
                };
                confirm(
                    root,
                    &gettext("Remove Shelf?"),
                    &gettext_f(
                        "\u{201c}{name}\u{201d} is removed from the sidebar. The books it \
                         shows stay in the collection.",
                        &[("name", &shelf.name)],
                    ),
                    &gettext("_Remove"),
                    &sender,
                    move || AppMsg::RemoveShelf(id),
                );
            }
            AppMsg::RemoveShelf(id) => {
                if let Err(why) = self.with_db(|db| db.delete_saved_shelf(id)) {
                    root.toast(&why);
                    return;
                }
                if self.editing == Some(id) {
                    self.end_search(root);
                }
                if self.refresh_sidebar(root) {
                    root.pop_to_section();
                    self.show(root);
                }
            }
            AppMsg::CustomizeSidebar => self.customize_sidebar(root),
            AppMsg::SectionHidden(code, hidden) => {
                if let Err(why) = self.with_db(|db| db.set_section_hidden(code, hidden)) {
                    root.toast(&why);
                }
                if self.refresh_sidebar(root) {
                    root.pop_to_section();
                    self.show(root);
                }
            }
            AppMsg::MoveShelf(id, earlier) => {
                if let Err(why) = self.with_db(|db| db.move_saved_shelf(id, earlier)) {
                    root.toast(&why);
                }
                self.refresh_sidebar(root);
            }
            AppMsg::Sort(by, reversed) => {
                self.sort = by;
                self.sort_reversed = reversed;
                if root.is_showing_books() {
                    self.show(root);
                }
            }
            AppMsg::GapsCountCollected(on) => {
                self.gaps_collected = on;
                self.show(root);
            }
            AppMsg::OpenSeries(series, name) => {
                self.shelf = Some(ShelfEntry {
                    shelf: Shelf::Series(series),
                    name,
                    detail: String::new(),
                    copies: 0,
                });
                self.show(root);
            }
            AppMsg::UseCorrection => {
                if let Some(corrected) = self.correction.take() {
                    root.search_entry().set_text(&corrected);
                }
            }
            AppMsg::ShowPreferences(show_rates) => self.show_preferences(show_rates, root, &sender),
            AppMsg::SettingsChanged(settings) => {
                self.settings = settings;
                if let Err(why) = settings.save(&settings::path()) {
                    root.toast(&gettext_f(
                        "Could not save the preferences: {reason}",
                        &[("reason", &why.to_string())],
                    ));
                }
                self.refresh_display(root);
                root.pop_to_section();
                self.show(root);
            }
            AppMsg::RatesChanged => {
                self.refresh_display(root);
                root.pop_to_section();
                self.show(root);
            }
            AppMsg::OpenCollectionNamed(name) => self.open_collection(&name, root),
            AppMsg::RenameCollection(name) => {
                let s = sender.clone();
                let old = name.clone();
                prompt::ask_text(
                    root,
                    prompt::Ask {
                        heading: &gettext("Rename Collection"),
                        body: &gettext(
                            "Only the name changes; everything in the collection stays.",
                        ),
                        field: &gettext("Name"),
                        initial: &name,
                        action: &gettext("_Rename"),
                        suggest: None,
                    },
                    move |new| s.input(AppMsg::DoRenameCollection(old.clone(), new)),
                );
            }
            AppMsg::DoRenameCollection(old, new) => {
                // An open collection is closed so its file can be renamed,
                // then opened again under the new name.
                let was_open = self.open.as_ref().is_some_and(|(n, _)| *n == old);
                if was_open {
                    self.form = None;
                    root.pop_to_section();
                    self.open = None;
                }
                let renamed = self
                    .store()
                    .and_then(|s| s.rename(&old, &new).map_err(|e| e.to_string()));
                match renamed {
                    Ok(info) => {
                        if was_open {
                            self.open_collection(&info.name, root);
                        }
                        root.toast(&gettext_f(
                            "Renamed to \u{201c}{name}\u{201d}",
                            &[("name", &info.name)],
                        ));
                    }
                    Err(why) => {
                        if was_open {
                            self.open_collection(&old, root);
                        }
                        root.toast(&gettext_f(
                            "Could not rename: {reason}",
                            &[("reason", &why)],
                        ));
                    }
                }
            }
            AppMsg::ConfirmDeleteCollection(name) => {
                let doomed = name.clone();
                confirm(
                    root,
                    &gettext("Delete Collection?"),
                    &gettext_f(
                        "\u{201c}{name}\u{201d} and everything in it will be deleted for good. \
                         Export a copy first if you may want it back.",
                        &[("name", &name)],
                    ),
                    &gettext("_Delete"),
                    &sender,
                    move || AppMsg::DoDeleteCollection(doomed.clone()),
                );
            }
            AppMsg::DoDeleteCollection(name) => {
                match self
                    .store()
                    .and_then(|s| s.delete(&name).map_err(|e| e.to_string()))
                {
                    Ok(()) => root.toast(&gettext_f(
                        "Deleted \u{201c}{name}\u{201d}",
                        &[("name", &name)],
                    )),
                    Err(why) => root.toast(&gettext_f(
                        "Could not delete: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::Manage(what, shelf) => {
                self.managed = match shelf {
                    None => self.shelf.clone(),
                    Some(at) => self.names.get(at).map(|n| n.borrow().0.clone()),
                };
                match (what, self.managed.as_ref().map(|e| e.shelf)) {
                    (_, None) => {}
                    (Manage::Rename, _) => sender.input(AppMsg::RenameEntry),
                    (Manage::Aliases, _) => sender.input(AppMsg::EntryAliases),
                    (Manage::Merge, _) => sender.input(AppMsg::MergeEntry),
                    (Manage::Delete, _) => sender.input(AppMsg::ConfirmDeleteEntry),
                    (Manage::EditSeries, Some(Shelf::Series(series))) => {
                        sender.input(AppMsg::EditSeries(series, None));
                    }
                    (Manage::EditSeries, Some(_)) => {}
                }
            }
            AppMsg::EditSeries(series, copy) => {
                let Some((_, db)) = &self.open else {
                    return;
                };
                let loaded = db.series(series).and_then(|s| {
                    let publisher = match s.fields.publisher {
                        Some(p) => db.lookup(p)?.name,
                        None => String::new(),
                    };
                    Ok((s, publisher))
                });
                match loaded {
                    Ok((current, publisher)) => {
                        let s = sender.clone();
                        series_dialog::ask(
                            root,
                            &current,
                            &publisher,
                            db,
                            move |fields, publisher| {
                                s.input(AppMsg::SaveSeries(
                                    series,
                                    Box::new(fields),
                                    publisher,
                                    copy,
                                ));
                            },
                        );
                    }
                    Err(why) => root.toast(&why.to_string()),
                }
            }
            AppMsg::SaveSeries(series, fields, publisher, copy) => {
                match self.with_db(|db| db.edit_series(series, &fields, &publisher)) {
                    Ok(()) => {
                        match copy {
                            Some(copy) => self.refresh_copy(copy, root, &sender),
                            None => self.after_manage(Some(Shelf::Series(series)), root),
                        }
                        root.toast(&gettext("Saved"));
                    }
                    Err(why) => {
                        root.toast(&gettext_f("Could not save: {reason}", &[("reason", &why)]))
                    }
                }
            }
            AppMsg::RenameEntry => {
                if let Some(entry) = &self.managed {
                    let initial = rename_text(entry);
                    let s = sender.clone();
                    prompt::ask_text(
                        root,
                        prompt::Ask {
                            heading: &gettext("Rename"),
                            body: &gettext("The new name shows everywhere this one is used."),
                            field: &gettext("Name"),
                            initial: &initial,
                            action: &gettext("_Rename"),
                            suggest: None,
                        },
                        move |name| s.input(AppMsg::DoRename(name)),
                    );
                }
            }
            AppMsg::DoRename(name) => {
                let Some(shelf) = self.managed.as_ref().map(|e| e.shelf) else {
                    return;
                };
                match self.with_db(|db| db.rename_shelf(shelf, &name)) {
                    Ok(()) => self.after_manage(Some(shelf), root),
                    Err(why) => root.toast(&gettext_f(
                        "Could not rename: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::EntryAliases => {
                let (Some(entry), Some((_, db))) = (&self.managed, &self.open) else {
                    return;
                };
                let shelf = entry.shelf;
                match db.shelf_aliases(shelf) {
                    Ok(current) => {
                        let (add_db, remove_db) = (Rc::downgrade(db), Rc::downgrade(db));
                        let hooks = prompt::AliasHooks {
                            add: Box::new(move |alias| match add_db.upgrade() {
                                Some(db) => db
                                    .add_shelf_alias(shelf, alias)
                                    .map_err(|e| capitalized(&e.to_string())),
                                None => Err(gettext("The collection is closed")),
                            }),
                            remove: Box::new(move |alias| match remove_db.upgrade() {
                                Some(db) => db
                                    .remove_shelf_alias(shelf, alias)
                                    .map_err(|e| capitalized(&e.to_string())),
                                None => Err(gettext("The collection is closed")),
                            }),
                        };
                        prompt::alternate_names(root, &entry.name, current, hooks);
                    }
                    Err(why) => root.toast(&why.to_string()),
                }
            }
            AppMsg::MergeEntry => {
                let (Some(entry), Some((_, db))) = (&self.managed, &self.open) else {
                    return;
                };
                let shelf = entry.shelf;
                let weak = Rc::downgrade(db);
                let suggest: prompt::Suggest = Rc::new(move |text| {
                    weak.upgrade()
                        .and_then(|db| db.shelf_suggestions(shelf, text).ok())
                        .unwrap_or_default()
                });
                let s = sender.clone();
                let body = gettext_f(
                    "Everything listed under \u{201c}{name}\u{201d} moves to the entry you choose. Its name is kept as an alternate name where the list has them.",
                    &[("name", &entry.name)],
                );
                prompt::ask_text(
                    root,
                    prompt::Ask {
                        heading: &gettext("Merge Into"),
                        body: &body,
                        field: &gettext("Merge Into"),
                        initial: "",
                        action: &gettext("_Merge"),
                        suggest: Some(suggest),
                    },
                    move |into| s.input(AppMsg::DoMerge(into)),
                );
            }
            AppMsg::DoMerge(into) => {
                let Some(shelf) = self.managed.as_ref().map(|e| e.shelf) else {
                    return;
                };
                match self.with_db(|db| db.merge_shelf(shelf, &into)) {
                    Ok(kept) => {
                        self.after_manage(Some(kept), root);
                        root.toast(&gettext("Merged"));
                    }
                    Err(why) => {
                        root.toast(&gettext_f("Could not merge: {reason}", &[("reason", &why)]))
                    }
                }
            }
            AppMsg::ConfirmDeleteEntry => {
                if let Some(entry) = &self.managed {
                    let body = match entry.shelf {
                        Shelf::Series(_) => gettext_f(
                            "\u{201c}{name}\u{201d} and every book in it move to Trash.",
                            &[("name", &entry.name)],
                        ),
                        _ => gettext_f(
                            "\u{201c}{name}\u{201d} will be deleted. A name still in use cannot be deleted; merge it into another instead.",
                            &[("name", &entry.name)],
                        ),
                    };
                    let action = match entry.shelf {
                        Shelf::Series(_) => gettext("_Move to Trash"),
                        _ => gettext("_Delete"),
                    };
                    confirm(root, &gettext("Delete?"), &body, &action, &sender, || {
                        AppMsg::DoDeleteEntry
                    });
                }
            }
            AppMsg::DoDeleteEntry => {
                let Some(shelf) = self.managed.as_ref().map(|e| e.shelf) else {
                    return;
                };
                match self.with_db(|db| db.delete_shelf(shelf)) {
                    Ok(trash) => {
                        self.after_manage(None, root);
                        match trash {
                            Some(trash) => {
                                let undo = sender.clone();
                                root.toast_undo(&gettext("Moved to Trash"), move || {
                                    undo.input(AppMsg::Restore(trash));
                                });
                            }
                            None => root.toast(&gettext("Deleted")),
                        }
                    }
                    Err(why) => root.toast(&gettext_f(
                        "Could not delete: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::Selecting(on) => {
                self.selecting = on;
                for view in [
                    self.grid.view.upcast_ref::<gtk::Widget>(),
                    self.table.view.upcast_ref(),
                ] {
                    view.set_property("single-click-activate", !on);
                    view.set_property("enable-rubberband", on);
                }
                if !on {
                    self.grid.selection_model.unselect_all();
                    self.table.selection_model.unselect_all();
                }
                self.update_selection(root);
            }
            AppMsg::SelectionChanged => self.update_selection(root),
            AppMsg::BulkEdit => {
                let copies = self.selected_copies();
                if let (Some((_, db)), false) = (&self.open, copies.is_empty()) {
                    let count = copies.len();
                    let s = sender.clone();
                    bulk_dialog::ask(root, count, db, move |change| {
                        s.input(AppMsg::ApplyBulk(copies.clone(), change));
                    });
                }
            }
            AppMsg::ApplyBulk(copies, change) => self.guarded(
                gettext("before changing several books at once"),
                AppMsg::DoBulk(copies, change),
                root,
                &sender,
            ),
            AppMsg::DoBulk(copies, change) => {
                match self.with_db(|db| db.bulk_edit(&copies, &change)) {
                    Ok(()) => {
                        root.select_toggle().set_active(false);
                        self.show(root);
                        root.toast(&ngettext_f(
                            "Changed {count} copy",
                            "Changed {count} copies",
                            copies.len(),
                            &[("count", &copies.len().to_string())],
                        ));
                    }
                    Err(why) => root.toast(&gettext_f(
                        "Nothing was changed: {reason}",
                        &[("reason", &why)],
                    )),
                }
            }
            AppMsg::TrashSelected => {
                let copies = self.selected_copies();
                if !copies.is_empty() {
                    let trashed = self.with_db(|db| db.trash_copies(&copies));
                    root.select_toggle().set_active(false);
                    self.after_trash(trashed, None, root, &sender);
                }
            }
            AppMsg::TrashCopy(copy) => {
                let issue = self.with_db(|db| Ok(db.edition(db.copy(copy)?.edition)?.issue));
                let trashed = self.with_db(|db| db.trash_copy(copy));
                self.after_trash(trashed, issue.ok(), root, &sender);
            }
            AppMsg::TrashEdition(edition) => {
                let issue = self.with_db(|db| Ok(db.edition(edition)?.issue));
                let trashed = self.with_db(|db| db.trash_edition(edition));
                self.after_trash(trashed, issue.ok(), root, &sender);
            }
            AppMsg::TrashIssue(issue) => {
                let trashed = self.with_db(|db| db.trash_issue(issue));
                self.after_trash(trashed, None, root, &sender);
            }
            AppMsg::Restore(trash) => match self.with_db(|db| db.restore(trash)) {
                Ok(()) => {
                    root.pop_to_section();
                    self.show(root);
                    root.toast(&gettext("Restored"));
                }
                Err(why) => root.toast(&gettext_f(
                    "Could not restore: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ConfirmPurge(trash, label) => confirm(
                root,
                &gettext("Delete Forever?"),
                &gettext_f(
                    "\u{201c}{name}\u{201d} and everything in it will be deleted. \
                     This cannot be undone.",
                    &[("name", &label)],
                ),
                &gettext("_Delete Forever"),
                &sender,
                move || AppMsg::Purge(trash),
            ),
            AppMsg::Purge(trash) => self.guarded(
                gettext("before deleting from Trash"),
                AppMsg::DoPurge(trash),
                root,
                &sender,
            ),
            AppMsg::DoPurge(trash) => match self.with_db(|db| db.purge(trash)) {
                Ok(()) => self.show(root),
                Err(why) => root.toast(&gettext_f(
                    "Could not delete: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ConfirmEmpty => confirm(
                root,
                &gettext("Empty Trash?"),
                &gettext("Everything in Trash will be deleted. This cannot be undone."),
                &gettext("_Empty Trash"),
                &sender,
                || AppMsg::EmptyTrash,
            ),
            AppMsg::EmptyTrash => self.guarded(
                gettext("before emptying Trash"),
                AppMsg::DoEmptyTrash,
                root,
                &sender,
            ),
            AppMsg::DoEmptyTrash => match self.with_db(|db| db.empty_trash()) {
                Ok(()) => self.show(root),
                Err(why) => root.toast(&gettext_f(
                    "Could not empty Trash: {reason}",
                    &[("reason", &why)],
                )),
            },
            AppMsg::ShelfBack => {
                // The form is left only through its Cancel, which asks
                // before discarding.
                if self.form.is_some() {
                    sender.input(AppMsg::FormCancel);
                    return;
                }
                if root.pop_page() {
                    return;
                }
                if self.shelf.take().is_some() {
                    if !matches!(self.section.content, Content::Names(..)) {
                        // A shelf opened from a report, such as Gaps.
                        self.show(root);
                        return;
                    }
                    // The list kept its place while hidden; reloading it
                    // would scroll back to the top.
                    self.show_header(root);
                    root.show_names();
                }
            }
            AppMsg::ViewChanged(as_list) => {
                self.as_list = as_list;
                if root.is_showing_books() {
                    root.show_books(as_list);
                }
            }
        }
    }

    fn update_cmd(&mut self, msg: CommandMsg, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            CommandMsg::PhotoReady { opened, .. } | CommandMsg::PhotoLoaded { opened, .. }
                if opened != self.opened =>
            {
                root.toast(&gettext(
                    "The photo was not added, as another collection is open now",
                ));
            }
            CommandMsg::PhotoLoaded {
                opened,
                copy,
                side,
                loaded: Ok(loaded),
            } => {
                root.end_working();
                let title = match side {
                    Side::Front => gettext("Edit Front Cover"),
                    Side::Back => gettext("Edit Back Cover"),
                    Side::Signature => gettext("Edit Signature Photo"),
                };
                let preview = loaded.preview.clone();
                // The full photo waits here for the edit; closing the
                // editor without saving drops it.
                let loaded = RefCell::new(Some(loaded));
                let sender = sender.clone();
                photo_editor::open(root, &title, preview, move |edit| {
                    if let Some(loaded) = loaded.borrow_mut().take() {
                        sender.input(AppMsg::PhotoEdited {
                            opened,
                            copy,
                            side,
                            loaded,
                            edit,
                        });
                    }
                });
            }
            CommandMsg::PhotoLoaded {
                loaded: Err(why), ..
            } => root.toast(&gettext_f(
                "Could not open the photo: {reason}",
                &[("reason", &why)],
            )),
            CommandMsg::PhotoReady {
                copy,
                side,
                photo: Ok(photo),
                ..
            } => self.store_photo(copy, side, &photo, PhotoDone::Added, root, &sender),
            CommandMsg::PhotoReady {
                photo: Err(why), ..
            } => root.toast(&gettext_f(
                "Could not add the photo: {reason}",
                &[("reason", &why)],
            )),
            CommandMsg::Imported(Ok(info)) => {
                self.open_collection(&info.name, root);
                root.toast(&gettext_f(
                    "Imported \u{201c}{name}\u{201d}",
                    &[("name", &info.name)],
                ));
            }
            CommandMsg::Imported(Err(why)) => {
                root.end_working();
                tell(root, &gettext("Could Not Import the Collection"), &why);
            }
            CommandMsg::BooksFileRead { opened, .. } if opened != self.opened => root.end_working(),
            CommandMsg::BooksFileRead {
                name,
                source: Ok(source),
                ..
            } => {
                root.end_working();
                if let Some((_, db)) = &self.open {
                    let s = sender.clone();
                    let keeper = self.import_keeper(root);
                    import_dialog::open(
                        root,
                        &name,
                        *source,
                        db,
                        self.settings.photos,
                        keeper,
                        move |n| s.input(AppMsg::BooksImported(n)),
                    );
                }
            }
            CommandMsg::BooksFileRead {
                source: Err(why), ..
            } => {
                root.end_working();
                tell(root, &gettext("Could Not Read the File"), &why);
            }
        }
    }
}

impl App {
    fn store(&self) -> Result<&CollectionStore, String> {
        self.store.as_ref().map_err(Clone::clone)
    }

    /// What work on the whole collection needs, if it can start now:
    /// nothing else under way, and a collection open.
    fn work_context(
        &self,
        root: &MainWindow,
    ) -> Option<(work::BusyGuard, CollectionStore, String, Rc<Database>)> {
        let Some(guard) = self.busy.begin() else {
            root.toast(&gettext("Wait for the work under way to finish"));
            return None;
        };
        match (self.store(), &self.open) {
            (Ok(store), Some((name, db))) => Some((guard, store.clone(), name.clone(), db.clone())),
            (Err(why), _) => {
                root.toast(&why);
                None
            }
            (_, None) => {
                root.toast(&gettext("Open a collection first"));
                None
            }
        }
    }

    /// Keeps a safety copy before a change that would be hard to undo,
    /// unless the preferences say not to, then makes the change with
    /// `then`.
    fn guarded(
        &self,
        reason: String,
        then: AppMsg,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
    ) {
        if !self.settings.safety_copies {
            sender.input(then);
            return;
        }
        let Some((guard, store, name, db)) = self.work_context(root) else {
            return;
        };
        let (root, sender) = (root.clone(), sender.clone());
        glib::spawn_future_local(async move {
            let progress = work::Progress::start(&root, &gettext("Keeping a Safety Copy"), false);
            let kept = work::safety_copy(&store, &name, &db, &reason, &progress).await;
            progress.finish();
            drop(guard);
            match kept {
                Ok(()) => sender.input(then),
                Err(why) => sender.input(AppMsg::Notify(gettext_f(
                    "Nothing was changed: {reason}",
                    &[("reason", &why)],
                ))),
            }
        });
    }

    /// What keeps a safety copy as an import starts, if the preferences
    /// ask for one.
    fn import_keeper(&self, root: &MainWindow) -> import_dialog::BeforeImport {
        let keeper = match (self.settings.safety_copies, self.store(), &self.open) {
            (true, Ok(store), Some((name, db))) => Some((
                store.clone(),
                name.clone(),
                Rc::downgrade(db),
                self.busy.clone(),
                root.clone(),
            )),
            _ => None,
        };
        let keeper = Rc::new(keeper);
        Box::new(move || {
            let keeper = keeper.clone();
            Box::pin(async move {
                let Some((store, name, db, busy, root)) = keeper.as_ref() else {
                    return Ok(());
                };
                let db = db
                    .upgrade()
                    .ok_or_else(|| gettext("the collection is no longer open"))?;
                let _guard = busy
                    .begin()
                    .ok_or_else(|| gettext("other work is under way"))?;
                let progress =
                    work::Progress::start(root, &gettext("Keeping a Safety Copy"), false);
                let reason = gettext("before importing books");
                let kept = work::safety_copy(store, name, &db, &reason, &progress).await;
                progress.finish();
                kept
            })
        })
    }

    /// Exports from a copy of the open collection, on a worker thread;
    /// `write` gets the copy, opened.
    fn export(
        &self,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
        photos: bool,
        write: impl FnOnce(&Database) -> mylongbox_core::Result<usize> + Send + 'static,
    ) {
        let Some((guard, store, _, db)) = self.work_context(root) else {
            return;
        };
        let (root, sender) = (root.clone(), sender.clone());
        glib::spawn_future_local(async move {
            let progress = work::Progress::start(&root, &gettext("Exporting"), false);
            let result = async {
                let staging = store.staging("export").map_err(|e| e.to_string())?;
                let copy = work::copy(&db, &staging, &progress).await?;
                let doing = gettext("Writing the export\u{2026}");
                work::in_background(&progress, &doing, move || {
                    let written = Database::open(&copy).and_then(|copy| write(&copy));
                    store.discard(&copy);
                    written.map_err(|e| e.to_string())
                })
                .await
            }
            .await;
            progress.finish();
            drop(guard);
            sender.input(AppMsg::ExportDone(result, photos));
        });
    }

    /// Saves a checked copy of the open collection to `path`, copying
    /// it in steps so the window stays responsive.
    fn export_copy(&self, path: PathBuf, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((guard, store, _, db)) = self.work_context(root) else {
            return;
        };
        let (root, sender) = (root.clone(), sender.clone());
        glib::spawn_future_local(async move {
            let progress = work::Progress::start(&root, &gettext("Exporting a Copy"), false);
            let result = async {
                let staging = store.staging("export").map_err(|e| e.to_string())?;
                let copy = work::copy(&db, &staging, &progress).await?;
                let doing = gettext("Checking the copy\u{2026}");
                work::in_background(&progress, &doing, move || {
                    store.finish_export(&copy, &path).map_err(|e| e.to_string())
                })
                .await
            }
            .await;
            progress.finish();
            drop(guard);
            sender.input(AppMsg::CopyExported(result));
        });
    }

    /// Checks a copy of the open collection for damage, on a worker
    /// thread.
    fn check_damage(&self, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((guard, store, _, db)) = self.work_context(root) else {
            return;
        };
        let (root, sender) = (root.clone(), sender.clone());
        glib::spawn_future_local(async move {
            let progress = work::Progress::start(&root, &gettext("Checking for Damage"), false);
            let result = async {
                let staging = store.staging("check").map_err(|e| e.to_string())?;
                let copy = work::copy(&db, &staging, &progress).await?;
                let doing = gettext("Reading every part of the collection\u{2026}");
                work::in_background(&progress, &doing, move || {
                    store.check_copy(&copy).map_err(|e| e.to_string())
                })
                .await
            }
            .await;
            progress.finish();
            drop(guard);
            sender.input(AppMsg::CheckDone(result));
        });
    }

    /// Compacts a copy of the open collection on a worker thread; the
    /// copy then takes the collection's place in [`Self::finish_compact`].
    /// The dialog shows from the start, since nothing may change the
    /// collection until then.
    fn compact(&self, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((guard, store, _, db)) = self.work_context(root) else {
            return;
        };
        let (root, sender) = (root.clone(), sender.clone());
        glib::spawn_future_local(async move {
            let progress = work::Progress::start(&root, &gettext("Compacting"), true);
            let result = async {
                let staging = store.staging("compact").map_err(|e| e.to_string())?;
                let copy = work::copy(&db, &staging, &progress).await?;
                // The collection is closed and reopened to swap the file,
                // so this reference must be gone by then.
                drop(db);
                let doing = gettext("Compacting the copy\u{2026}");
                work::in_background(&progress, &doing, move || {
                    store.compact_copy(&copy).map_err(|e| e.to_string())
                })
                .await
            }
            .await;
            progress.finish();
            drop(guard);
            sender.input(AppMsg::CompactReady(result));
        });
    }

    /// Puts a compacted copy in place of the open collection, closing and
    /// reopening it, if it is smaller.
    fn finish_compact(&mut self, result: Result<(PathBuf, u64, u64), String>, root: &MainWindow) {
        let (copy, before, after) = match result {
            Ok(done) => done,
            Err(why) => {
                root.toast(&gettext_f(
                    "Could not compact: {reason}",
                    &[("reason", &why)],
                ));
                return;
            }
        };
        let (Ok(store), Some((name, _))) = (self.store().cloned(), self.open.clone()) else {
            return;
        };
        if after >= before {
            store.discard(&copy);
            root.toast(&gettext_f(
                "Already compact at {size}",
                &[("size", &glib::format_size(before))],
            ));
            return;
        }
        // Closing releases the collection's lock so its file can be
        // swapped; reopening shows it afresh either way.
        self.open = None;
        covers::set_database(Weak::new());
        self.current_db.replace(Weak::new());
        let swapped = store.replace_with(&name, &copy);
        if swapped.is_err() {
            store.discard(&copy);
        }
        self.open_collection(&name, root);
        root.toast(&match swapped {
            Ok(()) => gettext_f(
                "Compacted from {before} to {after}",
                &[
                    ("before", &glib::format_size(before)),
                    ("after", &glib::format_size(after)),
                ],
            ),
            Err(e) => gettext_f("Could not compact: {reason}", &[("reason", &e.to_string())]),
        });
    }

    fn show_preferences(
        &mut self,
        show_rates: bool,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
    ) {
        let collections = self
            .store()
            .ok()
            .and_then(|s| s.list().ok())
            .unwrap_or_default();
        let msg = |f: fn(String) -> AppMsg| -> Box<dyn Fn(String)> {
            let s = sender.clone();
            Box::new(move |name| s.input(f(name)))
        };
        let hooks = prefs::Hooks {
            settings_changed: {
                let s = sender.clone();
                Box::new(move |settings| s.input(AppMsg::SettingsChanged(settings)))
            },
            rates_changed: {
                let s = sender.clone();
                Rc::new(move || s.input(AppMsg::RatesChanged))
            },
            open: msg(AppMsg::OpenCollectionNamed),
            rename: msg(AppMsg::RenameCollection),
            delete: msg(AppMsg::ConfirmDeleteCollection),
            action: {
                let root = root.clone();
                Box::new(move |name| {
                    ActionGroupExt::activate_action(&root, name, None);
                })
            },
            restore_safety: msg(AppMsg::RestoreSafetyCopy),
            check: {
                let s = sender.clone();
                Box::new(move || s.input(AppMsg::CheckDamage))
            },
            compact: {
                let s = sender.clone();
                Box::new(move || s.input(AppMsg::Compact))
            },
        };
        let state = prefs::State {
            settings: self.settings,
            collections: &collections,
            current: self.open.as_ref().map(|(n, _)| n.as_str()),
            db: self.open.as_ref().map(|(_, db)| db),
            show_rates,
            safety: self
                .open
                .as_ref()
                .and_then(|(name, _)| self.store().ok()?.safety_copy(name)),
        };
        prefs::dialog(&state, hooks).present(Some(root));
    }

    /// Applies the preferences and the open collection's exchange rates
    /// to how amounts and grades are shown, and reminds about rates that
    /// are missing or old.
    fn refresh_display(&self, root: &MainWindow) {
        let rates = self.with_db(|db| db.rate_table()).unwrap_or_default();
        let reminder = self.open.as_ref().and_then(|_| {
            let today = format::today()?;
            match (rates.latest(), self.settings.currency) {
                (None, Some(currency)) => Some(match currency {
                    Currency::Usd => {
                        gettext("Add an exchange rate to show every amount in US dollars")
                    }
                    Currency::Gold => gettext("Add an exchange rate to show every amount in gold"),
                    Currency::Btc => {
                        gettext("Add an exchange rate to show every amount in bitcoin")
                    }
                }),
                (Some(latest), _) if rates.needs_update(today) => {
                    let days = latest.date().days_until(today);
                    Some(ngettext_f(
                        "Your newest exchange rate is {days} day old",
                        "Your newest exchange rate is {days} days old",
                        usize::try_from(days).unwrap_or(usize::MAX),
                        &[("days", &days.to_string())],
                    ))
                }
                _ => None,
            }
        });
        root.set_rates_reminder(reminder.as_deref());
        format::set_display(format::Display {
            settings: self.settings,
            rates,
        });
    }

    /// Shows the shelf of `shelf` afresh, after its entry was renamed or
    /// another was merged into it, so its name and books are current.
    fn show_entry(&mut self, shelf: Shelf, root: &MainWindow) {
        let Content::Names(list, _, _) = self.section.content else {
            return;
        };
        let entry = self
            .with_db(|db| db.shelf_list(list))
            .ok()
            .and_then(|entries| entries.into_iter().find(|e| e.shelf == shelf));
        // The names list is reloaded too, so Back shows the new name.
        self.shelf = None;
        self.show(root);
        if let Some(entry) = entry {
            self.shelf = Some(entry);
            self.show(root);
        }
    }

    /// Shows the effect of renaming, merging, or deleting an entry, or
    /// of changing a series: the shelf shown follows the entry to
    /// `kept`, or leaves it if it is gone; a list changed from its own
    /// menu keeps its place.
    fn after_manage(&mut self, kept: Option<Shelf>, root: &MainWindow) {
        self.managed = None;
        if self.shelf.is_some() {
            match kept {
                Some(shelf) => self.show_entry(shelf, root),
                None => {
                    self.shelf = None;
                    self.show(root);
                }
            }
        } else {
            let scroll = root.names_scroll();
            self.show(root);
            root.restore_names_scroll(scroll);
        }
    }

    /// The copies selected in the view showing, in the order shown.
    fn selected_copies(&self) -> Vec<CopyId> {
        let model: &gtk::MultiSelection = if self.as_list {
            &self.table.selection_model
        } else {
            &self.grid.selection_model
        };
        (0..model.n_items())
            .filter(|&i| model.is_selected(i))
            .filter_map(|i| {
                let item = if self.as_list {
                    self.table.get_visible(i)
                } else {
                    self.grid.get_visible(i)
                }?;
                let copy = item.borrow().0.copy;
                Some(copy)
            })
            .collect()
    }

    /// Shows how many books are selected, and offers the actions only
    /// when there is something to act on.
    fn update_selection(&self, root: &MainWindow) {
        let count = u32::try_from(self.selected_copies().len()).unwrap_or(u32::MAX);
        root.show_selection(self.selecting, count);
        for name in ["bulk-edit", "trash-selected"] {
            if let Some(action) = root.lookup_action(name)
                && let Ok(action) = action.downcast::<gio::SimpleAction>()
            {
                action.set_enabled(self.selecting && count > 0 && self.form.is_none());
            }
        }
    }

    /// Runs `f` on the open collection.
    fn with_db<T>(
        &self,
        f: impl FnOnce(&Database) -> mylongbox_core::Result<T>,
    ) -> Result<T, String> {
        match &self.open {
            Some((_, db)) => f(db).map_err(|e| e.to_string()),
            None => Err(gettext("no collection is open")),
        }
    }

    /// Replaces the open page of a copy's issue with a fresh one, and the
    /// section beneath, after the copy changed.
    fn refresh_copy(&mut self, copy: CopyId, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Ok(issue) = self.with_db(|db| Ok(db.edition(db.copy(copy)?.edition)?.issue)) else {
            return;
        };
        root.pop_page();
        self.show(root);
        self.open_copy = Some(copy);
        self.open_issue(issue, root, sender);
    }

    /// Opens the full-size view of a copy's photos, decoded by the app's
    /// own limited decoder like every stored photo.
    fn view_photo(&self, copy: CopyId, side: Side, root: &MainWindow) {
        let full = |side| {
            self.with_db(|db| db.photo(copy, side))
                .ok()
                .flatten()
                .and_then(|p| covers::texture(&p.full_jpeg))
        };
        let title = self
            .with_db(|db| {
                let edition = db.edition(db.copy(copy)?.edition)?;
                Ok(db.issue_detail(edition.issue)?.label)
            })
            .unwrap_or_default();
        let photos = viewer::Photos {
            front: full(Side::Front),
            back: full(Side::Back),
            signature: full(Side::Signature),
        };
        viewer::open(root, &title, photos, side);
    }

    /// Stores a photo made or kept earlier, and shows the copy with it.
    fn store_photo(
        &mut self,
        copy: CopyId,
        side: Side,
        photo: &CoverPhoto,
        done: PhotoDone,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
    ) {
        match self.with_db(|db| db.set_photo(copy, side, photo)) {
            Ok(()) => {
                covers::forget(copy);
                self.refresh_copy(copy, root, sender);
                root.toast(&photo_message(side, done));
            }
            Err(why) => root.toast(&gettext_f(
                "Could not save the photo: {reason}",
                &[("reason", &why)],
            )),
        }
    }

    /// Shows the collection as it is after something moved to Trash, back
    /// on the page of `issue` if it is still there, and offers Undo.
    fn after_trash(
        &mut self,
        trashed: Result<TrashId, String>,
        issue: Option<IssueId>,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
    ) {
        match trashed {
            Ok(trash) => {
                root.pop_to_section();
                self.show(root);
                if let Some(issue) = issue
                    && self.with_db(|db| db.issue_detail(issue)).is_ok()
                {
                    self.open_issue(issue, root, sender);
                }
                let undo = sender.clone();
                root.toast_undo(&gettext("Moved to Trash"), move || {
                    undo.input(AppMsg::Restore(trash));
                });
            }
            Err(why) => root.toast(&gettext_f(
                "Could not move to Trash: {reason}",
                &[("reason", &why)],
            )),
        }
    }

    fn open_issue(&mut self, issue: IssueId, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((_, db)) = &self.open else {
            return;
        };
        match db.issue_detail(issue) {
            Ok(detail) => {
                let current = issue_page::current_copy(&detail, self.open_copy.take());
                if current.is_some() {
                    self.recent_copy = current;
                }
                let open = sender.clone();
                let edit = sender.clone();
                let prefill = sender.clone();
                let links = Links {
                    open: Rc::new(move |issue| open.input(AppMsg::OpenIssue(issue))),
                    select: {
                        let s = sender.clone();
                        Rc::new(move |copy| s.input(AppMsg::SelectCopy(copy)))
                    },
                    edit: Rc::new(move |copy| edit.input(AppMsg::EditCopy(copy))),
                    prefill: Rc::new(move |copy, kind| prefill.input(AppMsg::AddFrom(copy, kind))),
                    trash_copy: {
                        let s = sender.clone();
                        Rc::new(move |copy| s.input(AppMsg::TrashCopy(copy)))
                    },
                    trash_edition: {
                        let s = sender.clone();
                        Rc::new(move |edition| s.input(AppMsg::TrashEdition(edition)))
                    },
                    trash_issue: {
                        let s = sender.clone();
                        Rc::new(move |issue| s.input(AppMsg::TrashIssue(issue)))
                    },
                    add_value: {
                        let s = sender.clone();
                        Rc::new(move |copy| s.input(AppMsg::AddValue(copy)))
                    },
                    delete_value: {
                        let s = sender.clone();
                        Rc::new(move |copy, entry| s.input(AppMsg::DeleteValue(copy, entry)))
                    },
                    add_photo: {
                        let s = sender.clone();
                        Rc::new(move |copy, side| s.input(AppMsg::AddPhoto(copy, side)))
                    },
                    remove_photo: {
                        let s = sender.clone();
                        Rc::new(move |copy, side| s.input(AppMsg::RemovePhoto(copy, side)))
                    },
                    swap_photos: {
                        let s = sender.clone();
                        Rc::new(move |copy| s.input(AppMsg::SwapPhotos(copy)))
                    },
                    view_photo: {
                        let s = sender.clone();
                        Rc::new(move |copy, side| s.input(AppMsg::ViewPhoto(copy, side)))
                    },
                    edit_series: {
                        let s = sender.clone();
                        Rc::new(move |series, copy| s.input(AppMsg::EditSeries(series, copy)))
                    },
                };
                root.push_page(&issue_page::page(&detail, &links, current));
            }
            Err(why) => root.toast(&why.to_string()),
        }
    }

    fn open_form(&mut self, kind: FormKind, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((_, db)) = &self.open else {
            return;
        };
        if self.form.is_some() {
            return;
        }
        let loaded = match kind {
            FormKind::New(_) => Ok(None),
            FormKind::Edit(copy) => db.book_entry(copy).map(Some),
            FormKind::From(copy, prefill) => db.prefilled_entry(copy, prefill).map(Some),
        };
        let entry = match loaded {
            Ok(entry) => entry,
            Err(why) => {
                root.toast(&why.to_string());
                return;
            }
        };
        let category = match (&kind, &entry) {
            (FormKind::New(category), _) => *category,
            (_, Some(e)) => e.category,
            (_, None) => return,
        };
        let target = match kind {
            FormKind::Edit(copy) => Some(copy),
            FormKind::New(_) | FormKind::From(..) => None,
        };
        let heading = match kind {
            FormKind::From(_, Prefill::NextIssue) if category == Category::CollectedEdition => {
                Some(gettext("Add Next Volume"))
            }
            FormKind::From(_, Prefill::NextIssue) => Some(gettext("Add Next Issue")),
            FormKind::From(_, Prefill::AnotherCopy) => Some(gettext("Add Another Copy")),
            FormKind::From(_, Prefill::Variant) => Some(gettext("Add Variant")),
            FormKind::New(_) | FormKind::Edit(_) => None,
        };
        let save = sender.clone();
        let cancel = sender.clone();
        let hooks = Hooks {
            save: Rc::new(move || save.input(AppMsg::FormSave)),
            cancel: Rc::new(move || cancel.input(AppMsg::FormCancel)),
        };
        let focus = match kind {
            FormKind::New(_) => Focus::Series,
            FormKind::Edit(_) => Focus::Nowhere,
            FormKind::From(_, Prefill::NextIssue) => Focus::Title,
            FormKind::From(_, Prefill::Variant) => Focus::Variant,
            FormKind::From(_, Prefill::AnotherCopy) => Focus::Copy,
        };
        let form = Form::new(
            db,
            category,
            target,
            entry.as_ref(),
            heading.as_deref(),
            focus,
            hooks,
        );
        root.push_page(&form.page);
        self.form = Some(form);
    }

    fn save_form(&mut self, root: &MainWindow, sender: &ComponentSender<Self>) {
        let (Some(form), Some((_, db))) = (&self.form, &self.open) else {
            return;
        };
        let entry = match form.entry() {
            Ok(entry) => entry,
            Err(invalid) => {
                form.show_invalid(&invalid);
                root.toast(&invalid.message);
                return;
            }
        };
        let saved = match form.target {
            None => db.add_book(&entry),
            Some(copy) => db.update_book(copy, &entry),
        };
        match saved {
            Ok(saved) => {
                self.form = None;
                self.recent_copy = Some(saved.copy);
                // The page shows the copy just saved, not whichever copy
                // of the issue comes first.
                self.open_copy = Some(saved.copy);
                // Pages under the form show the book as it was, so they go
                // too, and Back from the saved book leads to the list.
                root.pop_to_section();
                self.show(root);
                self.open_issue(saved.issue, root, sender);
                root.toast(&gettext("Saved"));
            }
            Err(why) => root.toast(&why.to_string()),
        }
    }

    fn open_collection(&mut self, name: &str, root: &MainWindow) {
        // Release the current collection's lock before taking another.
        self.form = None;
        self.recent_copy = None;
        self.open = None;
        // An entry and a shelf name rows of the old collection.
        self.shelf = None;
        self.saved = None;
        self.editing = None;
        root.pop_to_section();
        self.end_search(root);
        self.opened += 1;
        covers::set_database(std::rc::Weak::new());
        match self.store().and_then(|s| {
            s.open_keeping(name, self.settings.safety_copies)
                .map_err(|e| e.to_string())
        }) {
            Ok(db) => {
                let db = Rc::new(db);
                covers::set_database(Rc::downgrade(&db));
                self.current_db.replace(Rc::downgrade(&db));
                self.open = Some((name.to_owned(), db));
            }
            Err(why) => root.toast(&gettext_f(
                "Could not open \u{201c}{name}\u{201d}: {reason}",
                &[("name", name), ("reason", &why)],
            )),
        }
        // Each collection keeps its own shelves and hidden sections.
        self.refresh_sidebar(root);
        self.update_save_button();
        self.refresh_display(root);
        self.show(root);
    }

    /// Shows the welcome page, or the current section of the open
    /// collection, reading it afresh.
    fn show(&mut self, root: &MainWindow) {
        root.show_welcome(self.open.is_none());
        for name in [
            "export-collection",
            "export-csv",
            "export-shown-csv",
            "export-zip",
            "import-books",
            "customize-sidebar",
            "add-comic",
            "add-magazine",
            "add-collected",
            "add-next",
        ] {
            if let Some(action) = root.lookup_action(name)
                && let Ok(action) = action.downcast::<gio::SimpleAction>()
            {
                action.set_enabled(self.open.is_some());
            }
        }
        if self.open.is_none() {
            return;
        }
        self.show_header(root);
        if let Err(why) = self.load(root) {
            root.toast(&why.to_string());
            root.show_empty(&gettext("Could Not Read the Collection"), &why.to_string());
        }
    }

    /// What the books shown are, for naming an export of them.
    fn shown_title(&self) -> String {
        if self.searching() {
            gettext("Search Results")
        } else {
            match (&self.shelf, self.saved_name()) {
                (Some(entry), _) => entry.name.clone(),
                (None, Some(name)) => name,
                (None, None) => gettext(self.section.name),
            }
        }
    }

    /// Whether search words or filters decide what is shown, rather than
    /// the section.
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
            || (self.filters_shown && self.filters.filter_set() != FilterSet::default())
    }

    /// Shows the books matching the search words and the open filter
    /// panel, across the whole collection.
    fn load_search(&mut self, root: &MainWindow) -> mylongbox_core::Result<()> {
        let Some((_, db)) = &self.open else {
            return Ok(());
        };
        let filters = if self.filters_shown {
            self.filters.filter_set()
        } else {
            FilterSet::default()
        };
        let query = self.query.trim();
        let (rows, correction) = if query.is_empty() {
            (db.filter_books(&filters)?, None)
        } else {
            let found = db.quick_search(query)?;
            let rows = match db.matching_copies(&filters)? {
                None => found.rows,
                Some(allowed) => found
                    .rows
                    .into_iter()
                    .filter(|r| allowed.contains(&r.copy))
                    .collect(),
            };
            (rows, found.did_you_mean)
        };
        root.show_correction(query, correction.as_deref());
        self.correction = correction;
        self.found = rows.len();
        // The header counts what was found, which is known only now.
        self.show_header(root);
        if rows.is_empty() {
            self.fill_books(Vec::new(), false, root);
            root.show_empty(
                &gettext("No Books Found"),
                &gettext(
                    "Try other words, or fewer filters. Search looks at series, issue numbers, \
                     titles, publishers, people, characters, teams, story arcs, tags, boxes, \
                     and notes.",
                ),
            );
        } else {
            self.fill_books(rows, false, root);
        }
        Ok(())
    }

    /// Leaves search, as when another section is chosen.
    fn end_search(&mut self, root: &MainWindow) {
        self.query.clear();
        self.filters_shown = false;
        self.correction = None;
        root.end_search();
    }

    /// The name of the saved shelf shown, if one is.
    fn saved_name(&self) -> Option<String> {
        let id = self.saved?;
        self.with_db(|db| db.saved_shelf(id)).ok().map(|s| s.name)
    }

    /// Words the filter panel's save button for adding a shelf or
    /// updating the one being edited, usable once the panel filters by
    /// something.
    fn update_save_button(&self) {
        let filtering = self.filters.filter_set() != FilterSet::default();
        if self.editing.is_some() {
            self.filters.set_save(
                &gettext("_Update Shelf"),
                &gettext("Keep These Filters for the Shelf Being Changed"),
                filtering,
            );
        } else {
            self.filters.set_save(
                &gettext("Add to _Sidebar\u{2026}"),
                &gettext("Keep These Filters as a Shelf in the Sidebar"),
                filtering && self.open.is_some(),
            );
        }
    }

    /// Updates the shelf being edited with the panel's filters, or asks
    /// for a name to keep them as a new shelf under.
    fn save_shelf(&mut self, root: &MainWindow) {
        let filters = self.filters.filter_set();
        if let Some(id) = self.editing {
            match self.with_db(|db| db.set_saved_shelf_filters(id, &filters)) {
                Ok(()) => {
                    self.editing = None;
                    self.filters.clear();
                    self.go_to_shelf(id, root);
                    root.toast(&gettext("Shelf updated"));
                }
                Err(why) => root.toast(&why),
            }
            return;
        }
        let sender = self.sender.clone();
        prompt::ask_text(
            root,
            prompt::Ask {
                heading: &gettext("Add to Sidebar"),
                body: &gettext(
                    "The shelf shows every book these filters find, and keeps up as the \
                     collection changes.",
                ),
                field: &gettext("Shelf Name"),
                initial: "",
                action: &gettext("_Add"),
                suggest: None,
            },
            move |name| sender.input(AppMsg::AddShelf(name)),
        );
    }

    fn ask_shelf_name(&self, id: SavedShelfId, root: &MainWindow) {
        let Ok(shelf) = self.with_db(|db| db.saved_shelf(id)) else {
            return;
        };
        let sender = self.sender.clone();
        prompt::ask_text(
            root,
            prompt::Ask {
                heading: &gettext("Rename Shelf"),
                body: "",
                field: &gettext("Shelf Name"),
                initial: &shelf.name,
                action: &gettext("_Rename"),
                suggest: None,
            },
            move |name| sender.input(AppMsg::ShelfRenamed(id, name)),
        );
    }

    /// Shows a shelf, leaving any search, and marks it in the sidebar.
    fn go_to_shelf(&mut self, id: SavedShelfId, root: &MainWindow) {
        self.saved = Some(id);
        self.shelf = None;
        self.end_search(root);
        self.refresh_sidebar(root);
        root.pop_to_section();
        self.show(root);
        root.show_content();
    }

    /// Opens the filter panel on a shelf's filters, so they can be
    /// changed and kept with Update Shelf.
    fn edit_shelf(&mut self, id: SavedShelfId, root: &MainWindow) {
        let Ok(shelf) = self.with_db(|db| db.saved_shelf(id)) else {
            return;
        };
        let Some(filters) = shelf.filters else {
            return;
        };
        let series: Vec<(SeriesId, String)> = self
            .with_db(|db| db.list_series())
            .unwrap_or_default()
            .into_iter()
            .map(|s| {
                (
                    s.id,
                    mylongbox_core::series_label(
                        &s.fields.name,
                        s.fields.start_year,
                        s.fields.volume,
                    ),
                )
            })
            .collect();
        let label = |id: SeriesId| {
            series
                .iter()
                .find(|(s, _)| *s == id)
                .map(|(_, l)| l.clone())
        };
        self.saved = Some(id);
        self.shelf = None;
        self.query.clear();
        root.search_entry().set_text("");
        self.filters.set_filter_set(&filters, &label);
        self.editing = Some(id);
        self.update_save_button();
        self.refresh_sidebar(root);
        // Opening the panel shows what its filters find, as it does for any
        // search; the header says which shelf is being changed.
        root.filter_toggle().set_active(true);
        root.pop_to_section();
        self.show(root);
    }

    /// Lists the visible sections and the open collection's shelves in
    /// the sidebar, and marks the one shown. If that one is gone, as when
    /// it was hidden or removed, Everything is shown instead, and `true`
    /// says the books shown must be read again.
    fn refresh_sidebar(&mut self, root: &MainWindow) -> bool {
        let (hidden, shelves) = match &self.open {
            Some((_, db)) => (
                db.hidden_sections().unwrap_or_default(),
                db.saved_shelves().unwrap_or_default(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let sidebar = root.sidebar();
        sidebar.remove_all();
        self.places.clear();
        let label = |text: &str| {
            gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .margin_start(6)
                .margin_end(6)
                .build()
        };
        let mut group_starts = Vec::new();
        let mut shelves_start = None;
        for (n, group) in SECTIONS.iter().enumerate() {
            // The owner's shelves go just above Trash.
            if n + 1 == SECTIONS.len() && !shelves.is_empty() {
                shelves_start = Some(self.places.len());
                for shelf in &shelves {
                    sidebar.append(&shelf_row(shelf, &label(&shelf.name), &self.sender));
                    self.places.push(Place::Saved(shelf.id));
                }
            }
            let visible: Vec<&'static Section> = group
                .iter()
                .filter(|s| s.locked() || !hidden.iter().any(|h| h == s.code))
                .collect();
            if visible.is_empty() {
                continue;
            }
            group_starts.push(self.places.len());
            for section in visible {
                sidebar.append(&label(&gettext(section.name)));
                self.places.push(Place::Section(section));
            }
        }
        sidebar.set_header_func(move |row, _| {
            let at = usize::try_from(row.index()).unwrap_or(0);
            if Some(at) == shelves_start {
                let header = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(6)
                    .build();
                header.append(&gtk::Separator::default());
                header.append(
                    &gtk::Label::builder()
                        .label(gettext("My Shelves"))
                        .xalign(0.0)
                        .margin_start(12)
                        .css_classes(["caption-heading", "dim-label"])
                        .build(),
                );
                row.set_header(Some(&header));
            } else {
                let starts_group = at > 0 && group_starts.contains(&at);
                row.set_header(starts_group.then(gtk::Separator::default).as_ref());
            }
        });
        let current = self.places.iter().position(|p| match (p, self.saved) {
            (Place::Saved(id), Some(saved)) => *id == saved,
            (Place::Section(s), None) => std::ptr::eq(*s, self.section),
            _ => false,
        });
        let moved = current.is_none();
        if moved {
            self.saved = None;
            self.editing = None;
            self.section = &SECTIONS[0][0];
            self.shelf = None;
        }
        if let Some(row) = sidebar.row_at_index(i32::try_from(current.unwrap_or(0)).unwrap_or(0)) {
            sidebar.select_row(Some(&row));
        }
        moved
    }

    fn customize_sidebar(&self, root: &MainWindow) {
        let Some((_, db)) = &self.open else {
            root.toast(&gettext("Open a collection first"));
            return;
        };
        let hidden = db.hidden_sections().unwrap_or_default();
        let entries = sections()
            .map(|s| customize::Entry {
                code: s.code,
                name: gettext(s.name),
                locked: s.locked(),
                hidden: hidden.iter().any(|h| h == s.code),
            })
            .collect();
        let shelves = db
            .saved_shelves()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect();
        let (hide, shift) = (self.sender.clone(), self.sender.clone());
        let hooks = customize::Hooks {
            set_hidden: Box::new(move |code, hidden| {
                hide.input(AppMsg::SectionHidden(code, hidden))
            }),
            move_shelf: Box::new(move |id, earlier| shift.input(AppMsg::MoveShelf(id, earlier))),
        };
        customize::dialog(entries, shelves, hooks).present(Some(root));
    }

    fn show_header(&self, root: &MainWindow) {
        let Some((collection, _)) = &self.open else {
            return;
        };
        if self.searching() {
            let found = ngettext_f(
                "{count} book",
                "{count} books",
                self.found,
                &[("count", &self.found.to_string())],
            );
            let title = match (self.editing, self.saved_name()) {
                (Some(_), Some(name)) => {
                    gettext_f("Changing \u{201c}{name}\u{201d}", &[("name", &name)])
                }
                _ => gettext("Search Results"),
            };
            root.set_location(collection, &title, &found);
            root.set_back_visible(false);
            return;
        }
        match (&self.shelf, self.saved_name()) {
            (Some(entry), _) => {
                root.set_location(collection, &entry.name, &gettext(self.section.name));
            }
            (None, Some(name)) => root.set_location(collection, &name, collection),
            (None, None) => root.set_location(collection, &gettext(self.section.name), collection),
        }
        root.set_back_visible(self.shelf.is_some());
        // Only some lists keep alternate names.
        if let Some(action) = root.lookup_action("entry-aliases")
            && let Ok(action) = action.downcast::<gio::SimpleAction>()
        {
            action.set_enabled(
                self.shelf
                    .as_ref()
                    .is_some_and(|e| Database::shelf_has_aliases(e.shelf)),
            );
        }
        if let Some(action) = root.lookup_action("edit-series-entry")
            && let Ok(action) = action.downcast::<gio::SimpleAction>()
        {
            action.set_enabled(
                self.shelf
                    .as_ref()
                    .is_some_and(|e| matches!(e.shelf, Shelf::Series(_))),
            );
        }
    }

    fn load(&mut self, root: &MainWindow) -> mylongbox_core::Result<()> {
        if self.searching() {
            return self.load_search(root);
        }
        root.show_correction("", None);
        let Some((_, db)) = &self.open else {
            return Ok(());
        };
        if let (None, Some(id)) = (&self.shelf, self.saved) {
            let shelf = db.saved_shelf(id)?;
            let Some(filters) = shelf.filters else {
                self.fill_books(Vec::new(), false, root);
                root.show_empty(
                    &gettext("This Shelf Cannot Be Read"),
                    &gettext(
                        "A newer version of MyLongboxDB saved its filters. Update the app to \
                         use it, or remove it.",
                    ),
                );
                return Ok(());
            };
            let rows = db.filter_books(&filters)?;
            if rows.is_empty() {
                let missing = db.missing_from(&filters)?;
                let hint = if missing.is_empty() {
                    gettext("No books match this shelf\u{2019}s filters yet.")
                } else {
                    let names: Vec<String> = missing
                        .iter()
                        .map(|n| format!("\u{201c}{n}\u{201d}"))
                        .collect();
                    gettext_f(
                        "Its filters mention {names}, which the collection no longer lists. \
                         Edit the shelf\u{2019}s filters to change them.",
                        &[("names", &names.join(", "))],
                    )
                };
                self.fill_books(Vec::new(), false, root);
                root.show_empty(&gettext("No Books on This Shelf"), &hint);
            } else {
                self.fill_books(rows, false, root);
            }
            return Ok(());
        }
        let (shelf, empty, hint) = match (&self.shelf, self.section.content) {
            (Some(entry), _) => (
                entry.shelf,
                gettext_noop("No Books Here"),
                gettext_noop("You don\u{2019}t own any of these books."),
            ),
            (None, Content::Books(shelf, empty, hint)) => (shelf, empty, hint),
            (None, Content::Names(list, empty, hint)) => {
                let entries = db.shelf_list(list)?;
                self.names.clear();
                if entries.is_empty() {
                    root.show_empty(&gettext(empty), &gettext(hint));
                } else {
                    self.names
                        .extend_from_iter((0..).zip(entries).map(|(at, e)| Name(e, at)));
                    root.show_names();
                }
                return Ok(());
            }
            (None, Content::Trash) => {
                let groups = db.trash_groups()?;
                if groups.is_empty() {
                    root.show_empty(
                        &gettext("Trash Is Empty"),
                        &gettext("Anything moved to Trash waits here until you empty it."),
                    );
                } else {
                    let (restore, purge, empty) = (
                        self.sender.clone(),
                        self.sender.clone(),
                        self.sender.clone(),
                    );
                    let hooks = trash_page::Hooks {
                        restore: Rc::new(move |id| restore.input(AppMsg::Restore(id))),
                        purge: Rc::new(move |id, label| {
                            purge.input(AppMsg::ConfirmPurge(id, label));
                        }),
                        empty: Rc::new(move || empty.input(AppMsg::ConfirmEmpty)),
                    };
                    root.show_custom(&trash_page::page(&groups, &hooks));
                }
                return Ok(());
            }
            (None, Content::Gaps) => {
                let gaps = db.gaps(self.gaps_collected)?;
                let (count, open) = (self.sender.clone(), self.sender.clone());
                let hooks = gaps_page::Hooks {
                    count_collected: Rc::new(move |on| count.input(AppMsg::GapsCountCollected(on))),
                    open: Rc::new(move |series, label| {
                        open.input(AppMsg::OpenSeries(series, label))
                    }),
                };
                root.show_custom(&gaps_page::page(&gaps, self.gaps_collected, &hooks));
                return Ok(());
            }
            (None, Content::Stats) => {
                let rows = db.browse(Shelf::Everything)?;
                if rows.is_empty() {
                    root.show_empty(
                        &gettext("Nothing to Count Yet"),
                        &gettext("Figures about your collection appear here as you add books."),
                    );
                    return Ok(());
                }
                let owned = |list| -> mylongbox_core::Result<usize> {
                    Ok(db.shelf_list(list)?.iter().filter(|e| e.copies > 0).count())
                };
                let size = self.store().ok().and_then(|store| {
                    let name = &self.open.as_ref()?.0;
                    let total = store
                        .list()
                        .ok()?
                        .into_iter()
                        .find(|c| &c.name == name)?
                        .size_bytes;
                    Some((total, db.photo_bytes().ok()?))
                });
                let counts = stats_page::Counts {
                    series: owned(ShelfList::Series)?,
                    publishers: owned(ShelfList::Publishers)?,
                    creators: owned(ShelfList::Creators)?,
                    size,
                };
                let open = self.sender.clone();
                let hooks = stats_page::Hooks {
                    open: Rc::new(move |issue, copy| open.input(AppMsg::OpenCopy(issue, copy))),
                };
                root.show_custom(&stats_page::page(&rows, &counts, &hooks));
                return Ok(());
            }
        };
        let rows = db.browse(shelf)?;
        if rows.is_empty() {
            self.fill_books(Vec::new(), false, root);
            root.show_empty(&gettext(empty), &gettext(hint));
        } else {
            self.fill_books(rows, shelf == Shelf::NeedsAttention, root);
        }
        Ok(())
    }

    /// Shows `rows` in the grid and the list, sorted as chosen, with
    /// their totals underneath. `needs` says on each card what its book
    /// still needs.
    ///
    /// Showing the same books again, as after a change made on a book's
    /// page, keeps the views where they were scrolled to, so going back
    /// returns to the book that was opened; other books start at the top.
    fn fill_books(&mut self, mut rows: Vec<CopyRow>, needs: bool, root: &MainWindow) {
        sorting::sort(&mut rows, self.sort, self.sort_reversed, &format::in_cents);
        root.set_totals(&format::totals(&rows));
        let view = self.view_key();
        let scroll = (self.books_view.as_ref() == Some(&view)).then(|| root.books_scroll());
        if scroll.is_none() {
            root.forget_books_scroll();
        }
        self.books_view = Some(view);
        self.grid.clear();
        self.table.clear();
        if rows.is_empty() {
            return;
        }
        self.grid
            .extend_from_iter(rows.iter().cloned().map(|r| Book(r, needs)));
        self.table
            .extend_from_iter(rows.into_iter().map(|r| Book(r, needs)));
        root.show_books(self.as_list);
        if let Some(scroll) = scroll {
            root.restore_books_scroll(scroll);
        }
    }

    /// What decides which books are shown and in what order, to tell a
    /// refresh of the same books from a move to others.
    fn view_key(&self) -> String {
        let filters = if self.filters_shown {
            format!("{:?}", self.filters.filter_set())
        } else {
            String::new()
        };
        format!(
            "{}|{}|{:?}|{:?}|{:?}|{}|{}|{filters}",
            self.opened,
            self.section.code,
            self.saved,
            self.shelf.as_ref().map(|e| e.shelf),
            (self.sort, self.sort_reversed),
            self.query.trim(),
            self.filters_shown,
        )
    }
}

fn add_action(root: &MainWindow, name: &str, sender: &ComponentSender<App>, msg: fn() -> AppMsg) {
    let action = gio::SimpleAction::new(name, None);
    let sender = sender.clone();
    action.connect_activate(move |_, _| sender.input(msg()));
    root.add_action(&action);
}

/// A shelf's row in the sidebar: its name and a menu of what can be
/// done with it.
fn shelf_row(shelf: &SavedShelf, label: &gtk::Label, sender: &ComponentSender<App>) -> gtk::Box {
    let id = shelf.id;
    let actions = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    for (name, text, msg) in [
        (
            "rename",
            gettext("_Rename\u{2026}"),
            AppMsg::RenameShelf as fn(SavedShelfId) -> AppMsg,
        ),
        ("edit", gettext("_Edit Filters"), AppMsg::EditShelf),
        (
            "remove",
            gettext("Re_move\u{2026}"),
            AppMsg::ConfirmRemoveShelf,
        ),
    ] {
        let action = gio::SimpleAction::new(name, None);
        // Rules a newer version wrote cannot be shown for editing.
        action.set_enabled(name != "edit" || shelf.filters.is_some());
        let sender = sender.clone();
        action.connect_activate(move |_, _| sender.input(msg(id)));
        actions.add_action(&action);
        menu.append(Some(&text), Some(&format!("shelf.{name}")));
    }
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("Shelf Menu"))
        .menu_model(&menu)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&gettext_f(
        "Menu of \u{201c}{name}\u{201d}",
        &[("name", &shelf.name)],
    ))]);
    let row = gtk::Box::builder().spacing(6).build();
    row.append(label);
    row.append(&button);
    row.insert_action_group("shelf", Some(&actions));
    row
}

fn ask_for_name(root: &MainWindow, sender: &ComponentSender<App>) {
    let entry = gtk::Entry::builder()
        .placeholder_text(gettext("My Comics"))
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("New Collection"))
        .body(gettext(
            "Choose a name. You can keep several collections and switch between them.",
        ))
        .extra_child(&entry)
        .focus_widget(&entry)
        .default_response("create")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[
        ("cancel", &gettext("_Cancel")),
        ("create", &gettext("C_reate")),
    ]);
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("create", false);
    let weak = dialog.downgrade();
    entry.connect_changed(move |entry| {
        if let Some(dialog) = weak.upgrade() {
            dialog.set_response_enabled("create", !entry.text().trim().is_empty());
        }
    });
    let sender = sender.clone();
    dialog.choose(Some(root), None::<&gio::Cancellable>, move |response| {
        if response == "create" {
            sender.input(AppMsg::CreateCollection(entry.text().into()));
        }
    });
}

/// What happened to a cover photo, for the message saying so.
#[derive(Clone, Copy)]
enum PhotoDone {
    Added,
    Removed,
    PutBack,
}

/// e.g. "Front cover removed".
fn photo_message(side: Side, done: PhotoDone) -> String {
    match (side, done) {
        (Side::Front, PhotoDone::Added) => gettext("Front cover added"),
        (Side::Front, PhotoDone::Removed) => gettext("Front cover removed"),
        (Side::Front, PhotoDone::PutBack) => gettext("Front cover put back"),
        (Side::Back, PhotoDone::Added) => gettext("Back cover added"),
        (Side::Back, PhotoDone::Removed) => gettext("Back cover removed"),
        (Side::Back, PhotoDone::PutBack) => gettext("Back cover put back"),
        (Side::Signature, PhotoDone::Added) => gettext("Signature photo added"),
        (Side::Signature, PhotoDone::Removed) => gettext("Signature photo removed"),
        (Side::Signature, PhotoDone::PutBack) => gettext("Signature photo put back"),
    }
}

/// Asks for a photo file through the system file chooser, which also
/// grants the app access to just that file.
fn pick_photo(root: &MainWindow, sender: &ComponentSender<App>, copy: CopyId, side: Side) {
    let photos = gtk::FileFilter::new();
    photos.set_name(Some(&gettext("Photos")));
    for mime in [
        "image/jpeg",
        "image/png",
        "image/webp",
        "image/heic",
        "image/heif",
    ] {
        photos.add_mime_type(mime);
    }
    for suffix in ["jpg", "jpeg", "png", "webp", "heic", "heif"] {
        photos.add_suffix(suffix);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&photos);
    let dialog = gtk::FileDialog::builder()
        .title(match side {
            Side::Front => gettext("Choose a Front Cover Photo"),
            Side::Back => gettext("Choose a Back Cover Photo"),
            Side::Signature => gettext("Choose a Photo of the Signature"),
        })
        .modal(true)
        .filters(&filters)
        .default_filter(&photos)
        .build();
    let sender = sender.clone();
    dialog.open(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::PhotoPicked(copy, side, path));
        }
    });
}

fn collection_filters() -> gio::ListStore {
    let collections = gtk::FileFilter::new();
    collections.set_name(Some(&gettext("MyLongboxDB collections")));
    collections.add_suffix("sqlite");
    let all = gtk::FileFilter::new();
    all.set_name(Some(&gettext("All files")));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&collections);
    filters.append(&all);
    filters
}

/// Asks for a CSV file or a full export to add books from.
fn pick_books_file(root: &MainWindow, sender: &ComponentSender<App>) {
    let books = gtk::FileFilter::new();
    books.set_name(Some(&gettext("CSV files and full exports")));
    for suffix in ["csv", "tsv", "txt", "zip"] {
        books.add_suffix(suffix);
    }
    for mime in ["text/csv", "text/tab-separated-values", "application/zip"] {
        books.add_mime_type(mime);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&books);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Import Books"))
        .modal(true)
        .filters(&filters)
        .default_filter(&books)
        .build();
    let sender = sender.clone();
    dialog.open(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::BooksFilePicked(path));
        }
    });
}

fn pick_file_to_import(root: &MainWindow, sender: &ComponentSender<App>) {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Import Collection File"))
        .modal(true)
        .filters(&collection_filters())
        .build();
    let sender = sender.clone();
    dialog.open(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::ImportPicked(path));
        }
    });
}

/// Asks where to save a full export, through the file chooser.
fn pick_zip_file(root: &MainWindow, sender: &ComponentSender<App>, name: &str) {
    let zip = gtk::FileFilter::new();
    zip.set_name(Some(&gettext("Zip archives")));
    zip.add_suffix("zip");
    zip.add_mime_type("application/zip");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&zip);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Full Export with Photos"))
        .modal(true)
        .initial_name(name)
        .filters(&filters)
        .build();
    let sender = sender.clone();
    dialog.save(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::ZipPicked(path));
        }
    });
}

/// Asks where to save a CSV export, through the file chooser.
fn pick_csv_file(
    root: &MainWindow,
    sender: &ComponentSender<App>,
    name: &str,
    copies: Option<Vec<CopyId>>,
) {
    let csv = gtk::FileFilter::new();
    csv.set_name(Some(&gettext("CSV files")));
    csv.add_suffix("csv");
    csv.add_mime_type("text/csv");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&csv);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Export as CSV"))
        .modal(true)
        .initial_name(name)
        .filters(&filters)
        .build();
    let sender = sender.clone();
    dialog.save(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::CsvPicked(path, copies.clone()));
        }
    });
}

fn pick_export_file(root: &MainWindow, sender: &ComponentSender<App>, file_name: &str) {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Export a Copy"))
        .modal(true)
        .initial_name(file_name)
        .filters(&collection_filters())
        .build();
    let sender = sender.clone();
    dialog.save(Some(root), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            sender.input(AppMsg::ExportPicked(path));
        }
    });
}

/// Asks for a copy's value on a date, with today filled in. Add is
/// enabled only once the amount and date can be read.
fn ask_value(root: &MainWindow, sender: &ComponentSender<App>, copy: CopyId) {
    // Translators: how much a copy is worth.
    let amount = adw::EntryRow::builder().title(gettext("Value")).build();
    let currencies = [gettext("USD"), gettext("Gold (oz)"), gettext("Bitcoin")];
    let currency = gtk::DropDown::from_strings(&currencies.each_ref().map(String::as_str));
    currency.set_valign(gtk::Align::Center);
    amount.add_suffix(&currency);
    let date = adw::EntryRow::builder()
        .title(gettext("Date (YYYY-MM-DD)"))
        .build();
    date.set_text(&format::today().map(|d| d.to_string()).unwrap_or_default());
    let source = adw::EntryRow::builder()
        .title(gettext("Source (e.g. GoCollect, eBay sold)"))
        .build();
    let fields = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    fields.append(&amount);
    fields.append(&date);
    fields.append(&source);

    let read = {
        let (amount, currency, date) = (amount.clone(), currency.clone(), date.clone());
        move || -> Option<(Date, Amount)> {
            let cleaned: String = amount
                .text()
                .trim()
                .trim_start_matches('$')
                .chars()
                .filter(|c| *c != ',')
                .collect();
            let currency = Currency::ALL[usize::try_from(currency.selected()).unwrap_or(0)];
            let value = Amount::parse(currency, &cleaned).ok()?;
            Some((date.text().trim().parse().ok()?, value))
        }
    };
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Add Value"))
        .body(gettext("What this copy was worth, and when."))
        .extra_child(&fields)
        .focus_widget(&amount)
        .default_response("add")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("add", &gettext("_Add"))]);
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("add", false);
    {
        let weak = dialog.downgrade();
        let read = read.clone();
        let check = move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("add", read().is_some());
            }
        };
        let c = check.clone();
        amount.connect_changed(move |_| c());
        let c = check.clone();
        date.connect_changed(move |_| c());
        currency.connect_selected_notify(move |_| check());
    }
    let sender = sender.clone();
    dialog.choose(Some(root), None::<&gio::Cancellable>, move |response| {
        if response == "add"
            && let Some((date, value)) = read()
        {
            sender.input(AppMsg::SaveValue(
                copy,
                date,
                value,
                source.text().trim().to_owned(),
            ));
        }
    });
}

/// Says why something failed in a dialog rather than a toast, which
/// shows only one line: the reason may be long, such as which command a
/// file picked by mistake belongs to.
fn tell(root: &MainWindow, heading: &str, reason: &str) {
    let mut body = capitalized(reason);
    if !body.ends_with(['.', '!', '?']) {
        body.push('.');
    }
    let dialog = adw::AlertDialog::new(Some(heading), Some(&body));
    dialog.add_response("close", &gettext("_Close"));
    dialog.present(Some(root));
}

/// Asks before something that cannot be undone, sending `yes()` if the
/// user goes ahead.
fn confirm(
    root: &MainWindow,
    heading: &str,
    body: &str,
    action: &str,
    sender: &ComponentSender<App>,
    yes: impl Fn() -> AppMsg + 'static,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .default_response("cancel")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("go", action)]);
    dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
    let sender = sender.clone();
    dialog.choose(Some(root), None::<&gio::Cancellable>, move |response| {
        if response == "go" {
            sender.input(yes());
        }
    });
}

/// Asks before throwing away a form's changes, then closes the form and
/// goes on with `then`, if given.
fn confirm_discard(root: &MainWindow, sender: &ComponentSender<App>, then: Option<AppMsg>) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Discard Changes?"))
        .body(gettext(
            "The changes made in this form have not been saved.",
        ))
        .default_response("keep")
        .close_response("keep")
        .build();
    dialog.add_responses(&[
        ("keep", &gettext("_Keep Editing")),
        ("discard", &gettext("_Discard")),
    ]);
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    let sender = sender.clone();
    let then = RefCell::new(then);
    dialog.choose(Some(root), None::<&gio::Cancellable>, move |response| {
        if response == "discard" {
            sender.input(AppMsg::FormClose);
            if let Some(then) = then.borrow_mut().take() {
                sender.input(then);
            }
        }
    });
}

/// The name to start renaming from: a series without the year its
/// listing adds.
fn rename_text(entry: &ShelfEntry) -> String {
    match entry.shelf {
        Shelf::Series(_) => entry
            .name
            .rsplit_once(" (")
            .filter(|(_, year)| year.len() == 5 && year.ends_with(')'))
            .map_or(entry.name.clone(), |(name, _)| name.to_owned()),
        _ => entry.name.clone(),
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
