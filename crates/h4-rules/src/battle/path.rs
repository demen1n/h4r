//! Path search on the battlefield: the search slot of the combat path finder
//! (rva `20d200`), the cover marking that follows it (`20e640`) and the path
//! reconstruction (`20e7b0`), as the exe does them
//! (`decomp/combat_path.md`; verified by executing the original code:
//! `tests/combat_path_vectors.tsv`).
//!
//! The search is Dijkstra's algorithm over the cells of the battlefield grid, but with
//! the exe's own quirks kept, because they decide which of two equally cheap paths a
//! creature takes:
//! - the queue is the exe's binary heap (MSVC `push_heap`/`pop_heap`), so ties come out
//!   in its order;
//! - a cell may sit in the queue several times; every copy is expanded again, from the
//!   cell's *current* record;
//! - stepping into an occupied cell (an enemy to attack, a wall) is a separate phase that
//!   costs 500 and then walks on through blocked cells for a few half-steps (the reach of a
//!   large creature or a `long_weapon`).
//!
//! The map is prepared elsewhere ([`crate::battle::path_template`],
//! [`crate::battle::path_prepare`]); the move command on a battle is
//! [`crate::battle::move_command`]. The zone-of-control directions (`172210`, issue #63)
//! are given by the caller.

use crate::battlefield::CombatFootprint;

/// The eight neighbours in the exe's order (the static table built by `3054b0`):
/// north-east, east, south-east, south, south-west, west, north-west, north. A pair is
/// (row, column) — (x, y) of the battlefield grid. Odd directions change both
/// coordinates: they are the diagonals and cost half as much again.
pub const DIRECTIONS: [(i32, i32); 8] = [(-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1)];

/// Bits of [`PathCell::flags`]. Bits 8–12 are a step counter ([`PathCell::steps`]),
/// bits 13–17 the direction of the last step ([`PathCell::direction`]).
pub mod flags {
    /// The search reached the cell: `cost` and `from` are a real path.
    pub const REACHED: u32 = 0x01;
    /// The creature can stand with its footprint corner here, or the cell is covered by
    /// the footprint of such a corner (see [`super::PathSearch::mark_cover`]).
    pub const COVERED: u32 = 0x02;
    /// Something stands here: a creature, a closed gate, a wall.
    pub const OCCUPIED: u32 = 0x04;
    /// The search never enters the cell (outside the field for this footprint, an obstacle).
    pub const IMPASSABLE: u32 = 0x10;
    /// A castle tower: moving inside costs nothing, the first step out costs 400.
    pub const TOWER: u32 = 0x20;
    /// A castle wall or gate line: fliers and teleporters pay more to cross it
    /// in directions north-east, east and north.
    pub const WALL: u32 = 0x40;
    /// Two meanings, as in the exe: in the occupied-cell phase "already passed through,
    /// with the step counter in bits 8–12"; on a reached cell "the step started within
    /// the creature's movement".
    pub const MARK: u32 = 0x80;
}

/// One cell's record in the path map, 28 bytes in the exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathCell {
    /// Cost to step into the cell (+0; from the terrain, 100 on plain ground).
    pub enter_cost: u16,
    /// Movement spent in a row over occupied cells, ending here (+2). A flier may
    /// cross occupied cells but has to leave them within its movement.
    pub over_occupied: u16,
    /// Cost of the path from the start (+4).
    pub cost: u32,
    /// [`flags`] plus the step counter and the direction (+8).
    pub flags: u32,
    /// The cell the path came from, (-1, -1) at the start (+0x0c, +0x10).
    pub from: (i32, i32),
    /// The footprint corner this cell belongs to (+0x14, +0x18): the cell itself after
    /// the search, then the cheapest corner whose footprint covers it.
    pub origin: (i32, i32),
}

