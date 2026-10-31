//! Original-exe modifier results, captured by tools/testbench/compare.py.
#[path = "../examples/support/modifier_case.rs"]
mod modifier_case;

#[test]
fn modifier_chain_matches_original_vectors() {
    let mut cases = 0;
    for line in include_str!("modifier_vectors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let (request, expected) = line.split_once('\t').expect("fixture has two columns");
        let fields: Vec<_> = request.split_whitespace().collect();
        assert_eq!(fields[0], "modify");
        let got = modifier_case::evaluate(&fields[1..]).expect("valid fixture");
        assert_eq!(got, expected.parse::<i32>().unwrap(), "{request}");
        cases += 1;
    }
    assert_eq!(cases, 1321);
}
