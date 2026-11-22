//! Does a castle wall stand between a striker and its victim (`1701f0`)? A melee strike
//! asks it for the damage roll ([`crate::damage::AcrossWall`]): the line from the
//! striker's center to the victim's is walked cell by cell, and a castle wall or a closed
//! gate on it changes the damage by the castle's multiplier.
//!
//! Verified whole against the exe in Unicorn (`tools/testbench/wall_line.py`,
//! `tests/wall_line_vectors.rs`). Castles aren't in the battle state yet (#74), so an
//! ordinary battle always answers [`AcrossWall::No`].

use crate::damage::AcrossWall;

/// A castle part standing in a cell, as far as the wall check cares (the obstacle type
/// `477790`: 0x24 `castle_wall`, 0x22 `castle_gate`; anything else is [`CastlePart::None`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastlePart {
    None,
    Wall,
    Gate,
}

/// What the line needs to know about the field.
pub struct WallField<'a> {
    /// Column range of each row (`x`), as [`crate::battlefield::BattlefieldGrid`] has them.
    pub row_bounds: &'a dyn Fn(i32) -> Option<(i32, i32)>,
    /// The castle part in a cell (`t_battlefield_cell::get_obstacle`, then its type).
    pub part: &'a dyn Fn((i32, i32)) -> CastlePart,
    /// The battlefield has a gate (`+0x340`) and it is closed (`1c0570`). Without a gate
    /// object a gate part doesn't block.
    pub gate_closed: bool,
}

/// Walk from `from` to `to` (points in 1/16 of a cell, the two footprint centers) and
/// tell whether a wall or a closed gate is on the way, and which way the line crosses it
/// (the sign of the row difference).
pub fn wall_between(field: &WallField, from: (i32, i32), to: (i32, i32)) -> AcrossWall {
    let (dx, dy) = (to.0.wrapping_sub(from.0), to.1.wrapping_sub(from.1));
    let step_x = if dx >= 0 { 1 } else { -1 };
    let step_y = if dy >= 0 { 1 } else { -1 };
    let (adx, ady) = (dx.wrapping_abs(), dy.wrapping_abs());
    // A line-drawing walk: `n` is the longer side, the error terms start where the
    // first point lies inside its cell.
    let n = adx.max(ady);
    let (mut x, mut y) = (from.0 >> 4, from.1 >> 4);
    let mut error_x = (from.0 - x * 16).wrapping_mul(n) >> 4;
    let mut error_y = (from.1 - y * 16).wrapping_mul(n) >> 4;
    // One step per whole cell of the longer side; the start cell itself isn't looked at.
    for _ in 0..(n >> 4).max(0) {
        error_x += adx;
        error_y += ady;
        if error_x >= n {
            x += step_x;
            error_x -= n;
        }
        if error_y >= n {
            y += step_y;
            error_y -= n;
        }
        let on_field = (field.row_bounds)(x).is_some_and(|(start, end)| start <= y && y < end);
        if !on_field {
            continue;
        }
        let blocked = match (field.part)((x, y)) {
            CastlePart::Wall => true,
            CastlePart::Gate => field.gate_closed,
            CastlePart::None => false,
        };
        if blocked {
            return if step_x < 0 { AcrossWall::TowardSmallerX } else { AcrossWall::TowardGreaterX };
        }
    }
    AcrossWall::No
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_rows(x: i32) -> Option<(i32, i32)> {
        (0..102).contains(&x).then_some((0, 102))
    }

    #[test]
    fn a_wall_row_between_the_two_is_found_with_its_direction() {
        let wall = |at: (i32, i32)| if at.0 == 50 { CastlePart::Wall } else { CastlePart::None };
        let field = WallField { row_bounds: &open_rows, part: &wall, gate_closed: false };
        let (a, b) = ((45 * 16 + 8, 40 * 16 + 8), (55 * 16 + 8, 42 * 16 + 8));
        assert_eq!(wall_between(&field, a, b), AcrossWall::TowardGreaterX);
        assert_eq!(wall_between(&field, b, a), AcrossWall::TowardSmallerX);
        // Both on the same side: nothing in between.
        assert_eq!(wall_between(&field, a, (48 * 16 + 8, 40 * 16 + 8)), AcrossWall::No);
    }

    #[test]
    fn an_open_gate_lets_the_line_through() {
        let gate = |at: (i32, i32)| if at.0 == 50 { CastlePart::Gate } else { CastlePart::None };
        let open = WallField { row_bounds: &open_rows, part: &gate, gate_closed: false };
        let closed = WallField { gate_closed: true, ..open };
        let (a, b) = ((45 * 16 + 8, 40 * 16 + 8), (55 * 16 + 8, 40 * 16 + 8));
        assert_eq!(wall_between(&open, a, b), AcrossWall::No);
        assert_eq!(wall_between(&closed, a, b), AcrossWall::TowardGreaterX);
    }
}
