//! How books and name-list entries look in the results views.

use adw::prelude::*;
use gtk::gio;
use gtk::pango::EllipsizeMode;
use mylongbox_core::{Category, CopyRow, Database, Need, Shelf, ShelfEntry};
use relm4::typed_view::column::{RelmColumn, TypedColumnView};
use relm4::typed_view::grid::RelmGridItem;
use relm4::typed_view::list::RelmListItem;

use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};
use crate::{covers, format};

/// One physical book in the cover grid or the list, and whether its card
/// says what the book still needs, as on the Needs Attention shelf.
pub struct Book(pub CopyRow, pub bool);

pub struct BookWidgets {
    photo: gtk::Picture,
    missing: gtk::Box,
    badge: gtk::Label,
    signed: gtk::Label,
    series: gtk::Label,
    issue: gtk::Label,
    needs: gtk::Label,
}

/// Comic covers are about 2:3.
const COVER_WIDTH: i32 = 140;
const COVER_HEIGHT: i32 = 212;

impl RelmGridItem for Book {
    type Root = gtk::Box;
    type Widgets = BookWidgets;

    fn setup(_: &gtk::ListItem) -> (gtk::Box, BookWidgets) {
        let missing = covers::placeholder(&gettext("No Cover Image"), true);
        // Filling the tile keeps the grid even; the book's page shows the
        // whole photo.
        // The title under it names the book, so the cover is passed
        // over by screen readers.
        let photo = gtk::Picture::builder()
            .accessible_role(gtk::AccessibleRole::Presentation)
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        let cover = gtk::Box::builder()
            .width_request(COVER_WIDTH)
            .height_request(COVER_HEIGHT)
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["card"])
            .build();
        cover.append(&photo);
        cover.append(&missing);
        let badge = gtk::Label::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(6)
            .margin_end(6)
            .css_classes(["cover-badge"])
            .build();
        let signed = gtk::Label::builder()
            .label(gettext("Signed"))
            .halign(gtk::Align::Start)
            .valign(gtk::Align::End)
            .margin_bottom(6)
            .margin_start(6)
            .css_classes(["cover-badge"])
            .build();
        let overlay = gtk::Overlay::builder().child(&cover).build();
        overlay.add_overlay(&badge);
        overlay.add_overlay(&signed);

        let label = |classes: &[&str]| {
            gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(EllipsizeMode::End)
                .max_width_chars(1)
                .css_classes(classes)
                .build()
        };
        // Two lines, so the start year that tells relaunches apart
        // usually survives.
        let series = label(&["caption-heading"]);
        series.set_wrap(true);
        series.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        series.set_lines(2);
        let issue = label(&["caption", "dim-label"]);
        let needs = label(&["caption", "warning"]);
        needs.set_wrap(true);
        needs.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        needs.set_lines(2);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .width_request(COVER_WIDTH)
            .build();
        root.append(&overlay);
        root.append(&series);
        root.append(&issue);
        root.append(&needs);
        (
            root,
            BookWidgets {
                photo,
                missing,
                badge,
                signed,
                series,
                issue,
                needs,
            },
        )
    }

    fn bind(&mut self, widgets: &mut BookWidgets, root: &mut gtk::Box) {
        let row = &self.0;
        let texture = covers::thumbnail(row.copy);
        widgets.photo.set_paintable(texture.as_ref());
        widgets.photo.set_visible(texture.is_some());
        widgets.missing.set_visible(texture.is_none());
        let badge = badge(row);
        widgets.badge.set_visible(!badge.is_empty());
        widgets.badge.set_label(&badge);
        widgets.signed.set_visible(row.signed);
        widgets.series.set_label(&row.series);
        widgets.issue.set_label(&issue_line(row));
        let needs = if self.1 {
            needs_line(row)
        } else {
            String::new()
        };
        widgets.needs.set_visible(!needs.is_empty());
        widgets.needs.set_label(&needs);
        root.set_tooltip_text(Some(&row.label()));
    }
}

