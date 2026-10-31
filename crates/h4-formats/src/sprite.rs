//! Paletted sprites.
//!
//! The basic unit is a layer (`t_bitmap_layer_24`; reader at rva 0x195570 in the Russian 3.0
//! exe):
//!
//! ```text
//! u16 palette size, u16 has_block_alpha, palette (B, G, R) * size
//! u16-prefixed name, u8 alpha depth, i32 left, top, right, bottom
//! (bottom - top) rows of: u16 x0, u16 x1 (relative to left), u32 offset into pixels
//! N bytes of palette indices, N = last row's offset + width
//! if alpha depth >= 4: (N + 1) / 2 bytes of 4-bit alpha, even pixel in the low nibble
//! if alpha depth == 1: no alpha bytes; palette index 0 is transparent (a colour key)
//!     and if has_block_alpha: (N + 131) / 128 more bytes (per-128-pixel flags; unused here)
//! ```
//!
//! A bitmap group (`layers.*`) is a u16 count followed by layers. An actor sequence
//! (`actor_sequence.*`) is u16 2, then a `t_image_sequence` (reader rva 0x349930): u16 version
//! (2, or 3 followed by u16 mode, i16 base frame, i16 base shadow), u16 count, then the frames and
//! their shadows as layers, then i32 x, i32 y (see [`ActorSequence::offset`]), a u16-prefixed string in version 1 only,
//! and in `actor_sequence` files a final u16 (unknown).
//! The three version-3 fields are recomputed by the game from layer names after reading
//! (rva 0x3490c0): mode 2 means there are `base frame` / `base shadow` layers drawn under every
//! animation frame, as in animated buildings.

use crate::reader::{bad, Reader};
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            left: self.left.min(o.left),
            top: self.top.min(o.top),
            right: self.right.max(o.right),
            bottom: self.bottom.max(o.bottom),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Row {
    x0: u16,
    x1: u16,
    offset: u32,
}

impl Row {
    /// Number of pixels stored for this row.
    fn len(&self) -> usize {
        (self.x1 as usize).saturating_sub(self.x0 as usize)
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub rect: Rect,
    /// RGB (already swapped from the file's BGR).
    palette: Vec<[u8; 3]>,
    rows: Vec<Row>,
    pixels: Vec<u8>,
    /// 4-bit alpha per pixel, two pixels per byte; empty means opaque.
    alpha: Vec<u8>,
    /// 1-bit layers have no alpha bytes: pixels with palette index 0 are see-through.
    /// (Found on the data: in the animated flags of `Quest Guard` 955 of 1286 pixels are
    /// index 0, a near-black that the game obviously doesn't draw.)
    index0_transparent: bool,
}

impl Layer {
    pub(crate) fn read(r: &mut Reader) -> Result<Layer> {
        let palette_size = r.u16()? as usize;
        let has_block_alpha = r.u16()? != 0;
        let palette = r.take(3 * palette_size)?.as_chunks::<3>().0.iter().map(|&[blue, green, red]| [red, green, blue]).collect();
        let name = r.str16()?;
        let alpha_depth = r.u8()?;
        let rect = Rect { left: r.i32()?, top: r.i32()?, right: r.i32()?, bottom: r.i32()? };
        // Check the size in i64: garbage coordinates near i32::MIN/MAX would overflow the
        // subtraction in `Rect::width`/`height` (a panic in debug builds).
        let wide_width = rect.right as i64 - rect.left as i64;
        let wide_height = rect.bottom as i64 - rect.top as i64;
        if !(-4096..=4096).contains(&wide_width) || !(-4096..=4096).contains(&wide_height) {
            let short: String = name.chars().take(32).collect();
            return bad(format!("layer {short:?} has an implausible size {rect:?}"));
        }
        let height = wide_height.max(0) as usize;
        let mut rows = Vec::with_capacity(height);
        for _ in 0..height {
            rows.push(Row { x0: r.u16()?, x1: r.u16()?, offset: r.u32()? });
        }
        let n = rows.last().map_or(0, |l| l.offset as usize + l.len());
        // Every row must lie inside the pixel data, or drawing would index past its end.
        // (The file only stores the total implicitly, through the last row.)
        if let Some(y) = rows.iter().position(|row| row.offset as usize + row.len() > n) {
            return bad(format!("layer {name:?}: row {y} points past the {n} bytes of pixel data"));
        }
        let pixels = r.take(n)?.to_vec();
        let mut alpha = Vec::new();
        if alpha_depth >= 4 && n > 0 {
            alpha = r.take(n.div_ceil(2))?.to_vec();
            if has_block_alpha {
                r.take((n + 131) / 128)?;
            }
        }
        let index0_transparent = alpha_depth == 1;
        Ok(Layer { name, rect, palette, rows, pixels, alpha, index0_transparent })
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Alpha-blend this layer over an RGBA8 `canvas` that covers `area` in layer coordinates.
    ///
    /// `Layer::read` has already checked that every row lies inside the pixel data. Pixels
    /// that would land outside `area` (or outside a canvas too small for `area`) are skipped,
    /// so drawing never panics.
    pub fn draw_onto(&self, canvas: &mut [u8], area: Rect) {
        // i64 so that positions far outside the area can't overflow
        let (area_w, area_h) = (area.width() as i64, area.height() as i64);
        if area_w <= 0 || area_h <= 0 {
            return;
        }
        for (y, row) in self.rows.iter().enumerate() {
            let cy = self.rect.top as i64 + y as i64 - area.top as i64;
            if cy < 0 || cy >= area_h {
                continue;
            }
            for i in 0..row.len() {
                let cx = self.rect.left as i64 + row.x0 as i64 + i as i64 - area.left as i64;
                if cx < 0 || cx >= area_w {
                    continue;
                }
                let k = row.offset as usize + i;
                let a = if self.index0_transparent && self.pixels[k] == 0 {
                    0
                } else if self.alpha.is_empty() {
                    255
                } else {
                    ((self.alpha[k / 2] >> (4 * (k & 1))) & 15) as u32 * 17
                };
                if a == 0 {
                    continue;
                }
                let [r, g, b] = self.palette.get(self.pixels[k] as usize).copied().unwrap_or([255, 0, 255]);
                let p = 4 * (cy * area_w + cx) as usize;
                let Some(dst) = canvas.get_mut(p..p + 4) else { continue };
                // "over" compositing on straight (non-premultiplied) alpha
                let da = dst[3] as u32;
                let out_a = a + da * (255 - a) / 255;
                if out_a == 0 {
                    continue;
                }
                for (c, s) in [r, g, b].into_iter().enumerate() {
                    dst[c] = ((s as u32 * a + dst[c] as u32 * da * (255 - a) / 255) / out_a) as u8;
                }
                dst[3] = out_a as u8;
            }
        }
    }

    /// This layer alone as RGBA8, sized to its own rect.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 4 * (self.rect.width() * self.rect.height()).max(0) as usize];
        self.draw_onto(&mut buf, self.rect);
        buf
    }
}

/// `layers.*`: named images such as the states of a button.
pub fn read_bitmap_group(data: &[u8]) -> Result<Vec<Layer>> {
    let mut r = Reader::new(data);
    let n = r.u16()?;
    (0..n).map(|_| Layer::read(&mut r)).collect()
}

/// `actor_sequence.*`: one animation of a creature or hero in one direction.
#[derive(Debug, Clone)]
pub struct ActorSequence {
    pub frames: Vec<Layer>,
    /// Shadow for each frame, when present (layers named `shadow NNN`).
    pub shadows: Vec<Layer>,
    /// `base shadow` and `base frame` (in that order): the still part of an animated object,
    /// drawn under every frame. Empty for creatures and heroes.
    pub base: Vec<Layer>,
    /// Added to layer coordinates to put the actor's feet at (0, 0). Read from the two i32s
    /// after the layers (they are about minus the middle-bottom of the frames).
    pub offset: (i32, i32),
}

impl ActorSequence {
    pub fn read(data: &[u8]) -> Result<ActorSequence> {
        Self::read_from(&mut Reader::new(data))
    }

