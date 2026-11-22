#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the castle wall check of a melee strike (1701f0).

Executes 1701f0 whole on a 102x102 battlefield (rows as the exe lays them out, cells of
0x1c bytes at +0x88): the line from one point to another (1/16 of a cell) walked cell by
cell, the obstacle type read by the real 477790. Fixture boundaries:
t_battlefield_cell::get_obstacle (1838d0) answers a fixture obstacle of type 0x24
(castle_wall), 0x22 (castle_gate) or another type per cell, and t_castle_gate::is_open
(1c0570) the fixture's gate state; the gate object (+0x340) is present or absent.
See decomp/melee_attack.md (the fourth argument of the damage roll).
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

HEAP = 0x7000000
FIELD, POINTS, GATE = HEAP, HEAP + 0x800, HEAP + 0x900
OBSTACLES = HEAP + 0x1000  # three fixture obstacles: wall, gate, other
ROW_TABLE, ROWS, CELLS = HEAP + 0x2000, HEAP + 0x2100, HEAP + 0x3000
TYPES = {'w': 0x24, 'g': 0x22, 'o': 0x05}


def rows():
    """The 102x102 battlefield's rows: a diamond inside the square (row layout 34db60)."""
    out = []
    for x in range(102):
        start = max(37 - x, x - 37, 0)
        out.append((start, max(start, min(38 + x, 166 - x, 102))))
    return out


ROW_BOUNDS = rows()


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x60000)
        self.parts, self.gate_open = {}, 0
        self.hooks = {reference.BASE + 0x1838d0: self.get_obstacle, reference.BASE + 0x1c0570: self.is_open}
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)
        o = self.o
        o.word(FIELD + 0x68, len(ROW_BOUNDS))
        o.word(FIELD + 0x7c, ROW_TABLE)
        o.word(ROW_TABLE + 8, ROWS)
        o.word(FIELD + 0x88, CELLS)
        index, self.base = 0, []
        for x, (start, end) in enumerate(ROW_BOUNDS):
            o.word(ROWS + 12 * x, index - start)
            o.word(ROWS + 12 * x + 4, start)
            o.word(ROWS + 12 * x + 8, end)
            self.base.append(index - start)
            index += end - start
        # Obstacle objects: [obj + 0xa0] -> header whose +0x88 is the type (477790).
        for i, kind in enumerate('wgo'):
            obj, header = OBSTACLES + 0x100 * i, OBSTACLES + 0x100 * i + 0x80
            o.word(obj + 0xa0, header)
            o.word(header + 0x88, TYPES[kind])
        self.objects = {kind: OBSTACLES + 0x100 * i for i, kind in enumerate('wgo')}

    def hook(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, self.hooks[address](uc.reg_read(UC_X86_REG_ECX)))
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def get_obstacle(self, cell):
        kind = self.parts.get((cell - CELLS) // 0x1c)
        return self.objects[kind] if kind else 0

    def is_open(self, this):
        if this != GATE - 0x28:
            raise RuntimeError('unexpected gate {:x}'.format(this))
        return self.gate_open

    def run(self, parts, has_gate, gate_open, start, end):
        self.parts = {self.base[x] + y: kind for (x, y), kind in parts.items()}
        self.gate_open = gate_open
        self.o.word(FIELD + 0x340, GATE if has_gate else 0)
        self.uc.mem_write(POINTS, struct.pack('<4i', *start, *end))
        return int(self.o.call(0x1701f0, (POINTS, POINTS + 8), this=FIELD)) & 0xff


def inside(x, y):
    return 0 <= x < 102 and ROW_BOUNDS[x][0] <= y < ROW_BOUNDS[x][1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=300)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(1701)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# W gate parts -> answers: a field with castle parts, then lines over it',
             '# gate: - no gate object, 0 closed, 1 open; parts: x:y:kind (w wall, g gate, o other obstacle);',
             '# answers: x0,y0,x1,y1=result (points in 1/16 of a cell; result of 1701f0: 0, 1 or 2)']
    for _ in range(args.count):
        parts = {}
        # A castle-like line: a wall along one row (or column) with a gate in it.
        if rng.random() < 0.7:
            along_x = rng.random() < 0.7
            fixed = rng.randint(20, 80)
            for t in range(rng.randint(10, 40), rng.randint(60, 95)):
                at = (fixed, t) if along_x else (t, fixed)
                if inside(*at):
                    parts[at] = 'w'
            for _ in range(rng.randint(0, 3)):
                t = rng.randint(30, 70)
                at = (fixed, t) if along_x else (t, fixed)
                if inside(*at):
                    parts[at] = 'g'
        for _ in range(rng.randint(0, 25)):
            at = (rng.randint(0, 101), rng.randint(0, 101))
            if inside(*at):
                parts[at] = rng.choice('wgoo')
        gate = rng.choice(['-', 0, 1])
        answers = []
        for _ in range(rng.randint(5, 15)):
            def point():
                return (rng.randint(-40, 102 * 16 + 40), rng.randint(-40, 102 * 16 + 40))
            start = point()
            if rng.random() < 0.5:
                end = (start[0] + rng.randint(-200, 200), start[1] + rng.randint(-200, 200))
            else:
                end = point()
            result = bench.run(parts, gate != '-', gate == 1, start, end)
            answers.append('{},{},{},{}={}'.format(*start, *end, result))
        lines.append('W {} {} -> {}'.format(
            gate, ';'.join('{}:{}:{}'.format(x, y, k) for (x, y), k in sorted(parts.items())) or '-',
            ' '.join(answers)))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe wall line fields'.format(args.count))


if __name__ == '__main__':
    main()
