//! Read every `.h4c` map of the installation: header, events and objects, tiles and heights.
//! Prints one line per map, then how many maps were read through to the tiles, which
//! object types stopped the others, whether the tiles start where the search from the end
//! puts them, and a summary of value ranges seen in the tiles.
//!
//! usage: cargo run --release --example check_maps -- [maps dir]
//! (default: the `maps` folder next to the game's data folder)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use h4_formats::archive::data_dir;
use h4_formats::map::{search_tiles_start, MapFile, TERRAIN_NAMES};
use h4_formats::map_body::ObjectKind;

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
    // events and objects
    let mut sequential = 0;
    let mut offset_mismatch = Vec::new();
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut object_types: BTreeMap<String, usize> = BTreeMap::new();
    let (mut objects, mut armies, mut events) = (0, 0, 0);
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
        match &map.body {
            Ok(body) => {
                sequential += 1;
                // the sequential read must agree with the search from the end
                match search_tiles_start(&map) {
                    Ok(s) if s == map.tiles_start => {}
                    other => offset_mismatch.push(format!("{name}: objects end at {:#x}, search says {other:?}", map.tiles_start)),
                }
                let e = &body.events;
                events += e.timed.len() + e.triggerable.len() + e.continuous.len() + e.placed.len();
                for o in &body.objects {
                    objects += 1;
                    match &o.kind {
                        ObjectKind::Army { .. } => armies += 1,
                        ObjectKind::Placed(p) => *object_types.entry(p.type_name.clone()).or_default() += 1,
                    }
                }
            }
            Err(e) => {
                // "... object type \"x\" ..." or another message: count by message start
                let key = e.split(" at 0x").next().unwrap_or(e).to_string();
                *blockers.entry(key).or_default() += 1;
            }
        }

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
    println!("{sequential}/{ok} read through events and objects to the tiles");
    println!("  {events} global events, {objects} objects ({armies} armies on the map)");
    if offset_mismatch.is_empty() {
        println!("  tiles start where the search from the end finds them on every one of them");
    }
    for m in &offset_mismatch {
        println!("  MISMATCH {m}");
    }
    if !blockers.is_empty() {
        println!("stopped by:");
        let mut list: Vec<_> = blockers.iter().collect();
        list.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (what, n) in list {
            println!("  {n:3}  {what}");
        }
    }
    let types: Vec<String> = object_types.iter().map(|(t, n)| format!("{t} {n}")).collect();
    println!("object types: {}", types.join(", "));
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
