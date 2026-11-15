//! The melee attack order and the retaliation flag, compared with answers
//! recorded from the original exe (`tools/testbench/melee_attack.py`).
//! Runs without the game.
use h4_rules::battle::melee::{
    abilities, effects, number_of_attacks, plan_melee_attack, AttackCount, Continuation, MeleeSituation,
    RetaliationFlag, Striker,
};

/// The values whose bit `i` is set in `mask`.
fn pick<T: Copy>(mask: u32, values: &[T]) -> Vec<T> {
    values.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, v)| *v).collect()
}

fn int(field: &str) -> i32 {
    field.parse().unwrap()
}

/// The stamp the bench's acting stack has (its `get_adjusted_speed` fixture).
const BENCH_PRIORITY: i32 = 1234;

/// The plan written the way the bench prints what the exe did.
fn render(plan: &h4_rules::battle::melee::MeleePlan, return_after: i32) -> (String, String) {
    let strikes = plan
        .strikes
        .iter()
        .map(|s| {
            let who = match s.by {
                Striker::Attacker => "A>d",
                Striker::Defender => "D>a",
            };
            format!("{who}:{}{}", u8::from(s.sequential), u8::from(s.retaliation))
        })
        .collect::<Vec<_>>()
        .join(",");
    // resolve_attacks is always queued first, then the continuation.
    let then = match plan.then {
        Continuation::EndMeleeAttack { strikes_left } => format!("resolve,end({strikes_left}:{return_after})"),
        Continuation::BeginRetaliation => format!("resolve,retaliation({return_after})"),
    };
    (strikes, then)
}

#[test]
fn melee_attack_rules_match_the_original() {
    let mut counts = [0; 5];
    for line in include_str!("melee_attack_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "N" => {
                let abilities = pick(
                    int(f[3]) as u32,
                    &[abilities::STRIKES_TWICE, abilities::EXTRA_MELEE_ATTACK, abilities::SHOOTS_TWICE, abilities::EXTRA_RANGED_ATTACK],
                );
                let effects = pick(int(f[2]) as u32, &[effects::DOUBLE_ATTACK, effects::EXTRA_ATTACK]);
                let got = number_of_attacks(&AttackCount {
                    ranged: f[1] == "1",
                    abilities: &abilities,
                    effects: &effects,
                    shots: int(f[4]),
                });
                assert_eq!(got, int(f[5]), "{line}");
                counts[0] += 1;
            }
            "S" => {
                let mut flag = RetaliationFlag { retaliated: f[1] == "1", stamp: int(f[2]) };
                flag.set(f[3] == "1", int(f[4]));
                assert_eq!((flag.retaliated, flag.stamp), (f[5] == "1", int(f[6])), "{line}");
                counts[1] += 1;
            }
            "C" => {
                let mut flag = RetaliationFlag { retaliated: f[1] == "1", stamp: int(f[2]) };
                flag.turn_started(int(f[3]));
                assert_eq!((flag.retaliated, flag.stamp), (f[4] == "1", int(f[5])), "{line}");
                counts[2] += 1;
            }
            "R" => {
                let mut flag = RetaliationFlag { retaliated: f[1] == "1", stamp: int(f[2]) };
                flag.new_round();
                assert_eq!((flag.retaliated, flag.stamp), (f[3] == "1", int(f[4])), "{line}");
                counts[3] += 1;
            }
            "B" => {
                let attacker = pick(
                    int(f[2]) as u32,
                    &[abilities::FIRST_STRIKE, abilities::FIRST_STRIKE_IMMUNITY, abilities::STRIKES_TWICE],
                );
                let defender = pick(
                    int(f[3]) as u32,
                    &[abilities::FIRST_STRIKE, abilities::FIRST_STRIKE_IMMUNITY, abilities::UNLIMITED_RETALIATION],
                );
                let attacks = number_of_attacks(&AttackCount { abilities: &attacker, ..Default::default() });
                let plan = plan_melee_attack(&MeleeSituation {
                    can_retaliate: f[1] == "1",
                    attacker_abilities: &attacker,
                    defender_abilities: &defender,
                    defender_can_act: f[4] == "ok",
                    attacker_slot_38: f[5] == "1",
                    defender_slot_38: f[6] == "1",
                    moved: int(f[7]),
                    attacks,
                });
                let (strikes, then) = render(&plan, int(f[8]));
                assert_eq!((strikes.as_str(), then.as_str()), (f[9], f[10]), "{line}");
                assert_eq!(plan.retaliating, f[11] == "1", "{line}");
                // The bench's target starts with the flag clear.
                let mut flag = RetaliationFlag::default();
                if let Some(value) = plan.set_retaliated {
                    flag.set(value, BENCH_PRIORITY);
                }
                let stamp = if flag.retaliated { flag.stamp.to_string() } else { "-".to_string() };
                assert_eq!((flag.retaliated, stamp.as_str()), (f[12] == "1", f[13]), "{line}");
                counts[4] += 1;
            }
            other => panic!("unknown vector kind {other}"),
        }
    }
    println!("attacks {} / set {} / clear {} / round {} / begin_attack {}", counts[0], counts[1], counts[2], counts[3], counts[4]);
    assert_eq!(counts, [640, 40, 96, 16, 6144]);
}
