//! Preparing a creature's path map before the search (rva `20c7f0`), as the exe does it
//! (`decomp/combat_path.md`; verified by executing the original code:
//! `tests/combat_path_prepare_vectors.tsv`).
//!
//! The preparation starts from the template map of the creature's footprint size
//! ([`crate::battle::path_template`]) and adds what changes during a battle:
//! 1. the objects standing on the field: creatures, gates, towers, walls — each marks
//!    every corner position where the creature's footprint would touch it;
//! 2. the start: the creature's own cell, or, for a creature in a castle tower, the exits
//!    around the tower;
//! 3. the entry cost of every cell from the terrain.
//!
//! Inputs the exe gets from rules that are not ported yet stay the caller's answers:
//! whether a creature is hostile or cowardly toward the mover, the gate's state, the
//! terrain masks of the cells and the mover's terrain cost list (`225320`).

use crate::battle::path::{flags, PathCell, PathMap, PathSearch};
use crate::battlefield::CombatFootprint;

/// A castle tower a creature stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TowerPlace {
    /// Corner of the tower's footprint in subcells.
    pub position: (i32, i32),
    /// Its footprint side in cells.
    pub size: i32,
}

/// The creature standing at a tower's center, as the tower check sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TowerOccupant {
    /// `is_hostile` toward the mover.
    pub hostile: bool,
    /// The mover's cowardice check against it (`1f52f0`) says it doesn't count.
    pub cowardly: bool,
    /// It has effect 0x89.
    pub effect_89: bool,
}

/// What an object of the battlefield's list is, with the answers the exe asks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldObjectKind {
    /// A creature: hostile, cowardly and effect 0x89 as for [`TowerOccupant`];
    /// a creature in a tower blocks with the tower's footprint.
    Creature { hostile: bool, cowardly: bool, effect_89: bool, tower: Option<TowerPlace> },
    /// Stationary scenery that isn't a castle part.
    Obstacle,
    /// `castle_gate`.
    Gate,
    /// `castle_tower`, with the creature standing at its center, if any (`170d90`).
    Tower { occupant: Option<TowerOccupant> },
    /// `castle_wall`.
    Wall,
    /// Anything else (neither stationary nor a creature).
    Other,
}

/// One entry of the battlefield's object list (`+0x448`), in list order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldObject {
    pub kind: FieldObjectKind,
    /// This is the moving creature itself (skipped).
    pub is_mover: bool,
    /// Virtual `+0x10` of the object; objects answering no are skipped.
    pub alive: bool,
    /// Virtual `+0x10` of its base: fixed scenery, already in the template (skipped).
    pub fixed: bool,
    /// Virtual `+4` of its base, asked only in attack mode of non-creatures: yes means
    /// it doesn't block.
    pub skip: bool,
    /// Footprint side in cells (virtual `+8`); 0 or less marks nothing.
    pub size: i32,
    /// Corner of the footprint in subcells.
    pub position: (i32, i32),
}

/// The castle gate, as the battlefield answers about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GateState {
    /// `has_gate` (`177b90`).
    pub present: bool,
    /// `177990`: when true the gate blocks nobody and is no wall line — read as
    /// "destroyed", not checked.
    pub state_177990: bool,
    /// `1778c0`: when false a defender passes the gate — read as "barred", not checked.
    pub state_1778c0: bool,
    /// The battlefield's `+0x64`: a defender whose row is above it gets the gate opened
    /// for the terrain refresh.
    pub row: i32,
}

/// The creature the map is prepared for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedMover {
    /// Corner of its footprint in subcells.
    pub position: (i32, i32),
    /// Its footprint side; must be the template's size.
    pub size: i32,
    /// The castle tower it stands in (byte `+0x9bc` and `get_tower`).
    pub tower: Option<TowerPlace>,
    /// The path finder's teleport flag (`+0x50`).
    pub teleports: bool,
    /// Flies for the path search: no effect 7, and `flying` or effect 0x2e.
    pub flies: bool,
    /// The virtual `+4` of its effects asked with 1 — read as "is on the defending
    /// side", not checked.
    pub defender: bool,
}

