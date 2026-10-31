use h4_rules::effects::Effectiveness;

#[test]
fn original_effectiveness_vectors() {
    let mut count = 0;
    for line in include_str!("effectiveness_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (input, expected) = line.split_once('\t').unwrap();
        let v: Vec<i32> = input
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        let effect = Effectiveness {
            eligible: v[0] != 0,
            resisted: v[1] != 0,
            resisted_between_sides: v[2] != 0,
            same_side: v[3] != 0,
            bypass_resistance: v[4] != 0,
            generic_resistance_only: v[5] != 0,
            effect_resistance: v[6],
            generic_resistance: v[7],
        };
        assert_eq!(
            effect.percent(),
            expected.parse::<i32>().unwrap(),
            "{input}"
        );
        count += 1;
    }
    assert_eq!(count, 320);
}
