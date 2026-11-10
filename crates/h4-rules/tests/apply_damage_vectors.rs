//! Applying damage to a stack, compared with answers recorded from the original
//! exe (`tools/testbench/apply_damage.py`). Runs without the game.
use h4_rules::battle::wounds::{accept_damage, kill, resolve_damage};

fn numbers(fields: &[&str]) -> Vec<i32> {
    fields.iter().map(|f| f.parse().unwrap()).collect()
}

#[test]
fn damage_application_matches_the_original() {
    let mut counts = [0; 3];
    for line in include_str!("apply_damage_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(' ').collect();
        match fields[0] {
            "R" => {
                let input = numbers(&fields[1..7]);
                let (count, wounds, pending, hp, permanent, deaths) =
                    (input[0], input[1], input[2], input[3], input[4] != 0, input[5]);
                let got = resolve_damage(count, wounds, pending, hp).unwrap();
                // "-" means the exe left "last killed" alone: nobody died.
                let killed = if fields[9] == "-" { 0 } else { fields[9].parse().unwrap() };
                let expected = numbers(&[fields[7], fields[8], fields[10], fields[11], fields[12]]);
                assert_eq!((got.count, got.wounds, got.killed), (expected[0], expected[1], killed), "{line}");
                // pending damage and the permanent flag are always cleared
                assert_eq!((expected[2], expected[4]), (0, 0), "{line}");
                // only kills flagged as permanent are added to the permanent deaths
                let permanent_deaths = deaths + if permanent { got.killed } else { 0 };
                assert_eq!(permanent_deaths, expected[3], "{line}");
                // The exe's "clear influence" call marks a stack left empty. It also
                // runs for a stack that was empty before (count 0), where nobody died.
                let emptied = fields[15] == "1";
                assert_eq!(emptied, (wounds + pending) / hp > 0 && got.count == 0, "{line}");
                counts[0] += 1;
            }
            "K" => {
                let v = numbers(&fields[1..]);
                let got = kill(v[0], v[1], v[2]);
                assert_eq!((got.count, got.wounds, got.killed), (v[3], v[4], v[5]), "{line}");
                counts[1] += 1;
            }
            "A" => {
                let v = numbers(&fields[1..]);
                let accepted = accept_damage(v[0], v[1], v[2]).unwrap();
                assert_eq!((accepted, v[1] + accepted), (v[3], v[4]), "{line}");
                counts[2] += 1;
            }
            other => panic!("unknown vector kind {other}"),
        }
    }
    assert_eq!(counts, [1642, 81, 77]);
}
