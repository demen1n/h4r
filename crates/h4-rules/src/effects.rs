//! Effect immunity/resistance filters and effectiveness from the original exe.
//! This is a partial eligibility pipeline: effect records, side/refresh/artifact
//! filters and the target resistance accessor are not yet connected to combat.
//! [`StoneGazeContext`] assembles damage inputs once the caller resolves those checks.

use crate::damage::ability;

/// Effect ids read from the exe keyword pairs at VA 0x994ac0.
/// These are spell/effect ids, a different namespace from creature abilities.
pub mod effect {
    pub const ANTI_MAGIC: u8 = 0x03;
    pub const BLIND: u8 = 0x0b;
    pub const COLD_RESISTANCE: u8 = 0x14;
    pub const FIRE_RESISTANCE: u8 = 0x28;
    pub const SANCTUARY: u8 = 0x89;
    pub const STONE_GAZE: u8 = 0x98;
}

/// Bits of an effect record's +0x3c word, named by their confirmed use here.
/// Some bits participate in other filters too; these names are not a full flag enum.
pub mod flag {
    pub const AFFECTS_ELEMENTALS: u32 = 0x02;
    pub const AFFECTS_MECHANICAL: u32 = 0x10;
    pub const AFFECTS_UNDEAD: u32 = 0x20;
    pub const COLD: u32 = 0x0400;
    /// Includes the full-resistance gate even without RESISTIBLE. Its wider meaning
    /// is unresolved (it also participates in the caster-effect-0x90 filter).
    pub const CHECK_FULL_RESISTANCE: u32 = 0x2000;
    pub const BLOCKED_BY_BERSERK: u32 = 0x4000;
    pub const FIRE: u32 = 0x8000;
    /// Enables the caster's negate_mind_immunity exception to target-kind checks.
    pub const NEGATE_MIND_IMMUNITY_APPLIES: u32 = 0x200000;
    pub const RESISTIBLE: u32 = 0x800000;
    pub const RESISTIBLE_BETWEEN_SIDES: u32 = 0x1000000;
}

/// Inputs supplied by the effect record and combat participants.
#[derive(Debug, Clone, Copy)]
pub struct Effectiveness {
    /// All target-eligibility checks passed, including filters outside this module.
    /// A rejected effect must stay at zero even if the caster ignores resistance.
    pub eligible: bool,
    /// Whether the record has [`flag::RESISTIBLE`].
    pub resisted: bool,
    /// Whether the record has [`flag::RESISTIBLE_BETWEEN_SIDES`].
    pub resisted_between_sides: bool,
    /// Equality of the two side ids, not friendliness or alliance.
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
        if !(self.resisted || (self.resisted_between_sides && !self.same_side))
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
        let has_effect = |id| self.target_effects.contains(&id);
        // Vulnerability bypasses these immunities, but not every eligibility filter.
        if has_ability(ability::SPELL_VULNERABILITY) {
            return true;
        }
        !(has_ability(ability::MAGIC_IMMUNITY)
            || has_effect(effect::SANCTUARY)
            || has_effect(effect::ANTI_MAGIC)
            || ((self.flags & flag::FIRE != 0)
                && (has_effect(effect::FIRE_RESISTANCE) || has_ability(ability::FIRE_RESISTANCE)))
            || ((self.flags & flag::COLD != 0)
                && (has_effect(effect::COLD_RESISTANCE) || has_ability(ability::COLD_RESISTANCE)))
            || ((self.flags & flag::BLOCKED_BY_BERSERK != 0) && has_ability(ability::BERSERK)))
    }

    /// Target-kind immunities and the caster's negate-mind-immunity exception (1b3a10).
    pub fn allows_target_kind(self) -> bool {
        let has_ability = |id| self.target_abilities.contains(&id);
        if has_ability(ability::SPELL_VULNERABILITY)
            || ((self.flags & flag::NEGATE_MIND_IMMUNITY_APPLIES != 0)
                && self
                    .caster_abilities
                    .is_some_and(|a| a.contains(&ability::NEGATE_MIND_IMMUNITY)))
        {
            return true;
        }
        !(((self.flags & flag::AFFECTS_MECHANICAL == 0) && has_ability(ability::MECHANICAL))
            || ((self.flags & flag::AFFECTS_UNDEAD == 0) && has_ability(ability::UNDEAD))
            || ((self.flags & flag::AFFECTS_ELEMENTALS == 0) && has_ability(ability::ELEMENTAL)))
    }

    /// The stone-gaze branch of 1b4770 (also shared by the blind spell).
    /// Blind targets cannot be affected by the gaze, unless they have spell vulnerability.
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
    /// Result of the combat hostility predicate; may differ from side-id inequality.
    pub hostile: bool,
    /// Equality of the two side ids, not friendliness or alliance.
    pub same_side: bool,
    /// Target virtual slot +0x7c(effect id), including all of its bonuses.
    pub resistance: i32,
}

