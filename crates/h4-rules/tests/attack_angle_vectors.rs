//! The attack position list compared with answers recorded from the original exe
//! (`tools/testbench/attack_angles.py`, rva `10c870`). Runs without the game.
use h4_rules::battle::attack_angles::{attack_positions, AngleAttacker, AngleTarget};
use h4_rules::battle::path::PathMap;

fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

#[test]
fn attack_positions_match_the_original() {
    let (mut cases, mut empty, mut entries, mut long_weapon) = (0, 0, 0, 0);
    for line in include_str!("attack_angle_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // A rows mover target cells -> entries
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = f[1]
            .split(';')
            .map(|r| {
                let (start, end) = r.split_once(':').unwrap();
                (number(start), number(end))
            })
            .collect();
        let m: Vec<&str> = f[2].split(':').collect();
        let attacker = AngleAttacker {
            size: number(m[0]),
            movement: number(m[1]),
            long_weapon: m[2].contains('l'),
            effect_90: m[2].contains('x'),
        };
        let t: Vec<&str> = f[3].split(':').collect();
        let target = AngleTarget {
            position: (number(t[0]), number(t[1])),
            size: number(t[2]),
            slot_3c: t[3].contains('a'),
            effect_89: t[3].contains('x'),
        };
        let mut map = PathMap::new(&rows);
        for (x, row) in f[4].split(';').enumerate() {
            let start = rows[x].0;
            for (k, cell) in row.split(',').enumerate() {
                let (flags, cost) = cell.split_once('/').unwrap();
                let record = map.cell_mut((x as i32, start + k as i32)).unwrap();
                record.flags = u32::from_str_radix(flags, 16).unwrap();
                record.cost = cost.parse().unwrap();
            }
        }
        assert_eq!(f[5], "->");
        let expected = f[6];

        let list = attack_positions(&map, attacker, target);
        let got = if list.is_empty() {
            "-".to_string()
        } else {
            list.iter()
                .map(|p| format!("{}:{}:{}:{}:{}", p.angle, p.at.0, p.at.1, p.distance, p.cost))
                .collect::<Vec<_>>()
                .join(";")
        };
        assert_eq!(got, expected, "{line}");

        cases += 1;
        empty += usize::from(list.is_empty());
        entries += list.len();
        long_weapon += usize::from(attacker.long_weapon && !list.is_empty());
    }
    eprintln!("{cases} cases, {empty} empty, {entries} positions, {long_weapon} with long_weapon");
    assert_eq!(cases, 400);
    assert!(empty < cases / 2 && long_weapon > 20);
}
