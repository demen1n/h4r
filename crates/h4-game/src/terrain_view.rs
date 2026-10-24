//! Terrain drawing. The whole map's ground is drawn once on the CPU into one big picture,
//! which is then cut into GPU textures of `CHUNK` pixels.
//!
//! How the game tiles terrain (worked out by experiment): metatiles of 10x10 tiles are laid
//! every 5 tiles in both directions, so each map tile is covered by four of them; their ragged
//! edges overlap and leave no holes. The two metatiles of a set alternate like a checkerboard.
//!
//! Where terrains meet, the higher-priority one is drawn over the edge of the lower tile
//! through a transition mask (see `h4_formats::transition`). Land over water uses the soft
//! 4-bit "land_to_water" masks, which makes the shoreline.

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::terrain::{Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};
use h4_formats::transition::{corner_code, Neighbours, Transitions};
use macroquad::prelude::*;

use crate::world::{Pos, Road, Terrain, World};

const STEP: i32 = 5;
const CHUNK: i32 = 1024;
const TILE_BYTES: usize = 4 * TILE_WIDTH * TILE_HEIGHT;

/// Screen geometry of the isometric map, in unscaled pixels.
pub struct Iso {
    /// Shift that puts the leftmost tile at x = 0.
    origin_x: i32,
    pub width: i32,
    pub height: i32,
}

impl Iso {
    pub fn new(world: &World) -> Iso {
        let origin_x = (world.height - 1) * TILE_WIDTH as i32 / 2;
        Iso {
            origin_x,
            width: (world.width + world.height) * TILE_WIDTH as i32 / 2,
            height: (world.width + world.height) * TILE_HEIGHT as i32 / 2 + TILE_HEIGHT as i32,
        }
    }

    /// Top-left corner of the tile's 64x32 box.
    pub fn tile_corner(&self, p: Pos) -> (i32, i32) {
        (
            self.origin_x + (p.col - p.row) * TILE_WIDTH as i32 / 2,
            (p.col + p.row) * TILE_HEIGHT as i32 / 2,
        )
    }

    /// Centre of the tile's diamond.
    pub fn tile_center(&self, p: Pos) -> Vec2 {
        let (x, y) = self.tile_corner(p);
        vec2((x + TILE_WIDTH as i32 / 2) as f32, (y + TILE_HEIGHT as i32 / 2) as f32)
    }

    /// The tile whose diamond contains map pixel (x, y).
    pub fn tile_at(&self, x: f32, y: f32) -> Pos {
        let u = (x - self.origin_x as f32 - TILE_WIDTH as f32 / 2.0) / (TILE_WIDTH as f32 / 2.0);
        let v = (y - TILE_HEIGHT as f32 / 2.0) / (TILE_HEIGHT as f32 / 2.0);
        Pos { col: ((u + v) / 2.0).round() as i32, row: ((v - u) / 2.0).round() as i32 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    /// terrain, texture set (0/1), which of the set's two metatiles (0/1)
    Terrain(Terrain, u8, u8),
    Road(Road, u8),
}

/// Metatiles loaded on first use.
struct Metatiles<'a> {
    res: &'a Resources,
    loaded: HashMap<Key, Metatile>,
}

impl Metatiles<'_> {
    fn get(&mut self, key: Key) -> Result<&Metatile, String> {
        if !self.loaded.contains_key(&key) {
            let name = match key {
                Key::Terrain(t, set, alt) => format!("terrain.{}.{}.{}.h4d", t.resource_name(), set + 1, alt + 1),
                Key::Road(r, alt) => format!("terrain.road.{}.{}.h4d", r.resource_name(), alt + 1),
            };
            let m = self.res.read(&name).and_then(|d| Metatile::read(&d)).map_err(|e| format!("{name}: {e}"))?;
            self.loaded.insert(key, m);
        }
        Ok(&self.loaded[&key])
    }

    /// One map tile of a terrain (or road) as a 64x32 RGBA picture, put together from the
    /// four metatiles that overlap at that spot. `key` picks the metatile for each of them.
    fn tile(&mut self, p: Pos, key: impl Fn(u8) -> Key) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; TILE_BYTES];
        // metatile anchors (multiples of STEP) whose 10x10 area contains this tile
        let anchors = |v: i32| {
            let a = v.div_euclid(STEP) * STEP;
            [a - STEP, a]
        };
        for row0 in anchors(p.row) {
            for col0 in anchors(p.col) {
                let alt = ((row0.div_euclid(STEP) + col0.div_euclid(STEP)).rem_euclid(2)) as u8;
                let (r, c) = ((p.row - row0) as usize, (p.col - col0) as usize);
                if r >= METATILE_SIZE || c >= METATILE_SIZE {
                    continue;
                }
                let m = self.get(key(alt))?;
                m.draw_tile(m.tile(r, c), &mut buf, TILE_WIDTH, 0, 0);
            }
        }
        Ok(buf)
    }
}

/// Alpha-blend a 64x32 tile picture onto the canvas at (x, y), scaled by `coverage(x, y)`.
fn blend(canvas: &mut [u8], width: usize, x: i32, y: i32, tile: &[u8], coverage: impl Fn(usize, usize) -> u8) {
    for ty in 0..TILE_HEIGHT {
        for tx in 0..TILE_WIDTH {
            let src = &tile[4 * (ty * TILE_WIDTH + tx)..][..4];
            let a = src[3] as u32 * coverage(tx, ty) as u32 / 255;
            if a == 0 {
                continue;
            }
            let (cx, cy) = (x + tx as i32, y + ty as i32);
            if cx < 0 || cy < 0 || cx as usize >= width {
                continue;
            }
            let o = 4 * (cy as usize * width + cx as usize);
            let Some(dst) = canvas.get_mut(o..o + 4) else { continue };
            for c in 0..3 {
                dst[c] = ((src[c] as u32 * a + dst[c] as u32 * (255 - a)) / 255) as u8;
            }
            dst[3] = dst[3].max(a as u8);
        }
    }
}

