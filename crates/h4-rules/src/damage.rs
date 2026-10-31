//! Damage of one creature stack hitting another, as `heroes4.exe` computes it
//! (`attack.cpp` / `combat_creature.cpp`; details and addresses in `decomp/damage.md`).
//!
//! The calculation has three parts, matching three functions of the game:
//!
//! 1. **Base damage** — the dice: every creature in the stack rolls between its low and
//!    high damage ([`roll_base_damage`]); the AI and the damage preview use the average
//!    instead ([`average_base_damage`]).
//! 2. **Modifiers** ([`modify_damage`]) — charge, giant slaying, luck, attack against
//!    defense, wards, fire and cold resistance.
//! 3. **Final limits** ([`finalize_damage`]) — no more than the target has hit points
//!    left, then the target's `block` ability.
//!
//! Everything is integer arithmetic except where the game multiplies by a float; there we
//! cut the fraction off the same way the game does (`__ftol` truncates toward zero, which
//! for positive numbers is `as i32` in Rust).
//!
//! Abilities are given by their number in the game's ability enum
//! (`re/enum_creature_ability.tsv`), because the checks in the exe are by number and a few
//! names in our enum table look doubtful (noted next to the constants).

use crate::random::CombatRandom;

/// Ability numbers the damage code checks. The name is the one from our enum table.
pub mod ability {
    pub const BLOCK: u8 = 0x07;
    pub const BREATH_ATTACK: u8 = 0x09;
    /// Checked on the target as a ward against the Chaos school.
    pub const CHAOS_PROTECTION: u8 = 0x0a;
    pub const CHARGING: u8 = 0x0b;
    pub const COLD_ATTACK: u8 = 0x0c;
    pub const COLD_RESISTANCE: u8 = 0x0d;
    /// Checked on the target as a ward against Death. Our enum table calls 0x10
    /// `devouring` and puts `death_protection` at 0x0e — one of them is probably wrong.
    pub const WARD_DEATH: u8 = 0x10;
    pub const FIRE_ATTACK: u8 = 0x14;
    pub const FIRE_RESISTANCE: u8 = 0x15;
    /// Doubles damage against "giant" targets in melee and at range. The table says
    /// `insubstantial`, which doesn't fit; likely a giantslayer variant.
    pub const SLAYER_ANY: u8 = 0x1c;
    pub const LIFE_PROTECTION: u8 = 0x22;
    /// Doubles damage against "giant" targets in melee. The table says `mechanical`.
    pub const SLAYER_MELEE: u8 = 0x2b;
    pub const RANGED: u8 = 0x33;
    pub const RANGED_GIANTSLAYER: u8 = 0x35;
    pub const LARGE_AREA_EFFECT: u8 = 0x53;
    pub const ARC_BREATH_ATTACK: u8 = 0x54;
}

/// The magic schools ("alignments"), in the game's order. A creature's school decides
/// which ward protects against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum School {
    Life,
    Order,
    Death,
    Chaos,
    Nature,
    Might,
}

/// Outcome of the luck check for this hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Luck {
    Normal,
    Good,
    Bad,
}

/// How strong luck is: good luck multiplies damage, bad luck divides it, by one of these.
/// The game picks the entry by a field of the battle (`[+0x60]` of the object at
/// `t_combat_creature + 0xa18`) whose meaning we don't know yet.
pub const LUCK_MULTIPLIERS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

/// What the damage code needs to know about the attacking stack.
#[derive(Debug, Clone)]
pub struct Attacker {
    /// The stack's id in the battle; the dice are keyed by it.
    pub id: u32,
    pub number: i32,
    pub damage_low: i32,
    pub damage_high: i32,
    /// Attack skill with all bonuses (`get_offense(ranged)`).
    pub offense: f32,
    /// `get_defense_reduction(ranged)`: lets the attack cancel the target's defense bonus.
    /// 1.0 means no effect.
    pub defense_reduction: f32,
    pub school: School,
    pub abilities: Vec<u8>,
    /// Spell effect 0x8c on the attacker: +50% against giant targets (name unknown).
    pub giant_bonus_effect: bool,
}

