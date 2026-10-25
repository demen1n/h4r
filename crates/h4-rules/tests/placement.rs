//! Random objects of every installed map get placed. Skipped when the game isn't installed.

use std::path::{Path, PathBuf};

use h4_formats::archive::{data_dir, Resources};
use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;
use h4_formats::table::Table;
use h4_rules::creatures;
use h4_rules::placement::{place_random_objects, Placement, Setup};
use h4_rules::random::GlobalRandom;

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

/// Types we place now; every object of these types must get a placement.
const HANDLED: &[&str] = &[
    "random_town", "random_monster", "random_random_monster", "random_creature_dwelling",
    "random_material_pile", "random_weekly_material_generator", "random_shrine", "random_temple",
];

#[test]
fn random_objects_of_all_maps() {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return;
    }
    let res = Resources::open(&dir).expect("open resources");
    let table = Table::read(&res.read("table.creatures.h4d").expect("read"), "table.creatures").expect("parse");
    let all = creatures::from_table(&table).expect("stats");
    let by_id = creatures::in_enum_order(&all).expect("enum order");

    let mut files = Vec::new();
    collect(&dir.parent().expect("parent").join("maps"), &mut files);
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    let mut monsters = 0;
    let mut heroes = 0;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let map = MapFile::read(&std::fs::read(path).unwrap(), &name).expect("map");
        let body = map.body.as_ref().expect("objects");
        let setup = Setup::from_header(&map.header, 1);
        let placed = place_random_objects(&map, &by_id, &setup, &mut GlobalRandom::new(12345));
        assert_eq!(placed.len(), body.objects.len());

        for (o, result) in body.objects.iter().zip(&placed) {
            if let ObjectKind::Army { army, .. } = &o.kind {
                let random = army.stacks.iter().filter(|s| matches!(s, Stack::Hero(h) if h.portrait < 0)).count();
                match result {
                    Some(Placement::Heroes(list)) => {
                        heroes += list.len();
                        for &(class, male) in list {
                            let model = h4_rules::heroes::adv_model_name(class as i8, male as i8).expect("base class");
                            if res.entry(&format!("adv_actor.{model}.h4d")).is_none() {
                                failures.push(format!("{name}: no hero model {model}"));
                            }
                        }
                    }
                    _ if random > 0 => failures.push(format!("{name}: army at ({}, {}): random hero not chosen", o.x, o.y)),
                    _ => {}
                }
            }
            let ObjectKind::Placed(p) = &o.kind else { continue };
            if !HANDLED.contains(&p.type_name.as_str()) {
                continue;
            }
            let Some(result) = result else {
                // A monster with the map maker's own strength range can roll less than any
                // creature of its level is worth; the game then places nothing either
                // ("Who First": level 4, 1..700 peasants).
                if p.monster_range.is_some() {
                    continue;
                }
                failures.push(format!("{name}: {} {}/{} at ({}, {}) not placed", p.type_name, p.major, p.minor, o.x, o.y));
                continue;
            };
            if let Some(model) = result.model() {
                if res.entry(&format!("adv_object.{model}.h4d")).is_none() {
                    failures.push(format!("{name}: no model {model:?} for {}", p.type_name));
                }
            }
            if let Placement::Monster { stacks } = result {
                monsters += 1;
                let main = &by_id[stacks[0].0];
                // `random_monster` of level N gives a level N creature
                if let Some(l) = p.major.strip_prefix("level_").and_then(|l| l.parse::<i32>().ok()) {
                    if main.level != l {
                        failures.push(format!("{name}: {} for {} has level {}", main.keyword, p.major, main.level));
                    }
                }
                if stacks.iter().any(|&(_, n)| n <= 0) {
                    failures.push(format!("{name}: monster {stacks:?}"));
                }
            }
        }
    }
    assert!(monsters > 5000, "only {monsters} monsters placed");
    assert!(heroes > 300, "only {heroes} heroes with a look");
    if !failures.is_empty() {
        failures.truncate(40);
        panic!("{} problems:\n  {}", failures.len(), failures.join("\n  "));
    }
}

/// Every base class in both genders has its hero model in the archives.
#[test]
fn hero_models_exist() {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return;
    }
    let res = Resources::open(&dir).expect("open resources");
    for class in 0..11 {
        for gender in 0..2 {
            let name = h4_rules::heroes::adv_model_name(class, gender).expect("base class");
            assert!(res.entry(&format!("adv_actor.{name}.h4d")).is_some(), "no adv_actor.{name}");
        }
    }
}
