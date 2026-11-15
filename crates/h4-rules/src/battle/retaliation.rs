//! May a stack answer an attack? The retaliation check of `t_combat_creature`
//! (rva `1f23c0`) and the "can shoot" check of `t_battlefield` (rva `170780`), as the
//! exe decides them
//! (`decomp/retaliation.md`; verified by executing the original code:
//! `tests/retaliation_vectors.tsv`).
//!
//! Everything the exe reads from the battle is an input here. Two answers come
//! from rules that are not ported yet and are given by the caller: whether an
//! effect may act on the target (`1b6120`, the immunity and resistance checks),
//! and how many enemies threaten the target (the battlefield's threat map).

use crate::battle::melee::RetaliationFlag;

/// Ability numbers (the game's keywords) used here.
pub mod abilities {
    pub const DEATH_PROTECTION: u8 = 0x10;
    pub const LONG_WEAPON: u8 = 0x25;
    pub const NO_RETALIATION: u8 = 0x2e;
    pub const PANIC: u8 = 0x30;
    pub const RANGED: u8 = 0x33;
}

/// Effect numbers (`t_spell`) used here. Their spell names are not checked yet.
pub mod effects {
    /// On the attacker: the target can't answer if effect 0x25 may act on it.
    pub const ATTACKER_24: u16 = 0x24;
    /// Effect asked about for [`ATTACKER_24`].
    pub const CHECKED_25: u16 = 0x25;
    /// Stops a stack from shooting.
    pub const NO_SHOOTING: u16 = 0x2f;
    /// Effect asked about for the attacker's `panic`.
    pub const CHECKED_6B: u16 = 0x6b;
    /// Two incapacitating effects, each with a strength field
    /// (`+0x810` for 0x9a, `+0x990` for 0xba).
    pub const INCAPACITATING_9A: u16 = 0x9a;
    pub const INCAPACITATING_BA: u16 = 0xba;
}

/// Where a stack stands, in the battlefield's own units: a cell is 16 subcells.
/// `position` is the corner of the footprint, `size` its side in cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Footprint {
    pub position: (i32, i32),
    pub size: i32,
}

/// Can the stack shoot (rva `170780`): it has the `ranged` ability,
/// no effect 0x2f, shots left, and nothing in the battlefield's threat map
/// (`threats` is the number stored there for this stack; it seems to count
/// enemies standing next to it, which is not verified).
pub fn can_shoot(abilities: &[u8], effects: &[u16], shots: i32, threats: i32) -> bool {
    abilities.contains(&abilities::RANGED)
        && !effects.contains(&effects::NO_SHOOTING)
        && shots > 0
        && threats == 0
}

/// Everything `can_retaliate` reads. The target is the stack that would answer.
#[derive(Debug, Clone, Default)]
pub struct RetaliationCheck<'a> {
    /// The third argument of the exe function. A normal attack passes 1
    /// (`decomp/melee_attack.md`); other callers (the AI) are not explored.
    pub n: i32,
    pub ranged: bool,
    pub target_flag: RetaliationFlag,
    /// The target's effects, and the strength fields of 0x9a / 0xba.
    pub target_effects: &'a [u16],
    pub target_strength_9a: i32,
    pub target_strength_ba: i32,
    pub target_abilities: &'a [u8],
    /// [`can_shoot`] for the target; only a ranged attack asks.
    pub target_can_shoot: bool,
    pub attacker_abilities: &'a [u8],
    pub attacker_effects: &'a [u16],
    pub attacker: Footprint,
    pub target: Footprint,
    /// Answers of the effect eligibility check `1b6120` for the target.
    pub effect_25_can_affect_target: bool,
    pub effect_6b_can_affect_target: bool,
}

/// How far a stack reaches in melee, in cells: 6 with `long_weapon`, else 2.
fn reach(abilities: &[u8]) -> i32 {
    if abilities.contains(&abilities::LONG_WEAPON) { 6 } else { 2 }
}

/// Distance in cells between the footprints' edges, as the exe measures it when
/// the attacker reaches further than the target. The target's center is taken
/// from its cell, the attacker's from its exact position.
fn gap_in_cells(attacker: Footprint, target: Footprint) -> i32 {
    let target_center = (
        (target.position.0 >> 4 << 4) + target.size * 8,
        (target.position.1 >> 4 << 4) + target.size * 8,
    );
    let attacker_center = (attacker.position.0 + attacker.size * 8, attacker.position.1 + attacker.size * 8);
    // 32-bit integer squares, then the x87 square root; +0.5 and truncation
    // (`__ftol`) round to the nearest whole subcell.
    let dx = target_center.0.wrapping_sub(attacker_center.0);
    let dy = target_center.1.wrapping_sub(attacker_center.1);
    let squares = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let distance = (f64::from(squares).sqrt() + 0.5) as i32;
    (distance - target.size * 8 - attacker.size * 8 + 8) >> 4
}

/// The retaliation check (attacker, ranged, n), checks in the exe's order (rva `1f23c0`).
pub fn can_retaliate(c: &RetaliationCheck) -> bool {
    let target_has_effect = |e: u16| c.target_effects.contains(&e);
    if c.target_flag.retaliated && c.n < 2 {
        return false; // already answered (the flag is cleared a round later)
    }
    if target_has_effect(effects::INCAPACITATING_BA) && c.target_strength_ba >= c.n {
        return false;
    }
    if target_has_effect(effects::INCAPACITATING_9A) && c.target_strength_9a >= c.n {
        return false;
    }
    if c.ranged {
        return c.target_can_shoot;
    }
    let attacker_has = |a: u8| c.attacker_abilities.contains(&a);
    if attacker_has(abilities::NO_RETALIATION) {
        return false;
    }
    let target_reach = reach(c.target_abilities);
    // A longer weapon hits from further than the target can answer.
    if reach(c.attacker_abilities) > target_reach && gap_in_cells(c.attacker, c.target) > target_reach {
        return false;
    }
    let target_protected = c.target_abilities.contains(&abilities::DEATH_PROTECTION);
    if c.attacker_effects.contains(&effects::ATTACKER_24) && c.effect_25_can_affect_target && !target_protected {
        return false;
    }
    if attacker_has(abilities::PANIC) && c.effect_6b_can_affect_target && !target_protected {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_neighbour_answers() {
        let one = |x| Footprint { position: (x, 800), size: 1 };
        assert!(can_retaliate(&RetaliationCheck {
            n: 1,
            attacker: one(784),
            target: one(800),
            ..Default::default()
        }));
    }
}
