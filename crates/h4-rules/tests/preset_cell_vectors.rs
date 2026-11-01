//! Original x86 context constructor + preset application outputs for CI.
use h4_rules::battlefield::{apply_preset_cell, cell_is_blocked, CellSurface};

#[test]
fn preset_overlays_match_native_adventure_context_for_every_code() {
    let mut count = 0;
    for line in include_str!("preset_cell_vectors.tsv").lines() {
        if line.starts_with('#') {
            continue;
        }
        let v: Vec<u8> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(v.len(), 6);
        let mut cell = CellSurface {
            terrain: v[2],
            flags: v[3],
        };
        apply_preset_cell(&mut cell, v[0], v[1] != 0);
        assert_eq!(
            cell,
            CellSurface {
                terrain: v[4],
                flags: v[5]
            },
            "{line}"
        );
        assert_eq!(cell_is_blocked(cell.flags, &[], None), v[5] & 4 != 0);
        count += 1;
    }
    assert_eq!(count, 3072);
}

#[test]
fn overlays_preserve_or_clear_previous_blocking_in_order() {
    for on_bridge in [false, true] {
        let mut cell = CellSurface::default();
        apply_preset_cell(&mut cell, 255, on_bridge);
        assert!(cell_is_blocked(cell.flags, &[], None));
        apply_preset_cell(&mut cell, 0, on_bridge);
        assert!(cell_is_blocked(cell.flags, &[], None));
        apply_preset_cell(&mut cell, 2, on_bridge);
        assert!(!cell_is_blocked(cell.flags, &[], None));
        apply_preset_cell(&mut cell, 0, on_bridge);
        assert_eq!(cell_is_blocked(cell.flags, &[], None), on_bridge);
    }
}
