//! Rules of Heroes of Might and Magic IV, separate from graphics and input.
//!
//! Nothing here draws or reads the mouse: functions take the state they need and return
//! results (plus a log of what happened), so they can be checked by tests and compared
//! with the original game. Randomness is always passed in explicitly.
//!
//! - [`random`]: the game's two random number generators, bit-exact.
//! - [`damage`]: damage of one stack hitting another.
//! - [`battle`]: a stand-in battle without a battlefield, and writing its result back.
//! - [`artifacts`]: artifact numbering, models and random picks.
//! - [`creatures`]: creature statistics from `table.creatures`.
//! - [`heroes`]: hero classes and hero models on the map.
//! - [`placement`]: what the random objects of a map become when a game starts.
//! - [`new_game`]: the [`h4_state::GameState`] of a map on day 1.
//! - [`movement`]: armies walking on the map: step costs, movement points, paths.
//! - [`turns`]: ending a turn and what a new day brings.

pub mod artifacts;
pub mod battle;
pub mod creatures;
pub mod damage;
pub mod heroes;
pub mod movement;
pub mod new_game;
pub mod placement;
pub mod random;
pub mod effects;
pub mod turns;
