//! Frame counts per action across all actors.
//!
//! usage: cargo run --release --example frame_counts -- <data dir> [action]

use std::collections::BTreeMap;

use h4_formats::archive::Resources;
use h4_formats::catalog;
use h4_formats::sprite::ActorSequence;

fn main() {
    let mut args = std::env::args().skip(1);
    let res = Resources::open(args.next().expect("data dir")).expect("open data");
    let only = args.next();
    let mut stats: BTreeMap<String, BTreeMap<usize, usize>> = BTreeMap::new();
    for (_, actor) in catalog::actors(&res) {
        for (action, dirs) in &actor.actions {
            if only.as_ref().is_some_and(|o| o != action) {
                continue;
            }
            let Some(name) = dirs.values().next() else { continue };
            let Ok(seq) = res.read(name).and_then(|d| ActorSequence::read(&d)) else { continue };
            *stats.entry(action.clone()).or_default().entry(seq.frames.len()).or_default() += 1;
            if only.is_some() {
                println!("{:24} {}", actor.name, seq.frames.len());
            }
        }
    }
    for (action, counts) in stats {
        let s: Vec<String> = counts.iter().map(|(f, n)| format!("{f}f x{n}")).collect();
        println!("{action:20} {}", s.join("  "));
    }
}
