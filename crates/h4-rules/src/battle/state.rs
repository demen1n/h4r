//! An owned, serializable battle, independent of the adventure map and renderer.
//!
//! This is the foundation for #53, not a new combat resolver. The caller supplies
//! the field and effective combat stats; `deploy_initial` supplies recovered
//! deployment (#57), `begin_initiative_round` schedules turns/wait/defense (#60),
//! `add_damage`/`apply_damage` turn damage into losses (#68).
//! The old `battle::fight` remains the map's stand-in until #75. Heroes retain their identity, with combat stats left for #71.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use h4_state::army::{ArmyId, Hero, MapArmy, Slot, SLOTS};
use h4_state::Color;
use serde::{Deserialize, Serialize};

use super::initiative::{
    defending_defense, morale_result, InitiativeStats, RoundInitiative, UnitInitiative,
};
use super::wounds;
use super::Side;
use crate::battlefield::{BattlefieldGrid, CellOccupant, CombatFootprint, PlacedObstacle};
use crate::creatures::CreatureStats;
use crate::random::CombatRandom;

/// Version of our JSON fixture, not the original game's saved-battle format.
pub const SNAPSHOT_VERSION: u32 = 3;

/// Stable within one battle. Also the occupant ID in the collision grid and the
/// ID passed to combat dice. IDs are never vector indices or adventure army IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleArmy {
    pub source: ArmyId,
    pub owner: Option<Color>,
    /// Preserve empty slots and map each original slot to its battle unit.
    pub slots: [Option<UnitId>; SLOTS],
}

/// Top-left collision cell and native rounded-footprint diameter.
/// Real combat models use sizes 3..7; explicit fixtures may use any nonzero size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitPosition {
    pub at: (i32, i32),
    pub size: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UnitKind {
    Creature {
        creature: usize,
        /// Copy table values so a fixture is independent of installed resources.
        stats: CreatureStats,
        initial_count: i32,
        count: i32,
        /// Damage the top creature has taken; the others are unhurt. This is
        /// how the exe keeps it (there is no "top creature hit points" field).
        wounds: i32,
        /// Damage accepted by `add_damage` and not yet turned into wounds.
        pending_damage: i32,
        /// Whether the pending damage kills for good (the flag of the latest
        /// `add_damage`). Who sets it in the game is not recovered yet.
        pending_permanent: bool,
        /// Creatures that died from damage flagged as permanent.
        permanent_deaths: i32,
        shots: i32,
    },
    /// No invented HP or combat stats. Heroes are participants, including in
    /// hero-only armies; their actual fighting rules belong to #71.
    Hero(Hero),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleUnit {
    pub id: UnitId,
    pub side: Side,
    pub slot: usize,
    pub kind: UnitKind,
    /// None until deployment. It is never silently replaced by a guessed cell.
    pub position: Option<UnitPosition>,
}

impl BattleUnit {
    pub fn alive(&self) -> bool {
        match &self.kind {
            UnitKind::Creature { count, .. } => *count > 0,
            UnitKind::Hero(_) => true,
        }
    }

    /// Hit points the whole stack has left, pending damage not counted.
    /// `None` for heroes: their combat hit points are not recovered yet (#71).
    pub fn total_hits(&self) -> Option<i32> {
        match &self.kind {
            UnitKind::Creature {
                stats,
                count,
                wounds,
                ..
            } => wounds::total_hits(*count, *wounds, stats.hit_points).ok(),
            UnitKind::Hero(_) => None,
        }
    }

    /// Hit points of the top creature: what the player sees as its health.
    /// 0 for a dead stack, `None` for heroes.
    pub fn top_hit_points(&self) -> Option<i32> {
        match &self.kind {
            UnitKind::Creature { count: 0, .. } => Some(0),
            UnitKind::Creature { stats, wounds, .. } => Some(stats.hit_points - wounds),
            UnitKind::Hero(_) => None,
        }
    }
}

/// Turn bookkeeping for either native initiative or an explicit fixture order.
/// In native mode `pending` is eligibility storage, not a precomputed sort.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TurnQueue {
    pub round: u32,
    pub active: Option<UnitId>,
    pub pending: VecDeque<UnitId>,
    pub completed: Vec<UnitId>,
    pub initiative: Option<RoundInitiative>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatEvent {
    Created {
        seed: u32,
    },
    Deployed {
        unit: UnitId,
        position: UnitPosition,
    },
    RoundStarted {
        round: u32,
        order: Vec<UnitId>,
    },
    TurnStarted {
        unit: UnitId,
    },
    TurnFinished {
        unit: UnitId,
    },
    InitiativeStarted {
        state: RoundInitiative,
    },
    Waited {
        unit: UnitId,
    },
    Defended {
        unit: UnitId,
    },
    TurnSpeedChanged {
        unit: UnitId,
        speed: i32,
    },
    Rolled {
        first: u32,
        second: u32,
        purpose: u32,
        value: u32,
    },
    /// `add_damage`: `accepted` of the `damage` asked for became pending.
    DamageAdded {
        unit: UnitId,
        damage: i32,
        accepted: i32,
        permanent: bool,
    },
    /// `apply_damage`: pending `damage` became wounds; the stack is left with
    /// `count` creatures and `wounds` on the top one.
    DamageApplied {
        unit: UnitId,
        damage: i32,
        killed: i32,
        count: i32,
        wounds: i32,
    },
    /// `kill`: creatures removed outright, without damage.
    Killed {
        unit: UnitId,
        killed: i32,
        count: i32,
    },
    /// The stack lost its last creature. Its body stays where it fell.
    Died {
        unit: UnitId,
    },
}

/// What `apply_damage` or `kill` did to a stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Losses {
    /// Damage that was turned into wounds (0 for `kill`).
    pub damage: i32,
    pub killed: i32,
    /// Creatures left.
    pub count: i32,
    /// Whether this step killed the stack.
    pub died: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BattleData {
    version: u32,
    field: BattlefieldGrid,
    armies: [BattleArmy; 2],
    units: Vec<BattleUnit>,
    turns: TurnQueue,
    random: CombatRandom,
    events: Vec<CombatEvent>,
}

/// Read access is shared; mutations go through methods that also write events.
/// Loading JSON validates cross-references and collision occupancy before use.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleState {
    data: BattleData,
}

