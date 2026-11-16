//! A battlefield read from a line of the line-of-fire / shot-penalty vector files.
#![allow(dead_code)] // each test file uses part of it

use std::collections::HashMap;

use h4_rules::battle::shooting::{footprint_center, FireCell, FireField, FireStack};

/// A stack name from the file: "S" is the shooter.
pub type Name = char;

/// What a cell holds: obstacle bit, obstacle type, stack.
type CellData = (bool, Option<i32>, Option<Name>);

pub struct World {
    pub rows: Vec<(i32, i32)>,
    pub gate: Option<bool>,
    pub stacks: HashMap<Name, FireStack>,
    pub cells: HashMap<(i32, i32), CellData>,
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

pub fn number(s: &str) -> i32 {
    s.parse().unwrap()
}

pub fn effects(s: &str) -> Vec<u16> {
    if s == "-" { Vec::new() } else { s.split(',').map(|e| e.parse().unwrap()).collect() }
}

pub fn name(s: &str) -> Option<Name> {
    if s == "-" { None } else { s.chars().next() }
}

pub fn stack(f: &[&str]) -> FireStack {
    // side flag effects size px py
    FireStack {
        side: f[0] == "1",
        flag_9bc: f[1] == "1",
        effects: effects(f[2]),
        center: footprint_center((number(f[4]), number(f[5])), number(f[3])),
    }
}


/// Read the eight world fields that both vector files start with:
/// rows gate stacks shooter cells target exact arg5.
pub fn parse_world(f: &[&str]) -> World {
    let rows = f[0].split(';').map(|r| {
        let (a, b) = r.split_once(':').unwrap();
        (number(a), number(b))
    });
    let mut stacks = HashMap::new();
    if f[2] != "-" {
        for s in f[2].split(';') {
            let p: Vec<&str> = s.split(':').collect();
            stacks.insert(name(p[0]).unwrap(), stack(&p[1..]));
        }
    }
    let shooter: Vec<&str> = f[3].split(':').collect();
    stacks.insert('S', stack(&shooter));
    let mut cells = HashMap::new();
    if f[4] != "-" {
        for c in f[4].split(';') {
            let p: Vec<&str> = c.split(':').collect();
            let obstacle = if p[3] == "-" { None } else { Some(number(p[3])) };
            cells.insert((number(p[0]), number(p[1])), (number(p[2]) & 0x20 != 0, obstacle, name(p[4])));
        }
    }
    World { rows: rows.collect(), gate: if f[1] == "-" { None } else { Some(f[1] == "1") }, stacks, cells }
}
