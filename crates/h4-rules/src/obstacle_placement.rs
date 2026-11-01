//! Random scenery on a combat field: port of `t_combat_obstacle_placer`
//! (`combat_obstacle_placer.cpp`, Buka 3.0 RVA 208ec0..20a6e0).
//!
//! How the original does it (details and addresses: `decomp/obstacle_placement.md`):
//!
//! 1. Keep the army start areas and the heroes' squares free.
//! 2. Unless the battle is on a bridge, cut three wandering free paths between
//!    the two sides.
//! 3. Walk the whole field breadth-first from one corner. At every cell roll
//!    against the cell's "density" (2% at first). On success, weigh every known
//!    obstacle model for this spot with `table.combat_obstacles` (terrain,
//!    neighbours already placed, overlap with them) and pick one at random.
//!    Around a placed obstacle the density rises to 20%, so scenery clumps.
//! 4. Unless the battle is on a bridge, find areas the obstacles have closed off
//!    and cut paths into them. **Not ported yet**: it needs the combat path
//!    finder (`t_combat_obstruction_finder`, RVA 20b480).
//!
//! All randomness comes from the combat generator with purposes 0x1e..0x23,
//! so the result depends only on the battle seed.
//!
//! The placer only reads the field; it returns the chosen models and positions.
//! The caller turns them into blocking segments (`PlacedObstacle::from_segment`).

use std::collections::VecDeque;

use h4_formats::combat_object::{obstacle_type_by_name, CombatHeaderEntry};
use h4_formats::table::Table;

use crate::battlefield::{BattlefieldGrid, FLAG_10};
use crate::random::CombatRandom;

/// Number of obstacle types in the exe enum (`OBSTACLE_TYPE_NAMES`).
const TYPE_COUNT: usize = 38;

// One row of the weights table per obstacle type, as laid out in the exe
// (85 ints, global at VA 0xaa7a38): 9 terrain weights, then 38 "adjacent"
// and 38 "overlap" weights, one per other obstacle type.
const ROW_LEN: usize = 85;
const ADJACENT_AT: usize = 9;
const OVERLAP_AT: usize = ADJACENT_AT + TYPE_COUNT;

/// Frequency words of the table, in the order of their values 0..=4
/// (keyword map VA 0x982a90). Matched ignoring ASCII case.
const FREQUENCIES: [&str; 5] = ["never", "rare", "seldom", "common", "usually"];
/// What each frequency is worth in each section (VA 0x982ab8, 0x982acc, 0x982ae0).
/// −5000 makes a neighbour or overlap practically forbidden.
const TERRAIN_VALUES: [i32; 5] = [0, 5, 10, 20, 30];
const ADJACENT_VALUES: [i32; 5] = [-5000, 0, 10, 30, 100];
const OVERLAP_VALUES: [i32; 5] = [-5000, 0, 10, 30, 30];

/// Map terrain names in the order of their ids (VA 0x9958d8).
const TERRAIN_NAMES: [&str; 19] = [
    "water",
    "grass",
    "rough",
    "swamp",
    "volcanic",
    "snow",
    "sand",
    "dirt",
    "subterranean",
    "river",
    "lava",
    "ice",
    "special_all",
    "special_life",
    "special_order",
    "special_death",
    "special_chaos",
    "special_nature",
    "special_combat",
];

/// Which of the 9 weight columns a cell's terrain id uses (RVA 484a10, table
/// built by 4842b0): river counts as water, lava as volcanic, ice as snow,
/// magic grounds as grass or volcanic.
const WEIGHT_TERRAIN: [usize; 19] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 4, 5, 1, 1, 1, 4, 4, 1, 1];

/// Terrain id the placer refuses outright (RVA 209900). In the map enum this is
/// "river"; whether combat cells use it the same way is not checked yet.
const REJECTED_TERRAIN: u8 = 9;

/// Army start slots of both sides, 7 per side, in map cells (x = row, y = cell
/// in the row). Static initializer RVA 224270, VA 0xaaf1e0 and 0xaaf218.
const START_SLOTS: [[(i32, i32); 7]; 2] = [
    [
        (76, 51),
        (85, 55),
        (76, 60),
        (85, 64),
        (76, 69),
        (85, 73),
        (76, 78),
    ],
    [
        (33, 32),
        (24, 37),
        (33, 41),
        (24, 46),
        (33, 50),
        (24, 55),
        (33, 59),
    ],
];
/// Hero positions of both sides (same initializer, VA 0xaaf2f0 and 0xaaf328).
const HERO_POSITIONS: [(i32, i32); 2] = [(91, 64), (18, 46)];

