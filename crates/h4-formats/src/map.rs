//! Adventure maps: `.h4c` files in the game's `maps` folder.
//!
//! A single-scenario `.h4c` is a small plain header and one gzip stream:
//!
//! ```text
//! "H4CAMPAIGN", u32 file version (0x16), u8 multi-scenario flag (0 here), gzip stream
//! ```
//!
//! The stream is the whole map, read by the game in one pass (`t_adventure_map::read_from_map`,
//! rva `d15c0` in our exe):
//!
//! 1. the map header ([`MapHeader`], reader at rva `37a320`): sizes, players, teams, texts;
//! 2. events, allowed artifacts/skills/spells, AI hints and then every object on the map
//!    ([`crate::map_body`]);
//! 3. the tiles of every level, row by row ([`Tile`], reader at rva `139d0`);
//! 4. one height byte per tile vertex.
//!
//! [`MapFile::read`] reads part 2 and then the tiles right after it. If part 2 fails (an
//! object type or version we don't know yet), it falls back to finding the tiles from the
//! end: parts 3 and 4 have sizes that follow from the header and end the stream, so the
//! tiles start at the one offset that parses into exactly the right number of valid tiles
//! followed by exactly the height bytes. On all 106 maps of the installation both ways
//! give the same offset.

use crate::cp1251;
use crate::map_body::{self, MapBody};
use crate::reader::{bad, Reader};
use crate::{Error, Result};

/// The terrain enum of the game (names from the table at rva `5958d8`).
pub const TERRAIN_NAMES: [&str; 19] = [
    "water",
    "grass",
    "rough",
    "swamp",
    "volcanic",
    "snow",
    "sand",
    "dirt",
    "subterranean",
    "river",
    "lava",
    "ice",
    "special_all",
    "special_life",
    "special_order",
    "special_death",
    "special_chaos",
    "special_nature",
    "special_combat",
];

/// How many subtypes each terrain has (table at rva `5709d8`). Subtype picks one of the
/// look variants, e.g. `terrain.grass.1.*` vs `terrain.grass.2.*`.
const TERRAIN_SUBTYPES: [u8; 19] = [2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];

/// Map versions the header reader of our exe accepts (it rejects anything else).
const MIN_VERSION: u16 = 20;
const MAX_VERSION: u16 = 29;

/// The header at the start of the decompressed map.
#[derive(Debug, Clone)]
pub struct MapHeader {
    pub version: u16,
    /// Present from version 28 on (0 before). Most likely the expansion the map needs:
    /// 0 base game, 1 Gathering Storm, 2 Winds of War. The 31 version-27 maps all have 0,
    /// "Bandits of the Rifen Maze" and other Winds of War maps 2. The game compares it with
    /// a creature's expansion when it picks random monsters (`map + 0x20c`, rva `3f3310`);
    /// that this field lands at `+0x20c` is inferred from offsets, not traced.
    pub expansion: u16,
    /// Width and height in tiles (the map is a diamond inside this square, see [`row_ranges`]).
    pub size: u16,
    /// 1, or 2 with an underground level.
    pub levels: u8,
    /// Seed of the terrain mosaic: which metatile of each pair every 4x4 block of tiles
    /// shows and how blocks overlap (`t_metatile_map`, rva `395400`; hint from openh4,
    /// checked in our exe). Many maps share values (0x4b40, 0x4cad...).
    pub terrain_seed: u32,
    pub players: Vec<PlayerSlot>,
    pub name: String,
    /// 0..=4. Probably the difficulty (the game has five levels) - unconfirmed.
    pub difficulty: u8,
    pub description: String,
    /// Each team is a bit mask of player colors (bit 0 = color 0).
    pub teams: Vec<u8>,
    /// Custom texts; `None` means the game's default text.
    pub loss_text: Option<String>,
    pub victory_text: Option<String>,
    /// From version 25 on (`true` before). Meaning unknown.
    pub unknown_flag_a: bool,
    /// From version 24 on. Two optional scenes, probably intro and ending.
    pub cut_scenes: [Option<CutScene>; 2],
    /// From version 27 on; free text, maps use it for author and version notes.
    pub notes: String,
    /// From version 29 on. Meaning unknown.
    pub unknown_flag_b: bool,
}

