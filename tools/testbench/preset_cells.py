#!/usr/bin/env python3
"""Record native preset overlays with the real adventure-position context.

Requires the SHA-checked Buka exe. Runs the native context constructor (40e5e0)
and preset application (162aa0), including context access and row iteration.
The fixture supplies a synthetic one-cell row and an adventure position;
no original instructions or context accessors are patched or substituted.
"""
import argparse
import itertools
import importlib.util
import struct
from pathlib import Path

spec = importlib.util.spec_from_file_location("h4_exe_reference", Path(__file__).with_name("compare.py"))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
Original, HEAP = reference.Original, reference.HEAP


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--vectors', type=Path, required=True)
    args = parser.parse_args()
    original = Original(args.exe)
    uc = original.uc
    field, window, context = HEAP + 0x100, HEAP + 0x1000, HEAP + 0x2000
    vector, entries, game, preset = (HEAP + i for i in (0x3000, 0x3100, 0x3200, 0x3300))
    rowvec, bounds, cell, codeptr, position = (HEAP + i for i in (0x3400, 0x3500, 0x3600, 0x3700, 0x3800))
    rows = ['# Native RVA 40e5e0 + 162aa0, rus-3.0 SHA-256 ' + reference.SHA256,
            '# code on_bridge terrain flags -> terrain flags']
    for code, on_bridge, terrain, flags in itertools.product(
            range(256), range(2), (0, 165), (0, 165, 255)):
        # The actual constructor copies all four adventure-position words.
        # Null army/town inputs are accepted by this constructor, without I/O.
        uc.mem_write(position, struct.pack('<4I', 17, 29, 1, on_bridge))
        original.call(0x40e5e0, (0, 0, position, 0, 0), this=context)
        if bytes(uc.mem_read(context + 0x14, 16)) != bytes(uc.mem_read(position, 16)):
            raise RuntimeError('native constructor did not preserve the adventure position')
        # The window owns a reference; native getters temporarily retain it.
        # Live cell and preset have identical synthetic one-row geometry.
        for address, value in (
                (context + 4, 1), (window + 0x140, context), (field + 0xc0, window),
                (field + 0x68, 1), (field + 0x7c, rowvec), (field + 0x88, cell),
                (rowvec + 8, bounds), (bounds, 0), (bounds + 4, 0), (bounds + 8, 1),
                (vector + 4, entries), (vector + 8, entries + 8), (entries + 4, game),
                (game + 8, preset), (preset + 4, 1), (preset + 8, 1),
                (preset + 0x1c, rowvec), (preset + 0x28, codeptr)):
            original.word(address, value)
        uc.mem_write(cell, bytes((terrain, flags)) + bytes(26))
        uc.mem_write(codeptr, bytes((code,)))
        original.call(0x162aa0, (vector,), this=field)
        if original.read_word(context + 4) != 1 or original.read_word(preset + 4) != 1:
            raise RuntimeError('native context/preset reference imbalance')
        result = bytes(uc.mem_read(cell, 2))
        rows.append(' '.join(map(str, (code, on_bridge, terrain, flags, *result))))
    args.vectors.write_text('\n'.join(rows) + '\n')
    print('{} native preset cell vectors -> {}'.format(len(rows) - 2, args.vectors))


if __name__ == '__main__':
    main()
