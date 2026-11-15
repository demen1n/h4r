"""Disassemble a function of heroes4.exe by RVA, naming call targets.

Usage: .venv/bin/python -I tools/disasm.py <exe> <rva> [length] [--names names.tsv]

Without a length it stops at the first `ret` that is followed by padding (int3/nop) or
by the next function, which is good enough for MSVC 6 code. Call targets are labelled
with names from the homm4-symbols `names.tsv` (first column rva, fifth the mangled
name), so the call order of a function can be read without Ghidra's stack confusion.
"""
import sys

import capstone
import pefile


def load_names(path):
    names = {}
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                cols = line.rstrip("\n").split("\t")
                if len(cols) > 5:
                    try:
                        names[int(cols[0], 16)] = cols[5] or cols[4]
                    except ValueError:
                        pass
    except OSError:
        pass
    return names


def main():
    args = sys.argv[1:]
    names_path = "third_party/homm4-symbols/maps/rus-3.0/names.tsv"
    if "--names" in args:
        i = args.index("--names")
        names_path = args[i + 1]
        del args[i:i + 2]
    exe, rva = args[0], int(args[1], 16)
    length = int(args[2], 16) if len(args) > 2 else 0x4000
    pe = pefile.PE(exe, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    code = pe.get_data(rva, length)
    names = load_names(names_path)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    last_end = rva
    for ins in md.disasm(code, base + rva):
        r = ins.address - base
        # Stop after a ret when the next byte starts padding or another named function.
        if len(args) <= 2 and r > rva and r in names:
            break
        if len(args) <= 2 and r == last_end and ins.mnemonic in ("int3",) and r > rva:
            break
        note = ""
        if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
            target = int(ins.op_str, 16) - base
            note = f"  ; {target:x} {names.get(target, '')}"
        print(f"{r:6x}  {ins.mnemonic:6} {ins.op_str}{note}")
        last_end = r + ins.size


if __name__ == "__main__":
    main()
