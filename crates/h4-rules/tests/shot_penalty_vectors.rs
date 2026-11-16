//! The shot penalty chain compared with answers recorded from the original exe
//! (`tools/testbench/shot_penalty.py`, rva `170800`, which runs the line-of-fire
//! trace, the edge distance and the penalty table). Runs without the game.
mod support {
    pub mod fire_world;
}
use h4_rules::battle::shooting::{shot_range_penalty, ShotInput};
use support::fire_world::{number, parse_world};

#[test]
fn shot_penalty_matches_the_original() {
    let mut seen = std::collections::BTreeMap::new();
    for line in include_str!("shot_penalty_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // P rows gate stacks shooter cells abilities shots threats target-name -> penalty
        let f: Vec<&str> = line.split(' ').collect();
        let world = parse_world(&[f[1], f[2], f[3], f[4], f[5], "0:0", "0", "-"]);
        let abilities: Vec<u8> = if f[6] == "-" { Vec::new() } else { f[6].split(',').map(|a| a.parse().unwrap()).collect() };
        let target = f[9].chars().next().unwrap();
        // Positions are the stacks' own: the corner of the footprint, from the world's centers.
        let shooter: Vec<&str> = f[4].split(':').collect();
        let target_text = f[3].split(';').find(|s| s.starts_with(target)).unwrap();
        let t: Vec<&str> = target_text.split(':').collect();
        let input = ShotInput {
            shooter_abilities: &abilities,
            shots: number(f[7]),
            threats: number(f[8]),
            shooter_position: (number(shooter[4]), number(shooter[5])),
            shooter_size: number(shooter[3]),
            target_position: (number(t[5]), number(t[6])),
            target_size: number(t[4]),
        };
        let got = shot_range_penalty(&world, 'S', target, &input);
        assert_eq!(got, number(f[11]), "{line}");
        *seen.entry(got).or_insert(0) += 1;
    }
    // Every answer the table can give is exercised.
    for penalty in [0, 1, 2, 4, 8] {
        assert!(seen.get(&penalty).copied().unwrap_or(0) >= 5, "{seen:?}");
    }
}
