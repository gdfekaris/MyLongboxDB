use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{
    Category, CollectionInfo, CollectionStore, CopyId, Database, IssueId, Shelf, ShelfEntry,
    ShelfList,
};
use relm4::typed_view::column::TypedColumnView;
use relm4::typed_view::grid::TypedGridView;
use relm4::typed_view::list::TypedListView;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::books::{self, Book, Name};
use crate::form::{Form, Hooks};
use crate::issue_page::{self, Links};
use crate::window::MainWindow;

/// What a sidebar section shows.
#[derive(Clone, Copy)]
enum Content {
    /// Books, or what to say when there are none.
    Books(Shelf, &'static str, &'static str),
    /// A list of names, each leading to its books.
    Names(ShelfList, &'static str, &'static str),
    NotYet,
}

struct Section {
    name: &'static str,
    content: Content,
}

const fn books(
    name: &'static str,
    shelf: Shelf,
    empty: &'static str,
    hint: &'static str,
) -> Section {
    Section {
        name,
        content: Content::Books(shelf, empty, hint),
    }
}

const fn names(
    name: &'static str,
    list: ShelfList,
    empty: &'static str,
    hint: &'static str,
) -> Section {
    Section {
        name,
        content: Content::Names(list, empty, hint),
    }
}

const fn not_yet(name: &'static str) -> Section {
    Section {
        name,
        content: Content::NotYet,
    }
}

/// The sidebar, in groups separated by a line.
const SECTIONS: &[&[Section]] = &[
    &[
        books(
            "Everything",
            Shelf::Everything,
            "Nothing Here Yet",
            "Comics, magazines, and collected editions you add will appear here.",
        ),
        books(
            "Comics",
            Shelf::Category(Category::Comic),
            "No Comics",
            "Comics you add will appear here.",
        ),
        books(
            "Magazines",
            Shelf::Category(Category::Magazine),
            "No Magazines",
            "Magazines you add will appear here.",
        ),
        books(
            "Collected Editions",
            Shelf::Category(Category::CollectedEdition),
            "No Collected Editions",
            "Trades, omnibuses, and other books you add will appear here.",
        ),
    ],
    &[
        names(
            "Series",
            ShelfList::Series,
            "No Series",
            "Series appear here as you add books.",
        ),
        names(
            "Publishers",
            ShelfList::Publishers,
            "No Publishers",
            "Publishers appear here as you add them to series.",
        ),
        names(
            "Creators",
            ShelfList::Creators,
            "No Creators",
            "Writers, artists, and editors appear here as you credit them.",
        ),
        names(
            "Characters",
            ShelfList::Characters,
            "No Characters",
            "Characters appear here as you add them to issues.",
        ),
        names(
            "Teams",
            ShelfList::Teams,
            "No Teams",
            "Teams appear here as you add them to issues.",
        ),
        names(
            "Story Arcs",
            ShelfList::StoryArcs,
            "No Story Arcs",
            "Story arcs appear here as you add them to issues.",
        ),
        names(
            "Storage Locations",
            ShelfList::StorageBoxes,
            "No Storage Locations",
            "Boxes appear here as you file books in them.",
        ),
    ],
    &[
        books(
            "Key Issues",
            Shelf::KeyIssues,
            "No Key Issues",
            "Issues you mark as key will appear here.",
        ),
        not_yet("Gaps"),
        not_yet("Duplicates"),
        not_yet("Needs Attention"),
    ],
    &[not_yet("Trash")],
];

fn sections() -> impl Iterator<Item = &'static Section> {
    SECTIONS.iter().flat_map(|group| group.iter())
}

pub struct App {
    /// The folder of collections, or why it could not be used.
    store: Result<CollectionStore, String>,
    /// The open collection and its name. Pages hold only weak references,
    /// so closing it here releases its lock.
    open: Option<(String, Rc<Database>)>,
    /// The add or edit form, while one is open.
    form: Option<Form>,
    section: &'static Section,
    /// The name-list entry whose books are shown, if any.
    shelf: Option<ShelfEntry>,
    as_list: bool,
    grid: TypedGridView<Book, gtk::SingleSelection>,
    table: TypedColumnView<Book, gtk::SingleSelection>,
    names: TypedListView<Name, gtk::SingleSelection>,
}

