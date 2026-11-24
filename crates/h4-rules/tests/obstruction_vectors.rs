//! The obstruction pass after obstacle placement against answers of the original
//! (`tools/testbench/obstacle_placer.py`, synthetic inputs; no game data needed):
//! which 7×7 squares are cleared, path by path, and which segments are removed.
#[path = "../examples/support/obstacle_case.rs"]
mod obstacle_case;

use h4_rules::obstacle_placement::{place_obstacles, ObstacleWeights};
use h4_rules::random::CombatRandom;

#[test]
fn carving_matches_the_original_finder() {
    let expected: Vec<&str> = include_str!("obstruction_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();
    let models = obstacle_case::synthetic_models();
    let weights = ObstacleWeights::from_table(&obstacle_case::synthetic_table());
    let scenarios = obstacle_case::scenarios(expected.len() as u32);
    assert_eq!(scenarios.len(), 24);
    let (mut paths, mut removed) = (0, 0);
    for (scenario, line) in scenarios.iter().zip(&expected) {
        let mut grid = obstacle_case::field(scenario);
        let mut random = CombatRandom::new(scenario.seed, 1);
        let placed = place_obstacles(&grid, &models, &weights, &scenario.context, &mut random);
        let ours = obstacle_case::carve(&mut grid, &models, &placed, &scenario.context);
        assert_eq!(
            ours.split_whitespace().collect::<Vec<_>>(),
            line.split_whitespace().collect::<Vec<_>>(),
            "scenario {}",
            scenario.index
        );
        let (carved, gone) = line.split_once("removed").unwrap();
        paths += carved.matches('|').count();
        removed += gone.split_whitespace().count();
    }
    println!("{paths} cleared paths and {removed} removed segments match");
    assert_eq!((paths, removed), (99, 361));
}
