//! A combat creature's terrain cost list (`225320`, with MSVC's `std::sort`) compared
//! with answers recorded from the original exe (`tools/testbench/terrain_costs.py`).
//! Runs without the game.
use std::collections::HashSet;

use h4_rules::battle::terrain_costs::{footprint_terrain_bits, terrain_costs, TerrainCostInput};
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};

#[test]
fn terrain_cost_lists_match_the_original() {
    let (mut cases, mut longest) = (0, 0);
    for line in include_str!("terrain_costs_vectors.tsv").lines() {
        if !line.starts_with("C ") {
            continue;
        }
        // C walk skill movement home -> pairs
        let (inputs, answer) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = inputs.split(' ').collect();
        let home: Vec<bool> = f[4].chars().map(|c| c == '1').collect();
        let input = TerrainCostInput {
            terrain_walk: f[1] == "1",
            hero_skill_0f: if f[2] == "-" { None } else { Some(f[2].parse().unwrap()) },
            movement: f[3].parse().unwrap(),
            at_home: std::array::from_fn(|i| home[i]),
        };
        let expected: Vec<(u32, i32)> = if answer == "-" {
            Vec::new()
        } else {
            answer
                .split(' ')
                .map(|p| {
                    let (bits, cost) = p.split_once(':').unwrap();
                    (u32::from_str_radix(bits, 16).unwrap(), cost.parse().unwrap())
                })
                .collect()
        };
        assert_eq!(terrain_costs(&input), expected, "{line}");
        cases += 1;
        longest = longest.max(expected.len());
    }
    assert_eq!(cases, 1200);
    // std::sort switches to a quicksort above 16 elements; the list never gets there.
    assert_eq!(longest, 15);
}

#[test]
fn footprint_terrain_bits_match_the_original() {
    let mut cases = 0;
    for line in include_str!("terrain_costs_vectors.tsv").lines() {
        if !line.starts_with("B ") {
            continue;
        }
        // B x y size creature cells -> bits
        let (inputs, answer) = line.split_once(" -> ").unwrap();
        let f: Vec<&str> = inputs.split(' ').collect();
        let int = |s: &str| s.parse::<i32>().unwrap();
        let hex = |s: &str| u8::from_str_radix(s, 16).unwrap();
        let mut grid = BattlefieldGrid::standard(CellSurface { terrain: 1, flags: 0x10 });
        let mut own = HashSet::new();
        if f[5] != "-" {
            for cell in f[5].split(';') {
                let c: Vec<&str> = cell.split(':').collect();
                let at = (int(c[0]), int(c[1]));
                *grid.surface_mut(at.0, at.1).unwrap() = CellSurface { terrain: hex(c[2]), flags: hex(c[3]) };
                if c[4] == "1" {
                    own.insert(at);
                }
            }
        }
        let with_creature = f[4] == "1";
        let bits = footprint_terrain_bits(&grid, (int(f[1]), int(f[2])), f[3].parse().unwrap(), |at| {
            !(with_creature && own.contains(&at))
        })
        .unwrap();
        assert_eq!(bits, u32::from_str_radix(answer, 16).unwrap(), "{line}");
        cases += 1;
    }
    assert_eq!(cases, 1200);
}
