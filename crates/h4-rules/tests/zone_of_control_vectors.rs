//! Zones of control compared with answers recorded from the original exe
//! (`tools/testbench/zone_of_control.py`: `172000` with the threat footprints of `48b350` /
//! `48b090`, then `172210`). Runs without the game.
use std::collections::HashMap;

use h4_rules::battle::zone_of_control::{zone_directions, Threat, ThreatMap, ZoneMover};
use h4_rules::battle::Side;

fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

#[test]
fn zones_of_control_match_the_original() {
    let (mut cases, mut threatened_cells, mut set, mut traits_seen) = (0, 0, 0, HashMap::new());
    for line in include_str!("zone_of_control_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // Z rows creatures -> threats answers...
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = f[1]
            .split(';')
            .map(|r| {
                let (start, end) = r.split_once(':').unwrap();
                (number(start), number(end))
            })
            .collect();
        let mut threats = Vec::new();
        let mut traits = Vec::new();
        for c in f[2].split(';') {
            let v: Vec<&str> = c.split(':').collect();
            let t = v[6];
            for letter in t.chars().filter(|&c| c != '-') {
                *traits_seen.entry(letter).or_insert(0) += 1;
            }
            threats.push(Threat {
                side: if v[4] == "0" { Side::Attacker } else { Side::Defender },
                alive: v[5] != "0",
                position: (number(v[0]), number(v[1])),
                size: number(v[2]),
                facing: number(v[3]) as u32,
                long_weapon: t.contains('l'),
                hydra_strike: t.contains('h'),
                flies: !t.contains('b') && (t.contains('f') || t.contains('e')),
            });
            traits.push(t);
        }
        assert_eq!(f[3], "->");
        let map = ThreatMap::build(&rows, &threats);

        // Every cell's threat list.
        let mut expected = HashMap::new();
        if f[4] != "-" {
            for cell in f[4].split(';') {
                let v: Vec<&str> = cell.split(':').collect();
                let list: Vec<usize> = v[2].split(',').map(|i| i.parse().unwrap()).collect();
                expected.insert((number(v[0]), number(v[1])), list);
            }
        }
        for (x, &(start, end)) in rows.iter().enumerate() {
            for y in start..end {
                let at = (x as i32, y);
                let want = expected.get(&at).map_or(&[][..], Vec::as_slice);
                assert_eq!(map.at(at), want, "threats of {at:?} in {line}");
            }
        }
        threatened_cells += expected.len();

        for answer in f[5..].iter().filter(|a| **a != "-") {
            let v: Vec<&str> = answer.split(':').collect();
            let index: usize = v[0].parse().unwrap();
            let t = traits[index];
            let mover = ZoneMover {
                side: threats[index].side,
                size: threats[index].size,
                flies: threats[index].flies,
                ignores_zones: t.contains('i'),
            };
            let mask = zone_directions(&map, &threats, mover, (number(v[1]), number(v[2])));
            let bits = (0..8).filter(|&d| mask[d]).fold(0u32, |all, d| all | 1 << d);
            assert_eq!(bits, u32::from_str_radix(v[3], 16).unwrap(), "{answer} in {line}");
            set += usize::from(bits != 0);
        }
        cases += 1;
    }
    eprintln!("{cases} fields, {threatened_cells} threatened cells, {set} zone masks set, traits {traits_seen:?}");
    assert_eq!(cases, 300);
    assert!(threatened_cells > 10_000 && set > 300, "{threatened_cells} cells, {set} masks");
    for letter in ['l', 'h', 'f', 'i', 'b', 'e'] {
        assert!(traits_seen.get(&letter).copied().unwrap_or(0) >= 30, "trait {letter}: {traits_seen:?}");
    }
}
