//! `adv_object.*`: models of adventure map objects — trees, mountains, mines, shrines...
//! (`t_adv_object_model_24`). Readers in the Russian 3.0 exe: model rva 0x4d190, footprint
//! (`t_adv_object_model_base_base::read`) 0x4c860, object type 0x65960, image 0x4c470.
//!
//! ```text
//! u16 version (1..6; all files of the install are 6)
//! u8  frames per second
//! u16 has_edge_triggers (version >= 3; 0 or 1)
//! u16 width, u16 height                         footprint in map cells
//! bits impassable[width * height]               one bit per cell, packed into bytes, LSB first
//! if has_edge_triggers:
//!     bits left_edge_trigger[width * (height + 1)]
//!     bits right_edge_trigger[(width + 1) * height]
//! else:
//!     bits trigger[width * height]               old format; the game reads and drops it
//! version >= 6:
//!     bits left_edge_blocked[2 * width * (height + 1)]
//!     bits right_edge_blocked[2 * (width + 1) * height]
//!     u8   vertex_height[(width + 1) * (height + 1)]
//! version >= 2: object type
//!     u16 0 (version), then three times u16 id + u16-prefixed name:
//!     type ("decorative", "mine", ...), major subtype ("tree", ...), minor subtype ("autumn")
//! u8  flat mode (version >= 4; 0..2, see `ObjectModel::flat_mode`)
//! u8  has_flag, then if set i32 x, i32 y flag offset (version >= 5)
//! u8  image kind: 0 overlay, 1 underlay (drawn flat under everything, like a crack)
//! image:
//!     u16 image version (overlay < 3), then an actor sequence (see `sprite`)
//!     overlay only: u32 n, n * i16 depth per 32-pixel column of the frames
//! ```
//!
//! Layout on the map (found from the data, not from code; see `decomp/adv_object.md`): after
//! the sequence offset, (0, 0) is the top corner of footprint cell (0, 0), the topmost one;
//! footprint x goes to the screen's lower left, y to the lower right. The depth columns cover
//! the frames (shadows left out) on a 32-pixel grid counted from that point.
//!
//! Bit arrays are rounded up to whole bytes each (`n.div_ceil(8)`, reader rva 0x4cff0).

use crate::reader::{bad, Reader};
use crate::sprite::ActorSequence;
use crate::Result;

/// `t_qualified_adv_object_type`: what the object is. The game looks names up first and
/// falls back to the numeric ids, so both are kept.
#[derive(Debug, Clone, Default)]
pub struct ObjectType {
    pub id: u16,
    pub name: String,
    pub major_id: u16,
    pub major: String,
    pub minor_id: u16,
    pub minor: String,
}

#[derive(Debug, Clone)]
pub struct ObjectModel {
    pub version: u16,
    pub frames_per_second: u8,
    /// Footprint size in cells.
    pub width: u16,
    pub height: u16,
    /// Row-major (`y * width + x`), true = heroes can't step on that cell.
    pub impassable: Vec<bool>,
    /// Older trigger cells, stored when there are no edge triggers. The game reads and drops
    /// them (rva 0x4c860); kept here only for inspection.
    pub trigger: Vec<bool>,
    pub object_type: ObjectType,
    /// Which footprint cells need flat ground (`is_cell_flat`): 0 none, 1 the impassable
    /// ones, 2 all.
    pub flat_mode: u8,
    pub flag_offset: Option<(i32, i32)>,
    /// Underlays are drawn flat, under every overlay (cracks, holes, roads-like decals).
    pub underlay: bool,
    pub image: ActorSequence,
    /// Overlay only: depth of each 32-pixel column of the image, used to sort the object
    /// against heroes and other objects.
    pub column_depths: Vec<i16>,
}

fn bits(r: &mut Reader, n: usize) -> Result<Vec<bool>> {
    let bytes = r.take(n.div_ceil(8))?;
    Ok((0..n).map(|i| bytes[i / 8] & (1 << (i % 8)) != 0).collect())
}

impl ObjectModel {
    pub fn read(data: &[u8]) -> Result<ObjectModel> {
        let mut r = Reader::new(data);
        let m = Self::read_from(&mut r)?;
        if r.remaining() != 0 {
            return bad(format!("{} bytes left after the object at {:#x}", r.remaining(), r.pos));
        }
        Ok(m)
    }

    fn read_from(r: &mut Reader) -> Result<ObjectModel> {
        let version = r.u16()?;
        if !(1..=6).contains(&version) {
            return bad(format!("unknown adv_object version {version}"));
        }
        let frames_per_second = r.u8()?;
        let edge_triggers = if version >= 3 { r.u16()? } else { 0 };
        if edge_triggers > 1 {
            return bad(format!("edge trigger flag {edge_triggers} at {:#x}", r.pos - 2));
        }
        let (width, height) = (r.u16()?, r.u16()?);
        let (w, h) = (width as usize, height as usize);
        if w * h > 64 * 64 {
            return bad(format!("implausible footprint {w}x{h}"));
        }
        let impassable = bits(r, w * h)?;
        let mut trigger = Vec::new();
        if edge_triggers == 1 {
            bits(r, w * (h + 1))?;
            bits(r, (w + 1) * h)?;
        } else {
            trigger = bits(r, w * h)?;
        }
        if version >= 6 {
            bits(r, 2 * w * (h + 1))?;
            bits(r, 2 * (w + 1) * h)?;
            r.take((w + 1) * (h + 1))?;
        }
        let object_type = if version >= 2 { read_type(r)? } else { ObjectType::default() };
        // before version 4 the game decides: objects larger than one cell sit on flat ground
        let flat_mode = if version >= 4 {
            r.u8()?
        } else {
            (width >= 2 || height >= 2) as u8
        };
        if flat_mode > 2 {
            return bad(format!("unknown flat mode {flat_mode}"));
        }
        let mut flag_offset = None;
        if version >= 5 && r.u8()? != 0 {
            flag_offset = Some((r.i32()?, r.i32()?));
        }
        let kind = r.u8()?;
        let underlay = match kind {
            0 => false,
            1 => true,
            k => return bad(format!("unknown image kind {k} at {:#x}", r.pos - 1)),
        };
        let image_version = r.u16()?;
        let image = ActorSequence::read_from(r)?;
        let mut column_depths = Vec::new();
        if !underlay {
            if image_version >= 3 {
                return bad(format!("unknown overlay image version {image_version}"));
            }
            let n = r.u32()? as usize;
            if n > 4096 {
                return bad(format!("implausible column count {n}"));
            }
            for _ in 0..n {
                column_depths.push(r.u16()? as i16);
            }
        }
        Ok(ObjectModel {
            version,
            frames_per_second,
            width,
            height,
            impassable,
            trigger,
            object_type,
            flat_mode,
            flag_offset,
            underlay,
            image,
            column_depths,
        })
    }
}

fn read_type(r: &mut Reader) -> Result<ObjectType> {
    let v = r.u16()?;
    if v != 0 {
        return bad(format!("unknown object type version {v}"));
    }
    Ok(ObjectType {
        id: r.u16()?,
        name: r.str16()?,
        major_id: r.u16()?,
        major: r.str16()?,
        minor_id: r.u16()?,
        minor: r.str16()?,
    })
}
