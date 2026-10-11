//! Reading a photo file into pixels, within hard limits, so a huge or
//! hostile file is turned away before it can use much memory.

use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// Largest file accepted.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Most pixels accepted: 100 megapixels, more than any phone camera.
pub const MAX_PIXELS: u64 = 100_000_000;

/// Longest side accepted, so a 1 by 100-million-pixel strip is refused
/// too.
pub const MAX_SIDE: u32 = 40_000;

/// The photo formats accepted, as found from the file's contents rather
/// than its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    WebP,
}

/// A decoded photo.
#[derive(Debug)]
pub struct Decoded {
    pub image: DynamicImage,
    pub format: Format,
}

/// Why a photo was turned away. Each reads as a sentence for the user.
#[derive(Debug)]
pub enum DecodeError {
    /// The file could not be read.
    Io(std::io::Error),
    TooLarge {
        bytes: u64,
    },
    TooManyPixels {
        width: u32,
        height: u32,
    },
    /// Not a photo in a format the app reads.
    Unsupported,
    /// The file claims to be a photo but its data is broken.
    Damaged(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the file could not be read: {e}"),
            Self::TooLarge { bytes } => write!(
                f,
                "the file is {} MB; photos may be at most {} MB",
                bytes.div_ceil(1024 * 1024),
                MAX_FILE_BYTES / (1024 * 1024)
            ),
            Self::TooManyPixels { width, height } => write!(
                f,
                "the photo is {width} \u{d7} {height} pixels; photos may have at most {} \
                 megapixels and {MAX_SIDE} pixels on a side",
                MAX_PIXELS / 1_000_000
            ),
            Self::Unsupported => f.write_str("this is not a JPEG, PNG, or WebP photo"),
            Self::Damaged(why) => write!(f, "the photo is damaged and cannot be read ({why})"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Reads the photo at `path`. The file is only read, never changed.
pub fn decode_file(path: &Path) -> Result<Decoded, DecodeError> {
    let file = File::open(path)?;
    let bytes = file.metadata()?.len();
    if bytes > MAX_FILE_BYTES {
        return Err(DecodeError::TooLarge { bytes });
    }
    // The size is checked again while reading, in case the file grows.
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    decode_bytes(&data)
}

/// Reads a photo held in memory.
pub fn decode_bytes(data: &[u8]) -> Result<Decoded, DecodeError> {
    let bytes = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if bytes > MAX_FILE_BYTES {
        return Err(DecodeError::TooLarge { bytes });
    }
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(DecodeError::Io)?;
    let format = match reader.format() {
        Some(ImageFormat::Jpeg) => Format::Jpeg,
        Some(ImageFormat::Png) => Format::Png,
        Some(ImageFormat::WebP) => Format::WebP,
        _ => return Err(DecodeError::Unsupported),
    };

    // The header gives the size without decoding, so an oversized photo
    // is refused before any pixels are allocated.
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| DecodeError::Damaged(e.to_string()))?;
    check_size(width, height)?;
    let image = decode_limited(data, width, height)?;
    Ok(Decoded { image, format })
}

fn check_size(width: u32, height: u32) -> Result<(), DecodeError> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS || width > MAX_SIDE || height > MAX_SIDE {
        return Err(DecodeError::TooManyPixels { width, height });
    }
    Ok(())
}

/// Decodes with the allocation caps on, a second guard should a header
/// understate what decoding needs.
fn decode_limited(data: &[u8], width: u32, height: u32) -> Result<DynamicImage, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(DecodeError::Io)?;
    reader.limits(limits());
    reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::TooManyPixels { width, height },
        other => DecodeError::Damaged(other.to_string()),
    })
}

