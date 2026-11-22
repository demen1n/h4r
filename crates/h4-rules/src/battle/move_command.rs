//! The move command on a [`BattleState`]: build the inputs of the path finder from the
//! state, run it and walk the active unit toward a cell.
//!
//! The steps, each a port checked against the exe on its own
//! (`decomp/combat_path.md`):
//! 1. the blockage map and the template of the unit's footprint size
//!    ([`crate::battle::path_template`]) from the field: scenery and blocked cells;
//! 2. the preparation for the unit ([`crate::battle::path_prepare`]): the other units
//!    as objects of the battlefield's list, the start, the entry costs;
//! 3. the search ([`crate::battle::path`]);
//! 4. where the unit stops (`16c4a0`): going back from the target toward the start, the
//!    first point of the path the movement pays for and that isn't occupied;
//! 5. the move itself (`16c300`): the unit stands there, and the path cost of that point
//!    becomes the "moved this turn" distance (`t_battlefield+0x314`, which `charging` and
//!    the target's first strike read — see `MeleeContext::moved`).
//!
//! Step 4 (and the turns to reach, [`turns_to_reach`], `171340`) is verified by executing
//! the original code (`tests/combat_path_stop_vectors.tsv`); step 5 is read from the
//! disassembly.
//!
//! What the state doesn't know yet stays the caller's input, in [`MoveRules`]:
//! - the movement points (virtual `+0x18` of the creature's stats; how the exe gets them
//!   from the table's `Move` isn't found yet);
//! - flying, teleport, `long_weapon` and effect 9 (the abilities and effects of a stack);
//! - the combat terrain map and the creature's terrain costs (#182);
//! - the zone-of-control directions (`172210`), a closure for [`move_unit`]:
//!   [`crate::battle::zone_of_control::zones_for`] builds it, with facings from the caller.
//!
//! The melee order with an approach ([`approach_and_attack`], `16b850`) builds on the
//! same steps; the cell to strike from must be one of the target's attack positions
//! ([`attack_positions_against`], `10c870`).
//!
//! Not supported yet: castles (gates, walls and towers are not in the state), a start in a
//! tower, cowardice and effect 0x89 (they matter only when the path is searched for an
//! attack, which a plain move doesn't do).

use crate::battle::attack_angles::{attack_positions, AngleAttacker, AngleTarget, AttackPosition};
use crate::battle::facing;
use crate::battle::melee_command::{melee_attack, MeleeContext, MeleeReport};
use crate::battle::path::{flags, Mover, PathSearch};
use crate::battle::path_prepare::{prepare, FieldObject, FieldObjectKind, GateState, PrepareInput, PreparedMover};
use crate::battle::path_template::{template, BlockageMap, Blocker, BlockerKind};
use crate::battle::state::{BattleState, BattleUnit, UnitId};
use crate::battlefield::{BattlefieldGrid, FLAG_BLOCKED};

/// What the rules know about the moving unit beyond the state.
#[derive(Clone, Copy)]
pub struct MoveRules<'a> {
    /// Movement points (virtual `+0x18` of the stats). A plain straight step costs 100.
    pub movement: i32,
    /// Flies for the path search: no effect 7, and ability `flying` or effect 0x2e.
    pub flies: bool,
    /// Ability 0x4a `teleport`.
    pub teleports: bool,
    /// Ability 0x25 `long_weapon`.
    pub long_weapon: bool,
    /// Effect 9: the unit can't move.
    pub held: bool,
    /// Terrain bits of a cell after the terrain refresh (#182).
    pub terrain: &'a dyn Fn((i32, i32)) -> u32,
    /// The unit's terrain cost list: (terrain bits, entry cost) (#182).
    pub costs: &'a [(u32, u16)],
}

fn no_terrain(_: (i32, i32)) -> u32 {
    0
}

impl MoveRules<'static> {
    /// A walker without abilities on plain ground: every cell costs 100 to enter.
    pub fn plain(movement: i32) -> Self {
        MoveRules {
            movement,
            flies: false,
            teleports: false,
            long_weapon: false,
            held: false,
            terrain: &no_terrain,
            costs: &[],
        }
    }
}

/// What a move did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveReport {
    pub unit: UnitId,
    /// Footprint corners before and after.
    pub from: (i32, i32),
    pub to: (i32, i32),
    /// The cells walked, in walking order, the start excluded and `to` last.
    pub path: Vec<(i32, i32)>,
    /// Path cost of `to`: what the exe keeps as "moved this turn".
    pub cost: u32,
}

