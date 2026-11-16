//! The line-of-fire trace compared with answers recorded from the original exe
//! (`tools/testbench/line_of_fire.py`, rva `16fe00`). Runs without the game.
use std::collections::HashMap;

use h4_rules::battle::shooting::{
    footprint_center, trace_line_of_fire, FireCell, FireField, FireStack, LineOfFire,
};

/// A stack name from the file: "S" is the shooter.
type Name = char;

/// What a cell holds: obstacle bit, obstacle type, stack.
type CellData = (bool, Option<i32>, Option<Name>);

struct World {
    rows: Vec<(i32, i32)>,
    gate: Option<bool>,
    stacks: HashMap<Name, FireStack>,
    cells: HashMap<(i32, i32), CellData>,
}

impl FireField for World {
    type Ref = Name;
    fn rows(&self) -> i32 {
        self.rows.len() as i32
    }
    fn row_bounds(&self, x: i32) -> (i32, i32) {
        self.rows[x as usize]
    }
    fn cell(&self, x: i32, y: i32) -> FireCell<Name> {
        match self.cells.get(&(x, y)) {
            Some(&(obstacle_flag, obstacle, creature)) => FireCell { obstacle_flag, obstacle, creature },
            None => FireCell::default(),
        }
    }
    fn stack(&self, stack: Name) -> FireStack {
        self.stacks[&stack].clone()
    }
    fn gate_open(&self) -> Option<bool> {
        self.gate
    }
}

fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

fn effects(s: &str) -> Vec<u16> {
    if s == "-" { Vec::new() } else { s.split(',').map(|e| e.parse().unwrap()).collect() }
}

fn name(s: &str) -> Option<Name> {
    if s == "-" { None } else { s.chars().next() }
}

fn stack(f: &[&str]) -> FireStack {
    // side flag effects size px py
    FireStack {
        side: f[0] == "1",
        flag_9bc: f[1] == "1",
        effects: effects(f[2]),
        center: footprint_center((number(f[4]), number(f[5])), number(f[3])),
    }
}

#[test]
fn line_of_fire_matches_the_original() {
    let mut verdicts = [0; 3];
    let mut hits = 0;
    let mut total = 0;
    for line in include_str!("line_of_fire_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(' ').collect();
        // T rows gate stacks shooter cells target exact arg5 -> hit out
        let rows = f[1].split(';').map(|r| {
            let (a, b) = r.split_once(':').unwrap();
            (number(a), number(b))
        });
        let mut stacks = HashMap::new();
        if f[3] != "-" {
            for s in f[3].split(';') {
                let p: Vec<&str> = s.split(':').collect();
                stacks.insert(name(p[0]).unwrap(), stack(&p[1..]));
            }
        }
        let shooter: Vec<&str> = f[4].split(':').collect();
        stacks.insert('S', stack(&shooter));
        let mut cells = HashMap::new();
        if f[5] != "-" {
            for c in f[5].split(';') {
                let p: Vec<&str> = c.split(':').collect();
                let obstacle = if p[3] == "-" { None } else { Some(number(p[3])) };
                cells.insert((number(p[0]), number(p[1])), (number(p[2]) & 0x20 != 0, obstacle, name(p[4])));
            }
        }
        let world = World {
            rows: rows.collect(),
            gate: if f[2] == "-" { None } else { Some(f[2] == "1") },
            stacks,
            cells,
        };
        let (tx, ty) = f[6].split_once(':').unwrap();
        let center = world.stacks[&'S'].center;
        let (hit, verdict) = trace_line_of_fire(&world, 'S', center, (number(tx), number(ty)), f[7] == "1", name(f[8]));
        let expected_hit = name(f[10]);
        let expected = number(f[11]);
        let got = match verdict {
            LineOfFire::Clear => 0,
            LineOfFire::Obstructed => 1,
            LineOfFire::Blocked => 2,
        };
        assert_eq!((hit, got), (expected_hit, expected), "{line}");
        verdicts[got as usize] += 1;
        hits += i32::from(hit.is_some());
        total += 1;
    }
    // The file must keep exercising every verdict and real hits.
    assert!(total >= 1000, "{total} cases");
    assert!(verdicts.iter().all(|&n| n >= 30), "{verdicts:?}");
    assert!(hits >= 200, "{hits} hits");
}
