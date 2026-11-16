//! The line-of-fire trace compared with answers recorded from the original exe
//! (`tools/testbench/line_of_fire.py`, rva `16fe00`). Runs without the game.
mod support {
    pub mod fire_world;
}
use h4_rules::battle::shooting::{trace_line_of_fire, LineOfFire};
use support::fire_world::{name, number, parse_world};

#[test]
fn line_of_fire_matches_the_original() {
    let mut verdicts = [0; 3];
    let mut hits = 0;
    let mut total = 0;
    for line in include_str!("line_of_fire_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // T rows gate stacks shooter cells target exact arg5 -> hit out
        let f: Vec<&str> = line.split(' ').collect();
        let world = parse_world(&f[1..9]);
        let (tx, ty) = f[6].split_once(':').unwrap();
        let center = world.stacks[&'S'].center;
        let (hit, verdict) = trace_line_of_fire(&world, 'S', center, (number(tx), number(ty)), f[7] == "1", name(f[8]));
        let expected = (name(f[10]), number(f[11]));
        let got = match verdict {
            LineOfFire::Clear => 0,
            LineOfFire::Obstructed => 1,
            LineOfFire::Blocked => 2,
        };
        assert_eq!((hit, got), expected, "{line}");
        verdicts[got as usize] += 1;
        hits += i32::from(hit.is_some());
        total += 1;
    }
    // The file must keep exercising every verdict and real hits.
    assert!(total >= 1000, "{total} cases");
    assert!(verdicts.iter().all(|&n| n >= 30), "{verdicts:?}");
    assert!(hits >= 200, "{hits} hits");
}
