//! Armies, creature stacks, heroes and artifacts as stored in `.h4c` maps.
//!
//! An army (`t_creature_array`, read at rva `640830`) is a u16 version (0) and seven slots.
//! Each slot starts with a byte saying what is in it (rva `640680`): 0xff nothing,
//! 0 a creature stack, 1 a hero. Heroes are big: they carry their own name, skills,
//! spells, artifacts and events.

use crate::map_script::{read_owned_events, text, zero_version, OwnedEvents};
use crate::reader::{bad, Reader};
use crate::Result;

/// Artifact types are below this (checked by the reader at rva `664410`).
const ARTIFACT_COUNT: u16 = 249;
/// These two artifact types (scroll and potion, judging by their behaviour) carry a spell.
const ARTIFACTS_WITH_SPELL: [u16; 2] = [0x7c, 0xa6];

/// `t_artifact` (rva `664410`): u16 type, plus u16 spell for scrolls/potions.
/// Returns the artifact type; the spell is checked but dropped.
pub(crate) fn read_artifact(r: &mut Reader) -> Result<u16> {
    let at = r.pos;
    let kind = r.u16()?;
    if kind >= ARTIFACT_COUNT {
        return bad(format!("artifact type {kind} at {at:#x}"));
    }
    if ARTIFACTS_WITH_SPELL.contains(&kind) {
        r.u16()?;
    }
    Ok(kind)
}

/// One slot of an army.
#[derive(Debug, Clone)]
pub enum Stack {
    Empty,
    Creatures { kind: i16, count: i16, artifacts: Vec<u16> },
    Hero(Box<Hero>),
}

/// Seven slots.
#[derive(Debug, Clone)]
pub struct Army {
    pub stacks: Vec<Stack>,
}

/// `t_creature_array::read` (rva `640830`).
pub(crate) fn read_army(r: &mut Reader) -> Result<Army> {
    zero_version(r, "army")?;
    let mut stacks = Vec::with_capacity(7);
    for _ in 0..7 {
        stacks.push(read_stack(r)?);
    }
    Ok(Army { stacks })
}

fn read_stack(r: &mut Reader) -> Result<Stack> {
    let at = r.pos;
    match r.u8()? {
        0xff => Ok(Stack::Empty),
        0 => {
            // t_creature::read (rva 63dc20): version 0/1, type (-1 = random?), count,
            // since version 1 artifacts carried by the stack (rva 653390)
            let version = r.u16()?;
            if version > 1 {
                return bad(format!("creature stack: unknown version {version} at {at:#x}"));
            }
            let kind = r.u16()? as i16;
            let count = r.u16()? as i16;
            let artifacts = if version >= 1 { read_artifact_list(r)? } else { Vec::new() };
            Ok(Stack::Creatures { kind, count, artifacts })
        }
        1 => Ok(Stack::Hero(Box::new(read_hero(r)?))),
        other => bad(format!("army slot kind {other} at {at:#x}")),
    }
}

/// u16 count + artifacts (rva `653390`).
fn read_artifact_list(r: &mut Reader) -> Result<Vec<u16>> {
    let n = r.u16()?;
    (0..n).map(|_| read_artifact(r)).collect()
}

/// A hero placed on the map or inside an army.
#[derive(Debug, Clone)]
pub struct Hero {
    /// Portrait / hero id; -1 = random.
    pub portrait: i16,
    /// Class (0..=10 a base class: knight, priest, lord, mage, death knight, necromancer,
    /// thief, sorcerer, archer, druid, barbarian) and gender (1 male, 0 female); -1 = random.
    /// Meanings from the exe's class table and hero model code, see `h4_rules::heroes`.
    pub class: i8,
    pub gender: i8,
    /// Custom name and biography (empty = the default for the portrait).
    pub name: String,
    pub biography: String,
    /// Artifacts in the 14 equipment slots.
    pub equipped: Vec<Option<u16>>,
    pub backpack: Vec<u16>,
    pub events: OwnedEvents,
}

/// `t_hero::read_from_map` (rva `72f470`), versions 8..10.
pub(crate) fn read_hero(r: &mut Reader) -> Result<Hero> {
    let at = r.pos;
    let version = r.u16()?;
    if !(8..=10).contains(&version) {
        return bad(format!("hero: unknown version {version} at {at:#x}"));
    }
    let portrait = r.u16()? as i16;
    let class = r.u8()? as i8;
    let gender = r.u8()? as i8;
    // Level and experience-like fields: one u8, two values (u16 before version 10,
    // u8 since), then two more u8. Their meaning isn't needed to read on.
    r.u8()?;
    if version < 10 {
        r.u16()?;
        r.u16()?;
    } else {
        r.u8()?;
        r.u8()?;
    }
    r.u8()?;
    r.u8()?;
    let name = text(r)?;
    let biography = text(r)?;
    // optional skill masteries: 36 bytes, -1 = none, 0..4
    if r.u8()? != 0 {
        r.take(36)?;
    }
    read_spell_set(r)?;
    // rva 72f8d0: 14 equipment slots, each a presence byte and an artifact
    let mut equipped = Vec::with_capacity(14);
    for _ in 0..14 {
        equipped.push(if r.u8()? != 0 { Some(read_artifact(r)?) } else { None });
    }
    let backpack = read_artifact_list(r)?;
    let events = read_owned_events(r, 3)?;
    if version >= 9 {
        r.u8()?;
    }
    Ok(Hero { portrait, class, gender, name, biography, equipped, backpack, events })
}

/// A set of spells (rva `865e40` -> `865bf0`): u8 version, then a bit set.
/// Version 0 has 184 bits (23 bytes); versions 1 and 2 have 24 bytes.
pub(crate) fn read_spell_set(r: &mut Reader) -> Result<Vec<u8>> {
    let at = r.pos;
    let bytes = match r.u8()? {
        0 => 23,
        1 | 2 => 24,
        v => return bad(format!("spell set: unknown version {v} at {at:#x}")),
    };
    Ok(r.take(bytes)?.to_vec())
}

/// A set of skills (rva `858150`): u8 version 0, then 5 bytes (36 bits).
pub(crate) fn read_skill_set(r: &mut Reader) -> Result<Vec<u8>> {
    let at = r.pos;
    let version = r.u8()?;
    if version != 0 {
        return bad(format!("skill set: unknown version {version} at {at:#x}"));
    }
    Ok(r.take(5)?.to_vec())
}
