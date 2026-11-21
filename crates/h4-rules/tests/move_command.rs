//! The move command on a battle: the path finder's inputs built from the state, the
//! stopping point when the movement runs out, and the state after the move.
//! Uses fixture stats, no game data.

use h4_rules::battle::melee_command::MeleeContext;
use h4_rules::battle::path::DIRECTIONS;
use h4_rules::battle::move_command::{
    approach_and_attack, attack_positions_against, destination, move_unit, search_for, AttackOrder, MoveRules,
};
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
    // It walked along (0, 1), direction 2, and now faces that way (it started at 0).
    let events = state.events();
    assert_eq!(
        &events[events.len() - 2..],
        &[
            CombatEvent::Moved { unit: a, from: (50, 40), to: (50, 45), cost: 500 },
            CombatEvent::Turned { unit: a, facing: 2 },
        ]
    );
    assert_eq!(state.unit(a).unwrap().facing, 2);
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

fn hit_points(state: &BattleState, id: UnitId) -> i32 {
    state.unit(id).unwrap().total_hits().unwrap()
}

#[test]
fn an_attack_from_where_the_unit_stands_moves_nothing() {
    let (mut state, a, t) = battle(open_field(), 1, (50, 42), (50, 45));
    let order = approach_and_attack(&mut state, a, t, (50, 42), &MoveRules::plain(1000), no_zones, &MeleeContext::default()).unwrap();
    let AttackOrder::Attacked { walked: None, melee } = order else { panic!("{order:?}") };
    assert!(!melee.strikes.is_empty());
    assert!(hit_points(&state, t) < 100);
    assert!(!state.events().iter().any(|e| matches!(e, CombatEvent::Moved { .. })));
}

#[test]
fn an_attack_position_within_reach_is_walked_to_and_the_target_struck() {
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 45));
    let order = approach_and_attack(&mut state, a, t, (50, 42), &MoveRules::plain(1000), no_zones, &MeleeContext::default()).unwrap();
    let AttackOrder::Attacked { walked: Some(walk), melee } = order else { panic!("{order:?}") };
    assert_eq!((walk.to, walk.cost), ((50, 42), 200));
    assert!(!melee.strikes.is_empty());
    assert!(hit_points(&state, t) < 100);
    assert_eq!(state.unit(a).unwrap().position.unwrap().at, (50, 42));
}

#[test]
fn an_attack_position_beyond_the_movement_is_only_approached() {
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 50));
    let order = approach_and_attack(&mut state, a, t, (50, 47), &MoveRules::plain(500), no_zones, &MeleeContext::default()).unwrap();
    let AttackOrder::Approached(walk) = order else { panic!("{order:?}") };
    assert_eq!((walk.to, walk.cost), ((50, 45), 500));
    assert_eq!(hit_points(&state, t), 100);
}

#[test]
fn an_unreached_attack_position_is_an_error() {
    let mut field = open_field();
    // Wall the target in: the cell next to it can't be reached.
    for (x, y) in [(49, 49), (49, 50), (49, 51), (50, 49), (50, 51), (51, 49), (51, 50), (51, 51)] {
        field.surface_mut(x, y).unwrap().flags |= FLAG_BLOCKED;
    }
    let (mut state, a, t) = battle(field, 1, (50, 40), (60, 60));
    let before = state.clone();
    assert!(approach_and_attack(&mut state, a, t, (50, 50), &MoveRules::plain(3000), no_zones, &MeleeContext::default()).is_err());
    assert_eq!(state, before);
}


#[test]
fn the_attack_positions_are_the_exes_list() {
    // Footprints of 1 with the reach of 2 (no long_weapon): the positions ring the target
    // two to three cells away, one per angle; a cell right next to it shares its angle
    // with a farther one and is dropped (the exe keeps the farther).
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 45));
    let list = attack_positions_against(&state, a, t, &MoveRules::plain(1000), no_zones).unwrap();
    assert_eq!(list.len(), 16);
    assert!(list.windows(2).all(|w| w[0].angle < w[1].angle));
    let ahead = list.iter().find(|p| p.angle == 90).unwrap();
    assert_eq!((ahead.at, ahead.distance, ahead.cost), ((50, 42), 48, 200));
    assert!(!list.iter().any(|p| p.at == (50, 44)));
    let before = state.clone();
    let refused = approach_and_attack(&mut state, a, t, (50, 44), &MoveRules::plain(1000), no_zones, &MeleeContext::default());
    assert!(refused.is_err());
    assert_eq!(state, before);
}

