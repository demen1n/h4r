//! The start of a melee attack and the retaliation flag, as the exe does them
//! (`decomp/melee_attack.md`, verified by executing the original code:
//! `tests/melee_attack_vectors.tsv`).
//!
//! Three pieces live here, all plain decisions on numbers and flags:
//!
//! - [`number_of_attacks`] — how many strikes (or shots) one attack makes
//!   (rva `1f1d10`);
//! - [`RetaliationFlag`] — "this stack has already retaliated", and when that is
//!   forgotten again (rva `1f6b20`, `1f6b60` and the round reset `1ecfb0`);
//! - [`plan_melee_attack`] — who strikes first, whether the two strikes happen at
//!   the same time, and what the battle does after the strike animation
//!   (start of a melee attack, a `t_battlefield` method, rva `16ede0`);
//! - [`plan_ranged_attack`] — whether a shot is answered by a shot, and when
//!   (start of a ranged attack, rva `1693d0`; read from the disassembly only).
//!
//! What a strike does (dice, damage, losses) is not here: that is `damage` and
//! `wounds`. Connecting these rules to `BattleState` is the next step.

/// Ability numbers used only by these rules (names are the game's keywords).
pub mod abilities {
    pub const EXTRA_MELEE_ATTACK: u8 = 0x12;
    pub const EXTRA_RANGED_ATTACK: u8 = 0x13;
    pub const FIRST_STRIKE: u8 = 0x17;
    pub const FIRST_STRIKE_IMMUNITY: u8 = 0x18;
    pub const RANGED_FIRST_STRIKE: u8 = 0x34;
    pub const SHOOTS_TWICE: u8 = 0x3b;
    pub const STRIKES_TWICE: u8 = 0x46;
    pub const UNLIMITED_RETALIATION: u8 = 0x4e;
}

/// Effects (spells on the stack, `t_spell`) that add attacks. Their names in the
/// game's spell list are not checked yet, only their numbers.
pub mod effects {
    /// Makes the base number of attacks 2 instead of 1.
    pub const DOUBLE_ATTACK: u16 = 0x7a;
    /// One more attack.
    pub const EXTRA_ATTACK: u16 = 0x82;
}

/// What [`number_of_attacks`] needs to know about the attacker.
#[derive(Debug, Clone, Default)]
pub struct AttackCount<'a> {
    pub ranged: bool,
    /// The creature's abilities (numbers of the game's ability enum, as in
    /// `damage::ability` and [`abilities`]).
    pub abilities: &'a [u8],
    /// Effects currently on the stack ([`effects`] numbers).
    pub effects: &'a [u16],
    /// Shots left. Only a ranged attack looks at it.
    pub shots: i32,
}

/// How many times the stack strikes (or shoots) in one attack (rva `1f1d10`):
///
/// ```text
/// 1, or 2 with effect 0x7a; +1 with effect 0x82
/// melee:  + strikes_twice + extra_melee_attack
/// ranged: + shoots_twice + extra_ranged_attack, then at most the shots left (at least 1)
/// ```
pub fn number_of_attacks(input: &AttackCount) -> i32 {
    let has = |a: u8| input.abilities.contains(&a);
    let mut n = if input.effects.contains(&effects::DOUBLE_ATTACK) { 2 } else { 1 };
    if input.effects.contains(&effects::EXTRA_ATTACK) {
        n += 1;
    }
    if input.ranged {
        n += i32::from(has(abilities::SHOOTS_TWICE)) + i32::from(has(abilities::EXTRA_RANGED_ATTACK));
        // A stack can't shoot more often than it has shots, but the exe never
        // answers less than one (a stack with no shots left still "shoots" once).
        if n > input.shots {
            n = input.shots.max(1);
        }
    } else {
        n += i32::from(has(abilities::STRIKES_TWICE)) + i32::from(has(abilities::EXTRA_MELEE_ATTACK));
    }
    n
}

/// How far below the current priority the remembered time stamp is put, so it
/// can't match anything in the same round. The round reset adds it back.
const ROUND_SHIFT: i32 = 3_000_000;

