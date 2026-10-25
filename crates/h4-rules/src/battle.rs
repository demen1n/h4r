//! A battle between two armies, fought out without a battlefield.
//!
//! **This is a stand-in, not the game's battle.** It exists so the vertical slice (walk,
//! attack, fight, write the result back) works end to end. What is the game's: the damage
//! of each hit (`crate::damage`, `decomp/damage.md`) and the battle's random numbers
//! (`CombatRandom`). What is made up here, until the real battle is decompiled:
//! - no battlefield: nobody moves, every stack can reach every enemy;
//! - turn order by speed, ties to the attacker and then to the lower slot (no morale,
//!   no waiting);
//! - a stack hits the first enemy stack still alive (in slot order);
//! - shooters shoot while they have shots, without range or obstacle penalties, and
//!   aren't struck back; every melee hit is struck back once if the target survives;
//! - no luck, no abilities, no spells; heroes don't fight, and a side with no creatures
//!   left has lost even if it still has heroes;
//! - the winner's heroes each get the experience of every enemy creature killed.

use h4_state::army::{Army, ArmyId, Slot, SLOTS};
use h4_state::GameState;

use crate::creatures::{CreatureStats, TOWN_TYPES};
use crate::damage::{self, Attacker, DamageEvent, Defender, Luck, School, LUCK_MULTIPLIERS};
use crate::random::{CombatRandom, GlobalRandom};

/// The two sides. The attacker is the army that stepped onto the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Attacker,
    Defender,
}

impl Side {
    fn other(self) -> Side {
        match self {
            Side::Attacker => Side::Defender,
            Side::Defender => Side::Attacker,
        }
    }
}

/// What happened, in order: for the battle log and for tests.
#[derive(Debug, Clone, PartialEq)]
pub enum BattleEvent {
    /// Stack `slot` of `side` hit stack `target` of the other side.
    Hit { side: Side, slot: usize, target: usize, ranged: bool, retaliation: bool, damage: i32, killed: i32 },
    /// The fight was stopped after this many rounds without a winner (it shouldn't happen:
    /// every hit does at least 1 damage).
    TooLong { rounds: u32 },
}

/// The result of a battle.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleOutcome {
    pub winner: Side,
    /// Both armies after the battle, with the dead removed. The loser's army may be
    /// empty of creatures but still hold its heroes.
    pub attacker: Army,
    pub defender: Army,
    /// Creatures killed per slot, for each side.
    pub attacker_losses: [i32; SLOTS],
    pub defender_losses: [i32; SLOTS],
    /// Experience for each hero of the winning side.
    pub experience: u32,
    pub log: Vec<BattleEvent>,
}

/// A stack during the battle.
struct Fighter {
    side: Side,
    slot: usize,
    /// Id for the dice: the game keys rolls by the creatures' ids in the battle.
    id: u32,
    stats: CreatureStats,
    count: i32,
    /// Hit points of the top creature (the others are unhurt).
    top_hp: i32,
    shots: i32,
    killed: i32,
}

impl Fighter {
    fn alive(&self) -> bool {
        self.count > 0
    }

    fn total_hit_points(&self) -> i32 {
        (self.count - 1) * self.stats.hit_points + self.top_hp
    }

    /// Take `damage` and return how many creatures died.
    fn take(&mut self, damage: i32) -> i32 {
        let hp = self.stats.hit_points.max(1);
        let left = self.total_hit_points() - damage;
        let before = self.count;
        if left <= 0 {
            self.count = 0;
            self.top_hp = 0;
        } else {
            // the remaining hit points fill whole creatures, the top one may be hurt
            self.count = (left + hp - 1) / hp;
            self.top_hp = left - (self.count - 1) * hp;
        }
        let died = before - self.count;
        self.killed += died;
        died
    }

    fn as_attacker(&self) -> Attacker {
        Attacker {
            id: self.id,
            number: self.count,
            damage_low: self.stats.damage_low,
            damage_high: self.stats.damage_high,
            offense: self.stats.attack as f32,
            defense_reduction: 1.0,
            school: school(self.stats.alignment),
            abilities: Vec::new(),
            giant_bonus_effect: false,
        }
    }

    fn as_defender(&self) -> Defender {
        Defender {
            id: self.id,
            defense_basic: self.stats.defense as f32,
            defense_bonus: 1.0,
            abilities: Vec::new(),
            warded_against: Vec::new(),
            giant: false,
            total_hit_points: self.total_hit_points(),
        }
    }
}

