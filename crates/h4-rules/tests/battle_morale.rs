//! Morale on a battle: the army's alignments, the loss penalty between rounds, the getter
//! and the snapshot. Uses fixture stats, no game data.

use h4_rules::battle::morale::{effect, StrengthEffects};
use h4_rules::battle::state::{BattleState, MoraleSources, UnitId};
use h4_rules::battle::Side;
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};
use h4_rules::creatures::CreatureStats;
use h4_state::army::{Army, ArmyId, MapArmy, Stack, SLOTS};
use h4_state::Pos;

fn creature(alignment: usize, hit_points: i32) -> CreatureStats {
    CreatureStats {
        keyword: format!("fixture{alignment}"),
        name: "Fixture creature".into(),
        level: 1,
        hit_points,
        damage_low: 2,
        damage_high: 4,
        attack: 4,
        defense: 4,
        speed: 6,
        movement: 20,
        shots: 0,
        weekly_growth: 10,
        alignment,
        experience: 7,
    }
}

fn army(id: u32, stacks: &[Stack]) -> MapArmy {
    MapArmy {
        id: ArmyId(id),
        owner: Some(0),
        at: Pos { level: 0, row: 3, col: 4 },
        army: Army::of(stacks),
        movement_left: [0.; SLOTS],
        growth_remainder: [0; SLOTS],
    }
}

/// Creatures: 0 life (10 HP), 1 death (10 HP), 2 order (10 HP).
fn battle() -> (BattleState, Vec<UnitId>) {
    let stats = vec![creature(0, 10), creature(2, 10), creature(1, 10)];
    let state = BattleState::from_armies(
        // life and death together: opposed
        &army(10, &[Stack { creature: 0, count: 10 }, Stack { creature: 1, count: 10 }]),
        &army(20, &[Stack { creature: 2, count: 20 }]),
        &stats,
        BattlefieldGrid::standard(CellSurface::default()),
        7,
    )
    .unwrap();
    let ids = state.units().iter().map(|u| u.id).collect();
    (state, ids)
}

fn plain<'a>(undead: &'a dyn Fn(UnitId, u8) -> bool) -> MoraleSources<'a> {
    MoraleSources {
        member_bonus: 0,
        member_morale_effect: false,
        influence: 0,
        has_ability: undead,
        has_effect: &|_| false,
    }
}

#[test]
fn alignments_and_undead_set_the_base() {
    let (state, ids) = battle();
    let nobody = |_: UnitId, _: u8| false;
    // 1 + life (0) + death (opposed, -5) = -4 for both; order alone: 1.
    assert_eq!(state.morale(ids[0], &plain(&nobody)).unwrap(), -4);
    assert_eq!(state.morale(ids[1], &plain(&nobody)).unwrap(), -4);
    assert_eq!(state.morale(ids[2], &plain(&nobody)).unwrap(), 1);
    // The death stack is undead: the life stack loses 2 more; the undead itself has none.
    let undead = |id: UnitId, a: u8| id == ids[1] && a == 0x4d;
    assert_eq!(state.morale(ids[0], &plain(&undead)).unwrap(), -6);
    assert_eq!(state.morale(ids[1], &plain(&undead)).unwrap(), 0);
    // Spells and the member's own bonus.
    let fervor = MoraleSources { has_effect: &|e| e == effect::SPIRITUAL_FERVOR, ..plain(&nobody) };
    assert_eq!(state.morale(ids[0], &fervor).unwrap(), 10);
    let bonus = MoraleSources { member_bonus: 3, member_morale_effect: true, ..plain(&nobody) };
    assert_eq!(state.morale(ids[2], &bonus).unwrap(), 9);
}

#[test]
fn losses_lower_the_morale_from_the_second_round_on() {
    let (mut state, ids) = battle();
    let none = |_| StrengthEffects::default();
    let nobody = |_: UnitId, _: u8| false;
    assert_eq!(state.update_loss_penalties(none), [0, 0]);
    assert_eq!(state.morale_losses().initial, Some([200, 200]));
    // The defenders lose half of their strength, the attackers a tenth.
    state.kill(ids[2], 10).unwrap();
    state.kill(ids[0], 2).unwrap();
    // shares: attackers (180-200)*10/200 = -1, defenders (100-200)*10/200 = -5
    assert_eq!(state.update_loss_penalties(none), [0, -4]);
    assert_eq!(state.morale(ids[2], &plain(&nobody)).unwrap(), 1 - 4);
    assert_eq!(state.side_strength(Side::Defender, none), 100);
    // The penalty and its log survive a snapshot; a tampered one doesn't load.
    let json = state.to_json().unwrap();
    assert_eq!(BattleState::from_json(&json).unwrap(), state);
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["morale"]["penalty"][1], -4);
    value["morale"]["penalty"][1] = (-3).into();
    assert!(BattleState::from_json(&value.to_string()).is_err());
}
