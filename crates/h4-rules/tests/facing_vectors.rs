//! The facing arithmetic compared with answers recorded from the original exe
//! (`tools/testbench/facing.py`): a striker turning toward its victim (`1684a0`) and the
//! straight line of a flight or a teleport (`11ee50` + `11edb0`). Runs without the game.
use h4_rules::battle::facing::{sector, straight, toward};
use h4_rules::battle::path_prepare::angle;

fn int(field: &str) -> i32 {
    field.parse().unwrap()
}

#[test]
fn facing_matches_the_original() {
    let (mut strikes, mut lines_of_flight) = (0, 0);
    for line in include_str!("facing_vectors.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            // T x y size x y size -> direction
            "T" => {
                let size = |s: &str| s.parse::<u8>().unwrap();
                let got = toward((int(f[1]), int(f[2])), size(f[3]), (int(f[4]), int(f[5])), size(f[6]));
                assert_eq!(i32::from(got), int(f[8]), "{line}");
                strikes += 1;
            }
            // F x y x y -> angle direction
            "F" => {
                let (from, to) = ((int(f[1]), int(f[2])), (int(f[3]), int(f[4])));
                let degrees = angle((to.0 - from.0, to.1 - from.1));
                assert_eq!(degrees, int(f[6]), "angle of {line}");
                assert_eq!(i32::from(sector(degrees)), int(f[7]), "sector of {line}");
                assert_eq!(i32::from(straight(from, to)), int(f[7]), "{line}");
                lines_of_flight += 1;
            }
            other => panic!("unknown record {other}"),
        }
    }
    assert_eq!((strikes, lines_of_flight), (2000, 2000));
}
