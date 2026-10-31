//! Readers must turn broken input into an `Err`, never a panic or an out-of-bounds read.
//!
//! These tests need no game data: each builds a small valid file byte by byte, checks that it
//! reads, and then feeds the readers truncated and corrupted versions of it. Game files can
//! be damaged (a half-downloaded mod, a bad disk), and a crash with no message is the worst
//! way to find out.

use std::io::Write;

use flate2::write::GzEncoder;
use flate2::Compression;
use h4_formats::adv_actor::AdvActorModel;
use h4_formats::archive::{read_directory, Archive, Resources};
use h4_formats::sprite::{read_bitmap_group, ActorSequence, Rect};
use h4_formats::terrain::{Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};
use h4_formats::transition::Transitions;

// ---------------------------------------------------------------------------------------
// Little helpers to write the same little-endian fields the readers expect.

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn i32le(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn str16(out: &mut Vec<u8>, s: &str) {
    u16le(out, s.len() as u16);
    out.extend_from_slice(s.as_bytes());
}

/// Every proper prefix of `data` must fail to parse (and must not panic).
fn assert_truncations_fail<T>(what: &str, data: &[u8], parse: impl Fn(&[u8]) -> h4_formats::Result<T>) {
    for len in 0..data.len() {
        assert!(parse(&data[..len]).is_err(), "{what} truncated to {len} of {} bytes parsed", data.len());
    }
}

// ---------------------------------------------------------------------------------------
// Sprite layers

/// One row of a layer: pixels x0..x1 (relative to left) stored at `offset` in the pixel data.
struct Row {
    x0: u16,
    x1: u16,
    offset: u32,
}

/// A layer with a two-colour palette (red, blue) and 4-bit alpha.
fn layer_bytes(rect: Rect, rows: &[Row], pixels: &[u8], alpha: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    u16le(&mut out, 2); // palette size
    u16le(&mut out, 0); // no block alpha
    out.extend_from_slice(&[0, 0, 255, 255, 0, 0]); // B, G, R: red, then blue
    str16(&mut out, "frame 001");
    out.push(4); // alpha depth
    for v in [rect.left, rect.top, rect.right, rect.bottom] {
        i32le(&mut out, v);
    }
    for row in rows {
        u16le(&mut out, row.x0);
        u16le(&mut out, row.x1);
        u32le(&mut out, row.offset);
    }
    out.extend_from_slice(pixels);
    out.extend_from_slice(alpha);
    out
}

/// A 2x2 layer at (10, 20): red/blue on the first row, blue/red on the second, all opaque.
fn small_layer() -> Vec<u8> {
    let rect = Rect { left: 10, top: 20, right: 12, bottom: 22 };
    let rows = [Row { x0: 0, x1: 2, offset: 0 }, Row { x0: 0, x1: 2, offset: 2 }];
    layer_bytes(rect, &rows, &[0, 1, 1, 0], &[0xff, 0xff])
}

fn group(layers: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    u16le(&mut out, layers.len() as u16);
    for l in layers {
        out.extend_from_slice(l);
    }
    out
}

#[test]
fn valid_layer_decodes() {
    let layers = read_bitmap_group(&group(&[small_layer()])).expect("valid group");
    assert_eq!(layers.len(), 1);
    let l = &layers[0];
    assert_eq!(l.name, "frame 001");
    assert_eq!(l.rect, Rect { left: 10, top: 20, right: 12, bottom: 22 });
    let rgba = l.to_rgba();
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    assert_eq!(rgba, [red, blue, blue, red].concat());
}

#[test]
fn truncated_layer_fails() {
    assert_truncations_fail("bitmap group", &group(&[small_layer()]), read_bitmap_group);
}

#[test]
fn layer_with_huge_rect_fails() {
    // Coordinates this far apart would overflow `right - left`; the reader must say no first.
    let rect = Rect { left: i32::MIN, top: 0, right: i32::MAX, bottom: 1 };
    let data = group(&[layer_bytes(rect, &[], &[], &[])]);
    assert!(read_bitmap_group(&data).is_err());
    let rect = Rect { left: 0, top: i32::MIN, right: 1, bottom: i32::MAX };
    assert!(read_bitmap_group(&group(&[layer_bytes(rect, &[], &[], &[])])).is_err());
}

#[test]
fn layer_row_past_pixel_data_fails() {
    // The first row claims 100 pixels at offset 0, but the pixel data (sized by the last
    // row) is only 2 bytes long. Drawing this would read far past the data.
    let rect = Rect { left: 0, top: 0, right: 100, bottom: 2 };
    let rows = [Row { x0: 0, x1: 100, offset: 0 }, Row { x0: 0, x1: 2, offset: 0 }];
    let data = group(&[layer_bytes(rect, &rows, &[0, 0], &[0xff])]);
    assert!(read_bitmap_group(&data).is_err());
}

#[test]
fn drawing_onto_a_mismatched_canvas_does_not_panic() {
    let layers = read_bitmap_group(&group(&[small_layer()])).unwrap();
    let l = &layers[0];
    // canvas smaller than the area it claims to cover
    let mut small = vec![0u8; 4];
    l.draw_onto(&mut small, l.rect);
    // empty and inverted areas
    l.draw_onto(&mut [], Rect { left: 0, top: 0, right: 0, bottom: 0 });
    l.draw_onto(&mut [], Rect { left: 5, top: 5, right: 0, bottom: 0 });
    // an area far away, where positions could overflow
    let mut buf = vec![0u8; 16];
    l.draw_onto(&mut buf, Rect { left: i32::MIN, top: i32::MIN, right: i32::MIN + 2, bottom: i32::MIN + 2 });
    assert!(buf.iter().all(|&b| b == 0), "nothing should land in a far-away area");
}

// ---------------------------------------------------------------------------------------
// Actor sequences

fn actor_sequence(version: u16) -> Vec<u8> {
    let mut out = Vec::new();
    u16le(&mut out, 2);
    u16le(&mut out, version);
    if version == 3 {
        u16le(&mut out, 1);
        i32le(&mut out, -1);
    }
    u16le(&mut out, 1);
    out.extend_from_slice(&small_layer());
    i32le(&mut out, -11);
    i32le(&mut out, -22);
    u16le(&mut out, 0);
    out
}

#[test]
fn valid_actor_sequences_decode() {
    for version in [2, 3] {
        let s = ActorSequence::read(&actor_sequence(version)).expect("valid sequence");
        assert_eq!(s.frames.len(), 1);
        assert!(s.shadows.is_empty());
        assert_eq!(s.offset, (-11, -22));
        assert_eq!(s.bounds(), Some(Rect { left: 10, top: 20, right: 12, bottom: 22 }));
    }
}

#[test]
fn truncated_actor_sequence_fails() {
    // The trailing u16 isn't read (its meaning is unknown), so leave it off the check.
    for version in [2, 3] {
        let data = actor_sequence(version);
        assert_truncations_fail("actor sequence", &data[..data.len() - 2], ActorSequence::read);
    }
}

#[test]
fn unknown_actor_sequence_version_fails() {
    assert!(ActorSequence::read(&actor_sequence(7)).is_err());
}

// ---------------------------------------------------------------------------------------
// adv_actor models

fn adv_actor(version: u16) -> Vec<u8> {
    let mut out = Vec::new();
    u16le(&mut out, version);
    out.push(1);
    for len in [16, 32, 16] {
        u16le(&mut out, len);
    }
    u16le(&mut out, 1); // one action
    str16(&mut out, "walk");
    out.push(12);
    for dir in ["ne", "e", "se", "s", "sw", "w", "nw", "n"] {
        str16(&mut out, &format!("Griffin.walk.{dir}"));
    }
    for v in [1, 2, 3, 4] {
        i32le(&mut out, v);
    }
    out
}

#[test]
fn valid_adv_actor_decodes() {
    let m = AdvActorModel::read(&adv_actor(3)).expect("valid model");
    assert_eq!((m.prewalk_length, m.walk_length, m.postwalk_length), (16, 32, 16));
    assert_eq!(m.action("walk").unwrap().frames_per_second, 12);
    assert_eq!(m.sequence("walk", "sw").as_deref(), Some("actor_sequence.Griffin.walk.sw.h4d"));
    assert_eq!((m.offset, m.flag_offset), ((1, 2), (3, 4)));
}

#[test]
fn truncated_adv_actor_fails() {
    assert_truncations_fail("adv_actor", &adv_actor(3), AdvActorModel::read);
}

#[test]
fn unknown_adv_actor_version_fails() {
    assert!(AdvActorModel::read(&adv_actor(2)).is_err());
}

// ---------------------------------------------------------------------------------------
// Terrain metatiles

/// A metatile whose tile 0 has one red pixel at the top of column 32 (the middle of the
/// diamond) and every other tile is empty. Column 32 is 31 pixels tall: draw 1 pixel,
/// then skip the remaining 30.
fn metatile_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    for t in 0..METATILE_SIZE * METATILE_SIZE {
        // column data: [count 1, index 0, skip 30] for column 32 of tile 0
        let data: &[u8] = if t == 0 { &[0, 1, 0, 30] } else { &[] };
        u32le(&mut out, data.len() as u32);
        out.extend_from_slice(data);
        for x in 0..TILE_WIDTH {
            // empty columns point at an offset where a single big skip would be; for tile 0
            // only column 32 has real data, the rest point past it and stop at once
            u32le(&mut out, if t == 0 && x == 32 { 0 } else { data.len() as u32 });
        }
    }
    u32le(&mut out, 1);
    out.extend_from_slice(&[0, 0, 255]); // red
    out
}

