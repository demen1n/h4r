//! Write all frames of one animation side by side into a PNG, without opening a window.
//!
//! usage: h4-dump <actor> <action> <direction> <out.png> [data dir] [--combat]

use h4_formats::archive::Resources;
use h4_formats::combat_actor::CombatActorModel;
use h4_formats::sprite::ActorSequence;
use h4_formats::{catalog, png};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let combat = std::env::args().any(|a| a == "--combat");
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--combat").collect();
    if args.len() < 4 {
        eprintln!("usage: h4-dump <actor> <action> <direction> <out.png> [data dir] [--combat]");
        std::process::exit(2);
    }
    let res = Resources::open(h4_formats::archive::data_dir(args.get(4).cloned()))?;
    let mut actors = if combat {
        let model = CombatActorModel::read(&res.read(&format!("combat_actor.{}.h4d", args[0]))?)?;
        let actor = catalog::Actor::from_combat_model(args[0].clone(), &model);
        if let Some(action) = model.action(&args[1].to_lowercase()) {
            println!("{}: {} fps, parameter {}", action.name, action.frames_per_second, action.parameter);
        }
        std::collections::BTreeMap::from([(args[0].to_lowercase(), actor)])
    } else {
        catalog::actors(&res)
    };
    let actor = actors.remove(&args[0].to_lowercase()).ok_or("no such actor")?;
    let name = actor
        .actions
        .get(&args[1].to_lowercase())
        .and_then(|d| d.get(args[2].to_lowercase().as_str()))
        .ok_or("no such action/direction")?;

    let seq = ActorSequence::read(&res.read(name)?)?;
    let area = seq.bounds().ok_or("empty sequence")?;
    let (w, h, n) = (area.width() as usize, area.height() as usize, seq.frames.len());
    let mut sheet = vec![0u8; 4 * w * n * h];
    for i in 0..n {
        let frame = seq.compose(i, area, true);
        for y in 0..h {
            let dst = 4 * (y * w * n + i * w);
            sheet[dst..dst + 4 * w].copy_from_slice(&frame[4 * y * w..4 * (y + 1) * w]);
        }
    }
    std::fs::write(&args[3], png::encode_rgba((w * n) as u32, h as u32, &sheet))?;
    println!("{name}: {n} frames, {} shadows, {w}x{h} -> {}", seq.shadows.len(), args[3]);
    Ok(())
}
