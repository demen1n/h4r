//! Zones of control on the battlefield: which cells a creature threatens, and in which
//! directions a step costs the zone penalty for a creature standing in them
//! (`decomp/zone_of_control.md`; verified by executing the original code:
//! `tests/zone_of_control_vectors.tsv`).
//!
//! Every creature threatens the cells of its threat footprint (`threat_footprint.cpp`):
//! a round area of side `size + 2 · reach` (reach 2, or 6 with `long_weapon`) around it,
//! cut to an arc of 200° in front of it (where it faces), or the whole circle with
//! `hydra_strike`. A creature standing on a cell threatened by a living enemy pays the
//! penalty (a quarter of its movement, in [`crate::battle::path`]) for steps in every
//! direction except straight toward or away from each such enemy. A flier is held only by
//! flying enemies; `ignore_zones_of_control` frees a creature from all of them.
//!
//! Where a creature faces comes from the battle state ([`crate::battle::state::BattleUnit::facing`]).

use std::collections::HashMap;

use crate::battle::attack_angles::angle_difference;
use crate::battle::path_prepare::{angle, direction};
use crate::battle::state::{BattleState, UnitId, UnitKind};
use crate::battle::Side;
use crate::battlefield::CombatFootprint;

/// The cells a creature threatens, relative to the corner of a `width` × `width` square
/// (`48b090`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreatFootprint {
    pub width: i32,
    /// Row by row: `cells[i * width + j]`.
    pub cells: Vec<bool>,
}

impl ThreatFootprint {
    /// Build it (`48b090`). A cell is in when its doubled offset from the center (a, b)
    /// has `a² + b² ≤ width²`, and, for an arc under 360°, its angle is within half the
    /// arc of the facing (direction `facing`, 45° apart).
    pub fn new(width: i32, facing: u32, arc: i32) -> Self {
        let mut cells = Vec::new();
        for i in 0..width.max(0) {
            let a = 1 - width + 2 * i;
            for j in 0..width {
                let b = 1 - width + 2 * j;
                let mut inside = a * a + b * b <= width * width;
                if inside && arc < 360 {
                    let difference = angle_difference(angle((a, b)), facing as i32 * 45);
                    if arc < difference * 2 {
                        inside = false;
                    }
                }
                cells.push(inside);
            }
        }
        ThreatFootprint { width, cells }
    }

    pub fn contains(&self, i: i32, j: i32) -> bool {
        (0..self.width).contains(&i) && (0..self.width).contains(&j) && self.cells[(i * self.width + j) as usize]
    }
}

/// A creature on the field, as zones of control ask about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    pub side: Side,
    /// Its number is not 0 (virtual `+0x40`).
    pub alive: bool,
    /// Corner of its footprint in subcells.
    pub position: (i32, i32),
    /// Footprint side in cells.
    pub size: i32,
    /// Where it faces: a direction of [`crate::battle::path::DIRECTIONS`] (`+0x6c`).
    pub facing: u32,
    /// Ability 0x25 `long_weapon`: reach 6 instead of 2.
    pub long_weapon: bool,
    /// Ability 0x1d `hydra_strike`: it threatens all around (arc 360° instead of 200°).
    pub hydra_strike: bool,
    /// No effect 7, and ability `flying` or effect 0x2e.
    pub flies: bool,
}

impl Threat {
    /// The threat footprint (`1efd60`): side `size + 2 · reach`, the arc, the facing.
    pub fn footprint(&self) -> ThreatFootprint {
        let reach = if self.long_weapon { 6 } else { 2 };
        let arc = if self.hydra_strike { 360 } else { 200 };
        ThreatFootprint::new(self.size + 2 * reach, self.facing, arc)
    }

    /// The center of its footprint in subcells.
    fn center(&self) -> (i32, i32) {
        (self.position.0 + self.size * 8, self.position.1 + self.size * 8)
    }
}

