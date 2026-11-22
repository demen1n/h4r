#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the facing of combat units.

Executes, whole, the two pieces of arithmetic that turn a unit (decomp/facing.md):

- 1684a0 (cdecl: who, who's corner x, y, whom, whom's corner x, y): both footprint
  centers in 1/65536 of a cell (corner << 16 + size << 15) and the direction 11ede0
  between them, the turn of a striker toward its victim. Fixture boundary: the
  footprint size (virtual +8 of each object), a supplied value.
- 11ee50 (angle of the line from one 3d point to another, whole degrees) followed by
  11edb0 (get_direction: angle -> one of 8 directions), as the flight animation 3dd6f0
  does with the creature's position (+0xc) and the landing point, both in 1/16 of a cell.

No fixture code stands in for the rules: the x87 atan2, __ftol and the sector
arithmetic run from the original image.
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

HEAP = 0x7000000
WHO, WHOM, POINTS = HEAP, HEAP + 0x100, HEAP + 0x200


def signed(value):
    return value - (1 << 32) if value >= 1 << 31 else value


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.o.uc.mem_map(HEAP, 0x1000)

    def toward(self, who, whom):
        """1684a0 on two (x, y, size) footprints."""
        o = self.o
        for slot, obj, (_, _, size) in ((0, WHO, who), (1, WHOM, whom)):
            vtable = obj + 0x80
            o.word(obj, vtable)
            o.word(vtable + 8, o.accessor(slot, size, pop=0))
        args = (WHO, who[0] & 0xffffffff, who[1] & 0xffffffff, WHOM, whom[0] & 0xffffffff, whom[1] & 0xffffffff)
        return int(self.o.call(0x1684a0, args, this=0, callee_pop=False))

    def straight(self, start, end):
        """11ee50 then 11edb0 on two 3d points (z is ignored by both)."""
        o = self.o
        o.uc.mem_write(POINTS, struct.pack('<iii', *start, 0) + struct.pack('<iii', *end, 0))
        angle = signed(int(o.call(0x11ee50, (POINTS, POINTS + 12), this=0, callee_pop=False)))
        direction = int(o.call(0x11edb0, (angle & 0xffffffff,), this=0, callee_pop=False))
        return angle, direction


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=2000)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(1930)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# T x y size x y size -> direction: 1684a0, who turns toward whom (corners in cells)',
             '# F x y x y -> angle direction: 11ee50 + 11edb0, from a point to a point (1/16 of a cell)']

    def cell():
        return rng.randrange(-4, 106)

    for i in range(args.count):
        if i % 4 == 0:
            # Neighbours and near neighbours: the usual case of a strike.
            x, y, size = cell(), cell(), rng.randint(1, 8)
            who = (x, y, size)
            whom = (x + rng.randint(-size - 8, size + 8), y + rng.randint(-size - 8, size + 8), rng.randint(1, 8))
        else:
            who = (cell(), cell(), rng.randint(1, 8))
            whom = (cell(), cell(), rng.randint(1, 8))
        lines.append('T {} {} {} {} {} {} -> {}'.format(*who, *whom, bench.toward(who, whom)))

    def point():
        # Usually cell corners; sometimes anywhere inside a cell.
        if rng.random() < 0.7:
            return rng.randrange(-4, 106) * 16
        return rng.randrange(-64, 106 * 16)

    for i in range(args.count):
        start = (point(), point())
        if i % 5 == 0:
            # Exact diagonals and straight lines: the sector boundaries of atan2.
            d = rng.randint(0, 40) * rng.choice((1, 16))
            end = (start[0] + d * rng.choice((-1, 0, 1)), start[1] + d * rng.choice((-1, 0, 1)))
        else:
            end = (point(), point())
        angle, direction = bench.straight(start, end)
        lines.append('F {} {} {} {} -> {} {}'.format(*start, *end, angle, direction))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe facing fixtures'.format(2 * args.count))


if __name__ == '__main__':
    main()