/// What the damage code needs to know about the target.
#[derive(Debug, Clone)]
pub struct Defender {
    pub id: u32,
    /// Defense skill without temporary bonuses (`t_has_defense` slot +8).
    pub defense_basic: f32,
    /// Multiplier from defending, spells and so on (`t_has_defense` slot +4); 1.0 = none.
    pub defense_bonus: f32,
    pub abilities: Vec<u8>,
    /// Ward spells on the target, by the school they protect against (spell effects
    /// 0x77 Life, 0x79 Order, 0x76 Death, 0x75 Chaos, 0x78 Nature).
    pub warded_against: Vec<School>,
    /// Target's virtual slot +0x40; the giant-slaying bonuses only apply when it's true.
    pub giant: bool,
    /// All hit points the target stack has (`slot +0x2c(true, -1)`).
    pub total_hit_points: i32,
}

impl Attacker {
    fn has(&self, a: u8) -> bool {
        self.abilities.contains(&a)
    }
}

impl Defender {
    fn has(&self, a: u8) -> bool {
        self.abilities.contains(&a)
    }
}

/// One step of the calculation, for the battle log and for debugging differences
/// with the original game.
#[derive(Debug, Clone, PartialEq)]
pub enum DamageEvent {
    Rolled { damage: i32 },
    Charge { damage: i32 },
    GiantSlaying { damage: i32 },
    Luck { luck: Luck, damage: i32 },
    AttackVsDefense { ratio: f32, damage: i32 },
    Ward { damage: i32 },
    FireResisted { damage: i32 },
    ColdResisted { damage: i32 },
    LimitedByHitPoints { damage: i32 },
    Blocked { damage: i32 },
}

/// The average base damage: `(low + high) * number / 2`. This is what
/// `get_melee_damage` uses (the AI's and the cursor's estimate), not the real hit.
pub fn average_base_damage(attacker: &Attacker) -> i32 {
    (attacker.damage_low + attacker.damage_high) * attacker.number / 2
}

/// The real base damage (rva `1ee240`): each creature of the stack rolls
/// `low + roll % (high - low + 1)`. Stacks of 10 or more roll ten times and scale the sum
/// by `number / 10`, so a stack of 1000 doesn't need 1000 rolls.
pub fn roll_base_damage(attacker: &Attacker, target_id: u32, dice: &mut CombatRandom) -> i32 {
    let range = attacker.damage_high - attacker.damage_low + 1;
    let mut roll_one = || {
        // purpose 0 = damage roll; the first id is the target, the second the attacker
        let roll = dice.roll(target_id, attacker.id, 0) as i32;
        attacker.damage_low + roll % range
    };
    if attacker.number < 10 {
        (0..attacker.number).map(|_| roll_one()).sum()
    } else {
        let ten: i32 = (0..10).map(|_| roll_one()).sum();
        attacker.number * ten / 10
    }
}

/// Apply the attacker's and defender's modifiers (rva `1edf30`).
///
/// `moved` is how far the attacker moved before hitting, used by `charging` (the unit is
/// the battlefield's internal distance; the bonus starts above 500).
pub fn modify_damage(
    base: i32,
    attacker: &Attacker,
    defender: &Defender,
    ranged: bool,
    moved: i32,
    luck: Luck,
    luck_multiplier: f64,
    log: &mut Vec<DamageEvent>,
) -> i32 {
    let mut damage = base;

    // Charging (melee only): after a long run, damage * (85 + moved * 10 / 350) / 100.
    if !ranged && attacker.has(ability::CHARGING) && moved > 500 {
        damage = (moved * 10 / 350 + 85) * damage / 100;
        log.push(DamageEvent::Charge { damage });
    }

    // Giant slaying: double damage, or +50% from the spell effect.
    if defender.giant {
        let slayer = attacker.has(ability::SLAYER_ANY)
            || if ranged {
                attacker.has(ability::RANGED_GIANTSLAYER)
            } else {
                attacker.has(ability::SLAYER_MELEE)
            };
        if slayer {
            damage *= 2;
            log.push(DamageEvent::GiantSlaying { damage });
        } else if attacker.giant_bonus_effect {
            damage += damage >> 1;
            log.push(DamageEvent::GiantSlaying { damage });
        }
    }

    // Luck multiplies or divides; the game computes in floating point and truncates.
    match luck {
        Luck::Normal => {}
        Luck::Good => damage = (luck_multiplier * damage as f64) as i32,
        Luck::Bad => damage = (damage as f64 / luck_multiplier) as i32,
    }
    if luck != Luck::Normal {
        log.push(DamageEvent::Luck { luck, damage });
    }

    // Attack against defense: the heart of the formula.
    // The original keeps ST(0) at CRT-selected 53-bit precision between the
    // coefficient call and __ftol; an intermediate f32 loses whole damage points.
    let ratio = damage_modifier_precise(attacker, defender);
    damage = (ratio * damage as f64) as i32;
    log.push(DamageEvent::AttackVsDefense { ratio: ratio as f32, damage });

    // Ward against the attacker's school: damage * 100 / 150, i.e. a third less.
    if is_warded(attacker.school, defender) {
        damage = damage * 100 / 150;
        log.push(DamageEvent::Ward { damage });
    }

    // Fire attacks (breath counts as fire; area attacks only when shooting) halve
    // against fire resistance; cold the same against cold resistance.
    let fire = attacker.has(ability::FIRE_ATTACK)
        || attacker.has(ability::BREATH_ATTACK)
        || attacker.has(ability::ARC_BREATH_ATTACK)
        || (attacker.has(ability::LARGE_AREA_EFFECT) && ranged);
    if fire && defender.has(ability::FIRE_RESISTANCE) {
        damage >>= 1;
        log.push(DamageEvent::FireResisted { damage });
    }
    if attacker.has(ability::COLD_ATTACK) && defender.has(ability::COLD_RESISTANCE) {
        damage >>= 1;
        log.push(DamageEvent::ColdResisted { damage });
    }

    // A hit always does at least 1.
    damage.max(1)
}

