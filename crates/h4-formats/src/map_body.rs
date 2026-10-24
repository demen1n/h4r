//! The middle of a `.h4c` map: everything between the header and the tiles.
//!
//! Order as in `t_adventure_map::read_from_map` (rva `d15c0`):
//!
//! 1. global events ([`GlobalEvents`]): timed, triggerable, continuous, placed;
//! 2. twelve obelisk reward lists (rva `7c65f0`, one per obelisk color);
//! 3. per-player data (rva `d0820`);
//! 4. allowed artifacts, an unknown bool array, allowed skills, allowed spells
//!    (rva `d0a40`, `d0c10`, `858150`, `d0f00`), two more artifact sets (rva `551330`);
//! 5. AI importance of artifacts and spells (rva `147f30`, `45ea60` twice);
//! 6. the objects (rva `cf010`): u32 count, then position + kind + body for each.
//!
//! Every object type has its own `read_from_map` (vtable slot 36 of its class). The type is
//! stored by *name* (the `t_qualified_adv_object_type` before the body); the numeric ids
//! next to the names are from an older enum and don't match the game's, so we go by names.
//! The bodies are read as far as needed to get to the next object; most of the fields are
//! only checked, a few useful ones (owner, names, garrison) are kept.
//!
//! Only map versions 27 and 29 occur in the installation; branches for older versions are
//! written where the readers made them obvious, but have not been seen on data.

use crate::map_army::{read_army, read_artifact, read_hero, read_skill_set, read_spell_set, Army, Hero};
use crate::map_script::{
    read_built_in, read_global_events, read_owned_continuous, read_owned_events, read_owned_timed,
    read_owned_triggerable, read_script, read_versioned_boolean, text, zero_version, GlobalEvents,
    OwnedEvents,
};
use crate::reader::{bad, Reader};
use crate::Result;

/// What lies between the header and the tiles.
#[derive(Debug, Clone)]
pub struct MapBody {
    pub events: GlobalEvents,
    pub objects: Vec<MapObject>,
}

/// One object on the map.
#[derive(Debug, Clone)]
pub struct MapObject {
    /// Position as stored (tile coordinates; which axis is which on screen is still to be
    /// checked against the game) and level (0 surface, 1 underground).
    pub x: i32,
    pub y: i32,
    pub level: i32,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone)]
pub enum ObjectKind {
    /// A creature army or hero walking on the map (`t_army`).
    Army { owner: u8, army: Army, events: OwnedEvents },
    /// Anything placed from an `adv_object.*` model.
    Placed(Placed),
}

#[derive(Debug, Clone)]
pub struct Placed {
    /// Model name: the resource is `adv_object.<model>`.
    pub model: String,
    /// Type, major and minor subtype names, e.g. `decorative` / `tree` / `swamp`.
    pub type_name: String,
    pub major: String,
    pub minor: String,
    /// Owner color (0..5, 6 = nobody) for objects that store one.
    pub owner: Option<u8>,
    /// A name stored with the object: town name, gateway/teleporter id, sign text...
    pub name: Option<String>,
    /// Garrison of towns, mines, garrisons.
    pub garrison: Option<Army>,
    /// The hero of a prison.
    pub hero: Option<Hero>,
}

/// Read the body starting at `r.pos` (right after the header). `players` are the colors of
/// the header's player slots, `version` the map version.
pub(crate) fn read_body(r: &mut Reader, version: u16, players: &[u8]) -> Result<MapBody> {
    let events = read_global_events(r)?;
    for _ in 0..12 {
        read_obelisk(r)?;
    }
    read_player_data(r, version, players)?;
    read_allowed(r, version)?;
    read_ai_importance(r, version)?;
    let objects = read_objects(r)?;
    Ok(MapBody { events, objects })
}

/// One obelisk color (rva `7c65f0`): version 0, a flag, u16, and (if the flag is set) a
/// list of rewards. A reward (rva `7c70f0`) is version 0, a type name and its fields.
fn read_obelisk(r: &mut Reader) -> Result<()> {
    zero_version(r, "obelisk")?;
    let has_rewards = r.u8()? != 0;
    r.u16()?;
    if has_rewards {
        for _ in 0..r.u16()? {
            zero_version(r, "obelisk reward")?;
            let at = r.pos;
            match r.str16()?.as_str() {
                // artifact and u32 count
                "artifact" => {
                    read_artifact(r)?;
                    r.u32()?;
                }
                // material type and amount
                "material" => {
                    r.u8()?;
                    r.u32()?;
                }
                other => return bad(format!("obelisk reward {other:?} at {at:#x}")),
            }
        }
    }
    Ok(())
}