impl ResistanceGate<'_> {
    pub fn allows(self) -> bool {
        let input = self.immunities;
        // Non-hostile targets are exempt here; immunity filters still apply separately.
        if (input.caster_abilities.is_some() && !self.hostile)
            || input
                .target_abilities
                .contains(&ability::SPELL_VULNERABILITY)
            || (input
                .caster_abilities
                .is_some_and(|a| a.contains(&ability::IGNORE_MAGIC_RESISTANCE))
                && !input.target_abilities.contains(&ability::MAGIC_IMMUNITY))
        {
            return true;
        }
        let applies = (input.flags & (flag::RESISTIBLE | flag::CHECK_FULL_RESISTANCE) != 0)
            || ((input.flags & flag::RESISTIBLE_BETWEEN_SIDES != 0)
                && input.caster_abilities.is_some()
                && !self.same_side);
        !applies || self.resistance < 100
    }
}

/// Resolves the spell-rule inputs needed by damage's stone-gaze bonus.
///
/// The four implemented filters and effectiveness share one set of abilities, flags
/// and resistance values, so a caller cannot accidentally resolve them differently.
/// This is not the full spell system: callers must supply the remaining checks.
#[derive(Debug, Clone, Copy)]
pub struct StoneGazeContext<'a> {
    /// Flags, abilities, active effects, sides and resistance for the stone-gaze record.
    pub resistance_gate: ResistanceGate<'a>,
    /// Result of all checks not implemented here (target state, artifacts, side/caster
    /// restrictions and refresh). None means unknown; never assume these checks pass.
    pub remaining_eligibility: Option<bool>,
    /// Target resistance queried with effect id -1, used by ignore_wards.
    pub generic_resistance: i32,
    /// Whether the target can be cast to a combat creature (as opposed to a wall).
    pub is_creature: bool,
    /// Target's virtual stone-gaze power (+0x90), used by finalization.
    pub power_percent: i32,
    /// HP of one target creature, not the remaining HP of its whole stack.
    pub creature_hit_points: i32,
}

impl StoneGazeContext<'_> {
    /// None keeps the damage API's "unknown: no bonus" behavior. An absent caster
    /// also leaves the result unresolved: stone gaze always needs an attacking stack.
    pub fn resolve(self) -> Option<crate::damage::Ability42Target> {
        let remaining_allowed = self.remaining_eligibility?;
        let input = self.resistance_gate.immunities;
        let caster = input.caster_abilities?;
        let can_affect = remaining_allowed
            && input.allows_general()
            && input.allows_target_kind()
            && input.allows_effect_98()
            && self.resistance_gate.allows();
        // Eligibility uses effect-specific resistance even with ignore_wards; only
        // the effectiveness helper switches to generic resistance afterwards.
        let effectiveness = Effectiveness {
            eligible: can_affect,
            resisted: input.flags & flag::RESISTIBLE != 0,
            resisted_between_sides: input.flags & flag::RESISTIBLE_BETWEEN_SIDES != 0,
            same_side: self.resistance_gate.same_side,
            bypass_resistance: caster.contains(&ability::IGNORE_MAGIC_RESISTANCE),
            generic_resistance_only: caster.contains(&ability::IGNORE_WARDS),
            effect_resistance: self.resistance_gate.resistance,
            generic_resistance: self.generic_resistance,
        };
        Some(crate::damage::Ability42Target {
            is_creature: self.is_creature,
            can_affect,
            effectiveness_percent: effectiveness.percent(),
            power_percent: self.power_percent,
            creature_hit_points: self.creature_hit_points,
        })
    }
}
