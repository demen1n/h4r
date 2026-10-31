//! Combat animation definitions (`combat_actor.*`).
//!
//! Layout recovered from the Buka 3.0 executable, reader RVA `1deb50`.
//! See `decomp/combat_actor.md` for evidence and unresolved field meanings.
//! Versions 5 and 6 are accepted: older branches exist in the executable but have
//! not been checked against real resources.

use crate::adv_actor::DIRECTIONS;
use crate::reader::{bad, Reader};
use crate::Result;

#[derive(Debug, Clone)]
pub struct CombatAction {
    pub name: String,
    pub frames_per_second: u8,
    /// Extra byte introduced in version 5. Its meaning is not yet confirmed.
    pub parameter: u8,
    /// Names without `actor_sequence.` and `.h4d`, in [`DIRECTIONS`] order.
    /// An empty string means this action has no sequence for that direction.
    pub sequences: [String; 8],
}

impl CombatAction {
    /// The original loader rebuilds these names for a separate composite cache;
    /// they are not ordinary `actor_sequence` resources.
    pub fn is_composite(&self) -> bool {
        matches!(self.name.as_str(), "full_walk" | "wait_and_base")
    }
}

#[derive(Debug, Clone)]
pub struct CombatActorModel {
    pub version: u16,
    /// Two model properties; their physical meaning still needs confirmation.
    pub properties: [u8; 2],
    pub prewalk_length: u16,
    pub walk_length: u16,
    pub postwalk_length: u16,
    /// Eight signed triples in direction order. Do not assume screen coordinates.
    pub directional_points: [(i32, i32, i32); 8],
    /// Actions explicitly stored in the resource, without engine-generated fallbacks.
    pub actions: Vec<CombatAction>,
    pub offset: (i32, i32),
}

impl CombatActorModel {
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.u16()?;
        if !matches!(version, 5 | 6) {
            return bad(format!("unsupported combat_actor version {version} at offset 0"));
        }
        let properties = [r.u8()?, r.u8()?];
        let prewalk_length = r.u16()?;
        let walk_length = r.u16()?;
        let postwalk_length = r.u16()?;
        let mut directional_points = [(0, 0, 0); 8];
        for p in &mut directional_points {
            *p = if version == 6 {
                (r.i32()?, r.i32()?, r.i32()?)
            } else {
                (
                    i32::from(properties[0] / 2),
                    i32::from(properties[0] / 2),
                    i32::from(properties[1] / 2),
                )
            };
        }
        let count = r.u16()? as usize;
        // Even empty action names and sequence names need 20 bytes per record.
        if count > r.remaining().saturating_sub(8) / 20 {
            return bad(format!(
                "combat_actor claims {count} actions at offset {:#x}, have {} bytes",
                r.pos,
                r.remaining()
            ));
        }
        let mut actions = Vec::with_capacity(count);
        for _ in 0..count {
            let name = r.str16()?;
            let frames_per_second = r.u8()?;
            let parameter = r.u8()?;
            let mut sequences: [String; 8] = Default::default();
            for s in &mut sequences {
                *s = r.str16()?;
            }
            actions.push(CombatAction {
                name,
                frames_per_second,
                parameter,
                sequences,
            });
        }
        let offset = (r.i32()?, r.i32()?);
        if r.remaining() != 0 {
            return bad(format!("combat_actor has {} trailing bytes at offset {:#x}", r.remaining(), r.pos));
        }
        Ok(Self {
            version,
            properties,
            prewalk_length,
            walk_length,
            postwalk_length,
            directional_points,
            actions,
            offset,
        })
    }

    pub fn action(&self, name: &str) -> Option<&CombatAction> {
        self.actions.iter().find(|a| a.name == name)
    }

    pub fn sequence(&self, action: &str, direction: &str) -> Option<String> {
        let action = self.action(action)?;
        let i = DIRECTIONS.iter().position(|d| *d == direction)?;
        let s = &action.sequences[i];
        (!s.is_empty()).then(|| format!("actor_sequence.{s}.h4d"))
    }
}
