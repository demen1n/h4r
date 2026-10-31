//! Our side of the original-exe comparison bench (`tools/testbench/compare.py`).
//!
//! The bench sends one request per line on stdin and reads one answer line per request
//! from stdout; it asks the original x86 code the same question and compares the two
//! answers. Requests are space-separated: a kind (`global`, `combat`, `dice`, `ability42`,
//! `attack`, `modify`, `ratio`, `final`) and its numbers. No game data is needed.
//!
//! The same parsing helpers (`support/*.rs`) are reused by the golden-vector tests in
//! `tests/`, so a stored original answer is checked through exactly this code path.
#[path = "support/attack_case.rs"]
mod attack_case;
#[path = "support/modifier_case.rs"]
mod modifier_case;
use h4_rules::{
    damage::*,
    random::{CombatRandom, GlobalRandom},
};
use std::io::{self, BufRead};
/// In a `combat` request, a roll triple with this purpose is not a roll but a marker:
/// "restart the generator with seed `first`". Real purposes are 0..=43 (44 of them, see
/// `CombatRandom::roll`), so 44 can't clash with a real roll.
const RESEED_MARKER: u32 = 44;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        let v: Vec<&str> = line.split_whitespace().collect();
        // Neutral creatures; each request kind overwrites only the fields it tests.
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
            ["combat", seed, modulus, rolls @ ..] => {
                if rolls.len() % 3 != 0 {
                    return Err("incomplete roll triple".into());
                }
                let modulus = modulus.parse()?;
                let mut rng = CombatRandom::new(seed.parse()?, modulus);
                let mut result = Vec::new();
                // The rolls come in triples (first id, second id, purpose).
                for [a, b, purpose] in rolls.as_chunks::<3>().0 {
                    let (a, b, purpose) = (a.parse()?, b.parse()?, purpose.parse()?);
                    if purpose == RESEED_MARKER {
                        rng = CombatRandom::new(a, modulus);
                    } else {
                        result.push(rng.roll(a, b, purpose).to_string());
                    }
                }
                println!("{}", result.join(" "));
            }
            ["dice", seed, modulus, attacker, target, number, low, high, hits] => {
                a.id = attacker.parse()?;
                a.number = number.parse()?;
                a.damage_low = low.parse()?;
                a.damage_high = high.parse()?;
                let target = target.parse()?;
                let mut rng = CombatRandom::new(seed.parse()?, modulus.parse()?);
                let mut result: Vec<String> = (0..hits.parse::<usize>()?)
                    .map(|_| roll_base_damage(&a, target, &mut rng).to_string())
                    .collect();
                // One more roll after the hits shows how many numbers the hits used up.
                result.push(rng.roll(target, a.id, 0).to_string());
                println!("{}", result.join(" "));
            }
            [kind @ ("ability42" | "attack"), fields @ ..] => {
                let values = attack_case::evaluate(kind, fields)?;
                println!(
                    "{}",
                    values
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            ["modify", fields @ ..] => println!("{}", modifier_case::evaluate(fields)?),
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
