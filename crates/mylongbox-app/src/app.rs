use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gio, glib};
use mylongbox_core::{CollectionInfo, CollectionStore, Database};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::window::MainWindow;

/// The sidebar, in groups separated by a line.
const SECTIONS: &[&[&str]] = &[
    &["Everything", "Comics", "Magazines", "Collected Editions"],
    &[
        "Series",
        "Publishers",
        "Creators",
        "Characters",
        "Teams",
        "Story Arcs",
        "Storage Locations",
    ],
    &["Key Issues", "Gaps", "Duplicates", "Needs Attention"],
    &["Trash"],
];

pub struct App {
    /// The folder of collections, or why it could not be used.
    store: Result<CollectionStore, String>,
    /// The open collection and its name.
    open: Option<(String, Database)>,
    section: &'static str,
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
        fill_sidebar(&root, &sender);

        let store = CollectionStore::new(glib::user_data_dir().join("collections"))
            .map_err(|e| e.to_string());
        let mut model = App {
            store,
            open: None,
            section: SECTIONS[0][0],
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
                if let Some(section) = SECTIONS.iter().flat_map(|g| g.iter()).nth(index) {
                    self.section = section;
                    self.show(root);
                    root.show_content();
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

    fn open_collection(&mut self, name: &str, root: &MainWindow) {
        // Release the current collection's lock before taking another.
        self.open = None;
        match self
            .store()
            .and_then(|s| s.open(name).map_err(|e| e.to_string()))
        {
            Ok(db) => self.open = Some((name.to_owned(), db)),
            Err(why) => root.toast(&format!("Could not open \u{201c}{name}\u{201d}: {why}")),
        }
        self.show(root);
    }

    fn show(&self, root: &MainWindow) {
        root.show_welcome(self.open.is_none());
        if let Some((name, _)) = &self.open {
            root.set_location(name, self.section);
        }
        if let Some(action) = root.lookup_action("export-collection")
            && let Ok(action) = action.downcast::<gio::SimpleAction>()
        {
            action.set_enabled(self.open.is_some());
        }
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
        for name in *group {
            let label = gtk::Label::builder()
                .label(*name)
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