impl PathCell {
    /// A fresh cell of the prepared map: nothing reached yet.
    pub fn new(at: (i32, i32), flags: u32, enter_cost: u16) -> Self {
        PathCell { enter_cost, over_occupied: 0, cost: 0, flags, from: (-1, -1), origin: at }
    }

    /// The step counter, bits 8–12: 2 for a straight step and 3 for a diagonal one,
    /// summed along the occupied-cell phase.
    pub fn steps(&self) -> u32 {
        (self.flags >> 8) & 0x1f
    }

    /// Direction of the step that entered the cell, an index into [`DIRECTIONS`].
    pub fn direction(&self) -> u32 {
        (self.flags >> 13) & 0x1f
    }
}

/// The grid of path records. Rows have their own column ranges, like the battlefield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    rows: Vec<(i32, i32)>,
    /// Index of the row's first cell in `cells`.
    first: Vec<usize>,
    cells: Vec<PathCell>,
}

impl PathMap {
    /// A map with these row ranges (`start..end` columns of each row); every cell is
    /// fresh ground: no flags, entry cost 100.
    pub fn new(rows: &[(i32, i32)]) -> Self {
        let mut first = Vec::with_capacity(rows.len());
        let mut cells = Vec::new();
        for (x, &(start, end)) in rows.iter().enumerate() {
            first.push(cells.len());
            for y in start..end.max(start) {
                cells.push(PathCell::new((x as i32, y), 0, 100));
            }
        }
        PathMap { rows: rows.to_vec(), first, cells }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Columns `start..end` of row `x`.
    pub fn row(&self, x: i32) -> Option<(i32, i32)> {
        usize::try_from(x).ok().and_then(|x| self.rows.get(x).copied())
    }

    fn index(&self, (x, y): (i32, i32)) -> Option<usize> {
        let (start, end) = self.row(x)?;
        if y < start || y >= end {
            return None;
        }
        Some(self.first[x as usize] + (y - start) as usize)
    }

    pub fn contains(&self, at: (i32, i32)) -> bool {
        self.index(at).is_some()
    }

    pub fn cell(&self, at: (i32, i32)) -> Option<&PathCell> {
        self.index(at).map(|i| &self.cells[i])
    }

    pub fn cell_mut(&mut self, at: (i32, i32)) -> Option<&mut PathCell> {
        self.index(at).map(|i| &mut self.cells[i])
    }

    /// All cells with their coordinates, row by row.
    pub fn cells(&self) -> impl Iterator<Item = ((i32, i32), &PathCell)> {
        self.rows.iter().enumerate().flat_map(move |(x, &(start, end))| {
            (start..end.max(start)).map(move |y| {
                let at = (x as i32, y);
                (at, &self.cells[self.first[x] + (y - start) as usize])
            })
        })
    }
}

/// What the search needs to know about the moving creature. Ability and effect numbers
/// are resolved by the caller, the way the exe asks about them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mover {
    /// Movement points (virtual `+0x18` of the creature's stats). Plain steps cost 100.
    pub movement: i32,
    /// Footprint side in cells (virtual `+8` of the combat object).
    pub footprint_size: i32,
    /// Ability 0x25 `long_weapon` (`1ef5e0`): the occupied-cell phase reaches
    /// 12 half-steps instead of 4.
    pub long_weapon: bool,
    /// Passes over occupied cells and pays wall crossings instead of terrain: no effect 7,
    /// and ability 0x19 `flying` or effect 0x2e.
    pub flies: bool,
    /// Effect 9: the creature can't move, the queue is emptied before the search.
    pub held: bool,
    /// The path finder's own flag (`+0x50`), set for ability 0x4a `teleport`: like flying,
    /// and zones of control don't matter.
    pub teleports: bool,
}

impl Mover {
    /// How far the occupied-cell phase may go: twice 6 with `long_weapon`, else twice 2.
    fn step_limit(&self) -> u32 {
        2 * if self.long_weapon { 6 } else { 2 }
    }

