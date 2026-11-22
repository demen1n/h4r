//! Movement points of a combat unit (`1ef750`) compared with answers recorded from the
//! original exe (`tools/testbench/move_points.py`). Runs without the game.
use h4_rules::battle::move_points::{creature_base, movement_points, CommandMode, MoveModifiers, HERO_BASE};
use h4_rules::damage::ability;

fn int(field: &str) -> i32 {
    field.parse().unwrap()
}

#[test]
fn movement_points_match_the_original() {
    let mut cases = 0;
    for line in include_str!("move_points_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // M member base army artifact at_sea sea_bonus effects mode -> points
        let f: Vec<&str> = line.split(' ').collect();
        let base = if f[1] == "c" { creature_base(int(f[2])) } else { HERO_BASE };
        let effects = if f[7] == "-" {
            Vec::new()
        } else {
            f[7].split(',').map(|e| u8::from_str_radix(e, 16).unwrap()).collect()
        };
        let modifiers = MoveModifiers {
            army_bonus: int(f[3]),
            artifact_bonus: int(f[4]),
            at_sea: f[5] == "1",
            abilities: if f[6] == "1" { vec![ability::SEA_BONUS] } else { Vec::new() },
            effects,
            mode: match f[8] {
                "0" => CommandMode::Normal,
                "1" => CommandMode::Unlimited,
                "2" => CommandMode::Capped,
                other => panic!("unknown mode {other}"),
            },
        };
        assert_eq!(movement_points(base, &modifiers), int(f[10]), "{line}");
        cases += 1;
    }
    assert_eq!(cases, 1500);
}
