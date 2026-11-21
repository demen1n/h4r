//! The whole battlefield path chain on a made-up field, as a usage example:
//! template → preparation → search → path. (Each step is checked against the exe by
//! its own vector test; this one only shows they fit together.)
use h4_rules::battle::path::{flags, Mover};
use h4_rules::battle::path_prepare::{prepare, FieldObject, GateState, PrepareInput, PreparedMover};
use h4_rules::battle::path_template::{template, BlockageMap, Blocker, BlockerKind};

#[test]
fn a_creature_walks_around_a_rock() {
    // A small square field of 30 rows of 30 cells.
    let rows = vec![(0, 30); 30];
    // A rock of size 4 in the middle, as fixed scenery.
    let rock = Blocker { kind: BlockerKind::Object { blocks: true, fixed: true }, position: (13 * 16, 13 * 16), size: 4 };
    let blockage = BlockageMap::build(&rows, &[rock], |_, _| false);
    let size = 3;
    let base = template(&rows, &blockage, size);

    // The creature stands at (14, 5) and nothing else is on the field.
    let mover = PreparedMover { position: (14 * 16, 5 * 16), size: i32::from(size), tower: None, teleports: false, flies: false, defender: false };
    let objects: [FieldObject; 0] = [];
    let plain = |_: (i32, i32)| 0;
    let mut prepared = prepare(&PrepareInput {
        template: &base,
        mover,
        objects: &objects,
        gate: GateState::default(),
        attack_mode: false,
        terrain: &plain,
        costs: &[],
    })
    .search;

    let walker = Mover { movement: 1000, footprint_size: i32::from(size), long_weapon: false, flies: false, held: false, teleports: false };
    prepared.run(&walker, |_| [false; 8]);

    // The far side of the rock is reachable, but not in a straight line: the path is
    // longer than the 14 straight steps and avoids every cell where the footprint would
    // touch the rock.
    let path = prepared.path_to((14, 22)).expect("reachable around the rock");
    assert!(path.len() > 14, "{path:?}");
    for at in &path {
        let cell = prepared.map.cell(*at).unwrap();
        assert_eq!(cell.flags & flags::OCCUPIED, 0, "{at:?} touches the rock");
    }
    // A cell under the rock can't be a footprint corner.
    assert!(prepared.path_to((14, 14)).is_none());
}
