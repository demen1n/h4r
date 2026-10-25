//! Adventure map prototype: a real map (`H4_MAP`) or a generated one, drawn with the
//! original terrain and objects, played by the rules of `h4_rules` on a `GameState`: the
//! player's heroes walk where you click, fight the monsters they walk into, and turns and
//! days pass.
//!
//! usage: h4-game [data dir]
//! mouse: left click — walk there / attack; right drag — scroll; wheel — zoom
//! keys: arrows/WASD — scroll, -/= — zoom, Space — centre on the army, Tab — next army,
//! Enter — end turn

mod actor_view;
mod armies;
mod mosaic;
mod mover;
mod object_view;
mod play;
mod relief;
mod terrain_view;
mod world;

use std::collections::HashMap;
use std::rc::Rc;

use h4_formats::archive::{data_dir, Resources};
use h4_formats::table::Table;
use h4_formats::adv_object::ObjectModel;
use h4_rules::artifacts::{self, ArtifactInfo};
use h4_rules::creatures::{self, CreatureStats};
use h4_rules::new_game::{self, Footprint};
use h4_rules::placement::{place_random_objects, Setup};
use h4_rules::random::GlobalRandom;
use h4_state::army::{Army, Hero, Slot, Stack};
use h4_state::{GameState, Player};
use macroquad::prelude::*;

use actor_view::Actors;
use armies::ArmyLooks;
use mover::Mover;
use object_view::{ObjectKind, Placed, Strip};
use play::{view_pos, Hover, Play};
use relief::Relief;
use terrain_view::{Iso, TerrainView};
use world::{Pos, Terrain, World};

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

/// Outline of a tile's diamond, raised with the ground.
fn draw_diamond(camera: &Camera, iso: &Iso, relief: &Relief, p: Pos, color: Color) {
    let corners = relief.diamond(iso, p).map(|c| camera.to_screen(c));
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        draw_line(a.x, a.y, b.x, b.y, 1.5, color);
    }
}

/// An order from `H4_ORDERS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Walk(Pos),
    EndTurn,
    NextArmy,
}

impl Order {
    /// `col,row`, `end` or `tab`; anything else is skipped.
    fn parse(s: &str) -> Option<Order> {
        match s.trim() {
            "end" => Some(Order::EndTurn),
            "tab" => Some(Order::NextArmy),
            s => {
                let (col, row) = s.split_once(',')?;
                Some(Order::Walk(Pos { col: col.trim().parse().ok()?, row: row.trim().parse().ok()? }))
            }
        }
    }
}

