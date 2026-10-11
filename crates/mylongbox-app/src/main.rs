//! MyLongboxDB desktop application.

#![forbid(unsafe_code)]

mod app;
mod autocomplete;
mod books;
mod form;
mod format;
mod issue_page;
mod window;

const APP_ID: &str = "io.github.gdfekaris.MyLongboxDB";

fn main() {
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}
