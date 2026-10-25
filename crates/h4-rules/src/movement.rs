//! Armies walking on the adventure map: what a step costs, how much movement an army has,
//! the way to a tile, and the walk itself. All as in the exe, see `decomp/movement.md`.
//!
//! Movement is counted in points; a straight step over grass costs 100. Each member of
//! an army keeps its own movement left, in its own units, and the army can go as far as
//! its slowest member.
//!
//! Not modelled yet: boats and the sea (everything is on land), hero skills and
//! artifacts (every hero has no Pathfinding and no Seamanship), temporary bonuses
//! (stables, wells), monster zones of control and objects triggered on the way.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use h4_state::army::{Army, ArmyId, MapArmy, Slot, SLOTS};
use h4_state::map::{terrain, Tile};
use h4_state::{GameState, Pos};

use crate::creatures::CreatureStats;

/// Number of terrain types (`h4_formats::map::TERRAIN_NAMES`).
pub const TERRAIN_TYPES: usize = 19;

/// Cost of a straight step on each terrain (`get_terrain_cost`, rva `250e90`): 100 unless
/// the table at VA `0x985378` says otherwise.
const TERRAIN_COST: [i32; TERRAIN_TYPES] = [
    100, // water
    100, // grass
    125, // rough
    200, // swamp
    125, // volcanic
    175, // snow
    150, // sand
    100, // dirt
    100, // subterranean
    150, // river
    100, // lava river
    150, // ice river
    100, 100, 100, // magic terrains 12..=14
    125, 125, // 15, 16
    100, // 17
    125, // 18
];

pub fn terrain_cost(terrain: u8) -> i32 {
    TERRAIN_COST.get(terrain as usize).copied().unwrap_or(100)
}

/// The native terrain of each town type (`h4_rules::creatures::TOWN_TYPES` order),
/// table at VA `0x9845e8`: haven grass, academy snow, necropolis volcanic, asylum swamp,
/// preserve grass, stronghold rough.
const NATIVE_TERRAIN: [u8; 6] = [1, 5, 4, 3, 1, 2];

/// Cost of a step for one creature (`t_creature::get_move_cost`, rva `23d520`): on its
/// native terrain 50 less, but never below 100.
pub fn creature_move_cost(creature: &CreatureStats, terrain: u8) -> i32 {
    let cost = terrain_cost(terrain);
    // terrains from 9 up (rivers, magic terrains) are nobody's native terrain
    let native = terrain < 9 && NATIVE_TERRAIN.get(creature.alignment) == Some(&terrain);
    if native && cost > 100 {
        (cost - 50).max(100)
    } else {
        cost
    }
}

/// Cost of a step on `terrain` for the whole army (`t_creature_array::get_move_cost`,
/// rva `23f960`): the slowest creature decides, then any member that helps the army
/// (a hero with Pathfinding) can bring it down. Heroes don't count in the first part.
pub fn army_move_cost(army: &Army, creatures: &[CreatureStats], terrain: u8) -> i32 {
    let slowest_creature = army
        .stacks()
        .filter(|s| s.count > 0)
        .map(|s| match creatures.get(s.creature) {
            Some(c) => creature_move_cost(c, terrain),
            None => terrain_cost(terrain),
        })
        .max();
    let mut cost = slowest_creature.unwrap_or_else(|| terrain_cost(terrain));
    for slot in army.slots.iter().flatten() {
        cost = cost.min(slot_army_move_cost(slot, terrain));
    }
    cost
}

/// How much Pathfinding takes off a step's cost, by mastery (VA `0x98915c`): none,
/// basic, advanced, expert, master, grand master.
const PATHFINDING_REDUCTION: [i32; 6] = [0, 25, 50, 100, 100, 100];

/// Pathfinding mastery of a hero as an index into [`PATHFINDING_REDUCTION`]. Heroes have
/// no skills in the state yet, so it is always "none".
fn pathfinding(_hero: &h4_state::army::Hero) -> usize {
    0
}