/// Attack divided by defense, kept within 0.05..=20 (rva `250a60`,
/// `t_abstract_creature::get_damage_modifier`).
///
/// The attacker's `defense_reduction` works against the target's `defense_bonus`: while
/// it's no bigger than the bonus it multiplies the attack; once it's bigger, the bonus is
/// simply ignored (but a bonus below 1, a penalty, stays). This is a literal translation of
/// the branches in the exe; the reading of what it means is ours.
pub fn damage_modifier(attacker: &Attacker, defender: &Defender) -> f32 {
    damage_modifier_precise(attacker, defender) as f32
}

// x87 precision is set to 53 bits by the original CRT (RVA 4f2ee8).
fn damage_modifier_precise(attacker: &Attacker, defender: &Defender) -> f64 {
    let mut offense = attacker.offense;
    let mut bonus = defender.defense_bonus;
    if attacker.defense_reduction <= bonus {
        offense *= attacker.defense_reduction;
    } else if bonus > 1.0 {
        bonus = 1.0;
    }
    // The adjusted numerator is explicitly stored to f32 in the original.
    // The denominator and division remain at 53-bit precision.
    let ratio = offense as f64 / (defender.defense_basic as f64 * bonus as f64);
    // Not `f32::clamp`: like the game, check the lower bound first and treat "not a number"
    // (0 / 0) as below it, so the result is always a usable number.
    if !(ratio >= 0.05_f32 as f64) {
        0.05_f32 as f64
    } else if ratio > 20.0 {
        20.0
    } else {
        ratio
    }
}

fn is_warded(school: School, defender: &Defender) -> bool {
    let by_spell = defender.warded_against.contains(&school);
    let by_ability = match school {
        School::Life => defender.has(ability::LIFE_PROTECTION),
        School::Death => defender.has(ability::WARD_DEATH),
        School::Chaos => defender.has(ability::CHAOS_PROTECTION),
        School::Order | School::Nature | School::Might => false,
    };
    // Might has no ward at all.
    school != School::Might && (by_spell || by_ability)
}

/// The last limits (rva `1ee540`): no more than the target's remaining hit points
/// (`already_dealt` is damage it has already taken in this attack), then `block` takes 30%
/// off, rounding to nearest.
///
/// Not done yet: an attacker ability 0x42 adds a bonus based on the target's hit points
/// when the target is a living creature; the condition isn't understood (see damage.md).
pub fn finalize_damage(damage: i32, defender: &Defender, already_dealt: i32, log: &mut Vec<DamageEvent>) -> i32 {
    let mut damage = damage;
    let left = (defender.total_hit_points - already_dealt).max(0);
    if damage > left {
        damage = left;
        log.push(DamageEvent::LimitedByHitPoints { damage });
    }
    if defender.has(ability::BLOCK) {
        damage = (damage * 7 + 5) / 10;
        log.push(DamageEvent::Blocked { damage });
    }
    damage
}

