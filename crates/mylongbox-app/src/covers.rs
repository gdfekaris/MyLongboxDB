//! Cover photos on screen, and readying new ones.
//!
//! Stored photos are decoded by the app's own size-limited decoder and
//! handed to GTK as plain pixels, never to GTK's image loaders, since a
//! collection file may have come from someone else.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Weak;

use adw::prelude::*;
use gtk::{gdk, glib};
use mylongbox_core::{CopyId, CoverPhoto, Database, Side};
use mylongbox_media::{
    Edit, MAX_SAVED_SIDE, Output, RgbImage, THUMBNAIL_EDGE, apply_edit, decode_bytes, decode_saved,
    encode, preview, read_file, upright,
};

/// Thumbnails kept decoded: a few screens of the cover grid.
const CACHED: usize = 240;

#[derive(Default)]
struct Cache {
    db: Weak<Database>,
    thumbnails: HashMap<CopyId, Option<gdk::Texture>>,
    /// Oldest first, so the longest unused is dropped first.
    order: VecDeque<CopyId>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
}

/// Points thumbnails at the open collection, or at none.
pub fn set_database(db: Weak<Database>) {
    CACHE.with_borrow_mut(|c| {
        *c = Cache {
            db,
            ..Cache::default()
        };
    });
}

/// Drops what is cached for a copy whose photos changed.
pub fn forget(copy: CopyId) {
    CACHE.with_borrow_mut(|c| {
        c.thumbnails.remove(&copy);
        c.order.retain(|k| *k != copy);
    });
}

/// The thumbnail of a copy's front cover, if it has one.
pub fn thumbnail(copy: CopyId) -> Option<gdk::Texture> {
    thumbnail_of(copy, Side::Front)
}

/// The thumbnail of either side. Only fronts are cached, as the grid
/// shows those again and again.
pub fn thumbnail_of(copy: CopyId, side: Side) -> Option<gdk::Texture> {
    if side == Side::Front
        && let Some(found) = CACHE.with_borrow(|c| c.thumbnails.get(&copy).cloned())
    {
        return found;
    }
    let db = CACHE.with_borrow(|c| c.db.upgrade())?;
    let texture = db
        .photo_thumbnail(copy, side)
        .ok()
        .flatten()
        .and_then(|jpeg| thumbnail_texture(&jpeg));
    if side == Side::Front {
        CACHE.with_borrow_mut(|c| {
            if c.order.len() >= CACHED
                && let Some(oldest) = c.order.pop_front()
            {
                c.thumbnails.remove(&oldest);
            }
            c.thumbnails.insert(copy, texture.clone());
            c.order.push_back(copy);
        });
    }
    texture
}

/// Decodes a stored full-size photo for display, or `None` if it cannot
/// be read or is not one the app could have saved.
pub fn texture(jpeg: &[u8]) -> Option<gdk::Texture> {
    texture_of(&decode_saved(jpeg, MAX_SAVED_SIDE).ok()?)
}

/// Decodes a stored thumbnail for display, like [`texture`], held to the
/// thumbnail's own size, since so many are kept at once.
pub fn thumbnail_texture(jpeg: &[u8]) -> Option<gdk::Texture> {
    texture_of(&decode_saved(jpeg, THUMBNAIL_EDGE).ok()?)
}

/// Hands pixels to GTK to show.
pub fn texture_of(image: &RgbImage) -> Option<gdk::Texture> {
    let (width, height) = image.dimensions();
    let stride = usize::try_from(width).ok()?.checked_mul(3)?;
    let texture = gdk::MemoryTexture::new(
        i32::try_from(width).ok()?,
        i32::try_from(height).ok()?,
        gdk::MemoryFormat::R8g8b8,
        &glib::Bytes::from(image.as_raw().as_slice()),
        stride,
    );
    Some(texture.upcast())
}

/// The longest edge of the picture the editor shows.
const PREVIEW_EDGE: u32 = 1200;

/// A photo read and ready for the editor.
pub struct Loaded {
    /// Upright and in sRGB, at full size.
    pub full: RgbImage,
    pub preview: RgbImage,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pixels would flood any log.
        f.debug_struct("Loaded")
            .field("full", &self.full.dimensions())
            .finish_non_exhaustive()
    }
}

