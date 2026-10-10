//! MyLongboxDB desktop application.

#![forbid(unsafe_code)]

mod app;
mod window;

const APP_ID: &str = "io.github.gdfekaris.MyLongboxDB";

fn main() {
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}
