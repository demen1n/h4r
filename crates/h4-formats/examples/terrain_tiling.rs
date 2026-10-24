//! Experiment: how do ragged metatiles tile? Draws a patch of one terrain with metatiles
//! placed on a grid of `step` tiles, variant chosen by grid parity, and reports holes.
//!
//! usage: cargo run --release --example terrain_tiling -- <data dir> <terrain> <step> <out.png>

use h4_formats::archive::Resources;
use h4_formats::png;
use h4_formats::terrain::{Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let res = Resources::open(&a[0]).expect("open data");
    let step: usize = a[2].parse().unwrap();
    let variants: Vec<Vec<Metatile>> = (1..=2)
        .map(|i| {
            (1..=2)
                .map(|j| Metatile::read(&res.read(&format!("terrain.{}.{i}.{j}.h4d", a[1])).unwrap()).unwrap())
                .collect()
        })
        .collect();

    // map of N x N tiles; screen position of tile (row, col)
    let n = 30usize;
    let (hw, hh) = (TILE_WIDTH as i32 / 2, TILE_HEIGHT as i32 / 2);
    let width = (2 * n as i32 + 2) * hw;
    let height = (2 * n as i32 + 2) * hh;
    let origin_x = n as i32 * hw;
    let mut canvas = vec![0u8; 4 * (width * height) as usize];

    // metatiles anchored every `step` tiles, starting before the map so edges are covered too
    let start = -(METATILE_SIZE as i32);
    let mut row0 = start;
    while row0 < n as i32 {
        let mut col0 = start;
        while col0 < n as i32 {
            let gi = (row0.div_euclid(step as i32)).rem_euclid(2) as usize;
            let gj = (col0.div_euclid(step as i32)).rem_euclid(2) as usize;
            // first index: texture set (look); second: alternate metatiles in a checkerboard
            let set: usize = std::env::var("SET").map_or(0, |s| s.parse().unwrap());
            let m = &variants[set][(gi + gj) % 2];
            for r in 0..METATILE_SIZE {
                for c in 0..METATILE_SIZE {
                    let (mr, mc) = (row0 + r as i32, col0 + c as i32);
                    if mr < 0 || mc < 0 || mr >= n as i32 || mc >= n as i32 {
                        continue;
                    }
                    let x = origin_x + (mc - mr) * hw;
                    let y = (mr + mc) * hh;
                    m.draw_tile(m.tile(r, c), &mut canvas, width as usize, x, y);
                }
            }
            col0 += step as i32;
        }
        row0 += step as i32;
    }

    // count transparent pixels inside the map's diamond
    let mut holes = 0;
    let mut total = 0;
    for y in 0..height {
        for x in 0..width {
            // inside the big diamond?
            let dx = (x - origin_x - hw).abs() as f32 / (n as f32 * hw as f32);
            let dy = (y - n as i32 * hh).abs() as f32 / (n as f32 * hh as f32);
            if dx + dy < 0.95 {
                total += 1;
                if canvas[4 * (y * width + x) as usize + 3] == 0 {
                    holes += 1;
                }
            }
        }
    }
    std::fs::write(&a[3], png::encode_rgba(width as u32, height as u32, &canvas)).unwrap();
    println!("step {step}: {holes} transparent of {total} pixels ({:.2}%)", 100.0 * holes as f32 / total as f32);
}