/// rva `d0820`, map version > 15: for each player slot u8 color, u8 flag (+ u16 if set),
/// and since version 23 one more byte.
fn read_player_data(r: &mut Reader, version: u16, players: &[u8]) -> Result<()> {
    if version <= 15 {
        return Ok(());
    }
    for _ in players {
        let at = r.pos;
        let color = r.u8()?;
        if !players.contains(&color) {
            return bad(format!("player data for color {color} that isn't on the map, at {at:#x}"));
        }
        if r.u8()? != 0 {
            r.u16()?;
        }
        if version >= 23 {
            r.u8()?;
        }
    }
    Ok(())
}

/// `t_artifact_set::read_impl` (rva `1516f0`): version 0, u16 bit count, the bits.
fn read_artifact_set(r: &mut Reader) -> Result<()> {
    zero_version(r, "artifact set")?;
    let bits = r.u16()? as usize;
    r.take(bits.div_ceil(8))?;
    Ok(())
}

/// Allowed artifacts, an unknown bool array, allowed skills and spells, two artifact sets.
fn read_allowed(r: &mut Reader, version: u16) -> Result<()> {
    if version > 17 {
        // allowed artifacts (rva d0a40)
        if r.u8()? != 0 {
            read_artifact_set(r)?;
        }
        // rva d0c10: a t_bool_array (u16 count, one byte each); seen once, 312 entries
        if r.u8()? != 0 {
            let n = r.u16()? as usize;
            r.take(n)?;
        }
    }
    // allowed skills (rva 858150), since version 18
    if version >= 18 && r.u8()? != 0 {
        read_skill_set(r)?;
    }
    // allowed spells (rva d0f00): u16 bit count (at most 188), the bits
    if version > 17 && r.u8()? != 0 {
        let at = r.pos;
        let bits = r.u16()? as usize;
        if bits > 188 {
            return bad(format!("{bits} allowed spells at {at:#x}"));
        }
        r.take(bits.div_ceil(8))?;
    }
    if version >= 22 {
        read_artifact_set(r)?;
        read_artifact_set(r)?;
    }
    Ok(())
}

/// Since version 23: AI importance of artifacts, then twice of spells. Each is version 0,
/// u16 count and (u16 id, u8 importance 1..2) pairs.
fn read_ai_importance(r: &mut Reader, version: u16) -> Result<()> {
    if version < 23 {
        return Ok(());
    }
    for (what, limit) in [("artifact", 248), ("spell", 189), ("spell", 189)] {
        zero_version(r, "AI importance map")?;
        for _ in 0..r.u16()? {
            let at = r.pos;
            let id = r.u16()?;
            let importance = r.u8()?;
            if id >= limit || !(1..=2).contains(&importance) {
                return bad(format!("AI importance of {what} {id}: {importance} at {at:#x}"));
            }
        }
    }
    Ok(())
}

/// The object list (rva `cf010`).
fn read_objects(r: &mut Reader) -> Result<Vec<MapObject>> {
    let count = r.u32()?;
    // each object takes at least 15 bytes; don't trust a huge count for the allocation
    let mut objects = Vec::with_capacity((count as usize).min(r.remaining() / 15));
    for _ in 0..count {
        let x = r.i32()?;
        let y = r.i32()?;
        let level = r.i32()?;
        let kind = if r.u8()? != 0 { read_map_army(r)? } else { ObjectKind::Placed(read_placed(r)?) };
        objects.push(MapObject { x, y, level, kind });
    }
    Ok(objects)
}

