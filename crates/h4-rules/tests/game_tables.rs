//! Checks on the real game tables. Skipped when the game isn't installed.

use h4_formats::archive::{data_dir, Resources};
use h4_formats::table::Table;
use h4_rules::creatures::{self, CreatureStats};

fn resources() -> Option<Resources> {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return None;
    }
    Some(Resources::open(&dir).unwrap_or_else(|e| panic!("open {}: {e}", dir.display())))
}

/// Rows in `table.creatures` of the Russian 3.0 release, header rows included.
const CREATURE_TABLE_ROWS: usize = 80;

#[test]
fn creature_table() {
    let Some(res) = resources() else { return };
    let data = res.read("table.creatures.h4d").expect("read table.creatures");
    let table = Table::read(&data, "table.creatures").expect("parse table.creatures");
    assert_eq!(table.rows.len(), CREATURE_TABLE_ROWS);
    let all = creatures::from_table(&table).expect("creature stats");
    // minus two header rows and four section titles before the expansions' creatures
    // ("Version 1.3", "Expansion 1 Creatures below", "Version 1.4", "Expansion 2 ...")
    assert_eq!(all.len(), CREATURE_TABLE_ROWS - 6);

    let find = |k: &str| -> &CreatureStats {
        all.iter().find(|c| c.keyword == k).unwrap_or_else(|| panic!("no {k}"))
    };
    // values as in the table (and in the game's creature window)
    let orc = find("orc");
    assert_eq!((orc.level, orc.hit_points, orc.damage_low, orc.damage_high), (1, 12, 1, 3));
    assert_eq!((orc.attack, orc.defense, orc.speed, orc.movement, orc.shots), (11, 9, 4, 18, 6));
    assert_eq!(orc.name, "Орк");

    for c in &all {
        assert!((1..=4).contains(&c.level), "{}: level {}", c.keyword, c.level);
        assert!(c.hit_points > 0, "{}: {} hit points", c.keyword, c.hit_points);
        assert!(0 < c.damage_low && c.damage_low <= c.damage_high, "{}: damage {}-{}", c.keyword, c.damage_low, c.damage_high);
        assert!(c.attack > 0 && c.defense > 0, "{}: attack {} defense {}", c.keyword, c.attack, c.defense);
        assert!(c.experience > 0, "{}: experience {}", c.keyword, c.experience);
    }

    // every creature of the game's enum has a row, and the table has nothing else
    let by_id = creatures::in_enum_order(&all).expect("creatures in enum order");
    assert_eq!(by_id.len(), all.len());
    assert_eq!((by_id[47].keyword.as_str(), by_id[47].experience), ("peasant", 4));
    assert_eq!(creatures::TOWN_TYPES[by_id[46].alignment], "asylum", "orcs are chaos");
}

/// Every artifact of the enum has a table row, every model exists in the archives, and
/// every artifact allowed by default and not disabled has one. (Two without a model,
/// `bow_of_the_white_stag` and `nomad_blackbow`, are not allowed by default, and every
/// installed map that lists its artifacts turns them off.)
#[test]
fn artifact_table_and_models() {
    let Some(res) = resources() else { return };
    let data = res.read("table.artifacts.h4d").expect("read table.Artifacts");
    let table = Table::read(&data, "table.Artifacts").expect("parse table.Artifacts");
    let info = h4_rules::artifacts::from_table(&table).expect("artifacts");
    assert_eq!(info.len(), 248);
    let potions = info.iter().filter(|a| a.slot == h4_rules::artifacts::POTION_SLOT).count();
    assert!(potions > 10, "{potions} potions");
    for (id, a) in info.iter().enumerate() {
        match h4_rules::artifacts::model(id) {
            Some(model) => assert!(res.entry(&format!("adv_object.{model}.h4d")).is_some(), "no adv_object.{model}"),
            None => assert!(
                h4_rules::artifacts::DISABLED.contains(&id) || !a.allowed_by_default,
                "{} has no model",
                h4_rules::artifacts::KEYWORDS[id]
            ),
        }
    }
}
