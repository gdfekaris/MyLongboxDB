//! How the app looks: one of three built-in styles, each light or dark,
//! as picked in Preferences. The styles are compiled in and no
//! stylesheet is ever read from a file, so nothing outside the app can
//! change how it draws.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::i18n::gettext_noop;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// The GNOME look the app has always had.
    #[default]
    Standard,
    /// Grey bevelled controls and navy title bars of early-1990s
    /// desktops, with smooth modern type.
    Retro,
    /// A 1930s Los Angeles newsstand in an Art Deco tower: gold,
    /// turquoise and geometric in its bones, inky and bright in its
    /// details.
    Pulp,
}

/// The styles in the order Preferences lists them.
pub const STYLES: [(&str, Style); 3] = [
    // Translators: the app's usual look.
    (gettext_noop("Standard"), Style::Standard),
    // Translators: a look styled after early-1990s desktop computers.
    (gettext_noop("Retro Desktop"), Style::Retro),
    // Translators: a look mixing 1930s comic books with Art Deco.
    (gettext_noop("Deco Pulp"), Style::Pulp),
];

impl Style {
    /// The name the settings file keeps it under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Retro => "retro",
            Self::Pulp => "pulp",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        STYLES.iter().map(|s| s.1).find(|s| s.key() == key)
    }

    fn css(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Retro => include_str!("styles/retro.css"),
            Self::Pulp => include_str!("styles/pulp.css"),
        }
    }
}

/// Light or dark. The owner picks; the desktop's own setting is not
/// followed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Light,
    Dark,
}

impl Scheme {
    pub fn key(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Light, Self::Dark]
            .into_iter()
            .find(|s| s.key() == key)
    }
}

thread_local! {
    static PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
    static ADDED: Cell<bool> = const { Cell::new(false) };
    static CURRENT: Cell<Style> = const { Cell::new(Style::Standard) };
    static CHASE: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    /// The desktop's own window-button layout and scroll bars, from
    /// before a style changed them.
    static DESKTOP: RefCell<Option<(Option<glib::GString>, bool)>> = const { RefCell::new(None) };
}

/// Window furniture beyond the stylesheets: Retro Desktop's control-menu
/// box at the left of the title bar, and, for Retro Desktop and Deco
/// Pulp, scroll bars that stay in view rather than appearing over the
/// content, so they can be drawn in the style. Any other style gets the
/// desktop's own back.
fn furnish(style: Style) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    DESKTOP.with_borrow_mut(|desktop| {
        let (layout, overlay) = desktop
            .get_or_insert_with(|| {
                (
                    settings.gtk_decoration_layout(),
                    settings.is_gtk_overlay_scrolling(),
                )
            })
            .clone();
        if style == Style::Retro {
            settings.set_gtk_decoration_layout(Some("icon:minimize,maximize,close"));
        } else {
            settings.set_gtk_decoration_layout(layout.as_deref());
        }
        settings.set_gtk_overlay_scrolling(match style {
            Style::Retro | Style::Pulp => false,
            Style::Standard => overlay,
        });
    });
}

/// How often Deco Pulp's marquee bulbs step along at night.
const CHASE_STEP: Duration = Duration::from_millis(1200);

/// Steps the marquee bulbs along by marking the header bars in turn,
/// while `on`. A timer does it rather than a stylesheet animation, which
/// would redraw the bar every frame for a change seen twice a cycle.
/// Nothing moves while the desktop has animations turned off.
fn chase(on: bool) {
    CHASE.with_borrow_mut(|chase| {
        if let Some(timer) = chase.take() {
            timer.remove();
        }
        for bar in header_bars() {
            bar.remove_css_class("chase");
        }
        if on {
            *chase = Some(glib::timeout_add_local(CHASE_STEP, || {
                let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
                for bar in header_bars() {
                    if bar.has_css_class("chase") {
                        bar.remove_css_class("chase");
                    } else if animate {
                        bar.add_css_class("chase");
                    }
                }
                glib::ControlFlow::Continue
            }));
        }
    });
}

