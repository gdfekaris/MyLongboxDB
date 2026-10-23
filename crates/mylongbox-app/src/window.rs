use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, gettext_f, ngettext_f};
use crate::style;

// The template macro reads the Blueprint file without telling Cargo about
// it; including the bytes makes edits to the file trigger a rebuild.
const _: &[u8] = include_bytes!("../../../data/ui/window.blp");

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(file = "../../data/ui/window.blp")]
    pub struct MainWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        /// The message saying work is under way, until another replaces it.
        pub working: std::cell::RefCell<Option<adw::Toast>>,
        #[template_child]
        pub stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub window_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub content_nav: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub section_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_header: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub back_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub view_toggle: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub select_toggle: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub sort_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub results_bar: TemplateChild<gtk::ActionBar>,
        #[template_child]
        pub results_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub add_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub entry_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub list_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub selection_bar: TemplateChild<gtk::ActionBar>,
        #[template_child]
        pub rates_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub search_toggle: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub filter_toggle: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub filter_bin: TemplateChild<adw::Bin>,
        #[template_child]
        pub correction_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub selection_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub grid_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub list_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub names_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub empty_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub custom_bin: TemplateChild<adw::Bin>,
        /// Where the grid and the list were, to scroll back to once
        /// they are next shown, when refilled under a page opened over
        /// them.
        pub pending_scroll: std::cell::Cell<Option<(f64, f64)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MainWindow {
        const NAME: &'static str = "MainWindow";
        type Type = super::MainWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MainWindow {
        fn constructed(&self) {
            self.parent_constructed();
            // The sidebar list keeps Tab to itself, so focus could never
            // leave it from the keyboard; Tab and Shift+Tab move on to the
            // books and the header instead.
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let content = self.content_page.get();
            keys.connect_key_pressed(move |_, key, _, _| {
                let direction = match key {
                    gtk::gdk::Key::Tab | gtk::gdk::Key::KP_Tab => gtk::DirectionType::TabForward,
                    gtk::gdk::Key::ISO_Left_Tab => gtk::DirectionType::TabBackward,
                    _ => return glib::Propagation::Proceed,
                };
                if content.child_focus(direction) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.sidebar.add_controller(keys);

            // Views refilled while hidden have no layout to scroll in
            // until they are shown again.
            let window = self.obj().downgrade();
            self.section_page.connect_shown(move |_| {
                if let Some(window) = window.upgrade()
                    && let Some(scroll) = window.imp().pending_scroll.take()
                {
                    window.restore_books_scroll(scroll);
                }
            });
        }
    }
    impl WidgetImpl for MainWindow {}
    impl WindowImpl for MainWindow {}
    impl ApplicationWindowImpl for MainWindow {}
    impl AdwApplicationWindowImpl for MainWindow {}
}

glib::wrapper! {
    pub struct MainWindow(ObjectSubclass<imp::MainWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::gio::ActionGroup, gtk::gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl MainWindow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Shows a short message at the bottom of the window. Messages from
    /// the core start in lowercase so they can be built into sentences;
    /// on their own they start with a capital.
    pub fn toast(&self, message: &str) {
        let mut chars = message.chars();
        let message: String = match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        };
        // Messages carry collection names and error text, which must not
        // be read as markup; markup goes off before the title is set.
        let toast = adw::Toast::new("");
        toast.set_use_markup(false);
        toast.set_title(&message);
        self.show_toast(toast);
    }

    /// Shows that work is under way, such as `Backing up…`, until the
    /// next message, which says how it went, takes its place.
    pub fn toast_working(&self, message: &str) {
        let toast = adw::Toast::new("");
        toast.set_use_markup(false);
        toast.set_title(message);
        toast.set_timeout(0);
        self.show_toast(toast.clone());
        self.imp().working.replace(Some(toast));
    }

    /// Takes away the message that work is under way, when it ended
    /// with something other than a message, such as a dialog opening.
    pub fn end_working(&self) {
        if let Some(working) = self.imp().working.take() {
            working.dismiss();
        }
    }

    fn show_toast(&self, toast: adw::Toast) {
        if let Some(working) = self.imp().working.take() {
            working.dismiss();
        }
        self.imp().toast_overlay.add_toast(toast);
    }

    /// A message with an Undo button that calls `undo`.
    pub fn toast_undo(&self, message: &str, undo: impl Fn() + 'static) {
        self.toast_button(message, &gettext("_Undo"), undo);
    }

    /// A message with a button, labelled `label`, that calls `pressed`.
    pub fn toast_button(&self, message: &str, label: &str, pressed: impl Fn() + 'static) {
        let toast = adw::Toast::new("");
        toast.set_use_markup(false);
        toast.set_title(message);
        toast.set_button_label(Some(label));
        toast.connect_button_clicked(move |_| pressed());
        self.show_toast(toast);
    }

    /// Shows the welcome page, or the open collection.
    pub fn show_welcome(&self, welcome: bool) {
        let page = if welcome { "welcome" } else { "collection" };
        self.imp().stack.set_visible_child_name(page);
    }

    pub fn sidebar(&self) -> gtk::ListBox {
        self.imp().sidebar.get()
    }

    /// Names the open collection in the window title, and what the
    /// content shows in its header.
    pub fn set_location(&self, collection: &str, title: &str, subtitle: &str) {
        let imp = self.imp();
        imp.window_title.set_title(title);
        imp.window_title.set_subtitle(subtitle);
        imp.content_page.set_title(title);
        imp.section_page.set_title(title);
        // Translators: the window title; {collection} is the open collection's name.
        self.set_title(Some(&gettext_f(
            "{collection} — MyLongboxDB",
            &[("collection", collection)],
        )));
    }

    /// Opens a page, such as a book's details, over the section.
    pub fn push_page(&self, page: &adw::NavigationPage) {
        self.imp().content_nav.push(page);
    }

    /// Closes any pages opened over the section, returning whether
    /// there were any.
    pub fn pop_to_section(&self) -> bool {
        let nav = &self.imp().content_nav;
        let open = nav.navigation_stack().n_items() > 1;
        if open {
            nav.pop_to_tag("section");
        }
        open
    }

    /// Closes the topmost page opened over the section, returning
    /// whether there was one.
    pub fn pop_page(&self) -> bool {
        let nav = &self.imp().content_nav;
        nav.navigation_stack().n_items() > 1 && nav.pop()
    }

    /// Shows the back button that leaves a shelf for its list of names.
    /// On narrow windows it replaces the button back to the sidebar.
    pub fn set_back_visible(&self, visible: bool) {
        let imp = self.imp();
        imp.back_button.set_visible(visible);
        // A shelf names one entry, which its menu can rename or merge.
        imp.entry_menu_button.set_visible(visible);
        imp.content_header.set_show_back_button(!visible);
    }

    /// Offers `menu` for the list of names shown, or hides it when no
    /// list is.
    pub fn set_list_menu(&self, menu: Option<&gio::MenuModel>) {
        let button = &self.imp().list_menu_button;
        button.set_menu_model(menu);
        button.set_visible(menu.is_some());
    }

    /// Puts the views of books and names in their places.
    pub fn set_views(&self, grid: &gtk::GridView, list: &gtk::ColumnView, names: &gtk::ListView) {
        let imp = self.imp();
        imp.grid_scroller.set_child(Some(grid));
        imp.list_scroller.set_child(Some(list));
        imp.names_scroller.set_child(Some(names));
    }

    /// Shows a reminder about the exchange rates, or hides it.
    pub fn set_rates_reminder(&self, reminder: Option<&str>) {
        let banner = &self.imp().rates_banner;
        banner.set_title(reminder.unwrap_or(""));
        banner.set_revealed(reminder.is_some());
    }

    /// Names the add button and fills the search box's hint in the
    /// style's words.
    pub fn set_wording(&self) {
        let imp = self.imp();
        imp.add_button
            .set_tooltip_text(Some(&gettext(style::word("Add"))));
        imp.search_entry
            .set_placeholder_text(Some(&gettext(style::word(
                "Search series, titles, people, characters, tags, notes\u{2026}",
            ))));
    }

    pub fn search_entry(&self) -> gtk::SearchEntry {
        self.imp().search_entry.get()
    }

    pub fn filter_toggle(&self) -> gtk::ToggleButton {
        self.imp().filter_toggle.get()
    }

    pub fn search_toggle(&self) -> gtk::ToggleButton {
        self.imp().search_toggle.get()
    }

    pub fn correction_banner(&self) -> adw::Banner {
        self.imp().correction_banner.get()
    }

    /// Puts the filter panel under the header, shown by the Filters
    /// button.
    pub fn set_filter_panel(&self, panel: &impl IsA<gtk::Widget>) {
        let imp = self.imp();
        imp.filter_bin.set_child(Some(panel));
        imp.search_bar.connect_entry(&*imp.search_entry);
        // Closing the search box, as with Escape, also stops searching.
        let entry = imp.search_entry.get();
        imp.search_toggle.connect_toggled(move |toggle| {
            if !toggle.is_active() {
                entry.set_text("");
            }
        });
        // Typing over the books starts a search, as in other GNOME apps.
        imp.search_bar
            .set_key_capture_widget(Some(&*imp.content_stack));
    }

    /// Says which corrected spelling the results are for, or hides the
    /// note.
    pub fn show_correction(&self, typed: &str, corrected: Option<&str>) {
        let banner = &self.imp().correction_banner;
        match corrected {
            Some(corrected) => {
                banner.set_title(&gettext_f(
                    "Nothing matches \u{201c}{typed}\u{201d}. Showing results for \u{201c}{corrected}\u{201d}.",
                    &[("typed", typed), ("corrected", corrected)],
                ));
                banner.set_revealed(true);
            }
            None => banner.set_revealed(false),
        }
    }

    /// Leaves searching and filtering, keeping the filter rows for next
    /// time.
    pub fn end_search(&self) {
        let imp = self.imp();
        imp.search_entry.set_text("");
        imp.search_toggle.set_active(false);
        imp.filter_toggle.set_active(false);
        imp.correction_banner.set_revealed(false);
    }

    pub fn select_toggle(&self) -> gtk::ToggleButton {
        self.imp().select_toggle.get()
    }

    /// Shows the bar of actions for selected books, saying how many.
    pub fn show_selection(&self, selecting: bool, count: u32) {
        let imp = self.imp();
        imp.selection_bar.set_revealed(selecting);
        imp.selection_label.set_label(&match count {
            0 => gettext("Select books to change"),
            n => ngettext_f(
                "{count} selected",
                "{count} selected",
                usize::try_from(n).unwrap_or(usize::MAX),
                &[("count", &n.to_string())],
            ),
        });
    }

    pub fn view_toggle(&self) -> adw::ToggleGroup {
        self.imp().view_toggle.get()
    }

    /// Shows how many books are shown and what they add up to, under
    /// them.
    pub fn set_totals(&self, text: &str) {
        self.imp().results_label.set_label(text);
    }

    /// Hides what only belongs with books: the view and sort buttons and
    /// the totals.
    fn hide_book_controls(&self) {
        let imp = self.imp();
        imp.view_toggle.set_visible(false);
        imp.sort_button.set_visible(false);
        imp.results_bar.set_revealed(false);
        imp.select_toggle.set_visible(false);
        imp.select_toggle.set_active(false);
    }

    /// Shows books as a cover grid or a list.
    pub fn show_books(&self, as_list: bool) {
        let imp = self.imp();
        imp.view_toggle.set_visible(true);
        imp.sort_button.set_visible(true);
        imp.results_bar.set_revealed(true);
        imp.select_toggle.set_visible(true);
        imp.content_stack
            .set_visible_child_name(if as_list { "list" } else { "grid" });
    }

    /// How far down the grid and the list are scrolled.
    pub fn books_scroll(&self) -> (f64, f64) {
        let imp = self.imp();
        // A refill while hidden has already lost the place it keeps.
        if let Some(pending) = imp.pending_scroll.get() {
            return pending;
        }
        (
            imp.grid_scroller.vadjustment().value(),
            imp.list_scroller.vadjustment().value(),
        )
    }

    /// Scrolls the grid and the list back to where `books_scroll` found
    /// them, once refilled. The views lay out their new rows only on the
    /// next frame, until when the old position may not fit, so it is set
    /// again once they have.
    pub fn restore_books_scroll(&self, (grid, list): (f64, f64)) {
        let imp = self.imp();
        if !imp.section_page.is_mapped() {
            imp.pending_scroll.set(Some((grid, list)));
            return;
        }
        imp.pending_scroll.set(None);
        let (grid_adj, list_adj) = (
            imp.grid_scroller.vadjustment(),
            imp.list_scroller.vadjustment(),
        );
        grid_adj.set_value(grid);
        list_adj.set_value(list);
        glib::idle_add_local_once(move || {
            grid_adj.set_value(grid);
            list_adj.set_value(list);
        });
    }

    /// How far down the list of names is scrolled.
    pub fn names_scroll(&self) -> f64 {
        self.imp().names_scroller.vadjustment().value()
    }

    /// Scrolls the list of names back to where `names_scroll` found it,
    /// once refilled; the new rows are laid out on the next frame.
    pub fn restore_names_scroll(&self, value: f64) {
        let adjustment = self.imp().names_scroller.vadjustment();
        adjustment.set_value(value);
        glib::idle_add_local_once(move || adjustment.set_value(value));
    }

    /// Drops a position kept for books no longer shown.
    pub fn forget_books_scroll(&self) {
        self.imp().pending_scroll.set(None);
    }

    pub fn is_showing_books(&self) -> bool {
        matches!(
            self.imp().content_stack.visible_child_name().as_deref(),
            Some("grid" | "list")
        )
    }

    /// Shows a view built for the section, such as Trash.
    pub fn show_custom(&self, view: &impl IsA<gtk::Widget>) {
        let imp = self.imp();
        self.hide_book_controls();
        imp.custom_bin.set_child(Some(view));
        imp.content_stack.set_visible_child_name("custom");
    }

    pub fn show_names(&self) {
        let imp = self.imp();
        self.hide_book_controls();
        imp.content_stack.set_visible_child_name("names");
    }

    /// Explains why there is nothing to show.
    pub fn show_empty(&self, title: &str, description: &str) {
        let imp = self.imp();
        self.hide_book_controls();
        imp.empty_page.set_title(title);
        // The description is always read as markup, and can hold error
        // text.
        imp.empty_page
            .set_description(Some(&glib::markup_escape_text(description)));
        imp.content_stack.set_visible_child_name("empty");
    }

    /// On narrow windows, moves from the sidebar to the content.
    pub fn show_content(&self) {
        self.imp().split_view.set_show_content(true);
    }
}
