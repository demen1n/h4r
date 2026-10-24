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
//!     and if has_block_alpha: (N + 131) / 128 more bytes (per-128-pixel flags; unused here)
//! ```
//!
//! A bitmap group (`layers.*`) is a u16 count followed by layers. An actor sequence
//! (`actor_sequence.*`) is u16 2, u16 version (2, or 3 followed by u16 k and k i32s), u16 count,
//! then the frames and their shadows as layers, then i32 x, i32 y (see [`ActorSequence::offset`])
//! and u16 (unknown).

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
}

impl Layer {
    fn read(r: &mut Reader) -> Result<Layer> {
        let palette_size = r.u16()? as usize;
        let has_block_alpha = r.u16()? != 0;
        let palette = r.take(3 * palette_size)?.chunks_exact(3).map(|c| [c[2], c[1], c[0]]).collect();
        let name = r.str16()?;
        let alpha_depth = r.u8()?;
        let rect = Rect { left: r.i32()?, top: r.i32()?, right: r.i32()?, bottom: r.i32()? };
        let height = rect.height().max(0) as usize;
        if height > 4096 || rect.width() > 4096 {
            return bad(format!("layer {name:?} has an implausible size {rect:?}"));
        }
        let mut rows = Vec::with_capacity(height);
        for _ in 0..height {
            rows.push(Row { x0: r.u16()?, x1: r.u16()?, offset: r.u32()? });
        }
        let n = rows.last().map_or(0, |l| l.offset as usize + (l.x1 as usize).saturating_sub(l.x0 as usize));
        let pixels = r.take(n)?.to_vec();
        let mut alpha = Vec::new();
        if alpha_depth >= 4 && n > 0 {
            alpha = r.take((n + 1) / 2)?.to_vec();
            if has_block_alpha {
                r.take((n + 131) / 128)?;
            }
        }
        Ok(Layer { name, rect, palette, rows, pixels, alpha })
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Alpha-blend this layer over an RGBA8 `canvas` that covers `area` in layer coordinates.
    pub fn draw_onto(&self, canvas: &mut [u8], area: Rect) {
        let width = area.width() as usize;
        for (y, row) in self.rows.iter().enumerate() {
            let cy = self.rect.top + y as i32 - area.top;
            if cy < 0 || cy >= area.height() {
                continue;
            }
            for i in 0..row.x1.saturating_sub(row.x0) as usize {
                let cx = self.rect.left + row.x0 as i32 + i as i32 - area.left;
                if cx < 0 || cx >= area.width() {
                    continue;
                }
                let k = row.offset as usize + i;
                let a = if self.alpha.is_empty() {
                    255
                } else {
                    ((self.alpha[k / 2] >> (4 * (k & 1))) & 15) as u32 * 17
                };
                if a == 0 {
                    continue;
                }
                let [r, g, b] = self.palette.get(self.pixels[k] as usize).copied().unwrap_or([255, 0, 255]);
                let p = 4 * (cy as usize * width + cx as usize);
                let dst = &mut canvas[p..p + 4];
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
    /// Added to layer coordinates to put the actor's feet at (0, 0). Read from the two i32s
    /// after the layers (they are about minus the middle-bottom of the frames).
    pub offset: (i32, i32),
}

impl ActorSequence {
    pub fn read(data: &[u8]) -> Result<ActorSequence> {
        let mut r = Reader::at(data, 2);
        match r.u16()? {
            2 => {}
            // version 3 adds a list before the layers; seen only as [1 item: -1]
            3 => {
                let k = r.u16()? as usize;
                r.take(4 * k)?;
            }
            v => return bad(format!("unknown actor sequence version {v}")),
        }
        let n = r.u16()?;
        let mut frames = Vec::new();
        let mut shadows = Vec::new();
        for _ in 0..n {
            let l = Layer::read(&mut r)?;
            if l.name.to_lowercase().starts_with("shadow") {
                shadows.push(l);
            } else {
                frames.push(l);
            }
        }
        let offset = (r.i32()?, r.i32()?);
        Ok(ActorSequence { frames, shadows, offset })
    }

    /// Bounding box of all frames and shadows, so every frame can share one canvas.
    pub fn bounds(&self) -> Option<Rect> {
        self.frames
            .iter()
            .chain(&self.shadows)
            .filter(|l| !l.is_empty())
            .map(|l| l.rect)
            .reduce(|a, b| a.union(&b))
    }

    /// Frame `i` with its shadow underneath, as RGBA8 over `area` (usually [`Self::bounds`]).
    pub fn compose(&self, i: usize, area: Rect, with_shadow: bool) -> Vec<u8> {
        let mut buf = vec![0u8; 4 * (area.width() * area.height()).max(0) as usize];
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
