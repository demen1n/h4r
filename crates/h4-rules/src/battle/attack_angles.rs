//! Where a creature may stand to strike a target in melee: the attack position list
//! (`t_attack_angle_list`, built by rva `10c870`), which the interface offers to the player
//! and the AI chooses from (`decomp/attack_angles.md`; verified by executing the original
//! code: `tests/attack_angle_vectors.tsv`).
//!
//! The list is built from a finished path search of the attacker:
//! 1. every footprint corner around the target (as far as the attacker's reach) that the
//!    search reached and that isn't occupied is a candidate;
//! 2. it stays if the distance between the attacker's and the target's footprint centers
//!    is within the reach;
//! 3. the candidates are sorted by the angle from the attacker to the target, then by the
//!    corner;
//! 4. candidates that take more turns to reach than the nearest one are dropped;
//! 5. of two neighbours in the list (the last and the first are neighbours too) whose
//!    angles differ by less than 12°, the one nearer to the target is dropped.
//!
//! Not supported yet: a target made of several parts (castle walls and gates: their
//! candidates may coincide, and the order of equal entries depends on the exe's sort),
//! and an attacker or a target standing in a castle tower (the exe then uses the tower's
//! footprint).

use crate::battle::path::{flags, PathMap};
use crate::battle::path_prepare::angle;

/// One place to strike from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackPosition {
    /// The angle from the attacker's center to the target's center, 0..360 (see
    /// [`angle`]).
    pub angle: i32,
    /// The attacker's footprint corner.
    pub at: (i32, i32),
    /// The distance between the two centers in subcells (16 per cell), rounded.
    pub distance: i32,
    /// The path cost of the corner.
    pub cost: u32,
}

/// The attacker, as the list asks about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AngleAttacker {
    /// Footprint side in cells.
    pub size: i32,
    /// Movement points (virtual `+0x18` of the stats), for the turns to reach.
    pub movement: i32,
    /// Ability 0x25 `long_weapon`: reach 6 instead of 2 (`1ef5e0`).
    pub long_weapon: bool,
    /// Effect 0x90: the list is empty.
    pub effect_90: bool,
}

/// The target, as the list asks about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AngleTarget {
    /// Corner of its footprint in subcells.
    pub position: (i32, i32),
    /// Footprint side in cells.
    pub size: i32,
    /// Its virtual `+0x3c`: when true the reach is 0 (only touching positions). What the
    /// slot means isn't known.
    pub slot_3c: bool,
    /// Effect 0x89: the list is empty.
    pub effect_89: bool,
}

/// Angles closer than this are one approach (`0x97a778`, compared as a double).
const NEAR_ANGLE: i32 = 12;

/// Build the attack position list (`10c870`) from the attacker's searched path map.
pub fn attack_positions(map: &PathMap, attacker: AngleAttacker, target: AngleTarget) -> Vec<AttackPosition> {
    if attacker.effect_90 || target.effect_89 {
        return Vec::new();
    }
    let reach = if target.slot_3c {
        0
    } else if attacker.long_weapon {
        6
    } else {
        2
    };
    let half = attacker.size * 8;
    let corner = (target.position.0 >> 4, target.position.1 >> 4);
    let center = (target.position.0 + target.size * 8, target.position.1 + target.size * 8);
    // The square of corners walked, and the farthest center distance accepted.
    let limit = target.size + 1 + 2 * reach;
    let farthest = reach * 16 + 8 + half + target.size * 8;

    let mut list = Vec::new();
    for i in -attacker.size..limit {
        let x = corner.0 - reach + i;
        for j in -attacker.size..limit {
            let y = corner.1 - reach + j;
            let Some(cell) = map.cell((x, y)) else { continue };
            if cell.flags & flags::REACHED == 0 || cell.flags & flags::OCCUPIED != 0 {
                continue;
            }
            let d = (center.0 - (x * 16 + half), center.1 - (y * 16 + half));
            let distance = rounded_length(d);
            if distance <= farthest {
                list.push(AttackPosition { angle: angle(d), at: (x, y), distance, cost: cell.cost });
            }
        }
    }
    // The exe's std::sort by (angle, row, column). Every corner appears once, so the
    // keys are distinct and any sort gives the same order.
    list.sort_by_key(|p| (p.angle, p.at.0, p.at.1));
    if list.len() < 2 {
        return list;
    }

    // Keep only the positions reachable in the fewest turns. A movement of 0 would make
    // the exe divide by zero; here nothing is dropped then.
    let turns = |p: &AttackPosition| (p.cost as i32).wrapping_sub(1).checked_div(attacker.movement);
    let fewest = list.iter().filter_map(turns).fold(100, i32::min);
    list.retain(|p| turns(p).is_none_or(|t| t <= fewest));

    // Of two neighbours with nearly the same angle, keep the farther one. The exe walks
    // from the last entry to the first, comparing each with the next (the first after the
    // last), while the list shrinks under it.
    for i in (0..list.len()).rev() {
        let j = (i + 1) % list.len();
        if i == j {
            continue;
        }
        if angle_difference(list[i].angle, list[j].angle) < NEAR_ANGLE {
            let drop = if list[i].distance < list[j].distance { i } else { j };
            list.remove(drop);
        }
    }
    list
}

/// `sqrt(dx² + dy²) + 0.5`, truncated (x87, then `__ftol`).
fn rounded_length(d: (i32, i32)) -> i32 {
    let square = d.0.wrapping_mul(d.0).wrapping_add(d.1.wrapping_mul(d.1));
    (f64::from(square).sqrt() + 0.5) as i32
}

/// The difference of two angles in degrees, 0..=180 (`11ed80`).
fn angle_difference(a: i32, b: i32) -> i32 {
    let d = a.wrapping_sub(b).wrapping_abs() % 360;
    if d < 181 { d } else { 360 - d }
}
