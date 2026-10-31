//! Finalization and connected numerical attacks captured from original machine code.
#[path = "../examples/support/attack_case.rs"]
mod attack_case;
#[allow(dead_code)]
#[path = "../examples/support/modifier_case.rs"]
mod modifier_case;

fn check(kind: &str, count: usize) {
    let mut cases = 0;
    for line in include_str!("attack_vectors.tsv")
        .lines()
        .filter(|line| line.starts_with(kind))
    {
        let (request, expected) = line.split_once('\t').expect("fixture has two columns");
        let fields: Vec<_> = request.split_whitespace().collect();
        let got = attack_case::evaluate(fields[0], &fields[1..]).expect("valid fixture");
        let expected: Vec<i32> = expected
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(got, expected, "{request}");
        cases += 1;
    }
    assert_eq!(cases, count);
}

#[test]
fn ability_42_matches_original_finalization() {
    check("ability42 ", 912);
}

#[test]
fn connected_attacks_and_rng_consumption_match_original() {
    check("attack ", 176);
}
