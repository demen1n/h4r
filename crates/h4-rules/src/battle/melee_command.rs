//! The melee attack and the shot commands on a [`BattleState`]: the plan from `battle::melee`
//! (who strikes, in what order), the retaliation check from `battle::retaliation`,
//! and the damage chain `attack_hit` → `add_damage` → `apply_damage`.
//!
//! What is *not* here yet (all of it is an input the caller gives, or a plain
//! default, never a guess about the exe):
//!
//! - target luck (#66): every strike rolls normal luck;
//! - the approach and `moved` from pathfinding (#62): the caller says how far the
//!   attacker moved;
//! - effect eligibility (`1b6120`), the martyr redirect, reactions after a hit;
//! - hero bonuses to attack and defense (#71): the table values are used as they are;
//! - the creature abilities and effects of each stack: the table has no ability
//!   lists in `CreatureStats`, so the caller passes them in [`StackRules`].

use crate::battle::luck::TargetLuck;
use crate::battle::melee::{
    abilities as melee_abilities, number_of_attacks, plan_melee_attack, plan_ranged_attack, AttackCount,
    Continuation, MeleeSituation, RangedSituation, ReturnFire, Striker,
};
use crate::battle::retaliation::{
    can_retaliate, can_shoot, effects as retaliation_effects, Footprint, RetaliationCheck,
};
use crate::battle::state::{BattleState, Losses, UnitId, UnitKind};
use crate::damage::{attack_hit, Attacker, Defender, HitOptions, School};

/// What the rules know about one stack beyond its table stats.
#[derive(Debug, Clone)]
pub struct StackRules {
    /// Numbers of the game's creature ability enum.
    pub abilities: Vec<u8>,
    /// Effects (`t_spell`) currently on the stack.
    pub effects: Vec<u16>,
    /// The school of the creature (its alignment), for wards.
    pub school: School,
}

impl Default for StackRules {
    fn default() -> Self {
        Self { abilities: Vec::new(), effects: Vec::new(), school: School::Might }
    }
}

/// Inputs of one melee attack that the battle state does not hold.
#[derive(Debug, Clone, Default)]
pub struct MeleeContext {
    pub attacker: StackRules,
    pub target: StackRules,
    /// How far the attacker moved this turn (`charging` and the target's first
    /// strike look at it; see `MeleeSituation::moved`).
    pub moved: i32,
}

/// One strike that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrikeReport {
    pub by: Striker,
    pub retaliation: bool,
    /// Damage the victim accepted (can be less than rolled).
    pub damage: i32,
    pub losses: Losses,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MeleeReport {
    pub strikes: Vec<StrikeReport>,
}

/// Everything one strike needs, copied out of the state so the dice can borrow it.
struct Fighter {
    id: UnitId,
    attacker: Attacker,
    defender: Defender,
}

fn fighter(state: &BattleState, id: UnitId, rules: &StackRules) -> Result<Fighter, String> {
    let unit = state.unit(id).ok_or("unknown battle unit")?;
    let UnitKind::Creature { stats, count, .. } = &unit.kind else {
        return Err("heroes do not fight in melee yet (#71)".into());
    };
    let total = unit.total_hits().ok_or("stack has no hit points")?;
    Ok(Fighter {
        id,
        attacker: Attacker {
            id: id.0,
            number: *count,
            damage_low: stats.damage_low,
            damage_high: stats.damage_high,
            offense: stats.attack as f32,
            defense_reduction: 1.0,
            school: rules.school,
            abilities: rules.abilities.clone(),
            giant_bonus_effect: false,
        },
        defender: Defender {
            id: id.0,
            defense_basic: stats.defense as f32,
            defense_bonus: 1.0,
            abilities: rules.abilities.clone(),
            warded_against: Vec::new(),
            giant: false,
            total_hit_points: total,
        },
    })
}

/// Roll one strike's damage (the dice go through the battle, so they are logged).
/// Nothing is applied yet: simultaneous strikes both roll before any loss.
///
/// The exe passes the damage roll whether a castle wall stands between the two
/// (`1701f0` from the striker's center to the victim's, [`crate::battle::wall_line`]).
/// Castles aren't in the battle state yet (#74), so here it is always
/// [`crate::damage::AcrossWall::No`], the default of [`HitOptions`].
fn roll_strike(state: &mut BattleState, from: &Fighter, to: &Fighter, moved: i32) -> i32 {
    roll_hit(state, from, to, HitOptions { moved, ..HitOptions::default() })
}

