//! Readers for Heroes of Might and Magic IV data files.
//!
//! - [`archive`]: `.h4r` resource archives and the game's archive priority order.
//! - [`sprite`]: paletted sprite layers (`t_bitmap_layer_24`), bitmap groups and actor sequences.
//! - [`catalog`]: actors and their animations by action and direction.
//! - [`png`]: a minimal RGBA PNG writer for dumping decoded images.

pub mod archive;
pub mod catalog;
pub mod png;
pub mod sprite;

mod reader;

pub use reader::{Error, Result};
