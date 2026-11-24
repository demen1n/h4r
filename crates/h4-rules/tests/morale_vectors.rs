//! Combat morale against answers of the original code (`tools/testbench/morale.py`):
//! the army's morale (`23fe40`), the morale getters (`1efbd0`, `1efb90`), a stack's
//! strength (`1f5350`) and the loss penalties at a new round (`176250`).
use h4_rules::battle::morale::{
    army_morale, effective_morale, loss_penalties, morale_without_spells, strength, ArmyMember,
    MoraleInputs, StrengthEffects,
};

fn ids(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    text.split(',').map(|v| u8::from_str_radix(v, 16).unwrap()).collect()
}

#[test]
fn morale_matches_the_original() {
    let mut counts = [0; 4];
    for line in include_str!("morale_vectors.tsv").lines().filter(|l| !l.starts_with('#')) {
        let (input, output) = line.split_once(" -> ").unwrap();
        let w: Vec<&str> = input.split_whitespace().collect();
        let out: Vec<i32> = output.split_whitespace().map(|v| v.parse().unwrap()).collect();
        match w[0] {
            "A" => {
                let own: usize = w[1].parse().unwrap();
                let members: Vec<ArmyMember> = w[2]
                    .split(';')
                    .map(|m| {
                        let f: Vec<&str> = m.split(':').collect();
                        if f[0] == "h" {
                            ArmyMember { alignment: f[1].parse().unwrap(), count: 1, undead: false }
                        } else {
                            ArmyMember {
                                alignment: f[1].parse().unwrap(),
                                count: f[2].parse().unwrap(),
                                undead: f[3] == "1",
                            }
                        }
                    })
                    .collect();
                assert_eq!(army_morale(own, &members), out[0], "{line}");
                counts[0] += 1;
            }
            "B" => {
                let abilities = ids(w[3]);
                let effects = ids(w[4]);
                let m = MoraleInputs {
                    base: w[1].parse().unwrap(),
                    influence: w[2].parse().unwrap(),
                    undead: abilities.contains(&0x4d),
                    mechanical: abilities.contains(&0x2a),
                    sorrow: effects.contains(&0x91),
                    spiritual_fervor: effects.contains(&0x94),
                    morale_effect: effects.contains(&0x67),
                    despair: effects.contains(&0x19),
                };
                assert_eq!([effective_morale(&m), morale_without_spells(&m)], [out[0], out[1]], "{line}");
                counts[1] += 1;
            }
            "C" => {
                let n: Vec<i32> = [1, 2, 3, 5, 6].iter().map(|&i| w[i].parse().unwrap()).collect();
                let effects = ids(w[4]);
                let on = |e: u8| effects.contains(&e);
                let e = StrengthEffects {
                    giant_strength: on(0x33),
                    health_boost: on(0x39),
                    prayer: on(0x72),
                    dragon_strength: on(0x21),
                    celestial_armor: on(0x10).then_some(n[3]),
                    life_drain: on(0x44).then_some(n[4]),
                };
                assert_eq!(strength(n[0], n[1], n[2], &e), out[0], "{line}");
                counts[2] += 1;
            }
            "D" => {
                let initial = [w[1].parse().unwrap(), w[2].parse().unwrap()];
                let old: [i32; 2] = [w[3].parse().unwrap(), w[4].parse().unwrap()];
                let units: Vec<(usize, i32, i32)> = w[5]
                    .split(';')
                    .map(|u| {
                        let f: Vec<i32> = u.split(':').map(|v| v.parse().unwrap()).collect();
                        (f[0] as usize, f[1], f[2])
                    })
                    .collect();
                let mut now = [0i32; 2];
                for &(side, value, _) in &units {
                    now[side] = now[side].wrapping_add(value);
                }
                let penalties = loss_penalties(initial, now);
                let bases: Vec<i32> =
                    units.iter().map(|&(side, _, base)| base + penalties[side] - old[side]).collect();
                assert_eq!(penalties.to_vec(), out[..2].to_vec(), "{line}");
                assert_eq!(bases, out[2..].to_vec(), "{line}");
                counts[3] += 1;
            }
            other => panic!("unknown vector kind {other}"),
        }
    }
    println!("army {}, getter {}, strength {}, losses {}", counts[0], counts[1], counts[2], counts[3]);
    assert_eq!(counts, [708, 920, 800, 500]);
}
