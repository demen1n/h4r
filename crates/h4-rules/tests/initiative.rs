use h4_rules::battle::deployment::Formation;
use h4_rules::battle::initiative::{
    defending_defense, morale_result, InitiativeStats, Morale, RoundInitiative, UnitInitiative,
};
use h4_rules::battle::state::{UnitId, UnitKind};
use h4_rules::battle::{BattleState, Side};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};
use h4_rules::creatures::CreatureStats;
use h4_rules::random::CombatRandom;
use h4_state::army::{Army, ArmyId, MapArmy, Slot, Stack, SLOTS};
use h4_state::Pos;

fn side(value: i32) -> Side {
    if value == 0 {
        Side::Attacker
    } else {
        Side::Defender
    }
}
fn entry(id: usize, values: &[i32]) -> UnitInitiative {
    UnitInitiative {
        unit: UnitId(id as u32 + 1),
        side: side(values[0]),
        stats: InitiativeStats {
            speed: values[1],
            morale: values[2],
        },
        morale_roll: values[3] as u8,
        waiting: values[4] != 0,
        defending: false,
    }
}
fn float_hex(s: &str) -> f32 {
    let bytes: Vec<u8> = (0..8)
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect();
    f32::from_le_bytes(bytes.try_into().unwrap())
}

#[test]
fn original_exe_initiative_vectors() {
    let mut counts = [0; 7];
    for line in include_str!("initiative_vectors.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let cols: Vec<_> = line.split_whitespace().collect();
        match cols[0] {
            "M" => {
                let result =
                    morale_result(cols[1].parse().unwrap(), cols[2].parse().unwrap()).unwrap();
                let code = match result {
                    Morale::Bad => -1,
                    Morale::Normal => 0,
                    Morale::Good => 1,
                };
                assert_eq!(code, cols[3].parse::<i32>().unwrap(), "{line}");
                counts[0] += 1;
            }
            "P" => {
                let values: Vec<i32> = cols[2..12].iter().map(|s| s.parse().unwrap()).collect();
                let a = entry(0, &values[..5]);
                let b = entry(1, &values[5..]);
                assert_eq!(
                    a.moves_before(&b, side(cols[1].parse().unwrap())),
                    cols[12] == "1",
                    "{line}"
                );
                assert_eq!(a.priority(), cols[13].parse::<i32>().unwrap(), "{line}");
                assert_eq!(b.priority(), cols[14].parse::<i32>().unwrap(), "{line}");
                counts[1] += 1;
            }
            "S" => {
                let values: Vec<Vec<i32>> = cols[2]
                    .split(';')
                    .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect())
                    .collect();
                let round = RoundInitiative {
                    last_side: side(cols[1].parse().unwrap()),
                    entries: values
                        .iter()
                        .enumerate()
                        .map(|(i, e)| entry(i, e))
                        .collect(),
                };
                let selected = round.next(|id| {
                    let v = &values[id.0 as usize - 1];
                    v[5] != 0 && v[6] == 0
                });
                assert_eq!(
                    selected.map_or(-1, |id| id.0 as i32 - 1),
                    cols[3].parse::<i32>().unwrap(),
                    "{line}"
                );
                counts[2] += 1;
            }
            "R" => {
                let seed = cols[1].parse().unwrap();
                let modulus = cols[2].parse().unwrap();
                let count = cols[3].parse::<u32>().unwrap();
                let rounds = cols[4].parse::<usize>().unwrap();
                let mut random = CombatRandom::new(seed, modulus);
                let actual: Vec<_> = (0..rounds)
                    .flat_map(|_| {
                        (1..=count)
                            .map(|id| random.roll(0, id, 0x1c) % 10)
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let expected: Vec<u32> = cols[5..].iter().map(|s| s.parse().unwrap()).collect();
                assert_eq!(actual, expected, "{line}");
                counts[3] += 1;
            }
            "D" => {
                let base = float_hex(cols[1]);
                let actual = if cols[2] == "1" {
                    defending_defense(base, cols[3] == "1")
                } else {
                    base
                };
                assert_eq!(actual.to_bits(), float_hex(cols[4]).to_bits(), "{line}");
                counts[4] += 1;
            }
            "T" => {
                let seed = cols[1].parse().unwrap();
                let entries: Vec<Vec<_>> = cols[2]
                    .split(';')
                    .map(|entry| entry.split(',').collect())
                    .collect();
                let mut battle = trace_battle(seed, entries.len());
                let stats: Vec<_> = battle
                    .units()
                    .iter()
                    .zip(&entries)
                    .map(|(unit, entry)| {
                        (
                            unit.id,
                            InitiativeStats {
                                speed: entry[1].parse().unwrap(),
                                morale: entry[2].parse().unwrap(),
                            },
                        )
                    })
                    .collect();
                let ids: Vec<_> = battle.units().iter().map(|unit| unit.id).collect();
                let mut round = 0;
                for (step, expected) in cols[3].split(';').enumerate() {
                    let fields: Vec<_> = expected.split(',').collect();
                    let next_round: u32 = fields[0].parse().unwrap();
                    if next_round != round {
                        if round != 0 {
                            assert_eq!(battle.begin_next_turn().unwrap(), None);
                        }
                        battle.begin_initiative_round(&stats).unwrap();
                        round = next_round;
                    }
                    let selected: usize = fields[1].parse().unwrap();
                    assert_eq!(
                        battle.begin_next_turn().unwrap(),
                        Some(ids[selected]),
                        "{line}, step {step}"
                    );
                    let priority =
                        battle.turns().initiative.as_ref().unwrap().entries[selected].priority();
                    assert_eq!(
                        priority,
                        fields[3].parse::<i32>().unwrap(),
                        "{line}, step {step}"
                    );
                    match fields[2] {
                        "w" => {
                            battle.wait().unwrap();
                        }
                        "d" => {
                            battle.defend().unwrap();
                        }
                        "f" => {
                            battle.finish_turn().unwrap();
                        }
                        _ => panic!("unexpected native action"),
                    }
                    let defense: u32 = ids
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| battle.is_defending(**id))
                        .map(|(i, _)| 1 << i)
                        .sum();
                    assert_eq!(
                        defense,
                        fields[4].parse::<u32>().unwrap(),
                        "{line}, step {step}"
                    );
                    if step % 3 == 0 {
                        battle = roundtrip(&battle);
                    }
                }
                assert_eq!(battle.begin_next_turn().unwrap(), None);
                counts[6] += 1;
            }
            "A" => {
                assert_eq!(&cols[1..], &["1", "1", "5"]);
                counts[5] += 1;
            }
            _ => panic!("unknown vector {line}"),
        }
    }
    assert_eq!(counts, [210, 1200, 160, 12, 24, 1, 12]);
}

