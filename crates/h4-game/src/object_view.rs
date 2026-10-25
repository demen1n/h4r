//! Stationary adventure map objects (trees, mountains, buildings) from `adv_object.*` models.
//!
//! How the game lays an object out (see `h4_formats::adv_object`):
//! - the image's anchor (layer coordinates + the sequence offset) is the top corner of the
//!   footprint's first cell, which is its topmost cell;
//! - the footprint's x axis goes to the screen's lower left (our `row`), y to the lower right
//!   (our `col`);
//! - the image is cut into 32-pixel columns, each with its own depth, so a hero can stand in
//!   front of one side of a mountain and behind the other.

use std::rc::Rc;

use h4_formats::adv_object::ObjectModel;
use h4_formats::archive::Resources;
use h4_formats::sprite::{Layer, Rect};
use macroquad::prelude::*;

use crate::terrain_view::Iso;
use crate::world::{Pos, World};
use crate::Camera;

/// Width of one depth column of an object image, in pixels.
const COLUMN_WIDTH: i32 = 32;

/// A decoded image set: one texture per animation frame, all covering the same area.
struct Frames {
    textures: Vec<Texture2D>,
    /// The textures' area in anchor coordinates (anchor = top corner of the first cell).
    area: Rect,
}

impl Frames {
    /// `still` layers are drawn under every frame (the `base frame`/`base shadow` of
    /// animated buildings); `frames` are the animation. Returns `None` if all are empty.
    fn build(still: &[&Layer], frames: &[&Layer], offset: (i32, i32)) -> Option<Frames> {
        let area = still.iter().chain(frames).filter(|l| !l.is_empty()).map(|l| l.rect).reduce(|a, b| a.union(&b))?;
        let count = frames.len().max(1);
        let textures = (0..count)
            .map(|i| {
                let mut buf = vec![0u8; 4 * (area.width() * area.height()) as usize];
                for l in still.iter().chain(frames.get(i)) {
                    l.draw_onto(&mut buf, area);
                }
                let t = Texture2D::from_rgba8(area.width() as u16, area.height() as u16, &buf);
                t.set_filter(FilterMode::Nearest);
                t
            })
            .collect();
        let area = Rect {
            left: area.left + offset.0,
            top: area.top + offset.1,
            right: area.right + offset.0,
            bottom: area.bottom + offset.1,
        };
        Some(Frames { textures, area })
    }

    fn texture(&self, time: f32, fps: f32) -> &Texture2D {
        let i = if fps > 0.0 { (time * fps) as usize % self.textures.len() } else { 0 };
        &self.textures[i]
    }
}

/// One object model, ready to draw.
pub struct ObjectKind {
    pub model: ObjectModel,
    image: Option<Frames>,
    shadow: Option<Frames>,
}

impl ObjectKind {
    pub fn load(res: &Resources, name: &str) -> Option<ObjectKind> {
        let model = res
            .read(&format!("adv_object.{name}.h4d"))
            .and_then(|d| ObjectModel::read(&d))
            .map_err(|e| eprintln!("adv_object.{name}: {e}"))
            .ok()?;
        let seq = &model.image;
        let is_shadow = |l: &&Layer| l.name.to_lowercase().contains("shadow");
        let base_image: Vec<&Layer> = seq.base.iter().filter(|l| !is_shadow(l)).collect();
        let base_shadow: Vec<&Layer> = seq.base.iter().filter(is_shadow).collect();
        let image = Frames::build(&base_image, &seq.frames.iter().collect::<Vec<_>>(), seq.offset);
        let shadow = Frames::build(&base_shadow, &seq.shadows.iter().collect::<Vec<_>>(), seq.offset);
        Some(ObjectKind { model, image, shadow })
    }

    /// Map tiles covered by the footprint when the first cell is at `at`, with whether each
    /// one is impassable.
    pub fn cells(&self, at: Pos) -> Vec<(Pos, bool)> {
        let (w, h) = (self.model.width as i32, self.model.height as i32);
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let blocked = self.model.impassable[(y * w + x) as usize];
                out.push((Pos { col: at.col + y, row: at.row + x }, blocked));
            }
        }
        out
    }

    fn fps(&self) -> f32 {
        self.model.frames_per_second as f32
    }
}

/// An object standing on the map.
pub struct Placed {
    pub kind: Rc<ObjectKind>,
    pub at: Pos,
    /// How many pixels the object stands above flat ground (`Relief::object_height`).
    pub lift: f32,
}

