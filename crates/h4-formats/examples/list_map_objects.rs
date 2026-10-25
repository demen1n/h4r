//! List the objects of one `.h4c` map: position, type, model, owner and name.
//! Decorations (`decorative`) are only counted, they are most of the list; `--all` lists them too.
//!
//! usage: cargo run --release --example list_map_objects -- <map.h4c> [--all]

use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;

fn main() {
    let path = std::env::args().nth(1).expect("usage: list_map_objects <map.h4c>");
    let map = MapFile::read(&std::fs::read(&path).expect("read map"), &path).unwrap_or_else(|e| panic!("{e}"));
    let body = map.body.as_ref().unwrap_or_else(|e| panic!("objects: {e}"));
    let all = std::env::args().any(|a| a == "--all");
    let mut decorations = 0;
    for o in &body.objects {
        let at = format!("({:3},{:3}) L{}", o.x, o.y, o.level);
        match &o.kind {
            ObjectKind::Placed(p) if p.type_name == "decorative" && !all => decorations += 1,
            ObjectKind::Placed(p) => println!(
                "{at}  {:24} {:40} owner {:?} {}",
                p.type_name,
                p.model,
                p.owner,
                p.name.as_deref().unwrap_or("")
            ),
            ObjectKind::Army { owner, army, .. } => {
                let stacks: Vec<String> = army
                    .stacks
                    .iter()
                    .filter_map(|s| match s {
                        Stack::Empty => None,
                        Stack::Creatures { kind, count, .. } => Some(format!("{count}x#{kind}")),
                        Stack::Hero(h) => Some(format!("hero #{} class {} gender {} {}", h.portrait, h.class, h.gender, h.name)),
                    })
                    .collect();
                println!("{at}  army owner {owner}: {}", stacks.join(", "));
            }
        }
    }
    println!("{} objects, {decorations} of them decorations", body.objects.len());
}
