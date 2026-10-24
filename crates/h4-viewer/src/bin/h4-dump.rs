//! Write all frames of one animation side by side into a PNG, without opening a window.
//!
//! usage: h4-dump <actor> <action> <direction> <out.png> [data dir]

use h4_formats::archive::Resources;
use h4_formats::sprite::ActorSequence;
use h4_formats::{catalog, png};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: h4-dump <actor> <action> <direction> <out.png> [data dir]");
        std::process::exit(2);
    }
    let res = Resources::open(h4_formats::archive::data_dir(args.get(4).cloned()))?;
    let actors = catalog::actors(&res);
    let actor = actors.get(&args[0].to_lowercase()).ok_or("no such actor")?;
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