/// Something upright on the map, drawn in back-to-front order.
enum Upright<'a> {
    Mover(&'a Mover),
    /// A 32-pixel slice of an object image.
    Strip(&'a Placed, Strip),
}

/// The objects of the game on `level`, ready to draw and lifted with the ground under them.
/// Each model is loaded once and shared by every copy on the map.
fn placed_objects(res: &Resources, state: &GameState, level: u8, relief: &Relief) -> Vec<Placed> {
    let mut kinds: HashMap<&str, Option<Rc<ObjectKind>>> = HashMap::new();
    let mut placed = Vec::new();
    for o in state.objects.iter().filter(|o| o.at.level == level) {
        let kind = kinds.entry(&o.model).or_insert_with(|| ObjectKind::load(res, &o.model).map(Rc::new));
        if let Some(kind) = kind {
            let at = view_pos(o.at);
            // the footprint's x runs along rows, y along columns
            let lift = relief.object_height(at, kind.model.width as i32, kind.model.height as i32) as f32;
            placed.push(Placed { kind: kind.clone(), at, lift });
        }
    }
    placed
}

/// Footprints of object models for `new_game::from_map`, read from `adv_object.<model>`.
/// Models are shared by many objects: each is read once.
fn footprints(res: &Resources) -> impl FnMut(&str) -> Option<Footprint> + '_ {
    let mut models: HashMap<String, Option<Rc<Footprint>>> = HashMap::new();
    move |model| {
        let entry = models.entry(model.to_string()).or_insert_with(|| {
            let data = res.read(&format!("adv_object.{model}.h4d")).ok()?;
            let m = ObjectModel::read(&data).ok()?;
            Some(Rc::new(Footprint { width: m.width as i32, height: m.height as i32, impassable: m.impassable }))
        });
        entry.as_deref().map(|f| Footprint { width: f.width, height: f.height, impassable: f.impassable.clone() })
    }
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

/// A hero of the current player for trying things out, with some troops: in the stand-in
/// battle a hero alone loses at once (heroes don't fight yet).
fn test_army() -> Army {
    let mut army = Army::of(&[
        Stack { creature: creature("pikeman"), count: 30 },
        Stack { creature: creature("crossbowman"), count: 20 },
    ]);
    army.slots[6] = Some(Slot::Hero(Hero {
        portrait: -1,
        name: String::new(),
        class: 0, // knight
        male: true,
        level: 1,
        experience: 0,
    }));
    army
}

/// Creature id by keyword, for armies made up here.
fn creature(keyword: &str) -> usize {
    h4_rules::creatures::KEYWORDS.iter().position(|k| *k == keyword).expect("keyword from the game's list")
}

/// The free dry tile nearest to `around` on `level`, for a test hero.
fn free_tile_near(state: &GameState, level: u8, around: Pos) -> Option<h4_state::Pos> {
    let free = |p: h4_state::Pos| {
        let ok = state.map.tile(p).is_some_and(|t| t.inside && !t.blocked && t.terrain != h4_state::map::terrain::WATER);
        ok && state.army_at(p).is_none()
    };
    (0..state.map.size).find_map(|r| {
        (-r..=r).flat_map(|dr| (-r..=r).map(move |dc| (dr, dc))).find_map(|(dr, dc)| {
            let p = h4_state::Pos { level, row: around.row + dr, col: around.col + dc };
            free(p).then_some(p)
        })
    })
}

/// The hand-made test map as a game: one human player with a hero, monsters standing
/// around, buildings and forests. Returns the state, the world (for the terrain picture)
/// and the objects to draw.
fn test_game(res: &Resources) -> (GameState, World, Vec<Placed>) {
    let mut world = World::generate(40, 40);
    // (model, column, row, how many)
    let monsters = [("Griffin", 18, 20, 4), ("Black Dragon", 33, 26, 2), ("Dwarf", 10, 22, 25), ("Cyclops", 30, 6, 4), ("White Tiger", 6, 12, 8)];
    let hero_at = Pos { col: 3, row: 36 };
    // keep objects off the tiles where someone stands
    world.take(hero_at, false);
    for &(_, col, row, _) in &monsters {
        world.take(Pos { col, row }, false);
    }
    let objects = place_objects(res, &mut world);

    let player = Player { color: 0, human: true, resources: [0; 7], alive: true };
    let mut state = GameState::new(world.ground(), vec![player]);
    state.add_army(Some(0), h4_state::Pos { level: 0, row: hero_at.row, col: hero_at.col }, test_army());
    for (model, col, row, count) in monsters {
        let creature = ArmyLooks::creature_of(model).expect("test monsters have keywords");
        state.add_army(None, h4_state::Pos { level: 0, row, col }, Army::of(&[Stack { creature, count }]));
    }
    (state, world, objects)
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

fn draw_mover(res: &Resources, camera: &Camera, iso: &Iso, relief: &Relief, m: &Mover) {
    let Some((anim, i)) = m.frame(res) else { return };
    let frame = &anim.frames[i];
    // standing on the raised ground, and following it while walking
    let feet = m.feet(iso);
    let at = camera.to_screen(feet - vec2(0.0, relief.ground_at(iso, feet)) + anim.origin);
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
    // the rules need the creatures' numbers for everything: walking, fighting, growing
    let (stats, arts) = game_tables(&res).unwrap_or_else(|e| {
        eprintln!("game tables: {e}");
        std::process::exit(1);
    });
    // The game's global generator. The game seeds it with the clock; a fixed seed (or
    // `H4_SEED`) gives the same game every time.
    let seed = std::env::var("H4_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut rng = GlobalRandom::new(seed);

    // H4_MAP=path/to/map.h4c (and H4_LEVEL=1 for underground): play a real map instead of
    // the generated one.
    let real_map = std::env::var("H4_MAP").ok().map(|path| {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        h4_formats::map::MapFile::read(&bytes, &path).unwrap_or_else(|e| panic!("{e}"))
    });
    let chosen_level: u8 = std::env::var("H4_LEVEL").ok().and_then(|l| l.parse().ok()).unwrap_or(0);
    let (mut state, level, iso, relief, terrain, objects) = match &real_map {
        Some(map) => {
            // what the random objects become, as the game decides at the start (normal difficulty)
            let setup = Setup::from_header(&map.header, 1);
            let placements = place_random_objects(map, &stats, &arts, &setup, &mut rng);
            let state = new_game::from_map(map, &placements, footprints(&res));
            let level = chosen_level.min(map.header.levels.saturating_sub(1));
            let relief = Relief::from_map(map, level as usize);
            // room above the map for the highest raised ground
            let iso = Iso::new(state.map.size, state.map.size, relief.max());
            // a real map is drawn by the transitions stored in it
            let terrain = TerrainView::build_map(&res, map, level as usize, &iso, &relief);
            let objects = placed_objects(&res, &state, level, &relief);
            (state, level, iso, relief, terrain, objects)
        }
        None => {
            let (state, world, objects) = test_game(&res);
            // the test map is flat
            let relief = Relief::flat(world.width);
            let iso = Iso::new(world.width, world.height, 0);
            // the test map works its transitions out
            let terrain = TerrainView::build(&res, &world, &iso);
            (state, 0, iso, relief, terrain, objects)
        }
    };
    let terrain = terrain.unwrap_or_else(|e| {
        eprintln!("terrain: {e}");
        std::process::exit(1);
    });

    // H4_START=col,row puts a test hero of the current player there (the nearest free
    // tile). Without it, a player with no army on the shown level gets one near the centre.
    let chosen = std::env::var("H4_START").ok().and_then(|s| {
        let (col, row) = s.split_once(',')?;
        Some(Pos { col: col.trim().parse().ok()?, row: row.trim().parse().ok()? })
    });
    let color = state.players.get(state.current_player).map(|p| p.color);
    let has_army = state.armies.iter().any(|a| a.owner.is_some() && a.owner == color && a.at.level == level);
    if chosen.is_some() || !has_army {
        let around = chosen.unwrap_or(Pos { col: state.map.size / 2, row: state.map.size / 2 });
        if let (Some(at), Some(_)) = (free_tile_near(&state, level, around), color) {
            state.add_army(color, at, test_army());
        }
    }

    let actors = Actors::new(&res);
    let looks = ArmyLooks::new(&res);
    let mut play = Play::new(state, stats, rng, level, &actors, &looks);

    // H4_FOOTPRINTS=1: outline object cells (red: impassable) to check them against the art
    let show_footprints = std::env::var("H4_FOOTPRINTS").is_ok();
    let mut time = 0.0;

    // Start zoom picked by eye against the original running under Wine on a Retina Mac:
    // 1.28 looks closest; zoom 1 is too small, 2 too coarse.
    let mut camera = Camera { pos: Vec2::ZERO, zoom: 1.28 };
    let map_center = vec2(iso.width as f32, iso.height as f32) / 2.0;
    // where a figure shows: its feet on the raised ground
    let shown_at = |f: &Mover| {
        let feet = f.feet(&iso);
        feet - vec2(0.0, relief.ground_at(&iso, feet))
    };
    camera.center_on(play.selected_figure().map_or(map_center, shown_at));
    // H4_OVERVIEW=1: start zoomed out over the whole map (handy with H4_SCREENSHOT)
    if std::env::var("H4_OVERVIEW").is_ok() {
        camera.zoom = (screen_width() / iso.width as f32).min(screen_height() / iso.height as f32);
        camera.center_on(map_center);
    }
    let mut drag_from: Option<Vec2> = None;
    // H4_SCREENSHOT=file.png: save one frame after things have loaded, then quit (for testing)
    let screenshot = std::env::var("H4_SCREENSHOT").ok();
    let mut frames = 0;
    // H4_ORDERS="col,row;end;tab": orders given one by one, each once the last one has been
    // shown (a click on a tile, end turn, next army). With H4_SCREENSHOT the picture is
    // taken after the last one, to check walks and battles without a person.
    let mut orders: Vec<Order> = std::env::var("H4_ORDERS").map_or(Vec::new(), |s| s.split(';').filter_map(Order::parse).collect());
    orders.reverse(); // taken from the back
    let mut last_message = String::new();

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

        // orders
        let hover = relief.pick(&iso, camera.to_map(mouse));
        let order = if play.busy() { None } else { orders.pop() };
        if let Some(order) = &order {
            println!("order {order:?}");
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            play.walk_to(hover);
        }
        if let Some(Order::Walk(to)) = order {
            play.walk_to(to);
        }
        if order == Some(Order::EndTurn) {
            play.end_turn();
        }
        if order == Some(Order::NextArmy) {
            play.select_next();
        }
        if order.is_some() || is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Enter) {
            // follow the selected army
            if let Some(f) = play.selected_figure() {
                camera.center_on(shown_at(f));
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            play.select_next();
        }
        if is_key_pressed(KeyCode::Enter) {
            play.end_turn();
        }
        if is_key_pressed(KeyCode::Space) {
            if let Some(f) = play.selected_figure() {
                camera.center_on(shown_at(f));
            }
        }
        play.update(&res, dt);
        // what happened also goes to the terminal, for runs without a person
        if play.message != last_message {
            println!("{}", play.message);
            last_message = play.message.clone();
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
        for m in play.figures() {
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
                Upright::Mover(m) => draw_mover(&res, &camera, &iso, &relief, m),
                Upright::Strip(o, s) => o.draw_strip(&camera, &iso, s, time),
            }
        }
        if show_footprints {
            for o in &objects {
                for (p, blocked) in o.kind.cells(o.at) {
                    let color = if blocked { Color::new(1.0, 0.2, 0.2, 0.9) } else { Color::new(0.2, 1.0, 0.2, 0.9) };
                    draw_diamond(&camera, &iso, &relief, p, color);
                }
            }
        }

        // the selected army, and what a click on the tile under the mouse would do
        if let Some(f) = play.selected_figure() {
            draw_diamond(&camera, &iso, &relief, f.next_tile(), Color::new(1.0, 0.85, 0.2, 0.8));
        }
        let color = match play.hover(hover) {
            Hover::Walk => Color::new(1.0, 1.0, 1.0, 0.6),
            Hover::Attack => Color::new(1.0, 0.6, 0.1, 0.9),
            Hover::Blocked => Color::new(1.0, 0.3, 0.3, 0.6),
        };
        draw_diamond(&camera, &iso, &relief, hover, color);

        // a dark band under the text, so it reads over trees and snow
        draw_rectangle(0.0, 0.0, screen_width(), 78.0, Color::new(0.0, 0.0, 0.0, 0.55));
        draw_rectangle(0.0, screen_height() - 30.0, screen_width(), 30.0, Color::new(0.0, 0.0, 0.0, 0.55));
        draw_text(play.status(), 12.0, 24.0, 24.0, WHITE);
        let under = play.army_at(hover).unwrap_or_default();
        draw_text(
            format!("tile {},{}   zoom x{:.2}   {} fps   {under}", hover.col, hover.row, camera.zoom, get_fps()),
            12.0,
            46.0,
            18.0,
            LIGHTGRAY,
        );
        draw_text(&play.message, 12.0, 68.0, 20.0, YELLOW);
        draw_text(
            "click: walk / attack   tab: next army   enter: end turn   space: centre   right drag / arrows: scroll   wheel / - =: zoom",
            12.0,
            screen_height() - 12.0,
            18.0,
            LIGHTGRAY,
        );

        frames += 1;
        if let Some(path) = &screenshot {
            if frames >= 30 && orders.is_empty() && !play.busy() {
                println!("{}", play.status());
                get_screen_data().export_png(path);
                break;
            }
        }
        next_frame().await;
    }
}