/// Reads a photo file and readies it for editing. Slow for large
/// photos, so it runs away from the window.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let bytes = read_file(path).map_err(|e| e.to_string())?;
    let full = confined(move || Ok(upright(decode_bytes(&bytes).map_err(|e| e.to_string())?)))?;
    let preview = preview(&full, PREVIEW_EDGE);
    Ok(Loaded { full, preview })
}

/// Decodes on a thread of its own that can reach no files, sockets, or
/// other descriptors, so a photo crafted to take over a decoder finds
/// nothing within reach of that thread. The decoders are the code most
/// exposed to files from elsewhere, and partly written in C.
fn confined<T: Send + 'static>(
    decode: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let thread = std::thread::Builder::new()
        .name("photo".into())
        .spawn(move || {
            let _decoders = mylongbox_media::load_decoders();
            match mylongbox_sandbox::confine_thread() {
                Ok(mylongbox_sandbox::Enforced::Fully) => {}
                // The filter still holds where Landlock does not.
                Ok(level) => eprintln!("mylongboxdb: photo thread file limits: {level:?}"),
                Err(why) => return Err(format!("the photo could not be read safely: {why}")),
            }
            decode()
        })
        .map_err(|e| e.to_string())?;
    thread
        .join()
        .unwrap_or_else(|_| Err("the photo could not be read".into()))
}

/// Applies the editor's changes to the full photo and makes the JPEGs
/// the collection keeps. Also slow, so also away from the window.
pub fn finish(full: &RgbImage, edit: &Edit, output: Output) -> Result<CoverPhoto, String> {
    let saved = if edit.is_identity() {
        encode(full, output)
    } else {
        encode(&apply_edit(full, edit), output)
    }
    .map_err(|e| e.to_string())?;
    Ok(CoverPhoto {
        full_jpeg: saved.full,
        thumb_jpeg: saved.thumbnail,
        width: saved.width,
        height: saved.height,
        sha256: saved.sha256,
    })
}

/// Makes the JPEGs the collection keeps from a photo file's bytes,
/// decoded with the same limits as a photo picked by hand. Slow, so it
/// runs away from the window.
pub fn prepare(bytes: &[u8], output: Output) -> Result<CoverPhoto, String> {
    let bytes = bytes.to_vec();
    let full = confined(move || Ok(upright(decode_bytes(&bytes).map_err(|e| e.to_string())?)))?;
    finish(&full, &Edit::default(), output)
}

/// What stands in for a missing photo: a card naming the missing side.
/// A `small` one, for the grid, shows only a short line.
pub fn placeholder(text: &str, small: bool) -> gtk::Box {
    let icon = gtk::Image::builder()
        .icon_name("image-missing-symbolic")
        .pixel_size(if small { 32 } else { 48 })
        .css_classes(["dim-label"])
        .build();
    let label = gtk::Label::builder()
        .label(text)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(14)
        .css_classes(if small {
            ["caption", "dim-label"]
        } else {
            ["caption-heading", "dim-label"]
        })
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .hexpand(true)
        .margin_start(8)
        .margin_end(8)
        .build();
    content.append(&icon);
    content.append(&label);
    content
}