    /// The price of crossing a wall for a flier: `(movement - 200) / (size + 1)`,
    /// at least 100; a teleporter always pays 100.
    fn wall_cost(&self) -> u32 {
        if self.teleports {
            return 100;
        }
        // A size of -1 would make the exe divide by zero; it never happens.
        let cost = (self.movement - 200).checked_div(self.footprint_size + 1).unwrap_or(100);
        cost.max(100) as u32
    }
}

/// A queued point: a copy of the cell's record when it was queued, and the cell.
#[derive(Debug, Clone, Copy)]
struct Queued {
    cell: PathCell,
    at: (i32, i32),
}

/// The path finder's state: the map and the queue.
#[derive(Debug, Clone)]
pub struct PathSearch {
    pub map: PathMap,
    queue: Vec<Queued>,
}

impl PathSearch {
    pub fn new(map: PathMap) -> Self {
        PathSearch { map, queue: Vec::new() }
    }

    /// Queue the creature's own cell as the start (`20e3f0`): cost 0, reached, no
    /// predecessor. The exe builds the record in a local and overwrites the cell with it,
    /// so the old flags are lost too (bits 13 and up come from an uninitialized local in
    /// the exe; here they are 0). The entry cost becomes 100; the real preparation
    /// then refreshes it from the terrain, so a caller following the exe sets it after.
    pub fn seed_start(&mut self, at: (i32, i32)) -> Result<(), &'static str> {
        let cell = PathCell { enter_cost: 100, over_occupied: 0, cost: 0, flags: flags::REACHED | flags::COVERED, from: (-1, -1), origin: at };
        *self.map.cell_mut(at).ok_or("start outside the path map")? = cell;
        self.push(Queued { cell, at });
        Ok(())
    }

    /// Run the search (`20d200`), then mark which cells the footprint covers (`20e640`).
    /// `zone` answers, for a cell being expanded, in which directions a step pays the
    /// zone-of-control penalty (a quarter of the movement); the exe asks it only when the
    /// creature doesn't teleport.
    pub fn run(&mut self, mover: &Mover, mut zone: impl FnMut((i32, i32)) -> [bool; 8]) {
        if self.queue.is_empty() {
            return;
        }
        if mover.held {
            self.queue.clear();
        }
        let movement = mover.movement;
        let zone_penalty = movement >> 2;
        let limit = mover.step_limit();
        let wall_cost = mover.wall_cost();
        let flies = mover.flies || mover.teleports;
        while let Some(point) = self.pop() {
            let at = point.at;
            let zones = if mover.teleports { [false; 8] } else { zone(at) };
            for (dir, &(dx, dy)) in DIRECTIONS.iter().enumerate() {
                let next = (at.0 + dx, at.1 + dy);
                if self.map.contains(next) {
                    self.step(at, dir as u32, next, mover, limit, wall_cost, flies, zones[dir], zone_penalty);
                }
            }
        }
        self.mark_cover(CombatFootprint { size: mover.footprint_size.clamp(0, 255) as u8 });
    }

    /// One neighbour of an expanded cell: the body of the direction loop of `20d200`.
    #[allow(clippy::too_many_arguments)] // the loop's locals, kept as in the exe
    fn step(&mut self, at: (i32, i32), dir: u32, next: (i32, i32), mover: &Mover, limit: u32, wall_cost: u32, flies: bool, zone: bool, zone_penalty: i32) {
        // The expanded cell is read again from the map, not from the queued copy.
        let here = *self.map.cell(at).expect("expanded cells are inside the map");
        let mut cell = *self.map.cell(next).expect("the caller checked the bounds");
        let diagonal = dir & 1;
        let movement = mover.movement as u32; // the exe compares path costs with it unsigned
        if cell.flags & flags::IMPASSABLE != 0 {
            return;
        }
        if here.flags & flags::REACHED == 0 {
            // Inside the occupied-cell phase: walk on through cells not reached yet,
            // adding 2 per straight and 3 per diagonal half-step, up to the limit.
            if cell.flags & flags::REACHED != 0 {
                return;
            }
            let steps = here.steps() + 2 + diagonal;
            if steps > limit {
                return;
            }
            if cell.flags & flags::MARK != 0 && cell.steps() <= steps {
                return;
            }
            cell.cost = here.cost.wrapping_add(500);
            cell.from = at;
            cell.flags = (cell.flags & 0xfffc_00ff) | ((dir & 0x1f) << 13) | ((steps & 0x1f) << 8) | flags::MARK;
            self.put(next, cell);
            return;
        }
        let reached = cell.flags & flags::REACHED != 0;
        if reached && cell.cost <= here.cost {
            return;
        }
        let occupied = cell.flags & flags::OCCUPIED != 0;
        if occupied && !flies {
            // An occupied cell (something to attack): enter the occupied-cell phase,
            // if the step starts within the movement.
            if here.cost > movement {
                return;
            }
            if cell.flags & flags::MARK != 0 && cell.steps() <= diagonal + 2 {
                return;
            }
            cell.cost = here.cost.wrapping_add(500);
            cell.from = at;
            let marked = (cell.flags & 0xfffc_1fff) | ((dir & 0x1f) << 13) | flags::MARK;
            cell.flags = set_steps(marked, diagonal + 2);
            self.put(next, cell);
            return;
        }
        // An ordinary step. Its price:
        let mut price: u32 = if here.flags & flags::TOWER != 0 {
            if cell.flags & flags::TOWER != 0 {
                // inside the tower: free, unless the cell is taken
                if occupied { 100 } else { 0 }
            } else if here.cost == 0 {
                400 // out of the tower right from the start
            } else {
                movement // out of the tower later: the whole movement
            }
        } else if !flies {
            u32::from(cell.enter_cost)
        } else if cell.flags & flags::WALL != 0 && (dir <= 1 || dir == 7) {
            wall_cost
        } else {
            100
        };
        if zone {
            price = price.wrapping_add(zone_penalty as u32);
        }
        if diagonal != 0 {
            price = price.wrapping_add(((price as i32) >> 1) as u32);
        }
        // Movement spent over occupied cells must fit in the movement.
        let over: u32 = if here.flags & flags::OCCUPIED == 0 && !occupied {
            0
        } else {
            let over = u32::from(here.over_occupied).wrapping_add(price);
            if over as i32 > mover.movement {
                return;
            }
            over
        };
        let total = price.wrapping_add(here.cost);
        if reached && total >= cell.cost {
            return;
        }
        let mut bits = set_steps(cell.flags, diagonal + 2);
        cell.origin = next;
        bits &= !flags::MARK;
        if movement >= here.cost {
            bits |= flags::MARK;
        }
        cell.over_occupied = over as u16;
        bits |= flags::REACHED;
        // Covered exactly when not occupied.
        bits = (bits & !flags::COVERED) | ((!bits >> 1) & flags::COVERED);
        cell.cost = total;
        cell.from = at;
        cell.flags = (bits & 0xfffc_1fff) | ((dir & 0x1f) << 13);
        self.put(next, cell);
    }

    /// The queued points in the queue's own (heap) order: the cell and the copy of its
    /// record taken when it was queued.
    pub fn queued(&self) -> impl Iterator<Item = ((i32, i32), &PathCell)> {
        self.queue.iter().map(|point| (point.at, &point.cell))
    }

    /// Empty the queue (the tower start does this before queueing the exits).
    pub(crate) fn clear_queue(&mut self) {
        self.queue.clear();
    }

    /// Store the record and queue a copy of it.
    pub(crate) fn put(&mut self, at: (i32, i32), cell: PathCell) {
        *self.map.cell_mut(at).expect("the caller checked the bounds") = cell;
        self.push(Queued { cell, at });
    }

    /// Append and sift up, like MSVC `push_heap` with "greater cost" as the order:
    /// the cheapest point is at the front; a point stops below a parent of equal cost.
    fn push(&mut self, point: Queued) {
        self.queue.push(point);
        let hole = self.queue.len() - 1;
        self.sift_up(hole, point);
    }

    /// Move `value` up from `hole` while its parent costs more (the exe's `_Push_heap`).
    fn sift_up(&mut self, mut hole: usize, value: Queued) {
        while hole > 0 {
            let parent = (hole - 1) / 2;
            if self.queue[parent].cell.cost <= value.cell.cost {
                break;
            }
            self.queue[hole] = self.queue[parent];
            hole = parent;
        }
        self.queue[hole] = value;
    }

    /// Take the cheapest point (`pop_heap`, rva `20c170`, then erase the last element).
    /// The hole left at the front first sinks to the bottom along the cheaper child
    /// (the right one on a tie), then the former last element rises from there.
    fn pop(&mut self) -> Option<Queued> {
        let last = self.queue.pop()?;
        if self.queue.is_empty() {
            return Some(last);
        }
        let top = self.queue[0];
        let len = self.queue.len();
        let mut hole = 0;
        let mut child = 2;
        while child < len {
            if self.queue[child].cell.cost > self.queue[child - 1].cell.cost {
                child -= 1;
            }
            self.queue[hole] = self.queue[child];
            hole = child;
            child = 2 * child + 2;
        }
        if child == len {
            self.queue[hole] = self.queue[child - 1];
            hole = child - 1;
        }
        self.sift_up(hole, last);
        Some(top)
    }

    /// After the search (`20e640`): every reached, unoccupied cell is a place where the
    /// footprint's corner may stand; each cell under such a footprint gets
    /// [`flags::COVERED`] and, as `origin`, the cheapest corner covering it (the first
    /// one wins a tie; corners go from the last row to the first, columns ascending).
    /// The exe doesn't check the bounds: the real preparation keeps the search away from
    /// corners whose footprint leaves the field. Here cells outside are skipped.
    pub fn mark_cover(&mut self, footprint: CombatFootprint) {
        for x in (0..self.map.row_count() as i32).rev() {
            let (start, end) = self.map.row(x).expect("x is a row index");
            for y in start..end {
                let corner = *self.map.cell((x, y)).expect("inside the row");
                if corner.flags & flags::REACHED == 0 || corner.flags & flags::OCCUPIED != 0 {
                    continue;
                }
                for (dx, dy) in footprint.cells() {
                    let Some(target) = self.map.cell((x + dx, y + dy)).copied() else { continue };
                    // The exe compares these two costs signed.
                    let take = target.flags & flags::COVERED == 0
                        || self.map.cell(target.origin).is_some_and(|owner| (corner.cost as i32) < (owner.cost as i32));
                    if take {
                        let target = self.map.cell_mut((x + dx, y + dy)).expect("checked above");
                        target.flags |= flags::COVERED;
                        target.origin = (x, y);
                    }
                }
            }
        }
    }

    /// The path to a reached cell (`20e7b0`): from the cell back along `from`, the start
    /// excluded — the order the exe stores it in. `None` if the cell wasn't reached or is
    /// the start itself.
    pub fn path_to(&self, target: (i32, i32)) -> Option<Vec<(i32, i32)>> {
        let mut at = target;
        let mut cell = *self.map.cell(at)?;
        if cell.flags & flags::REACHED == 0 {
            return None;
        }
        let mut path = Vec::new();
        while cell.from.1 >= 0 {
            path.push(at);
            at = cell.from;
            cell = *self.map.cell(at)?;
        }
        (!path.is_empty()).then_some(path)
    }
}

/// Replace the step counter (bits 8–12).
fn set_steps(flags: u32, steps: u32) -> u32 {
    (flags & !0x1f00) | ((steps & 0x1f) << 8)
}
