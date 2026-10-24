//! Parse every adv_object resource of every archive (not only the visible ones) and report
//! the ones that fail or don't end exactly at the end of the file.
//!
//! usage: cargo run --release --example check_adv_objects -- [data dir] [-v]

use std::collections::BTreeMap;

use h4_formats::adv_object::ObjectModel;
use h4_formats::archive::{data_dir, Resources};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    let dir = data_dir(args.iter().find(|a| !a.starts_with('-')).cloned());
    let res = Resources::open(&dir).expect("open data");
    let (mut ok, mut total, mut failed) = (0, 0, Vec::new());
    // how many objects of each type, and a sample of field values, to sanity-check meanings
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut column_mismatch = Vec::new();
    let mut flat_modes: BTreeMap<u8, usize> = BTreeMap::new();
    for a in &res.archives {
        let file = a.path.file_name().unwrap().to_string_lossy().to_string();
        for e in &a.entries {
            if !e.name.to_lowercase().starts_with("adv_object.") || e.alias.is_some() {
                continue;
            }
            total += 1;
            match a.read(e).and_then(|d| ObjectModel::read(&d)) {
                Ok(m) => {
                    ok += 1;
                    *types.entry(m.object_type.name.clone()).or_default() += 1;
                    *flat_modes.entry(m.flat_mode).or_default() += 1;
                    // overlay images keep one depth per 32-pixel column of the frames (shadows
                    // left out), counted in anchor coordinates, i.e. after the offset
                    let frames = m
                        .image
                        .frames
                        .iter()
                        .chain(&m.image.base)
                        .filter(|l| !l.is_empty() && !l.name.to_lowercase().contains("shadow"))
                        .map(|l| l.rect)
                        .reduce(|a, b| a.union(&b));
                    if let (false, Some(b)) = (m.underlay, frames) {
                        let (left, right) = (b.left + m.image.offset.0, b.right + m.image.offset.0);
                        let cols = (right + 31).div_euclid(32) - left.div_euclid(32);
                        if cols != m.column_depths.len() as i32 {
                            column_mismatch.push(format!("{} {:?} {cols} vs {}", e.name, b, m.column_depths.len()));
                        }
                    }
                    if verbose {
                        println!(
                            "{file} {}: {}x{} {}/{}/{} flat {} flag {:?} underlay {} frames {} cols {}",
                            e.name,
                            m.width,
                            m.height,
                            m.object_type.name,
                            m.object_type.major,
                            m.object_type.minor,
                            m.flat_mode,
                            m.flag_offset,
                            m.underlay,
                            m.image.frames.len(),
                            m.column_depths.len()
                        );
                    }
                }
                Err(err) => failed.push(format!("{file} {}: {err}", e.name)),
            }
        }
    }
    println!("{ok}/{total} parsed, {} failed", failed.len());
    println!("flat modes: {flat_modes:?}");
    println!("{} overlays whose column count differs from the frames width / 32", column_mismatch.len());
    for c in column_mismatch.iter().take(5) {
        println!("  {c}");
    }
    println!("types: {types:?}");
    for f in failed.iter().take(20) {
        println!("  {f}");
    }
}