/// A photo shown whole within its frame, inside something, such as a
/// button, that already says what it is to screen readers.
pub fn picture(texture: &gdk::Texture, fit: gtk::ContentFit) -> gtk::Picture {
    gtk::Picture::builder()
        .accessible_role(gtk::AccessibleRole::Presentation)
        .paintable(texture)
        .content_fit(fit)
        .can_shrink(true)
        .hexpand(true)
        .vexpand(true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mylongbox_media::Crop;
    use std::rc::Rc;

    fn photo_of(width: u32, height: u32) -> CoverPhoto {
        let image = image_of(width, height);
        let saved = encode(&image, Output::default()).unwrap();
        CoverPhoto {
            full_jpeg: saved.full,
            thumb_jpeg: saved.thumbnail,
            width: saved.width,
            height: saved.height,
            sha256: saved.sha256,
        }
    }

    fn image_of(width: u32, height: u32) -> mylongbox_media::RgbImage {
        mylongbox_media::RgbImage::from_pixel(width, height, mylongbox_media::Rgb([200, 40, 40]))
    }

    /// Makes `n` copies of one book.
    fn copies(db: &Database, n: usize) -> Vec<CopyId> {
        use mylongbox_core::{
            Category, CopyFields, EditionFields, IssueFields, IssueNumber, SeriesFields,
        };
        let series = db
            .create_series(&SeriesFields::new("X-Men", Category::Comic))
            .unwrap();
        let issue = db
            .create_issue(
                series,
                &IssueFields::new(Some(IssueNumber::new("1").unwrap())),
            )
            .unwrap();
        let edition = db.create_edition(issue, &EditionFields::default()).unwrap();
        (0..n)
            .map(|_| db.create_copy(edition, &CopyFields::default()).unwrap())
            .collect()
    }

    #[test]
    fn stored_photos_become_textures_of_their_size() {
        let photo = photo_of(200, 300);
        let shown = texture(&photo.full_jpeg).unwrap();
        assert_eq!((shown.width(), shown.height()), (200, 300));
        assert!(texture(b"\xFF\xD8\xFF not really a JPEG").is_none());
        assert!(texture(b"").is_none());
    }

    #[test]
    fn thumbnails_are_cached_until_forgotten() {
        let db = Rc::new(Database::open_in_memory().unwrap());
        let copy = copies(&db, 1)[0];
        set_database(Rc::downgrade(&db));
        assert!(thumbnail(copy).is_none());
        db.set_photo(copy, Side::Front, &photo_of(400, 600))
            .unwrap();
        assert!(thumbnail(copy).is_none(), "the empty answer is cached");
        forget(copy);
        let texture = thumbnail(copy).unwrap();
        assert_eq!((texture.width(), texture.height()), (213, 320));
        assert!(thumbnail_of(copy, Side::Back).is_none());

        // Closing the collection leaves nothing to show.
        set_database(Weak::new());
        assert!(thumbnail(copy).is_none());
    }

    #[test]
    fn the_cache_keeps_only_the_most_recent() {
        let db = Rc::new(Database::open_in_memory().unwrap());
        let copies = copies(&db, CACHED + 10);
        set_database(Rc::downgrade(&db));
        for copy in &copies {
            thumbnail(*copy);
        }
        CACHE.with_borrow(|c| {
            assert_eq!(c.thumbnails.len(), CACHED);
            assert_eq!(c.order.len(), CACHED);
            assert!(!c.thumbnails.contains_key(&copies[9]));
            assert!(c.thumbnails.contains_key(&copies[10]));
            assert!(c.thumbnails.contains_key(&copies[CACHED + 9]));
        });
        set_database(Weak::new());
    }

    #[test]
    fn photo_files_are_prepared_for_keeping() {
        let dir = std::env::temp_dir().join("mylongbox-covers-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cover.png");
        image_of(1000, 3000).save(&path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.full.dimensions(), (1000, 3000));
        assert_eq!(loaded.preview.dimensions(), (400, 1200));
        let photo = finish(&loaded.full, &Edit::default(), Output::default()).unwrap();
        assert_eq!((photo.width, photo.height), (533, 1600));
        assert!(photo.full_jpeg.starts_with(&[0xFF, 0xD8, 0xFF]));
        let thumbnail = thumbnail_texture(&photo.thumb_jpeg).unwrap();
        assert!(thumbnail_texture(&photo.full_jpeg).is_none(), "too large");
        assert_eq!((thumbnail.width(), thumbnail.height()), (107, 320));

        // The edit is applied to the full photo: turned, then halved.
        let mut edit = Edit::default();
        edit.turn_right();
        edit.crop = Crop {
            left: 0.0,
            top: 0.0,
            right: 0.5,
            bottom: 1.0,
        };
        let photo = finish(&loaded.full, &edit, Output::default()).unwrap();
        assert_eq!((photo.width, photo.height), (1500, 1000));

        std::fs::write(&path, b"not a photo").unwrap();
        assert_eq!(
            load(&path).unwrap_err(),
            "this is not a JPEG, PNG, WebP, or HEIC photo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