/// Caps on what decoding may allocate, matching the pixel limit: 100
/// megapixels of 16-bit RGBA, plus room for the decoder's own buffers.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_PIXELS * 8 + 64 * 1024 * 1024);
    limits
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::webp::WebPEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbImage};

    /// A small generated picture, so tests never need real photos.
    fn picture() -> RgbImage {
        RgbImage::from_fn(30, 20, |x, y| {
            image::Rgb([(x * 8) as u8, (y * 12) as u8, 128])
        })
    }

    fn encoded(format: ImageFormat) -> Vec<u8> {
        let pic = picture();
        let mut out = Vec::new();
        match format {
            ImageFormat::Jpeg => JpegEncoder::new_with_quality(&mut out, 90)
                .write_image(&pic, 30, 20, ExtendedColorType::Rgb8)
                .unwrap(),
            ImageFormat::WebP => WebPEncoder::new_lossless(&mut out)
                .write_image(&pic, 30, 20, ExtendedColorType::Rgb8)
                .unwrap(),
            _ => pic
                .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
                .unwrap(),
        }
        out
    }

    /// A PNG that claims `width` × `height` pixels, with correct
    /// checksums but no picture data.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &b in bytes {
                crc ^= u32::from(b);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let mut chunk = b"IHDR".to_vec();
        chunk.extend(width.to_be_bytes());
        chunk.extend(height.to_be_bytes());
        chunk.extend([8, 2, 0, 0, 0]);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(13u32.to_be_bytes());
        png.extend(&chunk);
        png.extend(crc32(&chunk).to_be_bytes());
        // An empty data chunk: readers look for one before giving the size.
        png.extend(0u32.to_be_bytes());
        png.extend(b"IDAT");
        png.extend(crc32(b"IDAT").to_be_bytes());
        png
    }

    #[test]
    fn jpeg_png_and_webp_decode() {
        for (format, expected) in [
            (ImageFormat::Jpeg, Format::Jpeg),
            (ImageFormat::Png, Format::Png),
            (ImageFormat::WebP, Format::WebP),
        ] {
            let decoded = decode_bytes(&encoded(format)).unwrap();
            assert_eq!(decoded.format, expected);
            assert_eq!((decoded.image.width(), decoded.image.height()), (30, 20));
        }
    }

    #[test]
    fn other_files_are_not_photos() {
        for data in [
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".as_slice(),
            b"just some text",
            b"",
        ] {
            assert!(matches!(decode_bytes(data), Err(DecodeError::Unsupported)));
        }
    }

    #[test]
    fn huge_pictures_are_refused_before_decoding() {
        for (width, height) in [(20_000, 20_000), (40_001, 1), (1, 40_001)] {
            match decode_bytes(&png_header(width, height)) {
                Err(DecodeError::TooManyPixels {
                    width: w,
                    height: h,
                }) => {
                    assert_eq!((w, h), (width, height));
                }
                other => panic!("{width}x{height}: {other:?}"),
            }
        }
        // Exactly at the limit is let through to decoding, which then
        // finds no picture data.
        assert!(matches!(
            decode_bytes(&png_header(10_000, 10_000)),
            Err(DecodeError::Damaged(_))
        ));
    }

    #[test]
    fn size_limits_hold_at_their_edges() {
        assert!(check_size(10_000, 10_000).is_ok());
        assert!(check_size(10_000, 10_001).is_err());
        assert!(check_size(40_000, 2_500).is_ok());
        assert!(check_size(40_001, 1).is_err());
        assert!(check_size(1, 40_001).is_err());
    }

    #[test]
    fn decoding_caps_allow_the_largest_accepted_photo() {
        // 100 megapixels of 16-bit RGBA must fit, or a legitimate photo
        // at the limit would be refused; the library's own default cap
        // is lower than that.
        let caps = limits();
        assert!(caps.max_alloc.unwrap() >= MAX_PIXELS * 8);
        assert_eq!(caps.max_image_width, Some(MAX_SIDE));
        assert_eq!(caps.max_image_height, Some(MAX_SIDE));
    }

    #[test]
    fn decoding_has_its_own_allocation_cap() {
        // Even past the header check, a picture too big to hold is
        // refused rather than allocated.
        assert!(matches!(
            decode_limited(&png_header(30_000, 30_000), 30_000, 30_000),
            Err(DecodeError::TooManyPixels { .. })
        ));
    }

    #[test]
    fn broken_photos_are_damaged() {
        let mut jpeg = encoded(ImageFormat::Jpeg);
        jpeg.truncate(jpeg.len() / 3);
        assert!(matches!(decode_bytes(&jpeg), Err(DecodeError::Damaged(_))));
        let mut png = encoded(ImageFormat::Png);
        png.truncate(60);
        assert!(matches!(decode_bytes(&png), Err(DecodeError::Damaged(_))));
    }

    #[test]
    fn big_files_are_refused_unread() {
        let too_big = usize::try_from(MAX_FILE_BYTES + 1).unwrap();
        assert!(matches!(
            decode_bytes(&vec![0; too_big]),
            Err(DecodeError::TooLarge { .. })
        ));
        let dir = std::env::temp_dir().join("mylongbox-media-test-big");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.jpg");
        // A sparse file: large on paper, no disk used.
        File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let err = decode_file(&path).unwrap_err();
        assert!(matches!(err, DecodeError::TooLarge { .. }));
        assert_eq!(
            err.to_string(),
            "the file is 51 MB; photos may be at most 50 MB"
        );
        let good = dir.join("good.png");
        std::fs::write(&good, encoded(ImageFormat::Png)).unwrap();
        assert_eq!(decode_file(&good).unwrap().format, Format::Png);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
