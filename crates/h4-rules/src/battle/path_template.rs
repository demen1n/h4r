//! The template path map of one footprint size, as the battlefield builds and caches
//! it (cache rva `10e4f0`): first a blockage map of the whole field (`20e900`), then,
//! for every cell, whether a footprint of that size fits there and what it would stand
//! on (`20dfd0`, with the fit check `1650d0` and the blockage query `20eb20`).
//! The path finder of every creature of that size starts from a copy of this map
//! (`decomp/combat_path.md`; verified by executing the original code:
//! `tests/combat_path_template_vectors.tsv`).
//!
//! What stands where is the caller's input: the positions, sizes and kinds of the
//! objects and the cells with the live "blocked" flag (`battlefield::FLAG_BLOCKED`).

use crate::battle::path::{flags, PathCell, PathMap};
use crate::battlefield::CombatFootprint;

/// Bits of a blockage value. The exe keeps a u32 per cell, but only these four bits
/// mean something (the rest come from a register that held the battlefield's address).
pub mod blockage {
    /// An obstacle, or a castle wall (with [`WALL`]). Becomes "occupied" in the template.
    pub const OBSTACLE: u32 = 0x1;
    /// The live cell is blocked (set together with [`OBSTACLE`]). Becomes path flag 0x08,
    /// which the search doesn't read.
    pub const BLOCKED_CELL: u32 = 0x2;
    /// A castle tower. Becomes the tower flag.
    pub const TOWER: u32 = 0x4;
    /// A castle wall. Becomes the wall flag.
    pub const WALL: u32 = 0x8;
}

/// What an object in the battlefield's lists is, for the blockage map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockerKind {
    /// Anything of the first list (`+0x44c`, all combat objects): marked as an obstacle,
    /// if it blocks and is fixed.
    Object { blocks: bool, fixed: bool },
    /// The second list (`+0x538`, castle parts), by object type: `castle_tower`.
    Tower,
    /// `castle_wall`.
    Wall,
    /// Any other type in the second list (`castle_gate`): not marked here; gates are
    /// handled per creature.
    OtherCastlePart,
}

/// One object for the blockage map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocker {
    pub kind: BlockerKind,
    /// Corner of the footprint in subcells (16 per cell), as objects store it.
    pub position: (i32, i32),
    /// Footprint side in cells (virtual `+8`); the footprint is the round mask.
    pub size: u8,
}

/// The field's blockage map: one value per cell (bits of [`blockage`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockageMap {
    /// Columns `start..end` of each row.
    rows: Vec<(i32, i32)>,
    /// One value per cell of each row.
    values: Vec<Vec<u32>>,
}

impl BlockageMap {
    /// Build it like `20e900`. `blockers` are in list order: the first list, then the
    /// second; `blocked` tells whether a live cell has the "blocked" flag.
    pub fn build(rows: &[(i32, i32)], blockers: &[Blocker], blocked: impl Fn(i32, i32) -> bool) -> Self {
        let values = rows.iter().map(|&(start, end)| vec![0; (end - start).max(0) as usize]).collect();
        let mut result = BlockageMap { rows: rows.to_vec(), values };
        for blocker in blockers {
            let value = match blocker.kind {
                BlockerKind::Object { blocks: true, fixed: true } => blockage::OBSTACLE,
                BlockerKind::Object { .. } | BlockerKind::OtherCastlePart => continue,
                BlockerKind::Tower => blockage::TOWER,
                BlockerKind::Wall => blockage::OBSTACLE | blockage::WALL,
            };
            // Subcells to cells: an arithmetic shift, so -1 becomes -1, not 0.
            let corner = (blocker.position.0 >> 4, blocker.position.1 >> 4);
            // Later objects overwrite earlier ones (the exe assigns, it doesn't add).
            for (dx, dy) in (CombatFootprint { size: blocker.size }).cells() {
                result.set((corner.0 + dx, corner.1 + dy), value);
            }
        }
        // Finally the blocked live cells, from the last row to the first.
        for x in (0..rows.len() as i32).rev() {
            let (start, end) = rows[x as usize];
            for y in start..end {
                if blocked(x, y) {
                    let value = result.get((x, y)).expect("inside the row");
                    result.set((x, y), value | blockage::OBSTACLE | blockage::BLOCKED_CELL);
                }
            }
        }
        result
    }

