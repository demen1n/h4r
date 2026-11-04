use h4_rules::battle::deployment::{starting_position, Formation};
use h4_rules::battle::state::UnitPosition;
use h4_rules::battle::{BattleState, Side};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface, FLAG_BLOCKED};
use h4_rules::creatures::CreatureStats;
use h4_state::army::{Army, ArmyId, Hero, MapArmy, Slot, Stack, SLOTS};
use h4_state::Pos;

fn army(id: u32, sizes: &[u8]) -> MapArmy {
    let mut army = Army::default();
    for (slot, &size) in sizes.iter().enumerate() {
        if size != 0 {
            army.slots[slot] = Some(Slot::Hero(Hero {
                portrait: -1,
                name: format!("Hero {slot}"),
                class: 0,
                male: true,
                level: 1,
                experience: 0,
            }));
        }
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
}

fn point(s: &str) -> (i32, i32) {
    let (x, y) = s.split_once(',').unwrap();
    (x.parse().unwrap(), y.parse().unwrap())
}

#[test]
fn native_deployment_and_compaction_vectors() {
    let mut cases = 0;
    for line in include_str!("deployment_vectors.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let cols: Vec<_> = line.split_whitespace().collect();
        let formation = [Formation::Loose, Formation::Tight, Formation::Square]
            [cols[0].parse::<usize>().unwrap()];
        let side = cols[1].parse::<usize>().unwrap();
        let sizes: Vec<u8> = cols[2].split(',').map(|s| s.parse().unwrap()).collect();
        let sources = if side == 0 {
            [army(1, &sizes), army(2, &[4])]
        } else {
            [army(1, &[4]), army(2, &sizes)]
        };
        let mut field = BattlefieldGrid::standard(CellSurface::default());
        if cols[3] != "-" {
            let (x, y) = point(cols[3]);
            field.surface_mut(x, y).unwrap().flags |= FLAG_BLOCKED;
        }
        let mut battle =
            BattleState::from_armies(&sources[0], &sources[1], &[], field, 42).unwrap();
        let diameters: Vec<_> = battle
            .units()
            .iter()
            .map(|u| {
                (
                    u.id,
                    if usize::from(u.side == Side::Defender) == side {
                        sizes[u.slot]
                    } else {
                        4
                    },
                )
            })
            .collect();
        let result = battle.deploy_initial([formation; 2], &diameters);
        // Some obstacle fixtures deliberately intersect the initial footprint.
        // The original installs it regardless; our public API rejects it atomically.
        if result.is_err() {
            assert!(cols[3] != "-", "unexpected failure: {line}: {result:?}");
            assert!(battle.units().iter().all(|u| u.position.is_none()));
            continue;
        }
        for slot in 0..7 {
            let id = battle.armies()[side].slots[slot];
            let expected = cols[4 + slot];
            if expected == "-" {
                assert!(id.is_none());
            } else {
                assert_eq!(
                    battle.unit(id.unwrap()).unwrap().position.unwrap().at,
                    point(expected),
                    "{line}, slot {slot}"
                );
            }
        }
        let json = battle.to_json().unwrap();
        assert_eq!(BattleState::from_json(&json).unwrap(), battle);
        cases += 1;
    }
    assert!(cases >= 130, "only {cases} fixtures were accepted");
}

#[test]
fn deployment_is_atomic_and_preserves_game_inputs() {
    let mut sources = [army(1, &[4, 0, 7]), army(2, &[3])];
    sources[0].army.slots[0] = Some(Slot::Creatures(Stack {
        creature: 0,
        count: 12,
    }));
    let stats = [CreatureStats {
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
    }];
    let originals = sources.clone();
    let mut battle = BattleState::from_armies(
        &sources[0],
        &sources[1],
        &stats,
        BattlefieldGrid::standard(CellSurface::default()),
        42,
    )
    .unwrap();
    let ids: Vec<_> = battle.units().iter().map(|u| u.id).collect();
    let sizes = [(ids[0], 4), (ids[1], 7), (ids[2], 3)];
    let before = battle.clone();
    battle
        .deploy_initial([Formation::Tight; 2], &sizes)
        .unwrap();
    for (old, new) in before.units().iter().zip(battle.units()) {
        assert_eq!(old.kind, new.kind);
    }
    let old_json: serde_json::Value = serde_json::from_str(&before.to_json().unwrap()).unwrap();
    let new_json: serde_json::Value = serde_json::from_str(&battle.to_json().unwrap()).unwrap();
    assert_eq!(old_json["random"], new_json["random"]);
    let previous = battle.clone();
    for bad in [
        vec![],
        vec![(ids[0], 4); 3],
        vec![
            (h4_rules::battle::state::UnitId(u32::MAX), 4),
            (ids[1], 7),
            (ids[2], 3),
        ],
        vec![(ids[0], 0), (ids[1], 7), (ids[2], 3)],
        vec![(ids[0], 255), (ids[1], 7), (ids[2], 3)],
    ] {
        assert!(battle.deploy_initial([Formation::Loose; 2], &bad).is_err());
        assert_eq!(battle, previous);
    }
    assert_eq!(sources, originals);
    // The rounded 7-cell footprint omits its corner, but occupies the middle.
    let pos = battle.unit(ids[1]).unwrap().position.unwrap();
    assert!(!battle
        .field()
        .cell(pos.at.0, pos.at.1)
        .unwrap()
        .occupants
        .iter()
        .any(|o| o.id == ids[1].0));
    assert!(battle
        .field()
        .cell(pos.at.0 + 3, pos.at.1 + 3)
        .unwrap()
        .occupants
        .iter()
        .any(|o| o.id == ids[1].0));
    battle.begin_round(&ids).unwrap();
    let started = battle.clone();
    assert!(battle
        .deploy_initial([Formation::Loose; 2], &sizes)
        .is_err());
    assert_eq!(battle, started);
}

#[test]
fn anchors_validate_slots_and_keep_even_diameter_rounding() {
    assert_eq!(
        starting_position(Formation::Loose, Side::Attacker, 0, 4).unwrap(),
        UnitPosition {
            at: (75, 50),
            size: 4
        }
    );
    assert_eq!(
        starting_position(Formation::Square, Side::Defender, 6, 7)
            .unwrap()
            .at,
        (15, 43)
    );
    assert!(starting_position(Formation::Loose, Side::Attacker, 7, 4).is_err());
    assert!(starting_position(Formation::Loose, Side::Attacker, 0, 0).is_err());
}
