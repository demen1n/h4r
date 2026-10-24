//! Write the unpacked part of every `.h4c` between the header and the tiles to files,
//! for looking at its bytes while the format is being worked out.
//! `index.tsv` lists the header fields the middle part depends on.
//!
//! usage: cargo run --release --example dump_map_middle -- <maps dir> <out dir>

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use h4_formats::map::MapFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out).expect("out dir");
    let mut files = Vec::new();
    collect(Path::new(&args[1]), &mut files);
    files.sort();
    let mut index = String::from("file\tversion\tplayers\tsize\tlevels\tmiddle\n");
    for (i, path) in files.iter().enumerate() {
        let bytes = std::fs::read(path).expect("read map");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let map = MapFile::read(&bytes, &name).unwrap_or_else(|e| panic!("{e}"));
        let file = format!("{i:03}.bin");
        std::fs::write(out.join(&file), &map.data[map.header_end..map.tiles_start]).expect("write");
        let h = &map.header;
        let colors: Vec<String> = h.players.iter().map(|p| p.color.to_string()).collect();
        writeln!(
            index,
            "{file}\t{}\t{}\t{}\t{}\t{}\t{name}",
            h.version,
            colors.join(","),
            h.size,
            h.levels,
            map.tiles_start - map.header_end
        )
        .unwrap();
    }
    std::fs::write(out.join("index.tsv"), index).expect("write index");
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("h4c")) {
            files.push(path);
        }
    }
}
