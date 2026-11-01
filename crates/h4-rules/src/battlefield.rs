//! Battlefield cell queries recovered from the Buka 3.0 executable.

/// Explicit answers to the two virtual queries used by RVA 183a10.
/// Static scenery producers are recovered below; other object kinds and
/// dynamic compound state still supply these answers explicitly.
#[derive(Debug, Clone, Copy)]
pub struct CellOccupant {
    pub id: u32,
    /// Result of virtual slot +0x5c. A true answer skips this occupant.
    pub skip_blocking: bool,
    /// Result of virtual slot +0x10.
    pub blocks: bool,
}

/// The two bytes changed by preset application (RVA 162aa0).
/// Occupants and the rest of the live cell are unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellSurface {
    pub terrain: u8,
    pub flags: u8,
}

/// Apply one preset byte to an existing cell, preserving unrelated flags.
/// `on_bridge` is the bridge flag of the adventure position, copied into the
/// combat context by RVA 40e5e0 and read by RVA 162aa0. It describes the place
/// of the battle, not the preset resource name.
/// Presets are overlays: zero and unrecognized values can leave a cell intact.
pub fn apply_preset_cell(cell: &mut CellSurface, value: u8, on_bridge: bool) {
    match value {
        0 if on_bridge => cell.flags |= 0x1c,
        1 => {
            cell.terrain = 1;
            cell.flags = (cell.flags & 0xe3) | if on_bridge { 0 } else { 0x10 };
        }
        2 => {
            cell.terrain = 1;
            cell.flags &= 0xe3;
        }
        255 => cell.flags = (cell.flags & 0xef) | 0x0c,
        _ => {}
    }
}

/// Port of `t_battlefield_cell::is_blocked` (RVA 183a10).
/// `flags` is byte +1 of a live battlefield cell, not a preset resource byte.
pub fn cell_is_blocked(
    flags: u8,
    occupants: &[CellOccupant],
    querying_object: Option<u32>,
) -> bool {
    flags & 4 != 0
        || occupants
            .iter()
            .any(|o| !o.skip_blocking && o.blocks && Some(o.id) != querying_object)
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
}

/// Circular footprint used by one native stationary-object segment (RVA 1fc340).
/// Coordinates are offsets from the segment's top-left map cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatFootprint {
    pub size: u8,
}

impl CombatFootprint {
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
#[derive(Debug, Clone, Copy)]
pub struct PlacedObstacle {
    pub at: (i32, i32),
    pub footprint: CombatFootprint,
    pub occupant: CellOccupant,
}

impl PlacedObstacle {
    /// A square blocking resource is a single segment in RVA 238690.
    /// Rectangular blocking resources need the image-dependent segment layout;
    /// accepting their bounding rectangle would invent blocked cells.
    pub fn from_square_model(
        model: &h4_formats::combat_object::CombatObject,
        at: (i32, i32),
        id: u32,
    ) -> Result<Self, &'static str> {
        if model.properties[0] == 0
            || model.properties[0] != model.properties[1]
            || (model.properties[0] > 1
                && model.attributes[0] == 0
                && model.flag == 0
                && !matches!(model.type_id, 0x23 | 0x25))
        {
            return Err("scenery requires a resolved compound segment layout");
        }
        Ok(Self {
            at,
            footprint: CombatFootprint {
                size: model.properties[0],
            },
            occupant: CellOccupant {
                id,
                blocks: model.attributes[0] != 0,
                skip_blocking: model.flag != 0,
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct BattlefieldCell {
    pub surface: CellSurface,
    pub occupants: Vec<CellOccupant>,
}

#[derive(Debug, Clone)]
struct GridRow {
    start: usize,
    cells: Vec<BattlefieldCell>,
}

/// Sparse field geometry, preset overlays and resolved occupants, without graphics.
/// The underlying surface is explicit: preset zero may inherit earlier state.
#[derive(Debug, Clone)]
pub struct BattlefieldGrid {
    rows: Vec<GridRow>,
}

impl BattlefieldGrid {
    pub fn from_preset(
        preset: &h4_formats::battlefield::BattlefieldPreset,
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
    pub fn apply_preset(
        &mut self,
        preset: &h4_formats::battlefield::BattlefieldPreset,
        on_bridge: bool,
    ) {
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

    pub fn cell(&self, x: i32, y: i32) -> Option<&BattlefieldCell> {
        let row = self.rows.get(usize::try_from(x).ok()?)?;
        row.cells
            .get(usize::try_from(y).ok()?.checked_sub(row.start)?)
    }

    /// Outside the stored field cannot be entered.
    pub fn is_blocked(&self, x: i32, y: i32, querying_object: Option<u32>) -> bool {
        self.cell(x, y).is_none_or(|cell| {
            cell_is_blocked(cell.surface.flags, &cell.occupants, querying_object)
        })
    }

    /// Add one resolved segment atomically. IDs identify individual native
    /// objects/segments, not their enclosing compound model. Overlap is retained,
    /// so excluding one segment never hides a second blocker in the same cell.
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