/// A melee hit: dice, modifiers, limits. The order is the one of `get_melee_damage`,
/// with the dice roll in place of the average (how the real attack chains the three
/// functions is still to be checked in `attack.cpp`).
pub fn melee_hit(
    attacker: &Attacker,
    defender: &Defender,
    moved: i32,
    luck: Luck,
    luck_multiplier: f64,
    dice: &mut CombatRandom,
    log: &mut Vec<DamageEvent>,
) -> i32 {
    let base = roll_base_damage(attacker, defender.id, dice);
    log.push(DamageEvent::Rolled { damage: base });
    let damage = modify_damage(base, attacker, defender, false, moved, luck, luck_multiplier, log);
    finalize_damage(damage, defender, 0, log)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orc() -> Attacker {
        // an orc from table.creatures: damage 1-3, attack 11
        Attacker {
            id: 1,
            number: 10,
            damage_low: 1,
            damage_high: 3,
            offense: 11.0,
            defense_reduction: 1.0,
            school: School::Chaos,
            abilities: vec![],
            giant_bonus_effect: false,
        }
    }

    fn target() -> Defender {
        // defense 11, so attack/defense = 1 against the orc
        Defender {
            id: 2,
            defense_basic: 11.0,
            defense_bonus: 1.0,
            abilities: vec![],
            warded_against: vec![],
            giant: false,
            total_hit_points: 1000,
        }
    }

    fn modify(base: i32, a: &Attacker, d: &Defender) -> i32 {
        modify_damage(base, a, d, false, 0, Luck::Normal, 2.0, &mut Vec::new())
    }

    #[test]
    fn equal_attack_and_defense_keep_the_damage() {
        assert_eq!(average_base_damage(&orc()), 20); // (1 + 3) * 10 / 2
        assert_eq!(modify(20, &orc(), &target()), 20);
    }

    #[test]
    fn ratio_is_clamped() {
        let mut d = target();
        d.defense_basic = 1000.0; // 11 / 1000 is below 0.05
        assert_eq!(damage_modifier(&orc(), &d), 0.05);
        assert_eq!(modify(100, &orc(), &d), 5);
        d.defense_basic = 0.1; // 11 / 0.1 = 110, above 20
        assert_eq!(damage_modifier(&orc(), &d), 20.0);
        assert_eq!(modify(100, &orc(), &d), 2000);
        d.defense_basic = 0.0; // division by zero gives infinity, still 20
        assert_eq!(damage_modifier(&orc(), &d), 20.0);
    }

    #[test]
    fn ratio_keeps_x87_intermediate_precision() {
        // Original exe, RVA 250a60: rounding the denominator to f32 loses one ULP.
        let mut a = orc();
        let mut d = target();
        a.offense = 0.1;
        d.defense_basic = 0.1;
        d.defense_bonus = 1.5;
        assert_eq!(damage_modifier(&a, &d).to_bits(), 0x3f2aaaab);
        a.defense_reduction = 0.5;
        assert_eq!(damage_modifier(&a, &d).to_bits(), 0x3eaaaaab);
    }

    #[test]
    fn scaling_keeps_the_coefficient_at_crt_precision() {
        let mut a = orc();
        let mut d = target();
        a.offense = 5.0;
        d.defense_basic = 6.0;
        // Original RVA 1edf30: prematurely storing 5/6 to f32 gives 4, not 5.
        assert_eq!(modify(6, &a, &d), 5);
    }

    #[test]
    fn defense_reduction_against_bonus() {
        let mut a = orc();
        let mut d = target();
        d.defense_bonus = 1.5;
        // no reduction: 11 / (11 * 1.5)
        assert!((damage_modifier(&a, &d) - 1.0 / 1.5).abs() < 1e-6);
        // reduction 1.2 <= 1.5 multiplies the attack: 11 * 1.2 / 16.5 = 0.8
        a.defense_reduction = 1.2;
        assert!((damage_modifier(&a, &d) - 0.8).abs() < 1e-6);
        // reduction 2 > 1.5 cancels the bonus: 11 / 11
        a.defense_reduction = 2.0;
        assert_eq!(damage_modifier(&a, &d), 1.0);
        // but a penalty (bonus below 1) stays: 11 / (11 * 0.5)
        d.defense_bonus = 0.5;
        assert_eq!(damage_modifier(&a, &d), 2.0);
    }

    #[test]
    fn ward_fire_cold_and_minimum() {
        let mut d = target();
        d.warded_against = vec![School::Chaos];
        assert_eq!(modify(100, &orc(), &d), 66); // 100 * 100 / 150 = 66.67, cut to 66
        let mut a = orc();
        a.school = School::Might; // nothing wards against Might
        assert_eq!(modify(100, &a, &d), 100);

        let mut a = orc();
        a.abilities = vec![ability::BREATH_ATTACK, ability::COLD_ATTACK];
        let mut d = target();
        d.abilities = vec![ability::FIRE_RESISTANCE, ability::COLD_RESISTANCE];
        assert_eq!(modify(101, &a, &d), 25); // 101 >> 1 = 50, >> 1 = 25
        assert_eq!(modify(1, &a, &d), 1); // 1 >> 1 >> 1 = 0, raised to 1

        // area effect counts as fire only when shooting
        a.abilities = vec![ability::LARGE_AREA_EFFECT];
        assert_eq!(modify(100, &a, &d), 100);
        let ranged = modify_damage(100, &a, &d, true, 0, Luck::Normal, 2.0, &mut Vec::new());
        assert_eq!(ranged, 50);
    }

    #[test]
    fn luck_charge_and_giants() {
        let a = orc();
        let d = target();
        let good = modify_damage(15, &a, &d, false, 0, Luck::Good, 1.25, &mut Vec::new());
        assert_eq!(good, 18); // 15 * 1.25 = 18.75, cut to 18
        let bad = modify_damage(15, &a, &d, false, 0, Luck::Bad, 2.0, &mut Vec::new());
        assert_eq!(bad, 7); // 15 / 2 = 7.5, cut to 7

        let mut charger = orc();
        charger.abilities = vec![ability::CHARGING];
        // moved 700: (700 * 10 / 350 + 85) = 105, so 100 * 105 / 100
        assert_eq!(modify_damage(100, &charger, &d, false, 700, Luck::Normal, 2.0, &mut Vec::new()), 105);
        // not above 500: no bonus; and never when shooting
        assert_eq!(modify_damage(100, &charger, &d, false, 500, Luck::Normal, 2.0, &mut Vec::new()), 100);
        assert_eq!(modify_damage(100, &charger, &d, true, 700, Luck::Normal, 2.0, &mut Vec::new()), 100);

        let mut giant = target();
        giant.giant = true;
        let mut slayer = orc();
        slayer.abilities = vec![ability::SLAYER_MELEE];
        assert_eq!(modify(10, &slayer, &giant), 20);
        slayer.abilities = vec![];
        slayer.giant_bonus_effect = true;
        assert_eq!(modify(11, &slayer, &giant), 16); // 11 + 11 / 2
    }

    #[test]
    fn hit_points_limit_then_block() {
        let mut d = target();
        d.total_hit_points = 30;
        let mut log = Vec::new();
        assert_eq!(finalize_damage(50, &d, 10, &mut log), 20); // only 30 - 10 left
        d.abilities = vec![ability::BLOCK];
        assert_eq!(finalize_damage(15, &d, 0, &mut log), 11); // (15 * 7 + 5) / 10 = 11
        assert_eq!(finalize_damage(50, &d, 30, &mut log), 0); // nothing left to hit
    }

    #[test]
    fn rolled_damage_stays_in_range_and_repeats_with_the_seed() {
        let mut small = orc();
        small.number = 5;
        let mut big = orc();
        big.number = 1000;
        for seed in 0..50 {
            let mut dice = CombatRandom::new(seed, 20);
            let s = roll_base_damage(&small, 2, &mut dice);
            assert!((5..=15).contains(&s), "5 orcs rolled {s}");
            let b = roll_base_damage(&big, 2, &mut dice);
            // ten rolls of 1..=3 scaled by 100
            assert!((1000..=3000).contains(&b) && b % 100 == 0, "1000 orcs rolled {b}");
        }
        let hit = |seed| {
            let mut dice = CombatRandom::new(seed, 20);
            melee_hit(&orc(), &target(), 0, Luck::Normal, 2.0, &mut dice, &mut Vec::new())
        };
        assert_eq!(hit(42), hit(42));
    }

    #[test]
    fn log_tells_the_story() {
        let mut d = target();
        d.warded_against = vec![School::Chaos];
        d.abilities = vec![ability::BLOCK];
        let mut log = Vec::new();
        let damage = modify_damage(30, &orc(), &d, false, 0, Luck::Normal, 2.0, &mut log);
        let damage = finalize_damage(damage, &d, 0, &mut log);
        assert_eq!(damage, 14); // 30 -> ward 20 -> block (20 * 7 + 5) / 10 = 14
        assert_eq!(
            log,
            [
                DamageEvent::AttackVsDefense { ratio: 1.0, damage: 30 },
                DamageEvent::Ward { damage: 20 },
                DamageEvent::Blocked { damage: 14 },
            ]
        );
    }
}
