//! Combat scenery models. Layout: Buka 3.0 readers 206aa0 / 206690.
//!
//! A resource is a header (`CombatObjectHeader`) followed by an image. The game
//! also keeps a cache of all headers without images in one resource
//! (`CombatHeaderTable`); the obstacle placer chooses its models from it.
use crate::reader::{bad, Reader};
use crate::sprite::ActorSequence;
use crate::Result;

/// Everything in a combat object resource except its image (reader RVA 206690).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatObjectHeader {
    pub version: u16,
    /// Overall dimensions in battlefield cells, before image-dependent segmentation.
    pub properties: [u8; 2],
    pub parameter: u8,
    pub frames_per_second: u8,
    /// Nonzero segments are skipped by the static cell-blocking query.
    pub flag: u8,
    /// Five bytes introduced in version 1. The first is the static blocking
    /// predicate; meanings of the remaining four are still unverified.
    pub attributes: [u8; 5],
    /// Obstacle type as stored: an older numbering that often disagrees with
    /// the exe enum (`autumn_tree` is stored as 1, the enum says 0). The game
    /// trusts the name; use `resolved_type`.
    pub type_id: u16,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct CombatObject {
    pub header: CombatObjectHeader,
    pub image: ActorSequence,
}

/// One image-dependent stationary segment, before placement on the field.
#[derive(Debug, Clone)]
pub struct CombatObjectSegment {
    pub size: u8,
    /// Top-left position in battlefield cells (native stores sixteenths).
    pub at: (i32, i32),
    pub image: ActorSequence,
}

/// Names of the exe enum `t_obstacle_type`, by value (keyword table, VA `0x98d960`).
/// The order is the exe's, not alphabetical: geyser 12, gray_rock 13, green_log 14.
pub const OBSTACLE_TYPE_NAMES: [&str; 38] = [
    "autumn_tree",
    "black_rock",
    "bones",
    "burnt_log",
    "cactus",
    "crack",
    "crate",
    "dead_tree",
    "decidous_tree",
    "desert_tree",
    "dry_shrub",
    "evergreen_tree",
    "geyser",
    "gray_rock",
    "green_log",
    "green_rock",
    "lava_crack",
    "log",
    "mushroom",
    "rope",
    "sack",
    "sandy_rock",
    "shrub",
    "snow_log",
    "snow_rock",
    "snow_shrub",
    "snow_tree",
    "sparkles",
    "stalagmite",
    "stump",
    "swamp_log",
    "swamp_shrub",
    "swamp_tree",
    "tan_rock",
    "castle_gate",
    "castle_tower",
    "castle_wall",
    "special",
];

/// `castle_gate`: the first castle part. Ordinary scenery has smaller types.
pub const CASTLE_GATE_TYPE: u16 = 0x22;
/// `castle_tower`.
pub const CASTLE_TOWER_TYPE: u16 = 0x23;
/// `special`; also the type the reader assumes for version 0 resources.
pub const SPECIAL_TYPE: u16 = 0x25;

/// Value of an obstacle type name, compared like the exe keyword map
/// (`_strnicmp` with equal lengths: ASCII case is ignored, nothing else).
pub fn obstacle_type_by_name(name: &str) -> Option<u16> {
    OBSTACLE_TYPE_NAMES
        .iter()
        .position(|known| known.eq_ignore_ascii_case(name))
        .map(|i| i as u16)
}

impl CombatObjectHeader {
    /// Static blocking predicate: the first extra byte (virtual +0x10, RVA 477620).
    pub fn blocks(&self) -> bool {
        self.attributes[0] != 0
    }

    /// The resource flag: a nonzero value makes the cell query ignore the
    /// object (virtual +0x5c, RVA 477660).
    pub fn skips_blocking(&self) -> bool {
        self.flag != 0
    }

    /// Obstacle type as the game uses it, a value of `t_obstacle_type`
    /// (reader RVA 206690): the stored number, clamped to `0..=0x25`, unless
    /// the stored name is a known type name that differs from it — then the
    /// name wins. Version 0 resources have no type and count as `special`.
    pub fn resolved_type(&self) -> u16 {
        if self.version == 0 {
            return SPECIAL_TYPE;
        }
        // The exe reads the u16 as a signed number.
        let stored = self.type_id as i16;
        let stored = if (0..=SPECIAL_TYPE as i16).contains(&stored) {
            stored as u16
        } else {
            SPECIAL_TYPE
        };
        if OBSTACLE_TYPE_NAMES[usize::from(stored)].eq_ignore_ascii_case(&self.type_name) {
            return stored;
        }
        obstacle_type_by_name(&self.type_name).unwrap_or(stored)
    }

