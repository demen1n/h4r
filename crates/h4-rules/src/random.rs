//! The game's random numbers, reproduced exactly (see `decomp/random.md`).
//!
//! Both generators below use the same formula as Microsoft C's `rand()`: a linear
//! congruential generator `state = state * 214013 + 2531011` (wrapping at 32 bits) whose
//! output is bits 16..30 of the new state, a number in 0..=32767. The game doesn't call the
//! C library for this; it has its own copies of the formula, so we need our own too.
//!
//! - [`GlobalRandom`] is the game's one global generator (`t_random_number_generator`):
//!   adventure map events, map generation, and the seed of each battle.
//! - [`CombatRandom`] is what battles use. Instead of "the next number", every roll asks for
//!   the number at a position computed from *who* rolls and *why*, so two computers (or a
//!   replay) get the same result no matter in which order the rolls happen.
//!
//! Rules never use a hidden global: whoever runs a battle owns its `CombatRandom` and passes
//! it in, so a test can fix the seed and get the same fight every time.

use std::collections::HashMap;

/// One step of the generator: advance `state` and return the new state.
fn step(state: u32) -> u32 {
    state.wrapping_mul(214_013).wrapping_add(2_531_011)
}

/// The 15-bit number the game takes from a state.
fn output(state: u32) -> u32 {
    (state >> 16) & 0x7fff
}

/// The global generator (`t_random_number_generator`, one instance at VA `0xac2018`).
/// At start-up the game seeds it with the current time (`time(NULL)`); loading a saved
/// game sets it again, apparently from the save.
#[derive(Debug, Clone)]
pub struct GlobalRandom {
    pub state: u32,
}

impl GlobalRandom {
    pub fn new(seed: u32) -> GlobalRandom {
        GlobalRandom { state: seed }
    }

    /// The next number, 0..=32767 (the game's `operator()()`).
    pub fn next(&mut self) -> u32 {
        self.state = step(self.state);
        output(self.state)
    }
}

/// How many states apart the remembered checkpoints are (the game uses the same spacing).
const CHECKPOINT_STEP: u32 = 128;

/// The battle generator (an object at `t_battlefield + 0x380`, read function at rva `203280`).
///
/// Think of the generator's output as one endless, fixed list of numbers determined by the
/// seed. A roll doesn't take "the next" number from that list; it computes an index from its
/// arguments and returns the number at that index. The arguments are two ids (usually the
/// creatures involved), a `purpose` code that says what the roll is for (0 is the damage
/// roll, 0x1b defensive luck against spells, 0x1c morale; the exe uses codes 0..=43) and a
/// counter of how many times this same (ids, purpose) combination has rolled before.
#[derive(Debug, Clone)]
pub struct CombatRandom {
    seed: u32,
    /// Ids are reduced modulo this for purposes below 30. In the game it's a field of the
    /// battlefield minus one (`[+0x438] - 1`, meaning not yet known); purposes 30 and up use 200.
    id_modulus: u32,
    /// How many times each (second id, purpose, first id) has rolled.
    counters: HashMap<(u32, u32, u32), u32>,
    /// `checkpoints[k]` is the state after `(k + 1) * 128` steps, so a roll at a large index
    /// doesn't have to step all the way from the seed (the game keeps the same cache).
    checkpoints: Vec<u32>,
}

impl CombatRandom {
    /// A fresh generator. In the game the seed is one number from the global generator,
    /// drawn when the battle starts (`GlobalRandom::next`, so 0..=32767).
    pub fn new(seed: u32, id_modulus: u32) -> CombatRandom {
        // The game would divide by zero here; we'd rather refuse early.
        assert!(id_modulus > 0, "id_modulus must be at least 1");
        CombatRandom { seed, id_modulus, counters: HashMap::new(), checkpoints: Vec::new() }
    }

    /// A number in 0..=32767 for this (first, second, purpose) roll.
    pub fn roll(&mut self, first: u32, second: u32, purpose: u32) -> u32 {
        let m = if purpose < 30 { self.id_modulus } else { 200 };
        let a = first % m;
        let b = second % m;
        let count = self.counters.entry((b, purpose, a)).or_insert(0);
        *count += 1;
        // 44 is the number of purpose codes, so different purposes never share an index.
        // The game computes this in 32-bit integers, hence the wrapping arithmetic.
        let index = purpose
            .wrapping_add(count.wrapping_mul(44))
            .wrapping_mul(m)
            .wrapping_add(b)
            .wrapping_mul(m)
            .wrapping_add(a);
        output(step(self.state_after(index)))
    }

    /// The generator state after `n` steps from the seed.
    fn state_after(&mut self, n: u32) -> u32 {
        let blocks = n / CHECKPOINT_STEP;
        // Make sure the checkpoints reach block `blocks` (they are built in order).
        while (self.checkpoints.len() as u32) < blocks {
            let mut state = self.checkpoints.last().copied().unwrap_or(self.seed);
            for _ in 0..CHECKPOINT_STEP {
                state = step(state);
            }
            self.checkpoints.push(state);
        }
        let mut state = if blocks == 0 { self.seed } else { self.checkpoints[blocks as usize - 1] };
        for _ in 0..n % CHECKPOINT_STEP {
            state = step(state);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_matches_msvc_rand() {
        // Microsoft C's rand() after srand(1) — a well-known sequence; the game's generator
        // is the same formula, so it must produce it too.
        let mut r = GlobalRandom::new(1);
        let got: Vec<u32> = (0..5).map(|_| r.next()).collect();
        assert_eq!(got, [41, 18467, 6334, 26500, 19169]);
    }

    /// The n-th output (0-based) of the plain sequence, the slow and obvious way.
    fn nth_output(seed: u32, n: u32) -> u32 {
        let mut r = GlobalRandom::new(seed);
        let mut value = 0;
        for _ in 0..=n {
            value = r.next();
        }
        value
    }

    #[test]
    fn combat_roll_picks_the_indexed_number() {
        // id_modulus 10, purpose 0, ids 3 and 7, first roll (count 1):
        // index = ((0 + 44 * 1) * 10 + 7) * 10 + 3 = 4473
        let mut r = CombatRandom::new(12345, 10);
        assert_eq!(r.roll(3, 7, 0), nth_output(12345, 4473));
        // the same roll again uses count 2: ((0 + 88) * 10 + 7) * 10 + 3 = 8873
        assert_eq!(r.roll(3, 7, 0), nth_output(12345, 8873));
        // ids are taken modulo 10, so 13 and 17 continue the same counter: count 3
        assert_eq!(r.roll(13, 17, 0), nth_output(12345, 13273));
    }

    #[test]
    fn purposes_from_30_use_modulus_200() {
        let mut r = CombatRandom::new(7, 10);
        // ((30 + 44) * 200 + 15) * 200 + 250 % 200 = 2_963_050
        assert_eq!(r.roll(250, 15, 30), nth_output(7, 2_963_050));
    }

    #[test]
    fn rolls_depend_on_their_keys_not_on_order() {
        // the point of the scheme: interleaving other rolls changes nothing
        let mut a = CombatRandom::new(99, 20);
        let mut b = CombatRandom::new(99, 20);
        let first = a.roll(1, 2, 0);
        b.roll(5, 6, 0x1c);
        b.roll(4, 4, 3);
        assert_eq!(b.roll(1, 2, 0), first);
    }

    #[test]
    fn small_indexes_start_from_the_seed() {
        // index below 128 doesn't touch the checkpoints: purpose 0, count 1, modulus 1 -> 44
        let mut r = CombatRandom::new(1, 1);
        assert_eq!(r.roll(0, 0, 0), nth_output(1, 44));
    }
}
