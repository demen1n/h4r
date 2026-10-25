//! Every installed map becomes a game state that makes sense. Skipped without the game.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use h4_formats::adv_object::ObjectModel;
use h4_formats::archive::{data_dir, Resources};
use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;
use h4_formats::table::Table;
use h4_rules::new_game::{self, Footprint};
use h4_rules::placement::{place_random_objects, Setup};
use h4_rules::random::GlobalRandom;
use h4_rules::{artifacts, creatures};

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
fn states_of_all_maps() {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return;
    }
    let res = Resources::open(&dir).expect("open resources");
    let table = |name: &str| Table::read(&res.read(&format!("table.{name}.h4d")).unwrap(), name).unwrap();
    let stats = creatures::in_enum_order(&creatures::from_table(&table("creatures")).unwrap()).unwrap();
    let arts = artifacts::from_table(&table("Artifacts")).unwrap();

    // models are shared by many maps: read each once
    let mut models: HashMap<String, Option<(i32, i32, Vec<bool>)>> = HashMap::new();
    let mut files = Vec::new();
    collect(&dir.parent().unwrap().join("maps"), &mut files);
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    let (mut armies, mut blocked_tiles) = (0, 0);
    let mut on_objects = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let map = MapFile::read(&std::fs::read(path).unwrap(), &name).unwrap();
        let placements = place_random_objects(&map, &stats, &arts, &Setup::from_header(&map.header, 1), &mut GlobalRandom::new(1));
        let state = new_game::from_map(&map, &placements, |model| {
            let entry = models.entry(model.to_string()).or_insert_with(|| {
                let data = res.read(&format!("adv_object.{model}.h4d")).ok()?;
                let m = ObjectModel::read(&data).ok()?;
                Some((m.width as i32, m.height as i32, m.impassable))
            });
            entry.as_ref().map(|(w, h, imp)| Footprint { width: *w, height: *h, impassable: imp.clone() })
        });

        // Every army of the file is there, plus the random monsters that got creatures.
        // 108 armies on the installed maps are empty (seven empty slots, neutral, no events);
        // they are left out. What the game does with them is still to be checked.
        let body = map.body.as_ref().unwrap();
        let stored = body
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Army { army, .. } if army.stacks.iter().any(|s| !matches!(s, Stack::Empty))))
            .count();
        let monsters = placements.iter().flatten().filter(|p| matches!(p, h4_rules::placement::Placement::Monster { .. })).count();
        if state.armies.len() != stored + monsters {
            failures.push(format!("{name}: {} armies, the file has {stored} + {monsters} random", state.armies.len()));
        }
        armies += state.armies.len();
        assert_eq!(state.players.len(), map.header.players.len());

        for a in &state.armies {
            match state.map.tile(a.at) {
                None => failures.push(format!("{name}: army {:?} off the map at {:?}", a.id, a.at)),
                Some(t) if !t.inside => failures.push(format!("{name}: army {:?} outside the diamond at {:?}", a.id, a.at)),
                // Some map makers put a monster right on a skeleton, a rock or a dwelling's
                // entrance: 54 armies of the installation (19 stored, the rest random
                // monsters). Counted, not failed.
                Some(t) if t.blocked => on_objects.push(format!("{name}: {:?}", a.at)),
                Some(_) => {}
            }
            if a.army.is_empty() {
                failures.push(format!("{name}: empty army {:?}", a.id));
            }
            if let Some(s) = a.army.stacks().find(|s| s.creature >= stats.len() || s.count <= 0) {
                failures.push(format!("{name}: bad stack {s:?}"));
            }
        }
        for level in 0..state.map.levels {
            for row in 0..state.map.size {
                for col in 0..state.map.size {
                    let t = state.map.tile(h4_state::Pos { level, row, col }).unwrap();
                    blocked_tiles += t.blocked as usize;
                }
            }
        }
    }
    println!("{} maps, {armies} armies, {blocked_tiles} blocked tiles", files.len());
    println!("{} armies stand on a blocking object: {on_objects:?}", on_objects.len());
    assert!(armies > 5000, "only {armies} armies");
    // a few are the maps' own quirks, many would mean footprints are put down wrong
    assert!(on_objects.len() * 100 < armies, "{} armies on blocked tiles", on_objects.len());
    if !failures.is_empty() {
        failures.truncate(40);
        panic!("{} problems:\n  {}", failures.len(), failures.join("\n  "));
    }
}