#[test]
fn valid_metatile_decodes() {
    let m = Metatile::read(&metatile_bytes()).expect("valid metatile");
    let mut canvas = vec![0u8; 4 * TILE_WIDTH * TILE_HEIGHT];
    m.draw_tile(m.tile(0, 0), &mut canvas, TILE_WIDTH, 0, 0);
    // column 32 is 31 tall and starts at y = 0 of the 32-pixel box
    let o = 4 * 32;
    assert_eq!(&canvas[o..o + 4], &[255, 0, 0, 255]);
    let drawn = canvas.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
    assert_eq!(drawn, 1);
}

#[test]
fn truncated_metatile_fails() {
    assert_truncations_fail("metatile", &metatile_bytes(), Metatile::read);
}

#[test]
fn metatile_with_trailing_bytes_fails() {
    let mut data = metatile_bytes();
    data.push(0);
    assert!(Metatile::read(&data).is_err());
}

#[test]
fn drawing_a_tile_with_bad_columns_does_not_panic() {
    // Column offsets past the data, a run longer than the data, and a palette index with no
    // colour: all come from the file and must not crash the drawing.
    let mut out = Vec::new();
    for t in 0..METATILE_SIZE * METATILE_SIZE {
        let data: &[u8] = if t == 0 { &[0, 200, 7, 7] } else { &[] };
        u32le(&mut out, data.len() as u32);
        out.extend_from_slice(data);
        for x in 0..TILE_WIDTH {
            u32le(&mut out, if x % 2 == 0 { 0 } else { u32::MAX });
        }
    }
    u32le(&mut out, 0); // empty palette
    let m = Metatile::read(&out).expect("structurally valid");
    let mut canvas = vec![0u8; 4 * TILE_WIDTH * TILE_HEIGHT];
    m.draw_tile(m.tile(0, 0), &mut canvas, TILE_WIDTH, 0, 0);
    m.draw_tile(m.tile(0, 0), &mut canvas, 0, 0, 0);
    m.draw_tile(m.tile(0, 0), &mut canvas, TILE_WIDTH, i32::MAX, i32::MIN);
}