/// A creature's magic school from its town type (both lists are in the game's order).
fn school(town_type: usize) -> School {
    debug_assert_eq!(TOWN_TYPES.len(), 6);
    match town_type {
        0 => School::Life,
        1 => School::Order,
        2 => School::Death,
        3 => School::Chaos,
        4 => School::Nature,
        _ => School::Might,
    }
}

/// The ids the dice use: 1..=7 for the attacker's slots, 8..=14 for the defender's.
const fn dice_id(side: Side, slot: usize) -> u32 {
    match side {
        Side::Attacker => slot as u32 + 1,
        Side::Defender => slot as u32 + 1 + SLOTS as u32,
    }
}

/// Ids are taken modulo this by the dice; it has to be above every id. In the game it
/// comes from the battlefield (`CombatRandom::new`).
pub const DICE_ID_MODULUS: u32 = 2 * SLOTS as u32 + 1;

/// More rounds than any real fight needs; a guard against a bug looping forever.
const MAX_ROUNDS: u32 = 1000;

/// Fight `attacker` against `defender`. `dice` is the battle's generator; in the game its
/// seed is one number from the global generator, drawn when the battle starts.
pub fn fight(attacker: &Army, defender: &Army, creatures: &[CreatureStats], dice: &mut CombatRandom) -> BattleOutcome {
    let mut fighters = Vec::new();
    for (side, army) in [(Side::Attacker, attacker), (Side::Defender, defender)] {
        for (slot, s) in army.slots.iter().enumerate() {
            let Some(Slot::Creatures(stack)) = s else { continue };
            // a creature we have no stats for can't fight; the state test makes sure
            // every map creature has them
            let Some(stats) = creatures.get(stack.creature) else { continue };
            if stack.count <= 0 {
                continue;
            }
            fighters.push(Fighter {
                side,
                slot,
                id: dice_id(side, slot),
                stats: stats.clone(),
                count: stack.count,
                top_hp: stats.hit_points,
                shots: stats.shots,
                killed: 0,
            });
        }
    }
    // turn order: faster first, then the attacker, then lower slots (stable sort)
    fighters.sort_by_key(|f| (std::cmp::Reverse(f.stats.speed), f.side == Side::Defender, f.slot));

    let mut log = Vec::new();
    let side_alive = |fighters: &[Fighter], side: Side| fighters.iter().any(|f| f.side == side && f.alive());
    let mut rounds = 0;
    while side_alive(&fighters, Side::Attacker) && side_alive(&fighters, Side::Defender) {
        rounds += 1;
        if rounds > MAX_ROUNDS {
            log.push(BattleEvent::TooLong { rounds: MAX_ROUNDS });
            break;
        }
        for turn in 0..fighters.len() {
            if !fighters[turn].alive() {
                continue;
            }
            let enemy = fighters[turn].side.other();
            // the first living enemy stack by slot
            let target = (0..fighters.len())
                .filter(|&i| fighters[i].side == enemy && fighters[i].alive())
                .min_by_key(|&i| fighters[i].slot);
            let Some(target) = target else { break };
            let ranged = fighters[turn].shots > 0;
            if ranged {
                fighters[turn].shots -= 1;
            }
            hit(&mut fighters, turn, target, ranged, false, dice, &mut log);
            // a melee hit is struck back if the target lived through it
            if !ranged && fighters[target].alive() {
                hit(&mut fighters, target, turn, false, true, dice, &mut log);
            }
        }
    }

    let winner = if side_alive(&fighters, Side::Attacker) { Side::Attacker } else { Side::Defender };
    let mut armies = [attacker.clone(), defender.clone()];
    let mut losses = [[0; SLOTS]; 2];
    let mut experience: u32 = 0;
    for f in &fighters {
        let i = (f.side == Side::Defender) as usize;
        losses[i][f.slot] = f.killed;
        let survivors = match &armies[i].slots[f.slot] {
            Some(Slot::Creatures(s)) if f.count > 0 => Some(Slot::Creatures(h4_state::army::Stack { count: f.count, ..*s })),
            _ => None,
        };
        armies[i].slots[f.slot] = survivors;
        if f.side != winner {
            experience += (f.killed.max(0) as u32) * f.stats.experience.max(0) as u32;
        }
    }
    let [attacker, defender] = armies;
    BattleOutcome {
        winner,
        attacker,
        defender,
        attacker_losses: losses[0],
        defender_losses: losses[1],
        experience,
        log,
    }
}

