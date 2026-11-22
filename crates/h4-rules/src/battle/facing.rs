//! Which way a combat unit faces after a strike or a straight move (`decomp/facing.md`).
//!
//! A facing is an index 0..8 into [`crate::battle::path::DIRECTIONS`]. Both functions
//! here are the exe's arithmetic, executed in Unicorn and compared
//! (`tools/testbench/facing.py`, `tests/facing_vectors.rs`):
//!
//! - [`toward`] — `1684a0`, a striker (or a retaliator) turning to its victim;
//! - [`straight`] — `11ee50` + `11edb0`, a flier landing or a teleporter arriving.
//!
//! A walker isn't here: it ends facing the direction of its last step, which the path
//! search already wrote into the target's record ([`crate::battle::path::PathCell::direction`]).

use crate::battle::path_prepare::{angle, direction};

/// An angle in whole degrees (0..360, as [`angle`] gives it) to one of eight directions
/// (`11edb0`): sectors of 45° centered on the directions.
pub fn sector(angle: i32) -> u8 {
    // The exe divides with a magic multiplier; for the angles it gets (0..360) this is
    // the plain division, rounded toward zero.
    (((2 * angle + 45) / 90) & 7) as u8
}

/// The direction from one footprint's center to another's (`1684a0`). Corners are in
/// cells, sizes in cells. The exe works in 1/65536 of a cell: center = corner · 0x10000
/// + size · 0x8000 (half the footprint), then the direction between the two centers.
pub fn toward(who: (i32, i32), who_size: u8, whom: (i32, i32), whom_size: u8) -> u8 {
    let center = |at: (i32, i32), size: u8| {
        let half = i32::from(size) * 0x8000;
        (at.0 * 0x10000 + half, at.1 * 0x10000 + half)
    };
    direction(center(who, who_size), center(whom, whom_size)) as u8
}

/// The direction of the straight line from one point to another (`11ee50`, then
/// `11edb0`), as a flight or a teleport turns the creature: from its position to where
/// it lands, both in 1/16 of a cell. `11ee50` is the same angle as [`angle`].
pub fn straight(from: (i32, i32), to: (i32, i32)) -> u8 {
    sector(angle((to.0 - from.0, to.1 - from.1)))
}