// ---------------------------------------------------------------------------------------
// Transition masks

/// One set with one fully covering 1-bit mask and one empty 4-bit mask.
fn transitions_bytes() -> Vec<u8> {
    let mut out = vec![1u8];
    str16(&mut out, "land 1");
    u32le(&mut out, 2);
    i32le(&mut out, 3333);
    out.push(1);
    out.extend_from_slice(&[0xff; 160]);
    i32le(&mut out, 1);
    out.push(4);
    out.extend_from_slice(&[0; 544]);
    out
}

#[test]
fn valid_transitions_decode() {
    let t = Transitions::read(&transitions_bytes()).expect("valid transitions");
    let full = t.mask("land 1", 3333).unwrap();
    // A full mask covers exactly the diamond: this also checks that the per-column sizes
    // add up to the 160 bytes the file stores (otherwise the last columns would read 0).
    for x in 0..TILE_WIDTH {
        let h = h4_formats::terrain::column_height(x);
        let top = (TILE_HEIGHT - h) / 2;
        for y in 0..TILE_HEIGHT {
            let inside = y >= top && y < top + h;
            assert_eq!(full.alpha(x, y), if inside { 255 } else { 0 }, "pixel ({x}, {y})");
        }
    }
    assert_eq!(full.alpha(TILE_WIDTH, 0), 0, "outside the tile");
    let empty = t.mask("land 1", 1).unwrap();
    assert_eq!(empty.depth, 4);
    assert_eq!(empty.alpha(32, 16), 0);
    // code 4441 isn't there: the lookup drops the diagonal-only corners and finds 1
    assert_eq!(t.mask("land 1", 4441).map(|m| m.depth), Some(4));
}