// Combat random purposes used by the placer.
const PURPOSE_PATH_BEND: u32 = 0x1e;
const PURPOSE_PATH_START: u32 = 0x1f;
const PURPOSE_PATH_END: u32 = 0x20;
const PURPOSE_GATE_PATH_START: u32 = 0x21;
const PURPOSE_PICK_MODEL: u32 = 0x22;
const PURPOSE_DENSITY: u32 = 0x23;

/// Chance (per 1000) that a cell gets an obstacle: at first, and next to one.
const DENSITY_DEFAULT: i32 = 20;
const DENSITY_NEAR_OBSTACLE: i32 = 200;

/// Weights from `table.combat_obstacles` (parser RVA 2089a0).
#[derive(Debug, Clone)]
pub struct ObstacleWeights {
    /// `rows[t]` is the 85-int row of obstacle type `t` (see `ROW_LEN`).
    rows: Vec<[i32; ROW_LEN]>,
}

impl ObstacleWeights {
    /// Read the table the way the exe does. Names that are not known keywords
    /// are skipped silently, like in the original: the installed table has
    /// typos ("snow logs", "burnt logs", "fungal tree"), so for example
    /// `snow_log` never gets a terrain weight and is never placed.
    pub fn from_table(table: &Table) -> ObstacleWeights {
        let mut rows = vec![[0; ROW_LEN]; TYPE_COUNT];
        let Some((header, data)) = table.rows.split_first() else {
            return ObstacleWeights { rows };
        };
        // Column of each obstacle type; a later duplicate wins.
        let mut column_of = [None; TYPE_COUNT];
        for (column, name) in header.iter().enumerate() {
            if let Some(t) = obstacle_type_by_name(name) {
                column_of[usize::from(t)] = Some(column);
            }
        }
        // A missing cell reads as empty text.
        fn cell(row: &[String], i: usize) -> &str {
            row.get(i).map(String::as_str).unwrap_or("")
        }
        let frequency = |word: &str| {
            FREQUENCIES
                .iter()
                .position(|f| f.eq_ignore_ascii_case(word))
        };
        // The first cell names the section; an empty one continues the previous.
        let mut section = 0;
        for row in data {
            let title = cell(row, 0);
            if !title.is_empty() {
                if let Some(s) = ["terrain", "adjacent", "overlap"]
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(title))
                {
                    section = s;
                }
            }
            // Which slot of each type's row this table row fills.
            let slot = match section {
                0 => TERRAIN_NAMES
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(cell(row, 1))),
                1 => obstacle_type_by_name(cell(row, 1)).map(|t| ADJACENT_AT + usize::from(t)),
                _ => obstacle_type_by_name(cell(row, 1)).map(|t| OVERLAP_AT + usize::from(t)),
            };
            let Some(slot) = slot else { continue };
            let values = [TERRAIN_VALUES, ADJACENT_VALUES, OVERLAP_VALUES][section];
            for (t, column) in column_of.iter().enumerate() {
                let Some(column) = column else { continue };
                if let Some(f) = frequency(cell(row, *column)) {
                    rows[t][slot] = values[f];
                }
            }
        }
        ObstacleWeights { rows }
    }

    /// The whole table in the exe's memory layout: 38 rows of 85 ints
    /// (for the comparison bench).
    pub fn exe_layout(&self) -> Vec<i32> {
        self.rows.iter().flatten().copied().collect()
    }

    /// Weight of obstacle type `t` on terrain id `terrain` (0 if unknown).
    pub fn terrain(&self, t: usize, terrain: u8) -> i32 {
        WEIGHT_TERRAIN
            .get(usize::from(terrain))
            .map_or(0, |&column| self.rows[t][column])
    }

    /// How much a candidate of type `candidate` likes standing near `placed`.
    pub fn adjacent(&self, placed: usize, candidate: usize) -> i32 {
        self.rows[placed][ADJACENT_AT + candidate]
    }

    /// How much a candidate of type `candidate` likes overlapping `placed`.
    pub fn overlap(&self, placed: usize, candidate: usize) -> i32 {
        self.rows[placed][OVERLAP_AT + candidate]
    }
}

