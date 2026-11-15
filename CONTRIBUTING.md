# Contributing to h4r

Code, review, reverse engineering, testing, game knowledge, documentation, and
translations are welcome. Issues and discussions can be in English or Russian.
Use English for code, code comments, and commit messages. Documentation may be
in English or Russian; keep corresponding setup instructions consistent.

## Choose a task

Start with an existing issue or open one describing the problem and a way to
reproduce it. Discuss substantial changes before implementing them. Check open
pull requests to avoid duplicating work, and keep each PR focused on one task.
Create a separate branch from `main`; use a separate worktree when another person
or agent is working in the same checkout.

The goal is to preserve the original game's rules and appearance. Intentional
behavior changes and convenience features belong behind separate options.

## Build and game data

Install Rust and Cargo plus your platform's native build tools. Development and
CI currently use Rust **1.99.1**; there is no established minimum supported Rust
version. From the repository root:

```sh
rustup toolchain install 1.99.1 --profile minimal --component clippy
cargo +1.99.1 build --workspace --all-targets --locked
```

Compilation and synthetic tests do not require the original game. To run the
prototype or resource-dependent tests, use your own legally acquired installation.
The reference edition is Russian Buka 3.0 with The Gathering Storm and Winds of
War; other editions and mods need separate verification. h4r does not provide
game data or download links for it.

Set `H4_DATA` to the installation's **data directory**, containing `heroes4.h4r`,
not its parent. Keep the sibling `maps` directory for map tests. Set the path
explicitly to avoid the developer-specific macOS fallback.

macOS / Linux:

```sh
export H4_DATA="/path/to/Heroes IV/data"
cargo +1.99.1 run --release -p h4-game -- "$H4_DATA"
```

Windows PowerShell:

```powershell
$env:H4_DATA = "C:\Games\Heroes IV\data"
cargo +1.99.1 run --release -p h4-game -- $env:H4_DATA
```

See [README.md](README.md#build-and-run) and
[README.ru.md](README.ru.md) for viewers, map selection, and controls.

## Implementation and evidence

- Keep file readers in `h4-formats`, owned game state in `h4-state`, and rules
  without graphics in `h4-rules`. Graphical applications live in `h4-game` and
  `h4-viewer`.
- Prefer readable code and comments that explain why. File-dependent library
  operations return `Result`; malformed input must not panic. Avoid `unwrap`
  in library code; use `expect` with an invariant explanation where failure is
  impossible. Tests and examples may use `unwrap`.
- Add focused regression tests for changed behavior. Use synthetic inputs or
  load resources from the contributor's local installation.
- Record recovered formats and mechanics in `FORMATS.md` or `decomp/*.md`.
  Describe the executable version, addresses, observations, fixture boundaries,
  and remaining assumptions so another contributor can check the conclusion.
  Distinguish evidence from guesses and visual inspection from exact comparison.
- Python tools support Python 3.9. Run standalone project scripts with
  `python3 -I`; avoid syntax or standard-library APIs requiring newer versions.

For optional comparisons against a supported local executable, follow
[the comparison bench instructions](tools/testbench/README.md). These checks
require separate Python dependencies and are not part of ordinary CI.

## Names of functions and the symbol map

A leaked linker map of a debug build circulates among researchers. It may be used
locally as a hint for where to look, but its names are not a source for this
repository:

- **Code.** Name things by what they do, in your own words. A descriptive name
  that happens to match the map (`can_retaliate`, `begin_attack`) is fine: a short
  functional name is not copied expression, and the code behind it is our own,
  written from observed behavior. Do not open the map to decide what to call
  something, and do not carry over its structure of names wholesale.
- **Research notes** (`decomp/*.md`, `FORMATS.md`, comments). Refer to a function
  by its RVA in the supported executable plus our own description, for example
  "start of a melee attack (`16ede0`)". Do not mark names as coming from the map,
  do not quote its mangled names or signatures, and do not list its method names
  next to addresses.
- **Class names** found in the executable itself (RTTI type descriptors such as
  `t_battlefield`, `t_combat_creature`, source-file names in assertion strings)
  are observations of the supported executable and may be used.
- **Tools** may read a local copy of the map or a name table generated from it
  (for example to label disassembly), but the map, its contents and generated
  tables stay local, and their output is not committed.

## What must stay local

Do not commit or attach to issues or PRs:

- Original game archives, extracted assets, maps, saves, executables, or other
  proprietary binaries.
- Raw decompiler output or disassembly. Keep these local; submit original
  research notes describing observations instead.
- The leaked linker symbol map, its contents, generated name tables, or names
  copied from it. Local research tools and ignored third-party directories are
  not permission to redistribute those materials.
- Personal installation paths, credentials, or local environment files.

Review `git diff --cached` before committing. Ignore rules are a convenience,
not a complete check of what is suitable to publish. Small synthetic fixtures
and original notes should make changes reviewable without distributing game
files. If a bug only occurs with original data, report the edition, resource
name, steps, and error message without attaching the resource itself.

Original contributions use `GPL-3.0-or-later`. Read
[LICENSING.md](LICENSING.md) for scope and provenance and
[THIRD_PARTY.md](THIRD_PARTY.md) for dependency licenses. Document the source of
any copied or adapted code, preserve notices, and check license compatibility.
A public repository without a license does not grant permission to copy its code.
Update the dependency inventory when adding or changing dependencies.

## Validate and open a pull request

Run the checks relevant to the change. For Rust changes, match CI:

```sh
cargo +1.99.1 build --workspace --all-targets --locked
cargo +1.99.1 test --workspace --locked -- --nocapture
cargo +1.99.1 clippy --workspace --all-targets --locked -- -D warnings
```

CI runs builds and tests on Linux, macOS, and Windows and Clippy on Linux. It
sets `H4_DATA` to a nonexistent directory: resource-dependent tests print skip
messages and return early, so Cargo reports them as passed. Green CI does not
verify original-resource compatibility or graphical startup. Report separately
which checks used game data and which graphical checks you performed. For a
documentation-only change, check links, commands, and consistency with the code;
a full Rust test run is usually unnecessary.

Open a PR targeting `main` with:

- The problem and resulting behavior; link the issue (`Closes #123` when fully
  addressed).
- The evidence behind recovered behavior, plus any assumptions or limitations.
- Validation commands and results, including skipped checks and the platform
  and game edition where relevant.

Keep generated files and unrelated edits out of the diff. Changes enter `main`
through a reviewed PR after CI passes. Address review feedback in the same
branch; do not merge just because local tests pass.
