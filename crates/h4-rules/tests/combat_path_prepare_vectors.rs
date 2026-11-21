//! The path map preparation compared with answers recorded from the original exe
//! (`tools/testbench/combat_path_prepare.py`, rva `20c7f0`, `51ede0`). Runs without the game.
use std::collections::HashMap;

use h4_rules::battle::path::{PathCell, PathMap};
use h4_rules::battle::path_prepare::{
    direction, prepare, FieldObject, FieldObjectKind, GateState, PrepareInput, PreparedMover, TowerOccupant, TowerPlace,
};

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

fn numbers(s: &str) -> Vec<i32> {
    if s == "-" {
        return Vec::new();
    }
    s.split(',').map(number).collect()
}

fn position(s: &str) -> (i32, i32) {
    let p: Vec<i32> = s.split('/').map(number).collect();
    (p[0], p[1])
}

fn tower(s: &str) -> Option<TowerPlace> {
    (s != "-").then(|| {
        let p: Vec<i32> = s.split('/').map(number).collect();
        TowerPlace { size: p[0], position: (p[1], p[2]) }
    })
}

fn record(c: &[&str]) -> ((i32, i32), PathCell) {
    let n = |i: usize| number(c[i]);
    let cell = PathCell {
        enter_cost: n(2) as u16,
        over_occupied: n(3) as u16,
        cost: n(4) as u32,
        flags: hex(c[5]),
        from: (n(6), n(7)),
        origin: (n(8), n(9)),
    };
    ((n(0), n(1)), cell)
}

#[test]
fn directions_match_the_original() {
    let mut total = 0;
    for line in include_str!("combat_path_prepare_vectors.tsv").lines().filter(|l| l.starts_with("D ")) {
        let f: Vec<&str> = line.split(' ').collect();
        assert_eq!(direction((0, 0), (number(f[1]), number(f[2]))), number(f[4]) as u32, "{line}");
        total += 1;
    }
    assert_eq!(total, 41 * 41);
}

#[test]
fn path_preparation_matches_the_original() {
    let (mut total, mut towers, mut opened, mut queued) = (0, 0, 0, 0);
    for line in include_str!("combat_path_prepare_vectors.tsv").lines().filter(|l| l.starts_with("R ")) {
        // R rows size template mover objects masks costs gate -> cells queue calls
        let f: Vec<&str> = line.split(' ').collect();
        let rows: Vec<(i32, i32)> = list(f[1]).iter().map(|r| (number(r[0]), number(r[1]))).collect();
        let size = number(f[2]);
        let template_flags: HashMap<(i32, i32), u32> =
            list(f[3]).iter().map(|c| ((number(c[0]), number(c[1])), hex(c[2]))).collect();
        let mut template = PathMap::new(&rows);
        let all: Vec<(i32, i32)> = template.cells().map(|(at, _)| at).collect();
        for &at in &all {
            let flags = template_flags.get(&at).copied().unwrap_or(0);
            *template.cell_mut(at).unwrap() =
                PathCell { enter_cost: 0, over_occupied: 0, cost: 0, flags, from: (0, 0), origin: (0, 0) };
        }

        let m: Vec<&str> = f[4].split(':').collect();
        let effects = numbers(m[2]);
        let abilities = numbers(m[3]);
        let mover = PreparedMover {
            position: position(m[0]),
            size,
            tower: tower(m[1]),
            teleports: m[5] == "1",
            flies: !effects.contains(&7) && (abilities.contains(&0x19) || effects.contains(&0x2e)),
            defender: m[4] == "1",
        };
        let attack_mode = m[6] == "1";

        let objects: Vec<FieldObject> = list(f[5])
            .iter()
            .map(|o| {
                if o[0] == "m" {
                    let kind = FieldObjectKind::Creature { hostile: false, cowardly: false, effect_89: false, tower: None };
                    return FieldObject { kind, is_mover: true, alive: true, fixed: false, skip: false, size, position: mover.position };
                }
                let effect_89 = numbers(o[6]).contains(&0x89);
                let (hostile, cowardly) = (o[7] == "1", o[8] == "1");
                let kind = match o[0] {
                    "c" => FieldObjectKind::Creature { hostile, cowardly, effect_89, tower: tower(o[9]) },
                    "o" => FieldObjectKind::Obstacle,
                    "g" => FieldObjectKind::Gate,
                    "t" => FieldObjectKind::Tower {
                        occupant: (o[10] != "-").then(|| {
                            let p: Vec<&str> = o[10].split('/').collect();
                            TowerOccupant { hostile: p[0] == "1", cowardly: p[1] == "1", effect_89: numbers(p[2]).contains(&0x89) }
                        }),
                    },
                    "w" => FieldObjectKind::Wall,
                    _ => FieldObjectKind::Other,
                };
                FieldObject {
                    kind,
                    is_mover: false,
                    alive: o[1] == "1",
                    fixed: o[2] == "1",
                    skip: o[3] == "1",
                    size: number(o[4]),
                    position: position(o[5]),
                }
            })
            .collect();
        let masks: HashMap<(i32, i32), u32> = list(f[6]).iter().map(|c| ((number(c[0]), number(c[1])), hex(c[2]))).collect();
        let costs: Vec<(u32, u16)> = list(f[7]).iter().map(|c| (hex(c[0]), number(c[1]) as u16)).collect();
        let g: Vec<i32> = f[8].split(':').map(number).collect();
        let gate = GateState { present: g[0] == 1, state_177990: g[1] == 1, state_1778c0: g[2] == 1, row: g[3] };

        let terrain = |at: (i32, i32)| masks.get(&at).copied().unwrap_or(0);
        let prepared = prepare(&PrepareInput {
            template: &template,
            mover,
            objects: &objects,
            gate,
            attack_mode,
            terrain: &terrain,
            costs: &costs,
        });

        // f[9] is "->"
        let mut expected: HashMap<(i32, i32), PathCell> = HashMap::new();
        for &at in &all {
            let flags = template_flags.get(&at).copied().unwrap_or(0);
            expected.insert(at, PathCell { enter_cost: 100, over_occupied: 0, cost: 0, flags, from: (0, 0), origin: (0, 0) });
        }
        for c in list(f[10]) {
            let (at, cell) = record(&c);
            expected.insert(at, cell);
        }
        for &at in &all {
            assert_eq!(prepared.search.map.cell(at), expected.get(&at), "cell {at:?} of {line}");
        }
        let queue: Vec<((i32, i32), PathCell)> = prepared.search.queued().map(|(at, cell)| (at, *cell)).collect();
        let want: Vec<((i32, i32), PathCell)> = list(f[11]).iter().map(|c| record(c)).collect();
        assert_eq!(queue, want, "queue of {line}");
        assert_eq!(prepared.opens_gate, f[12].contains("open1"), "gate of {line}");
        assert_eq!(!mover.teleports && !mover.flies, f[12].contains("costs"), "cost list of {line}");

        towers += i32::from(mover.tower.is_some());
        opened += i32::from(prepared.opens_gate);
        queued += queue.len();
        total += 1;
    }
    assert!(total >= 800 && towers >= 100 && opened >= 20 && queued >= 900, "{total} {towers} {opened} {queued}");
}
