//! Open-field turn selection, wait and defense (Buka 3.0).
//! Effective stats are inputs: resolving spells, artifacts and hero stats is
//! separate from scheduling. Native list order is retained for same-side ties.
use super::{state::UnitId, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitiativeStats {
    pub speed: i32,
    /// Resolved morale, after effects/immunities, in -10..=10.
    pub morale: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Morale {
    Bad,
    Normal,
    Good,
}

/// Native 1f32c0: positive morale moves earlier, negative morale later.
/// There is no guessed extra action or skipped turn here.
pub fn morale_result(morale: i32, roll: u8) -> Result<Morale, String> {
    if !(-10..=10).contains(&morale) || roll > 9 {
        return Err("morale needs -10..10 and a roll in 0..9".into());
    }
    Ok(if 9 - i32::from(roll) < -morale {
        Morale::Bad
    } else if i32::from(roll) < morale {
        Morale::Good
    } else {
        Morale::Normal
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInitiative {
    pub unit: UnitId,
    pub side: Side,
    pub stats: InitiativeStats,
    pub morale_roll: u8,
    pub waiting: bool,
    pub defending: bool,
}

impl UnitInitiative {
    pub fn morale(&self) -> Morale {
        morale_result(self.stats.morale, self.morale_roll).expect("validated initiative inputs")
    }

    /// Native 1f2cc0; wrapping operations preserve the x86 integer semantics.
    pub fn priority(&self) -> i32 {
        let morale = self.morale();
        let mut speed = self.stats.speed;
        if morale != Morale::Bad {
            speed = speed.wrapping_add(1000);
        }
        if morale == Morale::Good {
            speed = speed.wrapping_add(1000);
        }
        if self.waiting {
            speed = speed.wrapping_neg();
        }
        speed
    }

    /// Native 1f2d30. Same-side ties keep the first entry in battlefield order.
    pub fn moves_before(&self, other: &Self, last_side: Side) -> bool {
        let difference = self.priority().wrapping_sub(other.priority());
        difference > 0 || (difference == 0 && self.side != other.side && self.side != last_side)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundInitiative {
    /// Initialized to defender by native battlefield constructor 16102f.
    pub last_side: Side,
    pub entries: Vec<UnitInitiative>,
}

impl RoundInitiative {
    /// Scan rather than sort: native 16f250 dynamically selects one entry.
    pub fn next(&self, eligible: impl Fn(UnitId) -> bool) -> Option<UnitId> {
        let mut best: Option<&UnitInitiative> = None;
        for entry in self.entries.iter().filter(|entry| eligible(entry.unit)) {
            if best.is_none_or(|old| entry.moves_before(old, self.last_side)) {
                best = Some(entry);
            }
        }
        best.map(|entry| entry.unit)
    }

    pub(crate) fn entry_mut(&mut self, unit: UnitId) -> Result<&mut UnitInitiative, String> {
        self.entries
            .iter_mut()
            .find(|entry| entry.unit == unit)
            .ok_or("missing initiative unit".into())
    }
}

/// The stance component of native defense bonus getter 1f1a80. Other defense
/// modifiers are resolved elsewhere. Effect 0x1f is an explicit caller input.
pub fn defending_defense(defense: f32, effect_1f: bool) -> f32 {
    let defense = defense * 1.25;
    if effect_1f {
        defense * 1.5
    } else {
        defense
    }
}
