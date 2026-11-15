"""Find direct `call rel32` / `jmp rel32` sites that reach given RVAs in heroes4.exe.

Usage: .venv/bin/python -I tools/xrefs.py <exe> <functions.txt> <rva> [rva ...]

`functions.txt` is the output of tools/ghidra/FunctionsInRange.java (rva, size, name per
line); it is used to say which function each call site belongs to. Virtual calls
(`call [reg+N]`) can't be found this way.
"""
import bisect
import sys

import pefile


def main():
    exe, funcs_path, *targets = sys.argv[1:]
    targets = {int(t, 16) for t in targets}
    pe = pefile.PE(exe, fast_load=True)
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    start = text.VirtualAddress
    data = text.get_data()
    funcs = []
    for line in open(funcs_path, encoding="utf-8"):
        rva, size, name = line.rstrip("\n").split("\t", 2)
        funcs.append((int(rva, 16), int(size, 16), name))
    funcs.sort()
    starts = [f[0] for f in funcs]

    def owner(r):
        i = bisect.bisect_right(starts, r) - 1
        return funcs[i] if i >= 0 else (0, 0, "?")

    for i in range(len(data) - 5):
        if data[i] not in (0xE8, 0xE9):
            continue
        rel = int.from_bytes(data[i + 1:i + 5], "little", signed=True)
        site = start + i
        dest = site + 5 + rel
        if dest in targets:
            f = owner(site)
            kind = "call" if data[i] == 0xE8 else "jmp"
            print(f"{dest:x} <- {kind} at {site:x} in {f[0]:x} (size {f[1]:x}) {f[2][:80]}")


if __name__ == "__main__":
    main()
