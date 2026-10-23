//! How the app looks: one of three built-in styles, each light or dark,
//! as picked in Preferences. The styles are compiled in and no
//! stylesheet is ever read from a file, so nothing outside the app can
//! change how it draws.

use std::cell::Cell;

use gtk::gdk;

use crate::i18n::gettext_noop;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// The GNOME look the app has always had.
    #[default]
    Standard,
    /// Grey bevelled controls and navy title bars of early-1990s
    /// desktops, with smooth modern type.
    Retro,
    /// A comic shop's inked panels and bright primaries set in Art Deco
    /// gold, stepped borders and geometric capitals.
    Deco,
}

/// The styles in the order Preferences lists them.
pub const STYLES: [(&str, Style); 3] = [
    // Translators: the app's usual look.
    (gettext_noop("Standard"), Style::Standard),
    // Translators: a look styled after early-1990s desktop computers.
    (gettext_noop("Retro Desktop"), Style::Retro),
    // Translators: a look mixing comic-shop colors with Art Deco.
    (gettext_noop("Comic Deco"), Style::Deco),
];

impl Style {
    /// The name the settings file keeps it under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Retro => "retro",
            Self::Deco => "deco",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        STYLES.iter().map(|s| s.1).find(|s| s.key() == key)
    }

    fn css(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Retro => include_str!("styles/retro.css"),
            Self::Deco => include_str!("styles/deco.css"),
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
}
