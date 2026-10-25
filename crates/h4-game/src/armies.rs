//! Armies standing on a real map: wandering monsters and heroes (`ObjectKind::Army`).
//!
//! An army is drawn as one animated figure, like in the game: a hero if it has one,
//! otherwise its first creature stack. Random monsters are armies too, once
//! `h4_rules::placement` has chosen their creatures.

use std::collections::HashMap;

use h4_formats::archive::Resources;
use h4_formats::map::MapFile;
use h4_formats::map_army::Stack;
use h4_formats::map_body::ObjectKind;
use h4_rules::creatures::KEYWORDS;
use h4_rules::heroes;
use h4_rules::placement::Placement;

use crate::world::{Pos, World};

/// One army to put on the map: which `adv_actor` to animate and where.
pub struct MapArmy {
    pub actor: String,
    pub at: Pos,
}

/// The armies of `level`, and the random monsters that became armies (`placements`, one
/// entry per map object). Their tiles are marked taken and blocked, so the hero walks
/// around them (in the game stepping on one starts a fight, which we don't have yet).
pub fn map_armies(
    res: &Resources,
    world: &mut World,
    map: &MapFile,
    placements: &[Option<Placement>],
    level: usize,
    hero: &str,
) -> Vec<MapArmy> {
    let Ok(body) = &map.body else { return Vec::new() };
    let actors = actor_names(res);
    let mut out = Vec::new();
    for (i, o) in body.objects.iter().enumerate() {
        if o.level != level as i32 {
            continue;
        }
        // the hero leading the army, if there is one, else the first creature
        let (leader, creature) = match (&o.kind, placements.get(i)) {
            (ObjectKind::Army { army, .. }, _) => {
                let leader = army.stacks.iter().find_map(|s| match s {
                    Stack::Hero(h) => Some(h),
                    _ => None,
                });
                let creature = army.stacks.iter().find_map(|s| match s {
                    Stack::Creatures { kind, .. } => KEYWORDS.get(*kind as usize),
                    _ => None,
                });
                (leader, creature)
            }
            (ObjectKind::Placed(_), Some(Some(Placement::Monster { stacks }))) => {
                (None, stacks.first().and_then(|&(c, _)| KEYWORDS.get(c)))
            }
            _ => continue,
        };
        let actor = if let Some(h) = leader {
            // A hero the map leaves random got a class and gender in `placement`; one with
            // a portrait but no class keeps the default model for now.
            let chosen = match placements.get(i) {
                Some(Some(Placement::Army { heroes, .. })) => heroes.first().copied().flatten(),
                _ => None,
            };
            let name = match chosen {
                Some((class, male)) => heroes::adv_model_name(class as i8, male as i8),
                None => heroes::adv_model_name(h.class, h.gender),
            };
            Some(name.unwrap_or_else(|| hero.to_string()))
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
