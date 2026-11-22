//! What stepping onto rough ground costs a creature in battle: its list of
//! (terrain bits, entry cost) pairs (`225320`, `decomp/terrain_costs.md`). The path map
//! preparation (`20c7f0`, [`crate::battle::path_prepare`]) gives a free cell the cost of
//! the **last** entry whose bits match the cell's terrain bits, so the order of the list
//! matters: the exe sorts it by cost with MSVC's `std::sort` (for this short list, its
//! insertion sort), ported here step by step.
//!
//! The other half is the cell's terrain bits: the combat terrain map (`224e50`, one per
//! footprint size) keeps for every corner the OR of [`cell_terrain_bits`] over the
//! footprint ([`footprint_terrain_bits`], `224f60`).
//!
//! Both are verified whole against the exe in Unicorn (`tools/testbench/terrain_costs.py`,
//! `tests/terrain_costs_vectors.rs`).

use crate::battlefield::{BattlefieldGrid, CellSurface, CombatFootprint};

/// Terrain bits of a cell for terrain type `t` (1..=8, grass … subterranean): the plain
/// kind (`0x984158`: `1 << t`).
pub fn plain_bits(terrain: u32) -> u32 {
    1 << terrain
}

/// The second kind of the same terrain, 50 dearer (`0x9841a0`: `0x400 << t`).
pub fn rough_bits(terrain: u32) -> u32 {
    0x400 << terrain
}

/// Bits that cost half of the creature's movement (when it has more than 200).
pub const HALF_MOVEMENT: u32 = 0x400;
/// Bits that cost a quarter of the creature's movement (when it has more than 400).
pub const QUARTER_MOVEMENT: u32 = 0x200;

/// Entry cost of the plain kind of terrain types 1..=8 (VA `0x9841e4`): grass 100,
/// rough 125, swamp 150, volcanic 125, snow 150, sand 150, dirt 100, subterranean 100.
pub const TERRAIN_COSTS: [i32; 8] = [100, 125, 150, 125, 150, 150, 100, 100];

/// Flag of the cell byte `+1`: the second (rough) kind of the cell's terrain.
pub const CELL_ROUGH: u8 = 0x01;
/// Flag of the cell byte `+1`: the cell costs half of the movement (bit
/// [`HALF_MOVEMENT`]); not for the creature standing on it. Who sets it isn't known yet.
pub const CELL_HALF_MOVEMENT: u8 = 0x02;
/// Flag of the cell byte `+1`: the cell counts as plain grass, and the two special
/// bits above don't apply. Who sets it isn't known yet.
pub const CELL_AS_GRASS: u8 = 0x40;

/// Terrain bits of one cell (the body of `224f60`'s loop). `special` is false only when
/// the bits are refreshed for a creature and this cell is under that same creature
/// (`t_battlefield_cell::get_creature`): then the half-movement bit (and the water bit
/// of terrain 9) of its own cells don't count.
///
/// Terrain types 9 and above (river, lava, ice, special ones) take their bits through a
/// runtime table (`484a10`) that isn't ported: they are an error.
pub fn cell_terrain_bits(surface: CellSurface, special: bool) -> Result<u32, String> {
    let mut terrain = u32::from(surface.terrain & 0x1f);
    let mut bits = 0;
    if surface.flags & CELL_AS_GRASS != 0 {
        terrain = 1;
    } else if special {
        if terrain == 9 {
            bits |= 1;
        }
        if surface.flags & CELL_HALF_MOVEMENT != 0 {
            bits |= HALF_MOVEMENT;
        }
    }
    if terrain > 8 {
        return Err(format!("terrain type {terrain} has no ported terrain bits"));
    }
    bits |= if surface.flags & CELL_ROUGH == 0 {
        plain_bits(terrain)
    } else if terrain == 0 {
        // The rough table has no entry for water (0x9841a0 starts at 1).
        0
    } else {
        rough_bits(terrain)
    };
    Ok(bits)
}

/// The terrain bits of a footprint with its corner at `corner` (`224f60`): the OR of
/// [`cell_terrain_bits`] over its cells that are on the field. `special(cell)` tells
/// whether the cell's special bits count (see there); the whole map is built with all
/// of them counting (`224e50`), a refresh for a creature leaves out its own cells.
pub fn footprint_terrain_bits(
    grid: &BattlefieldGrid,
    corner: (i32, i32),
    size: u8,
    special: impl Fn((i32, i32)) -> bool,
) -> Result<u32, String> {
    let mut bits = 0;
    for (dx, dy) in (CombatFootprint { size }).cells() {
        let at = (corner.0 + dx, corner.1 + dy);
        // Off the field (outside the rows or their column ranges) adds nothing.
        if let Some(cell) = grid.cell(at.0, at.1) {
            bits |= cell_terrain_bits(cell.surface, special(at))?;
        }
    }
    Ok(bits)
}

/// The list as the path map preparation takes it: the exe keeps a cell's entry cost in
/// 16 bits (`PathCell::enter_cost`), so a cost is stored as its low 16 bits.
pub fn as_entry_costs(list: &[(u32, i32)]) -> Vec<(u32, u16)> {
    list.iter().map(|&(bits, cost)| (bits, cost as u16)).collect()
}

