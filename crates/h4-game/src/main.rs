//! Adventure map prototype: a generated map drawn with the original terrain, trees, mountains
//! and a few buildings, a hero who walks where you click, and creatures standing around.
//!
//! usage: h4-game [data dir]
//! mouse: left click — walk there; right drag — scroll; wheel — zoom
//! keys: arrows/WASD — scroll, -/= — zoom, Space — centre on the hero

mod actor_view;
mod armies;
mod mosaic;
mod mover;
mod object_view;
mod terrain_view;
mod world;

use std::collections::HashMap;
use std::rc::Rc;

use h4_formats::archive::{data_dir, Resources};
use h4_formats::table::Table;
use h4_rules::artifacts::{self, ArtifactInfo};
use h4_rules::creatures::{self, CreatureStats};
use h4_rules::placement::{place_random_objects, Placement, Setup};
use h4_rules::random::GlobalRandom;
use macroquad::prelude::*;

use actor_view::Actors;
use mover::Mover;
use object_view::{ObjectKind, Placed, Strip};
use terrain_view::{Iso, TerrainView};
use world::{Pos, Terrain, World};

const HERO: &str = "hero.might_might_male";

/// What part of the map is on screen: `pos` is the map pixel at the window's top-left.
pub struct Camera {
    pub pos: Vec2,
    pub zoom: f32,
}

impl Camera {
    pub fn to_screen(&self, p: Vec2) -> Vec2 {
        (p - self.pos) * self.zoom
    }

    pub fn to_map(&self, s: Vec2) -> Vec2 {
        s / self.zoom + self.pos
    }

    fn center_on(&mut self, p: Vec2) {
        self.pos = p - vec2(screen_width(), screen_height()) / (2.0 * self.zoom);
    }
}

/// Outline of a tile's diamond.
fn draw_diamond(camera: &Camera, iso: &Iso, p: Pos, color: Color) {
    let c = camera.to_screen(iso.tile_center(p));
    let (hw, hh) = (32.0 * camera.zoom, 16.0 * camera.zoom);
    let corners = [c + vec2(0.0, -hh), c + vec2(hw, 0.0), c + vec2(0.0, hh), c + vec2(-hw, 0.0)];
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        draw_line(a.x, a.y, b.x, b.y, 1.5, color);
    }
}

/// Something upright on the map, drawn in back-to-front order.
enum Upright<'a> {
    Mover(&'a Mover),
    /// A 32-pixel slice of an object image.
    Strip(&'a Placed, Strip),
}

/// The objects of a real map on `level`, at the positions stored in the map. They are put
/// down as they are (no check for free cells: the map maker already arranged them), and
/// their cells are marked taken / blocked for the path finder.
///
/// Random objects are drawn as what they became (`placements`, one entry per map object);
/// random monsters are armies and drawn by `armies::map_armies`.
fn map_objects(
    res: &Resources,
    world: &mut World,
    map: &h4_formats::map::MapFile,
    placements: &[Option<Placement>],
    level: usize,
) -> Vec<Placed> {
    let body = match &map.body {
        Ok(body) => body,
        Err(e) => {
            eprintln!("objects not shown: {e}");
            return Vec::new();
        }
    };
    let mut kinds: HashMap<&str, Option<Rc<ObjectKind>>> = HashMap::new();
    let mut placed = Vec::new();
    for (i, o) in body.objects.iter().enumerate() {
        // armies (heroes, wandering creatures) are put down by `armies::map_armies`
        let h4_formats::map_body::ObjectKind::Placed(p) = &o.kind else { continue };
        if o.level != level as i32 {
            continue;
        }
        let placement = placements.get(i).and_then(|p| p.as_ref());
        // a random monster is an army now, or nothing if no creature fitted
        if matches!(p.type_name.as_str(), "random_monster" | "random_random_monster") {
            continue;
        }
        let model = placement.and_then(|p| p.model()).unwrap_or(&p.model);
        let kind = kinds.entry(model).or_insert_with(|| ObjectKind::load(res, model).map(Rc::new));
        let Some(kind) = kind else { continue };
        // the stored x is our row, y our column. Checked on all maps of the installation:
        // this way 77% of sand decorations stand on sand and bridges mostly on water,
        // the other way round only 27% and mostly on grass.
        let at = Pos { col: o.y, row: o.x };
        // a bridge's cells are its deck: walkable, and over water too
        let bridge = p.type_name == "bridge";
        for (cell, blocked) in kind.cells(at) {
            if world.contains(cell) {
                if bridge {
                    world.take(cell, false);
                    world.bridge(cell);
                } else {
                    world.take(cell, blocked);
                }
            }
        }
        placed.push(Placed { kind: kind.clone(), at });
    }
    placed
}

/// Creature stats and artifact data in game enum order, for random objects.
fn game_tables(res: &Resources) -> Result<(Vec<CreatureStats>, Vec<ArtifactInfo>), String> {
    let table = |name: &str| -> Result<Table, String> {
        let data = res.read(&format!("table.{name}.h4d")).map_err(|e| e.to_string())?;
        Table::read(&data, name).map_err(|e| e.to_string())
    };
    let creatures = creatures::in_enum_order(&creatures::from_table(&table("creatures")?)?)?;
    let artifacts = artifacts::from_table(&table("Artifacts")?)?;
    Ok((creatures, artifacts))
}

/// Decide what the map's random objects become, as the game does at the start. The seed is
/// fixed (or `H4_SEED`), so the same map looks the same every time; the game uses the clock.
fn random_placements(res: &Resources, map: &h4_formats::map::MapFile) -> Vec<Option<Placement>> {
    let (stats, arts) = match game_tables(res) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("random objects stay random: {e}");
            return Vec::new();
        }
    };
    let seed = std::env::var("H4_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    // normal difficulty
    let setup = Setup::from_header(&map.header, 1);
    place_random_objects(map, &stats, &arts, &setup, &mut GlobalRandom::new(seed))
}

