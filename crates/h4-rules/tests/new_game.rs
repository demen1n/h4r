//! Every installed map becomes a game state that makes sense. Skipped without the game.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use h4_formats::adv_object::ObjectModel;
use h4_formats::archive::{data_dir, Resources};
use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;
use h4_formats::table::Table;
use h4_rules::movement;
use h4_rules::new_game::{self, Footprint};
use h4_rules::placement::{place_random_objects, Setup};
use h4_rules::random::GlobalRandom;
use h4_rules::{artifacts, creatures, turns};

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
    let (mut paths, mut unreachable_monsters) = (0, 0);
    let (mut grown, mut neutrals) = (0_i64, 0_i64);
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
        // Every player's army has a day's movement in the formula's range, and can find
        // a way to the nearest monster on its level if there is one; each step of the way
        // touches the one before and can be entered.
        let mut state = state;
        turns::begin_game(&mut state, &stats);
        for a in state.armies.iter().filter(|a| a.owner.is_some()) {
            let full = movement::max_movement(&a.army, &stats);
            if !(1200..=3000).contains(&full) || movement::movement_left(a, &stats) != full {
                failures.push(format!("{name}: army {:?} has {full} movement", a.id));
            }
            let nearest = state
                .armies
                .iter()
                .filter(|m| m.owner.is_none() && m.at.level == a.at.level)
                .min_by_key(|m| (m.at.row - a.at.row).abs().max((m.at.col - a.at.col).abs()));
            let Some(monster) = nearest else { continue };
            let Some(path) = movement::find_path(&state, a.id, monster.at, &stats) else {
                unreachable_monsters += 1;
                continue;
            };
            paths += 1;
            let mut prev = a.at;
            for (n, &p) in path.iter().enumerate() {
                let touching = (p.row - prev.row).abs() <= 1 && (p.col - prev.col).abs() <= 1 && p != prev;
                let last = n + 1 == path.len();
                let ok = match movement::obstacle(&state, a, p) {
                    movement::Obstacle::None => !last || p == monster.at,
                    movement::Obstacle::Enemy(id) => last && id == monster.id,
                    _ => false,
                };
                if !touching || !ok {
                    failures.push(format!("{name}: bad step {n} {p:?} on the way of {:?} to {:?}", a.id, monster.at));
                    break;
                }
                prev = p;
            }
        }

        // a week of turns: everybody ends their turn seven times; neutral monsters grow,
        // nobody else does
        let before: i32 = state.armies.iter().filter(|a| a.owner.is_none()).flat_map(|a| a.army.stacks()).map(|s| s.count).sum();
        let owned: i32 = state.armies.iter().filter(|a| a.owner.is_some()).flat_map(|a| a.army.stacks()).map(|s| s.count).sum();
        let alive = state.players.iter().filter(|p| p.alive).count().max(1);
        for _ in 0..7 * alive {
            turns::end_turn(&mut state, &stats);
        }
        let after: i32 = state.armies.iter().filter(|a| a.owner.is_none()).flat_map(|a| a.army.stacks()).map(|s| s.count).sum();
        let owned_after: i32 = state.armies.iter().filter(|a| a.owner.is_some()).flat_map(|a| a.army.stacks()).map(|s| s.count).sum();
        if state.day != 8 || owned_after != owned || after < before {
            failures.push(format!("{name}: after a week day {}, neutrals {before} -> {after}, owned {owned} -> {owned_after}", state.day));
        }
        grown += (after - before) as i64;
        neutrals += before as i64;

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
    println!("{paths} paths to the nearest monster, {unreachable_monsters} monsters out of reach");
    println!("neutral creatures: {neutrals}, grew by {grown} in a week");
    assert!(armies > 5000, "only {armies} armies");
    // a few are the maps' own quirks, many would mean footprints are put down wrong
    assert!(on_objects.len() * 100 < armies, "{} armies on blocked tiles", on_objects.len());
    if !failures.is_empty() {
        failures.truncate(40);
        panic!("{} problems:\n  {}", failures.len(), failures.join("\n  "));
    }
}
