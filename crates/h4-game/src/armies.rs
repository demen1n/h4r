//! Armies standing on a real map: wandering monsters and heroes (`ObjectKind::Army`).
//!
//! An army is drawn as one animated figure, like in the game: a hero if it has one,
//! otherwise its first creature stack.

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;

use crate::world::{Pos, World};

/// Creature keywords in the order of the game's creature enum, so `CREATURES[kind]` names
/// the creature a map stack stores as `kind`. Taken from the exe's keyword table at VA
/// `0x985574` (pairs of id + name, ids 0..73). Checked on all installed maps: with this
/// order high-level creatures come in small stacks and low-level ones in big stacks
/// (correlation of level with log(count) −0.80; reading the pairs the other way gives 0.03).
const CREATURES: [&str; 74] = [
    "air elemental", "archangel", "ballista", "bandit", "behemoth", "beholder", "black dragon",
    "bone dragon", "centaur", "cerberus", "champion", "crossbowman", "crusader", "cyclops",
    "venom spawn", "archdevil", "dragon golem", "dwarf", "earth elemental", "efreet", "elf",
    "faerie dragon", "fire elemental", "gargoyle", "genie", "ghost", "berserker", "gold golem",
    "griffin", "halfling", "harpy", "hydra", "ice demon", "imp", "leprechaun", "mage", "mantis",
    "medusa", "mermaid", "minotaur", "monk", "mummy", "naga", "nightmare", "nomad", "ogre mage",
    "orc", "peasant", "phoenix", "pikeman", "pirate", "satyr", "sea monster", "skeleton",
    "squire", "sprite", "thunderbird", "titan", "troglodyte", "troll", "unicorn", "vampire",
    "water elemental", "white tiger", "wolf", "zombie", "waspwort", "goblin knight",
    "evil sorceress", "gargantuan", "dark champion", "catapult", "frenzied gnasher",
    "mega dragon",
];

/// One army to put on the map: which `adv_actor` to animate and where.
pub struct MapArmy {
    pub actor: String,
    pub at: Pos,
}

/// The armies of `level`. Their tiles are marked taken and blocked, so the hero walks
/// around them (in the game stepping on one starts a fight, which we don't have yet).
pub fn map_armies(res: &Resources, world: &mut World, map: &MapFile, level: usize, hero: &str) -> Vec<MapArmy> {
    let Ok(body) = &map.body else { return Vec::new() };
    let actors = actor_names(res);
    let mut out = Vec::new();
    for o in &body.objects {
        let ObjectKind::Army { army, .. } = &o.kind else { continue };
        if o.level != level as i32 {
            continue;
        }
        let has_hero = army.stacks.iter().any(|s| matches!(s, Stack::Hero(_)));
        let creature = army.stacks.iter().find_map(|s| match s {
            Stack::Creatures { kind, .. } => CREATURES.get(*kind as usize),
            _ => None,
        });
        let actor = if has_hero {
            // TODO: pick the hero's own model by class and gender; one model for all for now
            Some(hero.to_string())
        } else {
            creature.and_then(|c| actors.get(&normalize(c)).cloned())
        };
        let Some(actor) = actor else {
            eprintln!("army at ({}, {}): no model for {creature:?}", o.x, o.y);
            continue;
        };
        // stored x is our row, y our column, as for the other objects
        let at = Pos { col: o.y, row: o.x };
        if world.contains(at) {
            world.take(at, true);
        }
        out.push(MapArmy { actor, at });
    }
    out
}

/// `adv_actor` names by their normalized form: the files are named inconsistently
/// (`Crossbow Man`, `black dragon`, `Ogre Mage`), the keywords are lower case.
fn actor_names(res: &Resources) -> HashMap<String, String> {
    res.names()
        .filter_map(|n| n.strip_prefix("adv_actor.")?.strip_suffix(".h4d"))
        .filter(|n| !n.starts_with("hero."))
        .map(|n| (normalize(n), n.to_string()))
        .collect()
}

/// Lower case without spaces: "Crossbow Man" and "crossbowman" become the same.
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
    }
}
