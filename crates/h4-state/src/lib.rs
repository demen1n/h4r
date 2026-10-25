//! The state of one game of Heroes IV: everything that changes while it is played.
//!
//! This crate is only data: no rules, no drawing, no file reading. `h4-rules` builds a
//! state from a map and changes it in response to actions; `h4-game` draws it. Keeping
//! the data apart lets both sides (and tests) share it without depending on each other.
//!
//! - [`map`]: the ground (terrain, roads, what blocks movement) and the objects on it.
//! - [`army`]: creature stacks, heroes and armies walking on the map.
//!
//! What's here is the minimum for a first end-to-end scenario (a hero walks, fights a
//! monster, the result is written back). Fields get added as rules need them.

pub mod army;
pub mod map;

use army::{ArmyId, MapArmy};
use map::{AdventureMap, MapObject};

/// Version of the state layout. Saved games will store it, so an old save can be told
/// apart from a new one; raise it whenever a field changes meaning or is added.
pub const VERSION: u32 = 1;

/// Player color as in the game and its maps: 0..=5. Objects with no owner have `None`
/// in our state (the maps store 6 for "nobody").
pub type Color = u8;

/// A place on the map. `row` and `col` are the tile, `level` 0 is the surface and 1 the
/// underground. Column grows towards the screen's lower right, row towards the lower left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub level: u8,
    pub row: i32,
    pub col: i32,
}

/// Gold and the six rare resources, in the game's order (gold, wood, ore, crystal,
/// sulfur, mercury, gems).
pub type Resources = [i32; 7];

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub color: Color,
    /// Whether a person may play this color (the map's "playable" flag). Who actually
    /// plays it is chosen when the game starts; for now a playable color is human.
    pub human: bool,
    pub resources: Resources,
    /// Still in the game: false once the player has lost.
    pub alive: bool,
}

/// The whole game.
#[derive(Debug, Clone, PartialEq)]
pub struct GameState {
    /// [`VERSION`] at the time the state was made.
    pub version: u32,
    /// Day of the game, starting at 1. A week is 7 days, a month 4 weeks.
    pub day: u32,
    /// Index into `players` of whose turn it is.
    pub current_player: usize,
    pub players: Vec<Player>,
    pub map: AdventureMap,
    /// Objects standing on the map that don't walk (towns, mines, trees, chests...).
    pub objects: Vec<MapObject>,
    /// Everything that walks: heroes with their troops and wandering monsters.
    pub armies: Vec<MapArmy>,
    /// The id the next new army gets. Ids are never reused, so an id kept in a log or
    /// in an event still means the same army after others are removed.
    pub next_army_id: u32,
}

impl GameState {
    /// An empty game on `map` with these players, day 1, the first player to move.
    pub fn new(map: AdventureMap, players: Vec<Player>) -> GameState {
        GameState {
            version: VERSION,
            day: 1,
            current_player: 0,
            players,
            map,
            objects: Vec::new(),
            armies: Vec::new(),
            next_army_id: 1,
        }
    }

    /// Week of the month, 1..=4.
    pub fn week(&self) -> u32 {
        (self.day - 1) / 7 % 4 + 1
    }

    /// Month of the game, starting at 1.
    pub fn month(&self) -> u32 {
        (self.day - 1) / 28 + 1
    }

    /// Put a new army on the map and return its id.
    pub fn add_army(&mut self, owner: Option<Color>, at: Pos, army: army::Army) -> ArmyId {
        let id = ArmyId(self.next_army_id);
        self.next_army_id += 1;
        self.armies.push(MapArmy { id, owner, at, army, movement_left: [0.0; army::SLOTS], growth_remainder: [0; army::SLOTS] });
        id
    }

    pub fn army(&self, id: ArmyId) -> Option<&MapArmy> {
        self.armies.iter().find(|a| a.id == id)
    }

    pub fn army_mut(&mut self, id: ArmyId) -> Option<&mut MapArmy> {
        self.armies.iter_mut().find(|a| a.id == id)
    }

    /// The army standing on `at`, if any. Only one army fits on a tile.
    pub fn army_at(&self, at: Pos) -> Option<&MapArmy> {
        self.armies.iter().find(|a| a.at == at)
    }

    /// Take an army off the map (it was defeated, or joined another). Returns it, or
    /// `None` if there was no army with this id.
    pub fn remove_army(&mut self, id: ArmyId) -> Option<MapArmy> {
        let i = self.armies.iter().position(|a| a.id == id)?;
        Some(self.armies.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use army::{Army, Stack};

    fn empty_game() -> GameState {
        GameState::new(AdventureMap::new(8, 1), Vec::new())
    }

    #[test]
    fn calendar() {
        let mut g = empty_game();
        assert_eq!((g.day, g.week(), g.month()), (1, 1, 1));
        g.day = 7;
        assert_eq!((g.week(), g.month()), (1, 1));
        g.day = 8;
        assert_eq!((g.week(), g.month()), (2, 1));
        g.day = 29;
        assert_eq!((g.week(), g.month()), (1, 2));
    }

    #[test]
    fn army_ids_are_not_reused() {
        let mut g = empty_game();
        let at = Pos { level: 0, row: 1, col: 1 };
        let a = g.add_army(None, at, Army::of(&[Stack { creature: 0, count: 5 }]));
        assert_eq!(g.army_at(at).map(|x| x.id), Some(a));
        assert!(g.remove_army(a).is_some());
        assert!(g.army(a).is_none());
        let b = g.add_army(None, at, Army::default());
        assert_ne!(a, b);
    }
}
