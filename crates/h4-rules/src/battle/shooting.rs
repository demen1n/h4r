//! The distance penalty of a shot, as the exe computes it (rva `170800`, read
//! from the disassembly; `decomp/shooting.md`). Not run in Unicorn yet.
//!
//! The divisor that `1eebf0` applies to the shot's damage is
//! [`range_effect`]`(edge distance, shot passed an obstacle)`. Three pieces:
//!
//! - [`can_aim`] — the checks `170800` makes before it measures anything;
//! - [`edge_distance`] — how far the target is in cells (`1f17d0`);
//! - [`range_effect`] — the penalty table (`2509f0`).
//!
//! What is *not* ported: the trace along the line of fire (`16fe00`), whose
//! result says whether the shot passed an obstacle ([`LineOfFire`]); it needs
//! the battlefield cells and is read only partially (`decomp/shooting.md`).

/// Ability numbers (the game's keywords).
pub mod abilities {
    pub const RANGED: u8 = 0x33;
    /// No distance penalty at all.
    pub const LONG_RANGE: u8 = 0x24;
    /// The penalty grows already at 20 cells.
    pub const SHORT_RANGE: u8 = 0x3c;
    /// Siege machines: an obstacle on the line does not double the penalty.
    pub const SIEGE_MACHINE: u8 = 0x3d;
}

/// Effects (`t_spell`) the checks look at; their spell names are not checked yet.
pub mod effects {
    /// Stops a stack from shooting.
    pub const NO_SHOOTING: u16 = 0x2f;
    /// On the target: it cannot be aimed at.
    pub const UNTARGETABLE: u16 = 0x89;
}

/// What the line-of-fire trace (`16fe00`) found. The exe writes 0, 1 or 2 to an
/// output argument; only 1 changes the penalty, and 2 refuses the shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOfFire {
    /// Nothing in the way (0).
    Clear,
    /// The shot passes an obstacle or another stack (1): the penalty doubles.
    Obstructed,
    /// The shot cannot be made (2).
    Blocked,
}

/// The shooter and the target as `170800` asks about them before it measures.
#[derive(Debug, Clone, Default)]
pub struct AimCheck<'a> {
    pub shooter_abilities: &'a [u8],
    pub shooter_effects: &'a [u16],
    pub shots: i32,
    /// The battlefield's threat number for the shooter (an enemy next to it).
    pub threats: i32,
    pub target_effects: &'a [u16],
}

/// May this stack shoot at this target at all (the first checks of `170800`):
/// it has `ranged`, no effect `0x2f`, shots left, no enemy next to it, and the
/// target has no effect `0x89`. The trace result is checked separately.
pub fn can_aim(c: &AimCheck) -> bool {
    c.shooter_abilities.contains(&abilities::RANGED)
        && !c.shooter_effects.contains(&effects::NO_SHOOTING)
        && c.shots > 0
        && c.threats == 0
        && !c.target_effects.contains(&effects::UNTARGETABLE)
}

/// Distance in whole cells from the shooter to the edge of the target
/// (rva `1f17d0`). All positions are in 16ths of a cell. `shooter_center` is the
/// shooter's footprint center (the exe takes it from the cell it stands on or
/// would shoot from); `*_size` is the footprint side in cells.
///
/// The target's center is its position plus half its size. The distance is the
/// rounded straight line (32-bit squares, x87 root, `+0.5` and truncation, the
/// same as `retaliation`'s reach), minus both half sizes plus half a cell, in
/// cells rounded down.
pub fn edge_distance(shooter_center: (i32, i32), shooter_size: i32, target_position: (i32, i32), target_size: i32) -> i32 {
    let center = (target_position.0 + target_size * 8, target_position.1 + target_size * 8);
    let dx = shooter_center.0.wrapping_sub(center.0);
    let dy = shooter_center.1.wrapping_sub(center.1);
    let squares = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let distance = (f64::from(squares).sqrt() + 0.5) as i32;
    (distance - target_size * 8 - shooter_size * 8 + 8) >> 4
}

/// The divisor of a shot's damage (rva `2509f0`) for a target `distance`
/// cells away:
///
/// ```text
/// long_range:  1
/// short_range: 1 below 20 cells, 2 from 20, 4 from 40
/// otherwise:   1 below 40 cells, 2 from 40
/// × 2 when the shot passed an obstacle, unless the shooter is a siege machine
/// ```
pub fn range_effect(distance: i32, abilities: &[u8], obstructed: bool) -> i32 {
    let has = |a: u8| abilities.contains(&a);
    let mut effect = if has(self::abilities::LONG_RANGE) {
        1
    } else if has(self::abilities::SHORT_RANGE) {
        match distance {
            40.. => 4,
            20.. => 2,
            _ => 1,
        }
    } else if distance >= 40 {
        2
    } else {
        1
    };
    if obstructed && !has(self::abilities::SIEGE_MACHINE) {
        effect *= 2;
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_shooters_lose_half_from_forty_cells() {
        assert_eq!(range_effect(39, &[], false), 1);
        assert_eq!(range_effect(40, &[], false), 2);
        assert_eq!(range_effect(100, &[], false), 2);
    }

    #[test]
    fn short_range_has_two_steps_and_long_range_none() {
        let short = [abilities::SHORT_RANGE];
        assert_eq!([19, 20, 39, 40].map(|d| range_effect(d, &short, false)), [1, 2, 2, 4]);
        assert_eq!(range_effect(500, &[abilities::LONG_RANGE], false), 1);
        // long_range wins over short_range: the exe asks for it first.
        assert_eq!(range_effect(500, &[abilities::SHORT_RANGE, abilities::LONG_RANGE], false), 1);
    }

    #[test]
    fn an_obstacle_doubles_the_penalty_except_for_siege_machines() {
        assert_eq!(range_effect(5, &[], true), 2);
        assert_eq!(range_effect(40, &[abilities::SHORT_RANGE], true), 8);
        assert_eq!(range_effect(5, &[abilities::LONG_RANGE], true), 2);
        assert_eq!(range_effect(5, &[abilities::SIEGE_MACHINE], true), 1);
    }

    #[test]
    fn edge_distance_counts_cells_between_the_edges() {
        // Two 1-cell stacks 5 cells apart on a row: centers 80 apart, edges 4 cells.
        assert_eq!(edge_distance((8, 8), 1, (80, 0), 1), 4);
        // Touching stacks are 0 apart.
        assert_eq!(edge_distance((8, 8), 1, (16, 0), 1), 0);
        // Bigger footprints shorten it.
        assert_eq!(edge_distance((16, 16), 2, (96, 0), 2), 4);
    }

    #[test]
    fn aiming_needs_ranged_shots_no_neighbours_and_a_free_target() {
        let base = AimCheck { shooter_abilities: &[abilities::RANGED], shots: 3, ..Default::default() };
        assert!(can_aim(&base));
        assert!(!can_aim(&AimCheck { shots: 0, ..base.clone() }));
        assert!(!can_aim(&AimCheck { threats: 1, ..base.clone() }));
        assert!(!can_aim(&AimCheck { shooter_effects: &[effects::NO_SHOOTING], ..base.clone() }));
        assert!(!can_aim(&AimCheck { target_effects: &[effects::UNTARGETABLE], ..base.clone() }));
        assert!(!can_aim(&AimCheck { shooter_abilities: &[], ..base }));
    }
}