/// Facts about the battle the placer needs.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlacerContext {
    /// Battle on a bridge (from the adventure position): no free paths are cut.
    pub on_bridge: bool,
    /// Byte `battlefield+0x36c`: every cell counts as water for the weights.
    /// Meaning not found yet (maybe a battle at sea).
    pub all_water: bool,
    /// Siege battles: where the castle is.
    pub siege: Option<SiegeLayout>,
}

/// Castle geometry used by the placer in a siege.
#[derive(Debug, Clone, Copy)]
pub struct SiegeLayout {
    /// Rows below this belong to the castle (`battlefield+100`): no obstacles there.
    pub castle_x: i32,
    /// Gate cell (`t_battlefield::get_gate_position`, RVA 177bc0); only `y` is used.
    pub gate: (i32, i32),
}

/// One chosen obstacle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObstaclePlacement {
    /// Index into the `models` slice given to `place_obstacles`.
    pub model: usize,
    /// Top-left cell of the model's footprint.
    pub at: (i32, i32),
}

/// Placer state of one cell (`t_obstacle_placer_data`, 0x138 bytes).
#[derive(Debug, Clone)]
struct CellState {
    /// Blocking obstacles may stand here (cleared near start positions and paths).
    allowed: bool,
    /// Already put into the walk queue.
    queued: bool,
    /// Chance per 1000 to try an obstacle here.
    density: i32,
    /// Sums of "adjacent" and "overlap" weights from obstacles placed so far,
    /// one per candidate type.
    adjacent: [i32; TYPE_COUNT],
    overlap: [i32; TYPE_COUNT],
}

/// Half-open rectangle of cells: x in x0..x1, y in y0..y1 (`t_map_rect_2d`).
#[derive(Debug, Clone, Copy)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

/// Choose obstacles for a field (port of `execute`, RVA 20a270).
///
/// `grid` must already have its terrain, presets and edge blocking
/// (`BattlefieldGrid::block_edges`). `models` are the candidates in the order
/// of the header table cache.
pub fn place_obstacles(
    grid: &BattlefieldGrid,
    models: &[CombatHeaderEntry],
    weights: &ObstacleWeights,
    context: &PlacerContext,
    random: &mut CombatRandom,
) -> Vec<ObstaclePlacement> {
    let mut placer = Placer::new(grid, models, weights, context, random);
    placer.clear_start_positions();
    if !context.on_bridge {
        placer.cut_paths();
    }
    // Start the walk at the first cell of the last row.
    let last = grid.row_count() as i32 - 1;
    if let Some((start, _)) = grid.row_bounds(last) {
        placer.queue.push_back((last, start));
        if let Some(cell) = placer.state_mut(last, start) {
            cell.queued = true;
        }
    }
    while let Some(point) = placer.queue.pop_front() {
        placer.examine(point);
    }
    placer.placed
}

struct Placer<'a> {
    grid: &'a BattlefieldGrid,
    models: &'a [CombatHeaderEntry],
    weights: &'a ObstacleWeights,
    context: &'a PlacerContext,
    random: &'a mut CombatRandom,
    /// One row of states per grid row, same bounds.
    states: Vec<Vec<CellState>>,
    queue: VecDeque<(i32, i32)>,
    placed: Vec<ObstaclePlacement>,
}

impl<'a> Placer<'a> {
    fn new(
        grid: &'a BattlefieldGrid,
        models: &'a [CombatHeaderEntry],
        weights: &'a ObstacleWeights,
        context: &'a PlacerContext,
        random: &'a mut CombatRandom,
    ) -> Self {
        // Constructor RVA 208ec0: allowed, not queued, density 20, sums zero.
        let fresh = CellState {
            allowed: true,
            queued: false,
            density: DENSITY_DEFAULT,
            adjacent: [0; TYPE_COUNT],
            overlap: [0; TYPE_COUNT],
        };
        let states = (0..grid.row_count() as i32)
            .map(|x| {
                let (start, end) = grid.row_bounds(x).expect("x is below row_count");
                vec![fresh.clone(); (end - start) as usize]
            })
            .collect();
        Placer {
            grid,
            models,
            weights,
            context,
            random,
            states,
            queue: VecDeque::new(),
            placed: Vec::new(),
        }
    }

