use h4_formats::battlefield::{BattlefieldPreset, BattlefieldRow};
use h4_rules::battlefield::*;

fn field() -> BattlefieldPreset {
    BattlefieldPreset {
        rows: (0..8)
            .map(|_| BattlefieldRow {
                start: 0,
                cells: vec![2; 8],
            })
            .collect(),
        layers: Vec::new(),
    }
}
fn obstacle(id: u32, at: (i32, i32), size: u8) -> PlacedObstacle {
    PlacedObstacle {
        at,
        footprint: CombatFootprint { size },
        occupant: CellOccupant {
            id,
            blocks: true,
            skip_blocking: false,
        },
    }
}
#[test]
fn native_footprints_match_including_rounded_corners() {
    let mut count = 0;
    for line in include_str!("footprint_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (size, hex) = line.split_once(' ').unwrap();
        let size: u8 = size.parse().unwrap();
        let shape = CombatFootprint { size };
        let bytes: Vec<_> = if hex == "-" {
            Vec::new()
        } else {
            hex.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect()
        };
        assert_eq!(bytes.len(), usize::from(size).pow(2));
        for x in 0..i32::from(size) {
            for y in 0..i32::from(size) {
                assert_eq!(
                    shape.contains(x, y),
                    bytes[(x * i32::from(size) + y) as usize] != 0,
                    "size {size}, ({x},{y})"
                );
            }
        }
        assert!(!shape.contains(-1, 0));
        assert!(!shape.contains(0, i32::from(size)));
        assert!(!shape.contains(i32::MAX, i32::MIN));
        count += 1;
    }
    assert_eq!(count, 18);
}
#[test]
fn blocking_matches_native_virtual_queries_and_compound_overrides() {
    let mut count = 0;
    for line in include_str!("obstacle_blocking_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let v: Vec<i32> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let model = h4_formats::combat_object::CombatObject {
            version: 2,
            properties: [1, 1],
            parameter: 0,
            frames_per_second: 10,
            flag: v[1] as u8,
            attributes: [v[0] as u8, 0, 0, 0, 0],
            type_id: 0x25,
            type_name: String::new(),
            image: h4_formats::sprite::ActorSequence {
                frames: Vec::new(),
                shadows: Vec::new(),
                base: Vec::new(),
                offset: (0, 0),
            },
        };
        let mut occupant = PlacedObstacle::from_square_model(&model, (0, 0), 1)
            .unwrap()
            .occupant;
        if v[2] >= 0 {
            occupant.skip_blocking = v[2] != 0;
        }
        let mut occupants = vec![occupant];
        if v[4] != 0 {
            occupants.push(CellOccupant {
                id: 2,
                blocks: true,
                skip_blocking: false,
            });
        }
        let query = if v[3] == 0 { None } else { Some(v[3] as u32) };
        assert_eq!(
            cell_is_blocked(v[5] as u8, &occupants, query),
            v[6] != 0,
            "{line}"
        );
        count += 1;
    }
    assert_eq!(count, 432);
}
#[test]
fn overlays_and_overlapping_segments_keep_independent_blockers() {
    let mut grid = BattlefieldGrid::from_preset(&field(), CellSurface::default(), false);
    grid.add_obstacle(obstacle(1, (2, 2), 5)).unwrap();
    assert!(!grid.is_blocked(2, 2, None)); // native rounded corner
    assert!(grid.is_blocked(3, 3, None));
    assert!(!grid.is_blocked(3, 3, Some(1)));
    assert!(grid.add_obstacle(obstacle(1, (0, 0), 1)).is_err());
    assert!(grid.cell(0, 0).unwrap().occupants.is_empty());
    grid.add_obstacle(obstacle(2, (3, 3), 1)).unwrap();
    assert!(grid.is_blocked(3, 3, Some(1)));
    grid.remove_object(2);
    assert!(!grid.is_blocked(3, 3, Some(1)));
    let mut overlay = field();
    overlay.rows[3].cells[3] = 255;
    grid.apply_preset(&overlay, false);
    assert!(grid.is_blocked(3, 3, Some(1))); // terrain cannot be excluded
    grid.remove_object(1);
    assert!(grid.is_blocked(3, 3, None));
    assert!(grid.is_blocked(-1, 0, None));
    assert!(grid.is_blocked(0, 8, None));
}
#[test]
fn partial_or_overflowing_placements_are_rejected_atomically() {
    let mut grid = BattlefieldGrid::from_preset(&field(), CellSurface::default(), false);
    for at in [(7, 7), (-1, 0), (i32::MAX, 0)] {
        assert!(grid.add_obstacle(obstacle(1, at, 2)).is_err());
    }
    assert!(grid.add_obstacle(obstacle(1, (0, 0), 0)).is_err());
    assert!(grid.cell(7, 7).unwrap().occupants.is_empty());
    assert!(grid.cell(0, 0).unwrap().occupants.is_empty());
}
#[test]
fn unmatched_overlay_geometry_is_skipped_without_losing_occupants() {
    let mut grid = BattlefieldGrid::from_preset(&field(), CellSurface::default(), false);
    grid.add_obstacle(obstacle(1, (1, 1), 1)).unwrap();
    let mut overlay = field();
    overlay.rows[0].cells.fill(255);
    overlay.rows[1].start = 1;
    overlay.rows[1].cells.fill(255);
    grid.apply_preset(&overlay, false);
    assert!(grid.is_blocked(0, 0, None));
    assert!(!grid.is_blocked(1, 1, Some(1)));
    assert!(grid.is_blocked(1, 1, None));
    overlay.rows.pop();
    overlay.rows[2].cells.fill(255);
    grid.apply_preset(&overlay, false);
    assert!(!grid.is_blocked(2, 2, None));
}

#[test]
fn every_stored_cell_center_matches_native_subcell_projection() {
    let mut count = 0;
    for line in include_str!("battlefield_center_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let v: Vec<i32> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(
            BattlefieldPreset::cell_center(v[0] as usize, v[1] as usize),
            Some((v[2], v[3])),
            "{line}"
        );
        count += 1;
    }
    assert_eq!(count, 4838);
}