/// What one member makes the army pay (`get_army_move_cost`): the terrain cost for
/// creatures; for a hero, less by their Pathfinding, but not below 100 (rva `331ac0`).
fn slot_army_move_cost(slot: &Slot, terrain: u8) -> i32 {
    let cost = terrain_cost(terrain);
    match slot {
        Slot::Hero(h) if cost > 100 => (cost - PATHFINDING_REDUCTION[pathfinding(h)]).max(100),
        _ => cost,
    }
}

/// The army's cost of a straight step on each terrain, worked out once before a search
/// as the game does.
pub fn army_costs(army: &Army, creatures: &[CreatureStats]) -> [i32; TERRAIN_TYPES] {
    let mut costs = [0; TERRAIN_TYPES];
    for (t, c) in costs.iter_mut().enumerate() {
        *c = army_move_cost(army, creatures, t as u8);
    }
    costs
}

/// Cost of the step from `from` to `to` (`t_adventure_path_finder::search_to`, rva
/// `ffe39`): 75 onto a bridge; along a road (both tiles have one) 100 on dirt roads and
/// 75 on gravel and cobblestone; otherwise the army's cost of the terrain. A diagonal
/// step costs half as much again.
pub fn step_cost(costs: &[i32; TERRAIN_TYPES], from: &Tile, to: &Tile, diagonal: bool) -> i32 {
    let straight = if to.bridge {
        75
    } else if from.road != 0 && to.road != 0 {
        if to.road == 2 {
            100
        } else {
            75
        }
    } else {
        costs.get(to.terrain as usize).copied().unwrap_or(100)
    };
    if diagonal {
        straight + straight / 2
    } else {
        straight
    }
}

/// Movement of one member before bonuses (`get_raw_adventure_movement`, rva `250db0`):
/// `m * 11 / 25 + 1200`, at most 3000, where `m` is the combat movement: the creature's
/// Move from `table.creatures` times 100, or 2400 for a hero. A creature we have no
/// stats for counts as having 0 Move.
pub fn raw_movement(slot: &Slot, creatures: &[CreatureStats]) -> i32 {
    let combat_movement = match slot {
        Slot::Creatures(s) => creatures.get(s.creature).map_or(0, |c| c.movement * 100),
        Slot::Hero(_) => 2400,
    };
    (combat_movement * 11 / 25 + 1200).min(3000)
}

/// The army-wide factor `k` that turns a member's own movement into map points
/// (`get_adventure_movement_modifier`, rva `250df0`): `bonus / raw + multiplier`.
/// On land the bonus is 0 for creatures and heroes alike, and the multiplier is the
/// largest of 1.0 and the heroes' Pathfinding multiplier (1.0 without the skill), so
/// for now `k` is always 1. It is kept as a function so the formula has one place.
fn modifier(_army: &Army, slot: &Slot, creatures: &[CreatureStats]) -> f64 {
    let bonus = 0.0_f32;
    let multiplier = 1.0_f32;
    let raw = raw_movement(slot, creatures) as f32;
    // the game divides in x87 precision; f64 is close to it
    bonus as f64 / raw as f64 + multiplier as f64
}

/// The members that count: stacks with creatures and heroes (all heroes are alive for
/// now; knocked-out heroes will be skipped here).
fn living(army: &Army) -> impl Iterator<Item = (usize, &Slot)> {
    army.slots.iter().enumerate().filter_map(|(i, s)| match s {
        Some(Slot::Creatures(c)) if c.count > 0 => Some((i, s.as_ref().expect("matched Some"))),
        Some(Slot::Hero(_)) => Some((i, s.as_ref().expect("matched Some"))),
        _ => None,
    })
}

/// Full movement of the army for a day (`get_max_movement`, rva `241a10`): the smallest
/// `raw * k`, rounded.
pub fn max_movement(army: &Army, creatures: &[CreatureStats]) -> i32 {
    living(army)
        .map(|(_, slot)| {
            let k = modifier(army, slot, creatures) as f32;
            (raw_movement(slot, creatures) as f64 * k as f64 + 0.5) as i32
        })
        .min()
        .unwrap_or(0)
}

