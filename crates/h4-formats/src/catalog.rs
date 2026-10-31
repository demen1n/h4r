//! Index of actor animations: `actor_sequence.<actor>.[combat.]<action>.<direction>.h4d`.

use std::collections::BTreeMap;

use crate::archive::Resources;
use crate::combat_actor::CombatActorModel;

/// Directions in clockwise order, starting north.
pub const DIRECTIONS: [&str; 8] = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];

#[derive(Debug, Default)]
pub struct Actor {
    /// Spelling as found in the first resource (names mix "Beholder" and "beholder").
    pub name: String,
    /// action -> direction -> resource name
    pub actions: BTreeMap<String, BTreeMap<&'static str, String>>,
}

impl Actor {
    /// Use definition references rather than guessing combat actions from filenames:
    /// several directions deliberately share one sequence.
    pub fn from_combat_model(name: String, model: &CombatActorModel) -> Self {
        let mut actor = Actor {
            name,
            actions: BTreeMap::new(),
        };
        for action in &model.actions {
            // These need the game's composite cache, not a single animation file.
            if action.is_composite() {
                continue;
            }
            let mut dirs = BTreeMap::new();
            for (dir, sequence) in crate::adv_actor::DIRECTIONS.iter().zip(&action.sequences) {
                if !sequence.is_empty() {
                    dirs.insert(*dir, format!("actor_sequence.{sequence}.h4d"));
                }
            }
            if !dirs.is_empty() {
                actor.actions.insert(action.name.clone(), dirs);
            }
        }
        actor
    }

    pub fn action_names(&self) -> Vec<&str> {
        self.actions.keys().map(String::as_str).collect()
    }

    /// Directions available for an action, in clockwise order.
    pub fn directions(&self, action: &str) -> Vec<&'static str> {
        let Some(dirs) = self.actions.get(action) else { return Vec::new() };
        DIRECTIONS.iter().copied().filter(|d| dirs.contains_key(d)).collect()
    }
}

/// All actors keyed by lowercase name.
pub fn actors(res: &Resources) -> BTreeMap<String, Actor> {
    let mut out: BTreeMap<String, Actor> = BTreeMap::new();
    for name in res.names() {
        let lower = name.to_lowercase();
        let Some(rest) = lower.strip_prefix("actor_sequence.").and_then(|s| s.strip_suffix(".h4d")) else {
            continue;
        };
        let parts: Vec<&str> = rest.split('.').collect();
        // heroes are "hero.<class>.<action>...": the actor name takes two parts
        let actor_parts = if parts[0] == "hero" { 2 } else { 1 };
        if parts.len() < actor_parts + 2 {
            continue;
        }
        let Some(&dir) = DIRECTIONS.iter().find(|d| **d == parts[parts.len() - 1]) else {
            continue;
        };
        let action = parts[actor_parts..parts.len() - 1].join(".");
        let actor = out.entry(parts[..actor_parts].join(".")).or_default();
        if actor.name.is_empty() {
            let original: Vec<&str> = name["actor_sequence.".len()..].split('.').collect();
            actor.name = original[..actor_parts].join(".");
        }
        actor.actions.entry(action).or_default().insert(dir, name.to_string());
    }
    out
}
