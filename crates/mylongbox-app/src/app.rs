use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::window::MainWindow;

pub struct App;

impl SimpleComponent for App {
    type Init = ();
    type Input = ();
    type Output = ();
    type Root = MainWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        MainWindow::new()
    }

    fn init(
        _init: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        ComponentParts {
            model: App,
            widgets: (),
        }
    }
}