/// One of up to six players.
#[derive(Debug, Clone, Copy)]
pub struct PlayerSlot {
    /// 0..=5, the player color.
    pub color: u8,
    /// The header requires at least one slot with this set; most likely "a human may play it".
    pub playable: bool,
    /// Meaning unknown (0 or 1).
    pub unknown: u8,
    /// Allowed town types as bits 0..=5 (0x3f = any).
    pub town_types: u8,
}

/// A scene shown with a map: a text and two names the game looks up in its own tables
/// (74 and 72 entries). Probably a video and a music track - unconfirmed.
#[derive(Debug, Clone)]
pub struct CutScene {
    pub text: String,
    pub first_name: String,
    pub second_name: String,
}

/// One tile as stored in the map (tile format version 3 or 4).
#[derive(Debug, Clone, Default)]
pub struct Tile {
    /// Index into [`TERRAIN_NAMES`].
    pub terrain: u8,
    /// 0 or 1 for terrains with two looks.
    pub subtype: u8,
    /// 0 = no road, else road type 1..=3.
    pub road: u8,
    /// 0..=2 (`get_rock_terrain_type`).
    pub rock_terrain: u8,
    /// `has_rock_terrain_transition`.
    pub rock_transition: bool,
    /// Six 3-bit fields plus bit 19 (the default is 0x80000). Probably per-player visibility.
    pub visibility: u32,
    /// Edges of neighbouring terrains drawn over this tile, precomputed by the editor.
    pub terrain_transitions: Vec<TerrainTransition>,
    pub road_transitions: Vec<RoadTransition>,
}

/// A mask from `transition.Transitions` drawn over a tile with another terrain.
#[derive(Debug, Clone, Copy)]
pub struct TerrainTransition {
    pub terrain: u8,
    pub subtype: u8,
    /// 9 bits; which mask (`get_id`).
    pub id: u16,
    /// 5 bits; which mask set (`get_mask_set`).
    pub mask_set: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct RoadTransition {
    pub road: u8,
    pub id: u16,
    pub mask_set: u8,
}

/// A whole map: header, events and objects, tiles.
pub struct MapFile {
    pub header: MapHeader,
    /// Events and objects, or why they couldn't be read (then the tiles were found from
    /// the end of the stream instead).
    pub body: std::result::Result<MapBody, String>,
    /// The decompressed stream.
    pub data: Vec<u8>,
    /// Where the header ends in `data`.
    pub header_end: usize,
    /// Where the tiles start in `data`.
    pub tiles_start: usize,
    /// Tiles of level 0 then level 1, each level row by row, each row from
    /// `row_ranges(size)[row].0` to `.1`.
    pub tiles: Vec<Tile>,
    /// One byte per vertex, same layout as tiles but with [`vertex_row_ranges`].
    pub heights: Vec<u8>,
}

impl MapFile {
    /// Read a `.h4c` file. `name` is only used in error messages.
    pub fn read(file: &[u8], name: &str) -> Result<MapFile> {
        let data = unpack(file).map_err(|e| context(e, name))?;
        let (header, header_end) = MapHeader::read(&data).map_err(|e| context(e, name))?;
        let players: Vec<u8> = header.players.iter().map(|p| p.color).collect();
        let mut r = Reader::at(&data, header_end);
        let body = map_body::read_body(&mut r, header.version, &players).map_err(|e| e.to_string());
        let (tiles_start, tiles, heights) = match &body {
            Ok(_) => read_tiles_at(&data, &header, r.pos),
            Err(_) => find_tiles(&data, &header, header_end),
        }
        .map_err(|e| context(e, name))?;
        Ok(MapFile { header, body, data, header_end, tiles_start, tiles, heights })
    }