/// What a book still needs, e.g. `Needs back photo, value`, or nothing
/// when it is complete.
pub fn needs_line(row: &CopyRow) -> String {
    let needs: Vec<String> = row
        .needs()
        .into_iter()
        .map(|need| match need {
            // Translators: these name what a book still lacks, listed
            // after "Needs", as in "Needs back photo, value".
            Need::FrontPhoto => gettext("front photo"),
            Need::BackPhoto => gettext("back photo"),
            Need::Grade if row.category == Category::CollectedEdition => gettext("condition"),
            Need::Grade => gettext("grade"),
            Need::StorageBox => gettext("box"),
            Need::Value => gettext("value"),
        })
        .collect();
    if needs.is_empty() {
        String::new()
    } else {
        // Translators: {missing} is a list such as "back photo, value".
        gettext_f("Needs {missing}", &[("missing", &needs.join(", "))])
    }
}

/// The grade of a comic or magazine, or the binding of a book.
fn badge(row: &CopyRow) -> String {
    match row.category {
        Category::CollectedEdition => row
            .binding
            .map(format::binding_badge)
            .unwrap_or_default()
            .to_owned(),
        Category::Comic | Category::Magazine => format::grade(row.grade, row.grade_type),
    }
}

/// The grade or book condition, and whether the copy is signed.
fn condition(row: &CopyRow) -> String {
    let condition = match row.category {
        Category::CollectedEdition => row
            .book_condition
            .map(format::condition)
            .unwrap_or_default()
            .to_owned(),
        Category::Comic | Category::Magazine => format::grade(row.grade, row.grade_type),
    };
    format::joined([
        condition,
        if row.signed {
            gettext("Signed")
        } else {
            String::new()
        },
    ])
}

fn number(row: &CopyRow) -> String {
    row.number
        .as_ref()
        .map(|n| format!("#{n}"))
        .unwrap_or_default()
}

/// e.g. `Cover B · 2nd printing`. A first printing goes unsaid.
pub fn version(variant: &str, printing: Option<u32>) -> String {
    let printing = printing.filter(|&p| p > 1).map(format::printing);
    format::joined([variant.to_owned(), printing.unwrap_or_default()])
}

/// The line under the cover: number, cover date, and version, or a
/// book's own title in place of the number.
fn issue_line(row: &CopyRow) -> String {
    let first = match &row.number {
        Some(_) => number(row),
        None => row.title.clone(),
    };
    format::joined([
        first,
        format::cover_date(row.cover_date),
        version(&row.variant, row.printing),
    ])
}

fn location(row: &CopyRow) -> String {
    match (&row.storage_box, row.box_position) {
        (Some(name), Some(position)) => format!("{name} · {position}"),
        (Some(name), None) => name.clone(),
        (None, _) => String::new(),
    }
}

/// Declares a list column of one line of text.
macro_rules! text_column {
    ($name:ident, $title:expr, expand: $expand:literal, xalign: $xalign:literal, $text:expr) => {
        pub struct $name;

        impl RelmColumn for $name {
            type Root = gtk::Label;
            type Widgets = ();
            type Item = Book;

            const COLUMN_NAME: &'static str = $title;
            const ENABLE_RESIZE: bool = true;
            const ENABLE_EXPAND: bool = $expand;

            fn header_title() -> String {
                gettext(Self::COLUMN_NAME)
            }

            fn setup(_: &gtk::ListItem) -> (gtk::Label, ()) {
                let label = gtk::Label::builder()
                    .xalign($xalign)
                    .ellipsize(EllipsizeMode::End)
                    .build();
                (label, ())
            }

            fn bind(item: &mut Book, _: &mut (), label: &mut gtk::Label) {
                let text: fn(&CopyRow) -> String = $text;
                label.set_label(&text(&item.0));
            }
        }
    };
}

