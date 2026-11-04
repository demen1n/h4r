//! Initial deployment for open-field battles (Buka 3.0, RVA 22da90/22e3b0).
//! Sizes are collision-cell diameters from combat models, not adventure tiles.

use super::state::{BattleState, UnitId, UnitPosition};
use super::Side;

/// The three army formations stored at creature_array + 0x2c.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formation {
    Loose,
    Tight,
    Square,
}

// Static initializer RVA 224270; [formation][side][original army slot].
const STARTS: [[[(i32, i32); 7]; 2]; 3] = [
    [
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
    ],
    [
        [
            (77, 54),
            (84, 57),
            (77, 61),
            (84, 64),
            (77, 68),
            (84, 71),
            (77, 75),
        ],
        [
            (32, 35),
            (25, 39),
            (32, 42),
            (25, 46),
            (32, 49),
            (25, 53),
            (32, 56),
        ],
    ],
    [
        [
            (77, 57),
            (84, 57),
            (77, 64),
            (84, 64),
            (77, 71),
            (84, 71),
            (91, 64),
        ],
        [
            (32, 39),
            (25, 39),
            (32, 46),
            (25, 46),
            (32, 53),
            (25, 53),
            (18, 46),
        ],
    ],
];

impl Formation {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Loose => 0,
            Self::Tight => 1,
            Self::Square => 2,
        }
    }
}

/// An anchor is converted to the top-left of the native rounded footprint.
/// Even diameters deliberately use `(size - 1) / 2`, just like the exe.
pub fn starting_position(
    formation: Formation,
    side: Side,
    slot: usize,
    size: u8,
) -> Result<UnitPosition, String> {
    if size == 0 {
        return Err("a battle model must have a nonzero footprint".into());
    }
    let &(x, y) = STARTS[formation.index()][usize::from(side == Side::Defender)]
        .get(slot)
        .ok_or("army slot must be in 0..7")?;
    let half = i32::from(size - 1) / 2;
    Ok(UnitPosition {
        at: (x - half, y - half),
        size,
    })
}

impl BattleState {
    /// Deploy both armies on a prepared open field, retaining empty slots.
    /// Every unit (including heroes) needs its actual model diameter. Missing,
    /// duplicate or unknown IDs fail atomically, as do blocked start positions.
    /// No game RNG, movement, stack counts or adventure armies are changed.
    /// Siege/tower placement and bridge overrides are separate battle contexts.
    pub fn deploy_initial(
        &mut self,
        formations: [Formation; 2],
        sizes: &[(UnitId, u8)],
    ) -> Result<(), String> {
        if sizes.len() != self.units().len() {
            return Err("provide exactly one model size per battle unit".into());
        }
        let mut trial = self.clone();
        // Clear the whole old deployment before checking new starts, so a unit's
        // previous position cannot accidentally block another unit's new one.
        trial.clear_deployment()?;
        let mut seen = std::collections::BTreeSet::new();
        for &(id, size) in sizes {
            if !seen.insert(id) {
                return Err("duplicate battle unit size".into());
            }
            trial.unit(id).ok_or("unknown battle unit")?;
            if size == 0 {
                return Err("a battle model must have a nonzero footprint".into());
            }
        }
        // Native order: attacker slots 0..6, compact attacker, then defender.
        for (side, formation) in [
            (Side::Attacker, formations[0]),
            (Side::Defender, formations[1]),
        ] {
            let slots = trial.armies()[usize::from(side == Side::Defender)].slots;
            for (slot, id) in slots.iter().enumerate() {
                if let Some(id) = id {
                    let size = sizes
                        .iter()
                        .find(|(unit, _)| unit == id)
                        .ok_or("missing battle unit size")?
                        .1;
                    trial.deploy(*id, starting_position(formation, side, slot, size)?)?;
                }
            }
            trial.compact_formation(formation, slots)?;
        }
        *self = trial;
        Ok(())
    }

    fn compact_formation(
        &mut self,
        formation: Formation,
        slots: [Option<UnitId>; 7],
    ) -> Result<(), String> {
        let mut pull = |from: usize, to: usize, limit| -> Result<bool, String> {
            match (slots[from], slots[to]) {
                (Some(from), Some(to)) => self.pull_towards(from, to, limit),
                _ => Ok(false),
            }
        };
        match formation {
            Formation::Loose => {}
            Formation::Tight => {
                // The native short-circuit order matters when a slot is empty.
                while pull(2, 4, 1)? && pull(4, 2, 1)? {}
                for (from, to) in [(0, 2), (6, 4), (1, 3), (5, 3)] {
                    pull(from, to, 100)?;
                }
            }
            Formation::Square => {
                for (from, to) in [
                    (2, 3),
                    (0, 2),
                    (4, 2),
                    (1, 3),
                    (5, 3),
                    (6, 3),
                    (0, 1),
                    (4, 5),
                ] {
                    pull(from, to, 100)?;
                }
            }
        }
        Ok(())
    }

    /// RVA 22e3b0 walks a fixed sign-vector until the next footprint is blocked.
    /// `limit` is inclusive: the original probes limit + 1 cells, then keeps
    /// the last position before the final probe even when that probe was free.
    fn pull_towards(&mut self, from: UnitId, to: UnitId, limit: usize) -> Result<bool, String> {
        let old = self
            .unit(from)
            .and_then(|u| u.position)
            .ok_or("unit is not deployed")?;
        let target = self
            .unit(to)
            .and_then(|u| u.position)
            .ok_or("target is not deployed")?;
        let dx = ((target.at.0 * 16 + i32::from(target.size) * 8
            - old.at.0 * 16
            - i32::from(old.size) * 8)
            / 16)
            .signum();
        let dy = ((target.at.1 * 16 + i32::from(target.size) * 8
            - old.at.1 * 16
            - i32::from(old.size) * 8)
            / 16)
            .signum();
        let mut probe = old;
        for _ in 0..=limit {
            probe.at.0 += dx;
            probe.at.1 += dy;
            if !self.can_deploy(from, probe) {
                break;
            }
        }
        probe.at.0 -= dx;
        probe.at.1 -= dy;
        if probe == old {
            return Ok(false);
        }
        self.deploy(from, probe)?;
        Ok(true)
    }
}