    /// The tile at (`row`, `column`) of `level`, if it lies inside the map diamond.
    pub fn tile(&self, level: usize, row: usize, column: usize) -> Option<&Tile> {
        let ranges = row_ranges(self.header.size as usize);
        let per_level: usize = ranges.iter().map(|(s, e)| e - s).sum();
        let (start, end) = *ranges.get(row)?;
        if column < start || column >= end {
            return None;
        }
        let before: usize = ranges[..row].iter().map(|(s, e)| e - s).sum();
        self.tiles.get(level * per_level + before + column - start)
    }

    /// The height of vertex (`row`, `column`) of `level`: the top corner of tile
    /// (`row`, `column`). `None` outside the map.
    pub fn height(&self, level: usize, row: usize, column: usize) -> Option<u8> {
        let ranges = vertex_row_ranges(self.header.size as usize);
        let per_level: usize = ranges.iter().map(|(s, e)| e - s).sum();
        let (start, end) = *ranges.get(row)?;
        if column < start || column >= end {
            return None;
        }
        let before: usize = ranges[..row].iter().map(|(s, e)| e - s).sum();
        self.heights.get(level * per_level + before + column - start).copied()
    }
}

fn context(e: Error, name: &str) -> Error {
    match e {
        Error::Format(m) => Error::Format(format!("{name}: {m}")),
        other => other,
    }
}

/// Check the plain header and decompress the map stream.
pub fn unpack(file: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(file);
    if r.take(10)? != b"H4CAMPAIGN" {
        return bad("no H4CAMPAIGN signature");
    }
    let version = r.u32()?;
    if version != 0x16 {
        return bad(format!("file version {version:#x}, only 0x16 is known"));
    }
    let multi = r.u8()?;
    if multi != 0 {
        // Campaigns hold several maps behind a list of map infos; not decoded yet.
        return bad("multi-scenario campaign files are not supported yet");
    }
    let mut out = Vec::new();
    use std::io::Read;
    flate2::read::GzDecoder::new(&file[r.pos..]).read_to_end(&mut out)?;
    Ok(out)
}

impl MapHeader {
    /// Read the header from the start of the decompressed stream. Returns the header and
    /// the offset right after it.
    pub fn read(data: &[u8]) -> Result<(MapHeader, usize)> {
        let mut r = Reader::new(data);
        let version = r.u16()?;
        if !(MIN_VERSION..=MAX_VERSION).contains(&version) {
            return bad(format!("map version {version}, expected {MIN_VERSION}..={MAX_VERSION}"));
        }
        let expansion = if version >= 28 { r.u16()? } else { 0 };
        let size = r.u16()?;
        let levels = r.u8()?;
        let terrain_seed = r.u32()?;

        let count = r.u8()?;
        if count == 0 || count > 6 {
            return bad(format!("{count} players at offset {:#x}, expected 1..=6", r.pos - 1));
        }
        let mut players: Vec<PlayerSlot> = Vec::new();
        for _ in 0..count {
            let at = r.pos;
            let color = r.u8()?;
            if color > 5 || players.iter().any(|p| p.color == color) {
                return bad(format!("bad or repeated player color {color} at offset {at:#x}"));
            }
            let playable = r.u8()? != 0;
            let unknown = r.u8()?;
            let town_types = read_town_type_set(&mut r)?;
            players.push(PlayerSlot { color, playable, unknown, town_types });
        }
        if !players.iter().any(|p| p.playable) {
            return bad("no playable player");
        }

        let name = str16(&mut r)?;
        let difficulty = r.u8()?;
        if difficulty > 4 {
            return bad(format!("difficulty {difficulty} at offset {:#x}, expected 0..=4", r.pos - 1));
        }
        let description = str16(&mut r)?;

        let teams_version = r.u16()?;
        if teams_version != 0 {
            return bad(format!("team list version {teams_version} at offset {:#x}, expected 0", r.pos - 2));
        }
        let team_count = r.u8()?;
        let teams: Vec<u8> = r.take(team_count as usize)?.to_vec();
        // Every present color is in exactly one team, absent colors in none - the game
        // checks the same.
        for color in 0..6u8 {
            let present = players.iter().any(|p| p.color == color);
            let in_teams = teams.iter().filter(|&&t| t & (1 << color) != 0).count();
            if in_teams != usize::from(present) {
                return bad(format!("color {color} is in {in_teams} teams"));
            }
        }

        let loss_text = if r.u8()? != 0 { Some(str16(&mut r)?) } else { None };
        let victory_text = if r.u8()? != 0 { Some(str16(&mut r)?) } else { None };
        let unknown_flag_a = if version >= 25 { r.u8()? != 0 } else { true };
        let mut cut_scenes = [None, None];
        if version >= 24 {
            for scene in cut_scenes.iter_mut() {
                if r.u8()? != 0 {
                    *scene = Some(read_cut_scene(&mut r)?);
                }
            }
        }
        let notes = if version >= 27 { str16(&mut r)? } else { String::new() };
        let unknown_flag_b = if version >= 29 { r.u8()? != 0 } else { false };

        let header = MapHeader {
            version,
            expansion,
            size,
            levels,
            terrain_seed,
            players,
            name,
            difficulty,
            description,
            teams,
            loss_text,
            victory_text,
            unknown_flag_a,
            cut_scenes,
            notes,
            unknown_flag_b,
        };
        Ok((header, r.pos))
    }

