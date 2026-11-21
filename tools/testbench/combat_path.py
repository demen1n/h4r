#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the battlefield path search (20d200).

Executes the search slot of the combat path finder whole: the min-heap of points
(vector insert 20be10, sift-up, the pop 20c170, erase 4144f0), every step cost and
flag branch, the cover marking that follows the search (20e640) and the path
reconstruction (20e7b0). The start point is queued by the real 20e3f0.

Fixture boundaries, each a plain lookup in supplied data:
- the map preparation 20c7f0 (copying the cached template map, blockage of
  creatures, gates, walls and towers, terrain costs, seeding): replaced by the
  supplied cell records; it only raises the "prepared" byte (+4). The start cell's
  entry cost is patched after 20e3f0, as the real preparation's terrain refresh does;
- the zone-of-control directions 172210 (8 bytes per point, supplied);
- the creature's footprint getter f0f0: returns a footprint built by the real
  constructor 1fc340 (its cache 1fc590 is bypassed);
- the creature's virtual accessors (movement, footprint size, abilities, effects);
- the static direction table: 305880 builds it through string maps at startup; the
  bench writes the eight offsets that 3054b0 stores (NE, E, SE, S, SW, W, NW, N).
See decomp/combat_path.md.
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

BASE = reference.BASE
HEAP = 0x7000000
VB = 0xe00
FINDER, VTABLE, INFO, RECORDS = HEAP, HEAP + 0x100, HEAP + 0x200, HEAP + 0x400
CREATURE, FOOTPRINT, POINT, OUT, DIRECTIONS = HEAP + 0x1000, HEAP + 0x3000, HEAP + 0x3100, HEAP + 0x3200, HEAP + 0x3300
FIELD = HEAP + 0x4000
CELLS = HEAP + 0x10000
# Offsets of the eight neighbours as 3054b0 stores them: (row, column) deltas.
OFFSETS = [(-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1)]
FLYING, LONG_WEAPON = 0x19, 0x25


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x100000)
        self.zones = {}
        self.hooks = {BASE + 0x20c7f0: (self.prepare, 0), BASE + 0x172210: (self.zone, 12),
                      BASE + 0xf0f0: (self.footprint, 0)}
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)

    def hook(self, uc, address, size, _):
        handler, pop = self.hooks[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        handler(uc, sp)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def prepare(self, uc, sp):
        if uc.reg_read(UC_X86_REG_ECX) != FINDER:
            raise RuntimeError('unexpected path map preparation')
        uc.mem_write(FINDER + 4, b'\x01')

    def zone(self, uc, sp):
        o = self.o
        if uc.reg_read(UC_X86_REG_ECX) != FIELD or o.read_word(sp + 4) != CREATURE:
            raise RuntimeError('unexpected zone-of-control lookup')
        point = o.read_word(sp + 8)
        x, y = struct.unpack('<ii', bytes(uc.mem_read(point, 8)))
        mask = self.zones.get((x, y), 0)
        uc.mem_write(o.read_word(sp + 12), bytes((mask >> d) & 1 for d in range(8)))

    def footprint(self, uc, sp):
        if uc.reg_read(UC_X86_REG_ECX) != CREATURE + 8:
            raise RuntimeError('unexpected footprint lookup')
        uc.reg_write(UC_X86_REG_EAX, FOOTPRINT)

    def creature(self, world):
        o = self.o
        self.uc.mem_write(CREATURE, bytes(0x1000))
        table, vbtable, base_vt, effects_vt = CREATURE + 0xf00, CREATURE + 0xf80, CREATURE + 0xfc0, CREATURE + 0xe80
        o.word(CREATURE + 0x98, table)
        o.word(table + 0x18, o.accessor(0, world['movement'], pop=0))
        abilities = [a for a, on in ((FLYING, 'f' in world['traits']), (LONG_WEAPON, 'l' in world['traits'])) if on]
        o.word(table + 0x68, o.lookup(1, abilities))
        o.word(CREATURE + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(CREATURE + 0x9c + VB, effects_vt)
        effects = [e for e, letter in ((7, 'b'), (9, 'h'), (0x2e, 'e')) if letter in world['traits']]
        o.word(effects_vt + 8, o.lookup(2, effects))
        o.word(CREATURE + 8, base_vt)
        o.word(base_vt + 8, o.accessor(3, world['size'], pop=0))
        o.word(CREATURE + 0xa18, FIELD)

    def build(self, world):
        o, uc = self.o, self.uc
        o.arena_next = reference.ARENA
        uc.mem_write(FINDER, bytes(0x100))
        o.word(FINDER, VTABLE)
        o.word(VTABLE, BASE + 0x20d200)  # slot 0: the search itself
        rows = world['rows']
        o.word(FINDER + 0x1c, len(rows))
        o.word(FINDER + 0x30, INFO)
        o.word(INFO + 8, RECORDS)
        o.word(FINDER + 0x3c, CELLS)
        o.word(FINDER + 0x4c, CREATURE)
        uc.mem_write(FINDER + 0x50, bytes((int('t' in world['traits']), 0)))
        index = 0
        self.base = []
        for x, (start, end) in enumerate(rows):
            o.word(RECORDS + 12 * x, index - start)
            o.word(RECORDS + 12 * x + 4, start)
            o.word(RECORDS + 12 * x + 8, end)
            self.base.append(index - start)
            index += end - start
        self.count = index
        records = bytearray()
        for x, (start, end) in enumerate(rows):
            for y in range(start, end):
                flags, enter = world['cells'].get((x, y), (0, 100))
                records += struct.pack('<HHIIiiii', enter, 0, 0, flags, -1, -1, x, y)
        uc.mem_write(CELLS, bytes(records))
        self.creature(world)
        o.call(0x1fc340, (world['size'],), this=FOOTPRINT)
        self.zones = world['zones']
        table = b''.join(struct.pack('<ii', *d) for d in OFFSETS)
        uc.mem_write(DIRECTIONS, table)
        o.word(0xaaad80, DIRECTIONS)
        uc.mem_write(0xaaad84, b'\x01')
        return bytes(records)

    def cell(self, x, y):
        return CELLS + 28 * (self.base[x] + y)

    def run(self, world):
        o, uc = self.o, self.uc
        before = self.build(world)
        # 20e3f0 builds the start record in a local whose upper flag bits are never
        # written; clear the stack so they are zero instead of leftovers.
        uc.mem_write(reference.STACK, bytes(0x8000))
        sx, sy = world['start']
        uc.mem_write(POINT, struct.pack('<ii', sx, sy))
        o.call(0x20e3f0, (POINT,), this=FINDER)
        uc.mem_write(self.cell(sx, sy), struct.pack('<H', world['start_enter']))
        before = bytearray(before)
        o.call(0x20d200, (), this=FINDER)
        after = bytes(uc.mem_read(CELLS, 28 * self.count))
        changed = []
        k = 0
        for x, (start, end) in enumerate(world['rows']):
            for y in range(start, end):
                if after[28 * k:28 * k + 28] != before[28 * k:28 * k + 28]:
                    enter, over, cost, flags, fx, fy, ox, oy = struct.unpack_from('<HHIIiiii', after, 28 * k)
                    changed.append('{}:{}:{}:{}:{}:{:x}:{}:{}:{}:{}'.format(x, y, enter, over, cost, flags, fx, fy, ox, oy))
                k += 1
        paths = []
        for tx, ty in world['targets']:
            uc.mem_write(OUT, bytes(16))
            found = int(o.call(0x20e7b0, (tx, ty, OUT), this=FINDER)) & 0xff
            begin, end = o.read_word(OUT + 4), o.read_word(OUT + 8)
            points = [struct.unpack('<ii', bytes(uc.mem_read(p + 28, 8))) for p in range(begin, end, 36)] if begin else []
            paths.append('{}:{}'.format(found, '/'.join('{},{}'.format(*p) for p in points) or '-'))
        return ';'.join(changed) or '-', ' '.join(paths)


def fits(rows, size, x, y):
    """Whether a size x size square at (x, y) stays inside the rows (the real preparation
    never lets the search stand on a cell whose footprint leaves the field)."""
    for dx in range(size):
        if x + dx >= len(rows):
            return False
        start, end = rows[x + dx]
        if y < start or y + size > end:
            return False
    return True


def random_world(rng):
    height = rng.randint(1, 16)
    rows = []
    for x in range(height):
        if rng.random() < 0.5:
            # a diamond-like edge, as on the battlefield
            middle = height / 2
            inset = int(abs(x - middle) * rng.choice([0.5, 1]))
            rows.append((inset, max(inset + 1, rng.randint(10, 16) - inset)))
        else:
            start = rng.choice([0, 0, 1, 2])
            rows.append((start, start + rng.randint(1, 16)))
    size = rng.choice([1, 1, 2, 2, 3, 3, 4, 5, 6, 7])
    traits = ''.join(letter for letter, p in (('f', 0.2), ('e', 0.08), ('b', 0.08), ('h', 0.04),
                                               ('l', 0.2), ('t', 0.12)) if rng.random() < p)
    movement = rng.choice([0, 100, 150, 300, 450, 600, 900, 1200, 2000, 5000, rng.randint(0, 3000)])
    cells = {}
    candidates = []
    density = rng.choice([0.0, 0.05, 0.15, 0.3])
    tower = None
    if rng.random() < 0.3:
        tower = (rng.randrange(height), rng.randrange(16), rng.randint(1, 4))
    wall_row = rng.randrange(height) if rng.random() < 0.3 else None
    for x, (start, end) in enumerate(rows):
        for y in range(start, end):
            if not fits(rows, size, x, y):
                cells[(x, y)] = (0x10, 100)
                continue
            flags = 0
            if rng.random() < density:
                flags |= rng.choice([0x10, 0x04, 0x04, 0x14, 0x40, 0x44])
            if tower and tower[0] <= x < tower[0] + tower[2] and tower[1] <= y < tower[1] + tower[2]:
                flags |= 0x20 | (0x04 if rng.random() < 0.2 else 0)
            if wall_row is not None and x == wall_row and rng.random() < 0.8:
                flags |= 0x40 | (0x04 if rng.random() < 0.5 else 0)
            enter = 100
            if rng.random() < 0.25:
                enter = rng.choice([0, 50, 100, 150, 200, 250, 400, 1000, rng.randint(0, 600)])
            if flags or enter != 100:
                cells[(x, y)] = (flags, enter)
            if not flags & 0x10:
                candidates.append((x, y))
    if not candidates:
        return None
    start = rng.choice(candidates)
    zones = {}
    if rng.random() < 0.5:
        for x, (s, e) in enumerate(rows):
            for y in range(s, e):
                if rng.random() < 0.1:
                    zones[(x, y)] = rng.randrange(256)
    targets = [rng.choice(candidates) for _ in range(rng.randint(1, 6))]
    if rng.random() < 0.3:
        targets.append(start)
    return {'rows': rows, 'size': size, 'traits': traits, 'movement': movement, 'cells': cells,
            'start': start, 'start_enter': rng.choice([100, 100, 150, 400]), 'zones': zones, 'targets': targets}


def encode(world):
    rows = ';'.join('{}:{}'.format(*r) for r in world['rows'])
    mover = '{}:{}:{}'.format(world['movement'], world['size'], world['traits'] or '-')
    start = '{}:{}:{}'.format(*world['start'], world['start_enter'])
    zones = ';'.join('{}:{}:{:x}'.format(x, y, m) for (x, y), m in sorted(world['zones'].items())) or '-'
    cells = ';'.join('{}:{}:{:x}:{}'.format(x, y, f, e) for (x, y), (f, e) in sorted(world['cells'].items())) or '-'
    targets = ';'.join('{}:{}'.format(*t) for t in world['targets'])
    return ' '.join([rows, mover, start, zones, cells, targets])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=500)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(62)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# P rows mover start zones cells targets -> changed paths',
             '# rows: start:end per row; mover: movement:footprint size:traits (f flying 0x19, l long_weapon 0x25,',
             '#   effects b 7, h 9, e 0x2e; t teleport finder); start: x:y:entry cost after the terrain refresh;',
             '# zones: x:y:direction mask (bit d = zone-of-control step penalty in direction d);',
             '# cells: x:y:flags hex:entry cost (others 0:100); targets: x:y for 20e7b0;',
             '# changed: x:y:entry:over:cost:flags hex:from x:from y:origin x:origin y for every record that changed;',
             '# paths: found:x,y/x,y... per target (points from the target back, start excluded)']
    made = 0
    while made < args.count:
        world = random_world(rng)
        if world is None:
            continue
        changed, paths = bench.run(world)
        lines.append('P {} -> {} {}'.format(encode(world), changed, paths))
        made += 1
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe path search fixtures'.format(made))


if __name__ == '__main__':
    main()
