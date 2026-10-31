//! Shared line-protocol adapter for the exe bench and its golden-vector tests.
use h4_rules::damage::*;

pub fn evaluate(v: &[&str]) -> Result<i32, Box<dyn std::error::Error>> {
    let [base, offense, reduction, defense, bonus, school, abilities, resistances, wards, giant, effect, ranged, moved, luck, strength] =
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
    let luck = match *luck {
        "0" => Luck::Normal,
        "1" => Luck::Good,
        "2" => Luck::Bad,
        _ => return Err("invalid luck".into()),
    };
    let multiplier = *LUCK_MULTIPLIERS
        .get(strength.parse::<usize>()?)
        .ok_or("invalid luck strength")?;
    Ok(modify_damage(
        base.parse()?,
        &a,
        &d,
        *ranged == "1",
        moved.parse()?,
        luck,
        multiplier,
        &mut Vec::new(),
    ))
}