    fn state(&self, x: i32, y: i32) -> Option<&CellState> {
        let (start, end) = self.grid.row_bounds(x)?;
        if y < start || y >= end {
            return None;
        }
        Some(&self.states[x as usize][(y - start) as usize])
    }

    fn state_mut(&mut self, x: i32, y: i32) -> Option<&mut CellState> {
        let (start, end) = self.grid.row_bounds(x)?;
        if y < start || y >= end {
            return None;
        }
        Some(&mut self.states[x as usize][(y - start) as usize])
    }

    /// A combat roll as a signed number, the way the exe divides it.
    fn roll(&mut self, x: i32, y: i32, purpose: u32) -> i32 {
        // Rolled cells are inside the field, so the coordinates are not negative.
        self.random.roll(x as u32, y as u32, purpose) as i32
    }

    /// `clear_rect` (RVA 209040): no blocking obstacles in this rectangle.
    /// The exe also removes scenery already standing there (only castle
    /// layouts have any); that part is not ported.
    fn clear(&mut self, r: Rect) {
        for x in r.x0..r.x1 {
            for y in r.y0..r.y1 {
                if let Some(cell) = self.state_mut(x, y) {
                    cell.allowed = false;
                }
            }
        }
    }

    /// `clear_start_positions` (RVA 209190).
    fn clear_start_positions(&mut self) {
        for side in 0..2 {
            // In a siege the defenders stand inside the castle.
            if side == 1 && self.context.siege.is_some() {
                continue;
            }
            let slots = START_SLOTS[side];
            // Two columns of slots: 0..6 and 1..5, each with a margin of 3.
            let mut a = Rect {
                x0: slots[0].0 - 3,
                y0: slots[0].1 - 3,
                x1: slots[6].0 + 4,
                y1: slots[6].1 + 4,
            };
            self.clear(a);
            let mut b = Rect {
                x0: slots[1].0 - 3,
                y0: slots[1].1 - 3,
                x1: slots[5].0 + 4,
                y1: slots[5].1 + 4,
            };
            self.clear(b);
            // Join them with a one-cell-wide staircase from the left one (b)
            // to the right one (a), its ends sliding one cell per step.
            if b.x0 > a.x0 {
                std::mem::swap(&mut a, &mut b);
            }
            let step_y0 = (a.y0 - b.y0).signum();
            let step_y1 = (a.y1 - b.y1).signum();
            let mut c = Rect {
                x0: b.x1 - 1,
                y0: b.y0,
                x1: b.x1,
                y1: b.y1,
            };
            while c.x0 <= a.x0 {
                self.clear(c);
                c.x0 += 1;
                c.x1 += 1;
                c.y0 += step_y0;
                c.y1 += step_y1;
            }
            let (hx, hy) = HERO_POSITIONS[side];
            self.clear(Rect {
                x0: hx - 3,
                y0: hy - 3,
                x1: hx + 4,
                y1: hy + 4,
            });
        }
        if let Some(siege) = self.context.siege {
            let (x, y) = (siege.castle_x, siege.gate.1);
            self.clear(Rect {
                x0: x,
                y0: y,
                x1: x + 7,
                y1: y + 7,
            });
        }
    }

    /// `cut_paths` (RVA 209710): two narrow paths and one wide one.
    fn cut_paths(&mut self) {
        self.cut_random_path(4);
        self.cut_random_path(4);
        match self.context.siege {
            None => self.cut_random_path(7),
            Some(siege) => {
                // The wide path leads to the castle gate.
                let from = (
                    START_SLOTS[0][0].0,
                    self.random_slot_y(0, PURPOSE_GATE_PATH_START),
                );
                let to = (siege.castle_x + 3, siege.gate.1 + 3);
                self.cut_path(from, to, 7);
            }
        }
    }

    /// A random row between the first and the last start slot of a side.
    fn random_slot_y(&mut self, side: usize, purpose: u32) -> i32 {
        let first = START_SLOTS[side][0].1;
        let last = START_SLOTS[side][6].1;
        self.roll(0, 0, purpose) % (last - first + 1) + first
    }