    /// Tile format version for this map version (`FUN_004cad60` in the exe).
    pub fn tile_version(&self) -> u8 {
        match self.version {
            26.. => 4,
            19..=25 => 3,
            8..=18 => 2,
            6 | 7 => 1,
            _ => 0,
        }
    }
}

fn str16(r: &mut Reader) -> Result<String> {
    let n = r.u16()? as usize;
    Ok(cp1251::decode(r.take(n)?))
}

/// `t_town_type_set::read`: u8 version (0), u8 bit mask of six town types.
fn read_town_type_set(r: &mut Reader) -> Result<u8> {
    let version = r.u8()?;
    if version != 0 {
        return bad(format!("town type set version {version} at offset {:#x}", r.pos - 1));
    }
    let mask = r.u8()?;
    // the game ignores bits 6 and 7
    Ok(mask & 0x3f)
}

fn read_cut_scene(r: &mut Reader) -> Result<CutScene> {
    let version = r.u16()?;
    if version != 0 {
        return bad(format!("cut scene version {version} at offset {:#x}", r.pos - 2));
    }
    Ok(CutScene { text: str16(r)?, first_name: str16(r)?, second_name: str16(r)? })
}

/// Which columns of each row are inside the map, as `start..end`.
///
/// The map is a diamond inside the `size` x `size` square: the top and bottom rows hold two
/// tiles, the middle rows all of them. This ports `t_isometric_map_base_base::base_initialize`
/// (rva `34db60`), called with a view of `size*h` x `size*h/2` pixels; `h` cancels out, so we
/// use 64. Integer division truncates toward zero, as in C.
pub fn row_ranges(size: usize) -> Vec<(usize, usize)> {
    let w = size as i64;
    let h = 64i64;
    let (view_x, view_y) = (w * h * 2 / 2, w * h / 2);
    let h2 = 2 * h;
    let a = (w * h - view_y) / 2;
    let b = (h2 * w - view_x) / 2 - (h2 * w) / 2;
    let mut down = (h - 1 + (view_y + a) * 2) / h;
    let mut up = (h2 - 1 + (view_x + b) * 2) / h2 + 1;
    let first_start = (a * 2) / h - 1 - down;
    let second_start = (b * 2 - h2 + 1) / h2 - up;
    let mut rows = Vec::with_capacity(size);
    for _ in 0..size {
        let mut start = first_start + down;
        if start < 0 {
            start = (second_start + up).max(0);
        }
        let end = if w < up { w.min(down) } else { up };
        rows.push((start as usize, end as usize));
        up += 1;
        down -= 1;
    }
    rows
}

/// Rows of tile vertices: one more row than tiles, each row spanning the corners of the
/// tiles above and below it (`t_isometric_vertex_map_base::initialize`, rva `34de80`).
pub fn vertex_row_ranges(size: usize) -> Vec<(usize, usize)> {
    let tiles = row_ranges(size);
    let mut rows = Vec::with_capacity(size + 1);
    rows.push((tiles[0].0, tiles[0].1 + 1));
    for i in 1..size {
        rows.push((tiles[i - 1].0.min(tiles[i].0), tiles[i - 1].1.max(tiles[i].1) + 1));
    }
    rows.push((tiles[size - 1].0, tiles[size - 1].1 + 1));
    rows
}

/// Read one tile (`FUN_004139d0`). `None` if the bytes can't be a tile.
fn read_tile(r: &mut Reader, version: u8) -> Result<Option<Tile>> {
    let terrain = r.u8()?;
    let subtype = r.u8()?;
    if !valid_terrain(terrain, subtype) {
        return Ok(None);
    }
    let road = r.u8()?;
    if road > 3 {
        return Ok(None);
    }
    let rock_terrain = r.u8()?;
    let rock_transition = r.u8()?;
    if rock_terrain > 2 || rock_transition > 1 {
        return Ok(None);
    }
    let visibility = match version {
        4 => r.u32()?,
        // version 3 packs the six fields into 2 bits each
        3 => {
            let packed = r.u16()? as u32;
            (0..6).fold(0x80000, |v, i| v | ((packed >> (2 * i)) & 3) << (3 * i))
        }
        _ => return bad(format!("tile version {version} is not supported")),
    };
    // Six 3-bit fields (bits 0..18) and bit 19, which the version 3 conversion always sets.
    // Every tile of every installed map has this shape; checking it rules out byte offsets
    // that would otherwise also parse (see `find_tiles`).
    if visibility & !0x3ffff != 0x80000 {
        return Ok(None);
    }
    let mut tile = Tile {
        terrain,
        subtype,
        road,
        rock_terrain,
        rock_transition: rock_transition != 0,
        visibility,
        ..Tile::default()
    };
    let count = r.u8()?;
    for _ in 0..count {
        let terrain = r.u8()?;
        let subtype = r.u8()?;
        if !valid_terrain(terrain, subtype) {
            return Ok(None);
        }
        let id = r.u16()? & 0x1ff;
        let mask_set = r.u8()? & 0x1f;
        tile.terrain_transitions.push(TerrainTransition { terrain, subtype, id, mask_set });
    }
    let count = r.u8()?;
    for _ in 0..count {
        let road = r.u8()?;
        if road > 3 {
            return Ok(None);
        }
        let id = r.u16()? & 0x1ff;
        let mask_set = r.u8()? & 0x1f;
        tile.road_transitions.push(RoadTransition { road, id, mask_set });
    }
    Ok(Some(tile))
}

fn valid_terrain(terrain: u8, subtype: u8) -> bool {
    (terrain as usize) < TERRAIN_SUBTYPES.len() && subtype < TERRAIN_SUBTYPES[terrain as usize]
}

/// Read the tiles starting at `start` (right after the objects); they must be followed by
/// exactly the height bytes.
fn read_tiles_at(data: &[u8], header: &MapHeader, start: usize) -> Result<(usize, Vec<Tile>, Vec<u8>)> {
    let (tile_count, vertex_count) = tile_and_vertex_counts(header)?;
    let version = header.tile_version();
    let mut r = Reader::at(data, start);
    let mut tiles = Vec::with_capacity(tile_count);
    for _ in 0..tile_count {
        let at = r.pos;
        match read_tile(&mut r, version)? {
            Some(t) => tiles.push(t),
            None => return bad(format!("invalid tile at {at:#x} (objects ended at {start:#x})")),
        }
    }
    if r.remaining() != vertex_count {
        return bad(format!(
            "after the tiles {} bytes are left, the height map needs {vertex_count}",
            r.remaining()
        ));
    }
    Ok((start, tiles, data[r.pos..].to_vec()))
}

/// Tiles and vertices of all levels.
fn tile_and_vertex_counts(header: &MapHeader) -> Result<(usize, usize)> {
    let size = header.size as usize;
    if size < 2 || size > 1024 || !(1..=2).contains(&header.levels) {
        return bad(format!("implausible map size {size} x {} levels", header.levels));
    }
    let levels = header.levels as usize;
    let tiles = row_ranges(size).iter().map(|(s, e)| e - s).sum::<usize>() * levels;
    let vertices = vertex_row_ranges(size).iter().map(|(s, e)| e - s).sum::<usize>() * levels;
    Ok((tiles, vertices))
}

/// Where the tiles start according to the search from the end (for checking the
/// sequential read, see `examples/check_maps.rs`).
pub fn search_tiles_start(map: &MapFile) -> Result<usize> {
    Ok(find_tiles(&map.data, &map.header, map.header_end)?.0)
}

/// Find the tile array by its end (see the module docs) and read tiles and heights.
fn find_tiles(data: &[u8], header: &MapHeader, header_end: usize) -> Result<(usize, Vec<Tile>, Vec<u8>)> {
    let size = header.size as usize;
    if size < 2 || size > 1024 || !(1..=2).contains(&header.levels) {
        return bad(format!("implausible map size {size} x {} levels", header.levels));
    }
    let levels = header.levels as usize;
    let tile_count: usize = row_ranges(size).iter().map(|(s, e)| e - s).sum::<usize>() * levels;
    let vertex_count: usize = vertex_row_ranges(size).iter().map(|(s, e)| e - s).sum::<usize>() * levels;
    let version = header.tile_version();
    if data.len() < header_end + vertex_count {
        return bad("map too short for its height map");
    }
    let tiles_end = data.len() - vertex_count;

    // tiles_left[p] = how many tiles, read one after another from offset p, end exactly at
    // tiles_end (None if they don't). A tile only ever moves forward, so filling the table
    // from the back needs one tile parse per offset instead of a full scan from each one.
    let mut tiles_left: Vec<Option<u32>> = vec![None; tiles_end + 1];
    tiles_left[tiles_end] = Some(0);
    for p in (header_end..tiles_end).rev() {
        let mut r = Reader::at(&data[..tiles_end], p);
        if let Ok(Some(_)) = read_tile(&mut r, version) {
            tiles_left[p] = tiles_left[r.pos].map(|n| n + 1);
        }
    }
    let starts: Vec<usize> =
        (header_end..tiles_end).filter(|&p| tiles_left[p] == Some(tile_count as u32)).collect();
    let start = match starts.as_slice() {
        [one] => *one,
        [] => return bad(format!("no offset gives {tile_count} tiles ending at {tiles_end:#x}")),
        many => return bad(format!("{} possible tile starts: {:x?}", many.len(), many)),
    };

    let mut r = Reader::at(&data[..tiles_end], start);
    let mut tiles = Vec::with_capacity(tile_count);
    for _ in 0..tile_count {
        // the search above already proved these all parse
        tiles.push(read_tile(&mut r, version)?.expect("tile checked by the search"));
    }
    Ok((start, tiles, data[tiles_end..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_map_is_a_diamond() {
        let rows = row_ranges(76);
        assert_eq!(rows.len(), 76);
        assert_eq!(rows[0], (37, 39));
        assert_eq!(rows[37], (0, 76));
        assert_eq!(rows[38], (0, 76));
        assert_eq!(rows[75], (37, 39));
        assert_eq!(rows.iter().map(|(s, e)| e - s).sum::<usize>(), 2964);
        let vertices = vertex_row_ranges(76);
        assert_eq!(vertices.len(), 77);
        assert_eq!(vertices.iter().map(|(s, e)| e - s).sum::<usize>(), 3117);
    }

    #[test]
    fn rejects_bad_signature() {
        assert!(unpack(b"H4SOMETHING\x16\0\0\0\0").is_err());
        assert!(unpack(b"H4CAMPAIGN").is_err());
    }
}