/// Movement the army has left (`get_movement`, rva `241940`): the smallest of the
/// members' own movement left times `k`, rounded.
pub fn movement_left(army: &MapArmy, creatures: &[CreatureStats]) -> i32 {
    living(&army.army)
        .map(|(i, slot)| {
            let k = modifier(&army.army, slot, creatures);
            (army.movement_left[i] as f64 * k + 0.5) as i32
        })
        .min()
        .unwrap_or(0)
}

/// Take `cost` points from every member (`change_movement`, rva `241870`): each loses
/// `cost / k` of its own movement, but not below 0.
pub fn spend_movement(army: &mut MapArmy, creatures: &[CreatureStats], cost: i32) {
    let mut left = army.movement_left;
    for (i, slot) in living(&army.army) {
        let k = modifier(&army.army, slot, creatures);
        let after = (left[i] as f64 - cost as f64 / k) as f32;
        left[i] = after.max(0.0);
    }
    army.movement_left = left;
}

/// A new day: every member gets its full movement back (its raw movement, in its own
/// units). Not checked against the exe's `process_new_day` yet.
pub fn refresh_movement(army: &mut MapArmy, creatures: &[CreatureStats]) {
    let mut left = [0.0; SLOTS];
    for (i, slot) in living(&army.army) {
        left[i] = raw_movement(slot, creatures) as f32;
    }
    army.movement_left = left;
}

/// The eight neighbours of a tile; odd indices are the diagonals, as in the game's
/// direction enum (the cost rule checks `direction & 1`).
const DIRECTIONS: [(i32, i32); 8] = [(-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1)];

/// Whether a land army can stand on this tile: inside the map, not under an object,
/// and not water unless a bridge crosses it.
fn walkable(tile: &Tile) -> bool {
    tile.inside && !tile.blocked && (tile.terrain != terrain::WATER || tile.bridge)
}

/// What is in the way of `army` stepping onto `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obstacle {
    /// Nothing: the step can be made.
    None,
    /// Off the map, an object, water.
    Impassable,
    /// An army of the same owner (meeting it would merge or trade, not done yet).
    Friend(ArmyId),
    /// An army of someone else, or neutral monsters: stepping there means a fight.
    Enemy(ArmyId),
}

pub fn obstacle(state: &GameState, army: &MapArmy, at: Pos) -> Obstacle {
    match state.map.tile(at) {
        Some(t) if walkable(t) => {}
        _ => return Obstacle::Impassable,
    }
    match state.army_at(at) {
        Some(other) if other.id == army.id => Obstacle::None,
        Some(other) if other.owner == army.owner => Obstacle::Friend(other.id),
        Some(other) => Obstacle::Enemy(other.id),
        None => Obstacle::None,
    }
}

