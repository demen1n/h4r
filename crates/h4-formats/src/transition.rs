//! Terrain transition masks: `transition.Transitions.h4d`.
//!
//! Where two kinds of terrain meet, the game draws the higher one over the edge of the lower
//! tile through a mask. The file holds 15 named sets ("land 1", "road 2", "land_to_water 3",
//! ...) of 93 masks each:
//!
//! ```text
//! u8 set count
//! per set: u16-prefixed name, u32 mask count, then per mask:
//!     i32 code, u8 depth (1 or 4 bits per pixel), data (160 bytes for 1 bit, 544 for 4)
//! ```
//!
//! Mask data covers the 64x32 tile diamond column by column (same column heights as terrain
//! tiles), each column packed to whole bytes, low bits/nibbles first.
//!
//! The code says which parts of the tile the other terrain covers, one decimal digit per
//! corner of the diamond: thousands = east, hundreds = south, tens = west, ones = north.
//! A digit is 0 (not covered), 1 (from the edge before the corner, going clockwise from north),
//! 2 (from the edge after it), 3 (both edges) or 4 (only the diagonal neighbour at the corner).
//! See [`corner_code`].
//!
//! Maps (`.h4c`) store the transitions the editor chose for every tile as a mask set number
//! (0..=19, see [`MAP_MASK_SETS`]) and the index of the mask within that set, in file order.

use std::collections::HashMap;

use crate::reader::{bad, Reader};
use crate::terrain::{column_height, TILE_HEIGHT, TILE_WIDTH};
use crate::Result;

#[derive(Debug, Clone)]
pub struct Mask {
    pub depth: u8,
    data: Vec<u8>,
    /// Offset of each column in `data`.
    columns: [usize; TILE_WIDTH],
}

impl Mask {
    fn new(depth: u8, data: Vec<u8>) -> Mask {
        let mut columns = [0; TILE_WIDTH];
        let mut offset = 0;
        for (x, c) in columns.iter_mut().enumerate() {
            *c = offset;
            let h = column_height(x);
            offset += if depth == 4 { h.div_ceil(2) } else { h.div_ceil(8) };
        }
        Mask { depth, data, columns }
    }

    /// Coverage 0..=255 at pixel (x, y) of the tile's 64x32 box.
    pub fn alpha(&self, x: usize, y: usize) -> u8 {
        if x >= TILE_WIDTH {
            return 0;
        }
        let h = column_height(x);
        let top = (TILE_HEIGHT - h) / 2;
        if y < top || y >= top + h {
            return 0;
        }
        let i = y - top;
        // `Transitions::read` takes exactly the bytes the columns need, so these lookups only
        // fail if that size table is wrong; treat that as "not covered" rather than panic.
        let byte = |n: usize| self.data.get(self.columns[x] + n).copied().unwrap_or(0);
        if self.depth == 4 {
            ((byte(i / 2) >> (4 * (i & 1))) & 15) * 17
        } else if (byte(i / 8) >> (i % 8)) & 1 != 0 {
            255
        } else {
            0
        }
    }
}

/// Which named set each of the 20 mask set numbers stored in maps refers to: five numbers per
/// kind of border, some of them sharing a set. This is the table at rva `6877a0` in our exe,
/// which the game walks when it loads the masks (`FUN_008c6010`, rva `4c6010`).
pub const MAP_MASK_SETS: [&str; 20] = [
    "water_to_land 1",
    "water_to_land 1",
    "water_to_land 2",
    "water_to_land 2",
    "water_to_land 3",
    "land_to_water 1",
    "land_to_water 1",
    "land_to_water 2",
    "land_to_water 2",
    "land_to_water 3",
    "road 1",
    "road 1",
    "road 2",
    "road 2",
    "road 2",
    "land 1",
    "land 2",
    "land 2",
    "land 3",
    "land 3",
];

/// All masks of the file: set name -> code -> mask.
pub struct Transitions {
    pub sets: HashMap<String, HashMap<i32, Mask>>,
    /// The codes of each set in file order: maps refer to masks by this position.
    pub order: HashMap<String, Vec<i32>>,
}

impl Transitions {
    pub fn read(data: &[u8]) -> Result<Transitions> {
        let mut r = Reader::new(data);
        let n = r.u8()?;
        let mut sets = HashMap::new();
        let mut order = HashMap::new();
        for _ in 0..n {
            let name = r.str16()?;
            let count = r.u32()?;
            let mut masks = HashMap::new();
            let mut codes = Vec::new();
            for _ in 0..count {
                let code = r.i32()?;
                let depth = r.u8()?;
                let size = match depth {
                    1 => 160,
                    4 => 544,
                    d => return bad(format!("mask {code} in set {name:?} has unknown depth {d}")),
                };
                masks.insert(code, Mask::new(depth, r.take(size)?.to_vec()));
                codes.push(code);
            }
            order.insert(name.clone(), codes);
            sets.insert(name, masks);
        }
        if r.remaining() != 0 {
            return bad(format!("{} bytes left after transitions", r.remaining()));
        }
        Ok(Transitions { sets, order })
    }

    /// A mask as a map refers to it: mask set number (0..=19) and position within the set.
    pub fn map_mask(&self, mask_set: u8, index: u16) -> Option<&Mask> {
        let name = *MAP_MASK_SETS.get(mask_set as usize)?;
        let code = *self.order.get(name)?.get(index as usize)?;
        self.sets.get(name)?.get(&code)
    }

    /// The mask for `code` in a set; if the exact code isn't there, try without the corners
    /// covered only diagonally (digit 4), which are the least visible part.
    pub fn mask(&self, set: &str, code: i32) -> Option<&Mask> {
        let masks = self.sets.get(set)?;
        masks.get(&code).or_else(|| {
            let mut c = 0;
            for place in [1000, 100, 10, 1] {
                let digit = code / place % 10;
                c += if digit == 4 { 0 } else { digit * place };
            }
            masks.get(&c)
        })
    }
}

/// Which of a tile's eight neighbours have the terrain being drawn over it, in screen terms.
#[derive(Debug, Clone, Copy, Default)]
pub struct Neighbours {
    pub n: bool,
    pub ne: bool,
    pub e: bool,
    pub se: bool,
    pub s: bool,
    pub sw: bool,
    pub w: bool,
    pub nw: bool,
}

/// The mask code for a set of covering neighbours (0 = nothing to draw).
pub fn corner_code(nb: Neighbours) -> i32 {
    let digit = |before: bool, after: bool, diagonal: bool| match (before, after) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 2,
        _ if diagonal => 4,
        _ => 0,
    };
    let east = digit(nb.ne, nb.se, nb.e);
    let south = digit(nb.se, nb.sw, nb.s);
    let west = digit(nb.sw, nb.nw, nb.w);
    let north = digit(nb.nw, nb.ne, nb.n);
    east * 1000 + south * 100 + west * 10 + north
}