fn battle() -> BattleState {
    let stats = vec![CreatureStats {
        keyword: "fixture".into(),
        name: "Fixture".into(),
        level: 1,
        hit_points: 10,
        damage_low: 1,
        damage_high: 3,
        attack: 4,
        defense: 8,
        speed: 5,
        movement: 20,
        shots: 8,
        weekly_growth: 10,
        alignment: 0,
        experience: 7,
    }];
    let source = |id, two| {
        let mut army = Army::of(&[Stack {
            creature: 0,
            count: 12,
        }]);
        if two {
            army.slots[2] = Some(Slot::Creatures(Stack {
                creature: 0,
                count: 7,
            }));
        }
        MapArmy {
            id: ArmyId(id),
            owner: Some(0),
            at: Pos {
                level: 0,
                row: 0,
                col: 0,
            },
            army,
            movement_left: [123.; SLOTS],
            growth_remainder: [17; SLOTS],
        }
    };
    let mut battle = BattleState::from_armies(
        &source(1, true),
        &source(2, false),
        &stats,
        BattlefieldGrid::standard(CellSurface::default()),
        1234,
    )
    .unwrap();
    let sizes: Vec<_> = battle.units().iter().map(|u| (u.id, 4)).collect();
    battle
        .deploy_initial([Formation::Loose; 2], &sizes)
        .unwrap();
    battle
}
fn trace_battle(seed: u32, count: usize) -> BattleState {
    let fixture = battle();
    let UnitKind::Creature { stats, .. } = &fixture.units()[0].kind else {
        unreachable!()
    };
    let source = |id, count| MapArmy {
        id: ArmyId(id),
        owner: Some(0),
        at: Pos {
            level: 0,
            row: 0,
            col: 0,
        },
        army: Army::of(&vec![
            Stack {
                creature: 0,
                count: 12
            };
            count
        ]),
        movement_left: [123.; SLOTS],
        growth_remainder: [17; SLOTS],
    };
    let mut battle = BattleState::from_armies(
        &source(1, count / 2),
        &source(2, count - count / 2),
        std::slice::from_ref(stats),
        BattlefieldGrid::standard(CellSurface::default()),
        seed,
    )
    .unwrap();
    let sizes: Vec<_> = battle.units().iter().map(|unit| (unit.id, 4)).collect();
    battle
        .deploy_initial([Formation::Loose; 2], &sizes)
        .unwrap();
    battle
}

fn inputs(battle: &BattleState) -> Vec<(UnitId, InitiativeStats)> {
    battle
        .units()
        .iter()
        .enumerate()
        .map(|(i, u)| {
            (
                u.id,
                InitiativeStats {
                    speed: if i == 0 { 10 } else { 5 },
                    morale: 0,
                },
            )
        })
        .collect()
}
fn roundtrip(battle: &BattleState) -> BattleState {
    let restored = BattleState::from_json(&battle.to_json().unwrap()).unwrap();
    assert_eq!(&restored, battle);
    restored
}