/// What `225320` asks the creature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerrainCostInput {
    /// Spell effect 0xb3 `terrain walk`: no terrain costs at all.
    pub terrain_walk: bool,
    /// The level of skill 0xf of the creature's hero (`+0x14` of the stats gives the
    /// hero itself for a hero and nothing for a creature, then virtual `+0x50`); `None`
    /// without a hero. Level 2 or more removes the terrain costs, any level takes 25 off.
    pub hero_skill_0f: Option<i32>,
    /// The creature's movement points ([`crate::battle::move_points`]).
    pub movement: i32,
    /// Terrain types 1..=8 that cost the creature 25 less (virtual `+0x64` of the stats,
    /// index 0 = type 1): native terrain and terrain artifacts, as the caller finds them.
    pub at_home: [bool; 8],
}

/// The creature's (terrain bits, entry cost) list, sorted as the exe sorts it.
pub fn terrain_costs(input: &TerrainCostInput) -> Vec<(u32, i32)> {
    if input.terrain_walk {
        return Vec::new();
    }
    let mut list = Vec::new();
    // Deep ground (mud, deep snow?) costs a share of the movement, not a fixed price.
    if input.movement > 200 {
        list.push((HALF_MOVEMENT, input.movement >> 1));
    }
    if input.movement > 400 {
        list.push((QUARTER_MOVEMENT, input.movement >> 2));
    }
    // -1 stands for "no hero", as in the exe.
    let skill = input.hero_skill_0f.unwrap_or(-1);
    if skill < 2 {
        for terrain in 1..=8u32 {
            let i = (terrain - 1) as usize;
            let mut plain = TERRAIN_COSTS[i];
            let mut rough = plain + 50;
            if input.at_home[i] {
                plain -= 25;
                rough -= 25;
            }
            // The exe also has a "-50 when 0 < skill < 0" branch that can never run.
            if skill >= 0 {
                plain -= 25;
                rough -= 25;
            }
            // Only what costs more than a plain step goes into the list.
            if plain > 100 {
                list.push((plain_bits(terrain), plain));
            }
            if rough > 100 {
                list.push((rough_bits(terrain), rough));
            }
        }
    }
    msvc_sort(&mut list, |a, b| a.1 < b.1);
    list
}

/// `std::sort` of the MSVC 6 library on this list (the tail of `225320`). For more than
/// 16 elements it would first run a quicksort with a median-of-three pivot (`225900`,
/// `225ac0`, `225b30`), but the list is never that long: at most 2 movement entries and
/// 13 terrain entries (grass, dirt and subterranean never cost more than 100). So only
/// its insertion sort (`225a40`) runs. It is stable here, but it is still the exe's
/// loop, not a library sort, so equal costs keep exactly the exe's order.
fn msvc_sort<T: Copy>(v: &mut [T], less: impl Fn(&T, &T) -> bool + Copy) {
    debug_assert!(v.len() <= 16, "the terrain cost list has at most 15 entries");
    insertion_sort(v, less);
}

/// `_Insertion_sort`: an element smaller than the first goes to the front, the others
/// slide back until something not greater is in front of them.
fn insertion_sort<T: Copy>(v: &mut [T], less: impl Fn(&T, &T) -> bool + Copy) {
    for i in 1..v.len() {
        let value = v[i];
        if less(&value, &v[0]) {
            v.copy_within(0..i, 1);
            v[0] = value;
        } else {
            unguarded_insert(v, i, less);
        }
    }
}

/// `_Unguarded_insert`: something not greater than `v[at]` is known to be in front.
fn unguarded_insert<T: Copy>(v: &mut [T], mut at: usize, less: impl Fn(&T, &T) -> bool) {
    let value = v[at];
    while less(&value, &v[at - 1]) {
        v[at] = v[at - 1];
        at -= 1;
    }
    v[at] = value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_creature_without_a_hero_pays_for_rough_ground() {
        let list = terrain_costs(&TerrainCostInput { movement: 100, ..Default::default() });
        // Sorted by cost: 125s, then 150s, then 175s and 200s.
        assert!(list.windows(2).all(|w| w[0].1 <= w[1].1));
        assert!(list.contains(&(plain_bits(3), 150)));
        assert!(list.contains(&(rough_bits(1), 150)));
        assert!(!list.iter().any(|&(bits, _)| bits == plain_bits(1)), "grass costs no more than a step");
    }

    #[test]
    fn terrain_walk_and_a_skilled_hero_remove_the_costs() {
        let walk = TerrainCostInput { terrain_walk: true, movement: 900, ..Default::default() };
        assert!(terrain_costs(&walk).is_empty());
        let skilled = TerrainCostInput { hero_skill_0f: Some(2), movement: 300, ..Default::default() };
        assert_eq!(terrain_costs(&skilled), vec![(HALF_MOVEMENT, 150)]);
    }

    #[test]
    fn the_longest_list_has_fifteen_entries() {
        let list = terrain_costs(&TerrainCostInput { movement: 1000, ..Default::default() });
        assert_eq!(list.len(), 15);
    }
}
