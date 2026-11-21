//! Where a moving creature stops (`16c4a0`) and in how many turns it gets somewhere
//! (`171340`), compared with answers recorded from the original exe
//! (`tools/testbench/combat_path_stop.py`, on top of the path search bench). Runs without
//! the game.
use std::collections::HashMap;

use h4_rules::battle::move_command::{destination, turns_to_reach};
use h4_rules::battle::path::{Mover, PathCell, PathMap, PathSearch};

fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn list(s: &str) -> Vec<Vec<&str>> {
    if s == "-" {
        return Vec::new();
    }
    s.split(';').map(|item| item.split(':').collect()).collect()
}

#[test]
fn stops_and_turns_match_the_original() {
    let (mut stops, mut cut_short, mut fallback, mut turns_seen) = (0, 0, 0, HashMap::new());
    for line in include_str!("combat_path_stop_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // S rows mover start zones cells -> answers (the world as in combat_path_vectors.tsv)
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = list(f[1]).iter().map(|r| (number(r[0]), number(r[1]))).collect();
        let m: Vec<&str> = f[2].split(':').collect();
        let has = |c| m[2].contains(c);
        let mover = Mover {
            movement: number(m[0]),
            footprint_size: number(m[1]),
            long_weapon: has('l'),
            flies: !has('b') && (has('f') || has('e')),
            held: has('h'),
            teleports: has('t'),
        };
        let start: Vec<i32> = f[3].split(':').map(number).collect();
        let zones: HashMap<(i32, i32), u32> = list(f[4]).iter().map(|z| ((number(z[0]), number(z[1])), hex(z[2]))).collect();
        let mut map = PathMap::new(&rows);
        for c in list(f[5]) {
            let at = (number(c[0]), number(c[1]));
            *map.cell_mut(at).unwrap() = PathCell::new(at, hex(c[2]), number(c[3]) as u16);
        }
        let mut search = PathSearch::new(map);
        let from = (start[0], start[1]);
        search.seed_start(from).unwrap();
        search.map.cell_mut(from).unwrap().enter_cost = start[2] as u16;
        search.run(&mover, |at| {
            let mask = zones.get(&at).copied().unwrap_or(0);
            std::array::from_fn(|d| (mask >> d) & 1 != 0)
        });

        assert_eq!(f[6], "->");
        for answer in &f[7..] {
            let a: Vec<&str> = answer.split(':').collect();
            let target = (number(a[0]), number(a[1]));
            if a[2] != "-" {
                let (x, y) = a[2].split_once(',').unwrap();
                let want = (number(x), number(y));
                assert_eq!(destination(&search, target, mover.movement), Some(want), "{answer} in {line}");
                stops += 1;
                cut_short += usize::from(want != target);
                // The exe's fallback: no point qualifies, the answer is the target itself.
                let cell = search.map.cell(target).unwrap();
                fallback += usize::from(want == target && (cell.cost > mover.movement as u32 || cell.flags & 4 != 0));
            }
            let turns = turns_to_reach(&search, from, target, mover.movement);
            assert_eq!(turns, number(a[3]), "{answer} in {line}");
            *turns_seen.entry(turns).or_insert(0) += 1;
        }
    }
    eprintln!("{stops} stops, {cut_short} short of the target, {fallback} fallbacks, turns {turns_seen:?}");
    // Keep exercising the branches: stops short of the target, the fallback, every turn count.
    assert!(stops >= 1000 && cut_short >= 100 && fallback >= 50, "{stops} stops, {cut_short} short, {fallback} fallbacks");
    for turns in [-1, 1, 2, 3, 4] {
        assert!(turns_seen.get(&turns).copied().unwrap_or(0) >= 20, "turns {turns}: {turns_seen:?}");
    }
}
