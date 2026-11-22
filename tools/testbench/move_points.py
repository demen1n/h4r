#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the movement points of a combat creature.

Executes 1ef750 (virtual +0x18 of t_combat_creature+0x98) whole, together with the
base it asks the army member for through the real vftables: a creature (23d5b0 ->
255370: the traits record, Move column at +0x5c, x100) or a hero (32a1f0). Fixture
boundaries: the traits table holds only the fixture's Move value (its "loaded" flag
is set instead of reading table.creatures), abilities (virtual +0x68) and active
effects (virtual +8 of the virtual base) are byte lookups, and the army bonus
(+0xaf8), artifact bonus (+0xa10), the battlefield's at-sea byte (+0x36c) and the
mode (+0xa2c) are plain fields. See decomp/move_points.md.
"""
import argparse
import importlib.util
import itertools
import random
from pathlib import Path

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

HEAP = 0x7000000
CREATURE, MEMBER, FIELD = HEAP, HEAP + 0x2000, HEAP + 0x3000
STATS = CREATURE + 0x98
VB = 0x200
# Real vftables of the army member's +8 part (RTTI): t_creature, t_hero.
CREATURE_VFT, HERO_VFT = 0x984624, 0x989298
TRAITS, TRAITS_LOADED, TRAITS_SIZE = 0xab2858, 0xab6919, 0xe0
SEA_BONUS = 0x3a
EFFECTS = [0x01, 0x07, 0x37, 0x68, 0x69, 0x8d]


def signed(value):
    return value - (1 << 32) if value >= 1 << 31 else value


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.o.uc.mem_map(HEAP, 0x4000)
        self.o.uc.mem_write(TRAITS_LOADED, b'\x01')

    def run(self, member, base, army, artifact, at_sea, abilities, effects, mode):
        o, uc = self.o, self.o.uc
        uc.mem_write(HEAP, bytes(0x4000))
        table, vbtable, effects_vt = CREATURE + 0x1c00, CREATURE + 0x1d00, CREATURE + 0x1e00
        o.word(STATS, table)
        o.word(table + 0x68, o.lookup(0, abilities))
        o.word(STATS + 4, vbtable)
        o.word(vbtable + 8, VB)
        o.word(STATS + 4 + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(1, effects))
        o.word(CREATURE + 0xa4, MEMBER)
        if member == 'c':
            o.word(MEMBER + 8, CREATURE_VFT)
            creature = 3
            o.word(MEMBER + 8 + 0x94, creature)
            o.word(TRAITS + creature * TRAITS_SIZE + 0x5c, base)
        else:
            o.word(MEMBER + 8, HERO_VFT)
        o.word(CREATURE + 0xaf8, army)
        o.word(CREATURE + 0xa10, artifact)
        o.word(CREATURE + 0xa18, FIELD)
        uc.mem_write(FIELD + 0x36c, bytes((at_sea,)))
        o.word(CREATURE + 0xa2c, mode)
        return signed(int(o.call(0x1ef750, (), this=STATS)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=1500)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(184)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# M member base army artifact at_sea sea_bonus effects mode -> points',
             '# member: c creature (base = table Move) or h hero (base ignored);',
             '# effects: comma-separated hex ids or -; mode: +0xa2c (0, 1, 2)']
    cases = []
    # Every combination of the switches on a few bases and bonuses.
    for member, base, (army, artifact), at_sea, sea, mode in itertools.product(
            'ch', [0, 5, 7], [(0, 0), (300, 0), (0, -250), (-900, -900)], [0, 1], [0, 1], [0, 1, 2]):
        for k in range(0, len(EFFECTS) + 1, 3):
            cases.append((member, base, army, artifact, at_sea, sea, EFFECTS[k:k + 3], mode))
    for effect in EFFECTS:
        for member in 'ch':
            cases.append((member, 9, 0, 0, 0, 0, [effect], 0))
    while len(cases) < args.count:
        cases.append((rng.choice('cch'), rng.randint(-3, 40), rng.choice([0, 0, rng.randint(-3000, 3000)]),
                      rng.choice([0, 0, rng.randint(-3000, 3000)]), rng.randint(0, 1), rng.randint(0, 1),
                      [e for e in EFFECTS if rng.random() < 0.25], rng.choice([0, 0, 0, 1, 2])))
    for member, base, army, artifact, at_sea, sea, effects, mode in cases:
        points = bench.run(member, base, army, artifact, at_sea, [SEA_BONUS] if sea else [], effects, mode)
        lines.append('M {} {} {} {} {} {} {} {} -> {}'.format(
            member, base, army, artifact, at_sea, sea,
            ','.join('{:x}'.format(e) for e in effects) or '-', mode, points))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe movement point fixtures'.format(len(cases)))


if __name__ == '__main__':
    main()
