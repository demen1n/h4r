//! Battlefield cells: preset overlays, obstacle footprints and the
//! "is this cell blocked?" query, recovered from the Buka 3.0 executable.
//!
//! No graphics here. Background and details: `decomp/battlefield.md`,
//! `decomp/combat_object.md`.

use h4_formats::battlefield::BattlefieldPreset;
use h4_formats::combat_object::{CombatObject, CombatObjectSegment};
use serde::{Deserialize, Serialize};

// Bits of the live cell's flag byte (+1). 0x08 is still unexplained; 0x10 is
// named by value because only one of its readers (the obstacle placer) is known.
/// The cell itself cannot be entered (checked by RVA 183a10).
pub const FLAG_BLOCKED: u8 = 0x04;
/// Meaning unknown; set by preset codes 0 (on a bridge) and 255.
pub const FLAG_08: u8 = 0x08;
/// Obstacles may be placed on the cell: the placer rejects cells without it
/// (RVA 209900). Set by preset code 1 off a bridge and by code 0 on one.
pub const FLAG_10: u8 = 0x10;
/// The three bits preset codes 0/1/2 overwrite.
const PRESET_BITS: u8 = FLAG_BLOCKED | FLAG_08 | FLAG_10;

/// Answers to the two virtual queries RVA 183a10 asks of each object in a cell.
/// Static scenery fills them from its resource (`PlacedObstacle`); other
/// object kinds and dynamic compound state still supply them explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellOccupant {
    /// Identity of one native object or segment, used to exclude "myself".
    pub id: u32,
    /// Result of virtual slot +0x5c. A true answer skips this occupant.
    pub skip_blocking: bool,
    /// Result of virtual slot +0x10.
    pub blocks: bool,
}

/// The two bytes changed by preset application (RVA 162aa0).
/// Occupants and the rest of the live cell are unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CellSurface {
    pub terrain: u8,
    pub flags: u8,
}

/// Apply one preset byte to an existing cell, preserving unrelated flags.
///
/// `on_bridge` is the bridge flag of the adventure position, copied into the
/// combat context by RVA 40e5e0 and read by RVA 162aa0. It describes the place
/// of the battle, not the preset resource name.
/// Presets are overlays: zero and unrecognized values can leave a cell intact.
pub fn apply_preset_cell(cell: &mut CellSurface, value: u8, on_bridge: bool) {
    match value {
        // On a bridge, 0 marks the area outside the deck: blocked.
        // On land it keeps whatever the cell already was.
        0 if on_bridge => cell.flags |= PRESET_BITS,
        // 1 and 2 clear the three bits and make the cell ordinary ground
        // (terrain 1); 1 also sets FLAG_10 when the battle is not on a bridge.
        1 => {
            cell.terrain = 1;
            cell.flags = (cell.flags & !PRESET_BITS) | if on_bridge { 0 } else { FLAG_10 };
        }
        2 => {
            cell.terrain = 1;
            cell.flags &= !PRESET_BITS;
        }
        // 255 blocks the cell and clears FLAG_10.
        255 => cell.flags = (cell.flags & !FLAG_10) | FLAG_BLOCKED | FLAG_08,
        _ => {}
    }
}

/// Port of `t_battlefield_cell::is_blocked` (RVA 183a10).
///
/// `flags` is byte +1 of a live battlefield cell, not a preset resource byte.
/// `querying_object` is the object asking (e.g. a creature that already stands
/// here): it never blocks itself.
pub fn cell_is_blocked(
    flags: u8,
    occupants: &[CellOccupant],
    querying_object: Option<u32>,
) -> bool {
    flags & FLAG_BLOCKED != 0
        || occupants
            .iter()
            .any(|o| !o.skip_blocking && o.blocks && Some(o.id) != querying_object)
}

/// Rounded square footprint of one stationary-object segment (RVA 1fc340).
/// Coordinates are offsets from the segment's top-left map cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatFootprint {
    pub size: u8,
}

impl CombatFootprint {
    /// A cell belongs to the footprint when its center lies inside a circle
    /// slightly larger than the inscribed one. Everything is doubled to stay
    /// in integers: the center of cell x is at 2x + 1, of the square at `size`.
    /// Up to size 4 the corners are still inside; from size 5 they are cut off.
    pub fn contains(self, x: i32, y: i32) -> bool {
        let size = i32::from(self.size);
        if x < 0 || y < 0 || x >= size || y >= size {
            return false;
        }
        let dx = 2 * x + 1 - size;
        let dy = 2 * y + 1 - size;
        dx * dx + dy * dy <= size * size + 2
    }

    pub fn cells(self) -> impl Iterator<Item = (i32, i32)> {
        let size = i32::from(self.size);
        (0..size)
            .flat_map(move |x| (0..size).filter_map(move |y| self.contains(x, y).then_some((x, y))))
    }
}

