use h4_formats::archive::{data_dir, Resources};
use h4_formats::battlefield::BattlefieldPreset;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resources = Resources::open(data_dir(std::env::args().nth(1)))?;
    let mut names: Vec<_> = resources
        .names()
        .filter(|n| n.to_lowercase().starts_with("battlefield_preset_map."))
        .collect();
    names.sort();
    for name in &names {
        let field = BattlefieldPreset::read(&resources.read(name)?)?;
        println!(
            "{name}: {} cells, {} layers",
            field.rows.iter().map(|r| r.cells.len()).sum::<usize>(),
            field.layers.len()
        );
    }
    println!("{} battlefields read without trailing bytes", names.len());
    Ok(())
}
