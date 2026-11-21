#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the attack position list (10c870).

Executes 10c870 whole: the walk over the cells around each part of the target, the
distance (x87 sqrt + 0.5, __ftol) and angle (11ed40: fpatan, __ftol) of every reached
free corner, the vector insert (2b2800), the MSVC sort (118190 / 118620 with the
comparison 10d020), the pruning by turns to reach and the pruning of near angles
(11ed80, erase 1104d0), and the list destruction (316510).

Fixture boundaries, each a plain lookup in supplied data:
- the path finder's search (slot 0): a no-op, its map is supplied already searched
  (records: cost +4, flags +8; rows like the real map);
- the battlefield's row bounds (+0x68: count, then the row table);
- virtual accessors of the creature (footprint size, movement, long_weapon 0x25,
  effect 0x90) and of the target (its list of parts, slot 0x28; slot 0x3c; effect 0x89).
Neither the mover nor the target stands in a castle tower (the tower branches are not
exercised). See decomp/attack_angles.md.
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

BASE = reference.BASE
HEAP = 0x7000000
VB = 0xe00
CACHE, FIELD, FINDER = HEAP, HEAP + 0x400, HEAP + 0x800
FIELD_ROWS, FINDER_INFO, ROWS = HEAP + 0x900, HEAP + 0xa00, HEAP + 0xb00
STUB, LIST_SLOT = HEAP + 0xf00, HEAP + 0xf10
CREATURE, TARGET, PART = HEAP + 0x1000, HEAP + 0x3000, HEAP + 0x5000
LIST, NODES, OUT = HEAP + 0x6000, HEAP + 0x6100, HEAP + 0x6200
CELLS = HEAP + 0x10000
OCCUPIED, REACHED = 0x04, 0x01


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x100000)
        self.uc.hook_add(UC_HOOK_CODE, self.parts, begin=LIST_SLOT, end=LIST_SLOT)

    def parts(self, uc, address, size, _):
        """Slot 0x28 of the target: write its list of parts (std::list of counted
        pointers) into the caller's list object."""
        o = self.o
        sp = uc.reg_read(UC_X86_REG_ESP)
        out = o.read_word(sp + 4)
        head = NODES
        node = NODES + 0x20
        o.word(head, node)
        o.word(head + 4, node)
        o.word(node, head)
        o.word(node + 4, head)
        o.word(node + 8, PART)
        o.word(out + 4, head)
        o.word(out + 8, 1)
        uc.reg_write(UC_X86_REG_EIP, o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 8)

    def rows(self, at, rows):
        index = 0
        base = []
        for x, (start, end) in enumerate(rows):
            self.o.word(at + 12 * x, index - start)
            self.o.word(at + 12 * x + 4, start)
            self.o.word(at + 12 * x + 8, end)
            base.append(index - start)
            index += end - start
        return base

    def build(self, world):
        o, uc = self.o, self.uc
        o.arena_next = reference.ARENA
        for area, size in ((CACHE, 0x1000), (CREATURE, 0x4000), (PART, 0x1000), (LIST, 0x400)):
            uc.mem_write(area, bytes(size))
        uc.mem_write(STUB, b'\xc3')
        o.word(CACHE + 0x108, FIELD)
        rows = world['rows']
        # Battlefield rows: count at +0x68, the table through +0x7c (+0x68 + 0x14) and +8.
        o.word(FIELD + 0x68, len(rows))
        o.word(FIELD + 0x7c, FIELD_ROWS)
        o.word(FIELD_ROWS + 8, ROWS)
        base = self.rows(ROWS, rows)
        # The path finder: slot 0 does nothing; the map at +0x1c (info +0x30, cells +0x3c).
        o.word(FINDER, FINDER + 0x80)
        o.word(FINDER + 0x80, STUB)
        o.word(FINDER + 0x30, FINDER_INFO)
        o.word(FINDER_INFO + 8, ROWS)
        records = bytearray()
        for x, (start, end) in enumerate(rows):
            for y in range(start, end):
                flags, cost = world['cells'][(x, y)]
                records += struct.pack('<HHIIiiii', 100, 0, cost, flags, -1, -1, x, y)
        uc.mem_write(CELLS, bytes(records))
        o.word(FINDER + 0x3c, CELLS)
        # The mover.
        mover = world['mover']
        table, vbtable, base_vt, effects_vt = CREATURE + 0xf00, CREATURE + 0xf80, CREATURE + 0xfc0, CREATURE + 0xe80
        o.word(CREATURE + 0x98, table)
        o.word(table + 0x18, o.accessor(0, mover['movement'], pop=0))
        o.word(table + 0x68, o.lookup(1, [0x25] if mover['long_weapon'] else []))
        o.word(CREATURE + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(CREATURE + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(2, [0x90] if mover['effect_90'] else []))
        o.word(CREATURE + 8, base_vt)
        o.word(base_vt + 8, o.accessor(3, mover['size'], pop=0))
        # The target: its parts, slot 0x3c, effect 0x89 through +4, not in a tower (+8).
        target = world['target']
        t_vt, t_vb, t_effects = TARGET + 0x100, TARGET + 0x180, TARGET + 0x1c0
        uc.mem_write(TARGET, bytes(0x400))
        o.word(TARGET, t_vt)
        o.word(t_vt + 0x28, LIST_SLOT)
        o.word(t_vt + 0x3c, o.accessor(4, int(target['slot_3c']), pop=0))
        o.word(TARGET + 4, t_vb)
        o.word(t_vb + 4, 0x200)
        o.word(TARGET + 4 + 0x200, t_effects)
        o.word(t_effects + 8, o.lookup(5, [0x89] if target['effect_89'] else []))
        # The part: refcount, position in subcells, footprint size through +8.
        p_vt = PART + 0x100
        o.word(PART + 4, 1000)
        uc.mem_write(PART + 0xc, struct.pack('<iii', *target['position'], 0))
        o.word(PART + 8, p_vt)
        o.word(p_vt + 8, o.accessor(6, target['size'], pop=0))
        uc.mem_write(OUT, bytes(16))
        return base

    def run(self, world):
        self.build(world)
        self.o.call(0x10c870, (CREATURE, TARGET, FINDER, OUT), this=CACHE)
        begin, end = self.o.read_word(OUT + 4), self.o.read_word(OUT + 8)
        entries = []
        for at in range(begin, end, 20) if begin else []:
            entries.append(struct.unpack('<iiiii', bytes(self.uc.mem_read(at, 20))))
        return entries


def random_world(rng):
    height = rng.randint(4, 24)
    rows = []
    for x in range(height):
        if rng.random() < 0.6:
            middle = height / 2
            inset = int(abs(x - middle) * rng.choice([0.5, 1]))
            rows.append((inset, max(inset + 1, rng.randint(14, 26) - inset)))
        else:
            start = rng.choice([0, 0, 1, 3])
            rows.append((start, start + rng.randint(1, 26)))
    # Costs grow with the distance from a start, with noise, like a real search.
    sx, sy = rng.randrange(height), rng.randrange(26)
    step = rng.choice([100, 100, 150, 250])
    density = rng.choice([0.0, 0.1, 0.25])
    unreached = rng.choice([0.0, 0.05, 0.2])
    cells = {}
    for x, (start, end) in enumerate(rows):
        for y in range(start, end):
            flags = 0 if rng.random() < unreached else REACHED
            if rng.random() < density:
                flags |= OCCUPIED
            if rng.random() < 0.1:
                flags |= rng.choice([0x02, 0x20, 0x80])
            cost = step * max(abs(x - sx), abs(y - sy)) + rng.choice([0, 0, 0, 50, rng.randint(0, 400)])
            if (x, y) == (sx, sy):
                cost = 0
            cells[(x, y)] = (flags, cost)
    mover = {
        'size': rng.choice([1, 1, 2, 3, 3, 4, 5, 6, 7]),
        'long_weapon': rng.random() < 0.25,
        'movement': rng.choice([1, 100, 300, 500, 700, 1000, 1500, 2500, rng.randint(1, 4000)]),
        'effect_90': rng.random() < 0.04,
    }
    target = {
        'size': rng.choice([1, 2, 3, 3, 4, 5, 6, 7]),
        'position': (rng.randint(-40, 16 * height + 20), rng.randint(-40, 16 * 26 + 20)),
        'slot_3c': rng.random() < 0.1,
        'effect_89': rng.random() < 0.04,
    }
    return {'rows': rows, 'cells': cells, 'mover': mover, 'target': target}


def encode(world):
    rows = ';'.join('{}:{}'.format(*r) for r in world['rows'])
    m, t = world['mover'], world['target']
    mover = '{}:{}:{}{}'.format(m['size'], m['movement'], 'l' if m['long_weapon'] else '', 'x' if m['effect_90'] else '')
    mover = mover if not mover.endswith(':') else mover + '-'
    target = '{}:{}:{}:{}{}'.format(*t['position'], t['size'], 'a' if t['slot_3c'] else '', 'x' if t['effect_89'] else '')
    target = target if not target.endswith(':') else target + '-'
    cells = []
    for x, (start, end) in enumerate(world['rows']):
        cells.append(','.join('{:x}/{}'.format(*world['cells'][(x, y)]) for y in range(start, end)))
    return ' '.join([rows, mover, target, ';'.join(cells)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=400)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(186)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# A rows mover target cells -> entries',
             '# rows: start:end per row; mover: footprint size:movement:traits (l long_weapon 0x25, x effect 0x90);',
             '# target: position x:position y (subcells):footprint size:traits (a slot 0x3c, x effect 0x89);',
             '# cells: per row (;) the cells (,) as flags hex/path cost of the searched map;',
             '# entries: angle:x:y:distance:cost in list order (- for none)']
    made = 0
    while made < args.count:
        world = random_world(rng)
        entries = bench.run(world)
        result = ';'.join(':'.join(str(v) for v in e) for e in entries) or '-'
        lines.append('A {} -> {}'.format(encode(world), result))
        made += 1
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe attack position fixtures'.format(made))


if __name__ == '__main__':
    main()
