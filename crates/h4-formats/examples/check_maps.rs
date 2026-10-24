//! Read every `.h4c` map of the installation: header, then tiles and heights.
//! Prints one line per map and a summary of value ranges seen in the tiles.
//!
//! usage: cargo run --release --example check_maps -- [maps dir]
//! (default: the `maps` folder next to the game's data folder)

use std::path::{Path, PathBuf};

use h4_formats::archive::data_dir;
use h4_formats::map::{MapFile, TERRAIN_NAMES};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir(None).parent().expect("data dir has a parent").join("maps"));
    let mut files = Vec::new();
    collect(&dir, &mut files);
    files.sort();

    let (mut ok, mut failed) = (0, Vec::new());
    // value ranges over all maps, to compare with what the format allows
    let mut max_transitions = 0;
    let mut max_mask_set = 0;
    let mut max_id = 0;
    let mut roads = [0usize; 4];
    let mut rock = [0usize; 3];
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(path).expect("read map");
        let map = match MapFile::read(&bytes, &name) {
            Ok(m) => m,
            Err(e) => {
                failed.push(e.to_string());
                continue;
            }
        };
        ok += 1;
        let h = &map.header;

        // terrain of each level, most common first
        let per_level = map.tiles.len() / h.levels as usize;
        let mut summary = Vec::new();
        for level in map.tiles.chunks(per_level) {
            let mut counts = [0usize; TERRAIN_NAMES.len()];
            for t in level {
                counts[t.terrain as usize] += 1;
            }
            let mut order: Vec<usize> = (0..counts.len()).filter(|&i| counts[i] > 0).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(counts[i]));
            let parts: Vec<String> =
                order.iter().take(3).map(|&i| format!("{} {}%", TERRAIN_NAMES[i], counts[i] * 100 / level.len())).collect();
            summary.push(parts.join(", "));
        }
        for t in &map.tiles {
            roads[t.road as usize] += 1;
            rock[t.rock_terrain as usize] += 1;
            max_transitions = max_transitions.max(t.terrain_transitions.len());
            for tr in &t.terrain_transitions {
                max_mask_set = max_mask_set.max(tr.mask_set);
                max_id = max_id.max(tr.id);
            }
        }
        println!(
            "{name:40} v{} {}x{} lv{} players {} diff {} | header {:#x}, objects {:#x}..{:#x} ({} KB), tiles {} | {} | {}",
            h.version,
            h.size,
            h.size,
            h.levels,
            h.players.len(),
            h.difficulty,
            map.header_end,
            map.header_end,
            map.tiles_start,
            (map.tiles_start - map.header_end) / 1024,
            map.tiles.len(),
            h.name,
            summary.join(" / ")
        );
    }
    println!("\n{ok}/{} maps read, {} failed", files.len(), failed.len());
    for f in &failed {
        println!("  {f}");
    }
    println!("roads by type {roads:?}, rock terrain {rock:?}");
    println!("terrain transitions: up to {max_transitions} per tile, mask set <= {max_mask_set}, id <= {max_id}");
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read maps dir").flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("h4c")) {
            out.push(p);
        }
    }
}
