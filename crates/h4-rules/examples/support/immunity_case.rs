//! Shared adapter for independent immunity-filter requests and original-answer tests.
//! Fields: flags, caster ability ids ("none" = no caster, "-" = no abilities),
//! target ability ids, active effect ids, hostility, side equality, target resistance.
//! Keeping the adapter shared ensures CI checks the same inputs as the exe bench.
use h4_rules::effects::{ImmunityFilters, ResistanceGate};

/// Owns parsed lists so the rule inputs can borrow the same values for every filter.
pub struct Case {
    flags: u32,
    caster: Option<Vec<u8>>,
    target: Vec<u8>,
    effects: Vec<u8>,
    hostile: bool,
    same: bool,
    resistance: i32,
}

impl Case {
    pub fn gate(&self) -> ResistanceGate<'_> {
        ResistanceGate {
            immunities: ImmunityFilters {
                flags: self.flags,
                caster_abilities: self.caster.as_deref(),
                target_abilities: &self.target,
                target_effects: &self.effects,
            },
            hostile: self.hostile,
            same_side: self.same,
            resistance: self.resistance,
        }
    }
}

pub fn parse(fields: &[&str]) -> Result<Case, Box<dyn std::error::Error>> {
    let [flags, caster, target, effects, hostile, same, resistance] = fields else {
        return Err("invalid immunity fields".into());
    };
    fn list(value: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if value == "-" {
            return Ok(Vec::new());
        }
        value.split(',').map(|s| Ok(s.parse()?)).collect()
    }
    Ok(Case {
        flags: flags.parse()?,
        caster: if *caster == "none" {
            None
        } else {
            Some(list(caster)?)
        },
        target: list(target)?,
        effects: list(effects)?,
        hostile: hostile.parse::<u8>()? != 0,
        same: same.parse::<u8>()? != 0,
        resistance: resistance.parse()?,
    })
}

pub fn evaluate(fields: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let case = parse(fields)?;
    let gate = case.gate();
    let input = gate.immunities;
    Ok([
        input.allows_general(),
        input.allows_target_kind(),
        input.allows_effect_98(),
        gate.allows(),
    ]
    .map(|b| u8::from(b).to_string())
    .join(" "))
}
