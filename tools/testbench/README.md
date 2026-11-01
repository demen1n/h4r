# Original-exe comparison bench

Runs the original rus-3.0 x86 machine code in Unicorn and compares its answers
with the public `h4-rules` functions, through the `exe_reference` example.
No translated C, game resources, or executable bytes are included here.

```sh
python3 -m venv .venv
.venv/bin/pip install -r tools/testbench/requirements.txt
.venv/bin/python tools/testbench/compare.py --exe bin/heroes4.exe
```

Requires Python 3.9+, Cargo, and a legally obtained, supported `heroes4.exe`.
The executable is identified by SHA-256 before any address is used. Other builds
are rejected. No Windows loader, imports, game initialization or Wine is needed.
On a restricted macOS sandbox Unicorn may abort during CPU/cache detection;
run the command with the sandbox's normal execution approval.
The harness executes the original CRT precision initializer (RVA `4f2ee8`) before
calling game functions: x87 uses 53-bit precision, as selected at exe startup
(`4ed10b` -> `4f2ee8` -> `4f8e99`). The FPU starts with reset control word `0x037f`; the CRT changes it to
`0x023f` (53-bit precision, nearest rounding, masked exceptions), which is checked
and recorded in the report. Skipping this initialization produces misleading differences.

The deterministic corpus has 11,249 requests:

- 8 seeds × 256 consecutive global RNG outputs (RVA `36a70`), including unsigned
  overflow and the high bit; 2,048 individual numbers.
- 320 attack/defense coefficient cases (RVA `250a60`), including defense reduction,
  defense penalties/bonuses, zero defense, `0/0`, both clamps and fractional inputs.
  Float results are compared bit for bit after the original x87 return is stored
  to `f32`.
- 144 finalization cases (RVA `1ee540`): remaining HP, damage already dealt,
  zero HP, overkill and block rounding. Ability `0x42` is explicitly absent.
- 16 combat RNG sequences (RVA `203280`): four seeds × four ID moduli,
  repeated and colliding keys, swapped IDs, purpose 29/30 boundary, unsigned IDs,
  small indexes after cached ones, reset to the same and a different seed.
- 48 damage dice sequences (RVA `1ee240`): two seeds, counts 0/1/5/9/10/11/19/1000,
  three damage ranges (including fixed damage), three consecutive hits and the
  next RNG number to verify consumption.

- 1,321 full modifier cases (RVA `1edf30`): charging at movement thresholds,
  melee/ranged giant slaying, effect `0x8c`, every luck multiplier and mode,
  all schools and ward sources, fire/cold/area attacks, defense reduction,
  clamps and zero damage, 625 integer-boundary cases and 128 deterministic
  mixed-effect cases.

- 912 ability-0x42 finalization cases (RVA `1ee540`): threshold, effectiveness
  truncation, target creature HP, all excluded abilities and combinations,
  non-creature targets, effect eligibility, HP caps, already dealt damage, block.
- 176 connected numerical attack sequences: three original `1ee240` calls
  (executing real `1edf30`), each followed by `1ee540` on the same fixture
  objects/RNG, plus the next RNG output. Includes melee/ranged and combined effects.

- 320 effectiveness cases (RVA `1f1de0`): flags, side equality, abilities
  0x52/0x1f and signed resistance edge values.
- 7,088 immunity scenarios: each executes general immunities (`1b3540`),
  target-kind immunities (`1b3a10`), the effect-0x98 branch of `1b4770`,
  and the full-resistance eligibility gate (`1b5750`). Four outputs per scenario.

- 896 connected stone-gaze scenarios: native eligibility wrapper/pipeline
  (`1b6140`/`1b30a0`), effectiveness (`1f1de0`) and final damage (`1ee540`).
  Unported target-state/artifact/side/refresh checks remain fixture boundaries.

Virtual accessors are tiny fixture stubs returning supplied values. The actual
RNG, arithmetic, branches, limits and rounding execute from the original image.
Calls have a 30-second execution timeout and a stack-balance check. Modified accessor
code invalidates Unicorn's translation cache. No original function is replaced
with a Python implementation of the rule under test.

