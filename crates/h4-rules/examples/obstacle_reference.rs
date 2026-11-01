//! Our side of the obstacle placer bench (`tools/testbench/obstacle_placer.py`).
//!
//! `obstacle_reference <data> <inputs> <mode> <count>`
//!
//! - `inputs`: `synthetic` (formulas in `support/obstacle_case.rs`, no game
//!   data needed) or `installed` (headers and `table.combat_obstacles` of the
//!   game in `<data>`; empty `<data>` means the usual search).
//! - `dump`: what the bench gives the original code — models (`w h type
//!   blocks`), our parse of the weights table, the table's cells, then every
//!   scenario with its cells.
//! - `place`: our placements, one line per scenario: `placed` and `model x y` triples.
#[path = "support/obstacle_case.rs"]
mod obstacle_case;

use std::io::{self, Write};

use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_object::{CombatHeaderEntry, CombatHeaderTable};
use h4_formats::table::Table;
use h4_rules::obstacle_placement::{place_obstacles, ObstacleWeights};
use h4_rules::random::CombatRandom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, inputs, mode, count] = args.as_slice() else {
        return Err(
            "usage: obstacle_reference <data> <synthetic|installed> <dump|place> <count>".into(),
        );
    };
    let (models, table): (Vec<CombatHeaderEntry>, Table) = if inputs == "installed" {
        let res = Resources::open(data_dir(Some(data.clone()).filter(|s| !s.is_empty())))?;
        let headers = CombatHeaderTable::read(&res.read(CombatHeaderTable::RESOURCE)?)?;
        let table = Table::read(&res.read("table.combat_obstacles.h4d")?, "combat_obstacles")?;
        (headers.entries, table)
    } else {
        (
            obstacle_case::synthetic_models(),
            obstacle_case::synthetic_table(),
        )
    };
    let weights = ObstacleWeights::from_table(&table);
    let scenarios = obstacle_case::scenarios(count.parse()?);
    let mut out = io::BufWriter::new(io::stdout().lock());
    if mode == "dump" {
        writeln!(out, "models {}", models.len())?;
        for e in &models {
            let h = &e.header;
            let [w, h_] = h.properties;
            writeln!(
                out,
                "{w} {h_} {} {}",
                h.resolved_type(),
                u8::from(h.blocks())
            )?;
        }
        let layout: Vec<String> = weights.exe_layout().iter().map(i32::to_string).collect();
        writeln!(out, "weights {}", layout.join(" "))?;
        // The table itself, so the bench can run the original parser on it.
        // The cells are ASCII; tabs separate them.
        writeln!(out, "rows {}", table.rows.len())?;
        for row in &table.rows {
            writeln!(out, "row\t{}", row.join("\t"))?;
        }
        for s in &scenarios {
            let c = s.context;
            writeln!(
                out,
                "scenario {} {} {}",
                s.seed,
                u8::from(c.all_water),
                u8::from(c.on_bridge)
            )?;
            let grid = obstacle_case::field(s);
            for x in 0..grid.row_count() as i32 {
                let (start, end) = grid.row_bounds(x).expect("x is a stored row");
                for y in start..end {
                    // Before edge blocking: the bench runs the original 162cb0 itself.
                    let (terrain, flags) = obstacle_case::surface(s.index, x, y);
                    writeln!(out, "cell {x} {y} {terrain} {flags}")?;
                }
            }
        }
        return Ok(());
    }
    for s in &scenarios {
        let grid = obstacle_case::field(s);
        let mut random = CombatRandom::new(s.seed, 1);
        let placed = place_obstacles(&grid, &models, &weights, &s.context, &mut random);
        let parts: Vec<String> = placed
            .iter()
            .map(|p| format!("{} {} {}", p.model, p.at.0, p.at.1))
            .collect();
        writeln!(out, "placed {}", parts.join(" "))?;
    }
    Ok(())
}
