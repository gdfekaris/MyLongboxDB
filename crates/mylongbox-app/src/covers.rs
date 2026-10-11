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
use mylongbox_media::{Output, decode_bytes, decode_file, encode, upright};

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
        .and_then(|jpeg| texture(&jpeg));
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

/// Decodes a stored JPEG for display, or `None` if it cannot be read.
pub fn texture(jpeg: &[u8]) -> Option<gdk::Texture> {
    let image = decode_bytes(jpeg).ok()?.image.into_rgb8();
    let (width, height) = image.dimensions();
    let stride = usize::try_from(width).ok()?.checked_mul(3)?;
    let texture = gdk::MemoryTexture::new(
        i32::try_from(width).ok()?,
        i32::try_from(height).ok()?,
        gdk::MemoryFormat::R8g8b8,
        &glib::Bytes::from_owned(image.into_raw()),
        stride,
    );
    Some(texture.upcast())
}

/// Reads a photo file and makes the JPEGs the collection keeps. Slow
/// for large photos, so it runs away from the window.
pub fn prepare(path: &Path, output: Output) -> Result<CoverPhoto, String> {
    let decoded = decode_file(path).map_err(|e| e.to_string())?;
    let saved = encode(&upright(decoded), output).map_err(|e| e.to_string())?;
    Ok(CoverPhoto {
        full_jpeg: saved.full,
        thumb_jpeg: saved.thumbnail,
        width: saved.width,
        height: saved.height,
        sha256: saved.sha256,
    })
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

/// A photo shown whole within its frame.
pub fn picture(texture: &gdk::Texture, fit: gtk::ContentFit) -> gtk::Picture {
    gtk::Picture::builder()
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
        let photo = prepare(&path, Output::default()).unwrap();
        assert_eq!((photo.width, photo.height), (533, 1600));
        assert!(photo.full_jpeg.starts_with(&[0xFF, 0xD8, 0xFF]));
        let thumbnail = texture(&photo.thumb_jpeg).unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (107, 320));

        std::fs::write(&path, b"not a photo").unwrap();
        assert_eq!(
            prepare(&path, Output::default()).unwrap_err(),
            "this is not a JPEG, PNG, WebP, or HEIC photo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
