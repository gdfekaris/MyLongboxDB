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
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}