    /// RVA 209650: a path from a random point of side 0's slot column to a
    /// random point of side 1's (or next to the castle wall in a siege).
    fn cut_random_path(&mut self, width: i32) {
        let from = (
            START_SLOTS[0][0].0,
            self.random_slot_y(0, PURPOSE_PATH_START),
        );
        let to_x = match self.context.siege {
            Some(siege) => siege.castle_x + 4,
            None => START_SLOTS[1][0].0,
        };
        let to = (to_x, self.random_slot_y(1, PURPOSE_PATH_END));
        self.cut_path(from, to, width);
    }

    /// RVA 2093f0: clear a `width`-wide square along a wandering line.
    ///
    /// Midpoint displacement: while the next target is more than one cell
    /// away, put a new target halfway there, pushed sideways by a random
    /// amount of up to half the half-distance. When the target is next to the
    /// current point, clear the square around the current point and step on.
    fn cut_path(&mut self, from: (i32, i32), to: (i32, i32), width: i32) {
        let half = (width - 1) / 2;
        let (mut x, mut y) = from;
        // The exe keeps the targets in a list and always works on the last one.
        let mut targets = vec![to];
        while let Some(&(tx, ty)) = targets.last() {
            let (dx, dy) = (tx - x, ty - y);
            if dx.abs() < 2 && dy.abs() < 2 {
                self.clear(Rect {
                    x0: x - half,
                    y0: y - half,
                    x1: x - half + width,
                    y1: y - half + width,
                });
                (x, y) = (tx, ty);
                targets.pop();
                continue;
            }
            // Integer halves, then the length of that half-step. The exe takes
            // an x87 square root and truncates it (`__ftol`); f64 gives the same.
            let (hx, hy) = (dx / 2, dy / 2);
            let length = f64::from(hx * hx + hy * hy).sqrt() as i32;
            let limit = length / 2;
            let shift = if limit > 0 {
                self.roll(0, 0, PURPOSE_PATH_BEND) % (2 * limit + 1) - limit
            } else {
                0
            };
            // Move the midpoint `shift` cells along the perpendicular (−hy, hx),
            // scaled down by the half-step length.
            let (mx, my) = if shift == 0 {
                (hx, hy)
            } else {
                (hx - hy * shift / length, hy + hx * shift / length)
            };
            targets.push((x + mx, y + my));
        }
    }

    /// `push_points` (RVA 2097c0): queue every cell of `r` widened by `margin`
    /// that is inside the field and not queued yet; x outer, y inner.
    fn queue_around(&mut self, r: Rect, margin: i32) {
        for x in r.x0 - margin..r.x1 + margin {
            for y in r.y0 - margin..r.y1 + margin {
                if let Some(cell) = self.state_mut(x, y) {
                    if !cell.queued {
                        cell.queued = true;
                        self.queue.push_back((x, y));
                    }
                }
            }
        }
    }

    /// Footprint of `model` when centred on `p`: top-left cell and size.
    fn footprint(&self, model: usize, (px, py): (i32, i32)) -> Rect {
        let [w, h] = self.models[model].header.properties.map(i32::from);
        let x0 = px - (w - 1) / 2;
        let y0 = py - (h - 1) / 2;
        Rect {
            x0,
            y0,
            x1: x0 + w,
            y1: y0 + h,
        }
    }

    /// `get_weight` (RVA 209900): how good `model` is centred on `p`;
    /// 0 or less means impossible.
    fn weight(&self, model: usize, p: (i32, i32)) -> i32 {
        let header = &self.models[model].header;
        let t = usize::from(header.resolved_type());
        let area = self.footprint(model, p);
        let mut lowest_terrain = 100;
        let mut score = 5;
        for x in area.x0..area.x1 {
            for y in area.y0..area.y1 {
                let (Some(state), Some(cell)) = (self.state(x, y), self.grid.cell(x, y)) else {
                    return 0;
                };
                if header.blocks() && !state.allowed {
                    return 0;
                }
                let mut terrain = cell.surface.terrain & 0x1f;
                if terrain == REJECTED_TERRAIN || cell.surface.flags & FLAG_10 == 0 {
                    return 0;
                }
                if self.context.all_water {
                    terrain = 0;
                }
                if self.context.siege.is_some_and(|s| x < s.castle_x) {
                    return 0;
                }
                let fit = self.weights.terrain(t, terrain);
                if fit < 1 {
                    return 0;
                }
                lowest_terrain = lowest_terrain.min(fit);
                score += state.adjacent[t] + state.overlap[t];
                if score < -4999 {
                    return 0;
                }
            }
        }
        score.wrapping_mul(lowest_terrain)
    }

