//! The game being played: the rules' [`GameState`] and the figures that show its armies.
//!
//! The state is the truth. Walking, battles and turns are done by `h4_rules` on the state
//! at once, and the figures ([`Mover`]) only catch up on screen: when a click comes, the
//! walk is decided and the army is already at its end in the state; its figure then walks
//! the steps, and a battle at the end of the walk is fought when the figure has arrived.
//!
//! What a player can do: pick one of their armies, walk it, attack by walking onto an
//! enemy, end the turn. Players who aren't human have no AI yet and pass their turn at
//! once.

use std::collections::BTreeMap;

use h4_formats::archive::Resources;
use h4_rules::battle::{self, Side};
use h4_rules::creatures::{CreatureStats, KEYWORDS};
use h4_rules::heroes::BASE_CLASSES;
use h4_rules::movement::{self, MoveEvent, Obstacle};
use h4_rules::random::GlobalRandom;
use h4_rules::turns;
use h4_state::army::{Army, ArmyId, Slot};
use h4_state::GameState;

use crate::actor_view::Actors;
use crate::armies::ArmyLooks;
use crate::mover::Mover;
use crate::world::Pos;

/// A tile of the shown level in the state's terms. The view only knows column and row.
fn state_pos(level: u8, p: Pos) -> h4_state::Pos {
    h4_state::Pos { level, row: p.row, col: p.col }
}

/// A tile of the state in the view's terms (the level is the shown one).
pub fn view_pos(p: h4_state::Pos) -> Pos {
    Pos { col: p.col, row: p.row }
}

/// What a click on a tile would do with the selected army.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    Walk,
    Attack,
    Blocked,
}

pub struct Play {
    pub state: GameState,
    creatures: Vec<CreatureStats>,
    /// The game's global generator: it has decided the random objects and now seeds every
    /// battle, as in the game.
    rng: GlobalRandom,
    /// The level on screen; armies on the other one have no figure.
    level: u8,
    /// One figure per army on the shown level. A `BTreeMap` so they are always visited in
    /// the same order.
    figures: BTreeMap<ArmyId, Mover>,
    /// The army the player moves.
    selected: Option<ArmyId>,
    /// A battle that waits for the attacker's figure to arrive: (attacker, defender).
    pending_attack: Option<(ArmyId, ArmyId)>,
    /// The last thing that happened, for the status line.
    pub message: String,
}

impl Play {
    /// Start playing `state` (just made by `new_game::from_map` or by hand), showing
    /// `level`. Armies whose model can't be found stay in the game but aren't drawn.
    pub fn new(
        mut state: GameState,
        creatures: Vec<CreatureStats>,
        rng: GlobalRandom,
        level: u8,
        actors: &Actors,
        looks: &ArmyLooks,
    ) -> Play {
        turns::begin_game(&mut state, &creatures);
        let mut figures = BTreeMap::new();
        for a in state.armies.iter().filter(|a| a.at.level == level) {
            let name = looks.actor(&a.army);
            let Some(actor) = name.as_deref().and_then(|n| actors.get(n)) else {
                eprintln!("army at ({}, {}): no model for {}", a.at.col, a.at.row, describe(&a.army));
                continue;
            };
            // the player's heroes look to the right like in the original, monsters at the viewer
            let facing = if a.owner.is_some() { "e" } else { "sw" };
            figures.insert(a.id, Mover::new(actor, view_pos(a.at), facing));
        }
        let mut play = Play {
            state,
            creatures,
            rng,
            level,
            figures,
            selected: None,
            pending_attack: None,
            message: String::new(),
        };
        if !play.is_human_turn() {
            play.end_turn();
        }
        play.select_next();
        play
    }

    fn is_human_turn(&self) -> bool {
        self.state.players.get(self.state.current_player).is_some_and(|p| p.human)
    }

    /// Something is still being shown (a walk, a battle to come): new orders wait.
    pub fn busy(&self) -> bool {
        self.pending_attack.is_some() || self.figures.values().any(Mover::is_moving)
    }

    pub fn figures(&self) -> impl Iterator<Item = &Mover> {
        self.figures.values()
    }

    pub fn selected_figure(&self) -> Option<&Mover> {
        self.figures.get(&self.selected?)
    }

    /// The current player's armies that have a figure, in the state's order.
    fn own_armies(&self) -> Vec<ArmyId> {
        let Some(player) = self.state.players.get(self.state.current_player) else { return Vec::new() };
        let own = self.state.armies.iter().filter(|a| a.owner == Some(player.color));
        own.map(|a| a.id).filter(|id| self.figures.contains_key(id)).collect()
    }

    /// Select the current player's next army (the first one if none is selected).
    pub fn select_next(&mut self) {
        let own = self.own_armies();
        let next = match self.selected.and_then(|s| own.iter().position(|&a| a == s)) {
            Some(i) => own.get((i + 1) % own.len()),
            None => own.first(),
        };
        self.selected = next.copied();
    }

