//! The last step of obstacle placement: find parts of the field that the scenery cut
//! off and clear a way to them (`t_combat_obstruction_finder`, rva `20b480`..`20bcf0`,
//! driven by the tail of the placer's `execute`, `20a462`..`20a6ac`).
//! Details: `decomp/obstacle_placement.md`.
//!
//! How the exe does it, in short:
//! 1. A path map for a big creature (footprint 7) is built from the field as it stands
//!    after placement ([`crate::battle::path_template`]). The search starts at the first
//!    cell of the last row where such a creature fits.
//! 2. The search is its own small Dijkstra: a step into a free cell costs nothing while
//!    the path is still free, every other step costs 2 (3 diagonally). So cost 0 means
//!    "a big creature walks here from the start", and a positive cost counts the steps
//!    through scenery needed to get there.
//! 3. Scanning from the last row, the first cell that is not free for the big creature
//!    but where a small one (footprint 2) could stand is a candidate; the cheapest
//!    covered corner around it (a 4-cell round mask) decides: cost above 0 means the
//!    place is cut off.
//! 4. The path from there back to the free area is cleared: every ordinary obstacle
//!    touching a 7×7 square at each path point is removed. The search goes on from the
//!    free end of the path, with the freed cells marked free again, and the scan repeats
//!    until nothing is cut off (or the same place comes up twice in a row).

use crate::battle::move_command::{path_rows, scenery_blockers};
use crate::battle::path::{flags, PathCell, PathSearch, DIRECTIONS};
use crate::battle::path_template::{template, BlockageMap};
use crate::battlefield::{BattlefieldGrid, CombatFootprint, FLAG_BLOCKED};

/// The footprint of the creature that must reach everywhere: the biggest one.
const BIG: u8 = 7;
/// The footprint of the smallest creature: a place it can stand on counts.
const SMALL: u8 = 2;
/// The mask around a candidate place searched for the cheapest covered corner.
const AROUND: u8 = 4;
/// In a siege the search stays out of rows below this one (the castle).
const SIEGE_FIRST_ROW: i32 = 0x36;
/// Path flags the finder never steps into: impassable (0x10) and "a blocked field cell
/// under the footprint" (0x08).
const CLOSED: u32 = flags::IMPASSABLE | 0x08;

/// What the pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Opening {
    /// Every cleared path, in the order they were found: from the cut-off place back to
    /// the first cell of the free area. A 7×7 square was cleared at each point (the point
    /// is its top-left corner).
    pub paths: Vec<Vec<(i32, i32)>>,
    /// Occupant IDs removed from the grid, in the order the exe removes them.
    pub removed: Vec<u32>,
}

/// Find cut-off parts of the field and clear ways to them, removing scenery from
/// `grid`. Run it after the placed obstacles are on the grid (as segments); the exe
/// skips it in a battle on a bridge (the caller checks that).
///
/// `siege` keeps the search out of the castle rows (siege fields aren't verified).
/// `removable` answers whether a blocking occupant may be removed: in the exe, objects
/// of an ordinary obstacle type (below `castle_gate`, 0x22); non-blocking occupants are
/// never removed.
pub fn open_cut_off_areas(
    grid: &mut BattlefieldGrid,
    siege: bool,
    removable: impl Fn(u32) -> bool,
) -> Opening {
    let rows = path_rows(grid);
    let blockage = BlockageMap::build(&rows, &scenery_blockers(grid, &[]), |x, y| {
        grid.cell(x, y).is_some_and(|c| c.surface.flags & FLAG_BLOCKED != 0)
    });
    let mut finder = Finder { search: PathSearch::new(template(&rows, &blockage, BIG)), siege };
    finder.seed();

    let mut opening = Opening::default();
    let mut previous = (-1, -1);
    while let Some(place) = finder.find_obstruction(grid) {
        if place == previous {
            break; // the clearing didn't help: give up
        }
        previous = place;
        let path = finder.path(place);
        for &(x, y) in &path {
            clear(grid, (x, y), &removable, &mut opening.removed);
        }
        let end = *path.last().expect("a path holds at least its target");
        finder.restart(grid, end);
        opening.paths.push(path);
    }
    opening
}

