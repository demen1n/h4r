//! Inspect combat definitions without opening a graphics window.
use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_actor::CombatActorModel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let res = Resources::open(data_dir(std::env::args().nth(1)))?;
    let mut names: Vec<_> = res.names().filter(|n| n.to_lowercase().starts_with("combat_actor.")).collect();
    names.sort();
    let mut failures = 0;
    let mut missing_references = 0;
    for name in &names {
        match res.read(name).and_then(|d| CombatActorModel::read(&d)) {
            Ok(model) => {
                println!(
                    "{name}: v{}, properties {:?}, walk {}/{}/{}, offset {:?}, {} actions",
                    model.version,
                    model.properties,
                    model.prewalk_length,
                    model.walk_length,
                    model.postwalk_length,
                    model.offset,
                    model.actions.len()
                );
                for a in &model.actions {
                    if a.is_composite() {
                        continue;
                    }
                    let missing: Vec<_> = a
                        .sequences
                        .iter()
                        .filter(|s| !s.is_empty())
                        .filter(|s| res.entry(&format!("actor_sequence.{s}.h4d")).is_none())
                        .collect();
                    println!(
                        "  {}: {} fps, parameter {}, {} directions, missing {:?}",
                        a.name,
                        a.frames_per_second,
                        a.parameter,
                        a.sequences.iter().filter(|s| !s.is_empty()).count(),
                        missing
                    );
                    missing_references += missing.len();
                }
            }
            Err(e) => {
                eprintln!("{name}: {e}");
                failures += 1;
            }
        }
    }
    println!(
        "{} combat actors, {failures} parse errors, {missing_references} missing non-composite references",
        names.len()
    );
    if failures != 0 {
        return Err(format!("{failures} combat actor errors").into());
    }
    Ok(())
}
