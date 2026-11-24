//! Morale of a creature in combat, as the exe computes it (`decomp/morale.md`; verified
//! by executing the original code: `tests/morale_vectors.tsv`).
//!
//! The number the initiative uses ([`crate::battle::initiative`]) is the creature's
//! *effective* morale (`1efbd0`), made of:
//! - a base the creature brings into the battle (`+0xae8` of the combat creature): the
//!   army member's own morale (`251500`: a bonus kept by the stack, plus 5 under the
//!   "morale" effect) and the morale of its army ([`army_morale`], `23fe40`): how well
//!   the alignments in the army get along;
//! - the side's loss penalty, added to the same base at the start of each round from the
//!   second on ([`loss_penalties`], `176250`): a side that has lost a larger share of its
//!   strength than the enemy loses up to 10 morale;
//! - auras of other participants (`+0xaec`, the strongest one; heroes' skills — not
//!   recovered, an input);
//! - then the getter: undead and mechanical creatures have none, a few spells set or
//!   shift it, and it stays within -10..=10.

/// Effect ids that matter here (the exe's keyword table at VA `0x994ac0`).
pub mod effect {
    /// "Despair": -10.
    pub const DESPAIR: u8 = 0x19;
    /// "Celestial armor": its absorbing pool counts as strength.
    pub const CELESTIAL_ARMOR: u8 = 0x10;
    /// "Dragon strength": double hit points for the strength.
    pub const DRAGON_STRENGTH: u8 = 0x21;
    /// "Giant strength": a quarter more hit points.
    pub const GIANT_STRENGTH: u8 = 0x33;
    /// "Health boost": a quarter more hit points.
    pub const HEALTH_BOOST: u8 = 0x39;
    /// "Life drain": its pool counts as strength.
    pub const LIFE_DRAIN: u8 = 0x44;
    /// "Morale": +5 (on the army member and in combat).
    pub const MORALE: u8 = 0x67;
    /// "Prayer": a quarter more hit points.
    pub const PRAYER: u8 = 0x72;
    /// "Sorrow": morale is -10, whatever else.
    pub const SORROW: u8 = 0x91;
    /// "Spiritual fervor": morale is 10, unless sorrow.
    pub const SPIRITUAL_FERVOR: u8 = 0x94;
}

/// The death alignment: undead in its own army don't bother it.
pub const DEATH: usize = 2;
/// The might alignment: neutral to every magic.
pub const MIGHT: usize = 5;

/// How two alignments relate (`11ecd0`): 0 the same, 1 allies (neighbours on the
/// circle life–order–death–chaos–nature), 2 might and any other, 3 opposed.
pub fn alignment_relation(a: usize, b: usize) -> usize {
    if a == b {
        return 0;
    }
    if a == MIGHT || b == MIGHT {
        return 2;
    }
    // The exe works with signed ints; alignments are 0..=4 here.
    let step = ((a as i32 - b as i32) + 5) % 5;
    if step == 1 || step == 4 {
        1
    } else {
        3
    }
}

/// Morale for having an alignment of each relation in the army (VA `0xa648d8`).
pub const RELATION_MORALE: [i32; 4] = [0, -1, -2, -5];

/// One member of an army, as [`army_morale`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmyMember {
    /// The member's alignment (a hero's from its class).
    pub alignment: usize,
    /// How many there are; a member with 0 is skipped (an empty slot).
    pub count: i32,
    /// A creature with the `undead` ability. Heroes never are (`f140`).
    pub undead: bool,
}

/// The morale an army gives one of its members of alignment `own` (`23fe40`): 1, plus
/// [`RELATION_MORALE`] for every alignment present (each counted once, the member's own
/// included, which adds 0), and -2 if anyone in the army is undead and `own` isn't
/// death. Alignments above might are not counted (the exe keeps flags for 0..=5).
pub fn army_morale(own: usize, members: &[ArmyMember]) -> i32 {
    let mut present = [false; 6];
    let mut undead = false;
    for member in members.iter().filter(|m| m.count != 0) {
        if let Some(flag) = present.get_mut(member.alignment) {
            *flag = true;
        }
        undead |= member.undead;
    }
    let mut morale = 1;
    for (alignment, _) in present.iter().enumerate().filter(|(_, &p)| p) {
        morale += RELATION_MORALE[alignment_relation(own, alignment)];
    }
    if undead && own != DEATH {
        morale -= 2;
    }
    morale
}

