//! Terrain drawing. The whole map's ground is drawn once on the CPU into one big picture,
//! which is then cut into GPU textures of `CHUNK` pixels.
//!
//! How the game tiles terrain (worked out by experiment): metatiles of 10x10 tiles are laid
//! every 5 tiles in both directions, so each map tile is covered by four of them; their ragged
//! edges overlap and leave no holes. The two metatiles of a set alternate like a checkerboard.

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::terrain::{Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};
use macroquad::prelude::*;

use crate::world::{Pos, Road, Terrain, World};

const STEP: i32 = 5;
const CHUNK: i32 = 1024;

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

pub struct TerrainView {
    chunks: Vec<(Vec2, Texture2D)>,
}

impl TerrainView {
    pub fn build(res: &Resources, world: &World, iso: &Iso) -> Result<TerrainView, String> {
        let metatiles = load_metatiles(res, world)?;
        let width = iso.width as usize;
        let mut canvas = vec![0u8; 4 * width * iso.height as usize];

        // terrain, then roads on top
        for layer in 0..2 {
            let mut row0 = -(METATILE_SIZE as i32);
            while row0 < world.height {
                let mut col0 = -(METATILE_SIZE as i32);
                while col0 < world.width {
                    let alt = ((row0.div_euclid(STEP) + col0.div_euclid(STEP)).rem_euclid(2)) as u8;
                    for r in 0..METATILE_SIZE {
                        for c in 0..METATILE_SIZE {
                            let p = Pos { col: col0 + c as i32, row: row0 + r as i32 };
                            if !world.contains(p) {
                                continue;
                            }
                            let t = world.tile(p);
                            let key = match (layer, t.road) {
                                (0, _) => Key::Terrain(t.terrain, t.set, alt),
                                (_, Some(road)) => Key::Road(road, alt),
                                _ => continue,
                            };
                            let m = &metatiles[&key];
                            let (x, y) = iso.tile_corner(p);
                            m.draw_tile(m.tile(r, c), &mut canvas, width, x, y);
                        }
                    }
                    col0 += STEP;
                }
                row0 += STEP;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Terrain(Terrain, u8, u8),
    Road(Road, u8),
}

/// Load the metatiles the map uses.
fn load_metatiles(res: &Resources, world: &World) -> Result<HashMap<Key, Metatile>, String> {
    let mut keys = Vec::new();
    for row in 0..world.height {
        for col in 0..world.width {
            let t = world.tile(Pos { col, row });
            for alt in 0..2 {
                keys.push(Key::Terrain(t.terrain, t.set, alt));
                if let Some(road) = t.road {
                    keys.push(Key::Road(road, alt));
                }
            }
        }
    }
    let mut out = HashMap::new();
    for key in keys {
        if out.contains_key(&key) {
            continue;
        }
        let name = match key {
            Key::Terrain(t, set, alt) => format!("terrain.{}.{}.{}.h4d", t.resource_name(), set + 1, alt + 1),
            Key::Road(r, alt) => format!("terrain.road.{}.{}.h4d", r.resource_name(), alt + 1),
        };
        let m = res.read(&name).and_then(|d| Metatile::read(&d)).map_err(|e| format!("{name}: {e}"))?;
        out.insert(key, m);
    }
    Ok(out)
}