/// "This stack has already retaliated" (`t_combat_creature+0xa9`) and the time
/// stamp that decides when it may retaliate again (`+0xb70`).
///
/// The time is the initiative priority of the stack that is acting (see
/// `battle::initiative`; priorities go down during a round). A stack that
/// retaliated may do it again one round later, at the same point of the turn
/// order: in the next round, as soon as a stack with a priority not higher than
/// the remembered one starts its turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetaliationFlag {
    pub retaliated: bool,
    /// Only meaningful while `retaliated` is set.
    pub stamp: i32,
}

impl RetaliationFlag {
    /// Mark the stack as having retaliated (rva `1f6b20`). `acting_priority` is the
    /// initiative priority of the stack whose turn it is now (rva `1f2cc0`). The stamp is written
    /// only when the flag goes from clear to set.
    pub fn set(&mut self, value: bool, acting_priority: i32) {
        if self.retaliated == value {
            return;
        }
        self.retaliated = value;
        if value {
            // 32-bit arithmetic in the exe; wrapping keeps odd fixture inputs exact.
            self.stamp = acting_priority.wrapping_sub(ROUND_SHIFT);
        }
    }

    /// The round reset (`1ecfb0`): bring the stamp back to the priority it was
    /// taken at, so the next round can reach it.
    pub fn new_round(&mut self) {
        if self.retaliated {
            self.stamp = self.stamp.wrapping_add(ROUND_SHIFT);
        }
    }

    /// Called for every stack when a new stack starts its turn (`1f6b60`, from
    /// `167d10`): the flag is forgotten once the turn order has come back to the
    /// point where the stack retaliated.
    pub fn turn_started(&mut self, acting_priority: i32) {
        if self.retaliated && acting_priority <= self.stamp {
            self.retaliated = false;
        }
    }
}

/// Everything [`plan_melee_attack`] looks at. The caller resolves each value
/// from the battle; nothing here is guessed from creature tables.
#[derive(Debug, Clone, Default)]
pub struct MeleeSituation<'a> {
    /// Answer of the retaliation check for (attacker, ranged = false, n = 1)
    /// (`decomp/retaliation.md`; not ported yet).
    pub can_retaliate: bool,
    pub attacker_abilities: &'a [u8],
    pub defender_abilities: &'a [u8],
    /// The target is alive and has none of the effects 0x0b, 0xba, 0x9a
    /// (rva `155ea0`).
    pub defender_can_act: bool,
    /// Virtual slot `+0x38` of the attacker's and the target's attackable object.
    /// Its meaning is not known; when either is true the attacker strikes first.
    pub attacker_slot_38: bool,
    pub defender_slot_38: bool,
    /// `t_battlefield+0x314`: how far the attacker moved this turn (the same
    /// number `charging` uses). Below 4 the defender's first strike is lost.
    pub moved: i32,
    /// The attacker's [`number_of_attacks`] for melee.
    pub attacks: i32,
}

/// Who swings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Striker {
    Attacker,
    Defender,
}

/// One animated strike (rva `16e520`) that adds its damage at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strike {
    pub by: Striker,
    /// The strike happens alone (`true`) or together with the other side's
    /// (`false`). The exe uses it for the target's flinch animation.
    pub sequential: bool,
    /// The strike is a retaliation (changes only the message text in the exe).
    pub retaliation: bool,
}

/// What runs after the strike animations and after the queued hits (rva `176160`) have
/// turned the pending damage into losses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuation {
    /// After the strike (rva `16ea60`): the attacker strikes again while `strikes_left > 0`
    /// (no retaliation for those), then the attack ends.
    EndMeleeAttack { strikes_left: i32 },
    /// Delayed retaliation (rva `16f130`): the target answers now, if it still can
    /// act, then `EndMeleeAttack` with the attacker's remaining strikes.
    BeginRetaliation,
}

