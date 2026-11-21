//! The battlefield path search compared with answers recorded from the original exe
//! (`tools/testbench/combat_path.py`, rva `20d200`, `20e640`, `20e7b0`). Runs without the game.
use std::collections::HashMap;

use h4_rules::battle::path::{flags, Mover, PathCell, PathMap, PathSearch};

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
fn path_search_matches_the_original() {
    let (mut total, mut reached, mut found, mut phase, mut covered_others) = (0, 0, 0, 0, 0);
    let mut traits_seen = HashMap::new();
    for line in include_str!("combat_path_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // P rows mover start zones cells targets -> changed paths...
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = list(f[1]).iter().map(|r| (number(r[0]), number(r[1]))).collect();
        let mover_fields: Vec<&str> = f[2].split(':').collect();
        let traits = mover_fields[2];
        for t in traits.chars().filter(|&c| c != '-') {
            *traits_seen.entry(t).or_insert(0) += 1;
        }
        let has = |c| traits.contains(c);
        let mover = Mover {
            movement: number(mover_fields[0]),
            footprint_size: number(mover_fields[1]),
            long_weapon: has('l'),
            // no effect 7, and the flying ability or effect 0x2e
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
        let before = map.clone();
        let mut search = PathSearch::new(map);
        search.seed_start((start[0], start[1])).unwrap();
        // the real preparation refreshes the start's entry cost after seeding it
        search.map.cell_mut((start[0], start[1])).unwrap().enter_cost = start[2] as u16;
        search.run(&mover, |at| {
            let mask = zones.get(&at).copied().unwrap_or(0);
            std::array::from_fn(|d| (mask >> d) & 1 != 0)
        });

        let arrow = f.iter().position(|&s| s == "->").unwrap();
        let mut expected = Vec::new();
        for c in list(f[arrow + 1]) {
            let n: Vec<i32> = c.iter().enumerate().map(|(i, v)| if i == 5 { hex(v) as i32 } else { number(v) }).collect();
            let cell = PathCell {
                enter_cost: n[2] as u16,
                over_occupied: n[3] as u16,
                cost: n[4] as u32,
                flags: n[5] as u32,
                from: (n[6], n[7]),
                origin: (n[8], n[9]),
            };
            expected.push(((n[0], n[1]), cell));
        }
        let got: Vec<((i32, i32), PathCell)> = search
            .map
            .cells()
            .filter(|(at, cell)| before.cell(*at) != Some(*cell))
            .map(|(at, cell)| (at, *cell))
            .collect();
        assert_eq!(got, expected, "{line}");
        for (at, cell) in &got {
            reached += i32::from(cell.flags & flags::REACHED != 0);
            phase += i32::from(cell.flags & (flags::REACHED | flags::MARK) == flags::MARK);
            covered_others += i32::from(cell.flags & flags::COVERED != 0 && cell.origin != *at);
        }

        let targets = list(f[6]);
        for (target, answer) in targets.iter().zip(&f[arrow + 2..]) {
            let (ok, points) = answer.split_once(':').unwrap();
            let want: Option<Vec<(i32, i32)>> = (ok == "1").then(|| {
                points.split('/').map(|p| {
                    let (x, y) = p.split_once(',').unwrap();
                    (number(x), number(y))
                }).collect()
            });
            let path = search.path_to((number(target[0]), number(target[1])));
            assert_eq!(path, want, "target {target:?} in {line}");
            found += i32::from(path.is_some());
        }
        total += 1;
    }
    // The file must keep exercising the branches: reached cells, the occupied-cell
    // phase, cover by other corners, found paths, every trait.
    assert!(total >= 400, "{total} cases");
    assert!(reached >= 10_000 && phase >= 500 && covered_others >= 1000 && found >= 500,
            "{reached} reached, {phase} phase, {covered_others} covered, {found} paths");
    for t in ['f', 'e', 'b', 'h', 'l', 't'] {
        assert!(traits_seen.get(&t).copied().unwrap_or(0) >= 15, "trait {t}: {traits_seen:?}");
    }
}
