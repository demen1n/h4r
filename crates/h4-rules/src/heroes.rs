//! Hero classes and how a hero looks on the adventure map.

use crate::creatures::TOWN_TYPES;

/// One of the eleven base hero classes (records at VA `0x989440`: id, keyword, alignment,
/// primary skill...). The higher ids are the advanced classes heroes grow into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeroClass {
    pub keyword: &'static str,
    /// Index into [`TOWN_TYPES`]; the barbarian is "might" (stronghold).
    pub alignment: usize,
    /// Drawn with the spellcaster model. The exe keeps a list of such classes at VA
    /// `0x989a54` (rva `337c50`); among the base classes it is the odd ids.
    pub spellcaster: bool,
}

/// Base classes in the game's order (ids 0..=10, as stored in maps).
pub const BASE_CLASSES: [HeroClass; 11] = [
    HeroClass { keyword: "knight", alignment: 0, spellcaster: false },
    HeroClass { keyword: "priest", alignment: 0, spellcaster: true },
    HeroClass { keyword: "lord", alignment: 1, spellcaster: false },
    HeroClass { keyword: "mage", alignment: 1, spellcaster: true },
    HeroClass { keyword: "death_knight", alignment: 2, spellcaster: false },
    HeroClass { keyword: "necromancer", alignment: 2, spellcaster: true },
    HeroClass { keyword: "thief", alignment: 3, spellcaster: false },
    HeroClass { keyword: "sorcerer", alignment: 3, spellcaster: true },
    HeroClass { keyword: "archer", alignment: 4, spellcaster: false },
    HeroClass { keyword: "druid", alignment: 4, spellcaster: true },
    HeroClass { keyword: "barbarian", alignment: 5, spellcaster: false },
];

/// Alignment names in hero model names (VA `0x996634`), in [`TOWN_TYPES`] order.
const MODEL_ALIGNMENTS: [&str; 6] = ["life", "order", "death", "chaos", "nature", "might"];

/// The `adv_actor` of a hero of a base class: `hero.<alignment>_<might|magic>_<male|female>`
/// (rva `2ad50` builds it, `737ca0` the part after `hero.`). The game passes the hero's
/// alignment, `gender == 1` as "male" and the spellcaster flag (call at rva `32e18d`). `None` for
/// a class or gender the map leaves random (-1) or that isn't a base class.
pub fn adv_model_name(class: i8, gender: i8) -> Option<String> {
    let c = BASE_CLASSES.get(usize::try_from(class).ok()?)?;
    let sex = match gender {
        0 => "female",
        1 => "male",
        _ => return None,
    };
    let kind = if c.spellcaster { "magic" } else { "might" };
    debug_assert_eq!(TOWN_TYPES.len(), MODEL_ALIGNMENTS.len());
    Some(format!("hero.{}_{kind}_{sex}", MODEL_ALIGNMENTS[c.alignment]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert_eq!(adv_model_name(0, 1).as_deref(), Some("hero.life_might_male"));
        assert_eq!(adv_model_name(7, 0).as_deref(), Some("hero.chaos_magic_female"));
        assert_eq!(adv_model_name(10, 1).as_deref(), Some("hero.might_might_male"));
        assert_eq!(adv_model_name(-1, 1), None);
        assert_eq!(adv_model_name(3, -1), None);
    }
}
