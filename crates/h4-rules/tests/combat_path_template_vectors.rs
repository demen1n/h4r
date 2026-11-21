//! The template path map of a footprint size compared with answers recorded from the
//! original exe (`tools/testbench/combat_path_template.py`, rva `20e900`, `20dfd0`).
//! Runs without the game.
use std::collections::HashSet;

use h4_rules::battle::path_template::{template, BlockageMap, Blocker, BlockerKind};

fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

fn list(s: &str) -> Vec<Vec<&str>> {
    if s == "-" {
        return Vec::new();
    }
    s.split(';').map(|item| item.split(':').collect()).collect()
}

fn blockers(s: &str) -> Vec<Blocker> {
    list(s)
        .iter()
        .map(|b| Blocker {
            kind: match b[0] {
                "o" => BlockerKind::Object { blocks: b[1] == "1", fixed: b[2] == "1" },
                "t" => BlockerKind::Tower,
                "w" => BlockerKind::Wall,
                _ => BlockerKind::OtherCastlePart,
            },
            size: number(b[3]) as u8,
            position: (number(b[4]), number(b[5])),
        })
        .collect()
}

#[test]
fn path_template_matches_the_original() {
    let (mut total, mut impassable, mut flagged) = (0, 0, [0; 4]);
    for line in include_str!("combat_path_template_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // M rows blocked objects towers size -> blockage template
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = list(f[1]).iter().map(|r| (number(r[0]), number(r[1]))).collect();
        let blocked: HashSet<(i32, i32)> = list(f[2]).iter().map(|c| (number(c[0]), number(c[1]))).collect();
        // The first list must not contain castle parts: they are told apart by the list.
        let mut all = blockers(f[3]);
        all.extend(blockers(f[4]).into_iter().map(|b| match b.kind {
            // an ordinary object in the castle list is not marked there
            BlockerKind::Object { .. } => Blocker { kind: BlockerKind::OtherCastlePart, ..b },
            _ => b,
        }));
        let map = BlockageMap::build(&rows, &all, |x, y| blocked.contains(&(x, y)));

        let mut got = Vec::new();
        for (x, &(start, end)) in rows.iter().enumerate() {
            for y in start..end {
                let value = map.get((x as i32, y)).unwrap();
                if value != 0 {
                    got.push(format!("{x}:{y}:{value:x}"));
                }
            }
        }
        assert_eq!(got.join(";"), f[7].replace('-', ""), "blockage of {line}");

        let template = template(&rows, &map, number(f[5]) as u8);
        let got: Vec<String> = template
            .cells()
            .filter(|(_, cell)| cell.flags != 0)
            .map(|((x, y), cell)| format!("{x}:{y}:{:x}", cell.flags))
            .collect();
        assert_eq!(got.join(";"), f[8].replace('-', ""), "template of {line}");
        for (_, cell) in template.cells() {
            impassable += i32::from(cell.flags & 0x10 != 0);
            for (i, bit) in [0x04, 0x08, 0x20, 0x40].iter().enumerate() {
                flagged[i] += i32::from(cell.flags & 0x10 == 0 && cell.flags & bit != 0);
            }
        }
        total += 1;
    }
    assert!(total >= 500, "{total} cases");
    assert!(impassable >= 1000 && flagged.iter().all(|&n| n >= 300), "{impassable} impassable, {flagged:?}");
}
