//! The hand-made test map: terrain per tile, roads, and which tiles objects take. No
//! drawing here. Walking on it goes by the rules' state, made with [`World::ground`].

use std::collections::VecDeque;

use h4_state::map::AdventureMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Terrain {
    Grass,
    Dirt,
    Sand,
    Snow,
    Swamp,
    Lava,
    Rough,
    // underground maps only; the test map has no underground level yet
    #[allow(dead_code)]
    Subterranean,
    Water,
}

impl Terrain {
    /// Which terrain is drawn over which at their border: higher wins. This is the order of
    /// the game's terrain enum (water, grass, rough, swamp, volcanic, snow, sand, dirt,
    /// subterranean), which is also the order its transition calculator evaluates them in.
    pub fn priority(self) -> u8 {
        match self {
            Terrain::Water => 0,
            Terrain::Grass => 1,
            Terrain::Rough => 2,
            Terrain::Swamp => 3,
            Terrain::Lava => 4,
            Terrain::Snow => 5,
            Terrain::Sand => 6,
            Terrain::Dirt => 7,
            Terrain::Subterranean => 8,
        }
    }

    /// Id in the game's terrain enum (`h4_formats::map::TERRAIN_NAMES`), as the rules use it.
    pub fn game_id(self) -> u8 {
        match self {
            Terrain::Water => 0,
            Terrain::Grass => 1,
            Terrain::Rough => 2,
            Terrain::Swamp => 3,
            Terrain::Lava => 4, // "volcanic"
            Terrain::Snow => 5,
            Terrain::Sand => 6,
            Terrain::Dirt => 7,
            Terrain::Subterranean => 8,
        }
    }

