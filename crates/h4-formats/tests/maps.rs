//! Every `.h4c` map of the installation read from front to back: header, events, objects,
//! tiles. Skips itself without the game (see `common`).

mod common;

use std::path::{Path, PathBuf};

use h4_formats::map::{search_tiles_start, MapFile};
use h4_formats::map_body::ObjectKind;

/// Maps in `maps/` (86) and `maps/Contest Maps/` (20) of our installation.
const MAP_COUNT: usize = 106;

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

#[test]
fn maps_read_through_objects_to_tiles() {
    let Some(res) = common::resources() else { return };
    let dir = h4_formats::archive::data_dir(None).parent().expect("data dir has a parent").join("maps");
    let mut files = Vec::new();
    collect(&dir, &mut files);
    assert_eq!(files.len(), MAP_COUNT, "maps in {}", dir.display());

    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let map = match MapFile::read(&std::fs::read(path).unwrap(), &name) {
            Ok(m) => m,
            Err(e) => {
                failures.push(e.to_string());
                continue;
            }
        };
        let body = match &map.body {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        // Reading the objects must end exactly where the tiles were found from the end.
        match search_tiles_start(&map) {
            Ok(s) if s == map.tiles_start => {}
            other => failures.push(format!("{name}: objects end at {:#x}, search: {other:?}", map.tiles_start)),
        }
        // Meaning, not just length: objects lie on the map and their models exist.
        // Objects at the edge may stick out a little (seen: x or y from -2 to size), so allow
        // a margin; a misread position would be far off.
        let size = map.header.size as i32;
        let levels = map.header.levels as i32;
        let near_map = -8..size + 8;
        for o in &body.objects {
            if !near_map.contains(&o.x) || !near_map.contains(&o.y) || !(0..levels).contains(&o.level) {
                failures.push(format!("{name}: object at ({}, {}, level {}) outside the map", o.x, o.y, o.level));
            }
            if let ObjectKind::Placed(p) = &o.kind {
                if res.entry(&format!("adv_object.{}.h4d", p.model)).is_none() {
                    failures.push(format!("{name}: no model adv_object.{}", p.model));
                }
            }
        }
    }
    common::assert_no_failures("maps", &failures);
}
