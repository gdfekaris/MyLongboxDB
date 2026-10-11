//! How books and name-list entries look in the results views.

use adw::prelude::*;
use gtk::pango::EllipsizeMode;
use mylongbox_core::{Binding, BookCondition, Category, CopyRow, CoverDate, GradeType, ShelfEntry};
use relm4::typed_view::column::{RelmColumn, TypedColumnView};
use relm4::typed_view::grid::RelmGridItem;
use relm4::typed_view::list::RelmListItem;

/// One physical book in the cover grid or the list.
pub struct Book(pub CopyRow);

pub struct BookWidgets {
    badge: gtk::Label,
    series: gtk::Label,
    issue: gtk::Label,
}

/// Comic covers are about 2:3.
const COVER_WIDTH: i32 = 140;
const COVER_HEIGHT: i32 = 212;

impl RelmGridItem for Book {
    type Root = gtk::Box;
    type Widgets = BookWidgets;

    fn setup(_: &gtk::ListItem) -> (gtk::Box, BookWidgets) {
        let placeholder = gtk::Image::builder()
            .icon_name("image-x-generic-symbolic")
            .pixel_size(48)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["dim-label"])
            .build();
        let cover = gtk::Box::builder()
            .width_request(COVER_WIDTH)
            .height_request(COVER_HEIGHT)
            .css_classes(["card"])
            .build();
        cover.append(&placeholder);
        let badge = gtk::Label::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(6)
            .margin_end(6)
            .css_classes(["cover-badge"])
            .build();
        let overlay = gtk::Overlay::builder().child(&cover).build();
        overlay.add_overlay(&badge);

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
        (
            root,
            BookWidgets {
                badge,
                series,
                issue,
            },
        )
    }

    fn bind(&mut self, widgets: &mut BookWidgets, root: &mut gtk::Box) {
        let row = &self.0;
        let badge = badge(row);
        widgets.badge.set_visible(!badge.is_empty());
        widgets.badge.set_label(&badge);
        widgets.series.set_label(&row.series);
        widgets.issue.set_label(&issue_line(row));
        root.set_tooltip_text(Some(&row.label()));
    }
}

/// The grade of a comic or magazine, or the binding of a book.
fn badge(row: &CopyRow) -> String {
    match row.category {
        Category::CollectedEdition => row
            .binding
            .map(|b| {
                match b {
                    Binding::Hardcover => "HC",
                    Binding::Softcover => "TPB",
                    Binding::OversizedHardcover => "OHC",
                }
                .to_owned()
            })
            .unwrap_or_default(),
        Category::Comic | Category::Magazine => grade(row),
    }
}

/// e.g. `CGC 9.8`, or `9.4` for the owner's own estimate.
fn grade(row: &CopyRow) -> String {
    let Some(grade) = row.grade else {
        return String::new();
    };
    match row.grade_type {
        Some(GradeType::Cgc) => format!("CGC {grade}"),
        Some(GradeType::Cbcs) => format!("CBCS {grade}"),
        Some(GradeType::Pgx) => format!("PGX {grade}"),
        Some(GradeType::Raw) | None => grade.to_string(),
    }
}

fn condition(row: &CopyRow) -> String {
    match row.category {
        Category::CollectedEdition => row
            .book_condition
            .map(|c| {
                match c {
                    BookCondition::Sealed => "Sealed",
                    BookCondition::New => "New",
                    BookCondition::LikeNew => "Like New",
                    BookCondition::VeryGood => "Very Good",
                    BookCondition::Good => "Good",
                    BookCondition::Fair => "Fair",
                    BookCondition::Poor => "Poor",
                }
                .to_owned()
            })
            .unwrap_or_default(),
        Category::Comic | Category::Magazine => grade(row),
    }
}

fn number(row: &CopyRow) -> String {
    row.number
        .as_ref()
        .map(|n| format!("#{n}"))
        .unwrap_or_default()
}

