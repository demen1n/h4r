//! Walk lengths and walk animation timing of map actors, to compare with recordings of
//! the game.
//!
//! usage: cargo run --release --example walk_speeds -- <data dir> [name prefix]

use h4_formats::adv_actor::AdvActorModel;
use h4_formats::archive::Resources;
use h4_formats::sprite::ActorSequence;

fn main() {
    let mut args = std::env::args().skip(1);
    let res = Resources::open(args.next().expect("data dir")).expect("open data");
    let prefix = args.next().unwrap_or_default();
    let mut names: Vec<&str> = res
        .names()
        .filter_map(|n| n.strip_prefix("adv_actor.")?.strip_suffix(".h4d"))
        .filter(|n| n.starts_with(&prefix))
        .collect();
    names.sort();
    println!("{:32} {:>4} {:>4} {:>4}  action    fps frames  seconds  length/s", "actor", "pre", "walk", "post");
    for name in names {
        let model = AdvActorModel::read(&res.read(&format!("adv_actor.{name}.h4d")).unwrap()).unwrap();
        for action in ["prewalk", "walk", "postwalk"] {
            let Some(a) = model.action(action) else { continue };
            let Some(seq) = model.sequence(action, "se") else { continue };
            let frames = ActorSequence::read(&res.read(&seq).unwrap()).unwrap().frames.len();
            let seconds = frames as f32 / a.frames_per_second.max(1) as f32;
            let length = match action {
                "prewalk" => model.prewalk_length,
                "postwalk" => model.postwalk_length,
                _ => model.walk_length,
            };
            println!(
                "{name:32} {:>4} {:>4} {:>4}  {action:9} {:>3} {:>6} {seconds:8.3} {:9.1}",
                model.prewalk_length,
                model.walk_length,
                model.postwalk_length,
                a.frames_per_second,
                frames,
                length as f32 / seconds
            );
        }
    }
}
