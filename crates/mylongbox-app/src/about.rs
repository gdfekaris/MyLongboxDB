//! The About window: which version of the app is running, and how big
//! the open collection has grown.

use adw::prelude::*;
use gtk::glib;

use crate::i18n::{gettext, gettext_f};

/// Where the source code is kept. Shown as text rather than a link, since
/// the app hands nothing to other programs, a web browser included.
const REPOSITORY: &str = "https://github.com/gdfekaris/MyLongboxDB";

/// How much room the open collection takes.
pub struct Size {
    pub collection: String,
    pub total: u64,
    pub photos: u64,
}

/// e.g. `33 MB, of which cover photos 31 MB`.
pub fn size_text(total: u64, photos: u64) -> String {
    gettext_f(
        "{size}, of which cover photos {photos}",
        &[
            ("size", &glib::format_size(total)),
            ("photos", &glib::format_size(photos)),
        ],
    )
}

pub fn show(parent: &impl IsA<gtk::Widget>, size: Option<Size>) {
    let mut comments = vec![gettext(
        "Catalog and search your comic books, magazines, and collected editions, \
         with everything kept in one file on this computer and nothing sent anywhere.",
    )];
    if let Some(size) = size {
        comments.push(gettext_f(
            "Collection \u{201c}{name}\u{201d}: {size}",
            &[
                ("name", &size.collection),
                ("size", &size_text(size.total, size.photos)),
            ],
        ));
    }
    comments.push(gettext_f(
        "Source code: {address}",
        &[("address", REPOSITORY)],
    ));
    // The comments are read as markup; none of this text is meant as any.
    let comments: Vec<String> = comments
        .iter()
        .map(|c| glib::markup_escape_text(c).to_string())
        .collect();
    let dialog = adw::AboutDialog::builder()
        .application_name("MyLongboxDB")
        .application_icon(crate::APP_ID)
        .developer_name("gdfekaris")
        .version(env!("CARGO_PKG_VERSION"))
        .copyright("© 2026 gdfekaris")
        .license_type(gtk::License::MitX11)
        .comments(comments.join("\n\n"))
        .build();
    dialog.present(Some(parent));
}
