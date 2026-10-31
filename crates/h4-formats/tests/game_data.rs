//! Regression tests over the real game data (skipped when the game isn't installed).
//!
//! Reading a file to its last byte proves only that the structure is right. Where we can,
//! these tests also check that the values make sense: sizes in range, references pointing at
//! resources that exist, counts matching what we found earlier. The expected counts are for
//! the Russian Buka 3.0 install described in CLAUDE.md; another release may differ.

mod common;

use common::{assert_no_failures, names_with_prefix, resources};
use h4_formats::adv_actor::AdvActorModel;
use h4_formats::sprite::{read_bitmap_group, ActorSequence, Layer};
use h4_formats::terrain::{column_height, Metatile, METATILE_SIZE, TILE_HEIGHT, TILE_WIDTH};
use h4_formats::transition::{Transitions, MAP_MASK_SETS};

const ACTOR_SEQUENCES: usize = 13_336;
// All archives together; the base game alone has fewer.
const ADV_ACTORS: usize = 154;
const TERRAINS: usize = 61;
const TRANSITION_SETS: usize = 15;
const MASKS_PER_SET: usize = 93;

// Oddities of the game's own data, not of our readers. Listed by name so that anything new
// still fails the tests.

/// Sequences whose shadows don't come one per frame: the Armageddon missile has 6 frames and
/// a single shadow.
const SHADOW_COUNT_EXCEPTIONS: &[&str] =
    &["actor_sequence.armageddon_missile.Missile.h4d", "actor_sequence.missile.armageddon.Missile.h4d"];

/// adv_actor models that name animations which aren't in any archive. The placeholder
/// "Random Hero" has a `fidget` action pointing at `stand` sequences it doesn't have.
const MISSING_SEQUENCE_EXCEPTIONS: &[&str] = &["adv_actor.hero.Random Hero.h4d"];

/// Aliases whose target doesn't exist: `layers.sample_cursor` points at
/// `layers.cursor.sample_cursor`, which no archive contains.
const BROKEN_ALIASES: &[&str] = &["layers.sample_cursor.h4d"];

/// A layer's rectangle must not be inside out, and its offset must be within reason.
fn check_layer(name: &str, layer: &Layer, failures: &mut Vec<String>) {
    let r = layer.rect;
    if !layer.is_empty() && (r.width() <= 0 || r.height() <= 0) {
        failures.push(format!("{name}: layer {:?} has an empty or inverted rect {r:?}", layer.name));
    }
}

#[test]
fn actor_sequences() {
    let Some(res) = resources() else { return };
    let names = names_with_prefix(&res, "actor_sequence.");
    assert_eq!(names.len(), ACTOR_SEQUENCES, "number of actor_sequence resources");

    let mut failures = Vec::new();
    for name in names {
        let seq = match res.read(name).and_then(|d| ActorSequence::read(&d)) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        if seq.frames.is_empty() {
            failures.push(format!("{name}: no frames"));
        }
        // Shadows, when present, come one per frame (`shadow NNN` next to `frame NNN`).
        let shadows_ok = seq.shadows.is_empty() || seq.shadows.len() == seq.frames.len();
        if !shadows_ok && !SHADOW_COUNT_EXCEPTIONS.contains(&name) {
            failures.push(format!("{name}: {} frames but {} shadows", seq.frames.len(), seq.shadows.len()));
        }
        for layer in seq.frames.iter().chain(&seq.shadows) {
            check_layer(name, layer, &mut failures);
        }
        let (x, y) = seq.offset;
        if x.abs() > 4096 || y.abs() > 4096 {
            failures.push(format!("{name}: implausible offset {:?}", seq.offset));
        }
    }
    assert_no_failures("actor sequences", &failures);
}

#[test]
fn adv_actors_reference_existing_sequences() {
    let Some(res) = resources() else { return };
    let names = names_with_prefix(&res, "adv_actor.");
    let mut parsed = 0;
    let mut failures = Vec::new();
    for name in &names {
        let model = match res.read(name).and_then(|d| AdvActorModel::read(&d)) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        parsed += 1;
        if model.actions.is_empty() {
            failures.push(format!("{name}: no actions"));
        }
        for action in &model.actions {
            if action.frames_per_second == 0 {
                failures.push(format!("{name}: action {:?} has 0 frames per second", action.name));
            }
            for seq in action.sequences.iter().filter(|s| !s.is_empty()) {
                let full = format!("actor_sequence.{seq}.h4d");
                if res.entry(&full).is_none() && !MISSING_SEQUENCE_EXCEPTIONS.contains(name) {
                    failures.push(format!("{name}: action {:?} refers to missing {full}", action.name));
                }
            }
        }
    }
    assert_no_failures("adv_actor models", &failures);
    assert_eq!(parsed, ADV_ACTORS, "number of adv_actor resources");
}

