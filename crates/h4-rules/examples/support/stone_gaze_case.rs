//! Connects effect filters to final damage for the exe bench and golden-vector tests.
//! Fields: the seven immunity fields, then generic resistance, remaining eligibility,
//! creature type, power, creature HP, attacker count, base damage, total HP, dealt, block.
use h4_rules::{damage::*, effects::StoneGazeContext};

pub fn evaluate(fields: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let (immunity, final_fields) = fields
        .split_at_checked(7)
        .ok_or("missing immunity fields")?;
    let [generic, remaining, creature, power, unit_hp, number, damage, hp, dealt, block] =
        final_fields
    else {
        return Err("invalid stone-gaze fields".into());
    };
    let case = super::immunity_case::parse(immunity)?;
    let gate = case.gate();
    let target = StoneGazeContext {
        resistance_gate: gate,
        remaining_eligibility: Some(remaining.parse::<u8>()? != 0),
        generic_resistance: generic.parse()?,
        is_creature: creature.parse::<u8>()? != 0,
        power_percent: power.parse()?,
        creature_hit_points: unit_hp.parse()?,
    }
    .resolve()
    .ok_or("stone gaze needs a caster")?;
    let a = Attacker {
        id: 1,
        number: number.parse()?,
        damage_low: 1,
        damage_high: 1,
        offense: 1.0,
        defense_reduction: 1.0,
        school: School::Might,
        abilities: gate
            .immunities
            .caster_abilities
            .ok_or("missing caster")?
            .to_vec(),
        giant_bonus_effect: false,
    };
    let mut abilities = gate.immunities.target_abilities.to_vec();
    if *block == "1" {
        abilities.push(ability::BLOCK);
    }
    let d = Defender {
        id: 2,
        defense_basic: 1.0,
        defense_bonus: 1.0,
        abilities,
        warded_against: Vec::new(),
        giant: false,
        total_hit_points: hp.parse()?,
    };
    let damage = finalize_attack_damage(
        damage.parse()?,
        &a,
        &d,
        dealt.parse()?,
        Some(&target),
        &mut Vec::new(),
    );
    Ok(format!(
        "{} {} {}",
        u8::from(target.can_affect),
        target.effectiveness_percent,
        damage
    ))
}