text_column!(SeriesColumn, gettext_noop("Series"), expand: true, xalign: 0.0, |r| r.series.clone());
// Translators: the heading of the issue number column, short for
// "number".
text_column!(NumberColumn, gettext_noop("No."), expand: false, xalign: 0.0, number);
text_column!(TitleColumn, gettext_noop("Title"), expand: true, xalign: 0.0, |r| r.title.clone());
text_column!(VersionColumn, gettext_noop("Variant"), expand: false, xalign: 0.0, |r| {
    version(&r.variant, r.printing)
});
text_column!(DateColumn, gettext_noop("Cover Date"), expand: false, xalign: 0.0, |r| {
    format::cover_date(r.cover_date)
});
text_column!(GradeColumn, gettext_noop("Grade"), expand: false, xalign: 0.0, condition);
text_column!(LocationColumn, gettext_noop("Location"), expand: false, xalign: 0.0, location);
text_column!(ValueColumn, gettext_noop("Value"), expand: false, xalign: 1.0, |r| {
    // What it is worth now, so at the newest rate.
    r.value.map(|v| format::money(v, None)).unwrap_or_default()
});

pub fn book_table() -> TypedColumnView<Book, gtk::MultiSelection> {
    let mut table = TypedColumnView::new();
    table.append_column::<SeriesColumn>();
    table.append_column::<NumberColumn>();
    table.append_column::<TitleColumn>();
    table.append_column::<VersionColumn>();
    table.append_column::<DateColumn>();
    table.append_column::<GradeColumn>();
    table.append_column::<LocationColumn>();
    table.append_column::<ValueColumn>();
    table
}

/// An entry of a name list, and its position in the list.
pub struct Name(pub ShelfEntry, pub u32);

pub struct NameWidgets {
    name: gtk::Label,
    detail: gtk::Label,
    count: gtk::Label,
    menu: gtk::MenuButton,
}

/// What can be done to an entry of a name list, from its own menu or
/// from the menu of the shelf it leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manage {
    Rename,
    Aliases,
    Merge,
    Delete,
    EditSeries,
}

impl Manage {
    const ALL: [(Self, &'static str); 5] = [
        (Self::Rename, "rename"),
        (Self::Aliases, "aliases"),
        (Self::Merge, "merge"),
        (Self::Delete, "delete"),
        (Self::EditSeries, "edit-series"),
    ];

    fn code(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(m, _)| *m == self)
            .map_or("", |(_, c)| c)
    }
}

/// The action target naming what to do to the entry at `position` in
/// the list shown, e.g. `rename 12`, for the `win.manage-name` action.
fn manage_target(what: Manage, position: u32) -> String {
    format!("{} {position}", what.code())
}

/// Reads a target made by [`manage_target`].
pub fn parse_manage_target(target: &str) -> Option<(Manage, u32)> {
    let (what, position) = target.split_once(' ')?;
    let what = Manage::ALL.iter().find(|(_, c)| *c == what)?.0;
    Some((what, position.parse().ok()?))
}

/// The menu of things to do to `shelf`'s entry, at `position` in its
/// list.
fn manage_menu(shelf: Shelf, position: u32) -> gio::Menu {
    let menu = gio::Menu::new();
    let add = |label: String, what: Manage| {
        let item = gio::MenuItem::new(Some(&label), None);
        let target = manage_target(what, position);
        item.set_action_and_target_value(Some("win.manage-name"), Some(&target.to_variant()));
        menu.append_item(&item);
    };
    if matches!(shelf, Shelf::Series(_)) {
        add(gettext("_Edit Series…"), Manage::EditSeries);
    }
    add(gettext("_Rename…"), Manage::Rename);
    if Database::shelf_has_aliases(shelf) {
        add(gettext("_Alternate Names…"), Manage::Aliases);
    }
    add(gettext("_Merge Into…"), Manage::Merge);
    add(gettext("_Delete…"), Manage::Delete);
    menu
}

impl RelmListItem for Name {
    type Root = gtk::Box;
    type Widgets = NameWidgets;

