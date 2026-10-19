//! Cover photos: a front and a back for each copy, kept in the
//! collection file as JPEGs the import pipeline made.

use rusqlite::{OptionalExtension, params};

use crate::catalog::{live, query_all};
use crate::db::Database;
use crate::error::{Error, Result};
use crate::model::{CopyId, RowUuid, Side};

/// The largest side a stored photo may have. The pipeline saves at most
/// 2400 pixels; this leaves room to raise that without letting absurd
/// sizes in.
pub const MAX_PHOTO_SIDE: u32 = 10_000;

/// A cover photo ready to store, or read back whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverPhoto {
    pub full_jpeg: Vec<u8>,
    pub thumb_jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// SHA-256 of `full_jpeg`.
    pub sha256: [u8; 32],
}

impl Database {
    /// Stores the photo of one side of a copy, replacing any already
    /// there.
    pub fn set_photo(&self, copy: CopyId, side: Side, photo: &CoverPhoto) -> Result<()> {
        live(self.copy(copy)?.trash)?;
        let sizes = 1..=MAX_PHOTO_SIDE;
        if !sizes.contains(&photo.width) || !sizes.contains(&photo.height) {
            return Err(Error::Invalid(format!(
                "a photo of {} \u{d7} {} pixels cannot be stored",
                photo.width, photo.height
            )));
        }
        // Only JPEGs the pipeline wrote belong here; anything else would
        // later be handed to the decoder as one.
        if !is_jpeg(&photo.full_jpeg) || !is_jpeg(&photo.thumb_jpeg) {
            return Err(Error::Invalid("a cover photo must be a JPEG".into()));
        }
        self.conn.execute(
            "INSERT INTO cover_photo
                 (uuid, copy_id, side, full_jpeg, thumb_jpeg, width, height, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (copy_id, side) DO UPDATE SET
                 full_jpeg = excluded.full_jpeg,
                 thumb_jpeg = excluded.thumb_jpeg,
                 width = excluded.width,
                 height = excluded.height,
                 sha256 = excluded.sha256",
            params![
                RowUuid::new_random(),
                copy,
                side,
                photo.full_jpeg,
                photo.thumb_jpeg,
                photo.width,
                photo.height,
                hex(&photo.sha256),
            ],
        )?;
        Ok(())
    }

    /// The photo of one side of a copy, whole.
    pub fn photo(&self, copy: CopyId, side: Side) -> Result<Option<CoverPhoto>> {
        Ok(self
            .conn
            .query_row(
                "SELECT full_jpeg, thumb_jpeg, width, height, sha256 FROM cover_photo
                 WHERE copy_id = ?1 AND side = ?2",
                params![copy, side],
                |row| {
                    Ok(CoverPhoto {
                        full_jpeg: row.get(0)?,
                        thumb_jpeg: row.get(1)?,
                        width: row.get(2)?,
                        height: row.get(3)?,
                        sha256: unhex(&row.get::<_, String>(4)?).unwrap_or_default(),
                    })
                },
            )
            .optional()?)
    }

    /// Just the thumbnail, as the cover grid needs, without reading the
    /// full photo.
    pub fn photo_thumbnail(&self, copy: CopyId, side: Side) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row(
                "SELECT thumb_jpeg FROM cover_photo WHERE copy_id = ?1 AND side = ?2",
                params![copy, side],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// How many bytes the photos take up, full size and thumbnails
    /// together, counting those of copies in Trash; most of a collection
    /// file is its photos.
    pub fn photo_bytes(&self) -> Result<u64> {
        let bytes: i64 = self.conn.query_row(
            "SELECT coalesce(sum(length(full_jpeg) + length(thumb_jpeg)), 0) FROM cover_photo",
            [],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(bytes).unwrap_or(0))
    }

    /// Which photos a copy has: front, then back, then signature.
    pub fn photo_sides(&self, copy: CopyId) -> Result<Vec<Side>> {
        query_all(
            &self.conn,
            "SELECT side FROM cover_photo WHERE copy_id = ?1
             ORDER BY CASE side WHEN 'front' THEN 0 WHEN 'back' THEN 1 ELSE 2 END",
            [copy],
            |row| row.get(0),
        )
    }

    /// Removes the photo of one side of a copy and gives it back, so the
    /// removal can be undone.
    pub fn remove_photo(&self, copy: CopyId, side: Side) -> Result<CoverPhoto> {
        live(self.copy(copy)?.trash)?;
        let tx = self.tx()?;
        let photo = self.photo(copy, side)?.ok_or(Error::NotFound)?;
        self.conn.execute(
            "DELETE FROM cover_photo WHERE copy_id = ?1 AND side = ?2",
            params![copy, side],
        )?;
        tx.commit()?;
        Ok(photo)
    }

    /// Exchanges the front and back photos of a copy, for when they were
    /// added the wrong way round. A single side moves to the other.
    pub fn swap_photos(&self, copy: CopyId) -> Result<()> {
        live(self.copy(copy)?.trash)?;
        let tx = self.tx()?;
        let front = self.photo(copy, Side::Front)?;
        let back = self.photo(copy, Side::Back)?;
        // Each side is unique per copy, and checked row by row, so the
        // photos are taken out and put back rather than relabelled. Only
        // these two sides: a signature photo is no side of the cover.
        self.conn.execute(
            "DELETE FROM cover_photo WHERE copy_id = ?1 AND side IN (?2, ?3)",
            params![copy, Side::Front, Side::Back],
        )?;
        if let Some(back) = back {
            self.set_photo(copy, Side::Front, &back)?;
        }
        if let Some(front) = front {
            self.set_photo(copy, Side::Back, &front)?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Whether `data` begins as a JPEG does.
fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut out = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}
