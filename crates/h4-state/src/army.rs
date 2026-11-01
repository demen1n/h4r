//! Armies: up to seven slots, each holding a stack of creatures or a hero.
//!
//! In Heroes IV a hero is not a commander standing beside the army but one of its seven
//! members, fighting in battle like a creature. An army without heroes can walk the map
//! too (wandering monsters, or troops sent off on their own).

use crate::{Color, Pos};
use serde::{Deserialize, Serialize};

/// How many members an army has room for.
pub const SLOTS: usize = 7;

/// Identifies an army on the map for the whole game (see `GameState::next_army_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArmyId(pub u32);

/// A number of creatures of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    /// Creature id: index into the game's creature enum (`h4_rules::creatures::KEYWORDS`).
    pub creature: usize,
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    /// Portrait id, which also gives the default name and biography; -1 if none was set.
    pub portrait: i16,
    /// The map's own name for the hero; empty means the default for the portrait.
    pub name: String,
    /// Base class, index into `h4_rules::heroes::BASE_CLASSES`.
    pub class: usize,
    pub male: bool,
    /// Level and experience. The map stores them in fields we don't understand yet, so
    /// for now every hero starts at level 1 with no experience.
    pub level: u32,
    pub experience: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Slot {
    Creatures(Stack),
    Hero(Hero),
}

/// Seven places, some of them empty. The position matters: it is the order in the army
/// window and the start position in battle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Army {
    pub slots: [Option<Slot>; SLOTS],
}

impl Army {
    /// An army of these creature stacks in the first slots (at most seven are used).
    pub fn of(stacks: &[Stack]) -> Army {
        let mut army = Army::default();
        for (slot, s) in army.slots.iter_mut().zip(stacks) {
            *slot = Some(Slot::Creatures(*s));
        }
        army
    }

    /// No one left: every slot is empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    pub fn heroes(&self) -> impl Iterator<Item = &Hero> {
        self.slots.iter().filter_map(|s| match s {
            Some(Slot::Hero(h)) => Some(h),
            _ => None,
        })
    }

    pub fn stacks(&self) -> impl Iterator<Item = &Stack> {
        self.slots.iter().filter_map(|s| match s {
            Some(Slot::Creatures(c)) => Some(c),
            _ => None,
        })
    }
}

/// An army standing on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct MapArmy {
    pub id: ArmyId,
    /// The player it belongs to; `None` for neutral monsters.
    pub owner: Option<Color>,
    pub at: Pos,
    pub army: Army,
    /// Movement left this turn, per slot, each in the slot's own units: the game keeps a
    /// float per stack (`+0x70`), full is the stack's raw movement, and the army's
    /// movement is worked out from all of them (`h4_rules::movement`). Filled in by the
    /// rules at the start of each day; 0 for empty slots.
    pub movement_left: [f32; SLOTS],
    /// Daily growth of a neutral stack not yet worth a whole creature, in 1/256 of one
    /// (the game's `+0xa4` of the stack), per slot.
    pub growth_remainder: [i32; SLOTS],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_of_an_army() {
        let mut army = Army::of(&[Stack { creature: 3, count: 10 }, Stack { creature: 4, count: 2 }]);
        assert_eq!(army.stacks().count(), 2);
        assert_eq!(army.heroes().count(), 0);
        army.slots[6] = Some(Slot::Hero(Hero {
            portrait: -1,
            name: String::new(),
            class: 0,
            male: true,
            level: 1,
            experience: 0,
        }));
        assert_eq!(army.heroes().count(), 1);
        army.slots = Default::default();
        assert!(army.is_empty());
    }
}