/// How a melee attack starts (rva `16ede0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeleePlan {
    /// Strikes started now, in this order.
    pub strikes: Vec<Strike>,
    /// What follows once the strikes are resolved.
    pub then: Continuation,
    /// `Some(v)`: the target's `RetaliationFlag::set(v)` is called now
    /// (`v` is false for `unlimited_retaliation`).
    pub mark_retaliated: Option<bool>,
    /// `t_battlefield+0x2f0`: the target retaliates in this exchange. Read by
    /// the after-strike step (the panic ability); set later for `BeginRetaliation`.
    pub retaliating: bool,
}

/// Decide the start of a melee attack, as the exe does (rva `16ede0`).
///
/// Usually a retaliation is **simultaneous**: both strikes start together, so
/// both roll their damage before any losses are applied, and the target answers
/// with its whole stack. First strike changes the order. Extra strikes of the
/// attacker always come after the retaliation and are never answered.
pub fn plan_melee_attack(s: &MeleeSituation) -> MeleePlan {
    use Striker::{Attacker, Defender};
    let attacker_has = |a: u8| s.attacker_abilities.contains(&a);
    let defender_has = |a: u8| s.defender_abilities.contains(&a);
    let strike = |by, sequential, retaliation| Strike { by, sequential, retaliation };

    if !s.can_retaliate {
        return MeleePlan {
            strikes: vec![strike(Attacker, true, false)],
            then: Continuation::EndMeleeAttack { strikes_left: s.attacks - 1 },
            mark_retaliated: None,
            retaliating: false,
        };
    }

    let mut attacker_first = attacker_has(abilities::FIRST_STRIKE)
        && !defender_has(abilities::FIRST_STRIKE_IMMUNITY);
    let mut defender_first = defender_has(abilities::FIRST_STRIKE)
        && !attacker_has(abilities::FIRST_STRIKE_IMMUNITY);
    if !s.defender_can_act || s.attacker_slot_38 || s.defender_slot_38 {
        // The target can't answer at once: the attacker strikes, and the target
        // gets its chance afterwards (BeginRetaliation checks it again).
        attacker_first = true;
        defender_first = false;
    } else if !attacker_first && s.moved < 4 {
        defender_first = false;
    }
    let unlimited = defender_has(abilities::UNLIMITED_RETALIATION);

    if attacker_first == defender_first {
        MeleePlan {
            strikes: vec![strike(Attacker, false, false), strike(Defender, false, true)],
            then: Continuation::EndMeleeAttack { strikes_left: s.attacks - 1 },
            mark_retaliated: Some(!unlimited),
            retaliating: true,
        }
    } else if attacker_first {
        MeleePlan {
            strikes: vec![strike(Attacker, true, false)],
            then: Continuation::BeginRetaliation,
            mark_retaliated: None,
            retaliating: false,
        }
    } else {
        // The defender strikes first; the attacker then makes all its strikes.
        MeleePlan {
            strikes: vec![strike(Defender, true, true)],
            then: Continuation::EndMeleeAttack { strikes_left: s.attacks },
            mark_retaliated: Some(!unlimited),
            retaliating: true,
        }
    }
}

/// Everything [`plan_ranged_attack`] looks at, resolved by the caller.
#[derive(Debug, Clone, Default)]
pub struct RangedSituation<'a> {
    /// The target's retaliation flag is set (byte `+0xa9`). Unlike the melee check
    /// (`1f23c0`) the shot start reads it alone, with no `n < 2` exception.
    pub target_retaliated: bool,
    /// The target could shoot now: `retaliation::can_shoot` (ranged ability, no
    /// effect 0x2f, shots left, no threat count).
    pub target_can_shoot: bool,
    /// The target is alive and has none of the effects 0x0b, 0xba, 0x9a (rva `155ea0`).
    pub target_can_act: bool,
    pub attacker_abilities: &'a [u8],
    pub target_abilities: &'a [u8],
}