/// Everything the preparation reads.
pub struct PrepareInput<'a> {
    /// The template of the mover's footprint size.
    pub template: &'a PathMap,
    pub mover: PreparedMover,
    /// The battlefield's object list, in order.
    pub objects: &'a [FieldObject],
    pub gate: GateState,
    /// The path finder's second flag (`+0x51`): only hostile creatures that can be
    /// fought block; gates stay open.
    pub attack_mode: bool,
    /// Terrain bits of a cell after the terrain refresh (the combat terrain map).
    pub terrain: &'a dyn Fn((i32, i32)) -> u32,
    /// The mover's terrain cost list (`225320`): (terrain bits, entry cost). Read only
    /// when the mover walks.
    pub costs: &'a [(u32, u16)],
}

/// The prepared map with its start, and whether the gate was opened for the terrain.
pub struct Prepared {
    pub search: PathSearch,
    /// The exe opened the gate (`1779c0`) around the terrain refresh for this mover: the
    /// caller's terrain bits must be those of an open gate.
    pub opens_gate: bool,
}

/// Prepare the path map (`20c7f0`).
pub fn prepare(input: &PrepareInput) -> Prepared {
    let mover = input.mover;
    let mut search = PathSearch::new(input.template.clone());

    // 1. Objects. The gate's bits are decided once.
    let gate_passable = input.attack_mode
        || input.gate.state_177990
        || (mover.defender && !input.gate.state_1778c0);
    let mut gate_bits = 0;
    if !gate_passable {
        gate_bits |= flags::OCCUPIED;
    }
    if !input.gate.state_177990 {
        gate_bits |= flags::WALL;
    }
    for object in input.objects {
        if !object.alive || object.fixed || object.is_mover {
            continue;
        }
        let bits = match object.kind {
            FieldObjectKind::Gate => Some(gate_bits),
            FieldObjectKind::Wall => Some(flags::OCCUPIED | flags::WALL),
            FieldObjectKind::Tower { occupant } => match occupant {
                None => Some(flags::TOWER),
                // A tower with a creature inside: free for this creature only if it is
                // not an enemy that can be fought (and only in attack mode).
                Some(o) if input.attack_mode && (!o.hostile || o.cowardly || o.effect_89) => Some(flags::TOWER),
                Some(_) => None,
            },
            FieldObjectKind::Creature { hostile, cowardly, effect_89, .. } => {
                (!input.attack_mode || (hostile && !cowardly && !effect_89)).then_some(flags::OCCUPIED)
            }
            FieldObjectKind::Obstacle | FieldObjectKind::Other => {
                (!input.attack_mode || !object.skip).then_some(flags::OCCUPIED)
            }
        };
        if let Some(bits) = bits {
            mark_object(&mut search.map, object, mover.size, bits);
        }
    }

    // 2. The start.
    let cell = (mover.position.0 >> 4, mover.position.1 >> 4);
    match mover.tower {
        // A start off the map can't happen: the creature stands on the field. (The exe
        // would write outside the map; here nothing is queued.)
        None => search.seed_start(cell).unwrap_or(()),
        Some(tower) => seed_tower(&mut search, cell, mover.size, tower),
    }

    // 3. Entry costs. Only a walker has a cost list; the gate may be opened for it.
    let walks = !mover.teleports && !mover.flies;
    let costs: &[(u32, u16)] = if walks { input.costs } else { &[] };
    let opens_gate = walks
        && input.gate.present
        && !input.gate.state_177990
        && mover.defender
        && cell.0 < input.gate.row;
    let any = costs.iter().fold(0, |all, &(bits, _)| all | bits);
    let cells: Vec<(i32, i32)> = search.map.cells().map(|(at, _)| at).collect();
    for at in cells {
        let cell = search.map.cell_mut(at).expect("listed above");
        cell.enter_cost = if cell.flags & flags::TOWER != 0 {
            400
        } else if cell.flags & flags::OCCUPIED == 0 && (input.terrain)(at) & any != 0 {
            // the last entry of the list matching the cell's terrain
            let bits = (input.terrain)(at);
            costs.iter().rev().find(|&&(mask, _)| mask & bits != 0).map_or(100, |&(_, cost)| cost)
        } else {
            100
        };
    }
    Prepared { search, opens_gate }
}