#[derive(Debug)]
pub enum AppMsg {
    NewCollection,
    CreateCollection(String),
    ImportCollection,
    ImportPicked(PathBuf),
    ExportCollection,
    ExportPicked(PathBuf),
    SectionSelected(usize),
    NameActivated(u32),
    /// A book in the grid or list, by position.
    BookActivated(u32),
    OpenIssue(IssueId),
    AddComic,
    EditCopy(CopyId),
    FormSave,
    /// Leave the form, asking first if it has unsaved changes.
    FormCancel,
    FormClose,
    ShelfBack,
    ViewChanged(bool),
}

#[derive(Debug)]
pub enum CommandMsg {
    Imported(Result<CollectionInfo, String>),
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
        add_action(&root, "export-collection", &sender, || {
            AppMsg::ExportCollection
        });
        add_action(&root, "shelf-back", &sender, || AppMsg::ShelfBack);
        add_action(&root, "add-comic", &sender, || AppMsg::AddComic);
        relm4::main_application().set_accels_for_action("win.add-comic", &["<Control>n"]);
        relm4::main_application().set_accels_for_action("win.shelf-back", &["<Alt>Left"]);
        fill_sidebar(&root, &sender);

        let css = gtk::CssProvider::new();
        css.load_from_string(books::CSS);
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(&root),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let grid = TypedGridView::<Book, gtk::SingleSelection>::new();
        grid.selection_model.set_autoselect(false);
        grid.view.set_max_columns(24);
        let table = books::book_table();
        table.selection_model.set_autoselect(false);
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
        let mut model = App {
            store,
            open: None,
            form: None,
            section: &SECTIONS[0][0],
            shelf: None,
            as_list: false,
            grid,
            table,
            names,
        };

        let startup = match &model.store {
            Ok(store) => store
                .last_opened()
                .or_else(|| store.list().ok()?.into_iter().next().map(|c| c.name)),
            Err(why) => {
                root.toast(&format!("Collections cannot be stored: {why}"));
                None
            }
        };
        match startup {
            Some(name) => model.open_collection(&name, &root),
            None => model.show(&root),
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
                    root.toast("Importing…");
                    sender.spawn_oneshot_command(move || {
                        CommandMsg::Imported(store.import(&path).map_err(|e| e.to_string()))
                    });
                }
                Err(why) => root.toast(&why),
            },
            AppMsg::ExportCollection => match &self.open {
                Some((name, _)) => pick_export_file(root, &sender, name),
                None => root.toast("Open a collection first"),
            },
            AppMsg::ExportPicked(path) => {
                let result = match (self.store(), &self.open) {
                    (Ok(store), Some((_, db))) => {
                        store.export(db, &path).map_err(|e| e.to_string())
                    }
                    (Err(why), _) => Err(why),
                    (_, None) => Err("Open a collection first".into()),
                };
                match result {
                    Ok(()) => root.toast("Copy saved"),
                    Err(why) => root.toast(&format!("Could not save a copy: {why}")),
                }
            }
            AppMsg::SectionSelected(index) => {
                if let Some(section) = sections().nth(index) {
                    self.section = section;
                    self.shelf = None;
                    root.pop_to_section();
                    self.show(root);
                    root.show_content();
                }
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
                    let issue = book.borrow().0.issue;
                    self.open_issue(issue, root, &sender);
                }
            }
            AppMsg::OpenIssue(issue) => self.open_issue(issue, root, &sender),
            AppMsg::AddComic => self.open_form(None, root, &sender),
            AppMsg::EditCopy(copy) => self.open_form(Some(copy), root, &sender),
            AppMsg::FormSave => self.save_form(root, &sender),
            AppMsg::FormCancel => match &self.form {
                Some(form) if form.is_dirty() => confirm_discard(root, &sender),
                Some(_) => sender.input(AppMsg::FormClose),
                None => {}
            },
            AppMsg::FormClose => {
                if self.form.take().is_some() {
                    root.pop_page();
                }
            }
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

    fn update_cmd(&mut self, msg: CommandMsg, _sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            CommandMsg::Imported(Ok(info)) => {
                self.open_collection(&info.name, root);
                root.toast(&format!("Imported \u{201c}{}\u{201d}", info.name));
            }
            CommandMsg::Imported(Err(why)) => root.toast(&format!("Could not import: {why}")),
        }
    }
}

