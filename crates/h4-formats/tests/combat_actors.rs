//! Structural and reference checks for combat definitions, plus corrupt input tests.
mod common;

use h4_formats::catalog::Actor;
use h4_formats::combat_actor::CombatActorModel;
use h4_formats::sprite::ActorSequence;

fn string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn definition(version: u16) -> Vec<u8> {
    let mut out = version.to_le_bytes().to_vec();
    out.extend_from_slice(&[6, 96]);
    for length in [16_u16, 140, 16] {
        out.extend_from_slice(&length.to_le_bytes());
    }
    if version == 6 {
        for _ in 0..8 {
            for coordinate in [-3_i32, 5, 48] {
                out.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    out.extend_from_slice(&1_u16.to_le_bytes());
    string(&mut out, "melee");
    out.extend_from_slice(&[9, 4]);
    // Deliberately use a shared sequence and a missing direction: filename-based
    // discovery cannot recover this mapping.
    for sequence in ["unit.combat.melee.ne", "unit.combat.melee.ne", "", "", "", "", "", ""] {
        string(&mut out, sequence);
    }
    out.extend_from_slice(&10_i32.to_le_bytes());
    out.extend_from_slice(&(-4_i32).to_le_bytes());
    out
}

#[test]
fn versions_and_explicit_direction_mapping() {
    for version in [5, 6] {
        let model = CombatActorModel::read(&definition(version)).unwrap();
        assert_eq!(model.version, version);
        assert_eq!(model.offset, (10, -4));
        assert_eq!(model.action("melee").unwrap().frames_per_second, 9);
        assert_eq!(model.action("melee").unwrap().parameter, 4);
        assert_eq!(model.directional_points[0], if version == 5 { (3, 3, 48) } else { (-3, 5, 48) });
        assert_eq!(model.sequence("melee", "e"), model.sequence("melee", "ne"));
        assert!(model.sequence("melee", "se").is_none());
        assert!(model.sequence("melee", "invalid").is_none());
        let actor = Actor::from_combat_model("unit".into(), &model);
        assert_eq!(actor.directions("melee"), vec!["ne", "e"]);
        assert_eq!(actor.actions["melee"]["e"], "actor_sequence.unit.combat.melee.ne.h4d");
    }
}

#[test]
fn damaged_definitions_fail_without_panicking() {
    for version in [5, 6] {
        let data = definition(version);
        for end in 0..data.len() {
            assert!(CombatActorModel::read(&data[..end]).is_err(), "v{version}, length {end}");
        }
        let mut trailing = data.clone();
        trailing.push(0);
        assert!(CombatActorModel::read(&trailing).is_err());
        let mut huge = data;
        let count_offset = if version == 6 { 106 } else { 10 };
        huge[count_offset..count_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(CombatActorModel::read(&huge).is_err());
    }
    assert!(CombatActorModel::read(&7_u16.to_le_bytes()).is_err());
    assert!(CombatActorModel::read(&4_u16.to_le_bytes()).is_err());
}

#[test]
fn installed_combat_models_and_sequences() {
    let Some(res) = common::resources() else { return };
    let names = common::names_with_prefix(&res, "combat_actor.");
    assert_eq!(names.len(), 158);
    let mut versions = [0; 7];
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut sequences = std::collections::BTreeSet::new();
    for name in names {
        let model = match res.read(name).and_then(|d| CombatActorModel::read(&d)) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        versions[model.version as usize] += 1;
        for action in &model.actions {
            if action.is_composite() {
                continue;
            }
            if action.frames_per_second == 0 {
                failures.push(format!("{name}: {} has zero fps", action.name));
            }
            for sequence in action.sequences.iter().filter(|s| !s.is_empty()) {
                let resource = format!("actor_sequence.{sequence}.h4d");
                if res.entry(&resource).is_none() {
                    missing.push((name.to_lowercase(), action.name.clone(), sequence.clone()));
                } else {
                    sequences.insert(resource);
                }
            }
        }
    }
    assert_eq!((versions[5], versions[6]), (7, 151));
    // Exact known broken references in the installed resources. Do not silently
    // accept a new missing animation, or an exception disappearing after a fix.
    let mut expected = Vec::new();
    for direction in ["e", "se", "s", "sw", "w", "nw", "n"] {
        expected.push((
            "combat_actor.behemoth.h4d".into(),
            "cast_spell".into(),
            format!("behemoth.combat.spellcast.{direction}"),
        ));
    }
    for _ in 0..8 {
        expected.push(("combat_actor.smoke.h4d".into(), "cast_spell".into(), "Smoke.Smoke00".into()));
    }
    for (action, sequence_action) in [
        ("postwalk", "postwalk"),
        ("prewalk", "prewalk"),
        ("wait", "stand"),
        ("walk", "walk"),
    ] {
        for direction in 1..=8 {
            expected.push((
                "combat_actor.test_hero.h4d".into(),
                action.into(),
                format!("test_hero.{sequence_action}.{sequence_action}_{direction}"),
            ));
        }
    }
    missing.sort();
    expected.sort();
    assert_eq!(missing, expected);
    // Decode each referenced animation once, including shared directions and aliases.
    for name in sequences {
        match res.read(&name).and_then(|d| ActorSequence::read(&d)) {
            Ok(sequence) if !sequence.frames.is_empty() => {}
            Ok(_) => failures.push(format!("{name}: empty animation")),
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    common::assert_no_failures("combat models and sequences", &failures);
}
