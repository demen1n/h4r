//! Inputs of the obstacle placer bench: synthetic models, a synthetic weights
//! table and battlefield scenarios, all computed from fixed formulas so the
//! bench and `tests/obstacle_placer_vectors.rs` build exactly the same ones
//! without game data.
//!
//! Shared by `examples/obstacle_reference.rs` and `tests/obstacle_placer_vectors.rs`.
#![allow(dead_code)] // each user needs a different part

use h4_formats::combat_object::{CombatHeaderEntry, CombatObjectHeader, OBSTACLE_TYPE_NAMES};
use h4_formats::table::Table;
use h4_rules::battlefield::{BattlefieldGrid, CellSurface};
use h4_rules::obstacle_placement::PlacerContext;

/// A small integer hash (xorshift-multiply). Any fixed formula would do; this
/// one mixes well enough that neighbouring inputs look unrelated.
pub fn mix(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^ (v >> 16)
}

fn pick(a: u32, b: u32, c: u32) -> u32 {
    mix(a.wrapping_mul(0x9e37_79b9) ^ mix(b.wrapping_add(0x51ed_270b) ^ mix(c)))
}

/// 60 models: ordinary types 0..=33 with sizes 0..=4 × 1..=3, most of them
/// blocking, plus two castle types. Model 7 has width 0 (a corner case of the
/// weight formula).
pub fn synthetic_models() -> Vec<CombatHeaderEntry> {
    (0..60u32)
        .map(|i| {
            let r = pick(1, i, 0);
            let type_id = if i >= 58 {
                0x22 + (i - 58) as u16
            } else {
                (r % 34) as u16
            };
            let width = if i == 7 { 0 } else { 1 + (r >> 8) % 4 } as u8;
            let height = (1 + (r >> 12) % 3) as u8;
            CombatHeaderEntry {
                name: format!("synthetic.{i}"),
                header: CombatObjectHeader {
                    version: 2,
                    properties: [width, height],
                    parameter: 0,
                    frames_per_second: 10,
                    flag: 0,
                    attributes: [u8::from(!(r >> 16).is_multiple_of(5)), 0, 0, 0, 0],
                    type_id,
                    type_name: OBSTACLE_TYPE_NAMES[usize::from(type_id)].to_string(),
                },
            }
        })
        .collect()
}

/// A weights table in the installed layout, with the oddities the parser has
/// to handle: mixed case, a duplicate column, unknown names, a "River" terrain
/// row, empty and unknown frequency words, extra cells.
pub fn synthetic_table() -> Table {
    const WORDS: [&str; 8] = [
        "never", "Rare", "seldom", "COMMON", "usually", "", "often", "never",
    ];
    let word = |a, b, c| WORDS[(pick(a, b, c) % 8) as usize].to_string();
    // Columns: every ordinary type, some in upper case, one duplicated, two unknown.
    let mut columns: Vec<String> = (0..34)
        .map(|t| {
            let name = OBSTACLE_TYPE_NAMES[t];
            if t % 5 == 0 {
                name.to_uppercase()
            } else {
                name.to_string()
            }
        })
        .collect();
    columns.insert(10, "fungal tree".to_string());
    columns.push("snow logs".to_string());
    columns.push("shrub".to_string()); // duplicate: the later column wins
    let mut header = vec!["Obstacle Type".to_string(), String::new()];
    header.extend(columns.iter().cloned());
    let mut rows = vec![header];
    let width = columns.len() as u32;
    let mut row = |section: &str, name: &str, k: u32, full: bool| {
        let mut cells = vec![section.to_string(), name.to_string()];
        // Every 7th row has extra cells after the last column. (Shorter rows
        // are not possible here: the exe reads cells by column index without
        // checking the row length.)
        let count = if full || k % 7 != 3 { width } else { width + 5 };
        cells.extend((0..count).map(|c| word(2, k, c)));
        rows.push(cells);
    };
    let terrains = [
        "water",
        "Grass",
        "rough",
        "swamp",
        "volcanic",
        "snow",
        "sand",
        "dirt",
        "subterranean",
        "River",
        "lava",
        "nowhere",
    ];
    for (k, name) in terrains.iter().enumerate() {
        row(if k == 0 { "Terrain" } else { "" }, name, k as u32, true);
    }
    for t in 0..36u32 {
        let name = match t {
            34 => "burnt logs".to_string(),
            35 => "Snow_Log".to_string(),
            _ => OBSTACLE_TYPE_NAMES[t as usize].to_string(),
        };
        row(if t == 0 { "ADJACENT" } else { "" }, &name, 100 + t, false);
    }
    for t in 0..36u32 {
        let name = if t >= 34 {
            "swamp logs".to_string()
        } else {
            OBSTACLE_TYPE_NAMES[t as usize].to_string()
        };
        row(if t == 0 { "overlap" } else { "" }, &name, 200 + t, false);
    }
    Table { rows }
}

/// One field to place obstacles on.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    pub index: u32,
    pub seed: u32,
    pub context: PlacerContext,
}

/// `count` scenarios cycling through six kinds of fields (see `surface`).
pub fn scenarios(count: u32) -> Vec<Scenario> {
    (0..count)
        .map(|index| Scenario {
            index,
            seed: pick(3, index, 0) % 32768,
            context: PlacerContext {
                all_water: index % 6 == 5,
                on_bridge: index % 6 == 3,
                siege: None,
            },
        })
        .collect()
}

/// Terrain and flags of a cell: uniform fields, terrain patches with rivers,
/// holes without the obstacle flag, a bridge-like strip, extra flag bits,
/// diagonal bands.
pub fn surface(index: u32, x: i32, y: i32) -> (u8, u8) {
    const TERRAINS: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 0, 10, 11, 13];
    let base = TERRAINS[(index % 12) as usize];
    let noise = pick(4, index, (x as u32) << 8 | y as u32);
    match index % 6 {
        0 => (base, 0x10),
        1 => {
            let terrain = if (x / 9 + y / 7) % 3 != 0 {
                base
            } else {
                ((x * 7 + y) % 12) as u8
            };
            (terrain, 0x10)
        }
        2 => {
            let flags = if noise % 100 >= 15 { 0x10 } else { 0 };
            (base, flags | if (noise >> 8) % 100 < 5 { 4 } else { 0 })
        }
        3 => (1, if (x - y).abs() > 6 { 0x1c } else { 0 }),
        4 => (base, 0x18),
        _ => ((base as i32 + (x + y) / 20) as u8 % 9, 0x10),
    }
}

/// The field of a scenario, with the edges blocked like the game does.
pub fn field(scenario: &Scenario) -> BattlefieldGrid {
    let mut grid = BattlefieldGrid::standard(CellSurface::default());
    for x in 0..grid.row_count() as i32 {
        let (start, end) = grid.row_bounds(x).expect("x is a stored row");
        for y in start..end {
            let (terrain, flags) = surface(scenario.index, x, y);
            *grid.surface_mut(x, y).expect("inside the row") = CellSurface { terrain, flags };
        }
    }
    grid.block_edges();
    grid
}
