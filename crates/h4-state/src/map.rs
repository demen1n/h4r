//! The ground of the adventure map and the objects standing on it.

use crate::{Color, Pos};

/// Terrain ids of the game's enum (`h4_formats::map::TERRAIN_NAMES`) that the rules need
/// to tell apart. The others are only looked at by the drawing code.
pub mod terrain {
    pub const WATER: u8 = 0;
}

/// One tile. Only what the rules need is here; the picture (subtypes, transitions) is
/// drawn straight from the map file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Terrain id, see [`terrain`].
    pub terrain: u8,
    /// 0 = no road, else 1 gravel, 2 dirt, 3 cobblestone.
    pub road: u8,
    /// An object stands here that nobody can walk through (a tree, a mountain, the body of
    /// a building).
    pub blocked: bool,
    /// A bridge covers the tile: it can be crossed although it is water.
    pub bridge: bool,
    /// Part of the map. A map is a diamond inside a square (`h4_formats::map::row_ranges`),
    /// the corners outside the diamond don't exist in the game.
    pub inside: bool,
}

impl Tile {
    /// Plain grass, nothing on it.
    pub const EMPTY: Tile = Tile { terrain: 1, road: 0, blocked: false, bridge: false, inside: true };
}

/// All levels of the map, `size` × `size` tiles each.
#[derive(Debug, Clone, PartialEq)]
pub struct AdventureMap {
    pub size: i32,
    pub levels: u8,
    /// Level by level, then row by row.
    tiles: Vec<Tile>,
}

impl AdventureMap {
    /// A map of empty grass with no corners cut off, for tests and generated maps.
    pub fn new(size: i32, levels: u8) -> AdventureMap {
        let n = (size * size) as usize * levels as usize;
        AdventureMap { size, levels, tiles: vec![Tile::EMPTY; n] }
    }

    /// Whether `at` is on the square of the map (it may still be outside the diamond).
    pub fn contains(&self, at: Pos) -> bool {
        at.level < self.levels && (0..self.size).contains(&at.row) && (0..self.size).contains(&at.col)
    }

    /// The tile at `at`, or `None` off the square.
    pub fn tile(&self, at: Pos) -> Option<&Tile> {
        self.index(at).map(|i| &self.tiles[i])
    }

    pub fn tile_mut(&mut self, at: Pos) -> Option<&mut Tile> {
        self.index(at).map(move |i| &mut self.tiles[i])
    }

    fn index(&self, at: Pos) -> Option<usize> {
        if !self.contains(at) {
            return None;
        }
        let per_level = (self.size * self.size) as usize;
        Some(at.level as usize * per_level + (at.row * self.size + at.col) as usize)
    }
}

/// An object that stays where it was put: a town, a mine, a chest, a tree...
#[derive(Debug, Clone, PartialEq)]
pub struct MapObject {
    /// Model name, the resource is `adv_object.<model>`. For a random object, the model
    /// of what it became.
    pub model: String,
    /// Object type as the map stores it (`town`, `mine`, `decorative`...).
    pub type_name: String,
    /// The tile of the footprint's first cell.
    pub at: Pos,
    pub owner: Option<Color>,
    /// Index of the object in the map file, to find what else the file says about it
    /// (scripts, garrisons) until those are part of the state.
    pub map_index: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_by_position() {
        let mut map = AdventureMap::new(4, 2);
        let under = Pos { level: 1, row: 3, col: 2 };
        map.tile_mut(under).expect("on the map").blocked = true;
        assert!(map.tile(under).expect("on the map").blocked);
        // the same tile on the surface is a different one
        assert!(!map.tile(Pos { level: 0, ..under }).expect("on the map").blocked);
        assert!(map.tile(Pos { level: 2, row: 0, col: 0 }).is_none());
        assert!(map.tile(Pos { level: 0, row: 4, col: 0 }).is_none());
        assert!(map.tile(Pos { level: 0, row: 0, col: -1 }).is_none());
    }
}