/// Which creatures threaten each cell of the field: the threat lists of the battlefield's
/// cells after adding every creature (`172000`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreatMap {
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl ThreatMap {
    /// Add the threats in order (`172000` for each): the footprint is centered on the
    /// creature (its corner `position >> 4` minus `(width − size) / 2`), cells off the
    /// field are skipped, a creature is listed once per cell.
    pub fn build(rows: &[(i32, i32)], threats: &[Threat]) -> Self {
        let mut map = ThreatMap::default();
        for (index, threat) in threats.iter().enumerate() {
            let footprint = threat.footprint();
            // (width − size) / 2 as the exe divides: toward zero (the difference is even).
            let shift = (footprint.width - threat.size) / 2;
            let corner = ((threat.position.0 >> 4) - shift, (threat.position.1 >> 4) - shift);
            for i in 0..footprint.width {
                for j in 0..footprint.width {
                    let (x, y) = (corner.0 + i, corner.1 + j);
                    let on_field = usize::try_from(x)
                        .ok()
                        .and_then(|x| rows.get(x))
                        .is_some_and(|&(start, end)| y >= start && y < end);
                    if footprint.contains(i, j) && on_field {
                        let list = map.cells.entry((x, y)).or_default();
                        if !list.contains(&index) {
                            list.push(index);
                        }
                    }
                }
            }
        }
        map
    }

    /// The creatures threatening a cell, in the order they were added.
    pub fn at(&self, cell: (i32, i32)) -> &[usize] {
        self.cells.get(&cell).map_or(&[], Vec::as_slice)
    }
}

/// The creature asking where its zone penalties are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneMover {
    pub side: Side,
    /// Footprint side in cells.
    pub size: i32,
    /// No effect 7, and ability `flying` or effect 0x2e: only flying enemies hold it.
    pub flies: bool,
    /// Ability 0x20 `ignore_zones_of_control`.
    pub ignores_zones: bool,
}

/// The zone-of-control directions of a mover with its corner at `at` (`172210`): for every
/// cell of its footprint, every living enemy threatening it (a flying one, for a flier)
/// turns the penalty on in all eight directions, except toward and away from that enemy.
/// Cells off the field are skipped (the exe doesn't check, but the path search never puts
/// a footprint there).
pub fn zone_directions(map: &ThreatMap, threats: &[Threat], mover: ZoneMover, at: (i32, i32)) -> [bool; 8] {
    if mover.ignores_zones {
        return [false; 8];
    }
    let center = (at.0 * 16 + mover.size * 8, at.1 * 16 + mover.size * 8);
    let mut zone: Option<[bool; 8]> = None;
    let footprint = CombatFootprint { size: mover.size.clamp(0, 255) as u8 };
    for (dx, dy) in footprint.cells() {
        for &index in map.at((at.0 + dx, at.1 + dy)) {
            let enemy = &threats[index];
            if enemy.side == mover.side || !enemy.alive || (mover.flies && !enemy.flies) {
                continue;
            }
            let directions = zone.get_or_insert([true; 8]);
            // The direction from the enemy to the mover, both ways along its line.
            let d = (direction(enemy.center(), center) & 3) as usize;
            directions[d] = false;
            directions[d + 4] = false;
        }
    }
    zone.unwrap_or([false; 8])
}

/// What the state doesn't know about a unit, for zones of control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ZoneTraits {
    pub long_weapon: bool,
    pub hydra_strike: bool,
    /// No effect 7, and ability `flying` or effect 0x2e.
    pub flies: bool,
    /// Ability 0x20 `ignore_zones_of_control`.
    pub ignores_zones: bool,
}

/// The zone-of-control directions for `mover` on this battle, as the path search asks for
/// them (`zone` of [`crate::battle::move_command::move_unit`]): every unit on the field is a
/// threat (in unit order; a stack is alive while it has creatures, a hero always) facing
/// the way the state says; `traits` gives the abilities and effects.
pub fn zones_for(
    state: &BattleState,
    mover: UnitId,
    traits: impl Fn(UnitId) -> ZoneTraits,
) -> Result<impl Fn((i32, i32)) -> [bool; 8], String> {
    let unit = state.unit(mover).ok_or("unknown battle unit")?;
    let size = unit.position.ok_or("the unit is not on the field")?.size;
    let own = traits(mover);
    let zone_mover = ZoneMover { side: unit.side, size: i32::from(size), flies: own.flies, ignores_zones: own.ignores_zones };
    let threats: Vec<Threat> = state
        .units()
        .iter()
        .filter_map(|u| {
            let at = u.position?;
            let t = traits(u.id);
            Some(Threat {
                side: u.side,
                alive: match &u.kind {
                    UnitKind::Creature { count, .. } => *count != 0,
                    UnitKind::Hero(_) => true,
                },
                position: (at.at.0 * 16, at.at.1 * 16),
                size: i32::from(at.size),
                facing: u32::from(u.facing),
                long_weapon: t.long_weapon,
                hydra_strike: t.hydra_strike,
                flies: t.flies,
            })
        })
        .collect();
    let field = state.field();
    let rows: Vec<(i32, i32)> = (0..field.row_count() as i32)
        .map(|x| field.row_bounds(x).expect("x is a row index"))
        .collect();
    let map = ThreatMap::build(&rows, &threats);
    Ok(move |at| zone_directions(&map, &threats, zone_mover, at))
}
