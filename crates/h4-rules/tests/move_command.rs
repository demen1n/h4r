//! The move command on a battle: the path finder's inputs built from the state, the
//! stopping point when the movement runs out, and the state after the move.
//! Uses fixture stats, no game data.

use h4_rules::battle::move_command::{destination, move_unit, search_for, MoveRules};
use h4_rules::battle::state::{BattleState, CombatEvent, UnitId, UnitPosition};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface, FLAG_BLOCKED};
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

fn army(id: u32) -> MapArmy {
    MapArmy {
        id: ArmyId(id),
        owner: Some(0),
        at: Pos { level: 0, row: 3, col: 4 },
        army: Army::of(&[Stack { creature: 0, count: 10 }]),
        movement_left: [0.; SLOTS],
        growth_remainder: [0; SLOTS],
    }
}

/// Two stacks of footprint `size` at the given corners; the first one's turn.
fn battle(field: BattlefieldGrid, size: u8, mover: (i32, i32), enemy: (i32, i32)) -> (BattleState, UnitId, UnitId) {
    let mut state = BattleState::from_armies(&army(10), &army(20), &stats(), field, 4321).unwrap();
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    state.deploy(ids[0], UnitPosition { at: mover, size }).unwrap();
    state.deploy(ids[1], UnitPosition { at: enemy, size }).unwrap();
    state.begin_round(&[ids[0], ids[1]]).unwrap();
    assert_eq!(state.begin_next_turn().unwrap(), Some(ids[0]));
    (state, ids[0], ids[1])
}

fn open_field() -> BattlefieldGrid {
    BattlefieldGrid::standard(CellSurface::default())
}

fn no_zones(_: (i32, i32)) -> [bool; 8] {
    [false; 8]
}

#[test]
fn a_reachable_cell_is_walked_to_in_straight_steps() {
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    let report = move_unit(&mut state, a, (50, 45), &MoveRules::plain(1000), no_zones).unwrap();
    assert_eq!(report.from, (50, 40));
    assert_eq!(report.to, (50, 45));
    assert_eq!(report.path, vec![(50, 41), (50, 42), (50, 43), (50, 44), (50, 45)]);
    assert_eq!(report.cost, 500);
    assert_eq!(state.unit(a).unwrap().position.unwrap().at, (50, 45));
    assert_eq!(
        state.events().last(),
        Some(&CombatEvent::Moved { unit: a, from: (50, 40), to: (50, 45), cost: 500 })
    );
    // The new place is in the grid and survives a snapshot.
    assert!(state.field().is_blocked(50, 45, None));
    assert!(!state.field().is_blocked(50, 40, None));
    let restored = BattleState::from_json(&state.to_json().unwrap()).unwrap();
    assert_eq!(restored, state);
    // Its turn goes on: a move doesn't finish it.
    assert_eq!(state.turns().active, Some(a));
}

#[test]
fn a_diagonal_step_costs_half_as_much_again() {
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    // (1, 1) is a diagonal of the grid: 150 per step.
    let report = move_unit(&mut state, a, (52, 42), &MoveRules::plain(1000), no_zones).unwrap();
    assert_eq!(report.cost, 300);
    assert_eq!(report.path.len(), 2);
}

#[test]
fn a_far_target_is_approached_as_far_as_the_movement_goes() {
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    let report = move_unit(&mut state, a, (50, 60), &MoveRules::plain(700), no_zones).unwrap();
    assert_eq!(report.to, (50, 47));
    assert_eq!(report.cost, 700);
    assert_eq!(report.path.len(), 7);
}

#[test]
fn an_enemy_corner_is_no_move_target() {
    // The occupied-cell phase prices the enemy's cells (+500, for an attack) but doesn't
    // mark them reached, so the path rebuild finds no path to them (`20e7b0`).
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 45));
    let rules = MoveRules::plain(1000);
    let search = search_for(&state, a, &rules, no_zones).unwrap();
    let enemy = search.map.cell((50, 45)).unwrap();
    assert_eq!(enemy.cost, 900);
    assert_eq!(destination(&search, (50, 45), rules.movement), None);
    assert!(move_unit(&mut state, a, (50, 45), &rules, no_zones).is_err());
    assert_eq!(state.unit(t).unwrap().position.unwrap().at, (50, 45));
}

#[test]
fn a_unit_walks_around_an_enemy_in_the_way() {
    // Footprints of 3: the enemy stands right between the start and the target.
    let (mut state, a, t) = battle(open_field(), 3, (50, 40), (50, 46));
    let report = move_unit(&mut state, a, (50, 52), &MoveRules::plain(2000), no_zones).unwrap();
    assert_eq!(report.to, (50, 52));
    // Straight on would be 12 steps (1200): the way around is longer.
    assert!(report.cost > 1200, "{report:?}");
    // No footprint along the way overlaps the enemy's.
    let enemy = state.unit(t).unwrap().position.unwrap().at;
    for &(x, y) in &report.path {
        assert!((x - enemy.0).abs() >= 3 || (y - enemy.1).abs() >= 3, "{:?} overlaps the enemy", (x, y));
    }
}

#[test]
fn blocked_cells_of_the_field_are_walked_around() {
    let mut field = open_field();
    // A wall of blocked cells across the straight way, rows 45..=55 of column 43.
    for x in 45..=55 {
        field.surface_mut(x, 43).unwrap().flags |= FLAG_BLOCKED;
    }
    let (mut state, a, _) = battle(field, 1, (50, 40), (50, 70));
    let report = move_unit(&mut state, a, (50, 46), &MoveRules::plain(3000), no_zones).unwrap();
    assert_eq!(report.to, (50, 46));
    assert!(report.path.iter().all(|&(x, y)| y != 43 || !(45..=55).contains(&x)), "{report:?}");
    // Straight on would be 600.
    assert!(report.cost > 600);
}

#[test]
fn only_the_active_unit_moves_and_a_failed_move_changes_nothing() {
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 70));
    let before = state.clone();
    assert!(move_unit(&mut state, t, (50, 65), &MoveRules::plain(1000), no_zones).is_err());
    // The start itself is no move.
    assert!(move_unit(&mut state, a, (50, 40), &MoveRules::plain(1000), no_zones).is_err());
    // Held (effect 9): nothing is reached.
    let held = MoveRules { held: true, ..MoveRules::plain(1000) };
    assert!(move_unit(&mut state, a, (50, 45), &held, no_zones).is_err());
    assert_eq!(state, before);
}

#[test]
fn a_zone_of_control_makes_steps_dearer() {
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    // Every step pays a quarter of the movement extra: 100 + 1000 / 4 = 350 a step.
    let report = move_unit(&mut state, a, (50, 42), &MoveRules::plain(1000), |_| [true; 8]).unwrap();
    assert_eq!(report.cost, 700);
}
