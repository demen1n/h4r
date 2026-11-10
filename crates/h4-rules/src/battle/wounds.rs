//! How damage turns into dead creatures (`decomp/apply_damage.md`).
//!
//! The game does not store "hit points of the top creature". A stack keeps its
//! `number` and its `wounds`: the damage the top creature has already taken.
//! All other creatures of the stack are unhurt, so the stack as a whole has
//! `number * hit_points - wounds` hit points left ([`total_hits`]).
//!
//! Damage arrives in two steps, like in the exe:
//! 1. [`accept_damage`] (`t_combat_creature::add_damage`, rva `1f2800`) adds a
//!    hit to the stack's *pending* damage, but never more than the stack can
//!    still lose;
//! 2. [`resolve_damage`] (`t_combat_creature::resolve_damage`, rva `1eee40`)
//!    moves the pending damage into the wounds and counts the dead.
//!
//! The functions here are plain arithmetic on numbers, so they can be compared
//! with the original code one to one (`tests/apply_damage_vectors.tsv`).
//! `BattleState` owns the fields and calls them.

/// A stack after its pending damage was resolved, or after a direct kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Creatures left.
    pub count: i32,
    /// Damage on the new top creature. Always 0 for a dead stack.
    pub wounds: i32,
    /// How many creatures died in this step.
    pub killed: i32,
}

/// Hit points the whole stack has left: every creature is whole except the top
/// one. Shields from spells and the hit point bonuses of effects, which the
/// exe adds in `get_total_hits` (rva `1f5350`), are not part of this yet.
pub fn total_hits(count: i32, wounds: i32, hit_points: i32) -> Result<i32, String> {
    count
        .checked_mul(hit_points)
        .and_then(|all| all.checked_sub(wounds))
        .ok_or_else(|| "stack hit points overflow".to_string())
}

/// How much of `damage` the stack takes on top of the damage already `pending`.
/// The exe ignores hits below 1 and cuts the rest so that pending damage never
/// exceeds `total_hits`. The result is what the caller adds to `pending`; it is
/// also the "damage dealt" that the game reports back to the attacker.
pub fn accept_damage(damage: i32, pending: i32, total_hits: i32) -> Result<i32, String> {
    if damage < 1 {
        return Ok(0);
    }
    let wanted = pending
        .checked_add(damage)
        .ok_or("pending damage overflow")?;
    let accepted = if wanted > total_hits {
        total_hits
            .checked_sub(pending)
            .ok_or("pending damage overflow")?
    } else {
        damage
    };
    Ok(accepted.max(0))
}

/// Move `pending` damage into the wounds and count the dead.
///
/// The steps and their order are the exe's:
/// - wounds grow by the pending damage;
/// - every full `hit_points` in the wounds is one dead creature;
/// - if nobody died, nothing else changes (even for an already empty stack);
/// - no more can die than the stack has; a stack that loses its last creature
///   also loses its wounds.
///
/// `hit_points` is the creature's current maximum and must be positive.
pub fn resolve_damage(
    count: i32,
    wounds: i32,
    pending: i32,
    hit_points: i32,
) -> Result<Resolved, String> {
    if hit_points < 1 {
        return Err("creature hit points must be positive".into());
    }
    let wounds = wounds.checked_add(pending).ok_or("wounds overflow")?;
    let killed = wounds / hit_points;
    if killed == 0 {
        return Ok(Resolved {
            count,
            wounds,
            killed: 0,
        });
    }
    let killed = killed.min(count);
    let count = count - killed;
    if count < 1 {
        return Ok(Resolved {
            count: 0,
            wounds: 0,
            killed,
        });
    }
    Ok(Resolved {
        count,
        // `killed` did not need the `min` above, so this cannot overflow.
        wounds: wounds - hit_points * killed,
        killed,
    })
}

/// Remove `number` creatures outright (`t_combat_creature::kill`, rva `1f3860`).
/// The survivors keep the wounds of the top creature; a stack killed to the
/// last creature has none. The exe does not check for a negative `number`,
/// and neither does this function: callers pass a count of creatures.
pub fn kill(count: i32, wounds: i32, number: i32) -> Resolved {
    let killed = number.min(count);
    let count = count - killed;
    Resolved {
        count,
        wounds: if count == 0 { 0 } else { wounds },
        killed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wounds_carry_over_between_hits() {
        // 5 creatures of 10 hit points
        let first = resolve_damage(5, 0, 15, 10).unwrap();
        assert_eq!((first.count, first.wounds, first.killed), (4, 5, 1));
        let second = resolve_damage(first.count, first.wounds, 5, 10).unwrap();
        assert_eq!((second.count, second.wounds, second.killed), (3, 0, 1));
        let last = resolve_damage(second.count, second.wounds, 100, 10).unwrap();
        assert_eq!((last.count, last.wounds, last.killed), (0, 0, 3));
    }

    #[test]
    fn pending_damage_is_capped_by_what_the_stack_has_left() {
        let total = total_hits(3, 4, 10).unwrap();
        assert_eq!(total, 26);
        assert_eq!(accept_damage(20, 0, total).unwrap(), 20);
        assert_eq!(accept_damage(20, 20, total).unwrap(), 6);
        assert_eq!(accept_damage(20, 26, total).unwrap(), 0);
        assert_eq!(accept_damage(0, 0, total).unwrap(), 0);
        assert_eq!(accept_damage(-5, 0, total).unwrap(), 0);
    }

    #[test]
    fn bad_numbers_are_errors_not_panics() {
        assert!(resolve_damage(1, 0, 1, 0).is_err());
        assert!(resolve_damage(1, i32::MAX, 1, 10).is_err());
        assert!(total_hits(i32::MAX, 0, 2).is_err());
        assert!(accept_damage(i32::MAX, 1, 10).is_err());
    }

    #[test]
    fn direct_kills_keep_the_top_creatures_wounds() {
        assert_eq!(
            kill(5, 3, 2),
            Resolved {
                count: 3,
                wounds: 3,
                killed: 2
            }
        );
        assert_eq!(
            kill(5, 3, 9),
            Resolved {
                count: 0,
                wounds: 0,
                killed: 5
            }
        );
    }
}
