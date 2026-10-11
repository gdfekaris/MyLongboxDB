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

    /// Names the open collection and the section being shown.
    pub fn set_location(&self, collection: &str, section: &str) {
        let imp = self.imp();
        imp.window_title.set_title(section);
        imp.window_title.set_subtitle(collection);
        imp.content_page.set_title(section);
        self.set_title(Some(&format!("{collection} — MyLongboxDB")));
    }

    /// On narrow windows, moves from the sidebar to the content.
    pub fn show_content(&self) {
        self.imp().split_view.set_show_content(true);
    }
}
