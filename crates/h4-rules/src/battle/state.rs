//! An owned, serializable battle, independent of the adventure map and renderer.
//!
//! This is the foundation for #53, not a new combat resolver. The caller supplies
//! the field, deployment (#57) and turn order (#60); no original placement or
//! initiative rule is guessed here. The old `battle::fight` remains the map's
//! stand-in until #75. Heroes retain their identity, with combat stats left for #71.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use h4_state::army::{ArmyId, Hero, MapArmy, Slot, SLOTS};
use h4_state::Color;
use serde::{Deserialize, Serialize};

use super::Side;
use crate::battlefield::{BattlefieldGrid, CellOccupant, CombatFootprint, PlacedObstacle};
use crate::creatures::CreatureStats;
use crate::random::CombatRandom;

/// Version of our JSON fixture, not the original game's saved-battle format.
pub const SNAPSHOT_VERSION: u32 = 1;

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

/// Explicit top-left cell and square size. Sizes 1 and 2 are supported here;
/// choosing a creature's size and initial position is the caller's job (#57).
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
        top_hp: i32,
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
}

/// Storage and bookkeeping only: the caller supplies the order, not a speed
/// sort masquerading as the original initiative rules. Waiting/defense are #60.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TurnQueue {
    pub round: u32,
    pub active: Option<UnitId>,
    pub pending: VecDeque<UnitId>,
    pub completed: Vec<UnitId>,
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
    Rolled {
        first: u32,
        second: u32,
        purpose: u32,
        value: u32,
    },
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

fn unit_obstacle(id: UnitId, position: UnitPosition) -> Result<PlacedObstacle, String> {
    if !matches!(position.size, 1 | 2) {
        return Err("battle units need an explicit size of 1 or 2".into());
    }
    Ok(PlacedObstacle {
        at: position.at,
        footprint: CombatFootprint {
            size: position.size,
        },
        occupant: CellOccupant {
            id: id.0,
            blocks: true,
            skip_blocking: false,
        },
    })
}

fn place_unit(
    field: &mut BattlefieldGrid,
    id: UnitId,
    position: UnitPosition,
) -> Result<(), String> {
    let obstacle = unit_obstacle(id, position)?;
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
                            top_hp: stats.hit_points,
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
        place_unit(&mut field, id, position)?;
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
        };
        self.data.events.push(CombatEvent::RoundStarted {
            round,
            order: order.to_vec(),
        });
        Ok(())
    }

    /// Units killed while waiting for their turn are dropped from the queue
    /// here: a dead stack cannot act. They stay listed if they already moved.
    pub fn begin_next_turn(&mut self) -> Result<Option<UnitId>, String> {
        if self.data.turns.active.is_some() {
            return Err("finish the active turn first".into());
        }
        while let Some(unit) = self.data.turns.pending.pop_front() {
            if self.unit(unit).is_some_and(BattleUnit::alive) {
                self.data.turns.active = Some(unit);
                self.data.events.push(CombatEvent::TurnStarted { unit });
                return Ok(Some(unit));
            }
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
                top_hp,
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
                    || (*count == 0 && *top_hp != 0)
                    || (*count > 0 && (*top_hp < 1 || *top_hp > stats.hit_points))
                {
                    return Err("invalid creature count, HP or shots".into());
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
                    if ids.contains(&id) {
                        if occupant.skip_blocking
                            || !occupant.blocks
                            || data.field.is_blocked(x as i32, y as i32, Some(id.0))
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
                    let obstacle = unit_obstacle(unit.id, position)?;
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
        if rounds != turns.round {
            return Err("battle log does not match the round counter".into());
        }
        if replay != data.random {
            return Err("combat RNG state does not match the logged rolls".into());
        }
        for event in &data.events {
            let valid = match event {
                CombatEvent::Deployed { unit, position } => {
                    ids.contains(unit) && matches!(position.size, 1 | 2)
                }
                CombatEvent::TurnStarted { unit } | CombatEvent::TurnFinished { unit } => {
                    ids.contains(unit)
                }
                CombatEvent::RoundStarted { order, .. } => order.iter().all(|id| ids.contains(id)),
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
