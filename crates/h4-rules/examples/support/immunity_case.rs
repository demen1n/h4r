use h4_rules::effects::{ImmunityFilters, ResistanceGate};

pub fn evaluate(fields: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let [flags, caster, target, effects, hostile, same, resistance] = fields else {
        return Err("invalid immunity fields".into());
    };
    fn list(value: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if value == "-" || value == "none" {
            return Ok(Vec::new());
        }
        value.split(',').map(|s| Ok(s.parse()?)).collect()
    }
    let caster_values = list(caster)?;
    let target_values = list(target)?;
    let effects_values = list(effects)?;
    let input = ImmunityFilters {
        flags: flags.parse()?,
        caster_abilities: if *caster == "none" {
            None
        } else {
            Some(&caster_values)
        },
        target_abilities: &target_values,
        target_effects: &effects_values,
    };
    let gate = ResistanceGate {
        immunities: input,
        hostile: hostile.parse::<u8>()? != 0,
        same_side: same.parse::<u8>()? != 0,
        resistance: resistance.parse()?,
    };
    Ok([
        input.allows_general(),
        input.allows_target_kind(),
        input.allows_effect_98(),
        gate.allows(),
    ]
    .map(|b| u8::from(b).to_string())
    .join(" "))
}
