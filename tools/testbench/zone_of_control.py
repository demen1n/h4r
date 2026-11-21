#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for zones of control on the battlefield.

Executes, whole:
- 172000 for every creature on a random field: its threat footprint (1efd60: width
  = size + 2 * reach, arc 200 or 360 with hydra_strike 0x1d, the creature's facing
  +0x6c) from the real cache 48b350 (map lookup 48b950, insert 48b700, the builder
  48b090 with the angle 11ed40, get_direction 11edb0 and the angle difference 11ed80),
  placed around the creature and added to the threat list of every cell on the field
  (1835f0);
- 172210 for a mover standing at a corner: the zone-of-control directions it reports to
  the path search (the direction 11ede0 from each threatening enemy).
The threat lists of all cells are recorded too.

Fixture boundaries, each a plain lookup in supplied data: the creature's combat
footprint (f0f0 → the real 1fc340, built up front), atexit (4ed013) for the cache's
static, and the virtual accessors of the creatures: footprint size, side (+0x74), number
(+0x40), abilities (+0x68: long_weapon 0x25, hydra_strike 0x1d, flying 0x19,
ignore_zones_of_control 0x20, and 0, asked and ignored), effects 7 and 0x2e.
See decomp/zone_of_control.md.
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
FIELD, ROW_TABLE, ROWS = HEAP, HEAP + 0x100, HEAP + 0x200
FOOTPRINTS = HEAP + 0x1000  # 0x100 per size
CODE = HEAP + 0x2000  # accessor stubs, 0x400 per creature
POINT, OUT = HEAP + 0x6000, HEAP + 0x6010
CREATURES = HEAP + 0x10000  # 0x1000 per creature
CELLS = HEAP + 0x40000
HEADS = HEAP + 0x80000
MAX_CREATURES = 8
FLYING, LONG_WEAPON, HYDRA, IGNORE_ZONES = 0x19, 0x25, 0x1d, 0x20


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x100000)
        self.hooks = {BASE + 0xf0f0: self.footprint, BASE + 0x4ed013: self.atexit}
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)

    def hook(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        self.hooks[address](uc, sp)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def footprint(self, uc, sp):
        """The creature's combat footprint (cached by size in the exe)."""
        base = uc.reg_read(UC_X86_REG_ECX) - 8
        index = (base - CREATURES) // 0x1000
        if not 0 <= index < MAX_CREATURES or base != CREATURES + 0x1000 * index:
            raise RuntimeError('unexpected footprint lookup')
        uc.reg_write(UC_X86_REG_EAX, FOOTPRINTS + 0x100 * self.sizes[index])

    def atexit(self, uc, sp):
        uc.reg_write(UC_X86_REG_EAX, 0)

    def stub_value(self, at, value):
        self.uc.mem_write(at, b'\xb8' + struct.pack('<I', value & 0xffffffff) + b'\xc3')
        self.uc.ctl_remove_cache(at, at + 16)
        return at

    def stub_lookup(self, at, values):
        """bool f(int) backed by a 256-byte table right after the code."""
        table = at + 0x20
        self.uc.mem_write(table, bytes(256))
        for v in values:
            self.uc.mem_write(table + v, b'\x01')
        self.uc.mem_write(at, b'\x8b\x44\x24\x04\x0f\xb6\x80' + struct.pack('<I', table) + b'\xc2\x04\x00')
        self.uc.ctl_remove_cache(at, at + 0x120)
        return at

    def creature(self, index, c):
        o = self.o
        base = CREATURES + 0x1000 * index
        code = CODE + 0x400 * index
        self.uc.mem_write(base, bytes(0x1000))
        o.word(base + 4, 1000)  # reference count, never released
        uc_stats, vbtable, base_vt, effects_vt = base + 0xf00, base + 0xf80, base + 0xfc0, base + 0xe80
        o.word(base + 0x98, uc_stats)
        abilities = [a for a, on in ((LONG_WEAPON, c['long_weapon']), (HYDRA, c['hydra']), (FLYING, c['flying']),
                                     (IGNORE_ZONES, c['ignore'])) if on]
        o.word(uc_stats + 0x68, self.stub_lookup(code, abilities))
        o.word(uc_stats + 0x74, self.stub_value(code + 0x140, c['side']))
        o.word(uc_stats + 0x40, self.stub_value(code + 0x150, c['number']))
        o.word(base + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(base + 0x9c + VB, effects_vt)
        effects = [e for e, on in ((7, c['effect_7']), (0x2e, c['effect_2e'])) if on]
        o.word(effects_vt + 8, self.stub_lookup(code + 0x160, effects))
        o.word(base + 8, base_vt)
        o.word(base_vt + 8, self.stub_value(code + 0x2a0, c['size']))
        self.uc.mem_write(base + 0xc, struct.pack('<iii', *c['position'], 0))
        o.word(base + 0x6c, c['facing'])
        return base

    def build(self, world):
        o, uc = self.o, self.uc
        o.arena_next = reference.ARENA
        o.word(0xad0228, 0)  # the threat footprint cache: build it again in the fresh arena
        # Combat footprints live in the arena too: build them again after the reset.
        for size in range(1, 8):
            o.call(0x1fc340, (size,), this=FOOTPRINTS + 0x100 * size)
        rows = world['rows']
        o.word(FIELD + 0x68, len(rows))
        o.word(FIELD + 0x7c, ROW_TABLE)
        o.word(ROW_TABLE + 8, ROWS)
        o.word(FIELD + 0x88, CELLS)
        index = 0
        self.base = []
        for x, (start, end) in enumerate(rows):
            o.word(ROWS + 12 * x, index - start)
            o.word(ROWS + 12 * x + 4, start)
            o.word(ROWS + 12 * x + 8, end)
            self.base.append(index - start)
            index += end - start
        uc.mem_write(CELLS, bytes(0x1c * index))
        uc.mem_write(HEADS, bytes(16 * index))
        for k in range(index):
            head = HEADS + 16 * k
            o.word(head, head)
            o.word(head + 4, head)
            o.word(CELLS + 0x1c * k + 0x14, head)
        self.sizes = [c['size'] for c in world['creatures']]
        self.objects = [self.creature(i, c) for i, c in enumerate(world['creatures'])]

    def cell(self, x, y):
        return CELLS + 0x1c * (self.base[x] + y)

    def run(self, world):
        o = self.o
        self.build(world)
        for obj in self.objects:
            o.call(0x172000, (obj,), this=FIELD)
        lists = []
        for x, (start, end) in enumerate(world['rows']):
            for y in range(start, end):
                head = o.read_word(self.cell(x, y) + 0x14)
                members, node = [], o.read_word(head)
                while node != head:
                    members.append(self.objects.index(o.read_word(node + 8)))
                    node = o.read_word(node)
                if members:
                    lists.append('{}:{}:{}'.format(x, y, ','.join(map(str, members))))
        answers = []
        for mover, at in world['queries']:
            self.uc.mem_write(POINT, struct.pack('<ii', *at))
            self.uc.mem_write(OUT, b'\xee' * 8)
            o.call(0x172210, (self.objects[mover], POINT, OUT), this=FIELD)
            mask = bytes(self.uc.mem_read(OUT, 8))
            bits = sum(1 << d for d in range(8) if mask[d])
            if any(b not in (0, 1) for b in mask):
                raise RuntimeError('unexpected mask bytes {}'.format(mask.hex()))
            answers.append('{}:{}:{}:{:x}'.format(mover, *at, bits))
        return ';'.join(lists) or '-', ' '.join(answers)


def fits(rows, size, x, y):
    for dx in range(size):
        if x + dx >= len(rows):
            return False
        start, end = rows[x + dx]
        if y < start or y + size > end:
            return False
    return True


def random_world(rng):
    height = rng.randint(6, 30)
    rows = []
    for x in range(height):
        middle = height / 2
        inset = int(abs(x - middle) * rng.choice([0, 0.5, 1]))
        rows.append((inset, max(inset + 1, rng.randint(16, 30) - inset)))
    creatures = []
    for _ in range(rng.randint(1, MAX_CREATURES)):
        size = rng.choice([1, 2, 3, 3, 4, 5, 6, 7])
        x = rng.randrange(-3, height + 2)
        y = rng.randrange(-3, 32)
        creatures.append({
            'size': size,
            'position': (x * 16 + rng.choice([0, 0, 0, rng.randrange(16)]), y * 16 + rng.choice([0, 0, 0, rng.randrange(16)])),
            'facing': rng.randrange(8),
            'side': rng.randrange(2),
            'number': rng.choice([0, 1, 5, 20, 20, 20]),
            'long_weapon': rng.random() < 0.25,
            'hydra': rng.random() < 0.15,
            'flying': rng.random() < 0.25,
            'ignore': rng.random() < 0.1,
            'effect_7': rng.random() < 0.08,
            'effect_2e': rng.random() < 0.08,
        })
    queries = []
    for _ in range(rng.randint(20, 50)):
        mover = rng.randrange(len(creatures))
        size = creatures[mover]['size']
        spots = [(x, y) for x, (s, e) in enumerate(rows) for y in range(s, e) if fits(rows, size, x, y)]
        # Mostly near another creature, where zones are.
        enemies = [c for c in creatures if c['side'] != creatures[mover]['side'] and c['number']]
        other = rng.choice(enemies) if enemies and rng.random() < 0.9 else rng.choice(creatures)
        ox, oy = other['position'][0] >> 4, other['position'][1] >> 4
        r = 6 if other['long_weapon'] else 2
        near = [(x, y) for x, y in spots
                if ox - r - size < x <= ox + other['size'] + r and oy - r - size < y <= oy + other['size'] + r]
        if near and rng.random() < 0.85:
            spots = near
        if spots:
            queries.append((mover, rng.choice(spots)))
    return {'rows': rows, 'creatures': creatures, 'queries': queries}


def encode(world):
    rows = ';'.join('{}:{}'.format(*r) for r in world['rows'])
    creatures = []
    for c in world['creatures']:
        traits = ''.join(letter for letter, key in (('l', 'long_weapon'), ('h', 'hydra'), ('f', 'flying'),
                                                     ('i', 'ignore'), ('b', 'effect_7'), ('e', 'effect_2e')) if c[key])
        creatures.append('{}:{}:{}:{}:{}:{}:{}'.format(*c['position'], c['size'], c['facing'], c['side'], c['number'], traits or '-'))
    return '{} {}'.format(rows, ';'.join(creatures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=300)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(63)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# Z rows creatures -> threats answers',
             '# rows: start:end per row; creatures: x:y (subcells):size:facing:side:number:traits',
             '#   (l long_weapon 0x25, h hydra_strike 0x1d, f flying 0x19, i ignore_zones_of_control 0x20,',
             '#   b effect 7, e effect 0x2e);',
             '# threats: x:y:creatures (indices, list order) for every cell with a non-empty threat list (172000);',
             '# answers: mover:x:y:mask hex per query (172210; bit d = zone penalty in direction d)']
    for _ in range(args.count):
        world = random_world(rng)
        threats, answers = bench.run(world)
        lines.append('Z {} -> {} {}'.format(encode(world), threats, answers or '-'))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe zone-of-control fixtures'.format(args.count))


if __name__ == '__main__':
    main()
