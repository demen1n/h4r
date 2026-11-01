//! Battle fixtures run without game files, graphics or a live adventure state.
use h4_rules::battle::state::{BattleState, CombatEvent, UnitId, UnitKind, UnitPosition};
use h4_rules::battle::Side;
use h4_rules::battlefield::{
    BattlefieldGrid, CellOccupant, CellSurface, CombatFootprint, PlacedObstacle, FLAG_BLOCKED,
};
use h4_rules::creatures::CreatureStats;
use h4_state::army::{Army, ArmyId, Hero, MapArmy, Slot, Stack, SLOTS};
use h4_state::Pos;

fn stats() -> Vec<CreatureStats> {
    vec![CreatureStats {
        keyword: "fixture".into(),
        name: "Fixture creature".into(),
        level: 1,
        hit_points: 10,
        damage_low: 1,
        damage_high: 3,
        attack: 4,
        defense: 5,
        speed: 6,
        movement: 20,
        shots: 8,
        weekly_growth: 10,
        alignment: 0,
        experience: 7,
    }]
}

fn army(id: u32) -> MapArmy {
    MapArmy {
        id: ArmyId(id),
        owner: Some(0),
        at: Pos {
            level: 0,
            row: 3,
            col: 4,
        },
        army: Army::of(&[Stack {
            creature: 0,
            count: 12,
        }]),
        movement_left: [123.; SLOTS],
        growth_remainder: [17; SLOTS],
    }
}

fn field() -> BattlefieldGrid {
    BattlefieldGrid::standard(CellSurface::default())
}

fn battle() -> BattleState {
    BattleState::from_armies(&army(10), &army(20), &stats(), field(), 1234).unwrap()
}

#[test]
fn creation_owns_sparse_armies_heroes_field_and_stats() {
    let mut attacker = army(10);
    attacker.army.slots[5] = attacker.army.slots[0].take();
    let mut defender = army(20);
    defender.owner = None;
    defender.army = Army::default();
    defender.army.slots[6] = Some(Slot::Hero(Hero {
        portrait: -1,
        name: "Hero-only army".into(),
        class: 0,
        male: true,
        level: 2,
        experience: 100,
    }));
    let originals = [attacker.clone(), defender.clone()];
    let mut table = stats();
    let mut ground = field();
    ground
        .add_obstacle(PlacedObstacle {
            at: (48, 48),
            footprint: CombatFootprint { size: 2 },
            // The allocator must avoid existing scenery identities.
            occupant: CellOccupant {
                id: 1,
                blocks: true,
                skip_blocking: false,
            },
        })
        .unwrap();
    let state = BattleState::from_armies(&attacker, &defender, &table, ground.clone(), 77).unwrap();
    assert_eq!([attacker.clone(), defender.clone()], originals);
    assert_eq!(state.units().len(), 2);
    assert_eq!(state.units()[0].slot, 5);
    assert_eq!(state.units()[0].side, Side::Attacker);
    assert_ne!(state.units()[0].id, UnitId(1));
    assert_eq!(state.armies()[0].slots[0], None);
    assert_eq!(state.armies()[0].slots[5], Some(state.units()[0].id));
    assert_eq!(state.armies()[1].source, ArmyId(20));
    assert_eq!(state.armies()[1].owner, None);
    assert!(matches!(&state.units()[1].kind, UnitKind::Hero(h) if h.name == "Hero-only army"));
    assert!(state.units()[1].alive());
    assert!(state.units().iter().all(|u| u.position.is_none()));
    assert_eq!(state.turns().round, 0);
    assert_eq!(state.events(), &[CombatEvent::Created { seed: 77 }]);
    assert_eq!(state.field(), &ground);
    // The battle remains independent after its inputs are changed or dropped.
    attacker.army.slots[5] = None;
    table[0].hit_points = 999;
    ground.surface_mut(50, 50).unwrap().flags = FLAG_BLOCKED;
    assert!(matches!(
        state.units()[0].kind,
        UnitKind::Creature {
            count: 12,
            top_hp: 10,
            shots: 8,
            ..
        }
    ));
    assert!(!state.field().is_blocked(50, 50, None));
    let json = state.to_json().unwrap();
    assert_eq!(BattleState::from_json(&json).unwrap(), state);
}

