//! Render every terrain metatile to PNG.
//!
//! usage: cargo run --release --example dump_terrain -- <data dir> <out dir>

use h4_formats::archive::Resources;
use h4_formats::png;
use h4_formats::terrain::Metatile;

fn main() {
    let mut args = std::env::args().skip(1);
    let res = Resources::open(args.next().expect("data dir")).expect("open data");
    let out = args.next().expect("out dir");
    std::fs::create_dir_all(&out).unwrap();
    let mut names: Vec<&str> = res.names().filter(|n| n.to_lowercase().starts_with("terrain.")).collect();
    names.sort();
    for name in names {
        match res.read(name).and_then(|d| Metatile::read(&d)) {
            Ok(m) => {
                let (area, rgba) = m.to_rgba();
                let path = format!("{out}/{}.png", name.trim_end_matches(".h4d"));
                std::fs::write(&path, png::encode_rgba(area.width() as u32, area.height() as u32, &rgba)).unwrap();
                println!("ok   {name}");
            }
            Err(e) => println!("FAIL {name}: {e}"),
        }
    }
}
