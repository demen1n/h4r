#!/usr/bin/env python3
"""Record native preset overlay branches; requires the SHA-checked Buka exe.

Executes the original switch inside RVA 162aa0, including the real 183780
setter. Stops before loop advancement. Context, input cell and preset byte
are fixture inputs; no original instructions are patched or substituted.
"""
import argparse
import itertools
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("h4_exe_reference", Path(__file__).with_name("compare.py"))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
Original, HEAP, STACK, BASE = reference.Original, reference.HEAP, reference.STACK, reference.BASE
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP,
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--vectors', type=Path, required=True)
    args = parser.parse_args()
    original = Original(args.exe)
    uc = original.uc
    cell, preset, sp = HEAP + 0xb000, HEAP + 0xb100, STACK + 0x8000
    rows = ['# Native RVA 162bc4..162c2b, rus-3.0 SHA-256 ' +
            '175eacb4838c86e5aee03f31e2406004a06dd238933517e2df40669cf17d36ff',
            '# code context terrain flags -> terrain flags']
    for code, context, terrain, flags in itertools.product(
            range(256), range(2), (0, 165), (0, 165, 255)):
        uc.mem_write(cell, bytes((terrain, flags)))
        uc.mem_write(preset, bytes((code,)))
        uc.mem_write(sp + 0x30, bytes((context,)))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ESI, cell)
        uc.reg_write(UC_X86_REG_EBP, preset)
        uc.emu_start(BASE + 0x162bc4, BASE + 0x162c2b, count=100)
        if (uc.reg_read(UC_X86_REG_EIP) != BASE + 0x162c2b or
                uc.reg_read(UC_X86_REG_ESP) != sp):
            raise RuntimeError('native branch failed to stop / stack imbalance')
        result = bytes(uc.mem_read(cell, 2))
        rows.append(' '.join(map(str, (code, context, terrain, flags, *result))))
    args.vectors.write_text('\n'.join(rows) + '\n')
    print('{} native preset cell vectors -> {}'.format(len(rows) - 2, args.vectors))


if __name__ == '__main__':
    main()
