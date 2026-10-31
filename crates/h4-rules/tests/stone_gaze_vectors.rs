//! Original eligibility/effectiveness/final-damage answers checked through the bench adapter.
#[allow(dead_code)]
#[path = "../examples/support/immunity_case.rs"]
mod immunity_case;
#[path = "../examples/support/stone_gaze_case.rs"]
mod stone_gaze_case;

use h4_rules::{
    damage::ability,
    effects::{flag, ImmunityFilters, ResistanceGate, StoneGazeContext},
};

#[test]
fn resolved_stone_gaze_matches_original() {
    let mut count = 0;
    for line in include_str!("stone_gaze_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (input, expected) = line.split_once('\t').expect("fixture has two columns");
        let fields: Vec<_> = input.split_whitespace().skip(1).collect();
        assert_eq!(
            stone_gaze_case::evaluate(&fields).expect("valid fixture"),
            expected,
            "{input}"
        );
        count += 1;
    }
    assert_eq!(count, 896);
}

#[test]
fn unknown_checks_or_missing_caster_keep_gaze_unresolved() {
    let mut context = StoneGazeContext {
        resistance_gate: ResistanceGate {
            immunities: ImmunityFilters {
                flags: flag::RESISTIBLE,
                caster_abilities: Some(&[ability::STONE_GAZE]),
                target_abilities: &[],
                target_effects: &[],
            },
            hostile: true,
            same_side: false,
            resistance: 25,
        },
        remaining_eligibility: None,
        generic_resistance: 75,
        is_creature: true,
        power_percent: 100,
        creature_hit_points: 37,
    };
    // Everything known here permits the gaze. Unknown external checks must still
    // prevent constructing a seemingly eligible target for the damage function.
    assert!(context.resolve().is_none());
    context.remaining_eligibility = Some(true);
    context.resistance_gate.immunities.caster_abilities = None;
    assert!(context.resolve().is_none());
}
