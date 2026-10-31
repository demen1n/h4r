//! Effect immunity/resistance filters and effectiveness from the original exe.
//! This is a partial eligibility pipeline: effect records, side/refresh/artifact
//! filters and the target resistance accessor are not yet connected to combat.

use crate::damage::ability;

/// Inputs supplied by the effect record and combat participants.
#[derive(Debug, Clone, Copy)]
pub struct Effectiveness {
    pub eligible: bool,
    /// Effect record flag 0x800000.
    pub resisted: bool,
    /// Effect record flag 0x1000000: resistance applies between different sides.
    pub resisted_between_sides: bool,
    pub same_side: bool,
    /// Attacker [`ability::IGNORE_MAGIC_RESISTANCE`], checked before ignore wards.
    pub bypass_resistance: bool,
    /// Attacker [`ability::IGNORE_WARDS`]: query resistance with effect id -1.
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

/// Confirmed immunity filters from RVA 1b3540, 1b3a10 and the 0x98 branch
/// of 1b4770. These are only part of the full 1b30a0 eligibility pipeline.
#[derive(Debug, Clone, Copy)]
pub struct ImmunityFilters<'a> {
    /// Effect-record flags (+0x3c); passed explicitly until records are loaded.
    pub flags: u32,
    /// None represents an effect without a caster.
    pub caster_abilities: Option<&'a [u8]>,
    pub target_abilities: &'a [u8],
    /// Active effect ids, as queried through the target's virtual slot +8.
    pub target_effects: &'a [u8],
}

impl ImmunityFilters<'_> {
    /// General ability/effect immunities (1b3540).
    pub fn allows_general(self) -> bool {
        let has_ability = |id| self.target_abilities.contains(&id);
        let effect = |id| self.target_effects.contains(&id);
        if has_ability(ability::SPELL_VULNERABILITY) {
            return true;
        }
        !(has_ability(ability::MAGIC_IMMUNITY)
            || effect(0x89)
            || effect(0x03)
            || self.flags & 0x8000 != 0 && (effect(0x28) || has_ability(ability::FIRE_RESISTANCE))
            || self.flags & 0x0400 != 0 && (effect(0x14) || has_ability(ability::COLD_RESISTANCE))
            || self.flags & 0x4000 != 0 && has_ability(ability::BERSERK))
    }

    /// Target-kind immunities and the caster's negate-mind-immunity exception (1b3a10).
    pub fn allows_target_kind(self) -> bool {
        let has_ability = |id| self.target_abilities.contains(&id);
        if has_ability(ability::SPELL_VULNERABILITY)
            || self.flags & 0x200000 != 0
                && self
                    .caster_abilities
                    .is_some_and(|a| a.contains(&ability::NEGATE_MIND_IMMUNITY))
        {
            return true;
        }
        !(self.flags & 0x10 == 0 && has_ability(ability::MECHANICAL)
            || self.flags & 0x20 == 0 && has_ability(ability::UNDEAD)
            || self.flags & 0x02 == 0 && has_ability(ability::ELEMENTAL))
    }

    /// The effect-0x98 branch of 1b4770 (also shared by effect 0x0b).
    pub fn allows_effect_98(self) -> bool {
        !self.target_abilities.contains(&ability::BLIND)
            || self
                .target_abilities
                .contains(&ability::SPELL_VULNERABILITY)
    }
}

/// Resistance gate in eligibility (1b5750), before effectiveness is calculated.
/// Hostility and side equality are separate: allied different sides may be non-hostile.
#[derive(Debug, Clone, Copy)]
pub struct ResistanceGate<'a> {
    pub immunities: ImmunityFilters<'a>,
    pub hostile: bool,
    pub same_side: bool,
    /// Target virtual slot +0x7c(effect id), including all of its bonuses.
    pub resistance: i32,
}

impl ResistanceGate<'_> {
    pub fn allows(self) -> bool {
        let input = self.immunities;
        if input.caster_abilities.is_some() && !self.hostile
            || input
                .target_abilities
                .contains(&ability::SPELL_VULNERABILITY)
            || input
                .caster_abilities
                .is_some_and(|a| a.contains(&ability::IGNORE_MAGIC_RESISTANCE))
                && !input.target_abilities.contains(&ability::MAGIC_IMMUNITY)
        {
            return true;
        }
        let applies = input.flags & (0x800000 | 0x2000) != 0
            || input.flags & 0x1000000 != 0 && input.caster_abilities.is_some() && !self.same_side;
        !applies || self.resistance < 100
    }
}