    /// `examine` (RVA 209fa0): maybe put an obstacle centred on `p`; either
    /// way the walk continues to the neighbours.
    fn examine(&mut self, p: (i32, i32)) {
        let (density, allowed) = {
            let cell = self.state(p.0, p.1).expect("only field cells are queued");
            (cell.density, cell.allowed)
        };
        if self.roll(p.0, p.1, PURPOSE_DENSITY) % 1000 < density && allowed {
            // Every model that fits, with its weight; and how many fit per type.
            let mut candidates = Vec::new();
            let mut per_type = [0; TYPE_COUNT];
            for model in 0..self.models.len() {
                let weight = self.weight(model, p);
                if weight > 0 {
                    per_type[usize::from(self.models[model].header.resolved_type())] += 1;
                    candidates.push((model, weight));
                }
            }
            // Give every type the same chance whatever its number of models:
            // scale each weight by (largest count / count of its type).
            let most = per_type.iter().copied().max().unwrap_or(0);
            let mut total = 0;
            for (model, weight) in &mut candidates {
                let count = per_type[usize::from(self.models[*model].header.resolved_type())];
                *weight = (weight.wrapping_mul(most) / count).max(1);
                total += *weight;
            }
            if total != 0 {
                // Walk the list until the roll is used up (`< 1`, so a roll
                // equal to the first weight still picks the first model).
                let mut left = self.roll(p.0, p.1, PURPOSE_PICK_MODEL) % total;
                let mut chosen = candidates[0].0;
                for &(model, weight) in &candidates {
                    chosen = model;
                    left -= weight;
                    if left < 1 {
                        break;
                    }
                }
                self.place(chosen, p);
                return;
            }
        }
        let here = Rect {
            x0: p.0,
            y0: p.1,
            x1: p.0 + 1,
            y1: p.1 + 1,
        };
        self.queue_around(here, 1);
    }

