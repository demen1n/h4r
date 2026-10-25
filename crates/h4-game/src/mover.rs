//! An actor on the map: where it stands, where it walks, and which animation frame shows.
//!
//! Walking goes through phases like in the game: `prewalk` (setting off) plays once, `walk`
//! loops, and `postwalk` (stopping) plays once as the actor arrives. Standing actors loop
//! `wait` and now and then play `fidget`.
//!
//! While walking, the picture moves in ticks, as in the game: every animation frame the
//! actor jumps by the phase's length divided by its frame count, and stands still between
//! frames. A frame lasts `100 * (1000 / fps) / percent` ms, where `percent` is the walk
//! speed setting (the player's armies walk at 150 by default). Compared with a screen
//! recording of the original: a straight step takes 0.4 s at 150 %, a diagonal 0.6 s.
//! Numbers from the Swift remake's notes on the exe (`ArmyWalk.swift`), checked against the
//! recording, not against our own decompile yet.

use std::collections::VecDeque;
use std::rc::Rc;

use h4_formats::archive::Resources;
use macroquad::prelude::*;

use crate::actor_view::{direction, step_length, Actor, Animation};
use crate::terrain_view::Iso;
use crate::world::Pos;

/// Walk speed of the player's armies in percent: the game's default (the options
/// dialog changes it). `H4_WALK_SPEED` overrides it.
pub const PLAYER_WALK_SPEED: u32 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Wait,
    Fidget,
    Prewalk,
    Walk,
    Postwalk,
}

impl Phase {
    fn action(self) -> &'static str {
        match self {
            Phase::Wait => "wait",
            Phase::Fidget => "fidget",
            Phase::Prewalk => "prewalk",
            Phase::Walk => "walk",
            Phase::Postwalk => "postwalk",
        }
    }

    fn loops(self) -> bool {
        matches!(self, Phase::Wait | Phase::Walk)
    }

    fn moving(self) -> bool {
        matches!(self, Phase::Prewalk | Phase::Walk | Phase::Postwalk)
    }
}

pub struct Mover {
    pub actor: Rc<Actor>,
    /// The tile we stand on, or the one the current step started from.
    pub tile: Pos,
    /// Tiles still to enter; the front one is the current step's target.
    path: VecDeque<Pos>,
    /// Distance covered within the current step.
    along: f32,
    pub facing: &'static str,
    phase: Phase,
    /// Time spent in the current phase (for standing animations), or since the last frame
    /// tick (while walking).
    clock: f32,
    /// Frame of the current walking phase.
    frame: usize,
    /// How far each frame of the current walking phase moves the actor.
    per_frame: f32,
    /// Walk speed in percent.
    speed: u32,
    /// Seconds until the next fidget while standing.
    fidget_in: f32,
}

impl Mover {
    pub fn new(actor: Rc<Actor>, tile: Pos, facing: &'static str) -> Mover {
        let speed = std::env::var("H4_WALK_SPEED").ok().and_then(|s| s.parse().ok()).unwrap_or(PLAYER_WALK_SPEED);
        Mover {
            actor,
            tile,
            path: VecDeque::new(),
            along: 0.0,
            facing,
            phase: Phase::Wait,
            clock: rand::gen_range(0.0, 1.0),
            frame: 0,
            per_frame: 0.0,
            speed: speed.max(1),
            fidget_in: rand::gen_range(3.0, 10.0),
        }
    }

    pub fn is_moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// Walk along `path` (tiles to enter, in order). While already walking, the current step
    /// is finished first and the new path must start from its target.
    pub fn walk(&mut self, path: Vec<Pos>) {
        let current = self.path.pop_front();
        self.path.clear();
        self.path.extend(current);
        self.path.extend(path);
    }

    /// Where a new path should start: the tile we stand on or are stepping into.
    pub fn next_tile(&self) -> Pos {
        self.path.front().copied().unwrap_or(self.tile)
    }

    fn animation(&self, res: &Resources, phase: Phase) -> Option<Rc<Animation>> {
        self.actor.animation(res, phase.action(), self.facing)
    }

    fn frame_count(&self, res: &Resources, phase: Phase) -> usize {
        self.animation(res, phase).map_or(1, |a| a.frames.len().max(1))
    }

    /// Seconds one frame of `phase` lasts at our walk speed: `100 * (1000 / fps) / percent`
    /// milliseconds, in integers like the game.
    fn frame_seconds(&self, phase: Phase) -> f32 {
        let fps = self.actor.model.action(phase.action()).map_or(10, |a| a.frames_per_second.max(1) as u32);
        (100 * (1000 / fps)) as f32 / self.speed as f32 / 1000.0
    }