/// Terrain plus texture set, like the game's `t_full_terrain_type` (type + subtype): the two
/// sets of a terrain look different, so their border needs a transition too.
type FullType = (Terrain, u8);

fn full_type(world: &World, p: Pos) -> FullType {
    let t = world.tile(p);
    (t.terrain, t.set)
}

/// Drawing order at borders: by terrain priority, then set.
fn rank(t: FullType) -> (u8, u8) {
    (t.0.priority(), t.1)
}

/// Which neighbours of `p` have full terrain type `t`.
fn neighbours_with(world: &World, p: Pos, t: FullType) -> Neighbours {
    let has = |dc: i32, dr: i32| {
        let q = Pos { col: p.col + dc, row: p.row + dr };
        world.contains(q) && full_type(world, q) == t
    };
    // map column grows to the screen's lower right, row to the lower left
    Neighbours {
        n: has(-1, -1),
        ne: has(0, -1),
        e: has(1, -1),
        se: has(1, 0),
        s: has(1, 1),
        sw: has(0, 1),
        w: has(-1, 1),
        nw: has(-1, 0),
    }
}

/// A stable pseudo-random number for a tile, so the same map always looks the same.
fn tile_hash(p: Pos) -> u32 {
    let h = (p.col as u32).wrapping_mul(73856093) ^ (p.row as u32).wrapping_mul(19349663);
    (h ^ (h >> 13)).wrapping_mul(0x5bd1e995) >> 7
}

pub struct TerrainView {
    chunks: Vec<(Vec2, Texture2D)>,
}

impl TerrainView {
    pub fn build(res: &Resources, world: &World, iso: &Iso) -> Result<TerrainView, String> {
        let transitions = res
            .read("transition.Transitions.h4d")
            .and_then(|d| Transitions::read(&d))
            .map_err(|e| format!("transitions: {e}"))?;
        let mut metatiles = Metatiles { res, loaded: HashMap::new() };
        let width = iso.width as usize;
        let mut canvas = vec![0u8; 4 * width * iso.height as usize];

        for row in 0..world.height {
            for col in 0..world.width {
                let p = Pos { col, row };
                let here = *world.tile(p);
                let (x, y) = iso.tile_corner(p);

                // the tile's own terrain
                let base = metatiles.tile(p, |alt| Key::Terrain(here.terrain, here.set, alt))?;
                blend(&mut canvas, width, x, y, &base, |_, _| 255);

                // higher terrains around it spill over its edges, lowest first
                let mine = full_type(world, p);
                let mut over: Vec<FullType> = Vec::new();
                for dr in -1..=1 {
                    for dc in -1..=1 {
                        let q = Pos { col: col + dc, row: row + dr };
                        if world.contains(q) {
                            let t = full_type(world, q);
                            if rank(t) > rank(mine) && !over.contains(&t) {
                                over.push(t);
                            }
                        }
                    }
                }
                over.sort_by_key(|t| rank(*t));
                for t in over {
                    let code = corner_code(neighbours_with(world, p, t));
                    let set = match (here.terrain == Terrain::Water, t.0 == Terrain::Water) {
                        (true, true) => format!("water {}", 1 + tile_hash(p) % 2),
                        (true, false) => format!("land_to_water {}", 1 + tile_hash(p) % 3),
                        _ => format!("land {}", 1 + tile_hash(p) % 3),
                    };
                    let Some(mask) = transitions.mask(&set, code) else { continue };
                    let texture = metatiles.tile(p, |alt| Key::Terrain(t.0, t.1, alt))?;
                    blend(&mut canvas, width, x, y, &texture, |mx, my| mask.alpha(mx, my));
                }

                if let Some(road) = here.road {
                    let texture = metatiles.tile(p, |alt| Key::Road(road, alt))?;
                    blend(&mut canvas, width, x, y, &texture, |_, _| 255);
                }
            }
        }

        let mut chunks = Vec::new();
        for cy in (0..iso.height).step_by(CHUNK as usize) {
            for cx in (0..iso.width).step_by(CHUNK as usize) {
                let w = CHUNK.min(iso.width - cx) as usize;
                let h = CHUNK.min(iso.height - cy) as usize;
                let mut part = Vec::with_capacity(4 * w * h);
                for y in 0..h {
                    let start = 4 * ((cy as usize + y) * width + cx as usize);
                    part.extend_from_slice(&canvas[start..start + 4 * w]);
                }
                let t = Texture2D::from_rgba8(w as u16, h as u16, &part);
                t.set_filter(FilterMode::Linear);
                chunks.push((vec2(cx as f32, cy as f32), t));
            }
        }
        Ok(TerrainView { chunks })
    }

    pub fn draw(&self, camera: &crate::Camera) {
        for (pos, t) in &self.chunks {
            let size = t.size() * camera.zoom;
            let at = camera.to_screen(*pos);
            if at.x > screen_width() || at.y > screen_height() || at.x + size.x < 0.0 || at.y + size.y < 0.0 {
                continue;
            }
            draw_texture_ex(t, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
        }
    }
}