/// The cheapest way for army `id` to `to`, as the tiles to step on (without the start).
/// Ends on an enemy army if `to` holds one (that last step is the attack). `None` if
/// there is no way. Movement left doesn't matter: the path may take several days.
///
/// A plain shortest-path search (Dijkstra) on the step costs. The game's search also
/// weighs days and its own order of neighbours, so on ties it may pick a different but
/// equally cheap path; that is still to be compared.
pub fn find_path(state: &GameState, id: ArmyId, to: Pos, creatures: &[CreatureStats]) -> Option<Vec<Pos>> {
    let army = state.army(id)?;
    let from = army.at;
    if from.level != to.level || !state.map.contains(to) {
        return None;
    }
    if from == to {
        return Some(Vec::new());
    }
    let costs = army_costs(&army.army, creatures);
    let size = state.map.size;
    let index = |p: Pos| (p.row * size + p.col) as usize;
    let n = (size * size) as usize;
    let mut best = vec![i64::MAX; n];
    let mut came_from: Vec<Option<Pos>> = vec![None; n];
    // the heap gives the smallest total first; the tile position breaks ties so the
    // result doesn't depend on how the heap orders equal entries
    let mut queue = BinaryHeap::new();
    best[index(from)] = 0;
    queue.push(Reverse((0_i64, from.row, from.col)));
    while let Some(Reverse((total, row, col))) = queue.pop() {
        let here = Pos { level: from.level, row, col };
        if here == to {
            break;
        }
        if total > best[index(here)] {
            continue; // an outdated entry: a cheaper way here was found meanwhile
        }
        // nobody walks through an army: an enemy tile can only be the last step
        if here != from && state.army_at(here).is_some() {
            continue;
        }
        let tile = state.map.tile(here).expect("only tiles on the map are queued");
        for (dir, &(dr, dc)) in DIRECTIONS.iter().enumerate() {
            let next = Pos { level: from.level, row: row + dr, col: col + dc };
            let entering = match obstacle(state, army, next) {
                Obstacle::None => true,
                Obstacle::Enemy(_) => next == to,
                Obstacle::Impassable | Obstacle::Friend(_) => false,
            };
            if !entering {
                continue;
            }
            let next_tile = state.map.tile(next).expect("obstacle() checked it is on the map");
            let total = total + step_cost(&costs, tile, next_tile, dir % 2 == 1) as i64;
            if total < best[index(next)] {
                best[index(next)] = total;
                came_from[index(next)] = Some(here);
                queue.push(Reverse((total, next.row, next.col)));
            }
        }
    }
    came_from[index(to)]?;
    let mut path = vec![to];
    let mut at = to;
    while let Some(prev) = came_from[index(at)] {
        if prev == from {
            break;
        }
        path.push(prev);
        at = prev;
    }
    path.reverse();
    Some(path)
}

/// What happened on a walk, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveEvent {
    /// The army stepped onto `to`, paying `cost` points.
    Stepped { to: Pos, cost: i32 },
    /// The army stopped before `at` because it has no movement left.
    OutOfMovement { at: Pos },
    /// The next tile can't be entered (the map changed since the path was found).
    Blocked { at: Pos },
    /// The next tile holds an army that isn't ours: the walk ends and a battle should
    /// follow. The army stays where it is.
    Attack { defender: ArmyId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    NoSuchArmy(ArmyId),
    /// Two tiles of the path (or the start and the first) don't touch.
    NotAdjacent { from: Pos, to: Pos },
}

