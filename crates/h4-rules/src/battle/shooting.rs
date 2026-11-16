//! The distance penalty of a shot, as the exe computes it (rva `170800`, read
//! from the disassembly; `decomp/shooting.md`). Not run in Unicorn yet.
//!
//! The divisor that `1eebf0` applies to the shot's damage is
//! [`range_effect`]`(edge distance, shot passed an obstacle)`. Three pieces:
//!
//! - [`can_aim`] — the checks `170800` makes before it measures anything;
//! - [`edge_distance`] — how far the target is in cells (`1f17d0`);
//! - [`range_effect`] — the penalty table (`2509f0`).
//!
//! What is *not* ported: the trace along the line of fire (`16fe00`), whose
//! result says whether the shot passed an obstacle ([`LineOfFire`]); it needs
//! the battlefield cells and is read only partially (`decomp/shooting.md`).

/// Ability numbers (the game's keywords).
pub mod abilities {
    pub const RANGED: u8 = 0x33;
    /// No distance penalty at all.
    pub const LONG_RANGE: u8 = 0x24;
    /// The penalty grows already at 20 cells.
    pub const SHORT_RANGE: u8 = 0x3c;
    /// Siege machines: an obstacle on the line does not double the penalty.
    pub const SIEGE_MACHINE: u8 = 0x3d;
}

/// Effects (`t_spell`) the checks look at; their spell names are not checked yet.
pub mod effects {
    /// Stops a stack from shooting.
    pub const NO_SHOOTING: u16 = 0x2f;
    /// On the target: it cannot be aimed at.
    pub const UNTARGETABLE: u16 = 0x89;
}

/// What the line-of-fire trace (`16fe00`) found. The exe writes 0, 1 or 2 to an
/// output argument; only 1 changes the penalty, and 2 refuses the shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOfFire {
    /// Nothing in the way (0).
    Clear,
    /// The shot passes an obstacle or another stack (1): the penalty doubles.
    Obstructed,
    /// The shot cannot be made (2).
    Blocked,
}

/// The shooter and the target as `170800` asks about them before it measures.
#[derive(Debug, Clone, Default)]
pub struct AimCheck<'a> {
    pub shooter_abilities: &'a [u8],
    pub shooter_effects: &'a [u16],
    pub shots: i32,
    /// The battlefield's threat number for the shooter (an enemy next to it).
    pub threats: i32,
    pub target_effects: &'a [u16],
}

/// May this stack shoot at this target at all (the first checks of `170800`):
/// it has `ranged`, no effect `0x2f`, shots left, no enemy next to it, and the
/// target has no effect `0x89`. The trace result is checked separately.
pub fn can_aim(c: &AimCheck) -> bool {
    c.shooter_abilities.contains(&abilities::RANGED)
        && !c.shooter_effects.contains(&effects::NO_SHOOTING)
        && c.shots > 0
        && c.threats == 0
        && !c.target_effects.contains(&effects::UNTARGETABLE)
}

/// Distance in whole cells from the shooter to the edge of the target
/// (rva `1f17d0`). All positions are in 16ths of a cell. `shooter_center` is the
/// shooter's footprint center (the exe takes it from the cell it stands on or
/// would shoot from); `*_size` is the footprint side in cells.
///
/// The target's center is its position plus half its size. The distance is the
/// rounded straight line (32-bit squares, x87 root, `+0.5` and truncation, the
/// same as `retaliation`'s reach), minus both half sizes plus half a cell, in
/// cells rounded down.
pub fn edge_distance(shooter_center: (i32, i32), shooter_size: i32, target_position: (i32, i32), target_size: i32) -> i32 {
    let center = (target_position.0 + target_size * 8, target_position.1 + target_size * 8);
    let dx = shooter_center.0.wrapping_sub(center.0);
    let dy = shooter_center.1.wrapping_sub(center.1);
    let squares = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let distance = (f64::from(squares).sqrt() + 0.5) as i32;
    (distance - target_size * 8 - shooter_size * 8 + 8) >> 4
}