    /// The selected army walks towards `target` as far as its movement goes today, or up
    /// to an enemy standing there and attacks it.
    pub fn walk_to(&mut self, target: Pos) {
        if self.busy() {
            return;
        }
        let Some(id) = self.selected else { return };
        let to = state_pos(self.level, target);
        let Some(path) = movement::find_path(&self.state, id, to, &self.creatures) else {
            self.message = "no way there".into();
            return;
        };
        let events = match movement::move_army(&mut self.state, id, &path, &self.creatures) {
            Ok(events) => events,
            // the path was just found for this army, so this would be a bug in the rules
            Err(e) => {
                self.message = format!("cannot walk: {e:?}");
                return;
            }
        };
        let mut steps = Vec::new();
        for e in events {
            match e {
                MoveEvent::Stepped { to, .. } => steps.push(view_pos(to)),
                MoveEvent::OutOfMovement { .. } => self.message = "no movement left today".into(),
                MoveEvent::Blocked { .. } => self.message = "the way is blocked".into(),
                MoveEvent::Attack { defender } => self.pending_attack = Some((id, defender)),
            }
        }
        if let Some(figure) = self.figures.get_mut(&id) {
            figure.walk(steps);
        }
    }

    /// Animate the figures, and fight a battle once its attacker has arrived.
    pub fn update(&mut self, res: &Resources, dt: f32) {
        for f in self.figures.values_mut() {
            f.update(res, dt);
        }
        if let Some((attacker, defender)) = self.pending_attack {
            if !self.figures.get(&attacker).is_some_and(Mover::is_moving) {
                self.pending_attack = None;
                self.fight(attacker, defender);
            }
        }
    }

    fn fight(&mut self, attacker: ArmyId, defender: ArmyId) {
        let enemy = self.state.army(defender).map(|a| describe(&a.army)).unwrap_or_default();
        let Some(report) = battle::attack(&mut self.state, attacker, defender, &self.creatures, &mut self.rng) else {
            return;
        };
        self.figures.remove(&report.removed);
        let o = &report.outcome;
        let killed: i32 = o.defender_losses.iter().sum();
        let lost: i32 = o.attacker_losses.iter().sum();
        self.message = match o.winner {
            Side::Attacker => {
                format!("won against {enemy}: killed {killed}, lost {lost}, heroes +{} experience", o.experience)
            }
            Side::Defender => format!("lost against {enemy}: killed {killed}, lost {lost}"),
        };
        if self.selected == Some(report.removed) {
            self.selected = None;
            self.select_next();
        }
    }

    /// The current player ends the turn. The next human player moves next; the others pass
    /// at once, as there is no AI yet.
    pub fn end_turn(&mut self) {
        if self.busy() {
            return;
        }
        // at most one round: if nobody is human, we stop somewhere instead of looping
        for _ in 0..=self.state.players.len() {
            turns::end_turn(&mut self.state, &self.creatures);
            if self.is_human_turn() {
                break;
            }
        }
        self.selected = None;
        self.select_next();
        let color = self.state.players.get(self.state.current_player).map_or(0, |p| p.color);
        self.message = format!("day {}, player {color} to move", self.state.day);
    }

    /// What a click on `p` would do.
    pub fn hover(&self, p: Pos) -> Hover {
        let at = state_pos(self.level, p);
        let Some(army) = self.selected.and_then(|id| self.state.army(id)) else {
            return Hover::Blocked;
        };
        match movement::obstacle(&self.state, army, at) {
            Obstacle::None => Hover::Walk,
            Obstacle::Enemy(_) => Hover::Attack,
            Obstacle::Impassable | Obstacle::Friend(_) => Hover::Blocked,
        }
    }

    /// Who stands on `p`, for the status line.
    pub fn army_at(&self, p: Pos) -> Option<String> {
        let a = self.state.army_at(state_pos(self.level, p))?;
        let owner = a.owner.map_or("neutral".to_string(), |c| format!("player {c}"));
        Some(format!("{owner}: {}", describe(&a.army)))
    }

    /// The day, whose turn it is, and the selected army with its movement.
    pub fn status(&self) -> String {
        let s = &self.state;
        let color = s.players.get(s.current_player).map_or(0, |p| p.color);
        let mut line = format!("day {} (week {}, month {})   player {color}", s.day, s.week(), s.month());
        if let Some(a) = self.selected.and_then(|id| s.army(id)) {
            let left = movement::movement_left(a, &self.creatures);
            let full = movement::max_movement(&a.army, &self.creatures);
            line += &format!("   {}   move {left}/{full}", describe(&a.army));
        }
        line
    }
}

/// An army in words: "knight (exp 0), 20 crossbowman".
fn describe(army: &Army) -> String {
    let members: Vec<String> = army
        .slots
        .iter()
        .flatten()
        .map(|s| match s {
            Slot::Hero(h) => {
                let class = BASE_CLASSES.get(h.class).map_or("hero", |c| c.keyword);
                format!("{class} (exp {})", h.experience)
            }
            Slot::Creatures(c) => format!("{} {}", c.count, KEYWORDS.get(c.creature).unwrap_or(&"?")),
        })
        .collect();
    members.join(", ")
}