/// Walk army `id` along `path` as far as its movement goes.
///
/// A step can always be made while there is any movement left, even if it costs more
/// than that; it then uses up all of it (`decomp/movement.md`). The walk ends early at an
/// enemy army ([`MoveEvent::Attack`]), at something in the way, or when movement runs
/// out.
pub fn move_army(state: &mut GameState, id: ArmyId, path: &[Pos], creatures: &[CreatureStats]) -> Result<Vec<MoveEvent>, MoveError> {
    let mut events = Vec::new();
    let costs = army_costs(&state.army(id).ok_or(MoveError::NoSuchArmy(id))?.army, creatures);
    for &next in path {
        let army = state.army(id).ok_or(MoveError::NoSuchArmy(id))?;
        let here = army.at;
        let (dr, dc) = (next.row - here.row, next.col - here.col);
        if next.level != here.level || dr.abs() > 1 || dc.abs() > 1 || (dr, dc) == (0, 0) {
            return Err(MoveError::NotAdjacent { from: here, to: next });
        }
        match obstacle(state, army, next) {
            Obstacle::None => {}
            Obstacle::Enemy(defender) => {
                events.push(MoveEvent::Attack { defender });
                break;
            }
            Obstacle::Impassable | Obstacle::Friend(_) => {
                events.push(MoveEvent::Blocked { at: next });
                break;
            }
        }
        if movement_left(army, creatures) <= 0 {
            events.push(MoveEvent::OutOfMovement { at: next });
            break;
        }
        let from_tile = state.map.tile(here).copied().unwrap_or(Tile::EMPTY);
        let to_tile = state.map.tile(next).copied().expect("obstacle() checked it is on the map");
        let cost = step_cost(&costs, &from_tile, &to_tile, dr != 0 && dc != 0);
        let army = state.army_mut(id).expect("looked up above");
        spend_movement(army, creatures, cost);
        army.at = next;
        events.push(MoveEvent::Stepped { to: next, cost });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use h4_state::army::{Hero, Stack};
    use h4_state::map::AdventureMap;

    /// A creature with the given Move and town type, the rest doesn't matter here.
    fn creature(movement: i32, alignment: usize) -> CreatureStats {
        CreatureStats {
            keyword: String::new(),
            name: String::new(),
            level: 1,
            hit_points: 10,
            damage_low: 1,
            damage_high: 2,
            attack: 5,
            defense: 5,
            speed: 5,
            movement,
            shots: 0,
            weekly_growth: 10,
            alignment,
            experience: 5,
        }
    }

    fn hero() -> Slot {
        Slot::Hero(Hero { portrait: -1, name: String::new(), class: 0, male: true, level: 1, experience: 0 })
    }

    const SWAMP: u8 = 3;
    const SNOW: u8 = 5;

    #[test]
    fn native_terrain_halves_the_penalty() {
        let academy = creature(20, 1); // native to snow
        let haven = creature(20, 0);
        assert_eq!(creature_move_cost(&academy, SNOW), 125);
        assert_eq!(creature_move_cost(&haven, SNOW), 175);
        // grass costs 100 anyway: being native doesn't make it cheaper
        assert_eq!(creature_move_cost(&haven, 1), 100);
    }

    #[test]
    fn slowest_creature_sets_the_army_cost() {
        let stats = vec![creature(20, 1), creature(20, 0)];
        let army = Army::of(&[Stack { creature: 0, count: 3 }, Stack { creature: 1, count: 3 }]);
        assert_eq!(army_move_cost(&army, &stats, SNOW), 175);
        let natives = Army::of(&[Stack { creature: 0, count: 3 }]);
        assert_eq!(army_move_cost(&natives, &stats, SNOW), 125);
        // a lone hero pays the plain terrain cost
        let mut alone = Army::default();
        alone.slots[0] = Some(hero());
        assert_eq!(army_move_cost(&alone, &stats, SWAMP), 200);
    }

    #[test]
    fn steps() {
        let costs = [100; TERRAIN_TYPES];
        let grass = Tile::EMPTY;
        let gravel = Tile { road: 1, ..Tile::EMPTY };
        let dirt_road = Tile { road: 2, ..Tile::EMPTY };
        assert_eq!(step_cost(&costs, &grass, &grass, false), 100);
        assert_eq!(step_cost(&costs, &grass, &grass, true), 150);
        // a road counts only if the step starts on one too
        assert_eq!(step_cost(&costs, &grass, &gravel, false), 100);
        assert_eq!(step_cost(&costs, &gravel, &gravel, false), 75);
        assert_eq!(step_cost(&costs, &gravel, &gravel, true), 112);
        assert_eq!(step_cost(&costs, &gravel, &dirt_road, false), 100);
        assert_eq!(step_cost(&costs, &grass, &Tile { bridge: true, ..Tile::EMPTY }, false), 75);
    }

    #[test]
    fn raw_movement_of_creatures_and_heroes() {
        let stats = vec![creature(27, 3), creature(80, 3)];
        let bandits = Slot::Creatures(Stack { creature: 0, count: 1 });
        // 2700 * 11 / 25 + 1200
        assert_eq!(raw_movement(&bandits, &stats), 2388);
        // capped
        assert_eq!(raw_movement(&Slot::Creatures(Stack { creature: 1, count: 1 }), &stats), 3000);
        assert_eq!(raw_movement(&hero(), &stats), 2256);
    }

    /// A 10×10 grass map with one army of the given stacks at (5, 5).
    fn game_with(army: Army) -> (GameState, ArmyId) {
        let mut g = GameState::new(AdventureMap::new(10, 1), Vec::new());
        let id = g.add_army(Some(0), Pos { level: 0, row: 5, col: 5 }, army);
        (g, id)
    }

    #[test]
    fn slowest_member_limits_the_army() {
        let stats = vec![creature(27, 3), creature(10, 3)];
        let mut army = Army::of(&[Stack { creature: 0, count: 5 }, Stack { creature: 1, count: 5 }]);
        army.slots[3] = Some(hero());
        assert_eq!(max_movement(&army, &stats), 1640); // 1000 * 11 / 25 + 1200
        let (mut g, id) = game_with(army);
        refresh_movement(g.army_mut(id).unwrap(), &stats);
        assert_eq!(movement_left(g.army(id).unwrap(), &stats), 1640);
        spend_movement(g.army_mut(id).unwrap(), &stats, 150);
        assert_eq!(movement_left(g.army(id).unwrap(), &stats), 1490);
        spend_movement(g.army_mut(id).unwrap(), &stats, 5000);
        assert_eq!(movement_left(g.army(id).unwrap(), &stats), 0);
    }

    #[test]
    fn last_step_uses_up_what_is_left() {
        let stats = vec![creature(0, 3)]; // raw movement 1200
        let (mut g, id) = game_with(Army::of(&[Stack { creature: 0, count: 1 }]));
        refresh_movement(g.army_mut(id).unwrap(), &stats);
        // back and forth between two grass tiles, 100 a step
        let back_and_forth: Vec<Pos> =
            (0..20).map(|i| Pos { level: 0, row: 5, col: if i % 2 == 0 { 6 } else { 5 } }).collect();
        let events = move_army(&mut g, id, &back_and_forth, &stats).unwrap();
        let walked = events.iter().filter(|e| matches!(e, MoveEvent::Stepped { .. })).count();
        assert_eq!(walked, 12);
        assert!(matches!(events.last(), Some(MoveEvent::OutOfMovement { .. })));
        assert_eq!(movement_left(g.army(id).unwrap(), &stats), 0);

        // with 50 left a step of 100 can still be made, and it takes the rest
        refresh_movement(g.army_mut(id).unwrap(), &stats);
        spend_movement(g.army_mut(id).unwrap(), &stats, 1150);
        let here = g.army(id).unwrap().at;
        let two = [Pos { col: here.col + 1, ..here }, Pos { col: here.col + 2, ..here }];
        let events = move_army(&mut g, id, &two, &stats).unwrap();
        assert_eq!(events[0], MoveEvent::Stepped { to: two[0], cost: 100 });
        assert_eq!(events[1], MoveEvent::OutOfMovement { at: two[1] });
    }

    #[test]
    fn paths_go_around_objects_and_end_on_enemies() {
        let stats = vec![creature(20, 3)];
        let (mut g, id) = game_with(Army::of(&[Stack { creature: 0, count: 1 }]));
        // a wall in column 6, rows 3..=7
        for row in 3..=7 {
            g.map.tile_mut(Pos { level: 0, row, col: 6 }).unwrap().blocked = true;
        }
        let to = Pos { level: 0, row: 5, col: 7 };
        let path = find_path(&g, id, to, &stats).unwrap();
        assert_eq!(path.last(), Some(&to));
        assert!(path.iter().all(|p| p.col != 6 || !(3..=7).contains(&p.row)));

        // a monster on the target: the path ends there, the walk stops in front of it
        let monster = g.add_army(None, to, Army::of(&[Stack { creature: 0, count: 9 }]));
        let path = find_path(&g, id, to, &stats).unwrap();
        assert_eq!(path.last(), Some(&to));
        refresh_movement(g.army_mut(id).unwrap(), &stats);
        let events = move_army(&mut g, id, &path, &stats).unwrap();
        assert_eq!(events.last(), Some(&MoveEvent::Attack { defender: monster }));
        assert_ne!(g.army(id).unwrap().at, to);
        // and nobody walks through it
        g.map.tile_mut(Pos { level: 0, row: 5, col: 8 }).unwrap().blocked = true;
        let beyond = Pos { level: 0, row: 5, col: 9 };
        let around = find_path(&g, id, beyond, &stats).unwrap();
        assert!(!around.contains(&to));
    }
}