#[test]
fn deployment_checks_whole_footprint_and_is_atomic() {
    let mut ground = field();
    ground.surface_mut(51, 51).unwrap().flags = FLAG_BLOCKED;
    let mut state = BattleState::from_armies(&army(10), &army(20), &stats(), ground, 3).unwrap();
    let a = state.units()[0].id;
    let b = state.units()[1].id;
    state
        .deploy(
            a,
            UnitPosition {
                at: (48, 48),
                size: 2,
            },
        )
        .unwrap();
    for (x, y) in [(48, 48), (49, 48), (48, 49), (49, 49)] {
        assert!(state.field().is_blocked(x, y, None));
        assert!(!state.field().is_blocked(x, y, Some(a.0)));
    }
    for position in [
        UnitPosition {
            at: (50, 50),
            size: 2,
        }, // far corner on blocked terrain
        UnitPosition {
            at: (-1, -1),
            size: 1,
        },
        UnitPosition {
            at: (i32::MAX, i32::MAX),
            size: 2,
        },
        UnitPosition {
            at: (50, 50),
            size: 0,
        },
        UnitPosition {
            at: (50, 50),
            size: 3,
        },
    ] {
        let before = state.clone();
        assert!(state.deploy(a, position).is_err());
        assert_eq!(state, before);
    }
    let before = state.clone();
    assert!(state
        .deploy(
            b,
            UnitPosition {
                at: (49, 49),
                size: 1
            }
        )
        .is_err());
    assert_eq!(state, before);
    state
        .deploy(
            a,
            UnitPosition {
                at: (52, 52),
                size: 1,
            },
        )
        .unwrap();
    assert!(!state.field().is_blocked(48, 48, None));
    assert_eq!(
        BattleState::from_json(&state.to_json().unwrap()).unwrap(),
        state
    );
}

fn deploy_both(state: &mut BattleState) -> Vec<UnitId> {
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    state
        .deploy(
            ids[0],
            UnitPosition {
                at: (48, 48),
                size: 1,
            },
        )
        .unwrap();
    state
        .deploy(
            ids[1],
            UnitPosition {
                at: (52, 52),
                size: 2,
            },
        )
        .unwrap();
    ids
}

#[test]
fn saved_active_turn_continues_with_identical_dice_queue_and_events() {
    let mut original = battle();
    let ids = deploy_both(&mut original);
    original.begin_round(&[ids[1], ids[0]]).unwrap();
    assert_eq!(original.begin_next_turn().unwrap(), Some(ids[1]));
    original.roll(ids[0].0, ids[1].0, 0);
    original.roll(ids[1].0, ids[0].0, 28);
    original.roll(50, 51, 35); // scenery uses modulus 200 and the same cache
    let json = original.to_json().unwrap();
    let mut restored = BattleState::from_json(&json).unwrap();
    assert_eq!(restored, original);
    assert_eq!(restored.to_json().unwrap(), json);
    for state in [&mut original, &mut restored] {
        state.roll(ids[0].0, ids[1].0, 0);
        state.roll(ids[1].0, ids[0].0, 28);
        assert_eq!(state.finish_turn().unwrap(), ids[1]);
        assert_eq!(state.begin_next_turn().unwrap(), Some(ids[0]));
        state.roll(ids[0].0, ids[1].0, 0);
        state.finish_turn().unwrap();
        assert_eq!(state.begin_next_turn().unwrap(), None);
        state.begin_round(&ids).unwrap();
    }
    assert_eq!(restored, original);
    assert_eq!(restored.to_json().unwrap(), original.to_json().unwrap());
    assert_eq!(restored.turns().round, 2);
}

