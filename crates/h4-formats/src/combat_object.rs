//! Combat scenery models. Layout: Buka 3.0 readers 206aa0 / 206690.
use crate::reader::{bad, Reader};
use crate::sprite::ActorSequence;
use crate::Result;

#[derive(Debug, Clone)]
pub struct CombatObject {
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
    /// Original numeric type and name, before the engine resolves their enum.
    pub type_id: u16,
    pub type_name: String,
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

/// Two type ids that RVA 238690 never splits into 1×1 pieces, even when the
/// model does not block. Their names in the type enum are not resolved yet;
/// 0x25 is also the type the reader assumes for version 0 resources.
const UNSPLIT_TYPE_IDS: [u16; 2] = [0x23, 0x25];

impl CombatObject {
    /// Static blocking predicate: the first extra byte (virtual +0x10, RVA 477620).
    pub fn blocks(&self) -> bool {
        self.attributes[0] != 0
    }

    /// The resource flag: a nonzero value makes the cell query ignore the
    /// object (virtual +0x5c, RVA 477660).
    pub fn skips_blocking(&self) -> bool {
        self.flag != 0
    }

    /// Side of one square segment in cells, as chosen by RVA 238690.
    ///
    /// Non-blocking, non-skipped models of ordinary types are cut into 1×1
    /// pieces. Everything else uses the largest square that tiles the model
    /// exactly: the greatest common divisor of width and height. A square
    /// model therefore stays a single segment. With a zero side the result is
    /// the other side (gcd(n, 0) = n), as in the executable.
    pub fn segment_size(&self) -> u8 {
        if !self.blocks() && !self.skips_blocking() && !UNSPLIT_TYPE_IDS.contains(&self.type_id) {
            return 1;
        }
        // Euclid's algorithm for the greatest common divisor.
        let [mut a, mut b] = self.properties;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }

    /// RVA 238690 at native image scale 1.0. All layers, including shadows,
    /// participate in deciding whether a segment exists.
    pub fn segments(&self) -> Result<Vec<CombatObjectSegment>> {
        let [width, height] = self.properties.map(i32::from);
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
        let elevation = i32::from(self.parameter);
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
        let version = r.u16()?;
        if version > 2 {
            return bad(format!("unsupported combat_object version {version}"));
        }
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
            (0x25, String::new())
        };
        let image = ActorSequence::read_image_from(&mut r)?;
        if r.remaining() != 0 {
            return bad(format!(
                "{} trailing combat_object bytes at {:#x}",
                r.remaining(),
                r.pos
            ));
        }
        Ok(Self {
            version,
            properties,
            parameter,
            frames_per_second,
            flag,
            attributes,
            type_id,
            type_name,
            image,
        })
    }
}