/// One resolved segment. Compound scenery can supply several of these.
/// Dynamic changes (e.g. a destroyed gate) must update the occupant inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedObstacle {
    pub at: (i32, i32),
    pub footprint: CombatFootprint,
    pub occupant: CellOccupant,
}

impl PlacedObstacle {
    /// Place an image-resolved segment. Each segment needs its own identity.
    pub fn from_segment(
        model: &CombatObject,
        segment: &CombatObjectSegment,
        at: (i32, i32),
        id: u32,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            at: (
                at.0.checked_add(segment.at.0)
                    .ok_or("obstacle coordinate overflow")?,
                at.1.checked_add(segment.at.1)
                    .ok_or("obstacle coordinate overflow")?,
            ),
            footprint: CombatFootprint { size: segment.size },
            occupant: Self::occupant(model, id),
        })
    }

    /// Shortcut for a model that RVA 238690 keeps as one square segment.
    /// Anything else needs `CombatObject::segments`: the image decides which
    /// pieces exist, so the bounding rectangle would invent blocked cells.
    pub fn from_square_model(
        model: &CombatObject,
        at: (i32, i32),
        id: u32,
    ) -> Result<Self, &'static str> {
        let [width, height] = model.header.properties;
        if width == 0 || width != height || model.segment_size() != width {
            return Err("scenery requires a resolved compound segment layout");
        }
        Ok(Self {
            at,
            footprint: CombatFootprint { size: width },
            occupant: Self::occupant(model, id),
        })
    }

    /// Static answers of a segment (RVA 477620 / 477660).
    fn occupant(model: &CombatObject, id: u32) -> CellOccupant {
        CellOccupant {
            id,
            blocks: model.blocks(),
            skip_blocking: model.skips_blocking(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattlefieldCell {
    pub surface: CellSurface,
    pub occupants: Vec<CellOccupant>,
}

/// One stored row: cells `start..start + cells.len()` (the field is a
/// diamond inside a square, so rows have different bounds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GridRow {
    start: usize,
    cells: Vec<BattlefieldCell>,
}

/// Sparse field geometry, preset overlays and resolved occupants, without graphics.
/// The underlying surface is explicit: preset zero may inherit earlier state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattlefieldGrid {
    rows: Vec<GridRow>,
}

impl BattlefieldGrid {
    /// The standard 102-row field with every cell set to `underlying`
    /// (no preset). The game builds this from the terrain around the battle;
    /// that generator is not ported yet, so the caller supplies the surface.
    pub fn standard(underlying: CellSurface) -> Self {
        let rows = h4_formats::battlefield::row_bounds()
            .map(|(start, end)| GridRow {
                start,
                cells: vec![
                    BattlefieldCell {
                        surface: underlying,
                        occupants: Vec::new(),
                    };
                    end - start
                ],
            })
            .collect();
        Self { rows }
    }

    /// Number of stored rows (valid `x` are `0..row_count()`).
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Half-open range of valid `y` in row `x`.
    pub fn row_bounds(&self, x: i32) -> Option<(i32, i32)> {
        let row = self.rows.get(usize::try_from(x).ok()?)?;
        let end = row.start.checked_add(row.cells.len())?;
        Some((i32::try_from(row.start).ok()?, i32::try_from(end).ok()?))
    }

    /// Port of RVA 162cb0, run after the presets and before obstacles: block
    /// the first and last cell of every row, and every cell with x + y < 55
    /// (the top corner of the diamond, far behind the backdrop's horizon).
    pub fn block_edges(&mut self) {
        for (x, row) in self.rows.iter_mut().enumerate() {
            let start = row.start;
            let count = row.cells.len();
            for (i, cell) in row.cells.iter_mut().enumerate() {
                let y = start + i;
                if i == 0 || i + 1 == count || x + y < 55 {
                    cell.surface.flags |= FLAG_BLOCKED;
                }
            }
        }
    }

    /// Field with the preset's geometry: every cell starts as `underlying`,
    /// then the preset is laid over it.
    pub fn from_preset(
        preset: &BattlefieldPreset,
        underlying: CellSurface,
        on_bridge: bool,
    ) -> Self {
        let rows = preset
            .rows
            .iter()
            .map(|row| GridRow {
                start: row.start,
                cells: row
                    .cells
                    .iter()
                    .map(|&code| {
                        let mut surface = underlying;
                        apply_preset_cell(&mut surface, code, on_bridge);
                        BattlefieldCell {
                            surface,
                            occupants: Vec::new(),
                        }
                    })
                    .collect(),
            })
            .collect();
        Self { rows }
    }

    /// Like RVA 162aa0: skip a different row count, and skip individual rows
    /// with different bounds. Occupants and unmatched surfaces stay intact.
    pub fn apply_preset(&mut self, preset: &BattlefieldPreset, on_bridge: bool) {
        if self.rows.len() != preset.rows.len() {
            return;
        }
        for (row, overlay) in self.rows.iter_mut().zip(&preset.rows) {
            if row.start != overlay.start || row.cells.len() != overlay.cells.len() {
                continue;
            }
            for (cell, &code) in row.cells.iter_mut().zip(&overlay.cells) {
                apply_preset_cell(&mut cell.surface, code, on_bridge);
            }
        }
    }

    /// `x` selects the row, `y` the cell within it (same as `BattlefieldPreset::cell`).
    pub fn cell(&self, x: i32, y: i32) -> Option<&BattlefieldCell> {
        let row = self.rows.get(usize::try_from(x).ok()?)?;
        row.cells
            .get(usize::try_from(y).ok()?.checked_sub(row.start)?)
    }

    /// Change the terrain and flags of one cell (for building fields in code).
    pub fn surface_mut(&mut self, x: i32, y: i32) -> Option<&mut CellSurface> {
        let row = self.rows.get_mut(usize::try_from(x).ok()?)?;
        let index = usize::try_from(y).ok()?.checked_sub(row.start)?;
        Some(&mut row.cells.get_mut(index)?.surface)
    }

    /// Outside the stored field cannot be entered.
    pub fn is_blocked(&self, x: i32, y: i32, querying_object: Option<u32>) -> bool {
        self.cell(x, y).is_none_or(|cell| {
            cell_is_blocked(cell.surface.flags, &cell.occupants, querying_object)
        })
    }

    /// Add one resolved segment atomically: either every cell of its footprint
    /// is inside the field and gets the occupant, or nothing changes.
    /// IDs identify individual native objects/segments, not their enclosing
    /// compound model. Overlap is retained, so excluding one segment never
    /// hides a second blocker in the same cell.
    pub fn add_obstacle(&mut self, obstacle: PlacedObstacle) -> Result<(), &'static str> {
        if obstacle.footprint.size == 0 {
            return Err("empty obstacle footprint");
        }
        if self.rows.iter().flat_map(|row| &row.cells).any(|cell| {
            cell.occupants
                .iter()
                .any(|occupant| occupant.id == obstacle.occupant.id)
        }) {
            return Err("obstacle ID is already placed");
        }
        // Check every cell first, change the field afterwards.
        let cells: Vec<_> = obstacle
            .footprint
            .cells()
            .map(|(x, y)| Some((obstacle.at.0.checked_add(x)?, obstacle.at.1.checked_add(y)?)))
            .collect::<Option<_>>()
            .ok_or("obstacle coordinate overflow")?;
        if cells.iter().any(|&(x, y)| self.cell(x, y).is_none()) {
            return Err("obstacle footprint leaves the stored battlefield");
        }
        for (x, y) in cells {
            // `self.cell(x, y)` succeeded above, so both indices are valid.
            let row = &mut self.rows[x as usize];
            row.cells[y as usize - row.start]
                .occupants
                .push(obstacle.occupant);
        }
        Ok(())
    }

    pub fn remove_object(&mut self, id: u32) {
        for row in &mut self.rows {
            for cell in &mut row.cells {
                cell.occupants.retain(|occupant| occupant.id != id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_flag_takes_precedence_and_only_other_blockers_count() {
        for flags in 0..=u8::MAX {
            for skip_blocking in [false, true] {
                for blocks in [false, true] {
                    let occupant = CellOccupant {
                        id: 7,
                        skip_blocking,
                        blocks,
                    };
                    assert_eq!(cell_is_blocked(flags, &[occupant], Some(7)), flags & 4 != 0);
                    assert_eq!(
                        cell_is_blocked(flags, &[occupant], Some(8)),
                        flags & 4 != 0 || (!skip_blocking && blocks)
                    );
                    assert_eq!(cell_is_blocked(flags, &[], None), flags & 4 != 0);
                }
            }
        }
        let occupants = [
            CellOccupant {
                id: 7,
                skip_blocking: false,
                blocks: true,
            },
            CellOccupant {
                id: 8,
                skip_blocking: false,
                blocks: true,
            },
        ];
        assert!(cell_is_blocked(0, &occupants, Some(7)));
    }

    #[test]
    fn named_flags_match_the_executable_masks() {
        // The masks as they appear in RVA 162aa0.
        assert_eq!(PRESET_BITS, 0x1c);
        assert_eq!(!PRESET_BITS, 0xe3);
        assert_eq!(!FLAG_10, 0xef);
        assert_eq!(FLAG_BLOCKED | FLAG_08, 0x0c);
    }
}