#[test]
fn queue_rejects_missing_duplicate_unknown_or_unplaced_units_without_events() {
    let mut state = battle();
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    assert!(state.begin_round(&ids).is_err());
    deploy_both(&mut state);
    for order in [
        vec![],
        vec![ids[0]],
        vec![ids[0], ids[0]],
        vec![ids[0], UnitId(999)],
    ] {
        let before = state.clone();
        assert!(state.begin_round(&order).is_err());
        assert_eq!(state, before);
    }
    state.begin_round(&ids).unwrap();
    assert!(state.begin_round(&ids).is_err());
    assert!(state.finish_turn().is_err());
    state.begin_next_turn().unwrap();
    let before = state.clone();
    assert!(state.begin_next_turn().is_err());
    assert!(state
        .deploy(
            ids[0],
            UnitPosition {
                at: (50, 50),
                size: 1
            }
        )
        .is_err());
    assert_eq!(state, before);
}

#[test]
fn snapshots_reject_corrupt_cross_references_geometry_hp_and_rng() {
    let mut state = battle();
    let ids = deploy_both(&mut state);
    state.begin_round(&ids).unwrap();
    let json = state.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edits = [
        ("/version", serde_json::json!(999)),
        ("/armies/1/source", serde_json::json!(10)),
        ("/armies/0/slots/0", serde_json::json!(999)),
        ("/units/0/id", serde_json::json!(0)),
        ("/units/0/slot", serde_json::json!(7)),
        ("/units/0/kind/Creature/top_hp", serde_json::json!(11)),
        ("/units/0/kind/Creature/count", serde_json::json!(-1)),
        ("/units/0/position/at", serde_json::json!([1, 1])),
        ("/field/rows/0/start", serde_json::json!(usize::MAX)),
        ("/turns/pending/0", serde_json::json!(999)),
        ("/random/id_modulus", serde_json::json!(0)),
        ("/random/checkpoints", serde_json::json!([0])),
    ];
    for (path, replacement) in edits {
        let mut broken = value.clone();
        *broken.pointer_mut(path).unwrap() = replacement;
        assert!(
            BattleState::from_json(&broken.to_string()).is_err(),
            "accepted invalid {path}"
        );
    }
    assert!(BattleState::from_json("{").is_err());
}

#[test]
fn invalid_map_inputs_fail_instead_of_disappearing_from_battle() {
    let attacker = army(10);
    assert!(BattleState::from_armies(&attacker, &attacker, &stats(), field(), 1).is_err());
    assert!(BattleState::from_armies(&attacker, &army(20), &[], field(), 1).is_err());
    let mut invalid = attacker.clone();
    invalid.army.slots[0] = Some(Slot::Creatures(Stack {
        creature: 0,
        count: 0,
    }));
    assert!(BattleState::from_armies(&invalid, &army(20), &stats(), field(), 1).is_err());
    let mut table = stats();
    table[0].hit_points = 0;
    assert!(BattleState::from_armies(&attacker, &army(20), &table, field(), 1).is_err());
}

#[test]
fn wounded_fixture_keeps_top_hp_shots_and_losses_instead_of_reinitializing() {
    let state = battle();
    let mut value: serde_json::Value = serde_json::from_str(&state.to_json().unwrap()).unwrap();
    value["units"][0]["kind"]["Creature"]["count"] = serde_json::json!(7);
    value["units"][0]["kind"]["Creature"]["top_hp"] = serde_json::json!(3);
    value["units"][0]["kind"]["Creature"]["shots"] = serde_json::json!(2);
    let restored = BattleState::from_json(&value.to_string()).unwrap();
    assert!(matches!(
        restored.units()[0].kind,
        UnitKind::Creature {
            initial_count: 12,
            count: 7,
            top_hp: 3,
            shots: 2,
            ..
        }
    ));
    assert_eq!(
        BattleState::from_json(&restored.to_json().unwrap()).unwrap(),
        restored
    );
}
