use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_object::CombatObject;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resources = Resources::open(data_dir(std::env::args().nth(1)))?;
    let mut names: Vec<_> = resources
        .names()
        .filter(|n| n.to_lowercase().starts_with("combat_object."))
        .collect();
    names.sort();
    let mut failures = 0;
    let mut versions = [0; 3];
    let mut segment_count = 0;
    let mut compound_count = 0;
    let mut showed_example = false;
    for name in &names {
        match CombatObject::read(&resources.read(name)?) {
            Ok(model) => {
                versions[model.version as usize] += 1;
                let segments = model.segments()?;
                segment_count += segments.len();
                if segments.len() > 1 {
                    compound_count += 1;
                    if !showed_example && model.attributes[0] != 0 && model.flag == 0 {
                        showed_example = true;
                        println!(
                            "compound example: {name}, {:?}, {} segments",
                            model.properties,
                            segments.len()
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("{name}: {e}");
                failures += 1;
            }
        }
    }
    println!(
        "{} combat objects, versions {versions:?}, {failures} errors",
        names.len()
    );
    println!("{segment_count} segments, {compound_count} compound resources");
    if failures > 0 {
        return Err(format!("{failures} invalid objects").into());
    }
    Ok(())
}
