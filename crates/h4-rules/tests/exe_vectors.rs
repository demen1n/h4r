//! Golden answers captured by tools/testbench/compare.py from the original exe.
//! These regression tests need neither the exe nor Unicorn.
use h4_rules::{
    damage::{roll_base_damage, Attacker, School},
    random::CombatRandom,
};

fn vectors(kind: &str) -> impl Iterator<Item = (&'static str, Vec<u32>, Vec<u32>)> + '_ {
    include_str!("exe_vectors.tsv")
        .lines()
        .filter_map(move |line| {
            if !line.starts_with(kind) {
                return None;
            }
            let (request, expected) = line.split_once('\t').expect("fixture has two columns");
            let input = request
                .split_whitespace()
                .skip(1)
                .map(|v| v.parse().unwrap())
                .collect();
            let output = expected
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            Some((request, input, output))
        })
}

#[test]
fn combat_rng_matches_original_vectors() {
    let mut cases = 0;
    for (request, input, expected) in vectors("combat ") {
        cases += 1;
        let (seed, modulus) = (input[0], input[1]);
        let mut rng = CombatRandom::new(seed, modulus);
        let mut got = Vec::new();
        for roll in input[2..].chunks_exact(3) {
            if roll[2] == 44 {
                rng = CombatRandom::new(roll[0], modulus);
            } else {
                got.push(rng.roll(roll[0], roll[1], roll[2]));
            }
        }
        assert_eq!(got, expected, "{request}");
    }
    assert_eq!(cases, 16);
}

#[test]
fn damage_dice_and_consumption_match_original_vectors() {
    let mut cases = 0;
    for (request, input, expected) in vectors("dice ") {
        cases += 1;
        let [seed, modulus, attacker, target, number, low, high, hits] = input[..] else {
            panic!("invalid fixture: {request}");
        };
        let a = Attacker {
            id: attacker,
            number: number as i32,
            damage_low: low as i32,
            damage_high: high as i32,
            offense: 1.0,
            defense_reduction: 1.0,
            school: School::Might,
            abilities: vec![],
            giant_bonus_effect: false,
        };
        let mut rng = CombatRandom::new(seed, modulus);
        let mut got: Vec<u32> = (0..hits)
            .map(|_| roll_base_damage(&a, target, &mut rng) as u32)
            .collect();
        got.push(rng.roll(target, attacker, 0));
        assert_eq!(got, expected, "{request}");
    }
    assert_eq!(cases, 48);
}
