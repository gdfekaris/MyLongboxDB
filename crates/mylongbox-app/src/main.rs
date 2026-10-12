//! MyLongboxDB desktop application.

#![forbid(unsafe_code)]

mod app;
mod autocomplete;
mod books;
mod bulk_dialog;
mod chart;
mod covers;
mod filter_fields;
mod filter_panel;
mod form;
mod format;
mod gaps_page;
mod import_dialog;
mod issue_page;
mod photo_editor;
mod prefs;
mod prompt;
mod settings;
mod sorting;
mod trash_page;
mod viewer;
mod window;

const APP_ID: &str = "io.github.gdfekaris.MyLongboxDB";

fn main() {
    // First, before GTK or anything else can open a socket. Inside the
    // Flatpak there is no network to reach anyway, so if an unusual
    // kernel refuses the filter, the app says so and carries on.
    if let Err(why) = mylongbox_sandbox::block_internet() {
        eprintln!("mylongboxdb: could not block internet sockets: {why}");
    }
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}