fn side_index(side: Side) -> usize {
    usize::from(side == Side::Defender)
}

fn occupied_ids(field: &BattlefieldGrid) -> BTreeSet<u32> {
    let mut ids = BTreeSet::new();
    for x in 0..field.row_count() as i32 {
        if let Some((start, end)) = field.row_bounds(x) {
            for y in start..end {
                let cell = field
                    .cell(x, y)
                    .expect("y comes from row_bounds(x), so the cell exists");
                ids.extend(cell.occupants.iter().map(|o| o.id));
            }
        }
    }
    ids
}

/// A unit as an object on the collision grid. A living unit blocks its cells;
/// the body of a dead one stays on the field but blocks nothing, like in the
/// exe (`t_combat_creature::blocks_movement`, rva `1ef5a0`: number > 0).
fn unit_obstacle(
    id: UnitId,
    position: UnitPosition,
    blocks: bool,
) -> Result<PlacedObstacle, String> {
    if position.size == 0 {
        return Err("battle units need a nonzero model footprint size".into());
    }
    Ok(PlacedObstacle {
        at: position.at,
        footprint: CombatFootprint {
            size: position.size,
        },
        occupant: CellOccupant {
            id: id.0,
            blocks,
            skip_blocking: false,
        },
    })
}

fn place_unit(
    field: &mut BattlefieldGrid,
    id: UnitId,
    position: UnitPosition,
    blocks: bool,
) -> Result<(), String> {
    let obstacle = unit_obstacle(id, position, blocks)?;
    for (dx, dy) in obstacle.footprint.cells() {
        let x = position
            .at
            .0
            .checked_add(dx)
            .ok_or("unit coordinate overflow")?;
        let y = position
            .at
            .1
            .checked_add(dy)
            .ok_or("unit coordinate overflow")?;
        if field.is_blocked(x, y, Some(id.0)) {
            return Err("unit position is blocked or outside the field".into());
        }
    }
    field.add_obstacle(obstacle).map_err(str::to_owned)
}

