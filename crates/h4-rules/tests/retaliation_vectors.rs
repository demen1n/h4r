//! can_retaliate and can_shoot compared with answers recorded from the original
//! exe (`tools/testbench/retaliation.py`). Runs without the game.
use h4_rules::battle::melee::RetaliationFlag;
use h4_rules::battle::retaliation::{abilities, can_retaliate, can_shoot, effects, Footprint, RetaliationCheck};

fn int(field: &str) -> i32 {
    field.parse().unwrap()
}

fn flag(field: &str) -> RetaliationFlag {
    RetaliationFlag { retaliated: field == "1", stamp: 0 }
}

/// Two 1×1 stacks next to each other, as the bench places them for F, S and E.
fn neighbours() -> (Footprint, Footprint) {
    (Footprint { position: (800, 784), size: 1 }, Footprint { position: (800, 800), size: 1 })
}

#[test]
fn retaliation_checks_match_the_original() {
    let mut counts = [0; 4];
    for line in include_str!("retaliation_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(' ').collect();
        let (attacker, target) = neighbours();
        let expected = *f.last().unwrap() == "1";
        let got = match f[0] {
            "F" => {
                let mut target_effects = Vec::new();
                if f[3] != "-" {
                    target_effects.push(effects::INCAPACITATING_BA);
                }
                if f[4] != "-" {
                    target_effects.push(effects::INCAPACITATING_9A);
                }
                counts[0] += 1;
                can_retaliate(&RetaliationCheck {
                    n: int(f[1]),
                    target_flag: flag(f[2]),
                    target_effects: &target_effects,
                    target_strength_ba: if f[3] == "-" { 0 } else { int(f[3]) },
                    target_strength_9a: if f[4] == "-" { 0 } else { int(f[4]) },
                    attacker,
                    target,
                    ..Default::default()
                })
            }
            "S" => {
                let target_abilities = if f[3] == "1" { vec![abilities::RANGED] } else { vec![] };
                let target_effects = if f[4] == "1" { vec![effects::NO_SHOOTING] } else { vec![] };
                counts[1] += 1;
                can_retaliate(&RetaliationCheck {
                    n: int(f[1]),
                    ranged: true,
                    target_flag: flag(f[2]),
                    target_abilities: &target_abilities,
                    target_effects: &target_effects,
                    target_can_shoot: can_shoot(&target_abilities, &target_effects, int(f[5]), int(f[6])),
                    attacker,
                    target,
                    ..Default::default()
                })
            }
            "G" => {
                let long = |field: &str| if field == "1" { vec![abilities::LONG_WEAPON] } else { vec![] };
                let (attacker_abilities, target_abilities) = (long(f[1]), long(f[2]));
                counts[2] += 1;
                can_retaliate(&RetaliationCheck {
                    n: 1,
                    attacker_abilities: &attacker_abilities,
                    target_abilities: &target_abilities,
                    attacker: Footprint { position: (int(f[5]), int(f[6])), size: int(f[3]) },
                    target: Footprint { position: (int(f[7]), int(f[8])), size: int(f[4]) },
                    ..Default::default()
                })
            }
            "E" => {
                let mut attacker_abilities = Vec::new();
                if f[1] == "1" {
                    attacker_abilities.push(abilities::NO_RETALIATION);
                }
                if f[4] == "1" {
                    attacker_abilities.push(abilities::PANIC);
                }
                let attacker_effects = if f[2] == "1" { vec![effects::ATTACKER_24] } else { vec![] };
                let target_abilities = if f[6] == "1" { vec![abilities::DEATH_PROTECTION] } else { vec![] };
                counts[3] += 1;
                can_retaliate(&RetaliationCheck {
                    n: 1,
                    attacker_abilities: &attacker_abilities,
                    attacker_effects: &attacker_effects,
                    target_abilities: &target_abilities,
                    effect_25_can_affect_target: f[3] == "1",
                    effect_6b_can_affect_target: f[5] == "1",
                    attacker,
                    target,
                    ..Default::default()
                })
            }
            other => panic!("unknown vector kind {other}"),
        };
        assert_eq!(got, expected, "{line}");
    }
    println!("flags {} / ranged {} / reach {} / effects {}", counts[0], counts[1], counts[2], counts[3]);
    assert_eq!(counts, [250, 144, 4032, 64]);
}