/// Every header bar in the app's windows, shown or not.
fn header_bars() -> Vec<gtk::Widget> {
    fn walk(widget: gtk::Widget, found: &mut Vec<gtk::Widget>) {
        if widget.css_name() == "headerbar" {
            found.push(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            walk(c, found);
        }
    }
    let mut found = Vec::new();
    for window in gtk::Window::list_toplevels() {
        walk(window, &mut found);
    }
    found
}

/// The style shown, for the few pieces a style adds beyond its
/// stylesheet. Whatever shows them is rebuilt when it changes.
pub fn current() -> Style {
    CURRENT.get()
}

/// Deco Pulp's names for a few of the app's places and actions, as
/// a shopkeeper would say them; `name` as is in every other style. Both
/// are untranslated, for `gettext`.
pub fn word(name: &'static str) -> &'static str {
    if current() != Style::Pulp {
        return name;
    }
    match name {
        // Translators: Deco Pulp's name for the Everything shelf, the
        // whole collection, as in a comic shop's back-issue stacks.
        "Everything" => gettext_noop("The Stacks"),
        // Translators: Deco Pulp's name for the Key Issues shelf.
        "Key Issues" => gettext_noop("The Vault"),
        // Translators: Deco Pulp's name for the Graded Books shelf.
        "Graded Books" => gettext_noop("Grading Room"),
        // Translators: Deco Pulp's name for Collection Stats, after Los
        // Angeles' Art Deco observatory.
        "Collection Stats" => gettext_noop("The Observatory"),
        // Translators: Deco Pulp's word for adding a book, as comics are
        // bagged and boarded.
        "Add" => gettext_noop("Bag It"),
        // Translators: Deco Pulp's search box hint.
        "Search series, titles, people, characters, tags, notes\u{2026}" => {
            gettext_noop("Search the stacks\u{2026}")
        }
        _ => name,
    }
}

/// Shows the app in `style` and `scheme`. The first call adds the
/// style's stylesheet to `display`, after the app's own, so a style can
/// restyle the app's few custom pieces too.
pub fn apply(display: &gdk::Display, style: Style, scheme: Scheme) {
    let (adw_scheme, css_scheme) = match scheme {
        Scheme::Light => (
            adw::ColorScheme::ForceLight,
            gtk::InterfaceColorScheme::Light,
        ),
        Scheme::Dark => (adw::ColorScheme::ForceDark, gtk::InterfaceColorScheme::Dark),
    };
    CURRENT.set(style);
    chase(style == Style::Pulp && scheme == Scheme::Dark);
    furnish(style);
    adw::StyleManager::default().set_color_scheme(adw_scheme);
    PROVIDER.with(|provider| {
        // Each stylesheet answers its own `prefers-color-scheme`
        // queries, and ours would otherwise always answer light.
        provider.set_prefers_color_scheme(css_scheme);
        provider.load_from_string(style.css());
        if !ADDED.replace(true) {
            gtk::style_context_add_provider_for_display(
                display,
                provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for (_, style) in STYLES {
            assert_eq!(Style::from_key(style.key()), Some(style));
        }
        for scheme in [Scheme::Light, Scheme::Dark] {
            assert_eq!(Scheme::from_key(scheme.key()), Some(scheme));
        }
        assert_eq!(Style::from_key("neon"), None);
        assert_eq!(Scheme::from_key("auto"), None);
    }

    #[test]
    fn only_deco_pulp_renames_places() {
        CURRENT.set(Style::Pulp);
        assert_eq!(word("Everything"), "The Stacks");
        assert_eq!(word("Comics"), "Comics");
        CURRENT.set(Style::Retro);
        assert_eq!(word("Everything"), "Everything");
        CURRENT.set(Style::Standard);
    }
}
