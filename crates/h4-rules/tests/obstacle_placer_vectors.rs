//! Obstacle placement against answers of the original placer
//! (`tools/testbench/obstacle_placer.py`, synthetic inputs; no game data needed).
#[path = "../examples/support/obstacle_case.rs"]
mod obstacle_case;

use h4_rules::obstacle_placement::{place_obstacles, ObstacleWeights};
use h4_rules::random::CombatRandom;

#[test]
fn placements_match_the_original_placer() {
    let expected: Vec<&str> = include_str!("obstacle_placer_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();
    let models = obstacle_case::synthetic_models();
    let weights = ObstacleWeights::from_table(&obstacle_case::synthetic_table());
    let scenarios = obstacle_case::scenarios(expected.len() as u32);
    assert_eq!(scenarios.len(), 24);
    let mut placements = 0;
    for (scenario, line) in scenarios.iter().zip(&expected) {
        let grid = obstacle_case::field(scenario);
        let mut random = CombatRandom::new(scenario.seed, 1);
        let placed = place_obstacles(&grid, &models, &weights, &scenario.context, &mut random);
        let ours: Vec<i32> = placed
            .iter()
            .flat_map(|p| [p.model as i32, p.at.0, p.at.1])
            .collect();
        let theirs: Vec<i32> = line
            .split_whitespace()
            .skip(1)
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(ours, theirs, "scenario {}", scenario.index);
        placements += placed.len();
    }
    println!("{placements} placements match");
    assert_eq!(placements, 3107);
}
