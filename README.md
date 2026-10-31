# h4r

**A native engine reimplementation for Heroes of Might and Magic IV, written in Rust.**

English · [Русский](README.ru.md)

[![CI](https://github.com/demen1n/h4r/actions/workflows/ci.yml/badge.svg)](https://github.com/demen1n/h4r/actions/workflows/ci.yml)

h4r aims to run the original Heroes IV and its expansions on macOS, Linux, and Windows without Wine. The approach is similar to OpenMW: the engine reads the data from a copy of the original game that you own. Game assets are not included or distributed with this project.

The goal is to preserve the original rules and appearance. Convenience features and intentional changes to the original behavior belong behind separate options.

**This is an early development prototype. It cannot play a complete game yet.**

![A standalone map from the original game rendered by h4r: roads, terrain transitions, elevation, objects, and a hero](docs/screenshots/adventure-map.jpg)

<details>
<summary>Another map</summary>

![A town by the water, treasure, and a mounted hero on another original map](docs/screenshots/adventure-map-town.jpg)

</details>

The screenshots show original game data rendered by h4r. The data is read from a local installation and is not part of this repository.

### Status

| Area | State |
| --- | --- |
| Archives, sprites, animations, terrain | ✅ Read and decoded |
| Original maps (`.h4c`) | ✅ All 106 in the reference installation are read; scripts are not executed |
| Adventure map rendering | 🟡 Terrain, roads, elevation, objects, and armies; not yet compared pixel-for-pixel with the original |
| Movement and turns | 🟡 Movement costs, paths, new days, and neutral growth; no ships, skills, or weeks/months |
| Combat | 🔴 Automatic placeholder with the original damage formula; no battlefield |
| Towns, adventure objects, heroes | 🔴 Not yet: objects are shown but cannot be visited |
| AI, saves, sound, campaigns | 🔴 Not yet |

Current focus: verifying recovered rules against the original executable and moving from the placeholder to a real battle.

**Help is welcome.** Code review, reverse engineering, testing, documentation, and knowledge of Heroes IV all help move the project forward. See [Contributing](#contributing).

## Why

This started with a simple wish: I wanted to play Heroes IV on my Apple Silicon Mac. Under Wine and Rosetta 2, the game would freeze for one or two seconds whenever a new music track started, for example when a hero crossed onto another terrain type or visited a building.

The bottleneck turned out to be the old MP3 decoder in Miles Sound System. Its x87 floating-point code was extremely slow in this setup, and Miles decoded the start of each track synchronously, blocking the game.

With help from an AI coding agent, I wrote a drop-in replacement for `Mp3dec.asi`, the decoder plugin loaded as a Windows library. The replacement uses minimp3 with SSE2 instead of the original x87 decoder. In a test through the real Miles library on an M1 Pro, decoding a seven-second jingle dropped from 1,896 ms to 9 ms. Replacing the plugin removed those music-change stalls and made the game comfortable to play.

That work lives in a separate repository: **[h4-mp3dec-fast](https://github.com/demen1n/h4-mp3dec-fast)**, with the replacement plugin, measurements, and installation instructions. It fixes the original game under Wine; h4r takes the next step toward a native engine.

The long-term goal is a maintainable, open-source engine that keeps the game playable across platforms while using the player's existing game installation.

## Development with AI

I started this project with little programming experience and am learning Rust as I go. Much of the initial foundation was built with substantial help from AI coding agents: investigating the original engine, understanding file formats, writing the Rust implementation, tests, and documentation. An engine of this size would have been far beyond what I could investigate and implement quickly on my own. AI assistance made it possible to get started and reach a working prototype.

The project still needs experienced eyes. Independent review of the code and research, comparisons with the original game, and corrections to mistaken assumptions are especially welcome. Research notes record the evidence behind recovered behavior and the parts that still need verification.

## Current state

- Reads original `.h4r` archives, including resource aliases and archive priority.
- Decodes sprites, animations, adventure objects, terrain, and transition masks.
- Reads all 106 standalone `.h4c` maps in the development installation, including their objects, events, scripts, terrain, and heights. Reading scripts does not mean executing them.
- Displays real adventure maps with animated armies, roads, terrain transitions, and elevation.
- Implements an initial game state, army movement, turn changes, new days, neutral stack growth, and random object placement.
- Has a complete prototype flow from movement through an attack to updating the map and ending the turn.
- Reads combat animation definitions and provides a viewer using their direction mappings and frame rates.
- Includes 88 tests covering formats, malformed input, rules, and integration with locally installed game data.

**Combat is currently an automatic placeholder.** It uses reconstructed damage calculations and random number generators, but has no battlefield or player-controlled tactics. Heroes do not fight in it yet. The combat animation viewer is a separate tool, not a playable battle system.

There is no AI, full town or adventure-object interaction, playable campaign progression, saving/loading, or sound playback in the prototype yet. Multiplayer and a map editor are outside the initial release scope.

Reconstructed behavior is documented with its evidence and remaining uncertainties. Passing tests and successfully reading files do not by themselves prove exact compatibility with the original game.

## Platforms and game data

Development currently takes place on **macOS with Apple Silicon**. macOS, Linux, and Windows are the target platforms; builds and runtime behavior have not yet been validated across all three.

The reference installation is the **Russian Buka 3.0 release with The Gathering Storm and Winds of War**. Other editions, localizations, and mods have not been comprehensively tested.

You need:

- Your own legally acquired copy of Heroes of Might and Magic IV, with its files installed or extracted locally.
- Rust and Cargo. The current development toolchain is Rust **1.99.1**; a minimum supported version has not been established.
- A native build toolchain and a desktop graphics environment for the graphical programs.

The engine runs natively. Wine is not required by the engine; some research and comparison tools use the original Windows executable separately.

Keep the installation's directory structure, especially if you want to run the map tests:

```text
Heroes IV/
├── data/
│   ├── heroes4.h4r
│   └── ...
└── maps/
    ├── Some Map.h4c
    └── ...
```

Point h4r at the **`data` directory**, not the installation root. Data lookup uses a command-line path first, then `H4_DATA`. There is also a developer-specific fallback to a Sikarugir installation on macOS; set your own path explicitly.

## Build and run

Run these commands from the repository root.

### Adventure map prototype

```sh
cargo run --release -p h4-game -- "/path/to/Heroes IV/data"
```

By default this opens a generated test map, which also uses original game resources. To open a real standalone map, set `H4_MAP`.

macOS / Linux:

```sh
H4_MAP="/path/to/Heroes IV/maps/Some Map.h4c" \
  cargo run --release -p h4-game -- "/path/to/Heroes IV/data"
```

Windows PowerShell:

```powershell
$env:H4_MAP = "C:\Games\Heroes IV\maps\Some Map.h4c"
cargo run --release -p h4-game -- "C:\Games\Heroes IV\data"
```

The prototype can add a test army if the current player has none on the displayed level. It is a development scene, not a faithful playable start of the map yet.

| Control | Action |
| --- | --- |
| Left click | Move the selected army or attack an enemy |
| Right drag; arrows / WASD | Pan the map |
| Mouse wheel; `-` / `=` | Zoom |
| `Tab` | Select the next army |
| `Space` | Center on the selected army |
| `Enter` | End the turn |

Optional environment variables:

| Variable | Purpose |
| --- | --- |
| `H4_DATA` | Path to the game's `data` directory |
| `H4_MAP` | Path to a standalone `.h4c` map |
| `H4_LEVEL=1` | Open the underground level, if present |
| `H4_SEED=1` | Set the random seed; the default is `1` |
| `H4_START=col,row` | Place a test army near the given tile |
| `H4_FOOTPRINTS=1` | Outline object footprints |
| `H4_OVERVIEW=1` | Start with an overview of the map |
| `H4_SCREENSHOT=out.png` | Capture a rendered frame and exit; requires a graphics environment |
| `H4_ORDERS="10,30;end;tab"` | Execute scripted movement and turn commands; useful with screenshots |

### Battlefield resource viewer

```sh
cargo run --release -p h4-viewer --bin h4-battlefield -- "/path/to/Heroes IV/data"
```

Left/right selects a backdrop, up/down selects scenery, and clicking places its anchor (yellow cross). `Space` pauses animation, `S` toggles shadows, and `Esc` exits. Rendering preserves layer coordinates and the stored animation offset. `H4_SCREENSHOT=out.png` captures a frame and exits. This inspects resources; battle rules, automatic placement, and cell blocking are not implemented here.

### Animation viewer

```sh
cargo run --release -p h4-viewer --bin h4-viewer -- "/path/to/Heroes IV/data"
cargo run --release -p h4-viewer --bin h4-viewer -- "/path/to/Heroes IV/data" --combat
```

Use left/right to switch actors, up/down to switch actions, `Q`/`E` to rotate, and `Space` to pause. `,`/`.` step through frames, `[`/`]` adjust speed, `-`/`=` zoom, and `S` toggles shadows.

The combat mode follows references in `combat_actor` rather than inferring directions from filenames. Composite actions `full_walk` and `wait_and_base` are not assembled yet. Known missing resources are reported instead of silently replaced.

Export a combat animation as a PNG frame strip without opening a window:

```sh
cargo run --release -p h4-viewer --bin h4-dump -- \
  Griffin melee e griffin.png "/path/to/Heroes IV/data" --combat
```

## Tests

The [GitHub Actions workflow](.github/workflows/ci.yml) builds all workspace targets
and runs tests on macOS, Linux, and Windows with Rust 1.99.1 and `--locked`.
CI explicitly points `H4_DATA` at a nonexistent directory and shows skip messages
in the test log. It does not launch graphics windows or verify original game data.

```sh
cargo test --workspace
```

Tests that need original game files look for `H4_DATA` or the developer fallback and return early when the data directory is absent. Use `-- --nocapture` to see their skip messages. A successful run without game data does not validate resource compatibility.

With game data available, the tests also check archives, animations, map loading, and initial game states. Some assertions record exact counts and known anomalies from the reference installation, so a different edition or a modified installation may produce failures that need investigation.

Inspect combat definitions without a graphics window:

```sh
cargo run -p h4-formats --example check_combat_actors -- "/path/to/Heroes IV/data"
```

## Project layout

| Crate | Responsibility |
| --- | --- |
| `h4-formats` | Read and decode original game files |
| `h4-state` | Game state as plain data, without graphics or rules |
| `h4-rules` | Game rules and state transitions, without graphics |
| `h4-game` | Adventure map prototype, rendering, and input |
| `h4-viewer` | Animation viewer and PNG export tool |

Rules are kept separate from rendering so they can be tested and compared with the original executable independently.

Further documentation is currently mostly in Russian:

- [Development plan](PLAN.md) — milestones, scope, and next steps.
- [Format registry](FORMATS.md) — what is understood, how it was checked, and known limitations.
- [Research notes](decomp/) — descriptions of recovered formats and algorithms.
- [Development context](CLAUDE.md) — local setup and detailed implementation notes.

## Contributing

If you care about Heroes IV and want to help keep it playable, you are welcome here. You do not need to be a Rust developer to contribute.

- **Code and review:** improve the implementation, investigate bugs, or review existing readers and rules.
- **Reverse engineering:** document a format or mechanic, or verify an existing interpretation.
- **Testing:** try the prototype on another platform, edition, or localization and report reproducible problems.
- **Game knowledge:** describe how a mechanic behaves in the original, with steps to reproduce it.
- **Documentation and translation:** make the setup instructions and research easier to follow in English or Russian.

Open an issue with a finding or an idea, or send a small pull request. English and Russian are both welcome. Please discuss substantial changes first so we can agree on the scope and how to check the result.

Do not submit original game assets, executables, or decompiler output. Tests should use synthetic data or read game files from a local installation.

## License

h4r's original source code and documentation are licensed under the **GNU General Public License, version 3 or (at your option) any later version** (`GPL-3.0-or-later`). See [LICENSE](LICENSE) for the full text and [LICENSING.md](LICENSING.md) for scope and provenance notes.

Dependencies retain their own licenses; see the [dependency inventory](THIRD_PARTY.md). The original game's code, assets, and trademarks are not licensed by this project.
