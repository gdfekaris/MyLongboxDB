//! Image import pipeline: decode within limits, orient, convert color,
//! resize, and re-encode so no source metadata survives.

#![forbid(unsafe_code)]

mod decode;
#[cfg(test)]
mod fixtures;
#[cfg(feature = "heic")]
mod heic;

pub use decode::{
    DecodeError, Decoded, Format, MAX_FILE_BYTES, MAX_PIXELS, MAX_SIDE, decode_bytes, decode_file,
};
