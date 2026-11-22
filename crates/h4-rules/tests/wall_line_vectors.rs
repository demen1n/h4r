//! The castle wall check of a melee strike (`1701f0`) compared with answers recorded from
//! the original exe (`tools/testbench/wall_line.py`). Runs without the game.
use std::collections::HashMap;

use h4_rules::battle::wall_line::{wall_between, CastlePart, WallField};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};
use h4_rules::damage::AcrossWall;

#[test]
fn wall_lines_match_the_original() {
    let grid = BattlefieldGrid::standard(CellSurface::default());
    let row_bounds = |x: i32| grid.row_bounds(x);
    let mut counts = [0; 3];
    for line in include_str!("wall_line_vectors.tsv").lines() {
        if !line.starts_with("W ") {
            continue;
        }
        // W gate parts -> answers
        let (inputs, answers) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = inputs.split(' ').collect();
        let mut parts = HashMap::new();
        if f[2] != "-" {
            for p in f[2].split(';') {
                let q: Vec<&str> = p.split(':').collect();
                let kind = match q[2] {
                    "w" => CastlePart::Wall,
                    "g" => CastlePart::Gate,
                    _ => CastlePart::None,
                };
                parts.insert((q[0].parse::<i32>().unwrap(), q[1].parse::<i32>().unwrap()), kind);
            }
        }
        let part = |at: (i32, i32)| parts.get(&at).copied().unwrap_or(CastlePart::None);
        let field = WallField { row_bounds: &row_bounds, part: &part, gate_closed: f[1] == "0" };
        for answer in answers.split(' ') {
            let (points, result) = answer.split_once('=').unwrap();
            let p: Vec<i32> = points.split(',').map(|v| v.parse().unwrap()).collect();
            let expected = match result {
                "0" => AcrossWall::No,
                "1" => AcrossWall::TowardGreaterX,
                "2" => AcrossWall::TowardSmallerX,
                other => panic!("unknown answer {other}"),
            };
            assert_eq!(wall_between(&field, (p[0], p[1]), (p[2], p[3])), expected, "{answer} in {line}");
            counts[result.parse::<usize>().unwrap()] += 1;
        }
    }
    assert!(counts.iter().all(|&c| c > 100), "{counts:?}");
}
