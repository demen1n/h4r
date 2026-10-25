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
//!
//! The generated test map works out those masks itself ([`TerrainView::build`]). A real map
//! from a `.h4c` file already has them, chosen by the map editor, so [`TerrainView::build_map`]
//! just draws what each tile lists, the way the game's tile drawer does (`FUN_0078a750` for
//! terrain, `FUN_0078aad0` for roads; see `decomp/terrain_draw.md`).

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::map::MapFile;
use crate::mosaic::Mosaic;
use h4_formats::terrain::{Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};
use h4_formats::transition::{corner_code, Mask, Neighbours, Transitions};
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

/// Metatiles loaded on first use, by resource name (`terrain.grass.1.2.h4d`).
struct Metatiles<'a> {
    res: &'a Resources,
    loaded: HashMap<String, Metatile>,
    /// For a real map: the game's layout of metatiles, and the level drawn. Without it
    /// (the generated map) metatiles are laid every `STEP` tiles in a checkerboard.
    mosaic: Option<(&'a Mosaic, i32)>,
}

/// What a tile picture is of, so the mosaic can say which metatile of the pair a block uses.
#[derive(Clone, Copy)]
enum Kind {
    Terrain(u8, u8),
    Road(u8),
    /// The generated map, which has no mosaic.
    Generated,
}

