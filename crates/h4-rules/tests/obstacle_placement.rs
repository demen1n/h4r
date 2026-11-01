//! Obstacle placement on the installed game's data (skipped without the game).

use std::collections::HashMap;

use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_object::{obstacle_type_by_name, CombatHeaderTable, CombatObject};
use h4_formats::table::Table;
use h4_rules::battlefield::{BattlefieldGrid, CellSurface, PlacedObstacle, FLAG_10};
use h4_rules::obstacle_placement::{place_obstacles, ObstacleWeights, PlacerContext};
use h4_rules::random::CombatRandom;

#[test]
fn installed_headers_and_weights_place_plausible_scenery() {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return;
    }
    let res = Resources::open(&dir).expect("open resources");
    let headers = CombatHeaderTable::read(&res.read(CombatHeaderTable::RESOURCE).unwrap()).unwrap();
    assert_eq!(headers.entries.len(), 838);
    assert_eq!(headers.unknown, [3, 1, 2002, 10, 45]);
    // The cached headers are the resources' headers, and stored type ids
    // agree with the type names.
    for entry in &headers.entries {
        let name = format!("combat_object.{}.h4d", entry.name);
        let model = CombatObject::read(&res.read(&name).unwrap()).unwrap();
        // The cache stores the resolved type; the resources an older number.
        let mut resolved = model.header.clone();
        resolved.type_id = model.header.resolved_type();
        assert_eq!(resolved, entry.header, "{name}");
        assert_eq!(entry.header.resolved_type(), entry.header.type_id, "{name}");
    }
    let table = Table::read(
        &res.read("table.combat_obstacles.h4d").unwrap(),
        "combat_obstacles",
    )
    .unwrap();
    let weights = ObstacleWeights::from_table(&table);
    // Typos in the installed table: "snow logs" never matches `snow_log`.
    let snow_log = usize::from(obstacle_type_by_name("snow_log").unwrap());
    assert!((0..9).all(|terrain| weights.terrain(snow_log, terrain) == 0));

    for (terrain, name) in [(1, "grass"), (6, "sand"), (5, "snow"), (8, "subterranean")] {
        let mut grid = BattlefieldGrid::standard(CellSurface {
            terrain,
            flags: FLAG_10,
        });
        grid.block_edges();
        let mut random = CombatRandom::new(12345, 1);
        let placed = place_obstacles(
            &grid,
            &headers.entries,
            &weights,
            &PlacerContext::default(),
            &mut random,
        );
        let mut types: HashMap<&str, usize> = HashMap::new();
        for p in &placed {
            let header = &headers.entries[p.model].header;
            assert!(
                weights.terrain(usize::from(header.type_id), terrain) > 0,
                "{name}: {}",
                header.type_name
            );
            *types.entry(header.type_name.as_str()).or_default() += 1;
        }
        println!("{name}: {} obstacles {types:?}", placed.len());
        assert!(placed.len() > 10, "{name}: only {} obstacles", placed.len());

        // Every placement becomes blocking segments that fit the field.
        let mut next_id = 1;
        for p in &placed {
            let entry = &headers.entries[p.model];
            let model = CombatObject::read(
                &res.read(&format!("combat_object.{}.h4d", entry.name))
                    .unwrap(),
            )
            .unwrap();
            if !model.blocks() || model.skips_blocking() {
                continue;
            }
            for segment in model.segments().unwrap() {
                let obstacle =
                    PlacedObstacle::from_segment(&model, &segment, p.at, next_id).unwrap();
                next_id += 1;
                grid.add_obstacle(obstacle).unwrap();
            }
        }
    }
}