    /// The value of a cell; `None` outside the field.
    pub fn get(&self, at: (i32, i32)) -> Option<u32> {
        let (x, y) = self.index(at)?;
        Some(self.values[x][y])
    }

    fn set(&mut self, at: (i32, i32), value: u32) {
        if let Some((x, y)) = self.index(at) {
            self.values[x][y] = value;
        }
    }

    /// Row and position in the row's values, if the cell is on the field.
    fn index(&self, (x, y): (i32, i32)) -> Option<(usize, usize)> {
        let row = usize::try_from(x).ok()?;
        let &(start, end) = self.rows.get(row)?;
        (y >= start && y < end).then(|| (row, (y - start) as usize))
    }

    /// What a footprint with its corner at `at` would stand on (`20eb20`): the bits of
    /// (in the template the fit check comes first, so the "outside" answer below is never
    /// reached there and the template vectors don't exercise it)
    /// every covered cell together; a cell outside the field counts as
    /// [`blockage::OBSTACLE`] | [`blockage::BLOCKED_CELL`] and stops the scan.
    pub fn under(&self, at: (i32, i32), footprint: CombatFootprint) -> u32 {
        let mut bits = 0;
        for (dx, dy) in footprint.cells() {
            match self.get((at.0 + dx, at.1 + dy)) {
                Some(value) => bits |= value & 0xf,
                None => return bits | blockage::OBSTACLE | blockage::BLOCKED_CELL,
            }
        }
        bits
    }
}

/// Does a footprint with its corner at `at` stay on the field (`1650d0`, inverted: the
/// exe answers "sticks out")? Only the first and the last cell of each mask row are
/// checked — enough, because rows of the field have no gaps.
pub fn footprint_fits(rows: &[(i32, i32)], at: (i32, i32), footprint: CombatFootprint) -> bool {
    let size = i32::from(footprint.size);
    let inside = |x: i32, y: i32| {
        usize::try_from(x).ok().and_then(|x| rows.get(x)).is_some_and(|&(start, end)| y >= start && y < end)
    };
    for dx in 0..size {
        // Every row of the round mask has cells: its first and last are symmetric.
        let first = (0..size).find(|&dy| footprint.contains(dx, dy)).unwrap_or(0);
        let last = (0..size).rev().find(|&dy| footprint.contains(dx, dy)).unwrap_or(size - 1);
        if !inside(at.0 + dx, at.1 + first) || !inside(at.0 + dx, at.1 + last) {
            return false;
        }
    }
    true
}

/// The template path map for a footprint size (`20dfd0`). Every record is zero except
/// the flags: where the footprint doesn't fit, impassable and occupied; elsewhere what
/// the footprint would stand on: an obstacle → occupied, a blocked cell → 0x08,
/// a tower → [`flags::TOWER`], a wall → [`flags::WALL`].
pub fn template(rows: &[(i32, i32)], blockage: &BlockageMap, footprint_size: u8) -> PathMap {
    let footprint = CombatFootprint { size: footprint_size };
    let mut map = PathMap::new(rows);
    for x in 0..rows.len() as i32 {
        let (start, end) = rows[x as usize];
        for y in start..end {
            let bits = if !footprint_fits(rows, (x, y), footprint) {
                flags::IMPASSABLE | flags::OCCUPIED
            } else {
                let under = blockage.under((x, y), footprint);
                // bit 0 → 0x04, bit 1 → 0x08, bit 2 → 0x20, bit 3 → 0x40
                (((under & 0xc) << 1) | (under & 0x3)) << 2
            };
            *map.cell_mut((x, y)).expect("inside the row") = PathCell {
                enter_cost: 0,
                over_occupied: 0,
                cost: 0,
                flags: bits,
                from: (0, 0),
                origin: (0, 0),
            };
        }
    }
    map
}