    /// RVA 209b40: record the obstacle and update the neighbourhood.
    fn place(&mut self, model: usize, p: (i32, i32)) {
        let area = self.footprint(model, p);
        self.placed.push(ObstaclePlacement {
            model,
            at: (area.x0, area.y0),
        });
        self.queue_around(area, 2);
        let t = usize::from(self.models[model].header.resolved_type());
        // Cells under the obstacle learn what overlapping it is worth...
        for x in area.x0..area.x1 {
            for y in area.y0..area.y1 {
                let weights = self.weights;
                if let Some(cell) = self.state_mut(x, y) {
                    for (candidate, sum) in cell.overlap.iter_mut().enumerate() {
                        *sum += weights.overlap(t, candidate);
                    }
                }
            }
        }
        // ...and cells within 3 of it what standing next to it is worth.
        for x in area.x0 - 3..area.x1 + 3 {
            for y in area.y0 - 3..area.y1 + 3 {
                let weights = self.weights;
                if let Some(cell) = self.state_mut(x, y) {
                    for (candidate, sum) in cell.adjacent.iter_mut().enumerate() {
                        *sum += weights.adjacent(t, candidate);
                    }
                }
            }
        }
        // Bigger obstacles make a wider zone where more scenery is likely.
        let reach = (area.y1 - area.y0 - 2 + area.x1 - area.x0) / 3;
        if reach >= 1 {
            for x in area.x0 - reach..area.x1 + reach {
                for y in area.y0 - reach..area.y1 + reach {
                    if let Some(cell) = self.state_mut(x, y) {
                        cell.density = DENSITY_NEAR_OBSTACLE;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battlefield::CellSurface;
    use h4_formats::combat_object::CombatObjectHeader;

    fn table(rows: &[&[&str]]) -> Table {
        Table {
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| c.to_string()).collect())
                .collect(),
        }
    }

    #[test]
    fn weights_follow_sections_and_skip_unknown_names() {
        let t = table(&[
            &["Obstacle Type", "", "autumn_tree", "snow logs", "TAN_ROCK"],
            &["Terrain", "grass", "Common", "usually", "rare"],
            &["", "Dirt", "never", "usually", "seldom"],
            &["Adjacent", "autumn_tree", "usually", "", "never"],
            &["", "fungal tree", "usually", "usually", "usually"],
            &["Overlap", "tan_rock", "rare", "", "common"],
        ]);
        let w = ObstacleWeights::from_table(&t);
        let (autumn, tan, snow_log) = (0, 33, 23);
        assert_eq!(w.terrain(autumn, 1), 20);
        assert_eq!(w.terrain(tan, 1), 5);
        assert_eq!(w.terrain(tan, 7), 10);
        // "snow logs" is not a keyword: the column is ignored.
        assert_eq!(w.terrain(snow_log, 1), 0);
        // River (9) uses the water column, lava (10) the volcanic one.
        assert_eq!(w.terrain(autumn, 9), w.terrain(autumn, 0));
        assert_eq!(w.adjacent(autumn, autumn), 100);
        assert_eq!(w.adjacent(tan, autumn), -5000);
        assert_eq!(w.overlap(autumn, tan), 0);
        assert_eq!(w.overlap(tan, tan), 30);
    }

    fn model(t: u16, w: u8, h: u8, blocks: bool) -> CombatHeaderEntry {
        CombatHeaderEntry {
            name: format!("test.{t}"),
            header: CombatObjectHeader {
                version: 2,
                properties: [w, h],
                parameter: 0,
                frames_per_second: 10,
                flag: 0,
                attributes: [u8::from(blocks), 0, 0, 0, 0],
                type_id: t,
                type_name: String::new(),
            },
        }
    }

    fn grass_weights() -> ObstacleWeights {
        ObstacleWeights::from_table(&table(&[
            &["Obstacle Type", "", "autumn_tree", "tan_rock"],
            &["Terrain", "grass", "common", "rare"],
            &["Overlap", "autumn_tree", "never", "never"],
            &["", "tan_rock", "never", "never"],
        ]))
    }

    fn grass_field() -> BattlefieldGrid {
        let mut grid = BattlefieldGrid::standard(CellSurface {
            terrain: 1,
            flags: FLAG_10,
        });
        grid.block_edges();
        grid
    }

    #[test]
    fn same_seed_same_field_and_footprints_stay_inside() {
        let grid = grass_field();
        let models = [
            model(0, 2, 2, true),
            model(33, 3, 1, true),
            model(33, 1, 1, false),
        ];
        let weights = grass_weights();
        let run = |seed| {
            let mut random = CombatRandom::new(seed, 1);
            place_obstacles(
                &grid,
                &models,
                &weights,
                &PlacerContext::default(),
                &mut random,
            )
        };
        let first = run(7);
        assert_eq!(first, run(7));
        assert_ne!(first, run(8));
        assert!(!first.is_empty());
        for p in &first {
            let [w, h] = models[p.model].header.properties.map(i32::from);
            for x in p.at.0..p.at.0 + w {
                for y in p.at.1..p.at.1 + h {
                    assert!(grid.cell(x, y).is_some(), "{p:?} leaves the field");
                }
            }
        }
    }

    #[test]
    fn start_positions_and_heroes_stay_free_of_blocking_obstacles() {
        let grid = grass_field();
        let models = [model(0, 1, 1, true)];
        let weights = grass_weights();
        for seed in 0..5 {
            let mut random = CombatRandom::new(seed, 1);
            let placed = place_obstacles(
                &grid,
                &models,
                &weights,
                &PlacerContext::default(),
                &mut random,
            );
            for p in &placed {
                for (side, slots) in START_SLOTS.iter().enumerate() {
                    for &(sx, sy) in slots.iter().chain([&HERO_POSITIONS[side]]) {
                        assert!(
                            (p.at.0 - sx).abs() > 2 || (p.at.1 - sy).abs() > 2,
                            "seed {seed}: {p:?} next to ({sx}, {sy})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cells_without_the_obstacle_flag_get_nothing() {
        let grid = BattlefieldGrid::standard(CellSurface {
            terrain: 1,
            flags: 0,
        });
        let models = [model(0, 1, 1, true)];
        let mut random = CombatRandom::new(3, 1);
        let placed = place_obstacles(
            &grid,
            &models,
            &grass_weights(),
            &PlacerContext::default(),
            &mut random,
        );
        assert!(placed.is_empty());
    }
}
