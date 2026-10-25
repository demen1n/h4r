//! Count random objects (types `random_*`) and random army stacks over all maps.
//!
//! usage: cargo run --release --example count_random_objects -- <dir with .h4c>...

use std::collections::BTreeMap;

use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("h4c")) {
            out.push(p);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        walk(std::path::Path::new(&a), &mut files);
    }
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut maps_with = 0;
    for f in &files {
        let map = MapFile::read(&std::fs::read(f).unwrap(), &f.to_string_lossy()).unwrap();
        let body = map.body.as_ref().unwrap();
        let mut seen = false;
        let mut bump = |k: String| {
            let e = counts.entry(k).or_default();
            e.0 += 1;
        };
        for o in &body.objects {
            match &o.kind {
                ObjectKind::Placed(p) if p.type_name.starts_with("random") => {
                    bump(format!("{} / {} / {}", p.type_name, p.major, p.minor));
                    seen = true;
                }
                ObjectKind::Army { army, .. } => {
                    for s in &army.stacks {
                        match s {
                            Stack::Creatures { kind, count, .. } if *kind < 0 || *count <= 0 => {
                                bump(format!("army stack kind<0:{} count<=0:{}", *kind < 0, *count <= 0));
                                seen = true;
                            }
                            Stack::Hero(h) if h.portrait < 0 => {
                                bump("army hero portrait -1".into());
                                seen = true;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        maps_with += seen as usize;
    }
    for (k, (n, _)) in &counts {
        println!("{n:6}  {k}");
    }
    println!("{} maps, {maps_with} with random things", files.len());
}
