//! Place the random objects of one map and print what they became.
//!
//! usage: cargo run --release -p h4-rules --example place_map -- <map.h4c> [seed] [difficulty 0..4]

use h4_formats::archive::{data_dir, Resources};
use h4_formats::map::MapFile;
use h4_formats::map_body::ObjectKind;
use h4_formats::table::Table;
use h4_rules::creatures::{self, TOWN_TYPES};
use h4_rules::placement::{place_random_objects, Placement, Setup, MATERIALS};
use h4_rules::random::GlobalRandom;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: place_map <map.h4c> [seed] [difficulty]");
    let seed = args.next().map_or(1, |s| s.parse().expect("seed"));
    let difficulty = args.next().map_or(1, |s| s.parse().expect("difficulty"));

    let res = Resources::open(&data_dir(None)).expect("game data");
    let table = Table::read(&res.read("table.creatures.h4d").expect("read"), "table.creatures").expect("parse");
    let by_id = creatures::in_enum_order(&creatures::from_table(&table).expect("stats")).expect("order");
    let map = MapFile::read(&std::fs::read(&path).expect("read map"), &path).expect("map");
    let setup = Setup::from_header(&map.header, difficulty);
    let placed = place_random_objects(&map, &by_id, &setup, &mut GlobalRandom::new(seed));

    let body = map.body.as_ref().expect("objects");
    for (o, result) in body.objects.iter().zip(&placed) {
        let Some(result) = result else { continue };
        let label = match &o.kind {
            ObjectKind::Placed(p) => format!("{} {}", p.type_name, p.major),
            ObjectKind::Army { owner, .. } => format!("army of {owner}"),
        };
        let what = match result {
            Placement::Town { town_type, model } => format!("{} town ({model})", TOWN_TYPES[*town_type]),
            Placement::Monster { stacks } => {
                let s: Vec<String> = stacks.iter().map(|&(c, n)| format!("{n} {}", by_id[c].keyword)).collect();
                s.join(" + ")
            }
            Placement::Dwelling { creature, model } => format!("{} dwelling ({model})", by_id[*creature].keyword),
            Placement::MaterialPile { material, amount, .. } => format!("{amount} {}", MATERIALS[*material]),
            Placement::WeeklyGenerator { material, amount, model } => format!("{amount} {} a week ({model})", MATERIALS[*material]),
            Placement::Shrine { model, .. } => model.clone(),
            Placement::Temple { model, .. } => model.clone(),
            Placement::Heroes(h) => {
                let s: Vec<String> = h.iter().map(|&(c, m)| h4_rules::heroes::adv_model_name(c as i8, m as i8).unwrap_or_default()).collect();
                s.join(", ")
            }
        };
        println!("({:3},{:3}) L{} {:28} {}", o.x, o.y, o.level, label, what);
    }
}