fn roll_hit(state: &mut BattleState, from: &Fighter, to: &Fighter, options: HitOptions) -> i32 {
    // The per-step log of the damage chain is for debugging; the battle log
    // keeps the dice.
    let mut steps = Vec::new();
    attack_hit(&from.attacker, &to.defender, &options, state, &mut steps)
}

fn footprint(state: &BattleState, id: UnitId) -> Result<Footprint, String> {
    let position = state
        .unit(id)
        .and_then(|u| u.position)
        .ok_or("both stacks must be deployed")?;
    // The check works in 16ths of a cell; our positions are whole cells.
    Ok(Footprint {
        position: (position.at.0 * 16, position.at.1 * 16),
        size: i32::from(position.size),
    })
}

fn creature_shots(state: &BattleState, id: UnitId) -> i32 {
    match state.unit(id).map(|u| &u.kind) {
        Some(UnitKind::Creature { shots, .. }) => *shots,
        _ => 0,
    }
}

/// Run a whole melee attack of `attacker` on `target`, in the order the exe
/// uses (`decomp/melee_attack.md`): the plan, the strikes (a retaliation is
/// simultaneous unless first strike says otherwise), the delayed retaliation,
/// then the attacker's extra strikes, which are never answered.
///
/// The retaliation flag is set on the target the way the exe does it. A stack
/// that dies stops the exchange; its answer is lost only if it died before it
/// could strike (simultaneous strikes both happen).
pub fn melee_attack(
    state: &mut BattleState,
    attacker: UnitId,
    target: UnitId,
    ctx: &MeleeContext,
) -> Result<MeleeReport, String> {
    let (a_unit, t_unit) = (
        state.unit(attacker).ok_or("unknown attacker")?,
        state.unit(target).ok_or("unknown target")?,
    );
    if a_unit.side == t_unit.side {
        return Err("a stack cannot attack its own side".into());
    }
    if !a_unit.alive() || !t_unit.alive() {
        return Err("both stacks must be alive".into());
    }
    let attacks = number_of_attacks(&AttackCount {
        ranged: false,
        abilities: &ctx.attacker.abilities,
        effects: &ctx.attacker.effects,
        shots: creature_shots(state, attacker),
    });
    let flag = state.retaliation_flag(target).ok_or("target is not a creature stack")?;
    let can_answer = can_retaliate(&RetaliationCheck {
        n: 1,
        ranged: false,
        target_flag: flag,
        target_effects: &ctx.target.effects,
        target_abilities: &ctx.target.abilities,
        attacker_abilities: &ctx.attacker.abilities,
        attacker_effects: &ctx.attacker.effects,
        attacker: footprint(state, attacker)?,
        target: footprint(state, target)?,
        ..Default::default()
    });
    let target_can_act = can_act(&ctx.target);
    let plan = plan_melee_attack(&MeleeSituation {
        can_retaliate: can_answer,
        attacker_abilities: &ctx.attacker.abilities,
        defender_abilities: &ctx.target.abilities,
        defender_can_act: target_can_act,
        moved: ctx.moved,
        attacks,
        ..Default::default()
    });
    let unlimited = !plan.mark_retaliated.unwrap_or(true);
    if let Some(value) = plan.mark_retaliated {
        state.set_retaliated(target, value)?;
    }

    let mut report = MeleeReport::default();
    // The plan's own strikes: one alone, or two together.
    let rolled: Vec<(Striker, bool, bool)> = plan
        .strikes
        .iter()
        .map(|s| (s.by, s.sequential, s.retaliation))
        .collect();
    let simultaneous = rolled.len() == 2;
    if simultaneous {
        // Both roll against the stacks as they stand, then both are applied.
        // Each striker turns toward its victim (`16e520` → `1684a0`).
        face(state, attacker, target)?;
        face(state, target, attacker)?;
        let a = fighter(state, attacker, &ctx.attacker)?;
        let t = fighter(state, target, &ctx.target)?;
        let by_attacker = roll_strike(state, &a, &t, ctx.moved);
        let by_target = roll_strike(state, &t, &a, 0);
        let hit_target = state.add_damage(target, by_attacker, false)?;
        let hit_attacker = state.add_damage(attacker, by_target, false)?;
        let on_target = state.apply_damage(target)?;
        let on_attacker = state.apply_damage(attacker)?;
        report.strikes.push(StrikeReport {
            by: Striker::Attacker,
            retaliation: false,
            damage: hit_target,
            losses: on_target,
        });
        report.strikes.push(StrikeReport {
            by: Striker::Defender,
            retaliation: true,
            damage: hit_attacker,
            losses: on_attacker,
        });
    } else if let Some(&(by, _, retaliation)) = rolled.first() {
        strike(state, &mut report, attacker, target, ctx, by, retaliation)?;
    }

    match plan.then {
        Continuation::EndMeleeAttack { strikes_left } => {
            extra_strikes(state, &mut report, attacker, target, ctx, strikes_left)?;
        }
        Continuation::BeginRetaliation => {
            // The target answers now if it is still alive and able to act.
            let alive = state.unit(target).is_some_and(|u| u.alive())
                && state.unit(attacker).is_some_and(|u| u.alive());
            if alive && target_can_act {
                state.set_retaliated(target, !unlimited)?;
                strike(state, &mut report, attacker, target, ctx, Striker::Defender, true)?;
            }
            extra_strikes(state, &mut report, attacker, target, ctx, attacks - 1)?;
        }
    }
    Ok(report)
}