/// `t_army::read_from_map` (rva `523a10`), versions up to 9.
fn read_map_army(r: &mut Reader) -> Result<ObjectKind> {
    let at = r.pos;
    let version = r.u16()?;
    if version == 0 || version > 9 {
        return bad(format!("map army: unsupported version {version} at {at:#x}"));
    }
    let owner = if version >= 2 { r.u8()? } else { 6 };
    if version < 3 {
        return bad(format!("map army version {version} (not seen in maps yet) at {at:#x}"));
    }
    let army = read_army(r)?;
    // patrol settings: two bytes, since version 5 behind a flag
    if version == 4 || (version >= 5 && r.u8()? != 0) {
        r.u8()?;
        r.u8()?;
    }
    let events = if version >= 6 { read_owned_events(r, 3)? } else { OwnedEvents::default() };
    for v in 7..=9 {
        if version >= v {
            r.u8()?;
        }
    }
    Ok(ObjectKind::Army { owner, army, events })
}

/// Object types whose `read_from_map` is the base one (`t_adventure_object::read_from_map`,
/// rva `fab60`): just a version that must be 0.
const PLAIN: &[&str] = &[
    "academy", "arena", "artifact_pile", "blacksmith", "blattner_stone", "border_gate", "border_guard",
    "bridge", "buoy", "campfire", "cartographer", "chain", "clover_field", "coliseum", "corpse",
    "creature_bank", "decorative", "dolphin_school", "exhausted_mine", "eye_of_the_magi", "faerie_ring",
    "flotsam", "fountain", "hut_of_the_magi", "idol_of_fortune", "kelp_beds", "keymaster_tent", "magic_gem",
    "mana_recharger", "material_pile", "medicine_wagon", "mercenary_camp", "movement_booster", "nothing",
    "obelisk", "oyster", "path_arrow", "pathfinder", "port", "rainbow", "random_artifact",
    "random_material_pile", "random_potion", "random_staff", "random_tome", "sacred_grove", "sanctuary",
    "selection_ring", "ship", "shipwreck_survivor", "sphinx", "stagecoach", "system_object", "tavern",
    "temple", "tower", "trading_post", "training_grounds", "treasure", "tree_of_knowledge", "vein",
    "victory_flag",
];

/// Types with version 0..1 and, in version 1, a name (rva `7232b0`, `t_adv_sign`).
const NAMED: &[&str] = &[
    "deletion_marker", "event_trigger", "ferry", "gateway", "ocean_bottle", "pandoras_box", "sign",
    "subterranean_gate", "teleporter_entrance", "teleporter_exit", "whirlpool",
];

/// Types read by `t_owned_scriptable_adv_object` (version up to 2).
const SCRIPTABLE: &[&str] = &[
    "creature_dwelling", "lighthouse", "random_weekly_material_generator", "shipyard",
    "weekly_material_generator", "windmill",
];

fn read_placed(r: &mut Reader) -> Result<Placed> {
    let model = r.str16()?;
    // t_qualified_adv_object_type (rva 465960): version 0, three (u16 id, name) pairs
    zero_version(r, "object type")?;
    r.u16()?;
    let type_name = r.str16()?;
    r.u16()?;
    let major = r.str16()?;
    r.u16()?;
    let minor = r.str16()?;
    let mut p = Placed { model, type_name, major, minor, owner: None, name: None, garrison: None, hero: None };
    read_placed_body(r, &mut p)?;
    Ok(p)
}

/// Read a u16 version and fail if it is above `max`.
fn version(r: &mut Reader, max: u16, what: &str) -> Result<u16> {
    let at = r.pos;
    let v = r.u16()?;
    if v > max {
        return bad(format!("{what}: unknown version {v} at {at:#x}"));
    }
    Ok(v)
}