    /// Distance from here to the end of the path.
    fn remaining(&self) -> f32 {
        let mut from = self.tile;
        let mut total = -self.along;
        for p in &self.path {
            total += step_length(direction(p.col - from.col, p.row - from.row));
            from = *p;
        }
        total
    }

    /// Start a walking phase whose frames together cover `length`.
    fn begin(&mut self, res: &Resources, phase: Phase, length: f32) {
        self.phase = phase;
        self.frame = 0;
        self.per_frame = length / self.frame_count(res, phase) as f32;
    }

    fn stand(&mut self) {
        self.phase = Phase::Wait;
        self.clock = 0.0;
    }

    pub fn update(&mut self, res: &Resources, dt: f32) {
        self.clock += dt;
        if !self.phase.moving() && !self.is_moving() {
            self.update_standing(res, dt);
            return;
        }
        if !self.phase.moving() {
            // a path came in while standing: set off on the next tick
            self.clock = 0.0;
            let length = self.actor.model.prewalk_length as f32;
            self.begin(res, Phase::Prewalk, length);
            if let Some(next) = self.path.front() {
                self.facing = direction(next.col - self.tile.col, next.row - self.tile.row);
            }
        }
        // everything that moves happens on frame ticks
        while self.phase.moving() && self.clock >= self.frame_seconds(self.phase) {
            self.clock -= self.frame_seconds(self.phase);
            self.tick(res);
        }
    }

    /// One frame of a walking phase: move by the frame's share, then show the next frame.
    fn tick(&mut self, res: &Resources) {
        self.along += self.per_frame;
        while let Some(next) = self.path.front().copied() {
            let length = step_length(direction(next.col - self.tile.col, next.row - self.tile.row));
            if self.along < length - 0.01 {
                break;
            }
            self.along -= length;
            self.tile = next;
            self.path.pop_front();
        }
        if let Some(next) = self.path.front() {
            self.facing = direction(next.col - self.tile.col, next.row - self.tile.row);
        } else {
            self.along = 0.0;
        }

        self.frame += 1;
        let frames = self.frame_count(res, self.phase);
        let stopping = self.actor.model.postwalk_length as f32;
        match self.phase {
            Phase::Postwalk if self.remaining() > stopping + 0.01 => {
                // a longer path came in while stopping: keep walking
                let length = self.actor.model.walk_length as f32;
                self.begin(res, Phase::Walk, length);
            }
            Phase::Postwalk if self.frame >= frames => self.stand(),
            Phase::Postwalk => {}
            _ if !self.is_moving() => self.stand(),
            // close enough to stop: the stopping frames cover exactly what is left
            _ if self.remaining() <= stopping + 0.01 => {
                let left = self.remaining();
                self.begin(res, Phase::Postwalk, left);
            }
            Phase::Prewalk if self.frame >= frames => {
                let length = self.actor.model.walk_length as f32;
                self.begin(res, Phase::Walk, length);
            }
            Phase::Walk if self.frame >= frames => self.frame = 0,
            _ => {}
        }
    }

    fn update_standing(&mut self, res: &Resources, dt: f32) {
        let played_once = self.animation(res, self.phase).map_or(true, |a| self.clock >= a.duration());
        match self.phase {
            Phase::Wait => {
                self.fidget_in -= dt;
                if self.fidget_in <= 0.0 && self.actor.model.action("fidget").is_some() {
                    self.phase = Phase::Fidget;
                    self.clock = 0.0;
                }
            }
            Phase::Fidget if played_once => {
                self.fidget_in = rand::gen_range(4.0, 12.0);
                self.stand();
            }
            _ => {}
        }
    }

    /// Map pixel of the feet, between tiles while walking.
    pub fn feet(&self, iso: &Iso) -> Vec2 {
        let here = iso.tile_center(self.tile);
        match self.path.front() {
            Some(next) => {
                let length = step_length(direction(next.col - self.tile.col, next.row - self.tile.row));
                here.lerp(iso.tile_center(*next), (self.along / length).min(1.0))
            }
            None => here,
        }
    }

    /// The animation and frame to draw now.
    pub fn frame(&self, res: &Resources) -> Option<(Rc<Animation>, usize)> {
        let anim = self.animation(res, self.phase).or_else(|| self.animation(res, Phase::Wait))?;
        let n = anim.frames.len();
        if n == 0 {
            return None;
        }
        let i = if self.phase.moving() { self.frame } else { (self.clock * anim.fps) as usize };
        let i = if self.phase.loops() { i % n } else { i.min(n - 1) };
        Some((anim, i))
    }
}