    fn setup(_: &gtk::ListItem) -> (gtk::Box, NameWidgets) {
        let name = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(EllipsizeMode::End)
            .build();
        let detail = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(EllipsizeMode::End)
            .css_classes(["caption", "dim-label"])
            .build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        text.append(&name);
        text.append(&detail);
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "numeric"])
            .build();
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(gettext("Manage This Entry"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let arrow = gtk::Image::from_icon_name("go-next-symbolic");
        let root = gtk::Box::builder()
            .spacing(12)
            .margin_start(6)
            .margin_end(6)
            .build();
        root.append(&text);
        root.append(&count);
        root.append(&menu);
        root.append(&arrow);
        (
            root,
            NameWidgets {
                name,
                detail,
                count,
                menu,
            },
        )
    }

    fn bind(&mut self, widgets: &mut NameWidgets, _: &mut gtk::Box) {
        let entry = &self.0;
        widgets.name.set_label(&entry.name);
        widgets
            .menu
            .set_menu_model(Some(&manage_menu(entry.shelf, self.1)));
        widgets.detail.set_label(&entry.detail);
        widgets.detail.set_visible(!entry.detail.is_empty());
        widgets.count.set_label(&match entry.copies {
            0 => gettext("None owned"),
            n => ngettext_f(
                "{count} book",
                "{count} books",
                usize::try_from(n).unwrap_or(usize::MAX),
                &[("count", &n.to_string())],
            ),
        });
    }
}

/// Styles the badge drawn over a cover, and covers that are buttons.
pub const CSS: &str = "
.cover-button {
    padding: 0;
}
.cover-badge {
    background-color: alpha(black, 0.75);
    color: white;
    border-radius: 6px;
    padding: 1px 6px;
    font-size: 0.8em;
    font-weight: bold;
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use mylongbox_core::{
        Binding, CopyFields, Database, EditionFields, Grade, GradeType, IssueFields, IssueNumber,
        SeriesFields, Shelf,
    };

    /// A plain copy of X-Men (1963) #1, as browsing finds it.
    fn row() -> CopyRow {
        let db = Database::open_in_memory().unwrap();
        let mut series = SeriesFields::new("X-Men", Category::Comic);
        series.start_year = Some(1963);
        let series = db.create_series(&series).unwrap();
        let number = IssueNumber::new("1").unwrap();
        let issue = db
            .create_issue(series, &IssueFields::new(Some(number)))
            .unwrap();
        let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
        db.create_copy(edition, &CopyFields::default()).unwrap();
        db.browse(Shelf::Everything).unwrap().remove(0)
    }

    #[test]
    fn issue_lines_join_number_date_and_version() {
        let r = CopyRow {
            variant: "Cover B".into(),
            printing: Some(2),
            ..row()
        };
        assert_eq!(issue_line(&r), "#1 · Cover B · 2nd printing");
        let dated = CopyRow {
            cover_date: mylongbox_core::CoverDate::new(1988, Some(5)),
            ..r
        };
        assert_eq!(issue_line(&dated), "#1 · May 1988 · Cover B · 2nd printing");
        let book = CopyRow {
            number: None,
            title: "Spider-Man Omnibus".into(),
            ..row()
        };
        assert_eq!(issue_line(&book), "Spider-Man Omnibus");
    }

    #[test]
    fn badges_show_grades_or_bindings() {
        let slab = CopyRow {
            grade: Grade::parse("9.8"),
            grade_type: Some(GradeType::Cgc),
            ..row()
        };
        assert_eq!(badge(&slab), "CGC 9.8");
        let raw = CopyRow {
            grade: Grade::parse("9.4"),
            grade_type: Some(GradeType::Raw),
            ..row()
        };
        assert_eq!(badge(&raw), "9.4");
        assert_eq!(badge(&row()), "");
        let book = CopyRow {
            category: Category::CollectedEdition,
            binding: Some(Binding::Softcover),
            ..row()
        };
        assert_eq!(badge(&book), "TPB");
    }

    #[test]
    fn manage_targets_read_back() {
        for (what, _) in Manage::ALL {
            let target = manage_target(what, 41);
            assert_eq!(parse_manage_target(&target), Some((what, 41)), "{target}");
        }
        for bad in ["", "rename", "rename x", "frobnicate 3", "rename -1"] {
            assert_eq!(parse_manage_target(bad), None, "{bad}");
        }
    }
}