impl Metatiles<'_> {
    fn get(&mut self, name: &str) -> Result<&Metatile, String> {
        if !self.loaded.contains_key(name) {
            let m = self.res.read(name).and_then(|d| Metatile::read(&d)).map_err(|e| format!("{name}: {e}"))?;
            self.loaded.insert(name.to_string(), m);
        }
        Ok(&self.loaded[name])
    }

    /// One map tile of a terrain (or road) as a 64x32 RGBA picture, put together from the
    /// four metatiles that overlap at that spot. `name(alt)` is the resource name of the
    /// set's first (`alt` 0) or second metatile.
    fn tile(&mut self, p: Pos, kind: Kind, name: impl Fn(u8) -> String) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; TILE_BYTES];
        if let Some((mosaic, level)) = self.mosaic {
            for paint in mosaic.paints(p.row, p.col, level) {
                let alt = match kind {
                    Kind::Terrain(t, s) => paint.block.terrain_picture(t, s),
                    Kind::Road(r) => paint.block.road_picture(r),
                    Kind::Generated => 0,
                };
                let m = self.get(&name(alt))?;
                m.draw_tile(m.tile(paint.row, paint.col), &mut buf, TILE_WIDTH, 0, 0);
            }
            return Ok(buf);
        }
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
                let m = self.get(&name(alt))?;
                m.draw_tile(m.tile(r, c), &mut buf, TILE_WIDTH, 0, 0);
            }
        }
        Ok(buf)
    }

    /// A tile of a terrain that has a single picture instead of a metatile pair (magic
    /// all/life/order). The game doesn't overlap these: it splits the map into 4x4 blocks and
    /// fills each block from the middle 4x4 tiles (3..=6) of the one picture
    /// (`FUN_0078b0d0`, rva `38b0d0`).
    fn single_tile(&mut self, p: Pos, name: &str) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; TILE_BYTES];
        let m = self.get(name)?;
        let (r, c) = (p.row.rem_euclid(4) as usize + 3, p.col.rem_euclid(4) as usize + 3);
        m.draw_tile(m.tile(r, c), &mut buf, TILE_WIDTH, 0, 0);
        Ok(buf)
    }

    /// The picture of a tile of the game's terrain `terrain` (index into
    /// `h4_formats::map::TERRAIN_NAMES`) and `subtype`.
    fn map_terrain(&mut self, p: Pos, terrain: u8, subtype: u8) -> Result<Vec<u8>, String> {
        // Resource names from the texture tables of the exe (rva 397b70 and its neighbours).
        // The terrains with two looks have `<name>.<subtype>.<alt>`, the others `<name>.<alt>`.
        let pair = |base: String| move |alt: u8| format!("terrain.{base}.{}.h4d", alt + 1);
        let kind = Kind::Terrain(terrain, subtype);
        match terrain {
            0..=8 => {
                // water, grass, rough, swamp, volcanic, snow, sand, dirt, subterranean
                let names = ["water", "grass", "rough", "swamp", "lava", "snow", "sand", "dirt", "subterranean"];
                self.tile(p, kind, pair(format!("{}.{}", names[terrain as usize], subtype + 1)))
            }
            9 => self.tile(p, kind, pair("river.water".into())),
            10 => self.tile(p, kind, pair("river.lava".into())),
            11 => self.tile(p, kind, pair("river.ice".into())),
            12 => self.single_tile(p, "terrain.magic.all.h4d"),
            13 => self.single_tile(p, "terrain.magic.life.h4d"),
            14 => self.single_tile(p, "terrain.magic.order.h4d"),
            15 => self.tile(p, kind, pair("magic.death".into())),
            16 => self.tile(p, kind, pair("magic.chaos".into())),
            17 => self.tile(p, kind, pair("magic.nature".into())),
            18 => self.tile(p, kind, pair("special.personal_combat".into())),
            t => Err(format!("unknown terrain {t}")),
        }
    }

    /// A tile of road type 1..=3 as stored in maps.
    fn map_road(&mut self, p: Pos, road: u8) -> Result<Vec<u8>, String> {
        let road = Road::from_map(road).ok_or_else(|| format!("unknown road {road}"))?;
        self.tile(p, Kind::Road(road as u8), |alt| road_name(road, alt))
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

/// Resource name of one metatile of a terrain of the generated map.
fn terrain_name(t: Terrain, set: u8, alt: u8) -> String {
    format!("terrain.{}.{}.{}.h4d", t.resource_name(), set + 1, alt + 1)
}

fn road_name(road: Road, alt: u8) -> String {
    format!("terrain.road.{}.{}.h4d", road.resource_name(), alt + 1)
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
        let mut metatiles = Metatiles { res, loaded: HashMap::new(), mosaic: None };
        let width = iso.width as usize;
        let mut canvas = vec![0u8; 4 * width * iso.height as usize];

        for row in 0..world.height {
            for col in 0..world.width {
                let p = Pos { col, row };
                let here = *world.tile(p);
                let (x, y) = iso.tile_corner(p);

                // the tile's own terrain
                let base = metatiles.tile(p, Kind::Generated, |alt| terrain_name(here.terrain, here.set, alt))?;
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
                    let texture = metatiles.tile(p, Kind::Generated, |alt| terrain_name(t.0, t.1, alt))?;
                    blend(&mut canvas, width, x, y, &texture, |mx, my| mask.alpha(mx, my));
                }

                if let Some(road) = here.road {
                    let texture = metatiles.tile(p, Kind::Generated, |alt| road_name(road, alt))?;
                    blend(&mut canvas, width, x, y, &texture, |_, _| 255);
                }
            }
        }

        Ok(TerrainView::from_canvas(&canvas, iso))
    }

    /// Draw one level of a real map by the transitions stored in its tiles. Tiles outside the
    /// map's diamond aren't drawn.
    pub fn build_map(res: &Resources, map: &MapFile, level: usize, iso: &Iso) -> Result<TerrainView, String> {
        let transitions = res
            .read("transition.Transitions.h4d")
            .and_then(|d| Transitions::read(&d))
            .map_err(|e| format!("transitions: {e}"))?;
        let mask = |set: u8, id: u16| transitions.map_mask(set, id).ok_or_else(|| format!("no mask {id} in mask set {set}"));
        let mosaic = Mosaic::new(map);
        let mut metatiles = Metatiles { res, loaded: HashMap::new(), mosaic: Some((&mosaic, level as i32)) };
        let width = iso.width as usize;
        let mut canvas = vec![0u8; 4 * width * iso.height as usize];
        let size = map.header.size as i32;

        for row in 0..size {
            for col in 0..size {
                let Some(tile) = map.tile(level, row as usize, col as usize) else { continue };
                let p = Pos { col, row };
                let (x, y) = iso.tile_corner(p);

                // the tile's own terrain, then the neighbours' edges in the stored order
                let base = metatiles.map_terrain(p, tile.terrain, tile.subtype)?;
                blend(&mut canvas, width, x, y, &base, |_, _| 255);
                for t in &tile.terrain_transitions {
                    let m = mask(t.mask_set, t.id)?;
                    let texture = metatiles.map_terrain(p, t.terrain, t.subtype)?;
                    blend(&mut canvas, width, x, y, &texture, |mx, my| m.alpha(mx, my));
                }

                if tile.road == 0 {
                    // a road next door spills over this tile's edge
                    for t in &tile.road_transitions {
                        let m = mask(t.mask_set, t.id)?;
                        let texture = metatiles.map_road(p, t.road)?;
                        blend(&mut canvas, width, x, y, &texture, |mx, my| m.alpha(mx, my));
                    }
                } else {
                    // The tile's own road. Transitions with road 0 cut its edge away where the
                    // road ends; transitions with another road type paint that road over it.
                    // As in the game, the cut applies to the finished picture.
                    let mut road = metatiles.map_road(p, tile.road)?;
                    let mut cuts: Vec<&Mask> = Vec::new();
                    for t in &tile.road_transitions {
                        let m = mask(t.mask_set, t.id)?;
                        if t.road == 0 {
                            cuts.push(m);
                        } else {
                            let other = metatiles.map_road(p, t.road)?;
                            blend(&mut road, TILE_WIDTH, 0, 0, &other, |mx, my| m.alpha(mx, my));
                        }
                    }
                    let kept = |mx: usize, my: usize| if cuts.iter().any(|m| m.alpha(mx, my) != 0) { 0 } else { 255 };
                    blend(&mut canvas, width, x, y, &road, kept);
                }
            }
        }
        shade_by_height(&mut canvas, width, map, level, iso);
        Ok(TerrainView::from_canvas(&canvas, iso))
    }

    /// Cut the finished picture of the map into GPU textures.
    fn from_canvas(canvas: &[u8], iso: &Iso) -> TerrainView {
        let width = iso.width as usize;
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
                t.set_filter(FilterMode::Nearest);
                chunks.push((vec2(cx as f32, cy as f32), t));
            }
        }
        TerrainView { chunks }
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

