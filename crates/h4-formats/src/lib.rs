//! Readers for Heroes of Might and Magic IV data files.
//!
//! - [`adv_actor`]: adventure map animation definitions (speeds, walk lengths).
//! - [`archive`]: `.h4r` resource archives and the game's archive priority order.
//! - [`sprite`]: paletted sprite layers (`t_bitmap_layer_24`), bitmap groups and actor sequences.
//! - [`catalog`]: actors and their animations by action and direction.
//! - [`terrain`]: terrain metatiles; [`transition`]: masks for edges between terrains.
//! - [`map`]: adventure maps (`.h4c`): header and tiles; [`map_body`]: events and objects in
//!   between ([`map_script`]: scripts and events, [`map_army`]: armies and heroes);
//!   [`cp1251`]: Russian text.
//! - [`png`]: a minimal RGBA PNG writer for dumping decoded images.

pub mod adv_actor;
pub mod adv_object;
pub mod archive;
pub mod catalog;
pub mod cp1251;
pub mod map;
pub mod map_army;
pub mod map_body;
pub mod map_script;
pub mod png;
pub mod sprite;
pub mod terrain;
pub mod transition;

mod reader;

pub use reader::{Error, Result};
