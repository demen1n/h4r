//! Turns a `modify` request of the exe bench into our damage types.
//!
//! Shared by `examples/exe_reference.rs` (live comparison with the exe) and the
//! golden-vector tests, so both read a request the same way.
use h4_rules::damage::*;

/// Reads the 15 modifier fields, in this order:
/// `base offense reduction defense bonus school attacker-abilities defender-abilities
/// ward-mask giant giant-effect ranged moved wall wall-strength`.
///
/// - ability lists are comma-separated decimal ids, `-` for none;
/// - `school` is an index into the game's order (life, order, death, chaos, nature, might);
/// - `ward-mask` bit i = warded against school i (by a spell);
/// - `wall`: 0 none, 1 crossed toward greater x, 2 toward smaller x (the exe's fourth
///   argument); `wall-strength` is an index into `WALL_MULTIPLIERS`.
///
/// Fields the request doesn't set (ids, stack size, HP) get neutral values; the target's
/// HP is huge so the final HP limit never interferes with a modifier test.
pub fn parse(
    v: &[&str],
) -> Result<(i32, Attacker, Defender, HitOptions), Box<dyn std::error::Error>> {
    let [base, offense, reduction, defense, bonus, school, abilities, resistances, wards, giant, effect, ranged, moved, wall, strength] =
        v
    else {
        return Err("invalid modifier fixture".into());
    };
    let schools = [
        School::Life,
        School::Order,
        School::Death,
        School::Chaos,
        School::Nature,
        School::Might,
    ];
    let list = |v: &str| -> Result<Vec<u8>, std::num::ParseIntError> {
        if v == "-" {
            Ok(vec![])
        } else {
            v.split(',').map(str::parse).collect()
        }
    };
    let a = Attacker {
        id: 1,
        number: 1,
        damage_low: 1,
        damage_high: 1,
        offense: offense.parse()?,
        defense_reduction: reduction.parse()?,
        school: *schools
            .get(school.parse::<usize>()?)
            .ok_or("invalid school")?,
        abilities: list(abilities)?,
        giant_bonus_effect: *effect == "1",
    };
    let mask: u32 = wards.parse()?;
    let d = Defender {
        id: 2,
        defense_basic: defense.parse()?,
        defense_bonus: bonus.parse()?,
        abilities: list(resistances)?,
        giant: *giant == "1",
        total_hit_points: 1000000,
        warded_against: schools
            .iter()
            .enumerate()
            .filter_map(|(i, s)| ((mask >> i) & 1 == 1).then_some(*s))
            .collect(),
    };
    let across_wall = match *wall {
        "0" => AcrossWall::No,
        "1" => AcrossWall::TowardGreaterX,
        "2" => AcrossWall::TowardSmallerX,
        _ => return Err("invalid wall".into()),
    };
    let multiplier = *WALL_MULTIPLIERS
        .get(strength.parse::<usize>()?)
        .ok_or("invalid wall strength")?;
    Ok((
        base.parse()?,
        a,
        d,
        HitOptions {
            ranged: *ranged == "1",
            moved: moved.parse()?,
            across_wall,
            wall_multiplier: multiplier,
            ..HitOptions::default()
        },
    ))
}

/// Answers a `modify` request: the damage after all modifiers.
pub fn evaluate(v: &[&str]) -> Result<i32, Box<dyn std::error::Error>> {
    let (base, a, d, options) = parse(v)?;
    Ok(modify_damage(
        base,
        &a,
        &d,
        options.ranged,
        options.moved,
        options.across_wall,
        options.wall_multiplier,
        &mut Vec::new(),
    ))
}
