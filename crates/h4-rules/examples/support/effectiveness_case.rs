//! Shared adapter for effectiveness requests and captured original-answer tests.
//! The first six fields are eligibility, two resistance flags, side equality and
//! two caster abilities; the last two are effect-specific and generic resistance.
use h4_rules::effects::Effectiveness;

pub fn evaluate(fields: &[&str]) -> Result<i32, Box<dyn std::error::Error>> {
    let [eligible, resisted, between, same, bypass, generic, specific_r, generic_r] = fields else {
        return Err("invalid effectiveness fields".into());
    };
    let input = Effectiveness {
        eligible: eligible.parse::<u8>()? != 0,
        resisted: resisted.parse::<u8>()? != 0,
        resisted_between_sides: between.parse::<u8>()? != 0,
        same_side: same.parse::<u8>()? != 0,
        bypass_resistance: bypass.parse::<u8>()? != 0,
        generic_resistance_only: generic.parse::<u8>()? != 0,
        effect_resistance: specific_r.parse()?,
        generic_resistance: generic_r.parse()?,
    };
    Ok(input.percent())
}