/// "Can act" without the alive part (rva `155ea0`): none of the effects 0x0b, 0xba, 0x9a.
fn can_act(rules: &StackRules) -> bool {
    !rules.effects.iter().any(|e| {
        *e == 0x0b || *e == retaliation_effects::INCAPACITATING_BA || *e == retaliation_effects::INCAPACITATING_9A
    })
}

fn alive(state: &BattleState, id: UnitId) -> bool {
    state.unit(id).is_some_and(|u| u.alive())
}

/// One strike on its own: roll, add the damage, turn it into losses.
fn strike(
    state: &mut BattleState,
    report: &mut MeleeReport,
    attacker: UnitId,
    target: UnitId,
    ctx: &MeleeContext,
    by: Striker,
    retaliation: bool,
) -> Result<(), String> {
    let (from, to, from_rules, to_rules, moved) = match by {
        Striker::Attacker => (attacker, target, &ctx.attacker, &ctx.target, ctx.moved),
        Striker::Defender => (target, attacker, &ctx.target, &ctx.attacker, 0),
    };
    if !state.unit(from).is_some_and(|u| u.alive()) || !state.unit(to).is_some_and(|u| u.alive()) {
        return Ok(());
    }
    face(state, from, to)?;
    let f = fighter(state, from, from_rules)?;
    let t = fighter(state, to, to_rules)?;
    let damage = roll_strike(state, &f, &t, moved);
    let accepted = state.add_damage(t.id, damage, false)?;
    let losses = state.apply_damage(t.id)?;
    report.strikes.push(StrikeReport { by, retaliation, damage: accepted, losses });
    Ok(())
}

/// A striker turns toward its victim before the blow: the strike animation (`16e520`)
/// takes the direction from the striker's center to the victim's (`1684a0`).
fn face(state: &mut BattleState, who: UnitId, whom: UnitId) -> Result<(), String> {
    let facing = state.facing_toward(who, whom)?;
    state.turn(who, facing)
}

/// The attacker's further strikes: nobody answers them.
fn extra_strikes(
    state: &mut BattleState,
    report: &mut MeleeReport,
    attacker: UnitId,
    target: UnitId,
    ctx: &MeleeContext,
    count: i32,
) -> Result<(), String> {
    for _ in 0..count.max(0) {
        strike(state, report, attacker, target, ctx, Striker::Attacker, false)?;
    }
    Ok(())
}

/// `unlimited_shots`: a shot costs nothing (checked before the shots field is decreased).
pub const UNLIMITED_SHOTS: u8 = 0x4f;

