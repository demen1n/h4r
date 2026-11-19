//! Save every `bitmap_raw.*` picture (main menu backgrounds) as PNG.
//!
//! `cargo run --release --example dump_bitmap_raw -- [data dir] [out dir]`
//! Only the visible resources are dumped: an archive earlier in the load order hides the
//! same name in later ones.

use h4_formats::archive::{data_dir, Resources};
use h4_formats::bitmap_raw::BitmapRaw;
use h4_formats::png;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let resources = Resources::open(data_dir(args.next()))?;
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| "bitmap_raw".into()));
    std::fs::create_dir_all(&out)?;

    let mut names: Vec<_> = resources.names().filter(|n| n.to_lowercase().starts_with("bitmap_raw.")).collect();
    names.sort();
    for name in &names {
        let picture = BitmapRaw::read(&resources.read(name)?, name)?;
        // Names come from the archive: drop path separators so a file can't land outside `out`.
        let file = name.replace(['/', '\\'], "_");
        let path = out.join(format!("{file}.png"));
        std::fs::write(&path, png::encode_rgba(picture.width, picture.height, &picture.to_rgba()))?;
        println!("{name}: {}x{} -> {}", picture.width, picture.height, path.display());
    }
    Ok(())
}