impl Placed {
    /// Map pixel of the anchor on flat ground: the top corner of the first footprint cell.
    fn flat_anchor(&self, iso: &Iso) -> Vec2 {
        let c = iso.tile_center(self.at);
        vec2(c.x, c.y - 16.0)
    }

    /// Where the anchor is drawn: lifted with the ground under the object.
    fn anchor(&self, iso: &Iso) -> Vec2 {
        self.flat_anchor(iso) - vec2(0.0, self.lift)
    }

    /// Whether any of the object's picture or shadow is inside the window. A real map has
    /// thousands of objects; skipping the ones off screen keeps the frame rate up.
    pub fn on_screen(&self, camera: &Camera, iso: &Iso) -> bool {
        let anchor = self.anchor(iso);
        let areas = [self.kind.image.as_ref().map(|i| i.area), self.kind.shadow.as_ref().map(|s| s.area)];
        areas.into_iter().flatten().any(|area| {
            let top_left = camera.to_screen(anchor + vec2(area.left as f32, area.top as f32));
            let bottom_right = camera.to_screen(anchor + vec2(area.right as f32, area.bottom as f32));
            bottom_right.x >= 0.0 && bottom_right.y >= 0.0 && top_left.x <= screen_width() && top_left.y <= screen_height()
        })
    }

    /// Flat on the ground, so drawn right after the terrain and before anything upright.
    pub fn draw_shadow(&self, camera: &Camera, iso: &Iso, time: f32) {
        if let Some(s) = &self.kind.shadow {
            let at = camera.to_screen(self.anchor(iso) + vec2(s.area.left as f32, s.area.top as f32));
            let size = vec2(s.area.width() as f32, s.area.height() as f32) * camera.zoom;
            let params = DrawTextureParams { dest_size: Some(size), ..Default::default() };
            draw_texture_ex(s.texture(time, self.kind.fps()), at.x, at.y, WHITE, params);
        }
    }

    /// The object's image as vertical strips, each with the map y it sorts by: whatever has
    /// a larger key is closer to the viewer and is drawn later.
    pub fn strips(&self, iso: &Iso) -> Vec<Strip> {
        let Some(image) = &self.kind.image else { return Vec::new() };
        let flat_y = self.flat_anchor(iso).y;
        let left_column = image.area.left.div_euclid(COLUMN_WIDTH);
        let mut out = Vec::new();
        for (i, &depth) in self.kind.model.column_depths.iter().enumerate() {
            let x0 = ((left_column + i as i32) * COLUMN_WIDTH).max(image.area.left);
            let x1 = ((left_column + i as i32 + 1) * COLUMN_WIDTH).min(image.area.right);
            if x0 >= x1 {
                continue;
            }
            // A depth of -(n + 1) means the column's front is the footprint cell n diagonal
            // steps below the first one, whose centre lies 16 * (n + 1) pixels below the
            // anchor. That is -16 * depth; flat decals have 0 and sort behind everything.
            // The key is taken on flat ground: lifting moves the picture, not its depth.
            let key = flat_y - 16.0 * depth as f32;
            out.push(Strip { x0, x1, key });
        }
        out
    }

    pub fn draw_strip(&self, camera: &Camera, iso: &Iso, strip: &Strip, time: f32) {
        let Some(image) = &self.kind.image else { return };
        let anchor = self.anchor(iso);
        let source = macroquad::math::Rect::new(
            (strip.x0 - image.area.left) as f32,
            0.0,
            (strip.x1 - strip.x0) as f32,
            image.area.height() as f32,
        );
        let at = camera.to_screen(anchor + vec2(strip.x0 as f32, image.area.top as f32));
        let params = DrawTextureParams { dest_size: Some(source.size() * camera.zoom), source: Some(source), ..Default::default() };
        draw_texture_ex(image.texture(time, self.kind.fps()), at.x, at.y, WHITE, params);
    }
}

/// A vertical slice of an object image, `x0..x1` in anchor coordinates.
pub struct Strip {
    x0: i32,
    x1: i32,
    pub key: f32,
}

/// Put an object on the map if all its cells are free (on the map, dry, off the road, not
/// taken), and mark its impassable cells as blocked.
pub fn place(world: &mut World, kind: &Rc<ObjectKind>, at: Pos) -> Option<Placed> {
    let cells = kind.cells(at);
    let free = cells.iter().all(|&(p, _)| world.passable(p) && world.tile(p).road.is_none() && !world.is_taken(p));
    if !free {
        return None;
    }
    for (p, blocked) in cells {
        world.take(p, blocked);
    }
    Some(Placed { kind: kind.clone(), at, lift: 0.0 })
}