/// What [`attack`] did to the game.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleReport {
    pub outcome: BattleOutcome,
    /// The army taken off the map: the loser.
    pub removed: ArmyId,
}

/// Army `attacker` attacks army `defender` on the map: fight, then write the result
/// back. The winner keeps its survivors and its heroes get the experience; the loser is
/// taken off the map with its heroes (what really happens to beaten heroes, and how
/// experience turns into levels, is still to be found in the exe).
///
/// The battle's seed is one number from the game's global generator, as in the game.
/// `None` if either army doesn't exist.
pub fn attack(
    state: &mut GameState,
    attacker: ArmyId,
    defender: ArmyId,
    creatures: &[CreatureStats],
    rng: &mut GlobalRandom,
) -> Option<BattleReport> {
    let a = state.army(attacker)?.army.clone();
    let d = state.army(defender)?.army.clone();
    let mut dice = CombatRandom::new(rng.next(), DICE_ID_MODULUS);
    let outcome = fight(&a, &d, creatures, &mut dice);
    let (winner, loser, survivors) = match outcome.winner {
        Side::Attacker => (attacker, defender, &outcome.attacker),
        Side::Defender => (defender, attacker, &outcome.defender),
    };
    let won = state.army_mut(winner).expect("looked up above");
    won.army = survivors.clone();
    for slot in won.army.slots.iter_mut().flatten() {
        if let Slot::Hero(h) = slot {
            h.experience += outcome.experience;
        }
    }
    state.remove_army(loser);
    Some(BattleReport { outcome, removed: loser })
}

/// Fighter `from` hits fighter `to`: the game's dice and damage formula, no luck.
fn hit(
    fighters: &mut [Fighter],
    from: usize,
    to: usize,
    ranged: bool,
    retaliation: bool,
    dice: &mut CombatRandom,
    log: &mut Vec<BattleEvent>,
) {
    let a = fighters[from].as_attacker();
    let d = fighters[to].as_defender();
    let mut steps: Vec<DamageEvent> = Vec::new();
    let damage = if ranged {
        let base = damage::roll_base_damage(&a, d.id, dice);
        let modified = damage::modify_damage(base, &a, &d, true, 0, Luck::Normal, LUCK_MULTIPLIERS[0], &mut steps);
        damage::finalize_damage(modified, &d, 0, &mut steps)
    } else {
        damage::melee_hit(&a, &d, 0, Luck::Normal, LUCK_MULTIPLIERS[0], dice, &mut steps)
    };
    let killed = fighters[to].take(damage);
    let (side, slot, target) = (fighters[from].side, fighters[from].slot, fighters[to].slot);
    log.push(BattleEvent::Hit { side, slot, target, ranged, retaliation, damage, killed });
}

#[cfg(test)]
mod tests {
    use super::*;
    use h4_state::army::{Hero, Stack};

    fn creature(hp: i32, low: i32, high: i32, attack: i32, defense: i32, speed: i32, shots: i32, xp: i32) -> CreatureStats {
        CreatureStats {
            keyword: String::new(),
            name: String::new(),
            level: 1,
            hit_points: hp,
            damage_low: low,
            damage_high: high,
            attack,
            defense,
            speed,
            movement: 20,
            shots,
            alignment: 0,
            experience: xp,
        }
    }

    #[test]
    fn wounds_carry_over_between_hits() {
        let stats = creature(10, 1, 1, 1, 1, 1, 0, 1);
        let mut f = Fighter { side: Side::Defender, slot: 0, id: 8, stats, count: 5, top_hp: 10, shots: 0, killed: 0 };
        assert_eq!(f.take(15), 1); // 35 hit points left: 4 creatures, the top one at 5
        assert_eq!((f.count, f.top_hp), (4, 5));
        assert_eq!(f.take(5), 1);
        assert_eq!((f.count, f.top_hp), (3, 10));
        assert_eq!(f.take(100), 3);
        assert!(!f.alive());
        assert_eq!(f.killed, 5);
    }