impl App {
    fn store(&self) -> Result<&CollectionStore, String> {
        self.store.as_ref().map_err(Clone::clone)
    }

    fn open_issue(&self, issue: IssueId, root: &MainWindow, sender: &ComponentSender<Self>) {
        let Some((_, db)) = &self.open else {
            return;
        };
        match db.issue_detail(issue) {
            Ok(detail) => {
                let open = sender.clone();
                let edit = sender.clone();
                let links = Links {
                    open: Rc::new(move |issue| open.input(AppMsg::OpenIssue(issue))),
                    edit: Rc::new(move |copy| edit.input(AppMsg::EditCopy(copy))),
                };
                root.push_page(&issue_page::page(&detail, &links));
            }
            Err(why) => root.toast(&why.to_string()),
        }
    }

    fn open_form(
        &mut self,
        copy: Option<CopyId>,
        root: &MainWindow,
        sender: &ComponentSender<Self>,
    ) {
        let Some((_, db)) = &self.open else {
            return;
        };
        if self.form.is_some() {
            return;
        }
        let entry = match copy.map(|c| db.book_entry(c)).transpose() {
            Ok(entry) => entry,
            Err(why) => {
                root.toast(&why.to_string());
                return;
            }
        };
        let save = sender.clone();
        let cancel = sender.clone();
        let hooks = Hooks {
            save: Rc::new(move || save.input(AppMsg::FormSave)),
            cancel: Rc::new(move || cancel.input(AppMsg::FormCancel)),
        };
        let form = Form::new(db, Category::Comic, copy, entry.as_ref(), hooks);
        root.push_page(&form.page);
        form.focus_first();
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
                let edited = form.target.is_some();
                self.form = None;
                root.pop_page();
                // An edit replaces the stale detail page under the form;
                // an add shows the new book.
                if edited {
                    root.pop_page();
                }
                self.show(root);
                self.open_issue(saved.issue, root, sender);
                root.toast("Saved");
            }
            Err(why) => root.toast(&why.to_string()),
        }
    }

    fn open_collection(&mut self, name: &str, root: &MainWindow) {
        // Release the current collection's lock before taking another.
        self.form = None;
        self.open = None;
        // An entry names a row of the old collection.
        self.shelf = None;
        root.pop_to_section();
        match self
            .store()
            .and_then(|s| s.open(name).map_err(|e| e.to_string()))
        {
            Ok(db) => self.open = Some((name.to_owned(), Rc::new(db))),
            Err(why) => root.toast(&format!("Could not open \u{201c}{name}\u{201d}: {why}")),
        }
        self.show(root);
    }

    /// Shows the welcome page, or the current section of the open
    /// collection, reading it afresh.
    fn show(&mut self, root: &MainWindow) {
        root.show_welcome(self.open.is_none());
        for name in ["export-collection", "add-comic"] {
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
            root.show_empty("Could Not Read the Collection", &why.to_string());
        }
    }

    fn show_header(&self, root: &MainWindow) {
        let Some((collection, _)) = &self.open else {
            return;
        };
        match &self.shelf {
            Some(entry) => root.set_location(collection, &entry.name, self.section.name),
            None => root.set_location(collection, self.section.name, collection),
        }
        root.set_back_visible(self.shelf.is_some());
    }

    fn load(&mut self, root: &MainWindow) -> mylongbox_core::Result<()> {
        let Some((_, db)) = &self.open else {
            return Ok(());
        };
        let (shelf, empty, hint) = match (&self.shelf, self.section.content) {
            (Some(entry), _) => (
                entry.shelf,
                "No Books Here",
                "You don\u{2019}t own any of these books.",
            ),
            (None, Content::Books(shelf, empty, hint)) => (shelf, empty, hint),
            (None, Content::Names(list, empty, hint)) => {
                let entries = db.shelf_list(list)?;
                self.names.clear();
                if entries.is_empty() {
                    root.show_empty(empty, hint);
                } else {
                    self.names.extend_from_iter(entries.into_iter().map(Name));
                    root.show_names();
                }
                return Ok(());
            }
            (None, Content::NotYet) => {
                root.show_empty(self.section.name, "Not available yet.");
                return Ok(());
            }
        };
        let rows = db.browse(shelf)?;
        self.grid.clear();
        self.table.clear();
        if rows.is_empty() {
            root.show_empty(empty, hint);
        } else {
            self.grid.extend_from_iter(rows.iter().cloned().map(Book));
            self.table.extend_from_iter(rows.into_iter().map(Book));
            root.show_books(self.as_list);
        }
        Ok(())
    }
}

