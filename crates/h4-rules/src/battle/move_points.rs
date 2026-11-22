//! How far a combat unit may move in one turn: its movement points (`1ef750`, the
//! virtual `+0x18` of the creature's stats object `t_combat_creature+0x98`; the path
//! search, the stop and "turns to reach" all read it). A plain straight step costs 100.
//! Details and addresses: `decomp/move_points.md`.
//!
//! The function adds up, in this order:
//!
//! 1. the base of the unit (`+0xa4` → virtual `+0x18` of its army member): a creature's
//!    `Move` column × 100 (`23d5b0`), a hero always 2400 (`32a1f0`);
//! 2. the army bonus (`t_combat_creature+0xaf8`, the best among the army's heroes, filled
//!    at `1f0ea0`) and the artifact bonus (`+0xa10`, added by artifact properties at
//!    `1d0350`) — both are inputs here, their sources aren't ported;
//! 3. +600 with ability `sea_bonus` in a battle at sea (battlefield byte `+0x36c`);
//! 4. +900 under haste or nature haste (effects 0x37, 0x68);
//! 5. halved under aging (effect 1), then halved again under slow, order slow or bind
//!    flyer (0x8d, 0x69, 7) — each an arithmetic shift, rounding down;
//! 6. a mode set by a combat command handler (`232250` → `232480`, `+0xa2c`; normally
//!    0): 1 adds 15000, 2 caps the result at 1200;
//! 7. never below 0.
//!
//! Verified whole against the exe in Unicorn (`tools/testbench/move_points.py`,
//! `tests/move_points_vectors.rs`).

use crate::damage::ability;
use crate::effects::effect;

/// The base of a hero: the same for every hero (`32a1f0`).
pub const HERO_BASE: i32 = 2400;

/// The base of a creature: its `Move` column × 100 (`23d5b0`, the traits' `+0x5c`).
pub fn creature_base(table_movement: i32) -> i32 {
    // The exe multiplies as ×5 ×5 ×4 with 32-bit wrapping; table values are small.
    table_movement.wrapping_mul(100)
}

/// The `+0xa2c` mode of a combat creature. It is set only by a combat command handler
/// (`232250`, command ids around 0x8060; it looks like a debugging command), so it is
/// [`CommandMode::Normal`] in an ordinary battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandMode {
    #[default]
    Normal,
    /// Mode 1: +15000, practically unlimited.
    Unlimited,
    /// Mode 2: no more than 1200.
    Capped,
}

/// Everything besides the base that `1ef750` reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveModifiers {
    /// Best movement bonus among the army's heroes (`+0xaf8`); 0 without one.
    pub army_bonus: i32,
    /// Sum of the unit's artifact movement bonuses (`+0xa10`).
    pub artifact_bonus: i32,
    /// The battle is at sea (battlefield byte `+0x36c`).
    pub at_sea: bool,
    /// Creature abilities ([`ability`] ids); only `sea_bonus` matters here.
    pub abilities: Vec<u8>,
    /// Active spell effects ([`effect`] ids).
    pub effects: Vec<u8>,
    pub mode: CommandMode,
}

/// Movement points of a unit with the given base (`1ef750`).
pub fn movement_points(base: i32, m: &MoveModifiers) -> i32 {
    let has_effect = |id: u8| m.effects.contains(&id);
    // 32-bit sums like the exe's registers.
    let mut points = base.wrapping_add(m.army_bonus).wrapping_add(m.artifact_bonus);
    if m.at_sea && m.abilities.contains(&ability::SEA_BONUS) {
        points = points.wrapping_add(600);
    }
    if has_effect(effect::HASTE) || has_effect(effect::NATURE_HASTE) {
        points = points.wrapping_add(900);
    }
    // `sar` by one: halves and rounds toward minus infinity, as `>>` on i32 does.
    if has_effect(effect::AGING) {
        points >>= 1;
    }
    if has_effect(effect::SLOW) || has_effect(effect::ORDER_SLOW) || has_effect(effect::BIND_FLYER) {
        points >>= 1;
    }
    match m.mode {
        CommandMode::Normal => {}
        CommandMode::Unlimited => points = points.wrapping_add(15000),
        CommandMode::Capped if points > 1200 => return 1200,
        CommandMode::Capped => {}
    }
    points.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_creature_moves_its_table_value() {
        assert_eq!(movement_points(creature_base(7), &MoveModifiers::default()), 700);
        assert_eq!(movement_points(HERO_BASE, &MoveModifiers::default()), 2400);
    }

    #[test]
    fn haste_comes_before_the_halvings() {
        let m = MoveModifiers { effects: vec![effect::HASTE, effect::SLOW, effect::AGING], ..Default::default() };
        // (500 + 900) / 2 / 2
        assert_eq!(movement_points(creature_base(5), &m), 350);
    }

    #[test]
    fn negative_bonuses_stop_at_zero() {
        let m = MoveModifiers { army_bonus: -1000, ..Default::default() };
        assert_eq!(movement_points(creature_base(5), &m), 0);
    }
}