/// e.g. `Cover B · 2nd printing`. A first printing goes unsaid.
fn version(row: &CopyRow) -> String {
    let printing = row.printing.filter(|&p| p > 1).map(|p| {
        let suffix = match (p % 10, p % 100) {
            (_, 11..=13) => "th",
            (1, _) => "st",
            (2, _) => "nd",
            (3, _) => "rd",
            _ => "th",
        };
        format!("{p}{suffix} printing")
    });
    [Some(row.variant.clone()), printing]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The line under the cover: number and version, or a book's own title.
fn issue_line(row: &CopyRow) -> String {
    let first = match &row.number {
        Some(_) => number(row),
        None => row.title.clone(),
    };
    [first, version(row)]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn cover_date(date: Option<CoverDate>) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match date {
        None => String::new(),
        Some(d) => match d.month() {
            Some(m) => format!("{} {}", MONTHS[usize::from(m - 1)], d.year()),
            None => d.year().to_string(),
        },
    }
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
    ($name:ident, $title:literal, expand: $expand:literal, xalign: $xalign:literal, $text:expr) => {
        pub struct $name;

        impl RelmColumn for $name {
            type Root = gtk::Label;
            type Widgets = ();
            type Item = Book;

            const COLUMN_NAME: &'static str = $title;
            const ENABLE_RESIZE: bool = true;
            const ENABLE_EXPAND: bool = $expand;

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

text_column!(SeriesColumn, "Series", expand: true, xalign: 0.0, |r| r.series.clone());
text_column!(NumberColumn, "No.", expand: false, xalign: 0.0, number);
text_column!(TitleColumn, "Title", expand: true, xalign: 0.0, |r| r.title.clone());
text_column!(VersionColumn, "Variant", expand: false, xalign: 0.0, version);
text_column!(DateColumn, "Cover Date", expand: false, xalign: 0.0, |r| {
    cover_date(r.cover_date)
});
text_column!(GradeColumn, "Grade", expand: false, xalign: 0.0, condition);
text_column!(LocationColumn, "Location", expand: false, xalign: 0.0, location);
text_column!(ValueColumn, "Value", expand: false, xalign: 1.0, |r| {
    r.value.map(|v| v.to_string()).unwrap_or_default()
});

pub fn book_table() -> TypedColumnView<Book, gtk::SingleSelection> {
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

/// One entry of a name list, such as a series or a creator.
pub struct Name(pub ShelfEntry);

pub struct NameWidgets {
    name: gtk::Label,
    detail: gtk::Label,
    count: gtk::Label,
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
        let arrow = gtk::Image::from_icon_name("go-next-symbolic");
        let root = gtk::Box::builder()
            .spacing(12)
            .margin_start(6)
            .margin_end(6)
            .build();
        root.append(&text);
        root.append(&count);
        root.append(&arrow);
        (
            root,
            NameWidgets {
                name,
                detail,
                count,
            },
        )
    }

    fn bind(&mut self, widgets: &mut NameWidgets, _: &mut gtk::Box) {
        let entry = &self.0;
        widgets.name.set_label(&entry.name);
        widgets.detail.set_label(&entry.detail);
        widgets.detail.set_visible(!entry.detail.is_empty());
        widgets.count.set_label(&match entry.copies {
            0 => "None owned".to_owned(),
            1 => "1 book".to_owned(),
            n => format!("{n} books"),
        });
    }
}

/// Styles the badge drawn over a cover.
pub const CSS: &str = "
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
        CopyFields, Database, EditionFields, Grade, IssueFields, IssueNumber, SeriesFields, Shelf,
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
    fn printings_get_ordinals() {
        let names: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111]
            .map(|p| {
                version(&CopyRow {
                    printing: Some(p),
                    ..row()
                })
            })
            .into();
        assert_eq!(
            names,
            [
                "",
                "2nd printing",
                "3rd printing",
                "4th printing",
                "11th printing",
                "12th printing",
                "13th printing",
                "21st printing",
                "22nd printing",
                "101st printing",
                "111th printing",
            ]
        );
    }

    #[test]
    fn issue_lines_join_number_and_version() {
        let r = CopyRow {
            variant: "Cover B".into(),
            printing: Some(2),
            ..row()
        };
        assert_eq!(issue_line(&r), "#1 · Cover B · 2nd printing");
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
    fn cover_dates_read_naturally() {
        assert_eq!(cover_date(CoverDate::new(1975, Some(3))), "Mar 1975");
        assert_eq!(cover_date(CoverDate::new(1975, None)), "1975");
        assert_eq!(cover_date(None), "");
    }
}