    /// Side of one square segment in cells, as chosen by RVA 238690.
    ///
    /// Non-blocking, non-skipped models are cut into 1×1 pieces, except
    /// castle towers and `special` objects. Everything else uses the largest
    /// square that tiles the model exactly: the greatest common divisor of
    /// width and height. A square model therefore stays a single segment.
    /// With a zero side the result is the other side (gcd(n, 0) = n), as in
    /// the executable.
    pub fn segment_size(&self) -> u8 {
        let unsplit = matches!(self.resolved_type(), CASTLE_TOWER_TYPE | SPECIAL_TYPE);
        if !self.blocks() && !self.skips_blocking() && !unsplit {
            return 1;
        }
        // Euclid's algorithm for the greatest common divisor.
        let [mut a, mut b] = self.properties;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }

    /// Read the header part of a resource; the image follows it.
    pub(crate) fn read_from(r: &mut Reader) -> Result<Self> {
        let version = r.u16()?;
        if version > 2 {
            return bad(format!("unsupported combat_object version {version}"));
        }
        // Version 2 stores width and height; older versions one side of a square.
        let first = r.u8()?;
        let properties = [first, if version == 2 { r.u8()? } else { first }];
        let parameter = r.u8()?;
        let frames_per_second = r.u8()?;
        let flag = r.u8()?;
        let mut attributes = [0; 5];
        let (type_id, type_name) = if version >= 1 {
            attributes.copy_from_slice(r.take(5)?);
            (r.u16()?, r.str16()?)
        } else {
            (SPECIAL_TYPE, String::new())
        };
        Ok(Self {
            version,
            properties,
            parameter,
            frames_per_second,
            flag,
            attributes,
            type_id,
            type_name,
        })
    }
}

impl CombatObject {
    pub fn blocks(&self) -> bool {
        self.header.blocks()
    }

    pub fn skips_blocking(&self) -> bool {
        self.header.skips_blocking()
    }

    pub fn segment_size(&self) -> u8 {
        self.header.segment_size()
    }

