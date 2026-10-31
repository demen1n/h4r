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

The deterministic corpus has 472 requests:

- 8 seeds × 256 consecutive global RNG outputs (RVA `36a70`), including unsigned
  overflow and the high bit; 2,048 individual numbers.
- 320 attack/defense coefficient cases (RVA `250a60`), including defense reduction,
  defense penalties/bonuses, zero defense, `0/0`, both clamps and fractional inputs.
  Float results are compared bit for bit after the original x87 return is stored
  to `f32`.
- 144 finalization cases (RVA `1ee540`): remaining HP, damage already dealt,
  zero HP, overkill and block rounding. Ability `0x42` is explicitly absent.

Virtual accessors are tiny fixture stubs returning supplied values. The actual
RNG, arithmetic, branches, limits and rounding execute from the original image.
Calls have an instruction budget and a stack-balance check. Modified accessor
code invalidates Unicorn's translation cache. No original function is replaced
with a Python implementation of the rule under test.

The report defaults to `target/testbench-report.json`, records the executable
hash and Unicorn version, and contains every mismatch with its input and both
answers. Differences cause exit status 1; unsupported images or execution errors
also fail the command. `--report PATH` selects another destination.

Verified on 31 October 2026: 472 requests, zero differences. The first run exposed
two one-ULP differences in the Rust damage coefficient: the original keeps the
denominator in x87 precision. The fix has a normal Rust regression test.

Scope still to add: the indexed **combat RNG** (`203280`), damage dice, the full
modifier chain (`1edf30`), ability `0x42`, and real creature/battlefield objects.
This is a function-level bench, not a replay or proof that full battles match.
The ordinary cross-platform CI does not require Unicorn or an original exe.
