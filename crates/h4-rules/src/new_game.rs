//! Starting a game: the state of a `.h4c` map on day 1.
//!
//! The map file is read by `h4_formats::map`, what its random objects become is decided by
//! [`crate::placement`]; here both are put together into a [`GameState`]: the ground with
//! what blocks it, the objects, the armies and the players.

use h4_formats::map::MapFile;
use h4_formats::map_army::{self, Stack as MapStack};
use h4_formats::map_body::ObjectKind;
use h4_state::army::{Army, Hero, Slot, Stack, SLOTS};
use h4_state::map::{AdventureMap, MapObject, Tile};
use h4_state::{Color, GameState, Player, Pos};

use crate::placement::Placement;

/// Which cells of an object model can't be walked through, as `ObjectModel` stores them
/// (`h4_formats::adv_object`): `width` × `height` flags, index `y * width + x`.
pub struct Footprint {
    pub width: i32,
    pub height: i32,
    pub impassable: Vec<bool>,
}

impl Footprint {
    /// The tiles covered when the first cell is at `at`, each with whether it blocks.
    /// The model's x runs along our rows and y along our columns (checked against the
    /// art in `h4-game`, `H4_FOOTPRINTS=1`).
    pub fn cells(&self, at: Pos) -> impl Iterator<Item = (Pos, bool)> + '_ {
        (0..self.height).flat_map(move |y| {
            (0..self.width).map(move |x| {
                let blocked = self.impassable.get((y * self.width + x) as usize).copied().unwrap_or(false);
                (Pos { level: at.level, row: at.row + x, col: at.col + y }, blocked)
            })
        })
    }
}

/// Maps store 6 for "no owner".
fn owner(color: u8) -> Option<Color> {
    (color < 6).then_some(color)
}

/// The state at the start of `map`.
///
/// - `placements`: what [`crate::placement::place_random_objects`] chose, one entry per
///   map object (an empty list leaves random objects as they are and drops random
///   monsters).
/// - `footprint`: the footprint of an object model by name, `None` if the model can't be
///   loaded. It is a function so the rules don't need to open the game archives
///   themselves: the caller reads `adv_object.<model>` however it likes.
///
/// Objects whose model has no footprint are kept but block nothing.
pub fn from_map(
    map: &MapFile,
    placements: &[Option<Placement>],
    mut footprint: impl FnMut(&str) -> Option<Footprint>,
) -> GameState {
    let players = map
        .header
        .players
        .iter()
        .map(|p| Player { color: p.color, human: p.playable, resources: [0; 7], alive: true })
        .collect();
    let mut state = GameState::new(ground(map), players);

    let Ok(body) = &map.body else { return state };
    for (i, o) in body.objects.iter().enumerate() {
        let at = Pos { level: o.level.clamp(0, u8::MAX as i32) as u8, row: o.x, col: o.y };
        let placement = placements.get(i).and_then(Option::as_ref);
        match &o.kind {
            ObjectKind::Army { owner: color, army, .. } => {
                let army = army_of(army, placement);
                if !army.is_empty() {
                    state.add_army(owner(*color), at, army);
                }
            }
            ObjectKind::Placed(p) if matches!(p.type_name.as_str(), "random_monster" | "random_random_monster") => {
                // a random monster becomes a wandering army, or nothing if no creature fitted
                if let Some(Placement::Monster { stacks }) = placement {
                    let stacks: Vec<Stack> = stacks.iter().map(|&(creature, count)| Stack { creature, count }).collect();
                    state.add_army(None, at, Army::of(&stacks));
                }
            }
            ObjectKind::Placed(p) => {
                let model = placement.and_then(Placement::model).unwrap_or(&p.model);
                if let Some(f) = footprint(model) {
                    let bridge = p.type_name == "bridge";
                    for (cell, blocked) in f.cells(at) {
                        let Some(tile) = state.map.tile_mut(cell) else { continue };
                        // a bridge's cells are its deck: walkable, and over water too
                        if bridge {
                            tile.bridge = true;
                        } else {
                            tile.blocked |= blocked;
                        }
                    }
                }
                state.objects.push(MapObject {
                    model: model.to_string(),
                    type_name: p.type_name.clone(),
                    at,
                    owner: p.owner.and_then(owner),
                    map_index: Some(i),
                });
            }
        }
    }
    state
}