/// Buildings placed by hand near the hero's start, then forests and mountains scattered
/// over the terrain they belong to. Objects that don't fit (water, road, another object)
/// are skipped.
fn place_objects(res: &Resources, world: &mut World) -> Vec<Placed> {
    // each model is loaded once and shared by every copy on the map
    let mut kinds: HashMap<&str, Option<Rc<ObjectKind>>> = HashMap::new();
    let mut placed = Vec::new();
    let mut put = |world: &mut World, name: &'static str, col: i32, row: i32| {
        let kind = kinds.entry(name).or_insert_with(|| ObjectKind::load(res, name).map(Rc::new));
        if let Some(k) = kind {
            if let Some(p) = object_view::place(world, k, Pos { col, row }) {
                placed.push(p);
            }
        }
    };

    // buildings around the start; the sawmill and its mirrored "R" twin check the footprint
    // orientation (2x3 vs 3x2)
    put(world, "creature generators.Hovel", 6, 31);
    put(world, "Skill Enhancer.Library", 9, 35);
    put(world, "movement modifiers.Stables", 1, 31);
    put(world, "Shroud Removers.Tower", 9, 31);
    put(world, "mine.Sawmill", 12, 32);
    put(world, "mine.Sawmill R", 4, 27);
    put(world, "mountains.Grass.31", 14, 36);
    put(world, "mountains.Grass.04", 0, 24);

    // scattered trees and mountains, picked by a fixed pseudo-random sequence so the map is
    // the same every run
    let grass_trees = [
        "trees.Grass.oak 1",
        "trees.Grass.oak 2",
        "trees.Grass.Oak 4",
        "trees.Grass.Cherry 2",
        "trees.Grass.willow 1",
        "trees.Grass.Plum 3",
        "trees.Grass.Magnolia 2",
    ];
    let snow_trees = ["trees.Snow.Picea01", "trees.Snow.Picea04", "trees.Snow.Cembra02", "trees.Snow.Dry4"];
    let grass_mountains = ["mountains.Grass.02", "mountains.Grass.05", "mountains.Grass.13", "mountains.Grass.26"];
    let snow_mountains = ["mountains.Snow.03", "mountains.Snow.05"];
    let mut seed: u32 = 12345;
    let mut next = move |n: usize| {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        (seed >> 16) as usize % n
    };
    for _ in 0..500 {
        let p = Pos { col: next(world.width as usize) as i32, row: next(world.height as usize) as i32 };
        let roll = next(100);
        let name = match world.tile(p).terrain {
            Terrain::Grass if roll < 6 => grass_mountains[next(grass_mountains.len())],
            Terrain::Grass => grass_trees[next(grass_trees.len())],
            Terrain::Snow if roll < 15 => snow_mountains[next(snow_mountains.len())],
            Terrain::Snow => snow_trees[next(snow_trees.len())],
            _ => continue,
        };
        // keep a clearing around the hero's start
        if (p.col - 3).abs() + (p.row - 36).abs() < 4 {
            continue;
        }
        put(world, name, p.col, p.row);
    }
    placed
}