    /// RVA 238690 at native image scale 1.0. All layers, including shadows,
    /// participate in deciding whether a segment exists.
    pub fn segments(&self) -> Result<Vec<CombatObjectSegment>> {
        let [width, height] = self.header.properties.map(i32::from);
        let size = i32::from(self.segment_size());
        // A model that stays one square, or has a zero side, takes the
        // executable's short path: no cropping, even with an empty image.
        if (size == width && size == height) || width == 0 || height == 0 {
            return Ok(vec![CombatObjectSegment {
                size: self.segment_size(),
                at: (0, 0),
                image: self.image.clone(),
            }]);
        }
        let Some(area) = self
            .image
            .frames
            .iter()
            .chain(&self.image.shadows)
            .chain(&self.image.base)
            .map(|l| l.rect)
            .reduce(|a, b| a.union(&b))
        else {
            return Ok(Vec::new());
        };
        let (ox, oy) = self.image.offset;
        // Bound work and arithmetic before constructing masks from untrusted
        // coordinates. This is far beyond any image in the installed corpus.
        let coordinates = [area.left, area.top, area.right, area.bottom, ox, oy];
        if coordinates.iter().any(|&v| i64::from(v).abs() > 16384) {
            return bad("combat object segmentation coordinates exceed 16384 pixels");
        }
        // Map cell corner (x, y) -> image pixel. One cell is a 32×16 diamond;
        // `parameter` lifts the cutting plane (decomp/combat_object.md).
        let elevation = i32::from(self.header.parameter);
        let project = |x, y| (16 * (y - x) - ox, 8 * (x + y) - elevation - oy);
        let mut segments = Vec::new();
        for x in (0..width).step_by(size as usize) {
            for y in (0..height).step_by(size as usize) {
                // Diamond corners of this segment: a = top, b = left,
                // c = right, d = bottom (only the needed coordinates).
                let (ax, ay) = project(x, y);
                let (bx, by) = project(x + size, y);
                let (cx, _) = project(x, y + size);
                let (_, dy) = project(x + size, y + size);
                // One half-open [left, right) interval per image row, relative
                // to `area.left`. Sides on the outer edge of the model extend
                // to the edge of the whole image, so overhanging parts of the
                // picture (a tree crown, a shadow) stay with an edge segment.
                let mut lines = Vec::new();
                let top = if x == 0 && y == 0 {
                    lines.extend(std::iter::repeat_n(
                        (0, area.width()),
                        (ay - area.top).max(0) as usize,
                    ));
                    area.top
                } else {
                    ay
                };
                // Upper half of the diamond: widens by 2 pixels per row.
                for i in 0..by - ay {
                    let d = 2 * (i + 1);
                    lines.push((
                        if y == 0 { 0 } else { ax - area.left - d },
                        if x == 0 {
                            area.width()
                        } else {
                            ax - area.left + d
                        },
                    ));
                }
                // Lower half: narrows by 2 pixels per row.
                for i in 0..dy - by {
                    let d = 2 * (i + 1);
                    lines.push((
                        if x + size == width {
                            if y == 0 {
                                0
                            } else {
                                bx - area.left
                            }
                        } else {
                            bx - area.left + d
                        },
                        if y + size == height {
                            if x == 0 {
                                area.width()
                            } else {
                                cx - area.left
                            }
                        } else {
                            cx - area.left - d
                        },
                    ));
                }
                if x + size == width || y + size == height {
                    let a = if x + size == width {
                        if y == 0 {
                            0
                        } else {
                            bx - area.left
                        }
                    } else {
                        ax - area.left
                    };
                    let b = if y + size == height {
                        if x == 0 {
                            area.width()
                        } else {
                            cx - area.left
                        }
                    } else {
                        ax - area.left
                    };
                    lines.extend(std::iter::repeat_n(
                        (a, b),
                        (area.bottom - dy).max(0) as usize,
                    ));
                }
                let crop = |layers: &[crate::sprite::Layer]| {
                    layers
                        .iter()
                        .map(|l| l.crop_scanlines((area.left, top), &lines))
                        .collect::<Vec<_>>()
                };
                let image = ActorSequence {
                    frames: crop(&self.image.frames),
                    shadows: crop(&self.image.shadows),
                    base: crop(&self.image.base),
                    offset: (-ax, -(ay + elevation)),
                };
                // Native group get_rect includes empty layers, too.
                let rect = image
                    .frames
                    .iter()
                    .chain(&image.shadows)
                    .chain(&image.base)
                    .map(|l| l.rect)
                    .reduce(|a, b| a.union(&b));
                if rect.is_some_and(|r| r.left != r.right) {
                    segments.push(CombatObjectSegment {
                        size: self.segment_size(),
                        at: (x, y),
                        image,
                    });
                }
            }
        }
        Ok(segments)
    }

    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let header = CombatObjectHeader::read_from(&mut r)?;
        let image = ActorSequence::read_image_from(&mut r)?;
        if r.remaining() != 0 {
            return bad(format!(
                "{} trailing combat_object bytes at {:#x}",
                r.remaining(),
                r.pos
            ));
        }
        Ok(Self { header, image })
    }
}

/// One cached header: the model's resource name and its header.
#[derive(Debug, Clone)]
pub struct CombatHeaderEntry {
    /// Name without the `combat_object.` prefix, e.g. `trees.Autumn.17`.
    pub name: String,
    pub header: CombatObjectHeader,
}

/// `combat_header_table_cache.combat_header.h4d`: headers of all combat
/// objects without images (`t_combat_header_table::read`, RVA 1ff240).
#[derive(Debug, Clone)]
pub struct CombatHeaderTable {
    /// Five u32 before the entry count, read but not used by the placer.
    /// Installed values: 3, 1, 2002, 10, 45. Meaning unknown.
    pub unknown: [u32; 5],
    /// In file order; the placer weighs candidates in this order.
    pub entries: Vec<CombatHeaderEntry>,
}

impl CombatHeaderTable {
    pub const RESOURCE: &'static str = "combat_header_table_cache.combat_header.h4d";

    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let unknown = [r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let count = r.u32()? as usize;
        // Each entry takes well over 8 bytes; refuse absurd counts before allocating.
        if count > r.remaining() / 8 {
            return bad(format!("combat header count {count} does not fit the file"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            // The name has a u32 length here, unlike the u16 strings inside the header.
            let len = r.u32()? as usize;
            let name = String::from_utf8_lossy(r.take(len)?).into_owned();
            let header = CombatObjectHeader::read_from(&mut r)?;
            entries.push(CombatHeaderEntry { name, header });
        }
        if r.remaining() != 0 {
            return bad(format!(
                "{} trailing combat header bytes at {:#x}",
                r.remaining(),
                r.pos
            ));
        }
        Ok(Self { unknown, entries })
    }
}
