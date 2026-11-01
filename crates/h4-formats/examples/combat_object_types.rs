//! Compare the stored type number of every combat object with its type name.
use std::collections::BTreeMap;

use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_object::{obstacle_type_by_name, CombatObject};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let res = Resources::open(data_dir(std::env::args().nth(1)))?;
    let mut pairs: BTreeMap<(u16, String, Option<u16>), usize> = BTreeMap::new();
    let names: Vec<String> = res
        .names()
        .filter(|n| n.starts_with("combat_object."))
        .map(str::to_owned)
        .collect();
    for name in names {
        let model = CombatObject::read(&res.read(&name)?)?;
        let h = model.header;
        *pairs
            .entry((
                h.type_id,
                h.type_name.clone(),
                obstacle_type_by_name(&h.type_name),
            ))
            .or_default() += 1;
    }
    for ((stored, name, by_name), count) in pairs {
        let mark = if Some(stored) == by_name {
            ""
        } else {
            "  <- differs"
        };
        println!("{stored:3} {name:16} {by_name:?} x{count}{mark}");
    }
    Ok(())
}