/// The divisor of a shot's damage (rva `2509f0`) for a target `distance`
/// cells away:
///
/// ```text
/// long_range:  1
/// short_range: 1 below 20 cells, 2 from 20, 4 from 40
/// otherwise:   1 below 40 cells, 2 from 40
/// × 2 when the shot passed an obstacle, unless the shooter is a siege machine
/// ```
pub fn range_effect(distance: i32, abilities: &[u8], obstructed: bool) -> i32 {
    let has = |a: u8| abilities.contains(&a);
    let mut effect = if has(self::abilities::LONG_RANGE) {
        1
    } else if has(self::abilities::SHORT_RANGE) {
        match distance {
            40.. => 4,
            20.. => 2,
            _ => 1,
        }
    } else if distance >= 40 {
        2
    } else {
        1
    };
    if obstructed && !has(self::abilities::SIEGE_MACHINE) {
        effect *= 2;
    }
    effect
}

/// Obstacle type numbers the trace tells apart.
pub mod obstacle_types {
    /// A wall piece: always stops the line.
    pub const WALL: i32 = 0x24;
    /// A castle gate: stops the line while it is closed.
    pub const GATE: i32 = 0x22;
}

/// A stack as the trace sees it. `Ref` is whatever the caller uses to name stacks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FireStack {
    /// Virtual slot `+0x74` of the creature: which side it fights for.
    pub side: bool,
    /// The byte at `+0x9bc` of the creature. Its meaning is not known; a stack
    /// with it set takes the shot even when another stack stands before it.
    pub flag_9bc: bool,
    /// Effects (`t_spell`) on the stack; the trace asks about 0x06 and 0x89.
    pub effects: Vec<u16>,
    /// Footprint center in 16ths of a cell (position plus half the size).
    pub center: (i32, i32),
}

/// What lies in one battlefield cell.
#[derive(Debug, Clone, Default)]
pub struct FireCell<Ref> {
    /// Bit `0x20` of the cell's second byte: "there may be an obstacle here".
    pub obstacle_flag: bool,
    /// The type of the obstacle in the cell (`get_obstacle`), if one is found.
    pub obstacle: Option<i32>,
    /// The living stack in the cell (`get_creature`).
    pub creature: Option<Ref>,
}

/// The battlefield as `16fe00` reads it.
pub trait FireField {
    type Ref: Copy + PartialEq;
    /// Number of rows (`x` values).
    fn rows(&self) -> i32;
    /// First and one-past-last `y` of a row (`d2290`, `d2270`).
    fn row_bounds(&self, x: i32) -> (i32, i32);
    fn cell(&self, x: i32, y: i32) -> FireCell<Self::Ref>;
    fn stack(&self, stack: Self::Ref) -> FireStack;
    /// The castle gate of this battlefield: `None` if there is none, else
    /// whether it is open.
    fn gate_open(&self) -> Option<bool>;
}

/// Center of a footprint standing at `position` (16ths of a cell) with side `size`.
pub fn footprint_center(position: (i32, i32), size: i32) -> (i32, i32) {
    (position.0.wrapping_add(size * 8), position.1.wrapping_add(size * 8))
}