#[test]
fn full_four_bit_mask_covers_the_whole_diamond() {
    let mut data = transitions_bytes();
    let n = data.len();
    data[n - 544..].fill(0xff);
    let t = Transitions::read(&data).unwrap();
    let m = t.mask("land 1", 1).unwrap();
    for x in 0..TILE_WIDTH {
        let h = h4_formats::terrain::column_height(x);
        let top = (TILE_HEIGHT - h) / 2;
        assert_eq!(m.alpha(x, top), 255);
        assert_eq!(m.alpha(x, top + h - 1), 255, "last pixel of column {x}");
    }
}

#[test]
fn truncated_transitions_fail() {
    assert_truncations_fail("transitions", &transitions_bytes(), Transitions::read);
}

#[test]
fn transitions_with_trailing_bytes_or_unknown_depth_fail() {
    let mut data = transitions_bytes();
    data.push(0);
    assert!(Transitions::read(&data).is_err());

    let mut data = transitions_bytes();
    let depth_at = 1 + 2 + "land 1".len() + 4 + 4;
    assert_eq!(data[depth_at], 1);
    data[depth_at] = 2;
    assert!(Transitions::read(&data).is_err());
}

// ---------------------------------------------------------------------------------------
// .h4r archives

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// An archive with one real entry ("a.txt" = "hello") and one alias pointing at it.
fn archive_bytes() -> Vec<u8> {
    let blob = gzip(b"hello");
    let mut out = b"H4R\x05".to_vec();
    let dir_offset = 8 + blob.len() as u32;
    u32le(&mut out, dir_offset);
    out.extend_from_slice(&blob);
    u32le(&mut out, 2);
    for (name, alias, offset, csize, size) in [("a.txt", "", 8, blob.len() as u32, 5), ("b.txt", "a.txt", 0, 0, 0)] {
        u32le(&mut out, offset);
        u32le(&mut out, csize);
        u32le(&mut out, size);
        u32le(&mut out, 0); // time
        str16(&mut out, name);
        str16(&mut out, "C:\\source\\path");
        str16(&mut out, alias);
        u32le(&mut out, 0); // type
    }
    out
}

/// Write `data` to a fresh folder under Cargo's temp dir for integration tests.
fn temp_archive(test: &str, data: &[u8]) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(test);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("heroes4.h4r");
    std::fs::write(&path, data).unwrap();
    path
}

#[test]
fn valid_archive_reads_with_aliases() {
    let path = temp_archive("valid_archive", &archive_bytes());
    let res = Resources::open(path.parent().unwrap()).expect("open");
    assert_eq!(res.read("A.TXT").unwrap(), b"hello", "names are case-insensitive");
    assert_eq!(res.read("b.txt").unwrap(), b"hello", "alias is followed");
    assert!(res.read("c.txt").is_err());
}

#[test]
fn truncated_archive_directory_fails() {
    let data = archive_bytes();
    let dir_offset = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    assert_truncations_fail("archive directory", &data[dir_offset..], read_directory);
}

#[test]
fn archive_directory_with_huge_count_fails() {
    // a count of 4 billion entries in an 8-byte directory must not try to allocate them
    let mut dir = Vec::new();
    u32le(&mut dir, u32::MAX);
    u32le(&mut dir, 0);
    assert!(read_directory(&dir).is_err());
}

#[test]
fn archive_with_entry_past_the_end_fails() {
    let mut data = archive_bytes();
    let dir_offset = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    // first entry's compressed size, right after the count and its offset
    let csize_at = dir_offset + 4 + 4;
    data[csize_at..csize_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Archive::open(temp_archive("entry_past_end", &data)).is_err());
}

#[test]
fn archive_with_bad_magic_or_directory_offset_fails() {
    let mut data = archive_bytes();
    data[3] = 4;
    assert!(Archive::open(temp_archive("bad_magic", &data)).is_err());

    let mut data = archive_bytes();
    data[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Archive::open(temp_archive("bad_dir_offset", &data)).is_err());

    assert!(Archive::open(temp_archive("empty", &[])).is_err());
}

#[test]
fn alias_loop_fails() {
    let mut data = archive_bytes();
    // turn "a.txt" into an alias of "b.txt", which already points back at "a.txt"
    let dir_offset = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let alias_len_at = dir_offset + 4 + 16 + 2 + "a.txt".len() + 2 + "C:\\source\\path".len();
    assert_eq!(&data[alias_len_at..alias_len_at + 2], &[0, 0]);
    let mut patched = data[..alias_len_at].to_vec();
    str16(&mut patched, "b.txt");
    patched.extend_from_slice(&data[alias_len_at + 2..]);
    data = patched;
    let path = temp_archive("alias_loop", &data);
    let res = Resources::open(path.parent().unwrap()).expect("open");
    assert!(res.read("a.txt").is_err());
}