/// Terrain and roads of all levels. Tiles outside the map's diamond are marked as such.
fn ground(map: &MapFile) -> AdventureMap {
    let size = map.header.size as i32;
    let mut ground = AdventureMap::new(size, map.header.levels);
    for level in 0..map.header.levels {
        for row in 0..size {
            for col in 0..size {
                let pos = Pos { level, row, col };
                let stored = map.tile(level as usize, row as usize, col as usize);
                let tile = match stored {
                    Some(t) => Tile { terrain: t.terrain, road: t.road, ..Tile::EMPTY },
                    None => Tile { inside: false, ..Tile::EMPTY },
                };
                *ground.tile_mut(pos).expect("pos is within size × size × levels") = tile;
            }
        }
    }
    ground
}

/// An army as the map stores it, with what [`Placement::Army`] decided for it: classes
/// of random heroes and sizes of stacks left at 0.
fn army_of(stored: &map_army::Army, placement: Option<&Placement>) -> Army {
    let (chosen_heroes, counts): (&[Option<(usize, bool)>], &[(usize, i32)]) = match placement {
        Some(Placement::Army { heroes, counts }) => (heroes, counts),
        _ => (&[], &[]),
    };
    let mut heroes = chosen_heroes.iter();
    let mut army = Army::default();
    for (slot, s) in stored.stacks.iter().enumerate().take(SLOTS) {
        army.slots[slot] = match s {
            MapStack::Empty => None,
            MapStack::Creatures { kind, count, .. } => {
                let count = match counts.iter().find(|(i, _)| *i == slot) {
                    Some(&(_, n)) => n,
                    None => *count as i32,
                };
                // a negative kind would be a random creature; no installed map has one
                match usize::try_from(*kind) {
                    Ok(creature) if count > 0 => Some(Slot::Creatures(Stack { creature, count })),
                    _ => None,
                }
            }
            MapStack::Hero(h) => {
                // the placement has one entry per hero, in this same order
                let chosen = heroes.next().copied().flatten();
                let stored = usize::try_from(h.class).ok().zip(match h.gender {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                });
                // A hero whose class we can't tell yet (a portrait without a class) still
                // goes in, as a knight, so the army isn't lost.
                let (class, male) = chosen.or(stored).unwrap_or((0, true));
                Some(Slot::Hero(Hero {
                    portrait: h.portrait,
                    name: h.name.clone(),
                    class,
                    male,
                    level: 1,
                    experience: 0,
                }))
            }
        };
    }
    army
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_axes() {
        // 2 wide (rows), 3 high (columns), only the last cell blocks
        let f = Footprint { width: 2, height: 3, impassable: vec![false, false, false, false, false, true] };
        let cells: Vec<(Pos, bool)> = f.cells(Pos { level: 0, row: 10, col: 20 }).collect();
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[1], (Pos { level: 0, row: 11, col: 20 }, false));
        assert_eq!(cells[5], (Pos { level: 0, row: 11, col: 22 }, true));
    }

    #[test]
    fn random_heroes_and_counts_fill_the_army() {
        let hero = |class: i8, gender: i8| {
            MapStack::Hero(Box::new(map_army::Hero {
                portrait: -1,
                class,
                gender,
                name: String::new(),
                biography: String::new(),
                equipped: Vec::new(),
                backpack: Vec::new(),
                events: Default::default(),
            }))
        };
        let stored = map_army::Army {
            stacks: vec![
                hero(3, 0),
                MapStack::Creatures { kind: 5, count: 0, artifacts: Vec::new() },
                MapStack::Empty,
                hero(-1, -1),
                MapStack::Creatures { kind: 7, count: 12, artifacts: Vec::new() },
                MapStack::Empty,
                MapStack::Empty,
            ],
        };
        let placement = Placement::Army { heroes: vec![Some((3, false)), Some((9, true))], counts: vec![(1, 40)] };
        let army = army_of(&stored, Some(&placement));
        let heroes: Vec<(usize, bool)> = army.heroes().map(|h| (h.class, h.male)).collect();
        assert_eq!(heroes, [(3, false), (9, true)]);
        let stacks: Vec<(usize, i32)> = army.stacks().map(|s| (s.creature, s.count)).collect();
        assert_eq!(stacks, [(5, 40), (7, 12)]);
        assert!(army.slots[2].is_none());
    }
}
