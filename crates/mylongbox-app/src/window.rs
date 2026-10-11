use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

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
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub grid_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub list_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub names_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub empty_page: TemplateChild<adw::StatusPage>,
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

    impl ObjectImpl for MainWindow {}
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
        self.imp()
            .toast_overlay
            .add_toast(adw::Toast::new(&message));
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
        self.set_title(Some(&format!("{collection} — MyLongboxDB")));
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
        imp.content_header.set_show_back_button(!visible);
    }

    /// Puts the views of books and names in their places.
    pub fn set_views(&self, grid: &gtk::GridView, list: &gtk::ColumnView, names: &gtk::ListView) {
        let imp = self.imp();
        imp.grid_scroller.set_child(Some(grid));
        imp.list_scroller.set_child(Some(list));
        imp.names_scroller.set_child(Some(names));
    }

    pub fn view_toggle(&self) -> adw::ToggleGroup {
        self.imp().view_toggle.get()
    }

    /// Shows books as a cover grid or a list.
    pub fn show_books(&self, as_list: bool) {
        let imp = self.imp();
        imp.view_toggle.set_visible(true);
        imp.content_stack
            .set_visible_child_name(if as_list { "list" } else { "grid" });
    }

    pub fn is_showing_books(&self) -> bool {
        matches!(
            self.imp().content_stack.visible_child_name().as_deref(),
            Some("grid" | "list")
        )
    }

    pub fn show_names(&self) {
        let imp = self.imp();
        imp.view_toggle.set_visible(false);
        imp.content_stack.set_visible_child_name("names");
    }

    /// Explains why there is nothing to show.
    pub fn show_empty(&self, title: &str, description: &str) {
        let imp = self.imp();
        imp.view_toggle.set_visible(false);
        imp.empty_page.set_title(title);
        imp.empty_page.set_description(Some(description));
        imp.content_stack.set_visible_child_name("empty");
    }

    /// On narrow windows, moves from the sidebar to the content.
    pub fn show_content(&self) {
        self.imp().split_view.set_show_content(true);
    }
}