fn add_action(root: &MainWindow, name: &str, sender: &ComponentSender<App>, msg: fn() -> AppMsg) {
    let action = gio::SimpleAction::new(name, None);
    let sender = sender.clone();
    action.connect_activate(move |_, _| sender.input(msg()));
    root.add_action(&action);
}

fn fill_sidebar(root: &MainWindow, sender: &ComponentSender<App>) {
    let sidebar = root.sidebar();
    let mut group_starts = Vec::new();
    let mut index = 0;
    for group in SECTIONS {
        group_starts.push(index);
        for section in *group {
            let label = gtk::Label::builder()
                .label(section.name)
                .xalign(0.0)
                .margin_start(6)
                .margin_end(6)
                .build();
            sidebar.append(&label);
            index += 1;
        }
    }
    sidebar.set_header_func(move |row, _| {
        let starts_group = row.index() > 0 && group_starts.contains(&(row.index() as usize));
        row.set_header(starts_group.then(gtk::Separator::default).as_ref());
    });
    if let Some(first) = sidebar.row_at_index(0) {
        sidebar.select_row(Some(&first));
    }
    let sender = sender.clone();
    sidebar.connect_row_activated(move |_, row| {
        if let Ok(index) = usize::try_from(row.index()) {
            sender.input(AppMsg::SectionSelected(index));
        }
    });
}

fn ask_for_name(root: &MainWindow, sender: &ComponentSender<App>) {
    let entry = gtk::Entry::builder()
        .placeholder_text("My Comics")
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("New Collection")
        .body("Choose a name. You can keep several collections and switch between them.")
        .extra_child(&entry)
        .focus_widget(&entry)
        .default_response("create")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "_Cancel"), ("create", "C_reate")]);
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

fn collection_filters() -> gio::ListStore {
    let collections = gtk::FileFilter::new();
    collections.set_name(Some("MyLongboxDB collections"));
    collections.add_suffix("sqlite");
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&collections);
    filters.append(&all);
    filters
}

fn pick_file_to_import(root: &MainWindow, sender: &ComponentSender<App>) {
    let dialog = gtk::FileDialog::builder()
        .title("Import Collection File")
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

fn pick_export_file(root: &MainWindow, sender: &ComponentSender<App>, name: &str) {
    let dialog = gtk::FileDialog::builder()
        .title("Export a Copy")
        .modal(true)
        .initial_name(format!("{name}.sqlite"))
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

fn confirm_discard(root: &MainWindow, sender: &ComponentSender<App>) {
    let dialog = adw::AlertDialog::builder()
        .heading("Discard Changes?")
        .body("The changes made in this form have not been saved.")
        .default_response("keep")
        .close_response("keep")
        .build();
    dialog.add_responses(&[("keep", "_Keep Editing"), ("discard", "_Discard")]);
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    let sender = sender.clone();
    dialog.choose(Some(root), None::<&gio::Cancellable>, move |response| {
        if response == "discard" {
            sender.input(AppMsg::FormClose);
        }
    });
}