#[test]
fn an_enemy_facing_the_mover_holds_it_in_its_zone() {
    use h4_rules::battle::zone_of_control::{zones_for, ZoneTraits};
    // The enemy at (50, 46) faces direction 6, (0, -1): toward the mover's column.
    let (mut state, a, t) = battle(open_field(), 1, (50, 40), (50, 46));
    state.turn(t, 6).unwrap();
    let zone = zones_for(&state, a, |_| ZoneTraits::default()).unwrap();
    // Two cells in front of it: every step costs extra except along the line to the enemy.
    assert_eq!(zone((50, 44)), [true, true, false, true, true, true, false, true]);
    // Far away, or behind it: no zone.
    assert_eq!(zone((50, 40)), [false; 8]);
    assert_eq!(zone((50, 49)), [false; 8]);

    // Walking straight toward the enemy pays nothing extra.
    let report = move_unit(&mut state, a, (50, 44), &MoveRules::plain(1000), &zone).unwrap();
    assert_eq!(report.cost, 400);
}

#[test]
fn a_step_sideways_out_of_a_zone_costs_a_quarter_of_the_movement_more() {
    use h4_rules::battle::zone_of_control::{zones_for, ZoneTraits};
    let (mut state, a, t) = battle(open_field(), 1, (50, 44), (50, 46));
    state.turn(t, 6).unwrap();
    let zone = zones_for(&state, a, |_| ZoneTraits::default()).unwrap();
    let free = search_for(&state, a, &MoveRules::plain(1000), no_zones).unwrap();
    let held = search_for(&state, a, &MoveRules::plain(1000), &zone).unwrap();
    assert_eq!(free.map.cell((51, 44)).unwrap().cost, 100);
    // Straight sideways would be 100 + 250; the cheaper way steps back along the line to
    // the enemy first (free), then diagonally from outside the zone: 100 + 150.
    assert_eq!(held.map.cell((51, 44)).unwrap().cost, 250);
    // Back along the line: no penalty.
    assert_eq!(held.map.cell((50, 43)).unwrap().cost, 100);
}

#[test]
fn the_sides_start_facing_each_other() {
    let (state, a, t) = battle(open_field(), 1, (80, 50), (20, 50));
    // The attackers stand in the far rows and face (-1, 0); the defenders face (1, 0).
    assert_eq!(state.unit(a).unwrap().facing, 0);
    assert_eq!(state.unit(t).unwrap().facing, 4);
}

#[test]
fn a_walker_faces_its_last_step_and_a_flier_its_whole_way() {
    // Walking around a wall of blocked cells: the last step decides.
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    let report = move_unit(&mut state, a, (52, 42), &MoveRules::plain(1000), no_zones).unwrap();
    let last = report.path[report.path.len() - 2];
    let step = (report.to.0 - last.0, report.to.1 - last.1);
    let expected = DIRECTIONS.iter().position(|d| *d == step).unwrap() as u8;
    assert_eq!(state.unit(a).unwrap().facing, expected);

    // A flier faces straight from where it stood to where it landed: (3, 1) is closest
    // to direction 4, (1, 0).
    let (mut state, a, _) = battle(open_field(), 1, (50, 40), (50, 70));
    let flier = MoveRules { flies: true, ..MoveRules::plain(1000) };
    move_unit(&mut state, a, (53, 41), &flier, no_zones).unwrap();
    assert_eq!(state.unit(a).unwrap().facing, 4);
}

#[test]
fn strikers_turn_toward_their_victims() {
    // The target stands to the side of the attacker: (0, 1), direction 2, from it.
    let (mut state, a, t) = battle(open_field(), 1, (50, 42), (50, 45));
    approach_and_attack(&mut state, a, t, (50, 42), &MoveRules::plain(1000), no_zones, &MeleeContext::default()).unwrap();
    assert_eq!(state.unit(a).unwrap().facing, 2);
    // It answered, turning back toward the attacker: (0, -1), direction 6.
    assert_eq!(state.unit(t).unwrap().facing, 6);
    assert!(state.events().contains(&CombatEvent::Turned { unit: t, facing: 6 }));
}
