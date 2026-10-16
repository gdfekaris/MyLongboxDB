//! MyLongboxDB desktop application.

#![forbid(unsafe_code)]

mod about;
mod app;
mod autocomplete;
mod books;
mod bulk_dialog;
mod chart;
mod covers;
mod customize;
mod filter_fields;
mod filter_panel;
mod form;
mod format;
mod gaps_page;
mod i18n;
mod import_dialog;
mod issue_page;
mod photo_editor;
mod prefs;
mod prompt;
mod series_dialog;
mod settings;
mod shortcuts;
mod sorting;
mod stats_page;
mod trash_page;
mod viewer;
mod window;
mod work;

const APP_ID: &str = "io.github.gdfekaris.MyLongboxDB";

fn main() {
    use_translations();
    restrict_self();
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}

/// Points gettext at the translations installed beside the app, in
/// `share/locale` next to the `bin` folder that holds it: `/app` in the
/// Flatpak. Without them the app is in English.
fn use_translations() {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("share/locale")));
    if let Some(dir) = dir
        && let Err(why) = mylongbox_sandbox::locale::use_translations(i18n::DOMAIN, &dir)
    {
        eprintln!("mylongboxdb: {why}");
    }
}

/// Locks the app down before GTK or anything else starts a thread or
/// opens a file or socket, since threads already running would escape
/// the file limits. Inside the Flatpak the sandbox already keeps the app
/// from the network and the owner's files, so if an unusual kernel
/// refuses a restriction, the app says so and carries on.
fn restrict_self() {
    use gtk::glib;
    let (data, cache, config) = (
        glib::user_data_dir(),
        glib::user_cache_dir(),
        glib::user_config_dir(),
    );
    for dir in [&data, &cache, &config] {
        let _ = std::fs::create_dir_all(dir);
    }
    // Files picked in the file chooser are reached through the document
    // portal's folder; the graphics driver's devices live under /dev.
    let portal = glib::user_runtime_dir().join("doc");
    let devices = std::path::Path::new("/dev");
    match mylongbox_sandbox::limit_writes(&[&data, &cache, &config, &portal, devices]) {
        Ok(mylongbox_sandbox::Enforced::Fully) => {}
        Ok(level) => eprintln!("mylongboxdb: file limits enforced: {level:?}"),
        Err(why) => eprintln!("mylongboxdb: could not limit file access: {why}"),
    }
    if let Err(why) = mylongbox_sandbox::block_internet() {
        eprintln!("mylongboxdb: could not block internet sockets: {why}");
    }
}