/// The army member's own morale (`251500`, virtual `+0x74` of a creature stack or
/// hero): the bonus it keeps (`+0x14`, from adventure-map visits and the like), plus 5
/// under the "morale" effect.
pub fn member_morale(bonus: i32, morale_effect: bool) -> i32 {
    bonus + if morale_effect { 5 } else { 0 }
}

/// What the morale getter asks of a combat creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoraleInputs {
    /// `+0xae8`: [`member_morale`] + [`army_morale`] at creation, plus the loss
    /// penalty changes since.
    pub base: i32,
    /// `+0xaec`: the strongest aura of the battle's influences on it.
    pub influence: i32,
    /// The `undead` and `mechanical` abilities ([`crate::damage::ability`]).
    pub undead: bool,
    pub mechanical: bool,
    pub sorrow: bool,
    pub spiritual_fervor: bool,
    pub morale_effect: bool,
    pub despair: bool,
}

/// Effective morale (`1efbd0`), in the exe's order: sorrow -10, spiritual fervor 10,
/// undead or mechanical 0; otherwise `base + influence`, +5 morale, -10 despair, limited
/// to -10..=10.
pub fn effective_morale(m: &MoraleInputs) -> i32 {
    if m.sorrow {
        return -10;
    }
    if m.spiritual_fervor {
        return 10;
    }
    if m.undead || m.mechanical {
        return 0;
    }
    let mut morale = m.base.wrapping_add(m.influence);
    if m.morale_effect {
        morale = morale.wrapping_add(5);
    }
    if m.despair {
        morale = morale.wrapping_sub(10);
    }
    morale.clamp(-10, 10)
}

/// Morale without spells (`1efb90`): undead or mechanical 0, else `base + influence`,
/// not limited.
pub fn morale_without_spells(m: &MoraleInputs) -> i32 {
    if m.undead || m.mechanical {
        0
    } else {
        m.base.wrapping_add(m.influence)
    }
}

/// What the strength of a stack depends on besides its number and wounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StrengthEffects {
    pub giant_strength: bool,
    pub health_boost: bool,
    pub prayer: bool,
    pub dragon_strength: bool,
    /// The pool of "celestial armor" (`+0x198`), counted only under the effect.
    pub celestial_armor: Option<i32>,
    /// The pool of "life drain" (`+0x408`), counted only under the effect.
    pub life_drain: Option<i32>,
}

/// A stack's strength for the morale of its side (`1f5350` with the arguments the
/// morale code passes: no resurrection part, no effect left out): hit points of one,
/// a quarter more (rounded half up: `(hp + 2) >> 2`) for each of giant strength, health
/// boost and prayer in that order, doubled by dragon strength; times the number, minus
/// the top creature's wounds; plus the celestial armor and life drain pools.
pub fn strength(count: i32, hit_points: i32, wounds: i32, e: &StrengthEffects) -> i32 {
    let mut hp = hit_points;
    for on in [e.giant_strength, e.health_boost, e.prayer] {
        if on {
            hp = hp.wrapping_add(hp.wrapping_add(2) >> 2);
        }
    }
    if e.dragon_strength {
        hp = hp.wrapping_add(hp);
    }
    let mut total = count.wrapping_mul(hp).wrapping_sub(wounds);
    if let Some(pool) = e.celestial_armor {
        total = total.wrapping_add(pool);
    }
    if let Some(pool) = e.life_drain {
        total = total.wrapping_add(pool);
    }
    total
}

/// The loss penalties of the two sides (`176250`), from their strength at the start of
/// the battle and now. A side's share lost is `(now - initial) * 10 / initial` (rounded
/// toward zero; 0 if it hasn't lost anything); its penalty is its share minus the
/// enemy's, but never above 0. Index 0 is the attacker.
pub fn loss_penalties(initial: [i32; 2], now: [i32; 2]) -> [i32; 2] {
    let share = |side: usize| {
        if now[side] < initial[side] {
            // `now < initial` makes `initial` above any real strength, so it isn't 0
            // unless a strength is negative (the exe would fault dividing by 0).
            now[side].wrapping_sub(initial[side]).wrapping_mul(10).checked_div(initial[side]).unwrap_or(0)
        } else {
            0
        }
    };
    let shares = [share(0), share(1)];
    [shares[0].wrapping_sub(shares[1]).min(0), shares[1].wrapping_sub(shares[0]).min(0)]
}
