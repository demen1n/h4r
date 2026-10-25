//! Which metatile pictures make up each tile of a real map, as the game lays them out
//! (`t_metatile_map`, rva `395400`).
//!
//! A metatile is a 10x10 tile picture. The map is cut into blocks of 4x4 tiles and every
//! block gets, from a generator seeded with a number stored in the map header:
//!
//! - a rank (the blocks are shuffled),
//! - for each terrain type and subtype, which of its metatiles it shows,
//! - the same for each road type,
//! - a "reach" of 0..=2 tiles: how far its metatile sticks out over the next blocks.
//!
//! A block's metatile starts `4 - reach` tiles before the block. A tile is painted by its
//! own block and by every neighbouring block of higher rank that reaches it; the highest of
//! those whose picture fully covers the tile (its "window") hides the ones below it.

use h4_formats::map::{row_ranges, MapFile};

const BLOCK: i32 = 4;
/// Terrain types (`h4_formats::map::TERRAIN_NAMES`).
const TERRAINS: usize = 19;
/// Subtypes per terrain type (VA `0x9709d8`).
const SUBTYPES: [usize; TERRAINS] = [2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
/// Metatiles per terrain type and subtype (rva `39ae00`): two, except the magic terrains
/// drawn from one picture (all, life, order).
const PICTURES: [u32; TERRAINS] = [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2];

/// One block's choices.
pub struct Block {
    rank: u32,
    /// Metatile (0 or 1) per terrain type and subtype.
    terrain: [[u8; 2]; TERRAINS],
    /// Metatile per road type 1..=3.
    road: [u8; 3],
    reach: i32,
}

impl Block {
    /// The metatile (0 = `.1`, 1 = `.2`) this block shows for a terrain and subtype.
    pub fn terrain_picture(&self, terrain: u8, subtype: u8) -> u8 {
        self.terrain.get(terrain as usize).and_then(|t| t.get(subtype as usize)).copied().unwrap_or(0)
    }

    /// The metatile this block shows for road type 1..=3.
    pub fn road_picture(&self, road: u8) -> u8 {
        self.road.get((road as usize).wrapping_sub(1)).copied().unwrap_or(0)
    }
}

/// One layer of a tile: a block and the tile of its metatile (row, column, 0..10).
pub struct Paint<'a> {
    pub block: &'a Block,
    pub row: usize,
    pub col: usize,
}

pub struct Mosaic {
    /// Per block row: the block columns it has, `[start, stop)`.
    spans: Vec<(i32, i32)>,
    /// Index of block (row, 0) on level 0, minus the row's start column.
    base: Vec<i32>,
    /// Blocks per level.
    stride: i32,
    blocks: Vec<Block>,
}

/// The game's global generator formula (`decomp/random.md`), stepped on a local state.
fn roll(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214013).wrapping_add(2531011);
    (*state >> 16) & 0x7fff
}

impl Mosaic {
    pub fn new(map: &MapFile) -> Mosaic {
        let rows = row_ranges(map.header.size as usize);
        let levels = map.header.levels.max(1) as i32;
        // Block rows cover four tile rows; a block row has the block columns any of its
        // tile rows touches.
        let mut spans = Vec::new();
        let mut base = Vec::new();
        let mut stride = 0;
        for chunk in rows.chunks(BLOCK as usize) {
            let start = chunk.iter().map(|&(s, _)| s as i32 / BLOCK).min().unwrap_or(0);
            let stop = chunk.iter().map(|&(_, e)| (e as i32 + BLOCK - 1) / BLOCK).max().unwrap_or(0);
            let (start, stop) = (start, stop.max(start));
            base.push(stride - start);
            stride += stop - start;
            spans.push((start, stop));
        }
        let count = (stride * levels) as usize;

        let mut state = map.header.terrain_seed;
        // the ranks: 0..count shuffled, element i swapped with a random one before it
        // (rva `396f10`)
        let mut ranks: Vec<u32> = (0..count as u32).collect();
        for i in 1..count {
            let j = roll(&mut state) as usize % i;
            ranks.swap(i, j);
        }
        let mut blocks = Vec::with_capacity(count);
        for &rank in &ranks {
            let mut terrain = [[0u8; 2]; TERRAINS];
            for t in 0..TERRAINS {
                for s in 0..SUBTYPES[t] {
                    terrain[t][s] = (roll(&mut state) % PICTURES[t]) as u8;
                }
            }
            let mut road = [0u8; 3];
            for r in &mut road {
                *r = (roll(&mut state) % 2) as u8;
            }
            // the exe steps the generator twice at once here (two steps fused into one
            // multiply), so the first of the two numbers is skipped
            roll(&mut state);
            let reach = (roll(&mut state) % 3) as i32;
            blocks.push(Block { rank, terrain, road, reach });
        }
        Mosaic { spans, base, stride, blocks }
    }

    fn block(&self, brow: i32, bcol: i32, level: i32) -> Option<&Block> {
        let &(start, stop) = self.spans.get(usize::try_from(brow).ok()?)?;
        if !(start..stop).contains(&bcol) {
            return None;
        }
        let i = self.base[brow as usize] + self.stride * level + bcol;
        self.blocks.get(usize::try_from(i).ok()?)
    }

    /// The layers of a tile, bottom first.
    pub fn paints(&self, row: i32, col: i32, level: i32) -> Vec<Paint<'_>> {
        let (brow, bcol) = (row.div_euclid(BLOCK), col.div_euclid(BLOCK));
        let (dr, dc) = (row - brow * BLOCK, col - bcol * BLOCK);
        let Some(own) = self.block(brow, bcol, level) else { return Vec::new() };

        // (block row, block column, block, fully covers the tile)
        let mut candidates = vec![(brow, bcol, own, true)];
        for sr in -1..=1 {
            for sc in -1..=1 {
                if sr == 0 && sc == 0 {
                    continue;
                }
                let Some(b) = self.block(brow + sr, bcol + sc, level) else { continue };
                if b.rank <= own.rank {
                    continue;
                }
                // Does the neighbour's picture reach this tile along one axis, and with a
                // full window (not just its ragged edge)? From a lower block it reaches
                // `reach + 2` tiles in (window `reach`), from a higher one it starts
                // `reach` tiles back (window from `reach + 2`).
                let along = |side: i32, d: i32| match side {
                    0 => (true, true),
                    -1 => (d < b.reach + 2, d < b.reach),
                    _ => (b.reach <= d, b.reach + 2 <= d),
                };
                let (cover_r, window_r) = along(sr, dr);
                let (cover_c, window_c) = along(sc, dc);
                if cover_r && cover_c {
                    candidates.push((brow + sr, bcol + sc, b, window_r && window_c));
                }
            }
        }
        candidates.sort_by(|a, b| b.2.rank.cmp(&a.2.rank));
        // the highest block with a full window hides everything below it
        let top = candidates.iter().position(|c| c.3).unwrap_or(candidates.len() - 1);
        candidates[..=top]
            .iter()
            .rev()
            .filter_map(|&(br, bc, block, _)| {
                let r = row - br * BLOCK + BLOCK - block.reach;
                let c = col - bc * BLOCK + BLOCK - block.reach;
                let r = usize::try_from(r).ok().filter(|&r| r < 10)?;
                let c = usize::try_from(c).ok().filter(|&c| c < 10)?;
                Some(Paint { block, row: r, col: c })
            })
            .collect()
    }
}
