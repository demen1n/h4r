//! Creature statistics from `table.creatures` (hit points, damage, attack, defense...).

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
        });
    }
    Ok(out)
}
