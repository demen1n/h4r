#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for a combat creature's terrain cost list (225320)
and for the terrain bits under a footprint (224f60).

Executes 225320 whole: the list of (terrain bits, entry cost) pairs it appends to a
vector (the real vector insert 386ae0 and operator new from the bench's arena), the
real static tables it builds on first use (0x984158, 0x9841a0) and the
real MSVC std::sort at its end (225900, 225a40, 225ac0, 225b30). Fixture boundaries:
the effect "terrain walk" (0xb3) is a byte lookup; the stats' virtual +0x14 (the
creature's hero) returns a fixture hero or nothing, whose +0x50 answers the skill
level; +0x18 (movement points) is a supplied value; +0x64 (cheaper terrain) is a byte
lookup.

224f60 runs whole on a 102x102 battlefield (rows as the exe lays them out, cells of 0x1c
bytes at +0x88) with random terrain bytes and flags around the corner, a footprint built
by the real 1fc340 for the size, and t_battlefield_cell::get_creature (183870) answering
the fixture creature on the cells marked as its own. Terrain types stay 0..8.

The terrain index mapping 484a10 (it registers an atexit handler) answers 0:
it fills only the table entries for terrain 9..18, which 225320 never reads (it reads
entries 1..8). See decomp/terrain_costs.md.
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
CREATURE, HERO, LIST = HEAP, HEAP + 0x2000, HEAP + 0x3000
FIELD, POINT, FOOTPRINTS = HEAP + 0x3800, HEAP + 0x3c00, HEAP + 0x4000
ROW_TABLE, ROWS, CELLS = HEAP + 0x5000, HEAP + 0x5100, HEAP + 0x6000
VB = 0x200
TERRAIN_WALK = 0xb3


def signed(value):
    return value - (1 << 32) if value >= 1 << 31 else value


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.o.uc.mem_map(HEAP, 0x60000)
        self.o.uc.hook_add(UC_HOOK_CODE, self.terrain_index, begin=reference.BASE + 0x484a10,
                           end=reference.BASE + 0x484a10)
        self.o.uc.hook_add(UC_HOOK_CODE, self.get_creature, begin=reference.BASE + 0x183870,
                           end=reference.BASE + 0x183870)
        self.own_cells = set()

    def get_creature(self, uc, address, size, _):
        """t_battlefield_cell::get_creature (thiscall, no arguments)."""
        sp = uc.reg_read(UC_X86_REG_ESP)
        cell = uc.reg_read(UC_X86_REG_ECX)
        uc.reg_write(UC_X86_REG_EAX, CREATURE if cell in self.own_cells else 0)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def terrain_index(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, 0)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)  # cdecl

    def run(self, walk, skill, movement, home):
        o, uc = self.o, self.o.uc
        uc.mem_write(HEAP, bytes(0x3800))
        o.arena_next = reference.ARENA
        stats = CREATURE + 0x98
        table, vbtable, effects_vt, hero_vt = CREATURE + 0x1c00, CREATURE + 0x1d00, CREATURE + 0x1e00, HERO + 0x100
        o.word(stats, table)
        o.word(stats + 4, vbtable)
        o.word(vbtable + 8, VB)
        o.word(stats + 4 + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(0, [TERRAIN_WALK] if walk else []))
        o.word(table + 0x14, o.accessor(1, 0 if skill is None else HERO, pop=0))
        o.word(table + 0x18, o.accessor(2, movement & 0xffffffff, pop=0))
        o.word(table + 0x64, o.lookup(3, [t for t in range(1, 9) if home[t - 1]]))
        o.word(HERO + 8, hero_vt)
        o.word(hero_vt + 0x50, o.accessor(4, (skill or 0) & 0xffffffff, pop=4))
        o.call(0x225320, (CREATURE,), this=LIST)
        begin, end = o.read_word(LIST + 4), o.read_word(LIST + 8)
        pairs = []
        for at in range(begin, end, 8):
            bits, cost = struct.unpack('<Ii', bytes(uc.mem_read(at, 8)))
            pairs.append((bits, cost))
        return pairs