/// Walk the line of fire from the shooter to the target point (rva `16fe00`).
///
/// `shooter_center` and `target` are in 16ths of a cell. The line is cut into
/// `max(|dx|, |dy|) >> 4` steps and cells are visited from the shooter's side to
/// the target, the last one being the target's own cell. `exact_center` is the
/// exe's fourth argument: a stack counts only if its center is exactly at
/// `target`. `skip_friend` is the fifth: a friendly stack that is not ignored.
///
/// Returns the stack the shot would hit (the first one on the line, or the last
/// one with `flag_9bc`) and the verdict. The comments mark the exe's own
/// variables so the control flow can be checked against the disassembly.
pub fn trace_line_of_fire<F: FireField>(
    field: &F,
    shooter: F::Ref,
    shooter_center: (i32, i32),
    target: (i32, i32),
    exact_center: bool,
    skip_friend: Option<F::Ref>,
) -> (Option<F::Ref>, LineOfFire) {
    let dx = target.0.wrapping_sub(shooter_center.0);
    let dy = target.1.wrapping_sub(shooter_center.1);
    let steps = dx.wrapping_abs().max(dy.wrapping_abs()) >> 4;
    let mut verdict = 0; // the output argument: 0 clear, 1 obstructed, 2 blocked
    let mut hit: Option<F::Ref> = None; // [esp+0x18]
    if steps == 0 {
        return (None, LineOfFire::Clear);
    }
    let me = field.stack(shooter);
    let mut wall_seen = false; // [esp+0x13]
    let (mut wall_first, mut wall_last) = (0, 0); // [esp+0x24], [esp+0x30]
    let mut shooter_index = 0; // [esp+0x2c]
    let mut run: Option<F::Ref> = None; // [esp+0x20]
    let mut run_index = 0; // [esp+0x28]

    let mut i = steps - 1;
    loop {
        // Position of this step: the target minus a share of the line (truncating division).
        let cx = target.0.wrapping_sub(dx.wrapping_mul(i) / steps) >> 4;
        let cy = target.1.wrapping_sub(dy.wrapping_mul(i) / steps) >> 4;
        step(
            field, shooter, &me, (cx, cy), i, (dx, target), exact_center, skip_friend,
            &mut State {
                verdict: &mut verdict, hit: &mut hit, wall_seen: &mut wall_seen,
                wall_first: &mut wall_first, wall_last: &mut wall_last,
                shooter_index: &mut shooter_index, run: &mut run, run_index: &mut run_index,
            },
        );
        if i == 0 {
            break;
        }
        i -= 1;
    }
    if wall_seen && hit.is_none() && dx < 0 {
        verdict = 2;
    }
    (hit, match verdict { 0 => LineOfFire::Clear, 1 => LineOfFire::Obstructed, _ => LineOfFire::Blocked })
}

/// The trace's running variables, named after their stack slots in the exe.
struct State<'a, R> {
    verdict: &'a mut i32,
    hit: &'a mut Option<R>,
    wall_seen: &'a mut bool,
    wall_first: &'a mut i32,
    wall_last: &'a mut i32,
    shooter_index: &'a mut i32,
    run: &'a mut Option<R>,
    run_index: &'a mut i32,
}

/// One cell of the line; every `return` is the exe's jump to the next step.
#[allow(clippy::too_many_arguments)]
fn step<F: FireField>(
    field: &F,
    shooter: F::Ref,
    me: &FireStack,
    (cx, cy): (i32, i32),
    i: i32,
    (dx, target): (i32, (i32, i32)),
    exact_center: bool,
    skip_friend: Option<F::Ref>,
    st: &mut State<F::Ref>,
) {
    if cx < 0 || cx >= field.rows() {
        return;
    }
    let (start, end) = field.row_bounds(cx);
    if cy < start || cy >= end {
        return;
    }
    let cell = field.cell(cx, cy);
    if cell.obstacle_flag {
        let wall = match cell.obstacle {
            Some(obstacle_types::WALL) => true,
            Some(obstacle_types::GATE) => field.gate_open() == Some(false),
            _ => false,
        };
        if wall {
            if !*st.wall_seen {
                *st.wall_first = i;
            }
            *st.wall_seen = true;
            *st.wall_last = i;
        } else if *st.verdict == 0 {
            *st.verdict = 1;
        }
    }
    let mut c = cell.creature;
    if c == Some(shooter) {
        *st.shooter_index = i;
        return;
    }
    // Which stack counts here.
    if exact_center && c.is_some_and(|stack| field.stack(stack).center != target) {
        c = None;
    }
    if c.is_none() || c != *st.run {
        *st.run = c;
        *st.run_index = i;
    }
    if let Some(stack) = c {
        let other = field.stack(stack);
        let ignored = if other.side == me.side
            && !me.effects.contains(&6)
            && !other.effects.contains(&6)
        {
            // A friend is looked through, except the one the caller named.
            Some(stack) != skip_friend
        } else {
            false
        };
        if ignored || other.effects.contains(&0x89) {
            return;
        }
    }
    if *st.wall_seen && !me.flag_9bc {
        if dx >= 0 {
            if *st.shooter_index - *st.wall_last > 4 {
                *st.verdict = 2;
                return;
            }
        } else {
            if *st.wall_first - *st.run_index > 4 {
                *st.verdict = 2;
            } else if *st.verdict == 0 {
                *st.verdict = 1;
            }
        }
    }
    let Some(stack) = c else { return };
    let flagged = field.stack(stack).flag_9bc;
    if st.hit.is_some() && !flagged {
        return;
    }
    *st.hit = Some(stack);
    if flagged {
        *st.verdict = 1;
    }
}

