#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the shot's line-of-fire trace (16fe00).

Executes the real t_battlefield method whole: the step arithmetic, the row bounds
(d2290, d2270), the cell indexing, the obstacle type (477790), the shooter's
footprint center (28c60) and every virtual call on creatures. Fixture boundaries,
each a plain lookup in supplied data: the obstacle and creature of a cell
(1838d0, 183870 walk std::lists with RTTI casts), the gate's "is open" (1c0570),
and the creatures' virtual accessors (side, size, effects). See decomp/shooting.md.
"""
import argparse
import importlib.util
import random
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

BASE = reference.BASE
HEAP = 0x7000000
VB = 0xe00
FIELD = HEAP + 0x30000
RECORDS, CELLS, GATE = HEAP + 0x32000, HEAP + 0x34000, HEAP + 0x38000
OBSTACLES = 0x7100000  # its own mapping: one small object per cell
SHOOTER, STACKS = HEAP + 0x10000, HEAP + 0x11000
OUT, POINT = HEAP + 0x3f000, HEAP + 0x3f100
FIELD_SIDE = 15


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x40000)
        self.uc.mem_map(OBSTACLES, 0x100000)
        self.cell_obstacle, self.cell_creature, self.gate_open = {}, {}, 0
        self.hooks = {BASE + 0x1838d0: self.get_obstacle, BASE + 0x183870: self.get_creature,
                      BASE + 0x1c0570: self.is_open}
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)

    def hook(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, self.hooks[address](uc.reg_read(UC_X86_REG_ECX)))
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def get_obstacle(self, cell):
        return self.cell_obstacle.get(cell, 0)

    def get_creature(self, cell):
        return self.cell_creature.get(cell, 0)

    def is_open(self, this):
        if this != GATE:
            raise RuntimeError('unexpected gate')
        return self.gate_open

    def creature(self, obj, slot, side, flag, effects, size, position):
        o = self.o
        self.uc.mem_write(obj, bytes(0x1000))
        table, vbtable, base_vt, effects_vt = obj + 0xf00, obj + 0xf80, obj + 0xfc0, obj + 0xe80
        o.word(obj + 0x98, table)
        o.word(table + 0x74, o.accessor(slot, side, pop=0))
        o.word(obj + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(obj + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(slot + 1, effects))
        o.word(obj + 8, base_vt)
        o.word(base_vt + 8, o.accessor(slot + 2, size, pop=0))
        for i, v in enumerate(position + (0,)):
            o.word(obj + 0xc + 4 * i, v)
        self.uc.mem_write(obj + 0x9bc, bytes((flag,)))

    def run(self, world):
        o, uc = self.o, self.uc
        uc.mem_write(FIELD, bytes(0x400))
        rows = world['rows']
        grid = FIELD + 0x68
        info = HEAP + 0x39000
        o.word(grid, len(rows))
        o.word(grid + 0x14, info)
        o.word(info + 8, RECORDS)
        o.word(grid + 0x20, CELLS)
        index, bases = 0, []
        for x, (start, end) in enumerate(rows):
            o.word(RECORDS + 12 * x, index - start)
            o.word(RECORDS + 12 * x + 4, start)
            o.word(RECORDS + 12 * x + 8, end)
            bases.append(index - start)
            index += end - start
        uc.mem_write(CELLS, bytes(28 * max(index, 1)))
        o.word(FIELD + 0x340, 0 if world['gate'] is None else GATE + 0x28)
        self.gate_open = 0 if world['gate'] is None else world['gate']
        self.creature(SHOOTER, 3, *world['shooter'])
        addresses = {'S': SHOOTER}
        for k, (name, side, flag, effects, size, position) in enumerate(world['stacks']):
            addresses[name] = STACKS + 0x1000 * k
            self.creature(addresses[name], 6 + 3 * k, side, flag, effects, size, position)
        self.cell_obstacle, self.cell_creature = {}, {}
        for n, (x, y, flags, obstacle, creature) in enumerate(world['cells']):
            cell = CELLS + 28 * (bases[x] + y)
            uc.mem_write(cell + 1, bytes((flags,)))
            if obstacle is not None:
                obj = OBSTACLES + 0x200 * n
                uc.mem_write(obj, bytes(0x200))
                o.word(obj + 0xa0, obj + 0x100)
                o.word(obj + 0x100 + 0x88, obstacle)
                self.cell_obstacle[cell] = obj
            if creature is not None:
                self.cell_creature[cell] = addresses[creature]
        o.word(POINT, world['target'][0])
        o.word(POINT + 4, world['target'][1])
        o.word(OUT, 0xdead)
        arg5 = addresses[world['arg5']] if world['arg5'] else 0
        hit = int(o.call(0x16fe00, (SHOOTER, POINT, OUT, world['exact'], arg5), this=FIELD))
        names = {v: k for k, v in addresses.items()}
        return [names.get(hit, '-') if hit else '-', o.read_word(OUT)]


def random_world(rng):
    rows = []
    for _ in range(rng.randint(1, FIELD_SIDE)):
        start = rng.choice([0, 0, 0, 1, 3])
        rows.append((start, start + rng.randint(1, FIELD_SIDE)))
    names = ['A', 'B', 'C', 'D'][:rng.randint(0, 4)]
    size_of = {}
    stacks = []
    for name in names:
        size_of[name] = rng.choice([1, 1, 2, 3])
        effects = [e for e in (6, 0x89, 0x2f) if rng.random() < 0.15]
        stacks.append((name, rng.randint(0, 1), int(rng.random() < 0.3), effects, size_of[name],
                       (rng.randrange(0, 200), rng.randrange(0, 200))))
    shooter = (rng.randint(0, 1), int(rng.random() < 0.3), [e for e in (6, 0x89) if rng.random() < 0.1],
               rng.choice([1, 1, 2]), (rng.randrange(-8, 16 * len(rows)), rng.randrange(-8, 16 * (len(rows) + 2))))
    density = rng.choice([0.05, 0.15, 0.3, 0.5])
    cells = []
    for x, (start, end) in enumerate(rows):
        for y in range(start, end):
            flags = rng.choice([0, 0, 0x20, 0x21, 0x20, 0x01]) if rng.random() < density * 2 else 0
            obstacle = rng.choice([None, 0x24, 0x22, 0x24, 5, 0x23]) if flags & 0x20 else None
            creature = None
            if rng.random() < density:
                creature = rng.choice(names + ['S']) if names else 'S'
            if flags or creature:
                cells.append((x, y, flags, obstacle, creature))
    target = (rng.randrange(-8, 16 * len(rows) + 8), rng.randrange(-8, 16 * (len(rows) + 2)))
    exact = int(rng.random() < 0.3)
    if exact and stacks and rng.random() < 0.7:
        # make one stack's center equal the target so the exact-center branch matters
        k = rng.randrange(len(stacks))
        name, side, flag, effects, size, _ = stacks[k]
        target = (rng.randrange(0, 16 * 10), rng.randrange(0, 16 * 10))
        stacks[k] = (name, side, flag, effects, size, (target[0] - size * 8, target[1] - size * 8))
    return {'rows': rows, 'gate': rng.choice([None, 0, 1]), 'stacks': stacks, 'shooter': shooter,
            'cells': cells, 'target': target, 'exact': exact,
            'arg5': rng.choice([None, None] + names)}


def encode(world):
    dash = lambda items: ','.join(map(str, items)) if items else '-'
    stacks = ';'.join('{}:{}:{}:{}:{}:{}:{}'.format(n, s, f, dash(e), z, p[0], p[1])
                      for n, s, f, e, z, p in world['stacks']) or '-'
    side, flag, effects, size, position = world['shooter']
    cells = ';'.join('{}:{}:{}:{}:{}'.format(x, y, f, '-' if o is None else o, c or '-')
                     for x, y, f, o, c in world['cells']) or '-'
    return ' '.join([
        ';'.join('{}:{}'.format(*r) for r in world['rows']),
        '-' if world['gate'] is None else str(world['gate']),
        stacks,
        '{}:{}:{}:{}:{}:{}'.format(side, flag, dash(effects), size, *position),
        cells, '{}:{}'.format(*world['target']), str(world['exact']), world['arg5'] or '-'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=3000)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(67)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# T rows gate stacks shooter cells target exact arg5 -> hit out',
             '# rows: start:end per row; gate: - none, 0 closed, 1 open; stacks: name:side:flag9bc:effects:size:px:py;',
             '# shooter: side:flag9bc:effects:size:px:py (S in cells); cells: x:y:flags:obstacle:stack; target: x:y subcells;',
             '# hit: name of the stack returned, - for none; out: 0 clear, 1 obstructed, 2 blocked']
    for _ in range(args.count):
        world = random_world(rng)
        lines.append('T {} -> {}'.format(encode(world), ' '.join(map(str, bench.run(world)))))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe line-of-fire fixtures'.format(args.count))


if __name__ == '__main__':
    main()
