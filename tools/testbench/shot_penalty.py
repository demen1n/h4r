#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the shot penalty chain (170800).

Executes the real t_battlefield method whole, with everything it calls: the
line-of-fire trace (16fe00), the edge distance (1f17d0) with the footprint center
(f100), the penalty table (2509f0), the effect and ability lookups. Fixture
boundaries are those of line_of_fire.py plus the threat lookup (10d200, a supplied
count). See decomp/shooting.md.
"""
import argparse
import importlib.util
import random
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('lof', Path(__file__).with_name('line_of_fire.py'))
lof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lof)
BASE = lof.BASE
THREAT = lof.HEAP + 0x3e000
RANGED, LONG_RANGE, SHORT_RANGE, SIEGE_MACHINE = 0x33, 0x24, 0x3c, 0x3d


class Bench(lof.Bench):
    def __init__(self, exe):
        super().__init__(exe)
        self.threats = 0
        address = BASE + 0x10d200
        self.uc.hook_add(UC_HOOK_CODE, self.threat_hook, begin=address, end=address)

    def threat_hook(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if uc.reg_read(UC_X86_REG_ECX) != lof.FIELD + 0xe8:
            raise RuntimeError('unexpected threat lookup')
        self.o.word(THREAT + 8, self.threats)
        uc.reg_write(UC_X86_REG_EAX, THREAT)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 8)  # the callee pops its argument

    def penalty(self, world):
        addresses = self.build(world)
        self.threats = world['threats']
        return int(self.o.call(0x170800, (lof.SHOOTER, addresses[world['target_name']]), this=lof.FIELD))


def random_shot(rng, long_range):
    world = lof.random_world(rng)
    rows = world['rows']
    if long_range:
        # A long thin field: the 20 and 40 cell steps of the penalty need distance.
        rows = [(0, rng.randint(2, 5)) for _ in range(rng.randint(30, 75))]
        world['rows'] = rows
        world['cells'] = [(x, y, 0x20 if rng.random() < 0.03 else 0, 5 if rng.random() < 0.03 else None, None)
                          for x, (s, e) in enumerate(rows) for y in range(s, e) if rng.random() < 0.03]
        world['cells'] = [c for c in world['cells'] if c[2] or c[3]]
        # Mostly clean long shots, so the distance steps are what changes the answer.
        if rng.random() < 0.7:
            world['cells'] = world['cells'][:1] if rng.random() < 0.3 else []
            world['stacks'] = [('A', 1 - world['shooter'][0], 0, [], rng.choice([1, 2]), (0, 0))]
            a = world['shooter']
            world['shooter'] = (a[0], 0, [], a[3], a[4])
            # Rows five cells wide keep the line inside; sometimes a whole row is an obstacle.
            rows = [(0, 5)] * len(rows)
            world['rows'] = rows
            if rng.random() < 0.4:
                world['cells'] = [(len(rows) // 2 - 1, y, 0x20, 5, None) for y in range(5)]
    if not world['stacks']:
        world['stacks'] = [('A', 1 - world['shooter'][0], 0, [], 1, (0, 0))]
    k = rng.randrange(len(world['stacks']))
    name, side, flag, effects, size, _ = world['stacks'][k]
    # The target stands in the field, in the cell its footprint covers.
    x = rng.randrange(len(rows) // 2, len(rows)) if long_range else rng.randrange(0, len(rows))
    y = rng.randrange(rows[x][0], rows[x][1])
    # The shot aims at the footprint's center, so the center is what lies in cell (x, y).
    position = (x * 16 + rng.randrange(0, 16) - size * 8, y * 16 + rng.randrange(0, 16) - size * 8)
    world['stacks'][k] = (name, side, flag, effects, size, position)
    world['cells'] = [c for c in world['cells'] if (c[0], c[1]) != (x, y)] + [(x, y, 0, None, name)]
    # The shooter at its own cell as well, usually.
    if long_range:
        sx = rng.randrange(0, len(rows) // 2)
        sy = rng.randrange(rows[sx][0], rows[sx][1])
        a = world['shooter']
        world['shooter'] = (a[0], a[1], a[2], a[3], (sx * 16 + rng.randrange(0, 8), sy * 16 + rng.randrange(0, 8)))
    clean = long_range and len(world['stacks']) == 1
    world['abilities'] = [a for a, p in ((RANGED, 0.97 if clean else 0.9), (LONG_RANGE, 0.2), (SHORT_RANGE, 0.4),
                                         (SIEGE_MACHINE, 0.3)) if rng.random() < p]
    world['shots'] = rng.choice([1, 3]) if clean else rng.choice([0, 1, 1, 3])
    world['threats'] = 0 if clean else rng.choice([0, 0, 0, 0, 1, 2])
    world['target_name'] = name
    world['target'] = (0, 0)
    world['exact'] = 0
    world['arg5'] = None
    return world


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=3000)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(670)
    lines = ['# Buka 3.0 SHA-256 ' + lof.reference.SHA256,
             '# P rows gate stacks shooter cells abilities shots threats target-name -> penalty',
             '# the world fields are those of line_of_fire_vectors.tsv (target and arg5 are not used);',
             '# abilities: numbers of the shooter; penalty: the divisor, 0 = no shot']
    for n in range(args.count):
        world = random_shot(rng, n % 3 == 0)
        fields = lof.encode(world).split(' ')[:5]
        abilities = ','.join(map(str, world['abilities'])) or '-'
        lines.append('P {} {} {} {} {} -> {}'.format(' '.join(fields), abilities, world['shots'],
                                                    world['threats'], world['target_name'], bench.penalty(world)))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe shot penalty fixtures'.format(args.count))


if __name__ == '__main__':
    main()