def rows():
    """The 102x102 battlefield's rows: a diamond inside the square (row layout 34db60)."""
    out = []
    for x in range(102):
        start = max(37 - x, x - 37, 0)
        out.append((start, max(start, min(38 + x, 166 - x, 102))))
    return out


ROW_BOUNDS = rows()


class FieldBench(Bench):
    def field(self, cells):
        """Lay out the field: every cell terrain 1, flags 0x10, then the given ones."""
        o, uc = self.o, self.o.uc
        o.arena_next = reference.ARENA
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
        uc.mem_write(CELLS, b''.join(b'\x01\x10' + bytes(0x1a) for _ in range(index)))
        self.own_cells = set()
        for (x, y), (terrain, flags, own) in cells.items():
            at = CELLS + 0x1c * (self.base[x] + y)
            uc.mem_write(at, bytes((terrain, flags)))
            if own:
                self.own_cells.add(at)

    def bits(self, corner, size, with_creature):
        o = self.o
        footprint = FOOTPRINTS + 0x100 * size
        o.call(0x1fc340, (size,), this=footprint)
        self.o.uc.mem_write(POINT, struct.pack('<ii', *corner))
        return int(o.call(0x224f60, (FIELD, POINT, footprint, CREATURE if with_creature else 0),
                          this=0, callee_pop=False))


def inside(x, y):
    return 0 <= x < 102 and ROW_BOUNDS[x][0] <= y < ROW_BOUNDS[x][1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=1200)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(182)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# C walk skill movement home -> pairs',
             '# skill: level of the hero skill 0xf or - (no hero); home: 8 bits for terrain 1..8;',
             '# pairs: bits hex:cost in list order, or -']
    cases = []
    for walk in (0, 1):
        for skill in (None, 0, 1, 2, 3):
            for movement in (0, 100, 200, 201, 300, 400, 401, 600, 1000):
                for home in ('00000000', '11111111', '10100100'):
                    cases.append((walk, skill, movement, home))
    while len(cases) < args.count:
        cases.append((int(rng.random() < 0.05), rng.choice([None, None, 0, 1, 2]), rng.randint(-100, 3000),
                      ''.join(rng.choice('01') for _ in range(8))))
    for walk, skill, movement, home in cases:
        pairs = bench.run(walk, skill, movement, [c == '1' for c in home])
        lines.append('C {} {} {} {} -> {}'.format(
            walk, '-' if skill is None else skill, movement, home,
            ' '.join('{:x}:{}'.format(b, c) for b, c in pairs) or '-'))
    field = FieldBench(args.exe)
    lines += ['# B x y size creature cells -> bits: 224f60 at corner (x, y); creature 1 = refreshed for the',
              '#   fixture creature; cells x:y:terrain byte hex:flags hex:own (others terrain 1, flags 10)']
    footprints = 0
    while footprints < args.count:
        size = rng.randint(1, 7)
        x, y = rng.randint(-3, 104), rng.randint(-3, 104)
        if rng.random() < 0.8 and not inside(x, y):
            continue
        cells = {}
        for dx in range(-1, size + 1):
            for dy in range(-1, size + 1):
                if inside(x + dx, y + dy) and rng.random() < 0.6:
                    terrain = rng.randint(0, 8) | (rng.randint(0, 7) << 5)
                    flags = rng.choice([0, 0x10, 1, 2, 3, 0x40, 0x41, 0x42, 0x13, rng.randint(0, 255)])
                    cells[(x + dx, y + dy)] = (terrain, flags, int(rng.random() < 0.3))
        with_creature = int(rng.random() < 0.5)
        field.field(cells)
        bits = field.bits((x, y), size, with_creature)
        lines.append('B {} {} {} {} {} -> {:x}'.format(
            x, y, size, with_creature,
            ';'.join('{}:{}:{:x}:{:x}:{}'.format(cx, cy, t, f, own) for (cx, cy), (t, f, own) in sorted(cells.items()))
            or '-', bits))
        footprints += 1
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe terrain cost fixtures, {} footprint terrain fixtures'.format(len(cases), footprints))


if __name__ == '__main__':
    main()