impl BattleState {
    /// Own copies of two armies and an already prepared field. This neither
    /// consumes global randomness nor changes either map army. Existing field
    /// occupants are retained as scenery; unit IDs avoid all their identities.
    pub fn from_armies(
        attacker: &MapArmy,
        defender: &MapArmy,
        creatures: &[CreatureStats],
        field: BattlefieldGrid,
        seed: u32,
    ) -> Result<Self, String> {
        if attacker.id == defender.id {
            return Err("an army cannot battle itself".into());
        }
        let mut armies = Vec::new();
        let mut units = Vec::new();
        let mut used = occupied_ids(&field);
        let mut next_id = 1u32;
        for (side, source) in [(Side::Attacker, attacker), (Side::Defender, defender)] {
            let mut army = BattleArmy {
                source: source.id,
                owner: source.owner,
                slots: [None; SLOTS],
            };
            for (slot, member) in source.army.slots.iter().enumerate() {
                let Some(member) = member else { continue };
                while used.contains(&next_id) {
                    next_id = next_id.checked_add(1).ok_or("battle unit IDs exhausted")?;
                }
                let id = UnitId(next_id);
                used.insert(next_id);
                let kind = match member {
                    Slot::Creatures(stack) => {
                        let stats = creatures
                            .get(stack.creature)
                            .ok_or_else(|| {
                                format!("missing creature stats for {}", stack.creature)
                            })?
                            .clone();
                        UnitKind::Creature {
                            creature: stack.creature,
                            wounds: 0,
                            pending_damage: 0,
                            pending_permanent: false,
                            permanent_deaths: 0,
                            shots: stats.shots,
                            stats,
                            initial_count: stack.count,
                            count: stack.count,
                        }
                    }
                    Slot::Hero(hero) => UnitKind::Hero(hero.clone()),
                };
                army.slots[slot] = Some(id);
                units.push(BattleUnit {
                    id,
                    side,
                    slot,
                    kind,
                    position: None,
                });
            }
            if army.slots.iter().all(Option::is_none) {
                return Err("each side needs at least one battle participant".into());
            }
            armies.push(army);
        }
        // Like the stand-in, our IDs/modulus are an explicit local convention,
        // not yet a recovered native battlefield allocation rule.
        let modulus = units
            .iter()
            .map(|u| u.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("battle RNG modulus overflow")?;
        let state = Self {
            data: BattleData {
                version: SNAPSHOT_VERSION,
                field,
                armies: armies.try_into().expect("two battle sides"),
                units,
                turns: TurnQueue::default(),
                random: CombatRandom::new(seed, modulus),
                events: vec![CombatEvent::Created { seed }],
            },
        };
        state.validate()?;
        Ok(state)
    }

    pub fn field(&self) -> &BattlefieldGrid {
        &self.data.field
    }
    pub fn armies(&self) -> &[BattleArmy; 2] {
        &self.data.armies
    }
    pub fn units(&self) -> &[BattleUnit] {
        &self.data.units
    }
    pub fn unit(&self, id: UnitId) -> Option<&BattleUnit> {
        self.data.units.iter().find(|u| u.id == id)
    }
    pub fn turns(&self) -> &TurnQueue {
        &self.data.turns
    }
    pub fn events(&self) -> &[CombatEvent] {
        &self.data.events
    }

    pub(crate) fn clear_deployment(&mut self) -> Result<(), String> {
        if self.data.turns.round != 0 {
            return Err("deployment is closed after round one starts".into());
        }
        for unit in &mut self.data.units {
            self.data.field.remove_object(unit.id.0);
            unit.position = None;
        }
        Ok(())
    }

    pub(crate) fn can_deploy(&self, id: UnitId, position: UnitPosition) -> bool {
        let Ok(obstacle) = unit_obstacle(id, position, true) else {
            return false;
        };
        obstacle.footprint.cells().all(|(dx, dy)| {
            match (position.at.0.checked_add(dx), position.at.1.checked_add(dy)) {
                (Some(x), Some(y)) => !self.data.field.is_blocked(x, y, Some(id.0)),
                _ => false,
            }
        })
    }

    /// Deployment can be revised before round one. Rejected placements leave
    /// both the old occupancy and the log untouched.
    pub fn deploy(&mut self, id: UnitId, position: UnitPosition) -> Result<(), String> {
        if self.data.turns.round != 0 {
            return Err("deployment is closed after round one starts".into());
        }
        let index = self
            .data
            .units
            .iter()
            .position(|u| u.id == id)
            .ok_or("unknown battle unit")?;
        let mut field = self.data.field.clone();
        field.remove_object(id.0);
        place_unit(&mut field, id, position, self.data.units[index].alive())?;
        self.data.field = field;
        self.data.units[index].position = Some(position);
        self.data
            .events
            .push(CombatEvent::Deployed { unit: id, position });
        Ok(())
    }

    /// Begin a round with an externally computed permutation of living units.
    /// This is queue bookkeeping, not a choice of the game's initiative rules.
    pub fn begin_round(&mut self, order: &[UnitId]) -> Result<(), String> {
        if self.data.turns.active.is_some() || !self.data.turns.pending.is_empty() {
            return Err("the previous round still has turns".into());
        }
        let expected: BTreeSet<_> = self
            .data
            .units
            .iter()
            .filter(|u| u.alive())
            .map(|u| u.id)
            .collect();
        let actual: BTreeSet<_> = order.iter().copied().collect();
        if expected.is_empty() || expected != actual || order.len() != actual.len() {
            return Err("turn order must contain each living unit exactly once".into());
        }
        if self
            .data
            .units
            .iter()
            .any(|u| u.alive() && u.position.is_none())
        {
            return Err("deploy all living units before starting a round".into());
        }
        let round = self
            .data
            .turns
            .round
            .checked_add(1)
            .ok_or("round counter overflow")?;
        self.data.turns = TurnQueue {
            round,
            active: None,
            pending: order.iter().copied().collect(),
            completed: Vec::new(),
            initiative: None,
        };
        self.data.events.push(CombatEvent::RoundStarted {
            round,
            order: order.to_vec(),
        });
        Ok(())
    }

    /// Begin a native scheduled round. Provide resolved speed/morale for every
    /// participant, including heroes: hero combat stats are not guessed here.
    /// Morale consumes one (0, unit ID, 0x1c) combat roll per participant in
    /// battlefield order, including dead entries, as native round reset does.
    pub fn begin_initiative_round(
        &mut self,
        stats: &[(UnitId, InitiativeStats)],
    ) -> Result<(), String> {
        let ids: BTreeSet<_> = self.data.units.iter().map(|u| u.id).collect();
        let supplied: BTreeSet<_> = stats.iter().map(|(id, _)| *id).collect();
        if stats.len() != ids.len() || supplied != ids {
            return Err("provide exactly one initiative input for every unit".into());
        }
        for (_, stat) in stats {
            morale_result(stat.morale, 0)?;
        }
        let previous = self.data.turns.initiative.clone();
        let mut trial = self.clone();
        let order: Vec<_> = trial
            .units()
            .iter()
            .filter(|u| u.alive())
            .map(|u| u.id)
            .collect();
        trial.begin_round(&order)?;
        let mut initiative = RoundInitiative {
            last_side: previous
                .as_ref()
                .map_or(Side::Defender, |old| old.last_side),
            entries: Vec::new(),
        };
        for unit in &self.data.units {
            let stat = stats
                .iter()
                .find(|(id, _)| *id == unit.id)
                .expect("IDs validated above")
                .1;
            let roll = (trial.roll(0, unit.id.0, 0x1c) % 10) as u8;
            initiative.entries.push(UnitInitiative {
                unit: unit.id,
                side: unit.side,
                stats: stat,
                morale_roll: roll,
                waiting: false,
                defending: previous
                    .as_ref()
                    .and_then(|old| old.entries.iter().find(|e| e.unit == unit.id))
                    .is_some_and(|e| e.defending),
            });
        }
        trial.data.events.push(CombatEvent::InitiativeStarted {
            state: initiative.clone(),
        });
        trial.data.turns.initiative = Some(initiative);
        *self = trial;
        Ok(())
    }

    /// Wait once in this round. Waiting does not finish the unit's action.
    pub fn wait(&mut self) -> Result<UnitId, String> {
        let id = self.data.turns.active.ok_or("there is no active turn")?;
        let initiative = self
            .data
            .turns
            .initiative
            .as_mut()
            .ok_or("waiting needs a native initiative round")?;
        let entry = initiative.entry_mut(id)?;
        if entry.waiting {
            return Err("this unit has already waited this round".into());
        }
        entry.waiting = true;
        self.data.turns.active = None;
        self.data.turns.pending.push_back(id);
        self.data.events.push(CombatEvent::Waited { unit: id });
        Ok(id)
    }

    /// End the action in a defensive stance. The stance lasts across round
    /// boundaries until this participant is next selected (native 167e2f).
    pub fn defend(&mut self) -> Result<UnitId, String> {
        let id = self.data.turns.active.ok_or("there is no active turn")?;
        let initiative = self
            .data
            .turns
            .initiative
            .as_mut()
            .ok_or("defense needs a native initiative round")?;
        initiative.entry_mut(id)?.defending = true;
        self.data.events.push(CombatEvent::Defended { unit: id });
        self.finish_turn()
    }

    pub fn is_defending(&self, id: UnitId) -> bool {
        self.data.turns.initiative.as_ref().is_some_and(|round| {
            round
                .entries
                .iter()
                .any(|entry| entry.unit == id && entry.defending)
        })
    }

    /// Apply only the native defensive-stance component. The complete damage
    /// command and other spell/artifact modifiers are separate rules (#68).
    pub fn apply_defense_stance(
        &self,
        id: UnitId,
        defense: f32,
        effect_1f: bool,
    ) -> Result<f32, String> {
        self.unit(id).ok_or("unknown battle unit")?;
        Ok(if self.is_defending(id) {
            defending_defense(defense, effect_1f)
        } else {
            defense
        })
    }

    /// Spell/effect rules can refresh effective speed without rebuilding the
    /// round or drawing fresh morale dice. Next selection scans the new values.
    pub fn set_turn_speed(&mut self, id: UnitId, speed: i32) -> Result<(), String> {
        self.data
            .turns
            .initiative
            .as_mut()
            .ok_or("no native initiative round")?
            .entry_mut(id)?
            .stats
            .speed = speed;
        self.data
            .events
            .push(CombatEvent::TurnSpeedChanged { unit: id, speed });
        Ok(())
    }

    /// Units killed while waiting for their turn are dropped from the queue
    /// here: a dead stack cannot act. They stay listed if they already moved.
    pub fn begin_next_turn(&mut self) -> Result<Option<UnitId>, String> {
        if self.data.turns.active.is_some() {
            return Err("finish the active turn first".into());
        }
        // Dead entries are removed before selection; waiting entries remain
        // eligible, but their negative priorities put them after ordinary ones.
        let alive: BTreeSet<_> = self
            .data
            .units
            .iter()
            .filter(|u| u.alive())
            .map(|u| u.id)
            .collect();
        self.data.turns.pending.retain(|id| alive.contains(id));
        let unit = if let Some(initiative) = &self.data.turns.initiative {
            initiative.next(|id| self.data.turns.pending.contains(&id))
        } else {
            self.data.turns.pending.front().copied()
        };
        if let Some(unit) = unit {
            self.data.turns.pending.retain(|id| *id != unit);
            if let Some(initiative) = self.data.turns.initiative.as_mut() {
                let entry = initiative.entry_mut(unit)?;
                let side = entry.side;
                entry.defending = false;
                initiative.last_side = side;
            }
            self.data.turns.active = Some(unit);
            self.data.events.push(CombatEvent::TurnStarted { unit });
            return Ok(Some(unit));
        }
        Ok(None)
    }

    pub fn finish_turn(&mut self) -> Result<UnitId, String> {
        let unit = self
            .data
            .turns
            .active
            .take()
            .ok_or("there is no active turn")?;
        self.data.turns.completed.push(unit);
        self.data.events.push(CombatEvent::TurnFinished { unit });
        Ok(unit)
    }

    /// A hit of `damage` on `target` (`t_combat_creature::add_damage`, rva
    /// `1f2800`). The damage only becomes *pending*: nobody dies until
    /// `apply_damage`. Several hits may pile up first, but never more than the
    /// stack has left, so the returned accepted damage can be less than asked.
    /// `permanent` marks the kills of this damage as permanent deaths.
    ///
    /// Not here yet: the target's luck roll, the martyr redirect, spell shields
    /// and "ignore the next hit" effects that the exe checks in this function.
    pub fn add_damage(
        &mut self,
        target: UnitId,
        damage: i32,
        permanent: bool,
    ) -> Result<i32, String> {
        let unit = self
            .data
            .units
            .iter_mut()
            .find(|u| u.id == target)
            .ok_or("unknown battle unit")?;
        let UnitKind::Creature {
            stats,
            count,
            wounds,
            pending_damage,
            pending_permanent,
            ..
        } = &mut unit.kind
        else {
            return Err("damage to heroes needs their combat stats (#71)".into());
        };
        // The exe ignores a hit below 1 before it touches anything.
        if damage < 1 {
            return Ok(0);
        }
        let total = wounds::total_hits(*count, *wounds, stats.hit_points)?;
        let accepted = wounds::accept_damage(damage, *pending_damage, total)?;
        *pending_damage += accepted;
        *pending_permanent = permanent;
        self.data.events.push(CombatEvent::DamageAdded {
            unit: target,
            damage,
            accepted,
            permanent,
        });
        Ok(accepted)
    }

    /// Turn the pending damage of `target` into wounds and losses
    /// (`t_combat_creature::apply_damage`, rva `1ef2d0`, and `resolve_damage`,
    /// rva `1eee40`). Without pending damage nothing happens and nothing is
    /// logged. A stack that loses its last creature dies: see [`Self::kill`].
    pub fn apply_damage(&mut self, target: UnitId) -> Result<Losses, String> {
        let unit = self
            .data
            .units
            .iter_mut()
            .find(|u| u.id == target)
            .ok_or("unknown battle unit")?;
        let UnitKind::Creature {
            stats,
            count,
            wounds,
            pending_damage,
            pending_permanent,
            permanent_deaths,
            ..
        } = &mut unit.kind
        else {
            return Err("damage to heroes needs their combat stats (#71)".into());
        };
        let damage = *pending_damage;
        if damage == 0 {
            return Ok(Losses {
                damage: 0,
                killed: 0,
                count: *count,
                died: false,
            });
        }
        let resolved = wounds::resolve_damage(*count, *wounds, damage, stats.hit_points)?;
        *count = resolved.count;
        *wounds = resolved.wounds;
        if *pending_permanent {
            *permanent_deaths += resolved.killed;
        }
        *pending_damage = 0;
        *pending_permanent = false;
        self.data.events.push(CombatEvent::DamageApplied {
            unit: target,
            damage,
            killed: resolved.killed,
            count: resolved.count,
            wounds: resolved.wounds,
        });
        let died = resolved.killed > 0 && resolved.count == 0;
        if died {
            self.unit_died(target)?;
        }
        Ok(Losses {
            damage,
            killed: resolved.killed,
            count: resolved.count,
            died,
        })
    }

    /// Remove `number` creatures of `target` outright, without damage
    /// (`t_combat_creature::kill`, rva `1f3860`); the survivors keep the top
    /// creature's wounds. Pending damage that no longer fits is dropped.
    ///
    /// When a stack dies (here or in `apply_damage`) its body stays on its
    /// cells but stops blocking them, and if it was the active unit its turn
    /// ends. Death animations, effects ending with the stack and abilities that
    /// react to a death are not part of this.
    pub fn kill(&mut self, target: UnitId, number: i32) -> Result<Losses, String> {
        if number < 0 {
            return Err("cannot kill a negative number of creatures".into());
        }
        let unit = self
            .data
            .units
            .iter_mut()
            .find(|u| u.id == target)
            .ok_or("unknown battle unit")?;
        let UnitKind::Creature {
            stats,
            count,
            wounds,
            pending_damage,
            ..
        } = &mut unit.kind
        else {
            return Err("killing heroes needs their combat rules (#71)".into());
        };
        let before = *count;
        let resolved = wounds::kill(*count, *wounds, number);
        *count = resolved.count;
        *wounds = resolved.wounds;
        // Our own bookkeeping, not the exe's: keep "pending <= hit points left".
        let total = wounds::total_hits(*count, *wounds, stats.hit_points)?;
        *pending_damage = (*pending_damage).min(total);
        self.data.events.push(CombatEvent::Killed {
            unit: target,
            killed: resolved.killed,
            count: resolved.count,
        });
        let died = before > 0 && resolved.count == 0;
        if died {
            self.unit_died(target)?;
        }
        Ok(Losses {
            damage: 0,
            killed: resolved.killed,
            count: resolved.count,
            died,
        })
    }

    /// Bookkeeping for a stack that just lost its last creature.
    fn unit_died(&mut self, id: UnitId) -> Result<(), String> {
        let position = self.unit(id).ok_or("unknown battle unit")?.position;
        if let Some(position) = position {
            // The same cells, now as a body that blocks nothing.
            self.data.field.remove_object(id.0);
            place_unit(&mut self.data.field, id, position, false)?;
        }
        self.data.events.push(CombatEvent::Died { unit: id });
        if self.data.turns.active == Some(id) {
            self.finish_turn()?;
        }
        Ok(())
    }

    /// Dice and their observable counters belong to the battle. Record each
    /// result so the same action stream can be compared after fixture reload.
    /// This is the only way the battle's generator is used, which is why
    /// loading a snapshot can replay the logged rolls and check the generator.
    pub fn roll(&mut self, first: u32, second: u32, purpose: u32) -> u32 {
        let value = self.data.random.roll(first, second, purpose);
        self.data.events.push(CombatEvent::Rolled {
            first,
            second,
            purpose,
            value,
        });
        value
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let state = Self {
            data: serde_json::from_str(json).map_err(|e| e.to_string())?,
        };
        state.validate()?;
        Ok(state)
    }

    fn validate(&self) -> Result<(), String> {
        let data = &self.data;
        if data.version != SNAPSHOT_VERSION {
            return Err(format!(
                "unsupported battle snapshot version {}",
                data.version
            ));
        }
        if data.armies[0].source == data.armies[1].source {
            return Err("an army cannot battle itself".into());
        }
        if data.armies.iter().any(|a| a.owner.is_some_and(|c| c >= 6)) {
            return Err("invalid battle army owner".into());
        }
        let bounds: Vec<_> = h4_formats::battlefield::row_bounds().collect();
        if data.field.row_count() != bounds.len()
            || bounds.iter().enumerate().any(|(x, &(start, end))| {
                data.field.row_bounds(x as i32) != Some((start as i32, end as i32))
            })
        {
            return Err("battle field does not have the standard geometry".into());
        }
        let mut ids = BTreeSet::new();
        let mut slots = [[None; SLOTS]; 2];
        for unit in &data.units {
            if unit.id.0 == 0 || !ids.insert(unit.id) || unit.slot >= SLOTS {
                return Err("invalid or duplicate battle unit identity".into());
            }
            if slots[side_index(unit.side)][unit.slot]
                .replace(unit.id)
                .is_some()
            {
                return Err("two battle units share an army slot".into());
            }
            if let UnitKind::Creature {
                stats,
                initial_count,
                count,
                wounds,
                pending_damage,
                permanent_deaths,
                shots,
                ..
            } = &unit.kind
            {
                if *initial_count <= 0
                    || *count < 0
                    || count > initial_count
                    || stats.hit_points <= 0
                    || stats.shots < 0
                    || *shots < 0
                    || *shots > stats.shots
                    || *wounds < 0
                    || (*count == 0 && *wounds != 0)
                    || (*count > 0 && *wounds >= stats.hit_points)
                    || *permanent_deaths < 0
                    || *permanent_deaths > initial_count - count
                {
                    return Err("invalid creature count, wounds or shots".into());
                }
                // `add_damage` never lets pending damage exceed what is left.
                let total = wounds::total_hits(*count, *wounds, stats.hit_points)?;
                if *pending_damage < 0 || *pending_damage > total {
                    return Err("invalid pending damage".into());
                }
            }
        }
        if data.armies[0].slots != slots[0] || data.armies[1].slots != slots[1] {
            return Err("army slots do not match battle units".into());
        }
        if data
            .armies
            .iter()
            .any(|a| a.slots.iter().all(Option::is_none))
        {
            return Err("each side needs at least one battle participant".into());
        }
        let random = data.random.snapshot();
        if ids.iter().any(|id| id.0 >= random.id_modulus) {
            return Err("battle unit IDs exceed the combat RNG modulus".into());
        }
        // Check every repeated scenery identity has consistent flags, and each
        // unit occupies exactly its declared footprint, with no extra cells.
        let mut occupants = BTreeMap::new();
        let mut unit_cells: BTreeMap<UnitId, BTreeSet<(i32, i32)>> = BTreeMap::new();
        for (x, &(start, end)) in bounds.iter().enumerate() {
            for y in start..end {
                let cell = data
                    .field
                    .cell(x as i32, y as i32)
                    .expect("the geometry check above matched these bounds");
                let mut seen = BTreeSet::new();
                for occupant in &cell.occupants {
                    if !seen.insert(occupant.id)
                        || occupants
                            .insert(occupant.id, *occupant)
                            .is_some_and(|old| old != *occupant)
                    {
                        return Err("inconsistent or duplicate field occupant".into());
                    }
                    let id = UnitId(occupant.id);
                    if let Some(unit) = data.units.iter().find(|u| u.id == id) {
                        // The living block their cells and stand on free ones;
                        // a body blocks nothing and others may stand over it.
                        let alive = unit.alive();
                        if occupant.skip_blocking
                            || occupant.blocks != alive
                            || (alive && data.field.is_blocked(x as i32, y as i32, Some(id.0)))
                        {
                            return Err("battle unit occupies a blocked cell".into());
                        }
                        unit_cells
                            .entry(id)
                            .or_default()
                            .insert((x as i32, y as i32));
                    }
                }
            }
        }
        for unit in &data.units {
            let expected = match unit.position {
                Some(position) => {
                    let obstacle = unit_obstacle(unit.id, position, unit.alive())?;
                    obstacle
                        .footprint
                        .cells()
                        .map(|(dx, dy)| {
                            Ok((
                                position
                                    .at
                                    .0
                                    .checked_add(dx)
                                    .ok_or("unit coordinate overflow")?,
                                position
                                    .at
                                    .1
                                    .checked_add(dy)
                                    .ok_or("unit coordinate overflow")?,
                            ))
                        })
                        .collect::<Result<BTreeSet<_>, String>>()?
                }
                None => BTreeSet::new(),
            };
            if unit_cells.remove(&unit.id).unwrap_or_default() != expected {
                return Err("battle unit position does not match its cell occupancy".into());
            }
        }
        let turns = &data.turns;
        let queue: Vec<_> = turns
            .pending
            .iter()
            .chain(turns.active.iter())
            .chain(&turns.completed)
            .copied()
            .collect();
        let queued: BTreeSet<_> = queue.iter().copied().collect();
        let alive: BTreeSet<_> = data
            .units
            .iter()
            .filter(|u| u.alive())
            .map(|u| u.id)
            .collect();
        // Units that died during the round may still be listed (pending or
        // completed), so the queue must cover the living, not equal them.
        if (turns.round == 0 && !queue.is_empty())
            || (turns.round > 0
                && (!queued.is_subset(&ids)
                    || !alive.is_subset(&queued)
                    || queue.len() != queued.len()
                    || turns.active.is_some_and(|id| !alive.contains(&id))
                    || data.units.iter().any(|u| u.alive() && u.position.is_none())))
        {
            return Err("invalid battle turn queue".into());
        }
        if let Some(initiative) = &turns.initiative {
            let native_ids: BTreeSet<_> = initiative.entries.iter().map(|e| e.unit).collect();
            if turns.round == 0 || native_ids != ids || native_ids.len() != initiative.entries.len()
            {
                return Err("invalid initiative participants".into());
            }
            for entry in &initiative.entries {
                morale_result(entry.stats.morale, entry.morale_roll)?;
                if self.unit(entry.unit).is_none_or(|u| u.side != entry.side) {
                    return Err("initiative side does not match participant".into());
                }
            }
        }
        if data.events.first() != Some(&CombatEvent::Created { seed: random.seed }) {
            return Err("battle log must begin with its creation seed".into());
        }
        // Replay every logged roll on a fresh generator: the values and the
        // final counters/checkpoints must match what the snapshot stores.
        let mut replay = CombatRandom::new(random.seed, random.id_modulus);
        let mut rounds = 0;
        for event in &data.events {
            match event {
                CombatEvent::Rolled {
                    first,
                    second,
                    purpose,
                    value,
                } => {
                    if replay.roll(*first, *second, *purpose) != *value {
                        return Err("logged roll does not match the battle seed".into());
                    }
                }
                CombatEvent::RoundStarted { round, .. } => {
                    rounds += 1;
                    if *round != rounds {
                        return Err("battle rounds must be logged in order".into());
                    }
                }
                _ => {}
            }
        }
        let mut native: Option<RoundInitiative> = None;
        let mut native_round = false;
        let mut rolls = BTreeMap::new();
        for event in &data.events {
            match event {
                CombatEvent::RoundStarted { .. } => {
                    if !native_round {
                        native = None;
                    }
                    native_round = false;
                    rolls.clear();
                }
                CombatEvent::Rolled {
                    first: 0,
                    second,
                    purpose: 0x1c,
                    value,
                } => {
                    rolls.insert(UnitId(*second), (*value % 10) as u8);
                }
                CombatEvent::InitiativeStarted { state } => {
                    let expected_side = native.as_ref().map_or(Side::Defender, |old| old.last_side);
                    if native_round
                        || state.last_side != expected_side
                        || state.entries.len() != data.units.len()
                    {
                        return Err("invalid initiative start event".into());
                    }
                    for (entry, unit) in state.entries.iter().zip(&data.units) {
                        let defending = native
                            .as_ref()
                            .and_then(|old| old.entries.iter().find(|e| e.unit == entry.unit))
                            .is_some_and(|e| e.defending);
                        if entry.unit != unit.id
                            || entry.side != unit.side
                            || entry.waiting
                            || entry.defending != defending
                            || rolls.get(&entry.unit) != Some(&entry.morale_roll)
                        {
                            return Err("initiative inputs do not match the round log".into());
                        }
                        morale_result(entry.stats.morale, entry.morale_roll)?;
                    }
                    native = Some(state.clone());
                    native_round = true;
                }
                CombatEvent::TurnStarted { unit } if native_round => {
                    let round = native.as_mut().expect("native round event exists");
                    let entry = round.entry_mut(*unit)?;
                    entry.defending = false;
                    let side = entry.side;
                    round.last_side = side;
                }
                CombatEvent::Waited { unit } if native_round => {
                    let entry = native
                        .as_mut()
                        .expect("native round event exists")
                        .entry_mut(*unit)?;
                    if entry.waiting {
                        return Err("unit waited twice in round log".into());
                    }
                    entry.waiting = true;
                }
                CombatEvent::Defended { unit } if native_round => {
                    native
                        .as_mut()
                        .expect("native round event exists")
                        .entry_mut(*unit)?
                        .defending = true;
                }
                CombatEvent::TurnSpeedChanged { unit, speed } if native_round => {
                    native
                        .as_mut()
                        .expect("native round event exists")
                        .entry_mut(*unit)?
                        .stats
                        .speed = *speed;
                }
                CombatEvent::Waited { .. }
                | CombatEvent::Defended { .. }
                | CombatEvent::TurnSpeedChanged { .. } => {
                    return Err("native action outside initiative round".into())
                }
                _ => {}
            }
        }
        if !native_round {
            native = None;
        }
        if native != turns.initiative {
            return Err("initiative state does not match its action log".into());
        }
        if rounds != turns.round {
            return Err("battle log does not match the round counter".into());
        }
        if replay != data.random {
            return Err("combat RNG state does not match the logged rolls".into());
        }
        for event in &data.events {
            let valid = match event {
                CombatEvent::Deployed { unit, position } => {
                    ids.contains(unit) && position.size != 0
                }
                CombatEvent::TurnStarted { unit }
                | CombatEvent::TurnFinished { unit }
                | CombatEvent::Waited { unit }
                | CombatEvent::Defended { unit }
                | CombatEvent::TurnSpeedChanged { unit, .. }
                | CombatEvent::DamageAdded { unit, .. }
                | CombatEvent::DamageApplied { unit, .. }
                | CombatEvent::Killed { unit, .. }
                | CombatEvent::Died { unit } => ids.contains(unit),
                CombatEvent::RoundStarted { order, .. } => order.iter().all(|id| ids.contains(id)),
                CombatEvent::InitiativeStarted { state } => {
                    state.entries.iter().all(|entry| ids.contains(&entry.unit))
                }
                CombatEvent::Rolled { .. } => true,
                CombatEvent::Created { seed } => *seed == random.seed,
            };
            if !valid {
                return Err("invalid battle log entry".into());
            }
        }
        Ok(())
    }
}
