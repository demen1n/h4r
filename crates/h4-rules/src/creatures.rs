//! Creature statistics from `table.creatures` (hit points, damage, attack, defense...)
//! and the creature numbering the game uses everywhere else (maps, saves, code).

use h4_formats::table::Table;

/// One row of `table.creatures`. Only the columns the rules use so far.
#[derive(Debug, Clone, PartialEq)]
pub struct CreatureStats {
    /// Internal name, e.g. `orc`.
    pub keyword: String,
    /// Display name (Russian in the Russian release).
    pub name: String,
    pub level: i32,
    pub hit_points: i32,
    pub damage_low: i32,
    pub damage_high: i32,
    pub attack: i32,
    pub defense: i32,
    pub speed: i32,
    pub movement: i32,
    pub shots: i32,
    /// Creatures a dwelling makes in a week (`Weekly Growth`, `+0x70` of the traits);
    /// neutral monsters on the map grow by it too.
    pub weekly_growth: i32,
    /// Town type the creature belongs to (see [`TOWN_TYPES`]); the table calls it
    /// alignment. In the exe it is at `+0x44` of the creature traits (rva `255390`).
    pub alignment: usize,
    /// Experience a hero gets for killing one; the game also measures the strength of
    /// random monsters with it (`+0x74` of the traits).
    pub experience: i32,
}

/// Town types in the game's order (the order of the town model names at VA `0xa5b6c8`).
/// The creature table names them by magic: life, order, death, chaos, nature, might.
pub const TOWN_TYPES: [&str; 6] = ["haven", "academy", "necropolis", "asylum", "preserve", "stronghold"];

/// The alignment names of `table.creatures`, in [`TOWN_TYPES`] order.
const ALIGNMENTS: [&str; 6] = ["life", "order", "death", "chaos", "nature", "might"];

/// Creature keywords in the order of the game's creature enum, so `KEYWORDS[id]` names
/// the creature a map or the code calls `id`. Taken from the exe's keyword table at VA
/// `0x985574` (pairs of id + name, ids 0..73). Checked on all installed maps: with this
/// order high-level creatures come in small stacks and low-level ones in big stacks
/// (correlation of level with log(count) −0.80; reading the pairs the other way gives 0.03).
/// The code agrees too: it treats 38, 50 and 52 (mermaid, pirate, sea monster) as the
/// water creatures and 47 (peasant) as the unit of monster strength (rva `3f3310`, `3f38a0`).
pub const KEYWORDS: [&str; 74] = [
    "air elemental", "archangel", "ballista", "bandit", "behemoth", "beholder", "black dragon",
    "bone dragon", "centaur", "cerberus", "champion", "crossbowman", "crusader", "cyclops",
    "venom spawn", "archdevil", "dragon golem", "dwarf", "earth elemental", "efreet", "elf",
    "faerie dragon", "fire elemental", "gargoyle", "genie", "ghost", "berserker", "gold golem",
    "griffin", "halfling", "harpy", "hydra", "ice demon", "imp", "leprechaun", "mage", "mantis",
    "medusa", "mermaid", "minotaur", "monk", "mummy", "naga", "nightmare", "nomad", "ogre mage",
    "orc", "peasant", "phoenix", "pikeman", "pirate", "satyr", "sea monster", "skeleton",
    "squire", "sprite", "thunderbird", "titan", "troglodyte", "troll", "unicorn", "vampire",
    "water elemental", "white tiger", "wolf", "zombie", "waspwort", "goblin knight",
    "evil sorceress", "gargantuan", "dark champion", "catapult", "frenzied gnasher",
    "mega dragon",
];

/// Which game a creature came with: 0 the base game, 1 and 2 the creatures the exe lists
/// at VA `0x985550` (ids 67..=70) and `0x985560` (71..=73). Stored at `+0xdc` of the traits.
pub fn expansion(id: usize) -> i32 {
    match id {
        67..=70 => 1,
        71..=73 => 2,
        _ => 0,
    }
}

/// The table's rows put in game enum order: `result[id]` is the creature the game calls
/// `id`. Fails if a keyword of [`KEYWORDS`] has no row.
pub fn in_enum_order(rows: &[CreatureStats]) -> Result<Vec<CreatureStats>, String> {
    KEYWORDS
        .iter()
        .map(|k| {
            rows.iter()
                .find(|c| c.keyword.eq_ignore_ascii_case(k))
                .cloned()
                .ok_or(format!("table.creatures has no {k:?}"))
        })
        .collect()
}

/// Turn the table into stats. Fails with a message if a column is missing or a cell that
/// should be a number isn't one.
pub fn from_table(table: &Table) -> Result<Vec<CreatureStats>, String> {
    // Find columns by their header text rather than by position, so a table with an extra
    // column still works.
    let col = |title: &str| table.column(title).ok_or(format!("table.creatures has no column {title:?}"));
    let keyword = col("Keyword")?;
    let name = col("Name")?;
    let level = col("Level")?;
    let hit_points = col("Hit Points")?;
    let low = col("Low")?;
    let high = col("High")?;
    let attack = col("Attack")?;
    let defense = col("Defense")?;
    let speed = col("Speed")?;
    let movement = col("Move")?;
    let shots = col("Shots")?;
    let alignment = col("Alignment")?;
    let experience = col("Experience")?;
    let weekly_growth = col("Weekly Growth")?;

    let mut out = Vec::new();
    for (i, row) in table.data_rows().iter().enumerate() {
        let text = |c: usize| row.get(c).map(|s| s.trim()).unwrap_or("");
        // Empty cells mean 0 (the designers left zeros out in places).
        let number = |c: usize| -> Result<i32, String> {
            let t = text(c);
            if t.is_empty() {
                return Ok(0);
            }
            t.parse().map_err(|_| format!("table.creatures row {}: {t:?} is not a number", i + 1))
        };
        // Rows without hit points aren't creatures: the table has section titles like
        // "Version 1.3" and "Expansion 1 Creatures below" before the expansions' creatures.
        if text(keyword).is_empty() || text(hit_points).is_empty() {
            continue;
        }
        // the table writes these in mixed case ("Life", "death")
        let align_name = text(alignment).to_lowercase();
        let Some(align) = ALIGNMENTS.iter().position(|a| *a == align_name) else {
            return Err(format!("table.creatures row {}: unknown alignment {align_name:?}", i + 1));
        };
        out.push(CreatureStats {
            keyword: text(keyword).to_string(),
            name: text(name).to_string(),
            level: number(level)?,
            hit_points: number(hit_points)?,
            damage_low: number(low)?,
            damage_high: number(high)?,
            attack: number(attack)?,
            defense: number(defense)?,
            speed: number(speed)?,
            movement: number(movement)?,
            shots: number(shots)?,
            weekly_growth: number(weekly_growth)?,
            alignment: align,
            experience: number(experience)?,
        });
    }
    Ok(out)
}
