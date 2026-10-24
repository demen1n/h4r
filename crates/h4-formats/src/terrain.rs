//! Adventure map terrain: `terrain.<type>.<variant>.h4d` metatiles.
//!
//! A metatile is 10x10 isometric tiles sharing one palette (`t_metatile_texture_24`; reader at
//! rva 0x397640, tile reader `t_adventure_tile_texture::read` at 0x397430):
//!
//! ```text
//! 100 tiles, row-major (row i, column j), each:
//!     u32 n, n bytes of column data, 64 * u32 offset of each column in that data
//! u32 palette size, palette (B, G, R) * size
//! ```
//!
//! A tile is a 64x32 diamond drawn as 64 columns; column x is `COLUMN_HEIGHT[x]` pixels tall
//! and vertically centred. Column data alternates `u8 skip` (transparent pixels) and
//! `u8 count, count palette indices` until the column's height is reached. Tiles near the
//! metatile's corners are fully transparent, and edge tiles are ragged: the game layers
//! metatiles and blends them with transition masks.

use crate::reader::{bad, Reader};
use crate::sprite::Rect;
use crate::Result;

pub const TILE_WIDTH: usize = 64;
pub const TILE_HEIGHT: usize = 32;
pub const METATILE_SIZE: usize = 10;

/// Height of each of the 64 columns of a tile diamond: 1, 1, 3, 3, ..., 31, 31, 31, 31, ..., 1, 1.
pub fn column_height(x: usize) -> usize {
    let from_edge = x.min(TILE_WIDTH - 1 - x);
    2 * (from_edge / 2) + 1
}

#[derive(Debug, Clone)]
pub struct Tile {
    data: Vec<u8>,
    columns: [u32; TILE_WIDTH],
}

#[derive(Debug, Clone)]
pub struct Metatile {
    /// Row-major, `METATILE_SIZE` x `METATILE_SIZE`.
    pub tiles: Vec<Tile>,
    /// RGB (already swapped from the file's BGR).
    palette: Vec<[u8; 3]>,
}

impl Metatile {
    pub fn read(data: &[u8]) -> Result<Metatile> {
        let mut r = Reader::new(data);
        let mut tiles = Vec::with_capacity(METATILE_SIZE * METATILE_SIZE);
        for _ in 0..METATILE_SIZE * METATILE_SIZE {
            let n = r.u32()? as usize;
            let data = r.take(n)?.to_vec();
            let mut columns = [0u32; TILE_WIDTH];
            for c in columns.iter_mut() {
                *c = r.u32()?;
            }
            tiles.push(Tile { data, columns });
        }
        let palette_size = r.u32()? as usize;
        let palette = r.take(3 * palette_size)?.chunks_exact(3).map(|c| [c[2], c[1], c[0]]).collect();
        if r.remaining() != 0 {
            return bad(format!("{} bytes left after metatile", r.remaining()));
        }
        Ok(Metatile { tiles, palette })
    }

    pub fn tile(&self, row: usize, col: usize) -> &Tile {
        &self.tiles[row * METATILE_SIZE + col]
    }

    /// Draw one tile with its top-left corner at (`x`, `y`) of an RGBA8 canvas `width` pixels
    /// wide. Transparent pixels leave the canvas untouched.
    pub fn draw_tile(&self, tile: &Tile, canvas: &mut [u8], width: usize, x: i32, y: i32) {
        let height = canvas.len() / 4 / width;
        for cx in 0..TILE_WIDTH {
            let h = column_height(cx);
            let top = (TILE_HEIGHT - h) / 2;
            let mut p = tile.columns[cx] as usize;
            let mut cy = 0;
            while cy < h {
                cy += tile.data[p] as usize;
                p += 1;
                if cy >= h {
                    break;
                }
                let count = tile.data[p] as usize;
                p += 1;
                for k in 0..count {
                    let px = x + cx as i32;
                    let py = y + (top + cy + k) as i32;
                    if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                        let [r, g, b] = self.palette[tile.data[p + k] as usize];
                        let o = 4 * (py as usize * width + px as usize);
                        canvas[o..o + 4].copy_from_slice(&[r, g, b, 255]);
                    }
                }
                p += count;
                cy += count;
            }
        }
    }

    /// Where tile (row, col) goes in an isometric picture of the whole metatile.
    pub fn tile_position(row: usize, col: usize) -> (i32, i32) {
        let half_w = TILE_WIDTH as i32 / 2;
        let half_h = TILE_HEIGHT as i32 / 2;
        ((col as i32 - row as i32) * half_w, (row + col) as i32 * half_h)
    }

    /// The whole metatile as one isometric RGBA8 picture.
    pub fn to_rgba(&self) -> (Rect, Vec<u8>) {
        let n = METATILE_SIZE as i32;
        let area = Rect {
            left: -(n - 1) * TILE_WIDTH as i32 / 2,
            top: 0,
            right: (n + 1) * TILE_WIDTH as i32 / 2,
            bottom: (n + 1) * TILE_HEIGHT as i32,
        };
        let width = area.width() as usize;
        let mut buf = vec![0u8; 4 * width * area.height() as usize];
        for row in 0..METATILE_SIZE {
            for col in 0..METATILE_SIZE {
                let (x, y) = Self::tile_position(row, col);
                self.draw_tile(self.tile(row, col), &mut buf, width, x - area.left, y - area.top);
            }
        }
        (area, buf)
    }
}