/// Columns `start..end` of every row of the field.
pub fn path_rows(field: &BattlefieldGrid) -> Vec<(i32, i32)> {
    (0..field.row_count() as i32)
        .map(|x| field.row_bounds(x).expect("x is a row index"))
        .collect()
}

/// The scenery of the field as blockers of the blockage map. The grid keeps occupants
/// per cell, not objects with a footprint, so every scenery cell becomes a blocker of
/// size 1: the exe marks the same cells (a footprint of scenery always assigns the same
/// value, so the order doesn't matter). Units are not scenery: they are prepared per
/// mover. Castle parts are not in the state yet.
pub fn scenery_blockers(field: &BattlefieldGrid, units: &[BattleUnit]) -> Vec<Blocker> {
    let is_unit = |id: u32| units.iter().any(|u| u.id.0 == id);
    let mut blockers = Vec::new();
    for x in 0..field.row_count() as i32 {
        let (start, end) = field.row_bounds(x).expect("x is a row index");
        for y in start..end {
            let cell = field.cell(x, y).expect("inside the row");
            for occupant in cell.occupants.iter().filter(|o| !is_unit(o.id)) {
                blockers.push(Blocker {
                    kind: BlockerKind::Object { blocks: occupant.blocks, fixed: true },
                    position: (x * 16, y * 16),
                    size: 1,
                });
            }
        }
    }
    blockers
}

/// Prepare and run the path search for a unit (`20c7f0` + `20d200`), not in attack mode.
/// `zone` gives the zone-of-control directions of a cell
/// ([`crate::battle::zone_of_control::zones_for`]).
pub fn search_for(
    state: &BattleState,
    id: UnitId,
    rules: &MoveRules,
    zone: impl FnMut((i32, i32)) -> [bool; 8],
) -> Result<PathSearch, String> {
    let unit = state.unit(id).ok_or("unknown battle unit")?;
    let position = unit.position.ok_or("the unit is not on the field")?;
    let field = state.field();
    let rows = path_rows(field);
    let blockage = BlockageMap::build(&rows, &scenery_blockers(field, state.units()), |x, y| {
        field.cell(x, y).is_some_and(|c| c.surface.flags & FLAG_BLOCKED != 0)
    });
    let base = template(&rows, &blockage, position.size);

    // Every unit on the field is an object of the battlefield's list. A dead stack's body
    // answers "no" to the exe's `+0x10` and blocks nothing.
    let objects: Vec<FieldObject> = state
        .units()
        .iter()
        .filter_map(|other| {
            let at = other.position?;
            Some(FieldObject {
                kind: FieldObjectKind::Creature {
                    hostile: other.side != unit.side,
                    cowardly: false,
                    effect_89: false,
                    tower: None,
                },
                is_mover: other.id == id,
                alive: other.alive(),
                fixed: false,
                skip: false,
                size: i32::from(at.size),
                position: (at.at.0 * 16, at.at.1 * 16),
            })
        })
        .collect();

    let mover = PreparedMover {
        position: (position.at.0 * 16, position.at.1 * 16),
        size: i32::from(position.size),
        tower: None,
        teleports: rules.teleports,
        flies: rules.flies,
        defender: unit.side == crate::battle::Side::Defender,
    };
    let mut search = prepare(&PrepareInput {
        template: &base,
        mover,
        objects: &objects,
        gate: GateState::default(),
        attack_mode: false,
        terrain: rules.terrain,
        costs: rules.costs,
    })
    .search;
    search.run(
        &Mover {
            movement: rules.movement,
            footprint_size: i32::from(position.size),
            long_weapon: rules.long_weapon,
            flies: rules.flies,
            held: rules.held,
            teleports: rules.teleports,
        },
        zone,
    );
    Ok(search)
}

/// Where a unit heading for `target` stops this turn (`16c4a0`): the path from the
/// target back to the start, and in it the first point with a cost within `movement`
/// (compared unsigned) that isn't occupied. If no point qualifies the exe answers the
/// target itself; that is kept here. `None` if the target isn't reached at all or is
/// the start.
pub fn destination(search: &PathSearch, target: (i32, i32), movement: i32) -> Option<(i32, i32)> {
    let path = search.path_to(target)?;
    let fits = |at: &(i32, i32)| {
        search
            .map
            .cell(*at)
            .is_some_and(|c| c.cost <= movement as u32 && c.flags & flags::OCCUPIED == 0)
    };
    Some(path.iter().copied().find(fits).unwrap_or(path[0]))
}

