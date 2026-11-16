//! The melee command on a battle: simultaneous retaliation, the retaliation
//! flag, extra strikes and snapshot replay. Uses fixture stats, no game data.

use h4_rules::battle::melee::Striker;
use h4_rules::battle::melee_command::{melee_attack, MeleeContext, StackRules};
use h4_rules::battle::state::{BattleState, UnitId, UnitPosition};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};
use h4_rules::creatures::CreatureStats;
use h4_state::army::{Army, ArmyId, MapArmy, Stack, SLOTS};
use h4_state::Pos;

fn stats() -> Vec<CreatureStats> {
    vec![CreatureStats {
        keyword: "fixture".into(),
        name: "Fixture creature".into(),
        level: 1,
        hit_points: 10,
        damage_low: 2,
        damage_high: 4,
        attack: 4,
        defense: 4,
        speed: 6,
        movement: 20,
        shots: 0,
        weekly_growth: 10,
        alignment: 0,
        experience: 7,
    }]
}

fn army(id: u32, count: i32) -> MapArmy {
    MapArmy {
        id: ArmyId(id),
        owner: Some(0),
        at: Pos { level: 0, row: 3, col: 4 },
        army: Army::of(&[Stack { creature: 0, count }]),
        movement_left: [0.; SLOTS],
        growth_remainder: [0; SLOTS],
    }
}

fn battle(attackers: i32, defenders: i32) -> (BattleState, UnitId, UnitId) {
    let mut state = BattleState::from_armies(
        &army(10, attackers),
        &army(20, defenders),
        &stats(),
        BattlefieldGrid::standard(CellSurface::default()),
        4321,
    )
    .unwrap();
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    state.deploy(ids[0], UnitPosition { at: (48, 48), size: 1 }).unwrap();
    state.deploy(ids[1], UnitPosition { at: (49, 48), size: 1 }).unwrap();
    (state, ids[0], ids[1])
}

#[test]
fn a_plain_attack_is_answered_once_and_both_sides_lose_creatures() {
    let (mut state, a, t) = battle(20, 20);
    let report = melee_attack(&mut state, a, t, &MeleeContext { moved: 10, ..Default::default() }).unwrap();
    assert_eq!(report.strikes.len(), 2);
    assert_eq!(report.strikes[0].by, Striker::Attacker);
    assert!(report.strikes[1].retaliation);
    assert!(report.strikes.iter().all(|s| s.damage > 0));
    // The answer came from the whole stack: both lost something.
    assert!(state.unit(t).unwrap().total_hits().unwrap() < 200);
    assert!(state.unit(a).unwrap().total_hits().unwrap() < 200);
    assert!(state.retaliation_flag(t).unwrap().retaliated);
}

#[test]
fn a_second_attack_in_the_same_round_is_not_answered() {
    let (mut state, a, t) = battle(20, 20);
    melee_attack(&mut state, a, t, &MeleeContext { moved: 10, ..Default::default() }).unwrap();
    let report = melee_attack(&mut state, a, t, &MeleeContext { moved: 10, ..Default::default() }).unwrap();
    assert_eq!(report.strikes.len(), 1);
}

#[test]
fn extra_strikes_come_after_the_answer_and_are_not_answered() {
    let (mut state, a, t) = battle(100, 100);
    let ctx = MeleeContext {
        moved: 10,
        attacker: StackRules { abilities: vec![0x46], ..Default::default() }, // strikes twice
        ..Default::default()
    };
    let report = melee_attack(&mut state, a, t, &ctx).unwrap();
    let who: Vec<_> = report.strikes.iter().map(|s| (s.by, s.retaliation)).collect();
    assert_eq!(
        who,
        [(Striker::Attacker, false), (Striker::Defender, true), (Striker::Attacker, false)]
    );
}

#[test]
fn a_stack_cannot_attack_its_own_side_or_a_dead_one() {
    let (mut state, a, t) = battle(1, 1);
    assert!(melee_attack(&mut state, a, a, &MeleeContext::default()).is_err());
    state.kill(t, 1).unwrap();
    assert!(melee_attack(&mut state, a, t, &MeleeContext::default()).is_err());
}

#[test]
fn the_flag_and_dice_survive_a_snapshot() {
    let (mut state, a, t) = battle(20, 20);
    melee_attack(&mut state, a, t, &MeleeContext { moved: 10, ..Default::default() }).unwrap();
    let mut loaded = BattleState::from_json(&state.to_json().unwrap()).unwrap();
    assert_eq!(loaded, state);
    let ctx = MeleeContext { moved: 10, ..Default::default() };
    let first = melee_attack(&mut state, t, a, &ctx).unwrap();
    let second = melee_attack(&mut loaded, t, a, &ctx).unwrap();
    assert_eq!(first, second);
}

#[test]
fn the_flag_is_forgotten_a_round_later_at_the_attackers_place_in_the_order() {
    use h4_rules::battle::initiative::InitiativeStats;
    let (mut state, a, t) = battle(20, 20);
    let stats = [
        (a, InitiativeStats { speed: 8, morale: 0 }),
        (t, InitiativeStats { speed: 6, morale: 0 }),
    ];
    state.begin_initiative_round(&stats).unwrap();
    assert_eq!(state.begin_next_turn().unwrap(), Some(a));
    melee_attack(&mut state, a, t, &MeleeContext { moved: 10, ..Default::default() }).unwrap();
    assert!(state.retaliation_flag(t).unwrap().retaliated);
    state.finish_turn().unwrap();
    // The target's own turn in the same round does not clear it.
    assert_eq!(state.begin_next_turn().unwrap(), Some(t));
    assert!(state.retaliation_flag(t).unwrap().retaliated);
    state.finish_turn().unwrap();
    assert_eq!(state.begin_next_turn().unwrap(), None);
    // Next round: it goes when the attacker's turn comes round again.
    state.begin_initiative_round(&stats).unwrap();
    assert_eq!(state.begin_next_turn().unwrap(), Some(a));
    assert!(!state.retaliation_flag(t).unwrap().retaliated);
}

#[test]
fn target_luck_rolls_one_logged_die_and_changes_the_accepted_damage() {
    use h4_rules::battle::luck::TargetLuck;
    use h4_rules::battle::state::CombatEvent;
    let (mut state, a, t) = battle(20, 20);
    // No hit, no die.
    assert_eq!(state.add_damage_with_luck(t, a, 0, false, 5).unwrap(), (0, TargetLuck::Normal));
    assert!(!state.events().iter().any(|e| matches!(e, CombatEvent::Rolled { purpose: 0x1b, .. })));
    // Luck 10 helps on every die (0..9): two thirds of 30.
    let (accepted, luck) = state.add_damage_with_luck(t, a, 30, false, 10).unwrap();
    assert_eq!((accepted, luck), (20, TargetLuck::Lucky));
    assert!(state.events().iter().any(
        |e| matches!(e, CombatEvent::Rolled { first, second, purpose: 0x1b, .. } if *first == t.0 && *second == a.0)
    ));
    // Luck -10 hurts on every die: one and a half times 30.
    let (accepted, luck) = state.add_damage_with_luck(a, t, 30, false, -10).unwrap();
    assert_eq!((accepted, luck), (45, TargetLuck::Unlucky));
}
