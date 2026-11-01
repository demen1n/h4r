//! Battlefield cell queries recovered from the Buka 3.0 executable.

/// Explicit answers to the two virtual queries used by RVA 183a10.
/// Their producers still need to be recovered for each combat object type.
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