/// In how many turns a unit standing with its corner at `from` gets to `target`
/// (`171340`, used by the AI): 1 if it is there already or the movement pays for the way;
/// otherwise the turns of repeated stops (`16c4a0`), each turn allowed `movement` more
/// than the cost of the previous stop, up to 4. A stop of the target itself ends the count
/// (also the exe's fallback when no point qualifies). -1 if there is no path.
pub fn turns_to_reach(search: &PathSearch, from: (i32, i32), target: (i32, i32), movement: i32) -> i32 {
    if from == target {
        return 1;
    }
    let Some(path) = search.path_to(target) else { return -1 };
    let mut turns = 0;
    let mut spent: i32 = 0;
    loop {
        turns += 1;
        let allowed = spent.wrapping_add(movement) as u32;
        let stop = path
            .iter()
            .position(|at| search.map.cell(*at).is_some_and(|c| c.cost <= allowed && c.flags & flags::OCCUPIED == 0))
            .unwrap_or(0);
        spent = search.map.cell(path[stop]).expect("a path point is on the map").cost as i32;
        if stop == 0 || turns >= 4 {
            return turns;
        }
    }
}

/// Walk the active unit toward `target` (a footprint corner): as far as its movement goes
/// this turn, as the exe does. The unit's turn stays active (it may still attack — not
/// connected yet). Fails, changing nothing, if the target can't be reached, if the
/// stopping point is beyond the movement or occupied (the exe's fallback above), or if
/// it isn't the unit's turn.
pub fn move_unit(
    state: &mut BattleState,
    id: UnitId,
    target: (i32, i32),
    rules: &MoveRules,
    zone: impl FnMut((i32, i32)) -> [bool; 8],
) -> Result<MoveReport, String> {
    if state.turns().active != Some(id) {
        return Err("only the unit whose turn it is can move".into());
    }
    let search = search_for(state, id, rules, zone)?;
    walk(state, id, &search, target, rules)
}

/// What [`approach_and_attack`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttackOrder {
    /// The attacker struck: after walking to the attack position (`walked`), or right
    /// where it stood (`None`).
    Attacked { walked: Option<MoveReport>, melee: MeleeReport },
    /// The attack position was beyond the movement: the attacker only walked toward it,
    /// as far as it could, and didn't strike.
    Approached(MoveReport),
}

/// The player's melee order with an approach (`16b850`): the active unit is to strike
/// `target` from the footprint corner `stand_at`. The path cost of `stand_at` decides:
/// - 0 (the unit stands there already): strike in place; "moved" is 0, because the exe
///   clears it when a unit's turn begins (`167db4`);
/// - more than the movement: walk toward it as far as the movement goes and don't strike
///   (`16c590`, the same stop as [`move_unit`]);
/// - otherwise: walk there (`16c300`; "moved" = that cost), then strike.
///
/// `stand_at` must be one of the target's attack positions ([`attack_positions_against`]):
/// the exe's interface and AI only offer those. `melee` gives the rest of the strike's inputs; its `moved` is replaced. The turn stays
/// active either way. A cell the search didn't reach is an error (the exe would read a
/// zero cost from the template and strike in place).
pub fn approach_and_attack(
    state: &mut BattleState,
    attacker: UnitId,
    target: UnitId,
    stand_at: (i32, i32),
    rules: &MoveRules,
    zone: impl FnMut((i32, i32)) -> [bool; 8],
    melee: &MeleeContext,
) -> Result<AttackOrder, String> {
    if state.turns().active != Some(attacker) {
        return Err("only the unit whose turn it is can attack".into());
    }
    let search = search_for(state, attacker, rules, zone)?;
    if !positions_on(state, &search, attacker, target, rules)?.iter().any(|p| p.at == stand_at) {
        return Err("not an attack position against this target".into());
    }
    let cell = *search.map.cell(stand_at).ok_or("the attack position is off the field")?;
    if cell.flags & flags::REACHED == 0 {
        return Err("the attack position cannot be reached".into());
    }
    let strike = |state: &mut BattleState, moved: u32| {
        let context = MeleeContext { moved: moved as i32, ..melee.clone() };
        melee_attack(state, attacker, target, &context)
    };
    if cell.cost < 1 {
        let melee = strike(state, 0)?;
        return Ok(AttackOrder::Attacked { walked: None, melee });
    }
    if rules.movement < 0 || cell.cost > rules.movement as u32 {
        let report = walk(state, attacker, &search, stand_at, rules)?;
        return Ok(AttackOrder::Approached(report));
    }
    // Strike only if the walk worked: on a failed attack the walk is undone.
    let before = state.clone();
    let report = walk_to(state, attacker, &search, stand_at, rules)?;
    match strike(state, report.cost) {
        Ok(melee) => Ok(AttackOrder::Attacked { walked: Some(report), melee }),
        Err(error) => {
            *state = before;
            Err(error)
        }
    }
}

