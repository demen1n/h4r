//! Turns and days: whose turn it is, and what a new day brings.
//!
//! Players move one after another; when the last one ends their turn, a new day starts
//! for everybody. From the exe (`decomp/turns.md`):
//! - every member of every army gets its full movement back (`t_creature_stack`'s
//!   `process_new_day`, rva `250ca0`);
//! - neutral monsters on the map grow a little every day (`t_creature::process_new_day`,
//!   rva `23de10`).
//!
//! Not modelled yet: the neutral turn between players (monsters don't move anyway), weeks
//! and months (dwellings, the "week of ..." creature), income, towns, timed bonuses
//! running out, and the order in which the game visits objects on a new day.

use h4_state::army::{Slot, SLOTS};
use h4_state::GameState;

use crate::creatures::CreatureStats;
use crate::movement;

/// What a call to [`end_turn`] did, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    /// The player at this index in `GameState::players` moves now.
    TurnOf { player: usize },
    /// A new day started (day numbers start at 1).
    NewDay { day: u32 },
    /// A neutral stack grew by `by` creatures.
    Grew { army: h4_state::army::ArmyId, slot: usize, by: i32 },
}

/// Get a game made by `new_game::from_map` ready for day 1: every army gets its
/// movement. (In the game a fresh stack starts with its movement already set.)
pub fn begin_game(state: &mut GameState, creatures: &[CreatureStats]) {
    for army in &mut state.armies {
        movement::refresh_movement(army, creatures);
    }
}

/// The current player ends their turn. The next player still in the game moves next; if
/// there is none after them, a new day begins and the first player still in the game
/// moves.
pub fn end_turn(state: &mut GameState, creatures: &[CreatureStats]) -> Vec<TurnEvent> {
    let mut events = Vec::new();
    let next = (state.current_player + 1..state.players.len()).find(|&i| state.players[i].alive);
    let player = match next {
        Some(p) => p,
        None => {
            new_day(state, creatures, &mut events);
            // with nobody left alive the turn stays where it is
            (0..state.players.len()).find(|&i| state.players[i].alive).unwrap_or(state.current_player)
        }
    };
    state.current_player = player;
    events.push(TurnEvent::TurnOf { player });
    events
}

/// Everything that happens when a day begins.
fn new_day(state: &mut GameState, creatures: &[CreatureStats], events: &mut Vec<TurnEvent>) {
    state.day += 1;
    events.push(TurnEvent::NewDay { day: state.day });
    for army in &mut state.armies {
        // monsters belonging to nobody grow; the game checks the stack has no owner
        if army.owner.is_none() {
            for slot in 0..SLOTS {
                let Some(Slot::Creatures(stack)) = &mut army.army.slots[slot] else { continue };
                let Some(stats) = creatures.get(stack.creature) else { continue };
                let (by, remainder) = daily_growth(stack.count, army.growth_remainder[slot], stats.weekly_growth);
                stack.count += by;
                army.growth_remainder[slot] = remainder;
                if by > 0 {
                    events.push(TurnEvent::Grew { army: army.id, slot, by });
                }
            }
        }
        movement::refresh_movement(army, creatures);
    }
}

/// One day's growth of a neutral stack of `count` (rva `23de10`): a tenth of the stack,
/// but at most the creature's weekly growth, spread over 7 days. The game counts in
/// 1/256 of a creature and keeps what doesn't add up to a whole one in `remainder`.
/// Returns the creatures added and the new remainder.
pub fn daily_growth(count: i32, remainder: i32, weekly_growth: i32) -> (i32, i32) {
    let weekly = (count * 256 / 10).min(weekly_growth * 256);
    let today = weekly / 7 + remainder;
    (today >> 8, today & 0xff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use h4_state::army::{Army, Stack};
    use h4_state::map::AdventureMap;
    use h4_state::{Player, Pos};

    fn creature(weekly_growth: i32) -> CreatureStats {
        CreatureStats {
            keyword: String::new(),
            name: String::new(),
            level: 1,
            hit_points: 10,
            damage_low: 1,
            damage_high: 2,
            attack: 5,
            defense: 5,
            speed: 5,
            movement: 20,
            shots: 0,
            weekly_growth,
            alignment: 0,
            experience: 5,
        }
    }

    #[test]
    fn growth_of_a_tenth_capped_by_the_weekly_growth() {
        // 70 creatures: a tenth is 7 a week, 1 a day exactly
        assert_eq!(daily_growth(70, 0, 20), (1, 0));
        // 1000 creatures, weekly growth 14: capped at 14 a week, 2 a day
        assert_eq!(daily_growth(1000, 0, 14), (2, 0));
        // 10 creatures: 1 a week, so 36/256 a day; the seventh day completes it
        let mut remainder = 0;
        let mut grown = 0;
        for _ in 0..7 {
            let (by, r) = daily_growth(10, remainder, 20);
            grown += by;
            remainder = r;
        }
        // 256 / 7 = 36 a day, 7 * 36 = 252: not quite one creature after a week
        assert_eq!((grown, remainder), (0, 252));
        assert_eq!(daily_growth(10, remainder, 20).0, 1);
    }

    fn player(color: u8) -> Player {
        Player { color, human: true, resources: [0; 7], alive: true }
    }

    #[test]
    fn turns_pass_and_days_begin() {
        let stats = vec![creature(20)];
        let mut g = GameState::new(AdventureMap::new(8, 1), vec![player(0), player(1), player(2)]);
        g.players[1].alive = false;
        let mine = g.add_army(Some(0), Pos { level: 0, row: 1, col: 1 }, Army::of(&[Stack { creature: 0, count: 5 }]));
        let wild = g.add_army(None, Pos { level: 0, row: 6, col: 6 }, Army::of(&[Stack { creature: 0, count: 70 }]));
        begin_game(&mut g, &stats);
        let full = movement::movement_left(g.army(mine).unwrap(), &stats);
        assert!(full > 0);
        movement::spend_movement(g.army_mut(mine).unwrap(), &stats, 500);

        // player 1 is out: from 0 straight to 2, same day
        assert_eq!(end_turn(&mut g, &stats), [TurnEvent::TurnOf { player: 2 }]);
        assert_eq!(g.day, 1);
        // after the last player a new day: movement back, monsters grew
        let events = end_turn(&mut g, &stats);
        assert_eq!(
            events,
            [
                TurnEvent::NewDay { day: 2 },
                TurnEvent::Grew { army: wild, slot: 0, by: 1 },
                TurnEvent::TurnOf { player: 0 },
            ]
        );
        assert_eq!(movement::movement_left(g.army(mine).unwrap(), &stats), full);
        // our own creatures don't grow
        assert_eq!(g.army(mine).unwrap().army.stacks().next().unwrap().count, 5);
        assert_eq!(g.army(wild).unwrap().army.stacks().next().unwrap().count, 71);
    }
}
