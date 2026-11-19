//! Full-screen pictures without a palette, `bitmap_raw.*.h4d`: the main menu backgrounds
//! `bitmap_raw.menu.main.0800/1024/1280` (one per screen size; the base game, Gathering Storm
//! and Winds of War each have their own set).
//!
//! Layout (reader rva `199190`): four u32 — a value the game skips (1 in every file),
//! height, width and the size of the pixel data — then the pixels, 3 bytes each, row after
//! row from the top, in blue, green, red order (like the palettes of the paletted sprites).
//! The game reads as many bytes as the size field says into a buffer of
//! `width * height * 3`; in the game's files the two are always equal, and we require that.

use crate::reader::{bad, Reader, Result};

/// Bytes per pixel: blue, green, red.
pub const BYTES_PER_PIXEL: usize = 3;

/// A decoded `bitmap_raw` picture.
#[derive(Debug, Clone)]
pub struct BitmapRaw {
    /// The first u32 of the file. The game skips it; it is 1 everywhere.
    pub unknown: u32,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels as blue, green, red bytes, rows from the top.
    pub pixels: Vec<u8>,
}

impl BitmapRaw {
    /// Parse a `bitmap_raw.*.h4d` file. `name` is only used in error messages.
    pub fn read(data: &[u8], name: &str) -> Result<BitmapRaw> {
        let mut r = Reader::new(data);
        let unknown = r.u32()?;
        let height = r.u32()?;
        let width = r.u32()?;
        let size = r.u32()? as usize;

        // An empty picture can't be shown or saved; no game file has one.
        if width == 0 || height == 0 {
            return bad(format!("{name}: empty picture {width}x{height}"));
        }
        // The size the picture needs. `checked_mul` returns `None` instead of overflowing
        // when a damaged header holds huge numbers.
        let needed = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(BYTES_PER_PIXEL));
        if needed != Some(size) {
            return bad(format!("{name}: {width}x{height} picture but {size} bytes of pixels"));
        }
        if r.remaining() != size {
            return bad(format!("{name}: {size} bytes of pixels expected, {} in the file", r.remaining()));
        }
        let pixels = r.take(size)?.to_vec();
        Ok(BitmapRaw { unknown, width, height, pixels })
    }

    /// The picture as RGBA bytes (fully opaque), ready for a texture or [`crate::png`].
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() / BYTES_PER_PIXEL * 4);
        // `as_chunks` views the bytes as an array of 3-byte pixels, [blue, green, red]
        // (`.0` is that array; `.1` would be leftover bytes, and `read` made sure there are none).
        for &[blue, green, red] in self.pixels.as_chunks::<BYTES_PER_PIXEL>().0 {
            out.extend_from_slice(&[red, green, blue, 255]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a file the way the game stores it.
    fn encode(width: u32, height: u32, size: u32, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [1, height, width, size] {
            out.extend(v.to_le_bytes());
        }
        out.extend(pixels);
        out
    }

    #[test]
    fn reads_pixels_as_bgr() {
        // 2x1: a blue pixel, then a red one
        let data = encode(2, 1, 6, &[255, 0, 0, 0, 0, 255]);
        let b = BitmapRaw::read(&data, "test").unwrap();
        assert_eq!((b.width, b.height, b.unknown), (2, 1, 1));
        assert_eq!(b.to_rgba(), [0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn rejects_bad_sizes() {
        let pixels = [0u8; 6];
        assert!(BitmapRaw::read(&encode(2, 1, 6, &pixels), "test").is_ok());
        for len in 0..22 {
            let data = encode(2, 1, 6, &pixels);
            assert!(BitmapRaw::read(&data[..len], "test").is_err(), "cut at {len}");
        }
        assert!(BitmapRaw::read(&encode(2, 1, 5, &pixels[..5]), "test").is_err(), "size field");
        assert!(BitmapRaw::read(&encode(0, 1, 0, &[]), "test").is_err(), "zero width");
        assert!(BitmapRaw::read(&encode(2, 0, 0, &[]), "test").is_err(), "zero height");
        assert!(BitmapRaw::read(&encode(2, 1, 6, &[0; 7]), "test").is_err(), "trailing byte");
        let huge = encode(u32::MAX, u32::MAX, 6, &pixels);
        assert!(BitmapRaw::read(&huge, "test").is_err(), "overflowing size");
    }
}