Combat RNG construction (`2031b0`), reseeding (`203430`), std::map insertion,
key comparison, vector growth and checkpoints execute from the original image.
The harness substitutes a bounded arena for `new`, ignores `delete` until the
scenario ends, and treats CRT mutex lock/unlock as no-ops in this single-threaded
fixture. It does not substitute a Python counter map or RNG cache.

For the dice-only fixture, dynamic_cast returns the supplied creature target and
`modify_damage` returns its input base damage unchanged. This deliberately
observes the original dice sum before modifiers; it does not verify the modifier
chain. Creature low/high damage and IDs are fixture values.

The report defaults to `target/testbench-report.json`, records the executable
hash, Unicorn version and x87 control word, and contains every mismatch with its input and both
answers. It also stores the original answers for combat/dice/modifier/finalization/attack cases in
`vectors`. Differences cause exit status 1; unsupported images or execution errors
also fail the command. `--report PATH` selects another destination.

Verified on 31 October 2026: 11,249 requests, zero differences. The first run exposed
two one-ULP differences in the Rust damage coefficient: the original keeps the
denominator in x87 precision. The fix has a normal Rust regression test.

The expanded run exposed a cache quirk: with ID modulus 1, a request below index
128 after a cached request returns the number after the former last checkpoint,
then clears the checkpoint vector. Rust now preserves it. This makes some rolls
order-dependent; whether live battles can use modulus 1 remains unknown.
All 64 original combat/dice vectors are stored in
`crates/h4-rules/tests/exe_vectors.tsv`; two ordinary Rust tests verify them without
Unicorn or game data, including reseeding and RNG consumption.

The modifier fixture executes `1edf30`, its nested `250a60` coefficient, luck
lookup (`17b8f0`) and the CRT float-to-integer conversion (`4ed234`) from the exe.
Only virtual input accessors (stats, abilities, effects) are fixture stubs. In this
mode the dice fixture's modifier interception is disabled.
The coefficient must stay at 53-bit precision until scaled damage is truncated;
rounding it to `f32` first caused 159 differences in this corpus (e.g. damage 6,
attack 5, defense 6: 4 instead of the original 5). `damage_modifier` still returns
`f32` for callers needing a stored coefficient; damage scaling uses the precise
internal calculation. A minimal unit test and all 1,321 original answers in
`tests/modifier_vectors.tsv` prevent regression.

Use `--only modify` (or `global`, `combat`, `dice`, `ratio`, `final`, `ability42`, `attack`) for a focused
run; it applies the same CRT initialization. A focused report contains only those
cases and replaces the selected report file.

Ability 0x42 is now implemented and verified via `finalize_attack_damage`.
Its bonus uses **target creature HP**, not attacker HP: after eligibility and
excluded-target checks, scale effect power by effectiveness, require power ×
attacker count >= 500, add the HP-based bonus, then cap and block.
The numerical pipeline is `attack_hit`; the battle stand-in now uses it.
New original answers are in `tests/attack_vectors.tsv` (912 finalization + 176
attack sequences), including a separate event-order unit test.

These fixtures supply dynamic-cast success and the result of the effect-0x98
eligibility check (`1b6140`). The full spell-targeting subsystem is not replaced
by guessed Rust rules: `Ability42Target` explicitly takes resolved eligibility
and effectiveness. The native `1f1de0` resistance helper executes with a supplied
resisted flag and virtual resistance; resistance-bypass abilities 0x52/0x1f are
not part of this fixture. No arithmetic under test is stubbed.

Connected attacks compose two original functions on shared fixture state; they
do not run the full battlefield attack dispatcher or apply deaths/retaliation.
Target HP and already-dealt inputs stay fixed over each three-hit sequence so
that RNG progression and the numerical chain can be compared independently.
Scope still to add: spell-targeting/immunity rules, real creature/battlefield
objects, ranged penalties, luck selection, and full attack side effects.
This is a function-level bench, not a replay or proof that full battles match.
The ordinary cross-platform CI does not require Unicorn or an original exe.

Effectiveness helper: `--only effectiveness` compares 320 cases of RVA `1f1de0`
(flags, sides, abilities 0x52/0x1f, specific versus generic resistance, i32 edges).
Eligibility and resistance accessors remain explicit fixture inputs. Expected
answers are retained in `h4-rules/tests/effectiveness_vectors.tsv` for CI.

