//! Raised ground: the map's vertex heights lift the terrain, objects and armies, as the game
//! draws them (`decomp/heights.md`).
//!
//! The game draws a tile with each corner raised by its vertex's height, one pixel per unit.
//! Every pixel column of the tile is stretched between the raised edges: in the left half
//! between the edges left→top and left→bottom, in the right half between top→right and
//! bottom→right. That is the same as cutting the diamond along its vertical diagonal into
//! two triangles and moving their corners up. Here the flat picture of the ground is warped
//! that way once, when it is built.

use h4_formats::map::MapFile;
use h4_formats::terrain::{column_height, TILE_HEIGHT, TILE_WIDTH};
use macroquad::prelude::*;

use crate::terrain_view::Iso;
use crate::world::Pos;

/// Vertex heights of one level.
pub struct Relief {
    size: i32,
    /// `(size + 1)²` vertices, row by row; 0 where the map has none (outside its diamond).
    heights: Vec<i32>,
}

/// Heights of a tile's corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Relief {
    /// Flat ground, for the generated map.
    pub fn flat(size: i32) -> Relief {
        Relief { size, heights: vec![0; ((size + 1) * (size + 1)) as usize] }
    }

    /// The heights of `level` of a map.
    pub fn from_map(map: &MapFile, level: usize) -> Relief {
        let size = map.header.size as i32;
        let mut relief = Relief::flat(size);
        for row in 0..=size {
            for col in 0..=size {
                if let Some(h) = map.height(level, row as usize, col as usize) {
                    relief.heights[(row * (size + 1) + col) as usize] = h as i32;
                }
            }
        }
        relief
    }

    /// The highest vertex: how much room the picture needs above the map.
    pub fn max(&self) -> i32 {
        self.heights.iter().copied().max().unwrap_or(0)
    }

    /// Height of vertex (`row`, `col`), the top corner of tile (`row`, `col`); 0 off the map.
    pub fn vertex(&self, row: i32, col: i32) -> i32 {
        if row < 0 || col < 0 || row > self.size || col > self.size {
            return 0;
        }
        self.heights[(row * (self.size + 1) + col) as usize]
    }

    pub fn corners(&self, p: Pos) -> Corners {
        Corners {
            top: self.vertex(p.row, p.col) as f32,
            right: self.vertex(p.row, p.col + 1) as f32,
            bottom: self.vertex(p.row + 1, p.col + 1) as f32,
            left: self.vertex(p.row + 1, p.col) as f32,
        }
    }

    /// How high an object stands whose footprint (`rows` × `cols` cells) starts at `at`:
    /// the rounded mean of the heights of all the footprint's vertices, as
    /// `t_abstract_adventure_map::compute_height` (rva `7940`) does it. (For some models the
    /// game only counts the vertices of some cells; that flag isn't read yet.)
    pub fn object_height(&self, at: Pos, rows: i32, cols: i32) -> i32 {
        let mut sum = 0;
        let mut count = 0;
        for row in at.row..=at.row + rows {
            for col in at.col..=at.col + cols {
                if (0..=self.size).contains(&row) && (0..=self.size).contains(&col) {
                    sum += self.vertex(row, col);
                    count += 1;
                }
            }
        }
        if count == 0 {
            return 0;
        }
        (sum + count / 2) / count
    }

    /// Height of the ground under flat map pixel `p`, interpolated inside the tile's two
    /// triangles. Used to lift armies, also between tiles while they walk.
    pub fn ground_at(&self, iso: &Iso, p: Vec2) -> f32 {
        let tile = iso.tile_at(p.x, p.y);
        let (x0, y0) = iso.tile_corner(tile);
        // u runs from the left corner (0) to the right one (2), v from the top (0) to the bottom (1)
        let u = ((p.x - x0 as f32) / 32.0).clamp(0.0, 2.0);
        let v = ((p.y - y0 as f32) / 32.0).clamp(0.0, 1.0);
        let c = self.corners(tile);
        // In each triangle the point is side + a·(top − side) + b·(bottom − side), where
        // side is the left or right corner; a and b come from solving that for (u, v).
        let (side, across) = if u < 1.0 { (c.left, u) } else { (c.right, 2.0 - u) };
        let a = (across - (2.0 * v - 1.0)) / 2.0;
        let b = (across + (2.0 * v - 1.0)) / 2.0;
        side + a * (c.top - side) + b * (c.bottom - side)
    }

    /// Screen corners of a tile's raised diamond: top, right, bottom, left.
    pub fn diamond(&self, iso: &Iso, p: Pos) -> [Vec2; 4] {
        let center = iso.tile_center(p);
        let c = self.corners(p);
        [
            center + vec2(0.0, -16.0 - c.top),
            center + vec2(32.0, -c.right),
            center + vec2(0.0, 16.0 - c.bottom),
            center + vec2(-32.0, -c.left),
        ]
    }

    /// The tile under map pixel `p` with the ground raised: the frontmost tile whose raised
    /// diamond holds the point. Raised ground shows above its flat place, so the tiles to
    /// look at lie at and below the flat tile under the point.
    pub fn pick(&self, iso: &Iso, p: Vec2) -> Pos {
        let flat = iso.tile_at(p.x, p.y);
        // one step down the screen is one more row and one more column: 32 pixels
        let reach = self.max() / 32 + 2;
        let mut best: Option<Pos> = None;
        for dr in -1..=reach {
            for dc in -1..=reach {
                let t = Pos { row: flat.row + dr, col: flat.col + dc };
                let inside = {
                    let [top, right, bottom, left] = self.diamond(iso, t);
                    in_triangle(p, left, top, bottom) || in_triangle(p, top, right, bottom)
                };
                if inside && best.is_none_or(|b| t.row + t.col > b.row + b.col) {
                    best = Some(t);
                }
            }
        }
        best.unwrap_or(flat)
    }

    /// Warp the flat picture of the ground (`canvas`, RGBA, `width` pixels wide, laid out
    /// by `iso`, which must leave room for the highest vertex above the map) so every tile's
    /// corners are raised by their heights.
    ///
    /// Tiles go from the back of the map to the front, so a raised tile covers the ones behind
    /// it. This works in place: ground only moves up, onto tiles already done, and each
    /// column's pixels are taken out before they are put back raised.
    pub fn raise(&self, canvas: &mut [u8], width: usize, iso: &Iso) {
        let mut column = Vec::with_capacity(4 * TILE_HEIGHT);
        // tiles with the same row + col are side by side on screen; smaller sums are behind
        for sum in 0..2 * self.size - 1 {
            for row in 0..self.size {
                let col = sum - row;
                if !(0..self.size).contains(&col) {
                    continue;
                }
                let p = Pos { row, col };
                let c = self.corners(p);
                if c == (Corners { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 }) {
                    continue; // flat: stays where it is
                }
                let (x0, y0) = iso.tile_corner(p);
                for px in 0..TILE_WIDTH {
                    let x = x0 + px as i32;
                    if x < 0 || x as usize >= width {
                        continue;
                    }
                    // the column in the flat picture, as the terrain files cut it
                    let h = column_height(px);
                    let top = y0 + ((TILE_HEIGHT - h) / 2) as i32;
                    // how far its upper and lower ends are raised, at the column's centre
                    let t = (px as f32 + 0.5) / 32.0;
                    let (lift_top, lift_bottom) = if t < 1.0 {
                        (c.left + (c.top - c.left) * t, c.left + (c.bottom - c.left) * t)
                    } else {
                        let t = t - 1.0;
                        (c.top + (c.right - c.top) * t, c.bottom + (c.right - c.bottom) * t)
                    };
                    let to_top = top as f32 - lift_top;
                    let to_bottom = (top + h as i32) as f32 - lift_bottom;

                    // take the column out
                    column.clear();
                    for y in top..top + h as i32 {
                        let i = 4 * (y as usize * width + x as usize);
                        if let Some(px) = canvas.get_mut(i..i + 4) {
                            column.extend_from_slice(px);
                            px.fill(0);
                        }
                    }
                    if column.len() != 4 * h || to_bottom <= to_top {
                        continue; // cut off by the canvas, or a slope facing away: hidden
                    }
                    // and put it back stretched between the raised ends
                    let (from, to) = (to_top.round() as i32, to_bottom.round() as i32);
                    for y in from.max(0)..to {
                        let along = (y as f32 + 0.5 - to_top) / (to_bottom - to_top);
                        let source = ((along * h as f32) as usize).min(h - 1);
                        let i = 4 * (y as usize * width + x as usize);
                        if let Some(px) = canvas.get_mut(i..i + 4) {
                            px.copy_from_slice(&column[4 * source..4 * source + 4]);
                        }
                    }
                }
            }
        }
    }
}

