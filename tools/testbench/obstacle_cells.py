#!/usr/bin/env python3
"""Record native footprint masks and stationary-cell blocking, without game I/O."""
import argparse
import importlib.util
import itertools
import struct
from pathlib import Path

spec = importlib.util.spec_from_file_location('h4_reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--footprints', type=Path, required=True)
    parser.add_argument('--blocking', type=Path, required=True)
    parser.add_argument('--centers', type=Path, required=True)
    args = parser.parse_args()
    original = reference.Original(args.exe)
    uc, heap = original.uc, reference.HEAP
    shape = heap + 0x100
    masks = ['# Native RVA 1fc340, SHA-256 ' + reference.SHA256, '# size byte-mask-hex (- for empty)']
    for size in list(range(17)) + [32]:
        original.call(0x1fc340, (size,), this=shape)
        mask = bytes(uc.mem_read(original.read_word(shape + 8), size * size)) if size else b''
        masks.append('{} {}'.format(size, mask.hex() or '-'))
    args.footprints.write_text('\n'.join(masks) + '\n')

    root, segment, group = heap + 0x1000, heap + 0x1200, heap + 0x1400
    obstacle, other, other_model = heap + 0x2000, heap + 0x2200, heap + 0x2400
    cell, first, second = heap + 0x3000, heap + 0x3100, heap + 0x3200
    # The real root-copy constructor maps resource attributes into segment +0x70.
    # Virtual methods +0x10/+0x5c use that copy, or the compound's override.
    for obj, model in ((obstacle, segment), (other, other_model)):
        original.word(obj, reference.BASE + 0x582c24)
        original.word(obj + 0xa0, model)
    original.word(other + 0x9c, 0)
    uc.mem_write(other_model + 0x74, b'\x01')
    uc.mem_write(other_model + 0x8c, b'\x00')
    uc.mem_write(cell, bytes(0x1c))
    original.call(0x183560, this=cell)
    sentinel = original.read_word(cell + 8)
    rows = ['# Native RVA 239200 + virtual accessors + 183a10, SHA-256 ' + reference.SHA256,
            '# attribute0 flag compound-override(-1 absent) query(0 none/1 self/2 other) second flags -> blocked']
    for attr, flag, override, query, extra, flags in itertools.product(
            (0, 1, 36, 255), (0, 1, 255), (-1, 0, 1), range(3), range(2), (0, 4)):
        uc.mem_write(root + 4, bytes((attr,)))
        uc.mem_write(root + 0x1c, bytes((flag,)))
        original.call(0x239200, (root,), this=segment + 0x70)
        original.word(obstacle + 0x9c, 0 if override == -1 else group)
        uc.mem_write(group + 0x25, bytes((max(override, 0),)))
        original.word(sentinel, first)
        original.word(first, second if extra else sentinel)
        original.word(first + 8, obstacle)
        original.word(second, sentinel)
        original.word(second + 8, other)
        uc.mem_write(cell + 1, bytes((flags,)))
        pointer = (0, obstacle + 8, other + 8)[query]
        # The x86 bool return occupies AL; other EAX bytes are unspecified.
        result = int(original.call(0x183a10, (pointer,), this=cell)) & 255
        if result not in (0, 1):
            raise RuntimeError('unexpected bool return')
        rows.append(' '.join(map(str, (attr, flag, override, query, extra, flags, result))))
    args.blocking.write_text('\n'.join(rows) + '\n')
    # Full-size viewport with identity scaling; native integer projection runs.
    field, point, output = heap + 0x4000, heap + 0x5000, heap + 0x5100
    for address, value in ((0xaa1c10, 1180), (0xaa1c14, 1024), (0xaa1cb8, 1),
                           (field + 0x68, 102), (field + 0x50, 0), (field + 0x520, 2)):
        original.word(address, value)
    uc.mem_write(0xaa1ca0, struct.pack('<6i', 1180, 1024, 1, 1, 1, 1))
    centers = ['# Native RVA 164b40, 1180x1024 identity viewport, SHA-256 ' + reference.SHA256,
               '# x y -> screen-x screen-y (subcell center = 16*coordinate+8)']
    for x in range(102):
        start = max(37-x, x-37, 0)
        end = max(start, min(38+x, 166-x, 102))
        for y in range(start, end):
            uc.mem_write(point, struct.pack('<3i', 16*x+8, 16*y+8, 0))
            original.call(0x164b40, (output, point), this=field)
            screen = struct.unpack('<2i', bytes(uc.mem_read(output, 8)))
            centers.append(' '.join(map(str, (x, y, *screen))))
    args.centers.write_text('\n'.join(centers) + '\n')
    print('{} native masks, {} blocking cases, {} cell centers'.format(
        len(masks) - 2, len(rows) - 2, len(centers) - 2))


if __name__ == '__main__':
    main()