/// How a shot is answered (rva `1693d0`, `decomp/return_fire.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnFire {
    /// No answer: the attacker makes all its shots.
    None,
    /// The target shoots back at once (rva `168d00` right after the attacker's
    /// hit): both shots roll before any loss, so the target answers with its whole
    /// stack. Then the attacker's remaining shots, unanswered.
    Simultaneous,
    /// The attacker's first shot lands first; after its losses the target shoots
    /// back if both stacks still stand and it can act (rva `169ca0`). Then the
    /// remaining shots. Happens with `ranged_first_strike` on the attacker only,
    /// or when the target can't act (then the later check finds it still can't).
    AfterFirstShot,
}

/// Decide the answer to a shot, as the exe's shot start does (rva `1693d0`).
///
/// ```text
/// no answer if: target retaliated, or it can't shoot
/// a_first = attacker has ranged_first_strike (0x34)
/// d_first = target   has ranged_first_strike
/// if target can't act: a_first = true, d_first = false
/// a_first and not d_first -> AfterFirstShot, otherwise Simultaneous
/// ```
///
/// `first_strike_immunity` plays no part here, and the target's own
/// `ranged_first_strike` only cancels the attacker's: the target never shoots first.
pub fn plan_ranged_attack(s: &RangedSituation) -> ReturnFire {
    if s.target_retaliated || !s.target_can_shoot {
        return ReturnFire::None;
    }
    let mut attacker_first = s.attacker_abilities.contains(&abilities::RANGED_FIRST_STRIKE);
    let mut target_first = s.target_abilities.contains(&abilities::RANGED_FIRST_STRIKE);
    if !s.target_can_act {
        attacker_first = true;
        target_first = false;
    }
    if attacker_first && !target_first {
        ReturnFire::AfterFirstShot
    } else {
        ReturnFire::Simultaneous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_exchange_is_simultaneous() {
        let plan = plan_melee_attack(&MeleeSituation {
            can_retaliate: true,
            defender_can_act: true,
            moved: 10,
            attacks: 1,
            ..Default::default()
        });
        assert_eq!(plan.strikes.len(), 2);
        assert!(plan.strikes.iter().all(|s| !s.sequential));
        assert_eq!(plan.then, Continuation::EndMeleeAttack { strikes_left: 0 });
        assert_eq!(plan.mark_retaliated, Some(true));
    }

    #[test]
    fn a_shot_is_answered_at_once_unless_the_attacker_shoots_first() {
        let answer = |retaliated, can_shoot, can_act, a: &[u8], t: &[u8]| {
            plan_ranged_attack(&RangedSituation {
                target_retaliated: retaliated,
                target_can_shoot: can_shoot,
                target_can_act: can_act,
                attacker_abilities: a,
                target_abilities: t,
            })
        };
        let first = [abilities::RANGED_FIRST_STRIKE];
        assert_eq!(answer(false, true, true, &[], &[]), ReturnFire::Simultaneous);
        assert_eq!(answer(true, true, true, &[], &[]), ReturnFire::None);
        assert_eq!(answer(false, false, true, &[], &[]), ReturnFire::None);
        assert_eq!(answer(false, true, true, &first, &[]), ReturnFire::AfterFirstShot);
        // Both shoot first: they cancel out.
        assert_eq!(answer(false, true, true, &first, &first), ReturnFire::Simultaneous);
        // The target's own first strike changes nothing.
        assert_eq!(answer(false, true, true, &[], &first), ReturnFire::Simultaneous);
        assert_eq!(answer(false, true, false, &[], &first), ReturnFire::AfterFirstShot);
    }

    #[test]
    fn retaliation_comes_back_a_round_later_at_the_same_point() {
        let mut flag = RetaliationFlag::default();
        flag.set(true, 1006); // retaliated while a stack with priority 1006 acted
        flag.turn_started(-2006); // same round: even the slowest turn doesn't clear it
        assert!(flag.retaliated);
        flag.new_round();
        flag.turn_started(2006); // next round, faster stacks first: still set
        assert!(flag.retaliated);
        flag.turn_started(1006); // the same point of the order
        assert!(!flag.retaliated);
    }
}
