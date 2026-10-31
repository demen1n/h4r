//! Line protocol used by tools/testbench/compare.py; no original game data required.
use h4_rules::{damage::*, random::GlobalRandom};
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        let v: Vec<&str> = line.split_whitespace().collect();
        let mut a = Attacker {
            id: 1,
            number: 1,
            damage_low: 1,
            damage_high: 1,
            offense: 1.0,
            defense_reduction: 1.0,
            school: School::Might,
            abilities: vec![],
            giant_bonus_effect: false,
        };
        let mut d = Defender {
            id: 2,
            defense_basic: 1.0,
            defense_bonus: 1.0,
            abilities: vec![],
            warded_against: vec![],
            giant: false,
            total_hit_points: 1,
        };
        match v.as_slice() {
            ["global", seed, count] => {
                let mut rng = GlobalRandom::new(seed.parse()?);
                let values: Vec<String> = (0..count.parse::<usize>()?)
                    .map(|_| rng.next().to_string())
                    .collect();
                println!("{}", values.join(" "));
            }
            ["ratio", offense, reduction, defense, bonus] => {
                a.offense = offense.parse()?;
                a.defense_reduction = reduction.parse()?;
                d.defense_basic = defense.parse()?;
                d.defense_bonus = bonus.parse()?;
                println!("{:08x}", damage_modifier(&a, &d).to_bits());
            }
            ["final", damage, hp, dealt, block] => {
                d.total_hit_points = hp.parse()?;
                if *block == "1" {
                    d.abilities.push(ability::BLOCK);
                }
                println!(
                    "{}",
                    finalize_damage(damage.parse()?, &d, dealt.parse()?, &mut Vec::new())
                );
            }
            _ => return Err(format!("invalid request: {line}").into()),
        }
    }
    Ok(())
}
