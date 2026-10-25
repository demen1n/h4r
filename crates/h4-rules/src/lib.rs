//! Rules of Heroes of Might and Magic IV, separate from graphics and input.
//!
//! Nothing here draws or reads the mouse: functions take the state they need and return
//! results (plus a log of what happened), so they can be checked by tests and compared
//! with the original game. Randomness is always passed in explicitly.
//!
//! - [`random`]: the game's two random number generators, bit-exact.
//! - [`damage`]: damage of one stack hitting another.
//! - [`creatures`]: creature statistics from `table.creatures`.
//! - [`heroes`]: hero classes and hero models on the map.
//! - [`placement`]: what the random objects of a map become when a game starts.

pub mod creatures;
pub mod damage;
pub mod heroes;
pub mod placement;
pub mod random;