Use `--only immunity` for the four independent eligibility filters. The fixture
supplies effect-0x98 record flags, ability/effect lists, side accessors and target
resistance. The original functions calculate each filter outcome; `is_hostile`
and flag checks execute natively. Localized diagnostic strings are marked
initialized and diagnostic output is disabled; a minimal SEH chain supports the
original no-output diagnostic helper. This does not test the full `1b30a0`
pipeline, artifact/side/refresh filters, record loading or resistance derivation.
All 7,088 original output rows live in `h4-rules/tests/immunity_vectors.tsv`.

Use `--only stone_gaze` for the `StoneGazeContext` bridge. Unlike the arithmetic
fixtures, this executes native `1b6140` and `1b30a0` with the four real filters.
Only unported checks are supplied: initial target state is allowed, artifact
lookup rejects nothing, refresh is allowed, and the side filter returns the
explicit remaining-eligibility input. Actual flags/ability/effect/side/resistance
accessors are still fixture data. The bench compares eligibility, effectiveness
and finalized damage, without damage dice/modifiers or the full attack command.
All 896 original answers are in `h4-rules/tests/stone_gaze_vectors.tsv`.
Unknown remaining eligibility has a separate Rust test and yields `None`, keeping
the no-bonus behavior of the stand-in battle until spell data is available.

Preset cell overlays have a separate focused fixture:

```sh
.venv/bin/python -I tools/testbench/preset_cells.py --exe bin/heroes4.exe \
  --vectors /tmp/preset_cell_vectors.tsv
```

It executes the full native context constructor (`40e5e0`) with an adventure
position, followed by full preset application (`162aa0`). Context getters,
reference counting, geometry checks, row traversal and the terrain setter
execute from the unmodified image. Only the supported platform allocator is
substituted by the usual bounded arena. The fixture supplies a synthetic
single-cell row, two army pointers set to null, and no town or garrison.
The constructor copies the position's bridge flag into context +0x20;
the initializer reads that actual context rather than a supplied switch input.
All 256 codes, both bridge modes, two initial terrain bytes and three initial
flag bytes produce 3,072 vectors. Stack and reference balances are checked.
Compare the output with `crates/h4-rules/tests/preset_cell_vectors.tsv`.
The normal Rust integration test checks these stored native answers in CI.
This verifies context-to-cell application, not the generation of the initial
field or a real 102-row preset. See `decomp/battlefield.md` for the bridge flag's
provenance and remaining limits.

Stationary obstacles and cell centers:

```sh
.venv/bin/python -I tools/testbench/obstacle_cells.py --exe bin/heroes4.exe \
  --footprints /tmp/footprint_vectors.tsv \
  --blocking /tmp/obstacle_blocking_vectors.tsv \
  --centers /tmp/battlefield_center_vectors.tsv
```

The fixture runs the real footprint constructor (`1fc340`, 18 sizes), root
copy constructor (`239200`), stationary-object virtual accessors (`477620`,
`477660`) and cell blocking query (`183a10`, 432 cases). It uses original
vtable pointers, explicit segment/compound inputs and linked-list fixtures;
none of these accessors are stubs. Dynamic compound changes, image-dependent
segmentation and automatic placement are outside this fixture.

It also executes the native projection (`164b40`) for all 4,838 stored cell
centers with a full-size 1180x1024 identity viewport. This caught an 8-pixel
vertical error: projecting integer map coordinates gives a cell vertex;
the actual center has subcell coordinates `(16*x+8,16*y+8)`.
All three output files are retained under `crates/h4-rules/tests/` and checked
by `battlefield_grid.rs` without the exe in CI.


### Compound obstacle images

```sh
.venv/bin/python tools/testbench/obstacle_segments.py --exe bin/heroes4.exe \
  --output crates/h4-formats/tests/fixtures/obstacle_segments.txt
cargo test -p h4-formats --test obstacle_segments --locked
```

Runs the full native RVA 238690 at scale 1.0 on 200 synthetic one/two-layer
fixtures, including real bitmap conversion and scanline cropping. Records segment
order, size, cell position, animation offset, layer rectangles and normalized
index/alpha coverage hashes. Only the existing allocator/platform hooks are used;
segmentation and image operations execute in the original code. Synthetic images
and returned values can be checked into the repository. No game resources are
stored in the vectors. See `decomp/combat_object.md` for remaining boundaries.
