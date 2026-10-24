//! Adventure map prototype: a generated map drawn with the original terrain, a hero who walks
//! where you click, and a few creatures standing around.
//!
//! usage: h4-game [data dir]
//! mouse: left click — walk there; right drag — scroll; wheel — zoom
//! keys: arrows/WASD — scroll, -/= — zoom, Space — centre on the hero

mod actor_view;
mod mover;
mod terrain_view;
mod world;

use h4_formats::archive::{data_dir, Resources};
use macroquad::prelude::*;

use actor_view::Actors;
use mover::Mover;
use terrain_view::{Iso, TerrainView};
use world::{Pos, World};

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
    // the generated one. Only its terrain and roads are read so far, no objects.
    let real_map = std::env::var("H4_MAP").ok().map(|path| {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        h4_formats::map::MapFile::read(&bytes, &path).unwrap_or_else(|e| panic!("{e}"))
    });
    let world = match &real_map {
        Some(map) => {
            let level = std::env::var("H4_LEVEL").ok().and_then(|l| l.parse().ok()).unwrap_or(0);
            let level = level.min(map.header.levels as usize - 1);
            World::from_map(map, level)
        }
        None => World::generate(40, 40),
    };
    let iso = Iso::new(&world);
    let terrain = match TerrainView::build(&res, &world, &iso) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("terrain: {e}");
            std::process::exit(1);
        }
    };
    let actors = Actors::new(&res);

    let spawn = |name: &str, col, row, facing| {
        let actor = actors.get(name).unwrap_or_else(|| panic!("no adv_actor.{name}"));
        Mover::new(actor, Pos { col, row }, facing)
    };
    let mut hero;
    let mut creatures = Vec::new();
    if real_map.is_some() {
        // creatures of a real map come with its objects, which aren't read yet
        let start = world.passable_near_center().unwrap_or(Pos { col: 0, row: 0 });
        hero = spawn(HERO, start.col, start.row, "e");
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

    let mut camera = Camera { pos: Vec2::ZERO, zoom: 1.0 };
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
            camera.zoom = (camera.zoom * zoom_by).clamp(0.4, 3.0);
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

        // drawing: ground, then actors from back to front
        clear_background(BLACK);
        terrain.draw(&camera);

        let mut movers: Vec<&Mover> = creatures.iter().chain(std::iter::once(&hero)).collect();
        // whoever stands lower on screen is closer to the viewer and is drawn later
        movers.sort_by(|a, b| a.feet(&iso).y.total_cmp(&b.feet(&iso).y));
        for m in movers {
            draw_mover(&res, &camera, &iso, m);
        }

        // where the mouse points
        let hover = {
            let m = camera.to_map(mouse);
            iso.tile_at(m.x, m.y)
        };
        if world.contains(hover) {
            let c = camera.to_screen(iso.tile_center(hover));
            let (hw, hh) = (32.0 * camera.zoom, 16.0 * camera.zoom);
            let color = if world.passable(hover) { Color::new(1.0, 1.0, 1.0, 0.6) } else { Color::new(1.0, 0.3, 0.3, 0.6) };
            let corners = [c + vec2(0.0, -hh), c + vec2(hw, 0.0), c + vec2(0.0, hh), c + vec2(-hw, 0.0)];
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                draw_line(a.x, a.y, b.x, b.y, 1.5, color);
            }
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