#[test]
fn terrain_metatiles() {
    let Some(res) = resources() else { return };
    let names = names_with_prefix(&res, "terrain.");
    assert_eq!(names.len(), TERRAINS, "number of terrain resources");

    let mut failures = Vec::new();
    for name in names {
        // `Metatile::read` itself insists on consuming the file exactly.
        let metatile = match res.read(name).and_then(|d| Metatile::read(&d)) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        assert_eq!(metatile.tiles.len(), METATILE_SIZE * METATILE_SIZE);
        // Every pixel a tile draws must lie inside its diamond: the runs of each column have to
        // add up to that column's height. Draw on a canvas twice as tall to catch overruns.
        let mut drawn_pixels = 0;
        for (i, tile) in metatile.tiles.iter().enumerate() {
            let mut canvas = vec![0u8; 4 * TILE_WIDTH * 2 * TILE_HEIGHT];
            metatile.draw_tile(tile, &mut canvas, TILE_WIDTH, 0, 0);
            for (p, px) in canvas.as_chunks::<4>().0.iter().enumerate() {
                if px[3] == 0 {
                    continue;
                }
                drawn_pixels += 1;
                let (x, y) = (p % TILE_WIDTH, p / TILE_WIDTH);
                let h = column_height(x);
                let top = (TILE_HEIGHT - h) / 2;
                if y < top || y >= top + h {
                    failures.push(format!("{name}: tile {i} draws outside its diamond at ({x}, {y})"));
                    break;
                }
            }
        }
        if drawn_pixels == 0 {
            failures.push(format!("{name}: every tile is transparent"));
        }
    }
    assert_no_failures("terrain metatiles", &failures);
}

#[test]
fn transition_masks() {
    let Some(res) = resources() else { return };
    let data = res.read("transition.Transitions.h4d").expect("read transitions");
    // `Transitions::read` itself insists on consuming the file exactly.
    let t = Transitions::read(&data).expect("parse transitions");
    assert_eq!(t.sets.len(), TRANSITION_SETS);

    let mut failures = Vec::new();
    for (set, masks) in &t.sets {
        let known = ["land ", "road ", "water ", "river ", "water_to_land ", "land_to_water "];
        if !known.iter().any(|k| set.starts_with(k)) {
            failures.push(format!("unexpected set name {set:?}"));
        }
        // The map is keyed by code, so this also says that no code appears twice.
        if masks.len() != MASKS_PER_SET {
            failures.push(format!("{set:?}: {} distinct masks", masks.len()));
        }
        for (code, mask) in masks {
            // One digit 0..=4 per corner (see `transition::corner_code`), so 0..=4444.
            let digits_ok = (0..4).all(|i| code / 10i32.pow(i) % 10 <= 4);
            if !(0..=4444).contains(code) || !digits_ok {
                failures.push(format!("{set:?}: mask code {code} is not four corner digits"));
            }
            let covered = (0..TILE_WIDTH).flat_map(|x| (0..TILE_HEIGHT).map(move |y| (x, y)));
            if covered.filter(|&(x, y)| mask.alpha(x, y) > 0).count() == 0 {
                failures.push(format!("{set:?}: mask {code} covers nothing"));
            }
        }
    }
    // Maps refer to masks by one of 20 set numbers and a position in the set.
    for (n, set) in MAP_MASK_SETS.iter().enumerate() {
        if t.order.get(*set).map(Vec::len) != Some(MASKS_PER_SET) {
            failures.push(format!("map mask set {n} names {set:?}, which has no {MASKS_PER_SET} masks"));
        }
    }
    assert_no_failures("transition masks", &failures);
}

#[test]
fn bitmap_groups() {
    let Some(res) = resources() else { return };
    let names = names_with_prefix(&res, "layers.");
    assert!(!names.is_empty(), "no layers.* resources");
    let mut failures = Vec::new();
    for name in names.into_iter().filter(|n| !BROKEN_ALIASES.contains(n)) {
        match res.read(name).and_then(|d| read_bitmap_group(&d)) {
            Ok(layers) => {
                for layer in &layers {
                    check_layer(name, layer, &mut failures);
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    assert_no_failures("bitmap groups", &failures);
}

#[test]
fn broken_aliases_are_still_broken() {
    let Some(res) = resources() else { return };
    // If one of these starts working (a different install, or a bug in alias handling),
    // the exception list above is out of date.
    for name in BROKEN_ALIASES {
        assert!(res.entry(name).is_some(), "{name} is no longer in the archives");
        assert!(res.read(name).is_err(), "{name} now resolves; remove it from BROKEN_ALIASES");
    }
}
