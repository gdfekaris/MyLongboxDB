//! The owner's display preferences, kept in a small file in the app's
//! own config folder. They belong to the person, not to a collection.

use std::path::{Path, PathBuf};

use gtk::glib;
use mylongbox_core::Currency;
use mylongbox_media::{LONG_EDGES, Output, QUALITIES};

const GROUP: &str = "display";
const IMAGES: &str = "images";
const BACKUP: &str = "backup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The currency amounts are shown in, or `None` for each as entered.
    pub currency: Option<Currency>,
    /// Whether grades show their letters, as in `9.4 NM`.
    pub grade_names: bool,
    /// How cover photos are saved.
    pub photos: Output,
    /// Whether a copy of the collection is kept before risky changes.
    pub safety_copies: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            currency: None,
            grade_names: false,
            photos: Output::default(),
            safety_copies: true,
        }
    }
}

/// Opens the settings file, or an empty one if it cannot be read.
fn read(path: &Path) -> glib::KeyFile {
    let file = glib::KeyFile::new();
    let _ = file.load_from_file(path, glib::KeyFileFlags::KEEP_COMMENTS);
    file
}

fn write(file: &glib::KeyFile, path: &Path) -> Result<(), glib::Error> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    file.save_to_file(path)
}

/// The folder backups are written to, once chosen.
pub fn backup_folder(path: &Path) -> Option<PathBuf> {
    let folder = read(path).string(BACKUP, "folder").ok()?;
    (!folder.is_empty()).then(|| PathBuf::from(folder.as_str()))
}

/// Remembers the folder backups are written to, keeping the rest of the
/// settings as they are.
pub fn set_backup_folder(path: &Path, folder: &Path) -> Result<(), glib::Error> {
    let file = read(path);
    file.set_string(BACKUP, "folder", &folder.to_string_lossy());
    write(&file, path)
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
            safety_copies: file.boolean(BACKUP, "safety-copies").unwrap_or(true),
        }
    }

    /// Saves the settings, keeping anything else the file holds, such as
    /// the backup folder.
    pub fn save(&self, path: &Path) -> Result<(), glib::Error> {
        let file = read(path);
        file.set_string(GROUP, "currency", self.currency.map_or("", |c| c.code()));
        file.set_boolean(GROUP, "grade-names", self.grade_names);
        file.set_integer(
            IMAGES,
            "long-edge",
            i32::try_from(self.photos.long_edge).unwrap_or(i32::MAX),
        );
        file.set_integer(IMAGES, "quality", i32::from(self.photos.quality));
        file.set_boolean(BACKUP, "safety-copies", self.safety_copies);
        write(&file, path)
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
            safety_copies: false,
        };
        wanted.save(&path).unwrap();
        assert_eq!(Settings::load(&path), wanted);
        // The backup folder is kept apart, and saving keeps it.
        assert_eq!(backup_folder(&path), None);
        set_backup_folder(&path, Path::new("/run/user/1000/doc/ab12/Backups")).unwrap();
        Settings::default().save(&path).unwrap();
        assert_eq!(
            backup_folder(&path).as_deref(),
            Some(Path::new("/run/user/1000/doc/ab12/Backups"))
        );
        assert_eq!(Settings::load(&path), Settings::default());
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
             [images]\nlong-edge=99999\nquality=7\n[backup]\nsafety-copies=perhaps\n",
        )
        .unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::write(&path, "[images]\nlong-edge=-5\nquality=many\n").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