    /// Name used in `terrain.<name>.<set>.<n>.h4d`.
    pub fn resource_name(self) -> &'static str {
        match self {
            Terrain::Grass => "grass",
            Terrain::Dirt => "dirt",
            Terrain::Sand => "sand",
            Terrain::Snow => "snow",
            Terrain::Swamp => "swamp",
            Terrain::Lava => "lava",
            Terrain::Rough => "rough",
            Terrain::Subterranean => "subterranean",
            Terrain::Water => "water",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Road {
    Dirt,
    Gravel,
    Cobblestone,
}

impl Road {
    /// Name used in `terrain.road.<name>.<n>.h4d`.
    pub fn resource_name(self) -> &'static str {
        match self {
            Road::Dirt => "dirt",
            Road::Gravel => "gravel",
            Road::Cobblestone => "cobblestone",
        }
    }

    /// Road type as stored in maps: 1 gravel, 2 dirt, 3 cobblestone (the order of the exe's
    /// road texture array at rva `58ca68`); 0 is no road.
    pub fn from_map(road: u8) -> Option<Road> {
        match road {
            1 => Some(Road::Gravel),
            2 => Some(Road::Dirt),
            3 => Some(Road::Cobblestone),
            _ => None,
        }
    }

    /// The other way round: the number maps and the rules use.
    pub fn game_id(self) -> u8 {
        match self {
            Road::Gravel => 1,
            Road::Dirt => 2,
            Road::Cobblestone => 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub terrain: Terrain,
    /// Which of the terrain's two texture sets (`.1.x` or `.2.x`): they differ in look.
    pub set: u8,
    pub road: Option<Road>,
}

/// A map position: column grows to the screen's lower right, row to the lower left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub col: i32,
    pub row: i32,
}

pub struct World {
    pub width: i32,
    pub height: i32,
    tiles: Vec<Tile>,
    /// Tiles under an object's footprint, passable or not: no other object may go there.
    taken: Vec<bool>,
    /// Tiles nobody can step on because an object stands there.
    blocked: Vec<bool>,
}

impl World {
    pub fn contains(&self, p: Pos) -> bool {
        p.col >= 0 && p.row >= 0 && p.col < self.width && p.row < self.height
    }

    pub fn tile(&self, p: Pos) -> &Tile {
        &self.tiles[self.index(p)]
    }

    fn tile_mut(&mut self, p: Pos) -> &mut Tile {
        let i = self.index(p);
        &mut self.tiles[i]
    }

    fn index(&self, p: Pos) -> usize {
        (p.row * self.width + p.col) as usize
    }

    pub fn passable(&self, p: Pos) -> bool {
        if !self.contains(p) {
            return false;
        }
        let i = self.index(p);
        self.tile(p).terrain != Terrain::Water && !self.blocked[i]
    }

    pub fn is_taken(&self, p: Pos) -> bool {
        self.contains(p) && self.taken[self.index(p)]
    }

    /// Mark a tile as part of an object's footprint; `blocked` if heroes can't enter it.
    pub fn take(&mut self, p: Pos, blocked: bool) {
        let i = self.index(p);
        self.taken[i] = true;
        self.blocked[i] |= blocked;
    }

    /// The ground as the rules see it (`h4_state`), with the objects' blocked cells (the test
    /// map has no bridges). The test map is square, as every map of the game is.
    pub fn ground(&self) -> AdventureMap {
        assert_eq!(self.width, self.height, "maps are square");
        let mut ground = AdventureMap::new(self.width, 1);
        for row in 0..self.height {
            for col in 0..self.width {
                let p = Pos { col, row };
                let i = self.index(p);
                let t = self.tile(p);
                let tile = ground.tile_mut(h4_state::Pos { level: 0, row, col }).expect("same size as the world");
                tile.terrain = t.terrain.game_id();
                tile.road = t.road.map_or(0, Road::game_id);
                tile.blocked = self.blocked[i];
            }
        }
        ground
    }

    /// A hand-made test map: grass with patches of other terrain, a lake and a road.
    pub fn generate(width: i32, height: i32) -> World {
        let blank = Tile { terrain: Terrain::Grass, set: 0, road: None };
        let n = (width * height) as usize;
        let mut w = World { width, height, tiles: vec![blank; n], taken: vec![false; n], blocked: vec![false; n] };
        for row in 0..height {
            for col in 0..width {
                let p = Pos { col, row };
                let (x, y) = (col as f32, row as f32);
                let t = w.tile_mut(p);
                // smooth noise picks which grass texture set a tile uses
                t.set = (noise(x * 0.15, y * 0.15) > 0.5) as u8;
                let terrain = if blob(x, y, 26.0, 12.0, 7.0, 4.5) {
                    Terrain::Water
                } else if blob(x, y, 26.0, 12.0, 8.5, 6.0) {
                    Terrain::Sand
                } else if x + y < 12.0 + 4.0 * noise(x * 0.3, y * 0.3) {
                    Terrain::Snow
                } else if blob(x, y, 9.0, 30.0, 6.0, 5.0) {
                    Terrain::Swamp
                } else if blob(x, y, 33.0, 32.0, 5.0, 6.0) {
                    Terrain::Rough
                } else if blob(x, y, 14.0, 16.0, 3.0, 3.0) {
                    Terrain::Dirt
                } else if blob(x, y, 36.0, 17.0, 3.0, 4.0) {
                    Terrain::Lava
                } else {
                    Terrain::Grass
                };
                t.terrain = terrain;
            }
        }
        // a road from the lower left towards the upper right, around the lake,
        // changing from cobblestone to dirt to gravel
        let route = [
            (Pos { col: 3, row: 36 }, Road::Cobblestone),
            (Pos { col: 16, row: 24 }, Road::Dirt),
            (Pos { col: 20, row: 4 }, Road::Gravel),
            (Pos { col: 37, row: 3 }, Road::Gravel),
        ];
        for leg in route.windows(2) {
            let ((from, road), (to, _)) = (leg[0], leg[1]);
            if let Some(path) = w.find_path_with(from, to, &NEIGHBOURS[..4]) {
                for p in std::iter::once(from).chain(path) {
                    w.tile_mut(p).road = Some(road);
                }
            }
        }
        w
    }

    /// Shortest path from `from` to `to` over passable tiles, not including `from`, with
    /// `steps` allowed. Breadth-first search: every step costs the same. Only used to lay
    /// roads on the test map, with the four straight steps, so their tiles touch along edges
    /// instead of at corners (walking uses `h4_rules::movement::find_path`).
    fn find_path_with(&self, from: Pos, to: Pos, steps: &[(i32, i32)]) -> Option<Vec<Pos>> {
        if !self.passable(to) {
            return None;
        }
        if from == to {
            return Some(Vec::new());
        }
        let index = |p: Pos| self.index(p);
        let mut came_from: Vec<Option<Pos>> = vec![None; self.tiles.len()];
        let mut queue = VecDeque::from([from]);
        came_from[index(from)] = Some(from);
        while let Some(p) = queue.pop_front() {
            if p == to {
                let mut path = vec![p];
                let mut cur = p;
                while let Some(prev) = came_from[index(cur)] {
                    if prev == from {
                        break;
                    }
                    path.push(prev);
                    cur = prev;
                }
                path.reverse();
                return Some(path);
            }
            for &(dc, dr) in steps {
                let n = Pos { col: p.col + dc, row: p.row + dr };
                if self.passable(n) && came_from[index(n)].is_none() {
                    came_from[index(n)] = Some(p);
                    queue.push_back(n);
                }
            }
        }
        None
    }
}

/// Straight steps first, then diagonals.
const NEIGHBOURS: [(i32, i32); 8] = [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)];

/// Inside a wobbly ellipse centred at (cx, cy) with radii (rx, ry)?
fn blob(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> bool {
    let d = ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2);
    d < 0.8 + 0.4 * noise(x * 0.4, y * 0.4)
}

/// Cheap smooth value noise in 0..1, deterministic.
fn noise(x: f32, y: f32) -> f32 {
    let hash = |ix: i32, iy: i32| {
        let mut h = (ix as u32).wrapping_mul(374761393) ^ (iy as u32).wrapping_mul(668265263);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        (h ^ (h >> 16)) as f32 / u32::MAX as f32
    };
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let top = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * s(fx);
    let bottom = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * s(fx);
    top + (bottom - top) * s(fy)
}
