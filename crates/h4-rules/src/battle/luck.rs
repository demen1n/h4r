//! The target's luck roll inside the damage intake (`t_combat_creature`, rva
//! `1f2800`; read from the disassembly, `decomp/apply_damage.md`).
//!
//! One battle die with purpose `0x1b` is rolled per hit, and the **target's**
//! luck decides what it means (the attacker's luck is a different thing: the
//! multiplier in `damage::modify_damage`):
//!
//! ```text
//! roll = dice(target, attacker, 0x1b) % 10
//! roll <  luck   -> damage = damage * 2 / 3, at least 1   (lucky target)
//! roll < -luck   -> damage = damage + (damage >> 1)       (unlucky target)
//! ```
//!
//! Only one of them can happen: for a positive luck `-luck` is negative, and
//! the roll is never below 0. Which callers ask for this roll is not known
//! yet, so the caller decides; the luck number comes from the caller too
//! (virtual slot `+0x34`, hero skills and artifacts are not ported).

/// What the roll did to the hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLuck {
    /// Nothing changed.
    Normal,
    /// The target was lucky: the hit is two thirds (the exe marks the stack for
    /// its "good luck" animation).
    Lucky,
    /// The target was unlucky: the hit is one and a half times.
    Unlucky,
}

/// Apply a rolled die (`roll` is the raw battle number, `% 10` is done here)
/// to the damage of a hit. `damage` is at least 1 here: the exe throws away
/// smaller hits before any die is rolled.
pub fn apply_target_luck(damage: i32, roll: u32, luck: i32) -> (i32, TargetLuck) {
    let roll = (roll % 10) as i32;
    if roll < luck {
        // 32-bit multiply and truncating division, as the exe does it.
        let two_thirds = (i64::from(damage) * 2 / 3) as i32;
        (two_thirds.max(1), TargetLuck::Lucky)
    } else if roll < luck.wrapping_neg() {
        (damage.wrapping_add(damage >> 1), TargetLuck::Unlucky)
    } else {
        (damage, TargetLuck::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_luck_never_changes_the_hit() {
        for roll in 0..10 {
            assert_eq!(apply_target_luck(30, roll, 0), (30, TargetLuck::Normal));
        }
    }

    #[test]
    fn luck_plus_one_helps_only_on_a_zero() {
        assert_eq!(apply_target_luck(30, 0, 1), (20, TargetLuck::Lucky));
        assert_eq!(apply_target_luck(30, 1, 1), (30, TargetLuck::Normal));
        // The die is reduced modulo 10 first.
        assert_eq!(apply_target_luck(30, 10, 1), (20, TargetLuck::Lucky));
    }

    #[test]
    fn bad_luck_adds_half_rounded_down() {
        assert_eq!(apply_target_luck(31, 0, -1), (46, TargetLuck::Unlucky));
        assert_eq!(apply_target_luck(31, 1, -1), (31, TargetLuck::Normal));
        assert_eq!(apply_target_luck(5, 4, -5), (7, TargetLuck::Unlucky));
    }

    #[test]
    fn a_lucky_hit_is_never_below_one() {
        assert_eq!(apply_target_luck(1, 0, 3), (1, TargetLuck::Lucky));
        assert_eq!(apply_target_luck(2, 2, 3), (1, TargetLuck::Lucky));
    }
}
