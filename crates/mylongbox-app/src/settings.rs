//! The owner's display preferences, kept in a small file in the app's
//! own config folder. They belong to the person, not to a collection.

use std::path::{Path, PathBuf};

use gtk::glib;
use mylongbox_core::Currency;
use mylongbox_media::{LONG_EDGES, Output, QUALITIES};

const GROUP: &str = "display";
const IMAGES: &str = "images";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// The currency amounts are shown in, or `None` for each as entered.
    pub currency: Option<Currency>,
    /// Whether grades show their letters, as in `9.4 NM`.
    pub grade_names: bool,
    /// How cover photos are saved.
    pub photos: Output,
}

pub fn path() -> PathBuf {
    glib::user_config_dir().join("settings.ini")
}

impl Settings {
    /// Reads the settings, falling back to the defaults for anything
    /// missing or unreadable, so a damaged file never stops the app.
    pub fn load(path: &Path) -> Self {
        let file = glib::KeyFile::new();
        if file.load_from_file(path, glib::KeyFileFlags::NONE).is_err() {
            return Self::default();
        }
        Self {
            currency: file
                .string(GROUP, "currency")
                .ok()
                .and_then(|code| Currency::from_code(&code)),
            grade_names: file.boolean(GROUP, "grade-names").unwrap_or(false),
            photos: Output {
                long_edge: file
                    .integer(IMAGES, "long-edge")
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| LONG_EDGES.contains(n))
                    .unwrap_or(Output::default().long_edge),
                quality: file
                    .integer(IMAGES, "quality")
                    .ok()
                    .and_then(|n| u8::try_from(n).ok())
                    .filter(|n| QUALITIES.contains(n))
                    .unwrap_or(Output::default().quality),
            },
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), glib::Error> {
        let file = glib::KeyFile::new();
        file.set_string(GROUP, "currency", self.currency.map_or("", |c| c.code()));
        file.set_boolean(GROUP, "grade-names", self.grade_names);
        file.set_integer(
            IMAGES,
            "long-edge",
            i32::try_from(self.photos.long_edge).unwrap_or(i32::MAX),
        );
        file.set_integer(IMAGES, "quality", i32::from(self.photos.quality));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        file.save_to_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mylongbox-settings-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("settings.ini")
    }

    #[test]
    fn settings_round_trip() {
        let path = scratch("round-trip");
        let wanted = Settings {
            currency: Some(Currency::Gold),
            grade_names: true,
            photos: Output {
                long_edge: 2000,
                quality: 90,
            },
        };
        wanted.save(&path).unwrap();
        assert_eq!(Settings::load(&path), wanted);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_or_damaged_settings_fall_back_to_defaults() {
        let path = scratch("damaged");
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[display]\ncurrency=PESOS\ngrade-names=maybe\n\
             [images]\nlong-edge=99999\nquality=7\n",
        )
        .unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::write(&path, "[images]\nlong-edge=-5\nquality=many\n").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