    /// Read a sequence that starts at the cursor. Adventure objects embed the same structure
    /// inside their own files (see `adv_object`), so the reader works on a shared cursor.
    pub(crate) fn read_from(r: &mut Reader) -> Result<ActorSequence> {
        // outer u16: 2 in actor_sequence files, 1 inside adventure objects; layout is the same
        r.u16()?;
        Self::read_image_from(r)
    }

    /// Image sequence without a resource wrapper, embedded in combat objects.
    pub(crate) fn read_image_from(r: &mut Reader) -> Result<ActorSequence> {
        let version = r.u16()?;
        match version {
            0..=2 => {}
            // version 3 adds mode, base frame and base shadow; the game derives them again
            // from the layer names, so we skip them too
            3 => {
                r.take(6)?;
            }
            v => return bad(format!("unknown actor sequence version {v}")),
        }
        let n = r.u16()?;
        let mut s = ActorSequence { frames: Vec::new(), shadows: Vec::new(), base: Vec::new(), offset: (0, 0) };
        for _ in 0..n {
            let l = Layer::read(r)?;
            let name = l.name.to_lowercase();
            if name.starts_with("base") {
                s.base.push(l);
            } else if name.starts_with("shadow") {
                s.shadows.push(l);
            } else {
                s.frames.push(l);
            }
        }
        // shadow before frame, so `base` can be drawn in order
        s.base.sort_by_key(|l| !l.name.to_lowercase().contains("shadow"));
        s.offset = (r.i32()?, r.i32()?);
        if version == 1 {
            // version 1 also stores a name here (seen empty)
            r.str16()?;
        }
        Ok(s)
    }

    /// Bounding box of all frames and shadows, so every frame can share one canvas.
    pub fn bounds(&self) -> Option<Rect> {
        self.frames
            .iter()
            .chain(&self.shadows)
            .chain(&self.base)
            .filter(|l| !l.is_empty())
            .map(|l| l.rect)
            .reduce(|a, b| a.union(&b))
    }

    /// Frame `i` with its shadow underneath, as RGBA8 over `area` (usually [`Self::bounds`]).
    pub fn compose(&self, i: usize, area: Rect, with_shadow: bool) -> Vec<u8> {
        let mut buf = vec![0u8; 4 * (area.width() * area.height()).max(0) as usize];
        for b in &self.base {
            if with_shadow || !b.name.to_lowercase().contains("shadow") {
                b.draw_onto(&mut buf, area);
            }
        }
        if with_shadow {
            if let Some(s) = self.shadows.get(i) {
                s.draw_onto(&mut buf, area);
            }
        }
        if let Some(f) = self.frames.get(i) {
            f.draw_onto(&mut buf, area);
        }
        buf
    }
}