struct Finder {
    search: PathSearch,
    siege: bool,
}

impl Finder {
    /// The first search start (`20b4b0`): from the last row up, the first cell where the
    /// big creature fits and stands on nothing. In a siege only rows from
    /// [`SIEGE_FIRST_ROW`] count. No such cell — nothing is queued and nothing is found.
    fn seed(&mut self) {
        for x in (0..self.search.map.row_count() as i32).rev() {
            if self.siege && x < SIEGE_FIRST_ROW {
                return;
            }
            let (start, end) = self.search.map.row(x).expect("x is a row index");
            for y in start..end {
                let cell = self.search.map.cell((x, y)).expect("inside the row");
                if cell.flags & flags::OCCUPIED == 0 {
                    self.search.seed_start((x, y)).expect("inside the map");
                    return;
                }
            }
        }
    }

    /// The search (`20b670`). Unlike the creature search, the expanded record is the
    /// queued copy, and the price of a step is 0 into a free cell while the path is still
    /// free (cost 0), else 2, and half again diagonally. Then the big footprint's cover is
    /// marked (`20e640`), but only if anything was queued at all.
    fn run(&mut self) {
        if !self.search.has_queued() {
            return;
        }
        while let Some((at, here)) = self.search.pop_queued() {
            for (dir, &(dx, dy)) in DIRECTIONS.iter().enumerate() {
                let next = (at.0 + dx, at.1 + dy);
                if !self.search.map.contains(next) || (self.siege && next.0 < SIEGE_FIRST_ROW) {
                    continue;
                }
                let cell = *self.search.map.cell(next).expect("checked above");
                if cell.flags & CLOSED != 0 {
                    continue;
                }
                let occupied = cell.flags & flags::OCCUPIED != 0;
                let mut price = if occupied || here.cost > 0 { 2 } else { 0 };
                if dir & 1 != 0 {
                    price += price >> 1;
                }
                // The exe keeps the occupied and impassable bits of the cell, clears 0x08,
                // sets "reached", "covered" exactly when free, and the direction. The other
                // bits come from a stale local of the previous step; nothing reads them,
                // so the cell's own are kept here.
                let mut bits = cell.flags & !(0x08 | flags::COVERED | 0x0003_e000);
                bits |= flags::REACHED | ((dir as u32 & 0x1f) << 13);
                if !occupied {
                    bits |= flags::COVERED;
                }
                let record = PathCell {
                    enter_cost: cell.enter_cost, // stale in the exe as well; unused
                    over_occupied: 0,
                    cost: here.cost.wrapping_add(price),
                    flags: bits,
                    from: at,
                    origin: next,
                };
                self.search.offer(next, record);
            }
        }
        self.search.mark_cover(CombatFootprint { size: BIG });
    }

    /// `find_obstruction` (`20ba60`): run the search, then scan from the last row up,
    /// columns ascending. A cell that is not free for the big creature (cost above 0, or
    /// closed) and where the small one could stand is a candidate; its answer is
    /// [`Self::cheapest_around`]; a positive cost there means cut off.
    fn find_obstruction(&mut self, grid: &BattlefieldGrid) -> Option<(i32, i32)> {
        self.run();
        let map = &self.search.map;
        for x in (0..map.row_count() as i32).rev() {
            let (start, end) = map.row(x).expect("x is a row index");
            for y in start..end {
                let cell = map.cell((x, y)).expect("inside the row");
                if cell.cost == 0 && cell.flags & CLOSED == 0 {
                    continue;
                }
                if !can_stand(grid, (x, y), SMALL) {
                    continue;
                }
                let place = self.cheapest_around((x, y));
                if map.cell(place).is_some_and(|c| c.cost > 0) {
                    return Some(place);
                }
            }
        }
        None
    }