/// Inputs of one shot that the battle state does not hold.
#[derive(Debug, Clone, Default)]
pub struct ShotContext {
    pub attacker: StackRules,
    pub target: StackRules,
    /// The distance penalty from [`crate::battle::shooting::shot_range_penalty`];
    /// 0 means the shot cannot be made. The caller supplies the battlefield view
    /// that function needs (a field of the siege rules is not ported, #74).
    pub penalty: i32,
    /// The target's luck number for its luck roll (#66).
    pub target_luck: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotReport {
    pub strike: StrikeReport,
    /// What the target's luck roll did to the hit.
    pub luck: crate::battle::luck::TargetLuck,
}

/// One shot of `attacker` at `target`, the order of the exe's damage routine
/// (rva `175ea0`, ranged branch, `decomp/shooting.md`):
///
/// ```text
/// one shot fewer unless unlimited_shots (rva 168933, in the shot animation's start)
/// damage = attack_hit(ranged, moved 0, normal luck)
/// if penalty > 1: damage /= penalty          // truncating
/// damage = max(damage, 1)
/// accepted = add_damage(damage, luck roll of the target: yes, permanent: no)
/// then the losses
/// ```
///
/// Before the damage the shooter turns toward the target: the shot animation
/// (`168ae0` → `168720`, action `ranged`) takes the direction `11ede0` from the
/// shooter's footprint center to the center of what the shot hits, both in 1/16 of a
/// cell — the same direction as [`BattleState::facing_toward`] (the centers differ
/// from `1684a0`'s only by a power-of-two scale). The exe aims at the object the line
/// of fire actually hits (`16fe00`), which can be an obstacle in the way; here the
/// target is always the stack, a blocked line isn't modelled.
///
/// This is one shot alone, never answered. A whole ranged attack — the number of
/// shots, the target shooting back — is [`ranged_attack`].
pub fn shoot(
    state: &mut BattleState,
    attacker: UnitId,
    target: UnitId,
    ctx: &ShotContext,
) -> Result<ShotReport, String> {
    check_shot(state, attacker, target, ctx.penalty)?;
    let (damage, luck) = fire(state, attacker, target, &ctx.attacker, &ctx.target, ctx.penalty, ctx.target_luck)?;
    let losses = state.apply_damage(target)?;
    Ok(ShotReport { strike: StrikeReport { by: Striker::Attacker, retaliation: false, damage, losses }, luck })
}

fn check_shot(state: &BattleState, attacker: UnitId, target: UnitId, penalty: i32) -> Result<(), String> {
    let (a_unit, t_unit) = (
        state.unit(attacker).ok_or("unknown attacker")?,
        state.unit(target).ok_or("unknown target")?,
    );
    if a_unit.side == t_unit.side {
        return Err("a stack cannot shoot its own side".into());
    }
    if !a_unit.alive() || !t_unit.alive() {
        return Err("both stacks must be alive".into());
    }
    if penalty <= 0 {
        return Err("the shot cannot be made".into());
    }
    if creature_shots(state, attacker) <= 0 {
        return Err("the stack has no shots left".into());
    }
    Ok(())
}

/// One shot up to the pending damage: spend the shot, turn, roll, divide by the
/// penalty, the victim's luck roll, `add_damage`. Losses are not applied here, so two
/// shots fired together both roll against whole stacks.
fn fire(
    state: &mut BattleState,
    from: UnitId,
    to: UnitId,
    from_rules: &StackRules,
    to_rules: &StackRules,
    penalty: i32,
    victim_luck: i32,
) -> Result<(i32, TargetLuck), String> {
    if !from_rules.abilities.contains(&UNLIMITED_SHOTS) {
        state.spend_shot(from)?;
    }
    let f = fighter(state, from, from_rules)?;
    let t = fighter(state, to, to_rules)?;
    face(state, from, to)?;
    let mut damage = roll_hit(state, &f, &t, HitOptions { ranged: true, ..HitOptions::default() });
    // `cmp ecx, 1; jle`: a penalty of 0 or 1 doesn't divide (a shot back needs no aim).
    if penalty > 1 {
        damage /= penalty;
    }
    damage = damage.max(1);
    state.add_damage_with_luck(to, from, damage, false, victim_luck)
}

/// Inputs of a whole ranged attack that the battle state does not hold.
#[derive(Debug, Clone, Default)]
pub struct RangedContext {
    pub attacker: StackRules,
    pub target: StackRules,
    /// The attacker's distance penalty against the target
    /// ([`crate::battle::shooting::shot_range_penalty`]); 0 means it can't shoot.
    pub penalty: i32,
    /// The same penalty for the target shooting back at the attacker. The exe divides
    /// only by a penalty above 1, so 0 here is a shot back at full damage.
    pub return_penalty: i32,
    /// Luck numbers of the two stacks, for the luck roll of whoever is hit (#66).
    pub attacker_luck: i32,
    pub target_luck: i32,
    /// The target's count in the battlefield's threat map (`10d200`; it seems to count
    /// enemies next to it). Anything but 0 keeps it from shooting back.
    pub target_threats: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RangedReport {
    /// Every shot in the order it was fired (a shot back has `retaliation` set).
    pub shots: Vec<ShotReport>,
}

/// A whole ranged attack of `attacker` on `target`, as the exe runs it
/// (`decomp/return_fire.md`; shot start `1693d0`, shot back `168d00`, the delayed
/// answer `169ca0`, the further shots `169930`):
///
/// - the number of shots is [`number_of_attacks`] for a ranged attack;
/// - the target shoots back once, if [`plan_ranged_attack`] allows it — usually
///   **together** with the first shot (both roll before any loss), after it with the
///   attacker's `ranged_first_strike`; shooting back sets the retaliation flag (not
///   with `unlimited_retaliation`) and costs the target a shot;
/// - the attacker's other shots follow while both stacks stand; nobody answers them.
///
/// Read from the disassembly, not executed against the exe. Not here: the line of
/// fire hitting something else on the way (the exe shoots back at whatever the line
/// from the target to the attacker meets first), area shooters (`1faa30`), poison
/// (#173).
pub fn ranged_attack(
    state: &mut BattleState,
    attacker: UnitId,
    target: UnitId,
    ctx: &RangedContext,
) -> Result<RangedReport, String> {
    check_shot(state, attacker, target, ctx.penalty)?;
    let shots_now = |state: &BattleState| {
        number_of_attacks(&AttackCount {
            ranged: true,
            abilities: &ctx.attacker.abilities,
            effects: &ctx.attacker.effects,
            shots: creature_shots(state, attacker),
        })
    };
    let attacks = shots_now(state);
    let retaliated = |state: &BattleState| -> Result<bool, String> {
        Ok(state.retaliation_flag(target).ok_or("target is not a creature stack")?.retaliated)
    };
    let target_can_shoot = |state: &BattleState| {
        can_shoot(&ctx.target.abilities, &ctx.target.effects, creature_shots(state, target), ctx.target_threats)
    };
    let target_can_act = can_act(&ctx.target);
    let plan = plan_ranged_attack(&RangedSituation {
        target_retaliated: retaliated(state)?,
        target_can_shoot: target_can_shoot(state),
        target_can_act,
        attacker_abilities: &ctx.attacker.abilities,
        target_abilities: &ctx.target.abilities,
    });
    let keep_flag = !ctx.target.abilities.contains(&melee_abilities::UNLIMITED_RETALIATION);
    let shot = |by, retaliation, damage, losses, luck| ShotReport {
        strike: StrikeReport { by, retaliation, damage, losses },
        luck,
    };

    let mut report = RangedReport::default();
    let further = match plan {
        ReturnFire::Simultaneous => {
            let (to_target, luck_t) =
                fire(state, attacker, target, &ctx.attacker, &ctx.target, ctx.penalty, ctx.target_luck)?;
            state.set_retaliated(target, keep_flag)?;
            let (to_attacker, luck_a) =
                fire(state, target, attacker, &ctx.target, &ctx.attacker, ctx.return_penalty, ctx.attacker_luck)?;
            let on_target = state.apply_damage(target)?;
            let on_attacker = state.apply_damage(attacker)?;
            report.shots.push(shot(Striker::Attacker, false, to_target, on_target, luck_t));
            report.shots.push(shot(Striker::Defender, true, to_attacker, on_attacker, luck_a));
            attacks - 1
        }
        ReturnFire::None | ReturnFire::AfterFirstShot => {
            let (damage, luck) =
                fire(state, attacker, target, &ctx.attacker, &ctx.target, ctx.penalty, ctx.target_luck)?;
            let losses = state.apply_damage(target)?;
            report.shots.push(shot(Striker::Attacker, false, damage, losses, luck));
            if plan == ReturnFire::None {
                attacks - 1
            } else {
                // `169ca0`: both stacks still stand and the target can act, then the
                // shot back checks everything again (`168d00`).
                if alive(state, attacker) && alive(state, target) && target_can_act
                    && !retaliated(state)?
                    && target_can_shoot(state)
                {
                    state.set_retaliated(target, keep_flag)?;
                    let (damage, luck) = fire(
                        state,
                        target,
                        attacker,
                        &ctx.target,
                        &ctx.attacker,
                        ctx.return_penalty,
                        ctx.attacker_luck,
                    )?;
                    let losses = state.apply_damage(attacker)?;
                    report.shots.push(shot(Striker::Defender, true, damage, losses, luck));
                }
                // The count is taken again here, after the first shot was spent: with
                // fewer shots than attacks this is one less than the other paths give.
                shots_now(state) - 1
            }
        }
    };
    // `169930`: one more shot while the attacker stands; it stops when the target fell.
    for _ in 0..further.max(0) {
        if !alive(state, attacker) || !alive(state, target) {
            break;
        }
        let (damage, luck) = fire(state, attacker, target, &ctx.attacker, &ctx.target, ctx.penalty, ctx.target_luck)?;
        let losses = state.apply_damage(target)?;
        report.shots.push(shot(Striker::Attacker, false, damage, losses, luck));
    }
    Ok(report)
}