/// Whether `p` is inside triangle `a`, `b`, `c` (either winding).
fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let side = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hill() -> Relief {
        let mut r = Relief::flat(4);
        r.heights[(2 * 5 + 2) as usize] = 20; // vertex (2, 2)
        r
    }

    #[test]
    fn object_height_is_the_rounded_mean() {
        let r = hill();
        // a 1×1 object on tile (1, 1): its bottom corner is the 20
        assert_eq!(r.object_height(Pos { row: 1, col: 1 }, 1, 1), 5);
        // 2×2 around the hill: 9 vertices, 20 / 9 rounds to 2
        assert_eq!(r.object_height(Pos { row: 1, col: 1 }, 2, 2), 2);
    }

    #[test]
    fn ground_follows_the_corners() {
        let r = hill();
        let iso = Iso::new(4, 4, 32);
        // the raised vertex is the bottom corner of tile (1, 1) and the top of (2, 2)
        let top_of = iso.tile_center(Pos { row: 2, col: 2 }) - vec2(0.0, 16.0);
        assert!((r.ground_at(&iso, top_of + vec2(0.0, 0.01)) - 20.0).abs() < 0.1);
        // halfway down tile (2, 2)'s middle the height has halved
        assert!((r.ground_at(&iso, iso.tile_center(Pos { row: 2, col: 2 })) - 10.0).abs() < 0.1);
        // far away the ground is flat
        assert_eq!(r.ground_at(&iso, iso.tile_center(Pos { row: 0, col: 3 })), 0.0);
    }

    #[test]
    fn picking_finds_the_raised_tile() {
        let r = hill();
        let iso = Iso::new(4, 4, 32);
        // the raised top corner of tile (2, 2) is 20 pixels above its flat place, inside the
        // flat tile (1, 1); a point just below it belongs to (2, 2)
        let [top, ..] = r.diamond(&iso, Pos { row: 2, col: 2 });
        assert_eq!(r.pick(&iso, top + vec2(0.0, 2.0)), Pos { row: 2, col: 2 });
    }
}
