//! `adv_actor.*`: how a hero or creature is animated on the adventure map
//! (`t_adv_actor_model_definition`).
//!
//! ```text
//! u16 version (3), u8 (1)
//! u16 prewalk length, u16 walk length, u16 postwalk length   distance covered by each animation;
//!                                                             one tile step is 32
//! u16 action count, then per action:
//!     u16-prefixed name ("wait", "fidget", "prewalk", "walk", "postwalk", "attack", ...)
//!     u8 frames per second
//!     8 u16-prefixed actor_sequence names, directions ne, e, se, s, sw, w, nw, n
//! i32 x, i32 y      offset (get_offset)
//! i32 x, i32 y      where the player's flag goes (get_flag_offset)
//! ```
//!
//! Method names come from the debug map (`get_frames_per_second`, `get_walk_length`, ...).

use crate::reader::{bad, Reader};
use crate::Result;

/// Direction order of the sequence names in each action.
pub const DIRECTIONS: [&str; 8] = ["ne", "e", "se", "s", "sw", "w", "nw", "n"];

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub frames_per_second: u8,
    /// `actor_sequence` names (without the `actor_sequence.` prefix), in [`DIRECTIONS`] order.
    pub sequences: [String; 8],
}

#[derive(Debug, Clone)]
pub struct AdvActorModel {
    pub prewalk_length: u16,
    pub walk_length: u16,
    pub postwalk_length: u16,
    pub actions: Vec<Action>,
    pub offset: (i32, i32),
    pub flag_offset: (i32, i32),
}

impl AdvActorModel {
    pub fn read(data: &[u8]) -> Result<AdvActorModel> {
        let mut r = Reader::new(data);
        let version = r.u16()?;
        if version != 3 {
            return bad(format!("unknown adv_actor version {version}"));
        }
        r.u8()?;
        let prewalk_length = r.u16()?;
        let walk_length = r.u16()?;
        let postwalk_length = r.u16()?;
        let count = r.u16()?;
        let mut actions = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name = r.str16()?;
            let frames_per_second = r.u8()?;
            let mut sequences: [String; 8] = Default::default();
            for s in sequences.iter_mut() {
                *s = r.str16()?;
            }
            actions.push(Action { name, frames_per_second, sequences });
        }
        let offset = (r.i32()?, r.i32()?);
        let flag_offset = (r.i32()?, r.i32()?);
        Ok(AdvActorModel { prewalk_length, walk_length, postwalk_length, actions, offset, flag_offset })
    }

    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }

    /// Full resource name of an action's animation in a direction.
    pub fn sequence(&self, action: &str, dir: &str) -> Option<String> {
        let a = self.action(action)?;
        let i = DIRECTIONS.iter().position(|d| *d == dir)?;
        let s = &a.sequences[i];
        (!s.is_empty()).then(|| format!("actor_sequence.{s}.h4d"))
    }
}
