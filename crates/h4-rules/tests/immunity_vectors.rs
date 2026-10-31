//! Filter outputs captured from original machine code, independently of Rust rules.
#[path = "../examples/support/immunity_case.rs"]
mod immunity_case;

#[test]
fn immunity_filters_match_original() {
    let mut count = 0;
    for line in include_str!("immunity_vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (input, expected) = line.split_once('\t').expect("fixture has two columns");
        let fields: Vec<_> = input.split_whitespace().skip(1).collect();
        let result = immunity_case::evaluate(&fields).expect("valid fixture");
        assert_eq!(result, expected, "{input}");
        count += 1;
    }
    assert_eq!(count, 7088);
}