    #[test]
    fn a_strong_army_beats_a_weak_one_and_earns_its_experience() {
        // 0: a sturdy fighter, 1: a weak one worth 7 experience each
        let stats = vec![creature(30, 5, 8, 12, 12, 6, 0, 40), creature(5, 1, 2, 3, 3, 4, 0, 7)];
        let mut heroes = Army::of(&[Stack { creature: 0, count: 20 }]);
        heroes.slots[6] = Some(Slot::Hero(Hero {
            portrait: -1,
            name: String::new(),
            class: 0,
            male: true,
            level: 1,
            experience: 0,
        }));
        let monsters = Army::of(&[Stack { creature: 1, count: 30 }]);
        let mut dice = CombatRandom::new(1234, DICE_ID_MODULUS);
        let out = fight(&heroes, &monsters, &stats, &mut dice);
        assert_eq!(out.winner, Side::Attacker);
        assert!(out.defender.is_empty());
        assert_eq!(out.defender_losses[0], 30);
        assert_eq!(out.experience, 30 * 7);
        // the hero is still there, and the survivors are fewer or as many as before
        assert_eq!(out.attacker.heroes().count(), 1);
        let left = out.attacker.stacks().next().map_or(0, |s| s.count);
        assert_eq!(left, 20 - out.attacker_losses[0]);
        // the same seed gives the same battle
        let again = fight(&heroes, &monsters, &stats, &mut CombatRandom::new(1234, DICE_ID_MODULUS));
        assert_eq!(again, out);
    }

    #[test]
    fn shooters_are_not_struck_back() {
        let stats = vec![creature(10, 2, 3, 10, 10, 9, 12, 5), creature(10, 2, 3, 10, 10, 1, 0, 5)];
        let archers = Army::of(&[Stack { creature: 0, count: 10 }]);
        let target = Army::of(&[Stack { creature: 1, count: 10 }]);
        let out = fight(&archers, &target, &stats, &mut CombatRandom::new(7, DICE_ID_MODULUS));
        let first = &out.log[0];
        assert!(matches!(first, BattleEvent::Hit { side: Side::Attacker, ranged: true, .. }));
        // the archers shoot first (faster) and nothing strikes back right after
        assert!(!matches!(out.log.get(1), Some(BattleEvent::Hit { retaliation: true, .. })));
    }

    /// The vertical slice in one go: a hero's army walks to monsters, attacks, wins, and
    /// the map shows it.
    #[test]
    fn walk_attack_and_write_back() {
        use crate::movement::{self, MoveEvent};
        use h4_state::map::AdventureMap;
        use h4_state::{GameState, Pos};

        let stats = vec![creature(30, 5, 8, 12, 12, 6, 0, 40), creature(5, 1, 2, 3, 3, 4, 0, 7)];
        let mut g = GameState::new(AdventureMap::new(12, 1), Vec::new());
        let mut army = Army::of(&[Stack { creature: 0, count: 20 }]);
        army.slots[6] = Some(Slot::Hero(Hero { portrait: -1, name: String::new(), class: 0, male: true, level: 1, experience: 0 }));
        let hero = g.add_army(Some(0), Pos { level: 0, row: 2, col: 2 }, army);
        let lair = Pos { level: 0, row: 2, col: 7 };
        let monsters = g.add_army(None, lair, Army::of(&[Stack { creature: 1, count: 30 }]));
        movement::refresh_movement(g.army_mut(hero).unwrap(), &stats);

        let path = movement::find_path(&g, hero, lair, &stats).unwrap();
        let events = movement::move_army(&mut g, hero, &path, &stats).unwrap();
        assert_eq!(events.last(), Some(&MoveEvent::Attack { defender: monsters }));
        let report = attack(&mut g, hero, monsters, &stats, &mut GlobalRandom::new(5)).unwrap();

        assert_eq!(report.removed, monsters);
        assert!(g.army(monsters).is_none());
        let after = g.army(hero).unwrap();
        assert_eq!(after.army.heroes().next().unwrap().experience, 30 * 7);
        // now the way is free: the army can step onto the lair
        let events = movement::move_army(&mut g, hero, &[lair], &stats).unwrap();
        assert_eq!(events, [MoveEvent::Stepped { to: lair, cost: 100 }]);
    }

    #[test]
    fn an_army_without_creatures_loses() {
        let stats = vec![creature(10, 1, 2, 5, 5, 5, 0, 5)];
        let mut lonely = Army::default();
        lonely.slots[0] = Some(Slot::Hero(Hero {
            portrait: -1,
            name: String::new(),
            class: 0,
            male: false,
            level: 1,
            experience: 0,
        }));
        let monsters = Army::of(&[Stack { creature: 0, count: 3 }]);
        let out = fight(&lonely, &monsters, &stats, &mut CombatRandom::new(1, DICE_ID_MODULUS));
        assert_eq!(out.winner, Side::Defender);
        assert_eq!(out.experience, 0);
        assert!(out.log.is_empty());
    }
}
