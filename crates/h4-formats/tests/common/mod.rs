//! Shared helpers for the tests that read the real game data.
//!
//! The game files are not in the repository (they come from the player's own copy), so these
//! tests look for them the same way the tools do (`$H4_DATA`, then the Sikarugir wrapper) and
//! skip themselves when there is nothing there. That keeps `cargo test` green on CI machines
//! without the game, while a developer with the game gets the full check.

use h4_formats::archive::{data_dir, Resources};

/// Open the game's archives, or `None` (after printing why) if the game isn't installed.
pub fn resources() -> Option<Resources> {
    let dir = data_dir(None);
    if !dir.is_dir() {
        println!("skipped: no game data at {}", dir.display());
        return None;
    }
    // The folder exists, so a failure to open it is a real bug, not a missing install.
    Some(Resources::open(&dir).unwrap_or_else(|e| panic!("open {}: {e}", dir.display())))
}

/// Visible resource names starting with `prefix` (case-insensitive), sorted for stable output.
// Each test file compiles this module separately; `maps.rs` does not need this helper.
#[allow(dead_code)]
pub fn names_with_prefix<'a>(res: &'a Resources, prefix: &str) -> Vec<&'a str> {
    let mut names: Vec<&str> = res.names().filter(|n| n.to_lowercase().starts_with(prefix)).collect();
    names.sort();
    names
}

/// Panic with up to 15 of the collected problems, so one test run shows many at once
/// instead of stopping at the first.
pub fn assert_no_failures(what: &str, failures: &[String]) {
    if !failures.is_empty() {
        let shown: Vec<&str> = failures.iter().take(15).map(String::as_str).collect();
        panic!("{} {what} failed:\n  {}", failures.len(), shown.join("\n  "));
    }
}
