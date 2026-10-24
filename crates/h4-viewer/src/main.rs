//! Browse and play actor animations from the original game files.
//!
//! usage: h4-viewer [data dir]
//! keys: ←/→ actor, ↑/↓ action, Q/E rotate, Space pause, ,/. step, [/] speed, -/= zoom, S shadow

use std::collections::BTreeMap;

use h4_formats::archive::Resources;
use h4_formats::catalog::{self, Actor};
use h4_formats::sprite::ActorSequence;
use macroquad::prelude::*;

struct Loaded {
    frames: Vec<Texture2D>,
    size: Vec2,
    shadows: usize,
}

struct Viewer {
    res: Resources,
    actors: Vec<Actor>,
    actor: usize,
    action: usize,
    direction: usize,
    frame: usize,
    clock: f32,
    fps: f32,
    zoom: f32,
    paused: bool,
    shadow: bool,
    loaded: Option<Loaded>,
    error: Option<String>,
}

impl Viewer {
    fn actor(&self) -> &Actor {
        &self.actors[self.actor]
    }

    fn action_name(&self) -> String {
        self.actor().action_names().get(self.action).map(|s| s.to_string()).unwrap_or_default()
    }

    fn direction_name(&self) -> &'static str {
        let dirs = self.actor().directions(&self.action_name());
        dirs.get(self.direction % dirs.len().max(1)).copied().unwrap_or("")
    }

    fn reload(&mut self) {
        self.frame = 0;
        self.clock = 0.0;
        self.loaded = None;
        self.error = None;
        let action = self.action_name();
        let dir = self.direction_name();
        let Some(name) = self.actor().actions.get(&action).and_then(|d| d.get(dir)).cloned() else {
            return;
        };
        let seq = match self.res.read(&name).and_then(|d| ActorSequence::read(&d)) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("{name}: {e}"));
                return;
            }
        };
        let Some(area) = seq.bounds() else { return };
        let (w, h) = (area.width() as u16, area.height() as u16);
        let frames = (0..seq.frames.len())
            .map(|i| {
                let t = Texture2D::from_rgba8(w, h, &seq.compose(i, area, self.shadow));
                t.set_filter(FilterMode::Nearest);
                t
            })
            .collect();
        self.loaded = Some(Loaded { frames, size: vec2(w as f32, h as f32), shadows: seq.shadows.len() });
    }

    fn handle_keys(&mut self) {
        let wrap = |i: usize, delta: isize, n: usize| ((i as isize + delta).rem_euclid(n.max(1) as isize)) as usize;
        let mut reload = false;
        if is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::Left) {
            let d = if is_key_pressed(KeyCode::Right) { 1 } else { -1 };
            let keep = self.action_name();
            self.actor = wrap(self.actor, d, self.actors.len());
            // stay on the same action if the next actor has it
            self.action = self.actor().action_names().iter().position(|a| *a == keep).unwrap_or(0);
            reload = true;
        }
        if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::Up) {
            let d = if is_key_pressed(KeyCode::Down) { 1 } else { -1 };
            self.action = wrap(self.action, d, self.actor().actions.len());
            reload = true;
        }
        if is_key_pressed(KeyCode::E) || is_key_pressed(KeyCode::Q) {
            let d = if is_key_pressed(KeyCode::E) { 1 } else { -1 };
            self.direction = wrap(self.direction, d, 8);
            reload = true;
        }
        if is_key_pressed(KeyCode::S) {
            self.shadow = !self.shadow;
            reload = true;
        }
        if is_key_pressed(KeyCode::Space) {
            self.paused = !self.paused;
        }
        let n = self.loaded.as_ref().map_or(1, |l| l.frames.len().max(1));
        if is_key_pressed(KeyCode::Period) {
            self.frame = (self.frame + 1) % n;
        }
        if is_key_pressed(KeyCode::Comma) {
            self.frame = (self.frame + n - 1) % n;
        }
        if is_key_pressed(KeyCode::RightBracket) {
            self.fps = (self.fps + 2.0).min(60.0);
        }
        if is_key_pressed(KeyCode::LeftBracket) {
            self.fps = (self.fps - 2.0).max(2.0);
        }
        if is_key_pressed(KeyCode::Equal) {
            self.zoom = (self.zoom + 0.5).min(8.0);
        }
        if is_key_pressed(KeyCode::Minus) {
            self.zoom = (self.zoom - 0.5).max(0.5);
        }
        if reload {
            self.reload();
        }
    }

    fn update(&mut self, dt: f32) {
        let Some(l) = &self.loaded else { return };
        if self.paused || l.frames.is_empty() {
            return;
        }
        self.clock += dt;
        let step = 1.0 / self.fps;
        while self.clock >= step {
            self.clock -= step;
            self.frame = (self.frame + 1) % l.frames.len();
        }
    }

    fn draw(&self) {
        clear_background(Color::from_rgba(52, 62, 44, 255));
        if let Some(l) = &self.loaded {
            if let Some(t) = l.frames.get(self.frame) {
                let size = l.size * self.zoom;
                let pos = vec2(screen_width(), screen_height()) / 2.0 - size / 2.0;
                draw_texture_ex(t, pos.x, pos.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
            }
        }
        let a = self.actor();
        let frames = self.loaded.as_ref().map_or(0, |l| l.frames.len());
        let lines = [
            format!("{}  ({}/{})", a.name, self.actor + 1, self.actors.len()),
            format!("{}  [{}]", self.action_name(), a.directions(&self.action_name()).join(" ")),
            format!(
                "dir {}   frame {}/{}   {} fps   x{}{}{}",
                self.direction_name(),
                self.frame + 1,
                frames,
                self.fps,
                self.zoom,
                if self.paused { "   paused" } else { "" },
                match &self.loaded {
                    Some(l) if self.shadow && l.shadows > 0 => "   shadow",
                    _ => "",
                },
            ),
            "<- -> actor   up/down action   Q/E rotate   space pause   , . step   [ ] speed   - = zoom   S shadow".into(),
        ];
        for (i, s) in lines.iter().enumerate() {
            draw_text(s, 16.0, 28.0 + 24.0 * i as f32, if i == 3 { 18.0 } else { 26.0 }, if i == 3 { GRAY } else { WHITE });
        }
        if let Some(e) = &self.error {
            draw_text(e, 16.0, screen_height() - 20.0, 22.0, RED);
        }
    }
}

fn window_conf() -> Conf {
    Conf { window_title: "Heroes IV animation viewer".into(), window_width: 1100, window_height: 800, high_dpi: true, ..Default::default() }
}

#[macroquad::main(window_conf)]
async fn main() {
    let dir = h4_formats::archive::data_dir(std::env::args().nth(1));
    let res = match Resources::open(&dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cannot open game data in {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let mut by_name: BTreeMap<String, Actor> = catalog::actors(&res);
    by_name.retain(|_, a| !a.actions.is_empty());
    let actors: Vec<Actor> = by_name.into_values().collect();
    let start = actors.iter().position(|a| a.name.eq_ignore_ascii_case("griffin")).unwrap_or(0);

    let mut v = Viewer {
        res,
        actors,
        actor: start,
        action: 0,
        direction: 2,
        frame: 0,
        clock: 0.0,
        fps: 12.0,
        zoom: 2.0,
        paused: false,
        shadow: true,
        loaded: None,
        error: None,
    };
    if let Some(i) = v.actor().action_names().iter().position(|a| *a == "walk") {
        v.action = i;
    }
    v.reload();

    loop {
        v.handle_keys();
        v.update(get_frame_time());
        v.draw();
        next_frame().await;
    }
}
