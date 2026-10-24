//! An actor on the map: where it stands, where it walks, and which animation frame shows.
//!
//! Walking goes through phases like in the game: `prewalk` (setting off) plays once, `walk`
//! loops, and `postwalk` (stopping) plays once as the actor arrives. Each animation moves the
//! actor by its model's length per play, so feet don't slide. Standing actors loop `wait` and
//! now and then play `fidget`.

use std::collections::VecDeque;
use std::rc::Rc;

use h4_formats::archive::Resources;
use macroquad::prelude::*;

use crate::actor_view::{direction, step_length, Actor, Animation};
use crate::terrain_view::Iso;
use crate::world::Pos;

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
    /// Time spent in the current phase.
    clock: f32,
    /// Seconds until the next fidget while standing.
    fidget_in: f32,
}

impl Mover {
    pub fn new(actor: Rc<Actor>, tile: Pos, facing: &'static str) -> Mover {
        Mover {
            actor,
            tile,
            path: VecDeque::new(),
            along: 0.0,
            facing,
            phase: Phase::Wait,
            clock: rand::gen_range(0.0, 1.0),
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
        if current.is_none() && !self.path.is_empty() {
            self.set_phase(Phase::Prewalk);
        }
    }

    /// Where a new path should start: the tile we stand on or are stepping into.
    pub fn next_tile(&self) -> Pos {
        self.path.front().copied().unwrap_or(self.tile)
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.clock = 0.0;
    }

    fn animation(&self, res: &Resources, phase: Phase) -> Option<Rc<Animation>> {
        self.actor.animation(res, phase.action(), self.facing)
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

    pub fn update(&mut self, res: &Resources, dt: f32) {
        self.clock += dt;
        let played_once = self.animation(res, self.phase).map_or(true, |a| self.clock >= a.duration());

        if !self.is_moving() {
            match self.phase {
                Phase::Wait => {
                    self.fidget_in -= dt;
                    if self.fidget_in <= 0.0 && self.actor.model.action("fidget").is_some() {
                        self.set_phase(Phase::Fidget);
                    }
                }
                Phase::Fidget if played_once => {
                    self.fidget_in = rand::gen_range(4.0, 12.0);
                    self.set_phase(Phase::Wait);
                }
                Phase::Fidget => {}
                // arrived (postwalk may still be finishing its last frames)
                _ if played_once => self.set_phase(Phase::Wait),
                _ => {}
            }
            return;
        }

        let Some(next) = self.path.front().copied() else { return };
        self.facing = direction(next.col - self.tile.col, next.row - self.tile.row);
        if matches!(self.phase, Phase::Wait | Phase::Fidget) {
            self.set_phase(Phase::Prewalk);
        }
        if self.phase == Phase::Prewalk && played_once {
            self.set_phase(Phase::Walk);
        }
        let stopping_distance = self.actor.model.postwalk_length as f32;
        if self.phase != Phase::Postwalk && self.remaining() <= stopping_distance {
            self.set_phase(Phase::Postwalk);
        } else if self.phase == Phase::Postwalk && self.remaining() > stopping_distance {
            // a new, longer path came in while stopping: keep walking
            self.set_phase(Phase::Walk);
        }

        self.along += self.actor.speed(res, self.phase.action(), self.facing) * dt;
        while let Some(next) = self.path.front().copied() {
            let length = step_length(direction(next.col - self.tile.col, next.row - self.tile.row));
            if self.along < length {
                break;
            }
            self.along -= length;
            self.tile = next;
            self.path.pop_front();
        }
        if self.path.is_empty() {
            self.along = 0.0;
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
        let i = (self.clock * anim.fps) as usize;
        let i = if self.phase.loops() { i % n } else { i.min(n - 1) };
        Some((anim, i))
    }
}