fn draw_mover(res: &Resources, camera: &Camera, iso: &Iso, m: &Mover) {
    let Some((anim, i)) = m.frame(res) else { return };
    let frame = &anim.frames[i];
    let at = camera.to_screen(m.feet(iso) + anim.origin);
    let size = frame.size() * camera.zoom;
    draw_texture_ex(frame, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Heroes IV — adventure map prototype".into(),
        window_width: 1280,
        window_height: 800,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let dir = data_dir(std::env::args().nth(1));
    let res = match Resources::open(&dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cannot open game data in {}: {e}", dir.display());
            std::process::exit(1);
        }
    };

    // H4_MAP=path/to/map.h4c (and H4_LEVEL=1 for underground): show a real map instead of
    // the generated one: its terrain, roads, objects and armies.
    let real_map = std::env::var("H4_MAP").ok().map(|path| {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        h4_formats::map::MapFile::read(&bytes, &path).unwrap_or_else(|e| panic!("{e}"))
    });
    let level = std::env::var("H4_LEVEL").ok().and_then(|l| l.parse().ok()).unwrap_or(0);
    let placements = match &real_map {
        Some(map) => random_placements(&res, map),
        None => Vec::new(),
    };
    let mut world = match &real_map {
        Some(map) => World::from_map(map, level.min(map.header.levels as usize - 1)),
        None => World::generate(40, 40),
    };
    let iso = Iso::new(&world);
    // a real map is drawn by the transitions stored in it, the test map works them out
    let terrain = match &real_map {
        Some(map) => TerrainView::build_map(&res, map, level.min(map.header.levels as usize - 1), &iso),
        None => TerrainView::build(&res, &world, &iso),
    };
    let terrain = match terrain {
        Ok(t) => t,
        Err(e) => {
            eprintln!("terrain: {e}");
            std::process::exit(1);
        }
    };
    let actors = Actors::new(&res);
    // a real map's own objects go down first, so the hero doesn't start inside one
    let map_placed = match &real_map {
        Some(map) => map_objects(&res, &mut world, map, &placements, level.min(map.header.levels as usize - 1)),
        None => Vec::new(),
    };
    let map_armies = match &real_map {
        Some(map) => {
            armies::map_armies(&res, &mut world, map, &placements, level.min(map.header.levels as usize - 1), HERO)
        }
        None => Vec::new(),
    };

    let spawn = |name: &str, col, row, facing| {
        let actor = actors.get(name).unwrap_or_else(|| panic!("no adv_actor.{name}"));
        Mover::new(actor, Pos { col, row }, facing)
    };
    let mut hero;
    let mut creatures;
    if real_map.is_some() {
        // H4_START=col,row puts the hero (and so the first screenshot) at a chosen tile
        let chosen = std::env::var("H4_START").ok().and_then(|s| {
            let (col, row) = s.split_once(',')?;
            Some(Pos { col: col.trim().parse().ok()?, row: row.trim().parse().ok()? })
        });
        let start = chosen.or_else(|| world.passable_near_center()).unwrap_or(Pos { col: 0, row: 0 });
        hero = spawn(HERO, start.col, start.row, "e");
        creatures = map_armies.iter().map(|a| spawn(&a.actor, a.at.col, a.at.row, "sw")).collect();
    } else {
        hero = spawn(HERO, 3, 36, "e");
        creatures = vec![
            spawn("Griffin", 18, 20, "sw"),
            spawn("Black Dragon", 33, 26, "w"),
            spawn("Dwarf", 10, 22, "se"),
            spawn("Cyclops", 30, 6, "s"),
            spawn("White Tiger", 6, 12, "e"),
        ];
    }
    // keep objects off the tiles where someone already stands
    for m in creatures.iter().chain(std::iter::once(&hero)) {
        world.take(m.next_tile(), false);
    }
    // the sample objects only fit the generated map
    let objects = if real_map.is_some() { map_placed } else { place_objects(&res, &mut world) };
    // H4_FOOTPRINTS=1: outline object cells (red: impassable) to check them against the art
    let show_footprints = std::env::var("H4_FOOTPRINTS").is_ok();
    let mut time = 0.0;

    // Start zoom picked by eye against the original running under Wine on a Retina Mac:
    // 1.28 looks closest; zoom 1 is too small, 2 too coarse.
    let mut camera = Camera { pos: Vec2::ZERO, zoom: 1.28 };
    camera.center_on(hero.feet(&iso));
    // H4_OVERVIEW=1: start zoomed out over the whole map (handy with H4_SCREENSHOT)
    if std::env::var("H4_OVERVIEW").is_ok() {
        camera.zoom = (screen_width() / iso.width as f32).min(screen_height() / iso.height as f32);
        camera.center_on(vec2(iso.width as f32, iso.height as f32) / 2.0);
    }
    let mut drag_from: Option<Vec2> = None;
    // H4_SCREENSHOT=file.png: save one frame after things have loaded, then quit (for testing)
    let screenshot = std::env::var("H4_SCREENSHOT").ok();
    let mut frames = 0;

    loop {
        let dt = get_frame_time();
        time += dt;
        let mouse = Vec2::from(mouse_position());

        // scrolling and zoom
        let mut scroll = Vec2::ZERO;
        if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
            scroll.x -= 1.0;
        }
        if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
            scroll.x += 1.0;
        }
        if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
            scroll.y -= 1.0;
        }
        if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
            scroll.y += 1.0;
        }
        camera.pos += scroll * 700.0 * dt / camera.zoom;
        if is_mouse_button_pressed(MouseButton::Right) {
            drag_from = Some(mouse);
        }
        if is_mouse_button_down(MouseButton::Right) {
            if let Some(from) = drag_from {
                camera.pos -= (mouse - from) / camera.zoom;
                drag_from = Some(mouse);
            }
        }
        let wheel = mouse_wheel().1;
        let zoom_by = if is_key_pressed(KeyCode::Equal) || wheel > 0.0 {
            1.25
        } else if is_key_pressed(KeyCode::Minus) || wheel < 0.0 {
            0.8
        } else {
            1.0
        };
        if zoom_by != 1.0 {
            // keep the map point under the mouse in place
            let anchor = camera.to_map(mouse);
            camera.zoom = (camera.zoom * zoom_by).clamp(0.4, 4.0);
            camera.pos = anchor - mouse / camera.zoom;
        }
        if is_key_pressed(KeyCode::Space) {
            camera.center_on(hero.feet(&iso));
        }

        // walking
        if is_mouse_button_pressed(MouseButton::Left) {
            let m = camera.to_map(mouse);
            let target = iso.tile_at(m.x, m.y);
            // finish the current step first, then go
            if let Some(path) = world.find_path(hero.next_tile(), target) {
                hero.walk(path);
            }
        }
        hero.update(&res, dt);
        for c in &mut creatures {
            c.update(&res, dt);
        }

        // drawing: ground, object shadows, then everything upright from back to front
        clear_background(BLACK);
        terrain.draw(&camera);
        let visible: Vec<&Placed> = objects.iter().filter(|o| o.on_screen(&camera, &iso)).collect();
        for o in &visible {
            o.draw_shadow(&camera, &iso, time);
        }

        // Whatever stands lower on screen is closer to the viewer and is drawn later. Objects
        // take part as 32-pixel strips, each sorted by the front of the footprint under it.
        let mut upright: Vec<(f32, Upright)> = Vec::new();
        for m in creatures.iter().chain(std::iter::once(&hero)) {
            upright.push((m.feet(&iso).y, Upright::Mover(m)));
        }
        for &o in &visible {
            for s in o.strips(&iso) {
                upright.push((s.key, Upright::Strip(o, s)));
            }
        }
        // on a tie (a hero beside an object's front cell) the hero goes in front
        upright.sort_by(|a, b| {
            let is_mover = |u: &Upright| matches!(u, Upright::Mover(_));
            a.0.total_cmp(&b.0).then(is_mover(&a.1).cmp(&is_mover(&b.1)))
        });
        for (_, u) in &upright {
            match u {
                Upright::Mover(m) => draw_mover(&res, &camera, &iso, m),
                Upright::Strip(o, s) => o.draw_strip(&camera, &iso, s, time),
            }
        }
        if show_footprints {
            for o in &objects {
                for (p, blocked) in o.kind.cells(o.at) {
                    let color = if blocked { Color::new(1.0, 0.2, 0.2, 0.9) } else { Color::new(0.2, 1.0, 0.2, 0.9) };
                    draw_diamond(&camera, &iso, p, color);
                }
            }
        }

        // where the mouse points
        let hover = {
            let m = camera.to_map(mouse);
            iso.tile_at(m.x, m.y)
        };
        if world.contains(hover) {
            let color = if world.passable(hover) { Color::new(1.0, 1.0, 1.0, 0.6) } else { Color::new(1.0, 0.3, 0.3, 0.6) };
            draw_diamond(&camera, &iso, hover, color);
        }

        let t = if world.contains(hover) { format!("{:?}", world.tile(hover).terrain) } else { String::new() };
        draw_text(
            &format!("tile {},{} {t}   zoom x{:.2}   {} fps", hover.col, hover.row, camera.zoom, get_fps()),
            12.0,
            24.0,
            24.0,
            WHITE,
        );
        draw_text("click: walk   right drag / arrows: scroll   wheel / - =: zoom   space: hero", 12.0, 46.0, 18.0, LIGHTGRAY);

        frames += 1;
        if let Some(path) = &screenshot {
            if frames == 30 {
                get_screen_data().export_png(path);
                break;
            }
        }
        next_frame().await;
    }
}
