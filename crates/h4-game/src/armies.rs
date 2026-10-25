//! How an army of the game state looks on the map.
//!
//! An army is drawn as one animated figure, like in the game: a hero if it has one,
//! otherwise its first creature stack.

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_rules::creatures::KEYWORDS;
use h4_rules::heroes;
use h4_state::army::Army;

/// Finds the `adv_actor` that shows an army.
pub struct ArmyLooks {
    /// Creature `adv_actor` names by their normalized form (see [`normalize`]).
    creatures: HashMap<String, String>,
}

impl ArmyLooks {
    pub fn new(res: &Resources) -> ArmyLooks {
        let creatures = res
            .names()
            .filter_map(|n| n.strip_prefix("adv_actor.")?.strip_suffix(".h4d"))
            .filter(|n| !n.starts_with("hero."))
            .map(|n| (normalize(n), n.to_string()))
            .collect();
        ArmyLooks { creatures }
    }

    /// The `adv_actor` of the army's leader: its first hero, or else its first creature
    /// stack. `None` for an empty army or a creature without a model.
    pub fn actor(&self, army: &Army) -> Option<String> {
        if let Some(h) = army.heroes().next() {
            // the class and gender are already decided (random ones by `placement`)
            return heroes::adv_model_name(h.class as i8, h.male as i8);
        }
        let creature = KEYWORDS.get(army.stacks().next()?.creature)?;
        self.creatures.get(&normalize(creature)).cloned()
    }

    /// The creature id whose keyword matches the `adv_actor` name, for armies made up by
    /// hand on the test map.
    pub fn creature_of(actor: &str) -> Option<usize> {
        KEYWORDS.iter().position(|k| normalize(k) == normalize(actor))
    }
}

/// Lower case without spaces: the files are named inconsistently (`Crossbow Man`,
/// `black dragon`, `Ogre Mage`), the keywords are lower case; this way they match.
fn normalize(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_matches_file_names_to_keywords() {
        assert_eq!(normalize("Crossbow Man"), normalize("crossbowman"));
        assert_eq!(normalize("black dragon"), normalize("Black Dragon"));
        assert_eq!(ArmyLooks::creature_of("Black Dragon"), Some(6));
    }
}