#[test]
fn wait_ties_defense_and_round_boundaries_continue_after_reload() {
    let mut battle = battle();
    let stats = inputs(&battle);
    let ids: Vec<_> = battle.units().iter().map(|u| u.id).collect();
    battle.begin_initiative_round(&stats).unwrap();
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[0]));
    battle.wait().unwrap();
    assert!(battle.turns().completed.is_empty());
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[2]));
    battle.defend().unwrap();
    assert!(battle.is_defending(ids[2]));
    assert_eq!(battle.apply_defense_stance(ids[2], 8., false).unwrap(), 10.);
    assert_eq!(battle.apply_defense_stance(ids[2], 8., true).unwrap(), 15.);
    let mut restored = roundtrip(&battle);
    for state in [&mut battle, &mut restored] {
        assert_eq!(state.begin_next_turn().unwrap(), Some(ids[1]));
        state.finish_turn().unwrap();
        assert_eq!(state.begin_next_turn().unwrap(), Some(ids[0]));
        let before = state.clone();
        assert!(state.wait().is_err());
        assert_eq!(*state, before);
        state.defend().unwrap();
        assert_eq!(state.begin_next_turn().unwrap(), None);
        state.begin_initiative_round(&stats).unwrap();
        // Defense is not cleared by the round reset.
        assert!(state.is_defending(ids[0]) && state.is_defending(ids[2]));
        assert_eq!(state.begin_next_turn().unwrap(), Some(ids[0]));
        assert!(!state.is_defending(ids[0]));
        assert!(state.is_defending(ids[2]));
        state.finish_turn().unwrap();
        assert_eq!(state.begin_next_turn().unwrap(), Some(ids[2]));
        assert!(!state.is_defending(ids[2]));
        roundtrip(state);
    }
    assert_eq!(battle, restored);
}

#[test]
fn waiting_is_slow_first_and_speed_refresh_is_dynamic() {
    let mut battle = battle();
    let stats = inputs(&battle);
    let ids: Vec<_> = battle.units().iter().map(|u| u.id).collect();
    battle.begin_initiative_round(&stats).unwrap();
    for _ in 0..3 {
        battle.begin_next_turn().unwrap();
        battle.wait().unwrap();
    }
    // Both slower stacks precede the fast one, with the opposite-side tie.
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[2]));
    battle.finish_turn().unwrap();
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[1]));
    battle.finish_turn().unwrap();
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[0]));
    battle.finish_turn().unwrap();
    battle.begin_initiative_round(&stats).unwrap();
    battle.set_turn_speed(ids[1], 20).unwrap();
    assert_eq!(battle.begin_next_turn().unwrap(), Some(ids[1]));
    roundtrip(&battle);
}

#[test]
fn invalid_actions_and_inputs_are_atomic_and_dead_stacks_are_skipped() {
    let mut battle = battle();
    let stats = inputs(&battle);
    let before = battle.clone();
    assert!(battle.wait().is_err());
    assert!(battle.defend().is_err());
    assert!(battle.begin_initiative_round(&stats[..2]).is_err());
    let mut bad = stats.clone();
    bad[0].1.morale = 11;
    assert!(battle.begin_initiative_round(&bad).is_err());
    bad = stats.clone();
    bad[1].0 = bad[0].0;
    assert!(battle.begin_initiative_round(&bad).is_err());
    assert_eq!(battle, before);
    battle.begin_initiative_round(&stats).unwrap();
    let before = battle.clone();
    assert!(battle.begin_initiative_round(&stats).is_err());
    assert_eq!(battle, before);
    // Damage commands arrive in #68. Simulate one casualty through a valid fixture.
    let mut value: serde_json::Value = serde_json::from_str(&battle.to_json().unwrap()).unwrap();
    value["units"][0]["kind"]["Creature"]["count"] = serde_json::json!(0);
    value["units"][0]["kind"]["Creature"]["top_hp"] = serde_json::json!(0);
    let mut dead = BattleState::from_json(&value.to_string()).unwrap();
    let next = dead.begin_next_turn().unwrap().unwrap();
    assert_ne!(next, dead.units()[0].id);
    assert!(matches!(
        dead.units()[0].kind,
        UnitKind::Creature { count: 0, .. }
    ));
    roundtrip(&dead);
}

#[test]
fn snapshots_reject_forged_native_metadata() {
    let mut battle = battle();
    let stats = inputs(&battle);
    battle.begin_initiative_round(&stats).unwrap();
    battle.begin_next_turn().unwrap();
    battle.wait().unwrap();
    let value: serde_json::Value = serde_json::from_str(&battle.to_json().unwrap()).unwrap();
    for (path, replacement) in [
        ("/turns/initiative/last_side", serde_json::json!("Defender")),
        ("/turns/initiative/entries/0/unit", serde_json::json!(999)),
        (
            "/turns/initiative/entries/0/morale_roll",
            serde_json::json!(10),
        ),
        (
            "/turns/initiative/entries/0/waiting",
            serde_json::json!(false),
        ),
        (
            "/turns/initiative/entries/0/defending",
            serde_json::json!(true),
        ),
        (
            "/turns/initiative/entries/0/stats/speed",
            serde_json::json!(99),
        ),
        ("/version", serde_json::json!(1)),
    ] {
        let mut broken = value.clone();
        *broken.pointer_mut(path).unwrap() = replacement;
        assert!(
            BattleState::from_json(&broken.to_string()).is_err(),
            "{path}"
        );
    }
}