/// What `170800` reads beyond the field's stacks: the shooter's own numbers
/// and where both stacks stand (16ths of a cell, the corner of the footprint).
#[derive(Debug, Clone, Default)]
pub struct ShotInput<'a> {
    pub shooter_abilities: &'a [u8],
    pub shots: i32,
    /// The battlefield's threat number for the shooter (see [`AimCheck`]).
    pub threats: i32,
    pub shooter_position: (i32, i32),
    pub shooter_size: i32,
    pub target_position: (i32, i32),
    pub target_size: i32,
}

/// The divisor of the shot's damage (rva `170800`, whole chain): 0 when the
/// shot cannot be made, else the penalty of [`range_effect`]. `1eebf0` then
/// divides by at least 1.
///
/// The shooter's effects and the target's come from the field's stacks. The
/// line of fire is traced to the target's center; the shot needs to hit the
/// target itself and not be refused (verdict 2). The distance is measured
/// from the cell the shooter stands in (its position rounded down to a cell).
pub fn shot_range_penalty<F: FireField>(field: &F, shooter: F::Ref, target: F::Ref, input: &ShotInput) -> i32 {
    let (me, other) = (field.stack(shooter), field.stack(target));
    let aim = AimCheck {
        shooter_abilities: input.shooter_abilities,
        shooter_effects: &me.effects,
        shots: input.shots,
        threats: input.threats,
        target_effects: &other.effects,
    };
    if !can_aim(&aim) {
        return 0;
    }
    let aim_at = footprint_center(input.target_position, input.target_size);
    let from = footprint_center(input.shooter_position, input.shooter_size);
    let (hit, verdict) = trace_line_of_fire(field, shooter, from, aim_at, false, None);
    if verdict == LineOfFire::Blocked || hit != Some(target) {
        return 0;
    }
    let cell = (input.shooter_position.0 >> 4 << 4, input.shooter_position.1 >> 4 << 4);
    let distance = edge_distance(
        footprint_center(cell, input.shooter_size),
        input.shooter_size,
        input.target_position,
        input.target_size,
    );
    range_effect(distance, input.shooter_abilities, verdict == LineOfFire::Obstructed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_shooters_lose_half_from_forty_cells() {
        assert_eq!(range_effect(39, &[], false), 1);
        assert_eq!(range_effect(40, &[], false), 2);
        assert_eq!(range_effect(100, &[], false), 2);
    }

    #[test]
    fn short_range_has_two_steps_and_long_range_none() {
        let short = [abilities::SHORT_RANGE];
        assert_eq!([19, 20, 39, 40].map(|d| range_effect(d, &short, false)), [1, 2, 2, 4]);
        assert_eq!(range_effect(500, &[abilities::LONG_RANGE], false), 1);
        // long_range wins over short_range: the exe asks for it first.
        assert_eq!(range_effect(500, &[abilities::SHORT_RANGE, abilities::LONG_RANGE], false), 1);
    }

    #[test]
    fn an_obstacle_doubles_the_penalty_except_for_siege_machines() {
        assert_eq!(range_effect(5, &[], true), 2);
        assert_eq!(range_effect(40, &[abilities::SHORT_RANGE], true), 8);
        assert_eq!(range_effect(5, &[abilities::LONG_RANGE], true), 2);
        assert_eq!(range_effect(5, &[abilities::SIEGE_MACHINE], true), 1);
    }

    #[test]
    fn edge_distance_counts_cells_between_the_edges() {
        // Two 1-cell stacks 5 cells apart on a row: centers 80 apart, edges 4 cells.
        assert_eq!(edge_distance((8, 8), 1, (80, 0), 1), 4);
        // Touching stacks are 0 apart.
        assert_eq!(edge_distance((8, 8), 1, (16, 0), 1), 0);
        // Bigger footprints shorten it.
        assert_eq!(edge_distance((16, 16), 2, (96, 0), 2), 4);
    }

    /// One row of cells; stacks and walls placed by hand.
    struct Row {
        cells: Vec<FireCell<u8>>,
    }

    impl FireField for Row {
        type Ref = u8;
        fn rows(&self) -> i32 {
            1
        }
        fn row_bounds(&self, _: i32) -> (i32, i32) {
            (0, self.cells.len() as i32)
        }
        fn cell(&self, _: i32, y: i32) -> FireCell<u8> {
            self.cells[y as usize].clone()
        }
        fn stack(&self, stack: u8) -> FireStack {
            // 1 is the shooter; 2 an enemy; 3 a friend of the shooter.
            FireStack { side: stack != 2, ..Default::default() }
        }
        fn gate_open(&self) -> Option<bool> {
            None
        }
    }

    fn row(stacks: &[(usize, u8)]) -> Row {
        let mut cells = vec![FireCell::default(); 8];
        for &(at, stack) in stacks {
            cells[at].creature = Some(stack);
        }
        Row { cells }
    }

    #[test]
    fn the_shot_hits_the_first_enemy_and_looks_through_friends() {
        // Shooter in cell 0, a friend in 2, an enemy in 4, the target point in cell 6.
        let field = row(&[(0, 1), (2, 3), (4, 2)]);
        let from = (8, 8);
        // The cell row is x, the cells run along y: aim along y.
        let (hit, verdict) = trace_line_of_fire(&field, 1, from, (8, 16 * 6 + 8), false, None);
        assert_eq!((hit, verdict), (Some(2), LineOfFire::Clear));
        // Without the enemy nothing is hit.
        let empty = row(&[(0, 1), (2, 3)]);
        assert_eq!(trace_line_of_fire(&empty, 1, from, (8, 16 * 6 + 8), false, None), (None, LineOfFire::Clear));
        // A point in the shooter's own cell has no line at all.
        assert_eq!(trace_line_of_fire(&field, 1, from, (8, 8), false, None), (None, LineOfFire::Clear));
    }

    #[test]
    fn a_wall_piece_on_the_line_that_is_not_a_wall_obstructs() {
        let mut field = row(&[(0, 1)]);
        field.cells[3].obstacle_flag = true;
        field.cells[3].obstacle = Some(5);
        let (hit, verdict) = trace_line_of_fire(&field, 1, (8, 8), (8, 16 * 6 + 8), false, None);
        assert_eq!((hit, verdict), (None, LineOfFire::Obstructed));
    }

    #[test]
    fn aiming_needs_ranged_shots_no_neighbours_and_a_free_target() {
        let base = AimCheck { shooter_abilities: &[abilities::RANGED], shots: 3, ..Default::default() };
        assert!(can_aim(&base));
        assert!(!can_aim(&AimCheck { shots: 0, ..base.clone() }));
        assert!(!can_aim(&AimCheck { threats: 1, ..base.clone() }));
        assert!(!can_aim(&AimCheck { shooter_effects: &[effects::NO_SHOOTING], ..base.clone() }));
        assert!(!can_aim(&AimCheck { target_effects: &[effects::UNTARGETABLE], ..base.clone() }));
        assert!(!can_aim(&AimCheck { shooter_abilities: &[], ..base }));
    }
}