/// Light and shade the ground by the map's vertex heights, so hills look round.
///
/// This is our own approximation, not the game's formula (that one is still to be found
/// in the exe): light comes from the upper left of the screen, a slope facing it gets
/// brighter, a slope facing away darker. Each vertex gets a brightness from the height
/// difference of its neighbours, and pixels inside a tile blend the brightness of the
/// tile's four corners.
fn shade_by_height(canvas: &mut [u8], width: usize, map: &MapFile, level: usize, iso: &Iso) {
    // how much one unit of height difference changes the brightness (picked by eye)
    const STRENGTH: f32 = 1.0 / 60.0;
    let size = map.header.size as i32;
    let height = |row: i32, col: i32| -> Option<f32> {
        if row < 0 || col < 0 {
            return None;
        }
        map.height(level, row as usize, col as usize).map(f32::from)
    };
    // Brightness of a vertex. Screen up-left is the direction of smaller columns (columns
    // grow to the lower right), so ground rising towards larger columns faces the light.
    // Rows (growing to the lower left) count half: their direction is only partly lit.
    let brightness = |row: i32, col: i32| -> f32 {
        let Some(here) = height(row, col) else { return 1.0 };
        let along = |a: Option<f32>, b: Option<f32>| b.unwrap_or(here) - a.unwrap_or(here);
        let slope = along(height(row, col - 1), height(row, col + 1)) + 0.5 * along(height(row - 1, col), height(row + 1, col));
        (1.0 + STRENGTH * slope).clamp(0.6, 1.4)
    };

    for row in 0..size {
        for col in 0..size {
            if map.tile(level, row as usize, col as usize).is_none() {
                continue;
            }
            // corners: top = vertex (row, col), right = (row, col + 1),
            // left = (row + 1, col), bottom = (row + 1, col + 1)
            let top = brightness(row, col);
            let right = brightness(row, col + 1);
            let left = brightness(row + 1, col);
            let bottom = brightness(row + 1, col + 1);
            let (x0, y0) = iso.tile_corner(Pos { col, row });
            for py in 0..TILE_HEIGHT as i32 {
                for px in 0..TILE_WIDTH as i32 {
                    // position inside the diamond: a runs along the column axis, b along rows
                    let u = (px as f32 + 0.5 - 32.0) / 32.0;
                    let v = (py as f32 + 0.5 - 16.0) / 16.0;
                    let a = (u + v) / 2.0 + 0.5;
                    let b = (v - u) / 2.0 + 0.5;
                    if !(0.0..1.0).contains(&a) || !(0.0..1.0).contains(&b) {
                        continue;
                    }
                    let light = top * (1.0 - a) * (1.0 - b) + right * a * (1.0 - b) + left * (1.0 - a) * b + bottom * a * b;
                    let (x, y) = (x0 + px, y0 + py);
                    if x < 0 || y < 0 || x as usize >= width {
                        continue;
                    }
                    let i = 4 * (y as usize * width + x as usize);
                    let Some(pixel) = canvas.get_mut(i..i + 3) else { continue };
                    for c in pixel {
                        *c = (*c as f32 * light).min(255.0) as u8;
                    }
                }
            }
        }
    }
}