fn read_placed_body(r: &mut Reader, p: &mut Placed) -> Result<()> {
    let t = p.type_name.clone();
    let t = t.as_str();
    if PLAIN.contains(&t) {
        zero_version(r, t)?;
    } else if NAMED.contains(&t) {
        if version(r, 1, t)? >= 1 {
            p.name = Some(text(r)?);
        }
    } else if SCRIPTABLE.contains(&t) {
        let v = version(r, 2, t)?;
        p.owner = read_owned_scriptable(r, v)?;
    } else {
        match t {
            // witch hut, library, school of magic...: version 1 has two skill sets
            "school" | "teacher" => {
                if version(r, 1, t)? >= 1 {
                    read_skill_set(r)?;
                    read_skill_set(r)?;
                }
            }
            "mine" | "garrison" => read_garrisonable(r, p)?,
            "abandoned_mine" => {
                // rva 425db0
                let v = version(r, 1, t)?;
                if v >= 1 {
                    for _ in 0..4 {
                        read_built_in(r)?;
                    }
                }
                read_owned_continuous(r)?;
                read_owned_timed(r)?;
                read_owned_triggerable(r)?;
                if v >= 1 && r.u8()? != 0 {
                    p.garrison = Some(read_army(r)?);
                }
            }
            // scrolls lying on the map: version 1 adds the u32 spell (rva 430820)
            "artifact" => {
                if version(r, 1, t)? >= 1 {
                    r.u32()?;
                }
            }
            "conservatory" => {
                if version(r, 1, t)? >= 1 {
                    read_spell_set(r)?;
                    read_spell_set(r)?;
                }
            }
            "obelisk_marker" => {
                if version(r, 1, t)? >= 1 {
                    r.u16()?;
                }
            }
            "outpost" => {
                if version(r, 1, t)? >= 1 {
                    p.owner = Some(r.u8()?);
                }
            }
            "prison" => {
                if version(r, 1, t)? >= 1 {
                    p.hero = Some(read_hero(r)?);
                }
            }
            "carryover_hero" => {
                // rva 5bf0a0
                let v = version(r, 2, t)?;
                if v >= 1 {
                    p.owner = Some(r.u8()?);
                    p.name = Some(text(r)?);
                    if v >= 2 && r.u8()? != 0 {
                        r.u8()?;
                        r.u8()?;
                    }
                    read_owned_events(r, 3)?;
                }
            }
            "random_creature_dwelling" => {
                // rva 43eec0
                let v = version(r, 3, t)?;
                let base_version = match v {
                    0 => 0,
                    1 | 2 => 1,
                    _ => 2,
                };
                p.owner = read_owned_scriptable(r, base_version)?;
                if v >= 2 {
                    read_random_kind(r)?;
                }
            }
            "random_conservatory" => {
                let v = version(r, 2, t)?;
                if v == 2 {
                    read_spell_set(r)?;
                    read_spell_set(r)?;
                }
                if v >= 1 {
                    read_random_kind(r)?;
                }
            }
            "random_monster" | "random_random_monster" => {
                // t_random_monster_base::read_from_map (rva 7f36a0)
                let v = version(r, 4, t)?;
                if v >= 3 && r.u8()? != 0 {
                    r.u8()?;
                    r.u8()?;
                }
                if v >= 2 {
                    read_owned_events(r, 3)?;
                }
                if v >= 1 {
                    read_random_kind(r)?;
                }
                if v >= 4 && r.i32()? >= 0 {
                    r.i32()?;
                }
            }
            "random_shrine" => {
                let v = version(r, 3, t)?;
                if v == 1 {
                    r.take(23)?;
                } else if v >= 2 {
                    read_spell_set(r)?;
                }
                if v >= 3 {
                    read_random_kind(r)?;
                }
            }
            "shrine" => {
                let v = version(r, 2, t)?;
                if v == 1 {
                    r.take(23)?;
                } else if v == 2 {
                    read_spell_set(r)?;
                }
            }
            "random_teacher" => {
                if version(r, 1, t)? >= 1 {
                    r.u16()?;
                    read_random_kind(r)?;
                }
            }
            "random_temple" => {
                if version(r, 1, t)? >= 1 {
                    read_random_kind(r)?;
                }
            }
            "quest_gate" | "quest_guard" | "seers_hut" => read_quest_site(r, t)?,
            "random_town" => {
                // t_random_town::read_from_map (rva 7f5220), version 7 in all maps
                let at = r.pos;
                let v = r.u16()?;
                if v != 7 {
                    return bad(format!("random town: version {v} (only 7 is known) at {at:#x}"));
                }
                read_random_kind(r)?;
                read_town_data(r, p)?;
            }
            "town" => {
                // rva 89a8d0, version 4 in all maps: two spell sets, then the town data
                let at = r.pos;
                let v = r.u16()?;
                if v != 4 {
                    return bad(format!("town: version {v} (only 4 is known) at {at:#x}"));
                }
                read_spell_set(r)?;
                read_spell_set(r)?;
                read_town_data(r, p)?;
            }
            other => return bad(format!("object type {other:?} (model {:?}) has no reader yet", p.model)),
        }
    }
    Ok(())
}