    /// `20b8f0`: within the round 4-cell mask starting one cell up and left of `at`, the
    /// cheapest corner: a covered cell stands for the corner that covers it; closed
    /// corners don't count; the first of equal costs wins, a free one (cost 0) at once.
    /// Nothing found — `at` itself.
    fn cheapest_around(&self, at: (i32, i32)) -> (i32, i32) {
        let map = &self.search.map;
        let mut best: Option<((i32, i32), u32)> = None;
        for (dx, dy) in (CombatFootprint { size: AROUND }).cells() {
            let mut point = (at.0 - 1 + dx, at.1 - 1 + dy);
            let Some(mut cell) = map.cell(point) else { continue };
            if cell.flags & flags::COVERED != 0 {
                point = cell.origin;
                cell = map.cell(point).expect("a cover origin is a cell of the map");
            }
            if cell.flags & CLOSED != 0 {
                continue;
            }
            if best.is_none_or(|(_, cost)| cell.cost < cost) {
                if cell.cost == 0 {
                    return point;
                }
                best = Some((point, cell.cost));
            }
        }
        best.map_or(at, |(point, _)| point)
    }

    /// `get_path` (`20bc10`): run the search (nothing is queued by now), then `target`
    /// and the cells it came from, up to and including the first one with cost 0.
    fn path(&mut self, target: (i32, i32)) -> Vec<(i32, i32)> {
        self.run();
        let map = &self.search.map;
        let mut path = vec![target];
        let mut cell = map.cell(target).expect("the target is a cell of the map");
        while cell.cost > 0 {
            path.push(cell.from);
            cell = map.cell(cell.from).expect("a reached cell comes from a cell of the map");
        }
        path
    }

    /// `20bcf0`: start the search again from `at` (the free end of the cleared path;
    /// the records stay as they are, so the search goes on where it stopped), and mark
    /// every occupied, not closed cell free again if the big creature now fits there.
    fn restart(&mut self, grid: &BattlefieldGrid, at: (i32, i32)) {
        self.search.seed_start(at).expect("a path point is a cell of the map");
        for x in (0..self.search.map.row_count() as i32).rev() {
            let (start, end) = self.search.map.row(x).expect("x is a row index");
            for y in start..end {
                let cell = self.search.map.cell_mut((x, y)).expect("inside the row");
                if cell.flags & CLOSED == 0 && cell.flags & flags::OCCUPIED != 0 && can_stand(grid, (x, y), BIG) {
                    cell.flags &= !flags::OCCUPIED;
                }
            }
        }
    }
}

/// Could a creature of this footprint stand with its corner at `at` (`1651e0`): every
/// cell of the round mask on the field and not blocked (a creature that isn't on the
/// field yet, so no occupant is "itself").
fn can_stand(grid: &BattlefieldGrid, at: (i32, i32), size: u8) -> bool {
    (CombatFootprint { size }).cells().all(|(dx, dy)| !grid.is_blocked(at.0 + dx, at.1 + dy, None))
}

/// `clear_rect` (`209040`) for a 7×7 square with its corner at `at`: every blocking
/// removable occupant of its cells leaves the field (all its cells, not only these).
fn clear(grid: &mut BattlefieldGrid, at: (i32, i32), removable: &impl Fn(u32) -> bool, removed: &mut Vec<u32>) {
    let size = i32::from(BIG);
    for x in at.0..at.0 + size {
        for y in at.1..at.1 + size {
            let Some(cell) = grid.cell(x, y) else { continue };
            let doomed: Vec<u32> = cell
                .occupants
                .iter()
                .filter(|o| o.blocks && removable(o.id))
                .map(|o| o.id)
                .collect();
            for id in doomed {
                grid.remove_object(id);
                removed.push(id);
            }
        }
    }
}
