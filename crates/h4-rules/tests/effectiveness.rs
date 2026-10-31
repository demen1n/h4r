//! Original answers are checked through the same adapter used by the live exe bench.
#[path = "../examples/support/effectiveness_case.rs"]
mod effectiveness_case;

#[test]
fn original_effectiveness_vectors() {
    let mut count = 0;
    for line in include_str!("effectiveness_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (input, expected) = line.split_once('\t').unwrap();
        let fields: Vec<_> = input.split_whitespace().skip(1).collect();
        let actual = effectiveness_case::evaluate(&fields).expect("valid fixture");
        assert_eq!(actual, expected.parse::<i32>().unwrap(), "{input}");
        count += 1;
    }
    assert_eq!(count, 320);
}
