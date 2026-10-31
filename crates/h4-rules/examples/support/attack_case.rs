//! Shared original-exe finalization/attack fixture adapter.
use h4_rules::{damage::*, random::CombatRandom};

pub fn evaluate(kind: &str, mut fields: &[&str]) -> Result<Vec<i32>, Box<dyn std::error::Error>> {
    let prefix: Vec<u32> = if kind == "attack" {
        let p = fields[..5]
            .iter()
            .map(|v| v.parse())
            .collect::<Result<_, _>>()?;
        fields = &fields[5..];
        p
    } else {
        vec![]
    };
    let (base, mut a, mut d, mut options) = super::modifier_case::parse(&fields[..15])?;
    let [number, hp, dealt, unit_hp, creature, eligible, effectiveness, power, excluded, block] =
        &fields[15..]
    else {
        return Err("invalid finalization fixture".into());
    };
    a.number = number.parse()?;
    d.total_hit_points = hp.parse()?;
    let excluded: u32 = excluded.parse()?;
    for (i, ability) in [ability::UNDEAD, ability::ABILITY_2A, ability::ELEMENTAL]
        .iter()
        .enumerate()
    {
        if excluded & (1 << i) != 0 {
            d.abilities.push(*ability);
        }
    }
    if *block == "1" {
        d.abilities.push(ability::BLOCK);
    }
    options.already_dealt = dealt.parse()?;
    options.ability_42 = Some(Ability42Target {
        is_creature: *creature == "1",
        can_affect: *eligible == "1",
        effectiveness_percent: effectiveness.parse()?,
        power_percent: power.parse()?,
        creature_hit_points: unit_hp.parse()?,
    });
    if kind == "ability42" {
        return Ok(vec![finalize_attack_damage(
            base,
            &a,
            &d,
            options.already_dealt,
            options.ability_42.as_ref(),
            &mut Vec::new(),
        )]);
    }
    let [seed, modulus, low, high, hits] = prefix[..] else {
        return Err("invalid attack prefix".into());
    };
    a.id = 3;
    d.id = if *creature == "1" { 7 } else { 0 };
    a.damage_low = low as i32;
    a.damage_high = high as i32;
    let mut dice = CombatRandom::new(seed, modulus);
    let mut result = (0..hits)
        .map(|_| attack_hit(&a, &d, &options, &mut dice, &mut Vec::new()))
        .collect::<Vec<_>>();
    result.push(dice.roll(d.id, a.id, 0) as i32);
    Ok(result)
}
