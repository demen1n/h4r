//! Parse every actor_sequence resource and report the ones that fail.
//!
//! usage: cargo run --release --example check_sequences -- <data dir>

use h4_formats::archive::Resources;
use h4_formats::sprite::ActorSequence;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: check_sequences <data dir>");
    let res = Resources::open(&dir).expect("open data");
    let mut names: Vec<&str> = res.names().filter(|n| n.to_lowercase().starts_with("actor_sequence.")).collect();
    names.sort();
    let (mut ok, mut failed, mut no_shadow) = (0, Vec::new(), 0);
    for name in &names {
        match res.read(name).and_then(|d| ActorSequence::read(&d)) {
            Ok(s) => {
                ok += 1;
                if s.shadows.is_empty() {
                    no_shadow += 1;
                }
            }
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    println!("{ok}/{} parsed, {no_shadow} without shadows, {} failed", names.len(), failed.len());
    for f in failed.iter().take(15) {
        println!("  {f}");
    }
}
