//! Effect effectiveness after eligibility (RVA 1f1de0).
//! Eligibility and the target resistance accessor still belong to future spell rules.

/// Inputs supplied by the effect record and combat participants.
#[derive(Debug, Clone, Copy)]
pub struct Effectiveness {
    pub eligible: bool,
    /// Effect record flag 0x800000.
    pub resisted: bool,
    /// Effect record flag 0x1000000: resistance applies between different sides.
    pub resisted_between_sides: bool,
    pub same_side: bool,
    /// Attacker ability 0x52, checked before 0x1f.
    pub bypass_resistance: bool,
    /// Attacker ability 0x1f: query resistance with effect id -1.
    pub generic_resistance_only: bool,
    /// Target virtual slot +0x7c(effect id).
    pub effect_resistance: i32,
    /// Target virtual slot +0x7c(-1).
    pub generic_resistance: i32,
}

impl Effectiveness {
    /// Percentage used by effect arithmetic. The exe does not clamp this value.
    pub fn percent(self) -> i32 {
        if !self.eligible {
            return 0;
        }
        if !(self.resisted || self.resisted_between_sides && !self.same_side)
            || self.bypass_resistance
        {
            return 100;
        }
        100_i32.wrapping_sub(if self.generic_resistance_only {
            self.generic_resistance
        } else {
            self.effect_resistance
        })
    }
}
