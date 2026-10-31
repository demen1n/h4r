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

/// Port of `t_battlefield_cell::is_blocked` (RVA 183a10).
/// `flags` is byte +1 of a live battlefield cell, not a preset resource byte.
pub fn cell_is_blocked(flags: u8, occupants: &[CellOccupant], querying_object: Option<u32>) -> bool {
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