/// Walk toward `target` on a finished search: the stop of `16c4a0`, then the move.
fn walk(state: &mut BattleState, id: UnitId, search: &PathSearch, target: (i32, i32), rules: &MoveRules) -> Result<MoveReport, String> {
    let movement = rules.movement;
    let to = destination(search, target, movement).ok_or("the target cannot be reached")?;
    let cell = search.map.cell(to).expect("a path point is on the map");
    if cell.cost > movement as u32 || cell.flags & flags::OCCUPIED != 0 {
        return Err("no point of the path can be reached this turn".into());
    }
    walk_to(state, id, search, to, rules)
}

/// Move to a reached cell of a finished search (`16c300`). The unit ends up facing the
/// way it went: a walker the direction of its last step (the walk animation turns it
/// at every step to the direction in the path point, `3def70`); a flier or a teleporter
/// straight from where it stood to where it lands (`t_play_combat_flight` `3dd6f0` with
/// `11ee50`, the teleport `47e2a0` with `11ede0`; [`facing::straight`]). Both points are
/// footprint corners in 1/16 of a cell: the creature's position `+0xc` and the landing
/// point `16bad0` builds as cell · 16. A landing in a castle tower (`1f6610`) would
/// shift the point; towers aren't moved into here.
fn walk_to(state: &mut BattleState, id: UnitId, search: &PathSearch, to: (i32, i32), rules: &MoveRules) -> Result<MoveReport, String> {
    let from = state
        .unit(id)
        .and_then(|u| u.position)
        .ok_or("the unit is not on the field")?
        .at;
    let cost = search.map.cell(to).ok_or("the target is off the field")?.cost;
    let mut path = search.path_to(to).ok_or("the target cannot be reached")?;
    path.reverse();
    let facing = if rules.flies || rules.teleports {
        facing::straight((from.0 * 16, from.1 * 16), (to.0 * 16, to.1 * 16))
    } else {
        search.map.cell(to).expect("checked above").direction() as u8
    };
    state.relocate(id, to, cost, facing)?;
    Ok(MoveReport { unit: id, from, to, path, cost })
}

/// The places the active unit may strike `target` from (`10c870` on the unit's path
/// search without attack mode, as the interface asks for them): sorted by angle, each
/// with its corner, distance and path cost. A creature target is a single part, and its
/// virtual `+0x3c` is false (`f140`). Effects 0x90 and 0x89 aren't in the state yet.
pub fn attack_positions_against(
    state: &BattleState,
    attacker: UnitId,
    target: UnitId,
    rules: &MoveRules,
    zone: impl FnMut((i32, i32)) -> [bool; 8],
) -> Result<Vec<AttackPosition>, String> {
    let search = search_for(state, attacker, rules, zone)?;
    positions_on(state, &search, attacker, target, rules)
}

fn positions_on(
    state: &BattleState,
    search: &PathSearch,
    attacker: UnitId,
    target: UnitId,
    rules: &MoveRules,
) -> Result<Vec<AttackPosition>, String> {
    let size = |id: UnitId| state.unit(id).and_then(|u| u.position).ok_or("the unit is not on the field");
    let (own, other) = (size(attacker)?, size(target)?);
    Ok(attack_positions(
        &search.map,
        AngleAttacker {
            size: i32::from(own.size),
            movement: rules.movement,
            long_weapon: rules.long_weapon,
            effect_90: false,
        },
        AngleTarget {
            position: (other.at.0 * 16, other.at.1 * 16),
            size: i32::from(other.size),
            slot_3c: false,
            effect_89: false,
        },
    ))
}
