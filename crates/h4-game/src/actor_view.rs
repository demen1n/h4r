//! Animated actors (heroes, creatures): frames are decoded on first use and kept as textures.

use std::cell::RefCell;
use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::catalog::{self, Actor};
use h4_formats::sprite::ActorSequence;
use macroquad::prelude::*;

/// One animation in one direction, ready to draw.
pub struct Animation {
    pub frames: Vec<Texture2D>,
    /// Where the textures' top-left corner sits relative to the actor's feet.
    pub origin: Vec2,
}

/// All animations of the actors we use, loaded lazily.
pub struct Actors<'a> {
    res: &'a Resources,
    catalog: std::collections::BTreeMap<String, Actor>,
    cache: RefCell<HashMap<(String, String, &'static str), Option<std::rc::Rc<Animation>>>>,
}

impl<'a> Actors<'a> {
    pub fn new(res: &'a Resources) -> Actors<'a> {
        Actors { res, catalog: catalog::actors(res), cache: RefCell::new(HashMap::new()) }
    }

    /// `None` if the actor has no such animation (or it failed to load).
    pub fn get(&self, actor: &str, action: &str, dir: &'static str) -> Option<std::rc::Rc<Animation>> {
        let key = (actor.to_lowercase(), action.to_string(), dir);
        if let Some(a) = self.cache.borrow().get(&key) {
            return a.clone();
        }
        let loaded = self.load(&key.0, action, dir).map(std::rc::Rc::new);
        self.cache.borrow_mut().insert(key, loaded.clone());
        loaded
    }

    fn load(&self, actor: &str, action: &str, dir: &str) -> Option<Animation> {
        let name = self.catalog.get(actor)?.actions.get(action)?.get(dir)?;
        let seq = ActorSequence::read(&self.res.read(name).ok()?).ok()?;
        let area = seq.bounds()?;
        let frames = (0..seq.frames.len())
            .map(|i| {
                let t = Texture2D::from_rgba8(area.width() as u16, area.height() as u16, &seq.compose(i, area, true));
                t.set_filter(FilterMode::Linear);
                t
            })
            .collect();
        let origin = vec2((area.left + seq.offset.0) as f32, (area.top + seq.offset.1) as f32);
        Some(Animation { frames, origin })
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
