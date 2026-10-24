//! Animated actors (heroes, creatures) on the adventure map. Each actor is described by its
//! `adv_actor.<name>` model: which animation plays for which action and direction, at what
//! speed, and how far one walk cycle moves it. Frames are decoded on first use.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use h4_formats::adv_actor::AdvActorModel;
use h4_formats::archive::Resources;
use h4_formats::sprite::ActorSequence;
use macroquad::prelude::*;

/// Distance of one straight tile step, in the units of `walk_length` etc.
pub const STEP_LENGTH: f32 = 32.0;

/// One animation in one direction, ready to draw.
pub struct Animation {
    pub frames: Vec<Texture2D>,
    /// Where the textures' top-left corner sits relative to the actor's feet.
    pub origin: Vec2,
    pub fps: f32,
}

impl Animation {
    /// Seconds to play every frame once.
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.fps
    }
}

pub struct Actor {
    pub model: AdvActorModel,
    animations: RefCell<HashMap<(String, &'static str), Option<Rc<Animation>>>>,
}

impl Actor {
    /// `None` if the actor has no such action/direction (or it failed to load).
    pub fn animation(&self, res: &Resources, action: &str, dir: &'static str) -> Option<Rc<Animation>> {
        let key = (action.to_string(), dir);
        if let Some(a) = self.animations.borrow().get(&key) {
            return a.clone();
        }
        let loaded = self.load(res, action, dir).map(Rc::new);
        self.animations.borrow_mut().insert(key, loaded.clone());
        loaded
    }

    fn load(&self, res: &Resources, action: &str, dir: &str) -> Option<Animation> {
        let fps = self.model.action(action)?.frames_per_second as f32;
        let seq = ActorSequence::read(&res.read(&self.model.sequence(action, dir)?).ok()?).ok()?;
        let area = seq.bounds()?;
        let frames = (0..seq.frames.len())
            .map(|i| {
                let t = Texture2D::from_rgba8(area.width() as u16, area.height() as u16, &seq.compose(i, area, true));
                t.set_filter(FilterMode::Linear);
                t
            })
            .collect();
        let origin = vec2(
            (area.left + seq.offset.0 + self.model.offset.0) as f32,
            (area.top + seq.offset.1 + self.model.offset.1) as f32,
        );
        Some(Animation { frames, origin, fps: fps.max(1.0) })
    }

    /// Distance per second while playing a moving action (prewalk, walk, postwalk).
    pub fn speed(&self, res: &Resources, action: &str, dir: &'static str) -> f32 {
        let length = match action {
            "prewalk" => self.model.prewalk_length,
            "postwalk" => self.model.postwalk_length,
            _ => self.model.walk_length,
        } as f32;
        match self.animation(res, action, dir) {
            Some(a) if a.duration() > 0.0 => length / a.duration(),
            _ => STEP_LENGTH * 2.0,
        }
    }
}

/// The actors in use, loaded by name on first use.
pub struct Actors<'a> {
    pub res: &'a Resources,
    actors: RefCell<HashMap<String, Option<Rc<Actor>>>>,
}

impl<'a> Actors<'a> {
    pub fn new(res: &'a Resources) -> Actors<'a> {
        Actors { res, actors: RefCell::new(HashMap::new()) }
    }

    pub fn get(&self, name: &str) -> Option<Rc<Actor>> {
        if let Some(a) = self.actors.borrow().get(name) {
            return a.clone();
        }
        let loaded = self
            .res
            .read(&format!("adv_actor.{name}.h4d"))
            .and_then(|d| AdvActorModel::read(&d))
            .map_err(|e| eprintln!("adv_actor.{name}: {e}"))
            .ok()
            .map(|model| Rc::new(Actor { model, animations: RefCell::new(HashMap::new()) }));
        self.actors.borrow_mut().insert(name.to_string(), loaded.clone());
        loaded
    }
}

/// Which way a step from one tile to a neighbour faces on screen.
/// Map column grows towards the lower right of the screen, row towards the lower left.
pub fn direction(dcol: i32, drow: i32) -> &'static str {
    match (dcol.signum(), drow.signum()) {
        (1, 0) => "se",
        (0, 1) => "sw",
        (-1, 0) => "nw",
        (0, -1) => "ne",
        (1, 1) => "s",
        (-1, -1) => "n",
        (1, -1) => "e",
        (-1, 1) => "w",
        _ => "s",
    }
}

/// Length of a step in a direction: n, s, e, w cross a tile corner to corner and are longer.
pub fn step_length(dir: &str) -> f32 {
    if dir.len() == 1 {
        STEP_LENGTH * std::f32::consts::SQRT_2
    } else {
        STEP_LENGTH
    }
}