/// `t_owned_scriptable_adv_object::read_from_map` (rva `7d7550`) with the version of the
/// derived class: owner since 1, triggerable events since 2.
fn read_owned_scriptable(r: &mut Reader, version: u16) -> Result<Option<u8>> {
    let mut owner = None;
    if version >= 1 {
        let at = r.pos;
        let o = r.u8()?;
        if o > 6 {
            return bad(format!("owner {o} at {at:#x}"));
        }
        owner = Some(o);
    }
    if version >= 2 {
        read_owned_triggerable(r)?;
    }
    Ok(owner)
}

/// Mines and garrisons (rva `7d42b0`), versions up to 2.
fn read_garrisonable(r: &mut Reader, p: &mut Placed) -> Result<()> {
    let v = version(r, 2, &p.type_name)?;
    let owner = if v >= 1 { r.u8()? } else { 6 };
    p.owner = Some(owner);
    if v >= 2 && owner != 6 {
        r.u8()?;
    }
    if v >= 2 {
        for _ in 0..4 {
            read_built_in(r)?;
        }
    }
    read_owned_continuous(r)?;
    read_owned_timed(r)?;
    read_owned_triggerable(r)?;
    if v >= 2 && r.u8()? != 0 {
        p.garrison = Some(read_army(r)?);
    }
    Ok(())
}

/// How a random object picks its owner/kind (rva `75c950`): version 0, a name from
/// `none`, `player`, `town` (table at `98ad44`), then a player byte or a town name.
fn read_random_kind(r: &mut Reader) -> Result<()> {
    zero_version(r, "random object kind")?;
    let at = r.pos;
    match r.str16()?.as_str() {
        "none" => {}
        "player" => {
            r.u8()?;
        }
        "town" => {
            text(r)?;
        }
        other => return bad(format!("random object kind {other:?} at {at:#x}")),
    }
    Ok(())
}

/// `t_abstract_town::read_map_data` (rva `417640`), versions 6..8.
fn read_town_data(r: &mut Reader, p: &mut Placed) -> Result<()> {
    let at = r.pos;
    let v = r.u16()?;
    if !(6..=8).contains(&v) {
        return bad(format!("town data: unknown version {v} at {at:#x}"));
    }
    let town_type = r.u8()?;
    if town_type > 6 {
        return bad(format!("town type {town_type} at {:#x}", r.pos - 1));
    }
    p.name = Some(text(r)?);
    // garrison
    if r.u8()? != 0 {
        p.garrison = Some(read_army(r)?);
    }
    // rva 4178c0: either two 43-bit building sets (present / allowed?) or one flag byte
    if r.u8()? != 0 {
        r.take(12)?;
    } else {
        r.u8()?;
    }
    for _ in 0..4 {
        read_built_in(r)?;
    }
    read_owned_timed(r)?;
    read_owned_triggerable(r)?;
    read_owned_continuous(r)?;
    if v >= 7 && r.u8()? != 0 {
        read_skill_set(r)?;
        read_skill_set(r)?;
    }
    if v >= 8 {
        r.u8()?;
    }
    Ok(())
}

/// Quest gate / guard and seer's hut (`t_quest_site`, rva `7ed940`, `7ef7a0`, `7efa80`).
/// Since version 1: four texts, the quest script, a u16-versioned condition; guards and
/// huts add texts and (version 2) completion scripts.
fn read_quest_site(r: &mut Reader, t: &str) -> Result<()> {
    let max = if t == "quest_gate" { 1 } else { 2 };
    let v = version(r, max, t)?;
    if v == 0 {
        return Ok(());
    }
    for _ in 0..4 {
        text(r)?;
    }
    read_script(r)?;
    read_versioned_boolean(r)?;
    match t {
        "quest_guard" => {
            text(r)?;
            if v >= 2 {
                read_script(r)?;
            }
        }
        "seers_hut" => {
            text(r)?;
            if v >= 2 {
                read_script(r)?;
            }
            text(r)?;
            if v < 2 {
                read_script(r)?;
            }
        }
        _ => {}
    }
    Ok(())
}