/// Mark an object (`20e0f0`): every corner position where a footprint of `mover_size`
/// would overlap the object's footprint gets `bits` added. A creature in a tower uses
/// the tower's corner and size instead.
fn mark_object(map: &mut PathMap, object: &FieldObject, mover_size: i32, bits: u32) {
    if object.size <= 0 {
        return;
    }
    let (mut size, mut position) = (object.size, object.position);
    if let FieldObjectKind::Creature { tower: Some(tower), .. } = object.kind {
        size = tower.size;
        position = tower.position;
    }
    let corner = (position.0 >> 4, position.1 >> 4);
    let object_fp = footprint(size);
    let mover_fp = footprint(mover_size);
    for dx in (1 - mover_size)..size {
        for dy in (1 - mover_size)..size {
            if !touches(object_fp, mover_fp, (dx, dy)) {
                continue;
            }
            if let Some(cell) = map.cell_mut((corner.0 + dx, corner.1 + dy)) {
                cell.flags |= bits;
            }
        }
    }
}

fn footprint(size: i32) -> CombatFootprint {
    CombatFootprint { size: size.clamp(0, 255) as u8 }
}

/// Does a mover footprint with its corner `offset` cells from the object's corner
/// overlap the object (`1fc810`, the cells of the expanded footprint `1fc460`)?
pub fn touches(object: CombatFootprint, mover: CombatFootprint, offset: (i32, i32)) -> bool {
    mover.cells().any(|(i, j)| object.contains(i + offset.0, j + offset.1))
}

/// The start of a creature in a castle tower (`20c6d0`): its own cell becomes the reached
/// start inside the tower (not queued), and every free cell along the tower's sides is
/// queued at cost 400, coming from the creature's cell (`20c400`).
fn seed_tower(search: &mut PathSearch, cell: (i32, i32), size: i32, tower: TowerPlace) {
    search.clear_queue();
    let (tx, ty) = (tower.position.0 >> 4, tower.position.1 >> 4);
    let ts = tower.size;
    // Two passes as in the exe: rows tx.. stepping by 1, columns at the two sides;
    // then columns ty.. stepping by 1, rows at the two ends.
    seed_exits(search, cell, (tx, ty - size, tx - size + 1 + ts, ty + 1 + ts), (1, ts + size));
    seed_exits(search, cell, (tx - size, ty, tx + 1 + ts, ty - size + 1 + ts), (ts + size, 1));
}

/// One pass of `20c400`: the rectangle `x0..x1` × `y0..y1` walked with `step`.
fn seed_exits(search: &mut PathSearch, from: (i32, i32), rect: (i32, i32, i32, i32), step: (i32, i32)) {
    // The creature's own cell: reached at cost 0, inside the tower, nothing else.
    if let Some(own) = search.map.cell_mut(from) {
        own.cost = 0;
        own.from = (-1, -1);
        own.over_occupied = 0;
        own.origin = from;
        own.flags = ((own.flags | 3) & 0xffff_ffa3) | 0x23;
    }
    let (x0, y0, x1, y1) = rect;
    let mut x = x0;
    while x < x1 {
        let mut y = y0;
        while y < y1 {
            let free = search.map.cell((x, y)).is_some_and(|c| c.flags & (flags::OCCUPIED | flags::IMPASSABLE) == 0);
            if free {
                let exit = PathCell {
                    enter_cost: 100,
                    over_occupied: 0,
                    cost: 400,
                    flags: flags::REACHED | flags::COVERED | (direction(from, (x, y)) << 13),
                    from,
                    origin: (x, y),
                };
                search.put((x, y), exit);
            }
            y += step.1;
        }
        x += step.0;
    }
}

/// The direction from one cell to another as an index into
/// [`crate::battle::path::DIRECTIONS`] (`51ede0`): the [`angle`] to it, then eight
/// sectors of 45° centered on the directions.
pub fn direction(from: (i32, i32), to: (i32, i32)) -> u32 {
    let turned = angle((to.0 - from.0, to.1 - from.1));
    (((2 * turned + 45) / 90) & 7) as u32
}

/// The angle of a vector (rows, columns) in whole degrees, 0..360 (`11ed40`, and the
/// same arithmetic inside `51ede0`): x87 `atan2` in degrees, truncated, then turned.
/// Direction 0 (north-east, one row up) is 0°; the angle grows clockwise on screen.
pub fn angle(d: (i32, i32)) -> i32 {
    // The exe negates the row difference as an integer before loading it: no -0.0, so
    // the angle of (0, 0) is 0, not 180.
    let rows = f64::from(d.0.wrapping_neg());
    let columns = f64::from(d.1);
    // atan2(columns, -rows), in degrees with the exe's sign: × -180 / π, then __ftol
    // (truncation toward zero).
    let degrees = (columns.atan2(rows) * -180.0 / std::f64::consts::PI) as i32;
    (360 - degrees) % 360
}
