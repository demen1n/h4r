//! Compare each sequence's stored offset with the middle-bottom of its frames.
//!
//! usage: cargo run --release --example actor_offsets -- <data dir> <actor> [actor...]

use h4_formats::archive::Resources;
use h4_formats::catalog;
use h4_formats::sprite::ActorSequence;

fn main() {
    let mut args = std::env::args().skip(1);
    let res = Resources::open(args.next().expect("data dir")).expect("open data");
    let actors = catalog::actors(&res);
    let wanted: Vec<String> = args.collect();
    if wanted.is_empty() {
        let heroes: Vec<&str> = actors.keys().filter(|k| k.starts_with("hero.")).map(String::as_str).collect();
        println!("heroes: {}", heroes.join(", "));
        return;
    }
    for w in wanted {
        let a = &actors[&w.to_lowercase()];
        println!("{}: {}", a.name, a.action_names().join(" "));
        for (action, dirs) in &a.actions {
            for (dir, name) in dirs.iter().take(2) {
                let s = ActorSequence::read(&res.read(name).unwrap()).unwrap();
                let b = s.frames.iter().map(|l| l.rect).reduce(|x, y| x.union(&y)).unwrap();
                println!(
                    "  {action:16} {dir:2} offset {:?}  frames x {}..{} y {}..{}  -> feet at ({}, {})",
                    s.offset, b.left, b.right, b.top, b.bottom,
                    (b.left + b.right) / 2 + s.offset.0, b.bottom + s.offset.1
                );
            }
        }
    }
}
