#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for combat morale.

Executes whole:
- A: the army's morale for a member of one alignment (23fe40) with the alignment
  relation (11ecd0) and its table (VA a648d8), over army members built on the real
  vftables of t_creature and t_hero: count (23d730 / 67a50), alignment (23d980 through
  the traits 255370, +0x44 / the hero's +0xa0) and the undead flag (23d9f0: traits
  +0xd4 bit 13 / f140, never);
- B: the morale getter of a combat creature (1efbd0) and its spell-free variant
  (1efb90);
- C: a stack's strength for the morale (1f5350 with the arguments 0, -1 the morale code
  passes);
- D: the loss penalty update at a new round (176250) over a list of creatures.

Fixture boundaries: the creature traits table holds only the fixture's alignment and
undead bit (its "loaded" flag is set instead of reading table.creatures); abilities
(virtual +0x68 of the stats) and active effects (virtual +8 of the virtual base) are byte
lookups; the hit points of one creature (virtual +0x30 of the army member) and, in D,
each creature's strength (virtual +0x2c of its +0x9b4 part) are supplied values.
See decomp/morale.md.
"""
import argparse
import importlib.util
import random
from pathlib import Path

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

HEAP = 0x7000000
ARMY, MEMBERS, CREATURE, FIELD, NODES = HEAP, HEAP + 0x1000, HEAP + 0x4000, HEAP + 0x8000, HEAP + 0x9000
UNITS = HEAP + 0x10000  # 0x1000 per creature in D
VB = 0x200
CREATURE_MAIN, CREATURE_PART = 0x98469c, 0x984624
HERO_MAIN, HERO_PART = 0x989310, 0x989298
TRAITS, TRAITS_LOADED, TRAITS_SIZE = 0xab2858, 0xab6919, 0xe0
MORALE_EFFECTS = [0x19, 0x67, 0x91, 0x94]
STRENGTH_EFFECTS = [0x10, 0x21, 0x33, 0x39, 0x44, 0x72]
UNDEAD, MECHANICAL = 0x4d, 0x2a


def signed(value):
    value &= 0xffffffff
    return value - (1 << 32) if value >= 1 << 31 else value


def hexes(values):
    return ','.join('{:x}'.format(v) for v in values) or '-'


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.o.uc.mem_map(HEAP, 0x40000)
        self.o.uc.mem_write(TRAITS_LOADED, b'\x01')

    def army(self, own, members):
        """members: ('c', alignment, count, undead) or ('h', alignment)."""
        o, uc = self.o, self.o.uc
        uc.mem_write(ARMY, bytes(0x4000))
        # Seven slots always hold an object; a free one is a stack of 0 (empty slot).
        members = list(members) + [('c', 0, 0, False)] * (7 - len(members))
        for i, member in enumerate(members):
            m = MEMBERS + 0x400 * i
            o.word(ARMY + 0x10 + 4 * i, m)
            if member[0] == 'c':
                _, alignment, count, undead = member
                o.word(m, CREATURE_MAIN)
                o.word(m + 8, CREATURE_PART)
                o.word(m + 0x9c, i)  # creature id = slot: its own traits record
                o.word(m + 0xa0, count)
                record = TRAITS + i * TRAITS_SIZE
                uc.mem_write(record, bytes(TRAITS_SIZE))
                o.word(record + 0x44, alignment)
                o.word(record + 0xd4, 1 << 13 if undead else 0)
            else:
                o.word(m, HERO_MAIN)
                o.word(m + 8, HERO_PART)
                o.word(m + 0xa8, member[1])
        return signed(int(o.call(0x23fe40, (own,), this=ARMY)))

    def creature(self, abilities, effects):
        o, uc = self.o, self.o.uc
        uc.mem_write(CREATURE, bytes(0x2000))
        table, vbtable, effects_vt = CREATURE + 0x1c00, CREATURE + 0x1d00, CREATURE + 0x1e00
        o.word(CREATURE + 0x98, table)
        o.word(table + 0x68, o.lookup(0, abilities))
        o.word(CREATURE + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(CREATURE + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(1, effects))

    def morale(self, base, influence, abilities, effects):
        o = self.o
        self.creature(abilities, effects)
        o.word(CREATURE + 0xae8, base)
        o.word(CREATURE + 0xaec, influence)
        return (signed(int(o.call(0x1efbd0, (), this=CREATURE))),
                signed(int(o.call(0x1efb90, (), this=CREATURE))))

    def strength(self, count, hp, wounds, effects, celestial, drain):
        o = self.o
        self.creature([], effects)
        member, member_vt = CREATURE + 0x1800, CREATURE + 0x1900
        o.word(CREATURE + 0xa4, member)
        o.word(member + 8, member_vt)
        o.word(member_vt + 0x30, o.accessor(0, hp, pop=0))
        o.word(CREATURE + 0xb0, count)
        o.word(CREATURE + 0xa0 + VB, wounds)  # +4 of the virtual base
        o.word(CREATURE + 0x198, celestial)
        o.word(CREATURE + 0x408, drain)
        return signed(int(o.call(0x1f5350, (0, 0xffffffff), this=CREATURE + 0x9b4)))

    def losses(self, initial, old, units):
        """units: (side, strength, ae8). Returns new penalties and every ae8."""
        o, uc = self.o, self.o.uc
        uc.mem_write(FIELD, bytes(0x1000))
        head = NODES
        nodes = [NODES + 0x10 * (i + 1) for i in range(len(units))]
        ring = [head] + nodes
        for i, node in enumerate(ring):
            o.word(node, ring[(i + 1) % len(ring)])
            o.word(node + 4, ring[i - 1])
        o.word(FIELD + 0xd4, head)
        for k, (side, value, base) in enumerate(units):
            c = UNITS + 0x1000 * k
            uc.mem_write(c, bytes(0x1000))
            uc.mem_write(c + 0xa0, bytes((side,)))
            vt = c + 0xc00
            o.word(c + 0x9b4, vt)
            o.word(vt + 0x2c, o.accessor(10 + k, value, pop=8))
            o.word(c + 0xae8, base)
            o.word(nodes[k] + 8, c)
        for s in range(2):
            o.word(FIELD + 0x454 + 4 * s, initial[s])
            o.word(FIELD + 0x40c + 4 * s, old[s])
        o.call(0x176250, (), this=FIELD)
        penalties = [signed(o.read_word(FIELD + 0x40c + 4 * s)) for s in range(2)]
        bases = [signed(o.read_word(UNITS + 0x1000 * k + 0xae8)) for k in range(len(units))]
        return penalties, bases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(61)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# A own members -> army morale (23fe40); member c:alignment:count:undead or h:alignment',
             '# B base influence abilities effects -> morale (1efbd0) morale_without_spells (1efb90)',
             '# C count hp wounds effects celestial drain -> strength (1f5350 with 0, -1)',
             '# D initial0 initial1 old0 old1 units(side:strength:ae8) -> penalty0 penalty1 ae8... (176250)',
             '# abilities/effects: comma-separated hex ids or -']
    count = {'A': 0, 'B': 0, 'C': 0, 'D': 0}

    # A: every own alignment with every single other member, then random armies.
    armies = []
    for own in range(6):
        for alignment in range(6):
            armies.append((own, [('c', alignment, 5, False)]))
            armies.append((own, [('h', alignment)]))
            armies.append((own, [('c', alignment, 3, True)]))
    for _ in range(600):
        members = []
        for _ in range(7):
            kind = rng.random()
            if kind < 0.2:
                members.append(('c', rng.randrange(6), 0, rng.random() < 0.3))  # empty slot
            elif kind < 0.35:
                members.append(('h', rng.randrange(6)))
            else:
                members.append(('c', rng.randrange(6), rng.choice([1, 7, 30]), rng.random() < 0.15))
        armies.append((rng.randrange(6), members))
    for own, members in armies:
        text = ';'.join('h:{}'.format(m[1]) if m[0] == 'h' else 'c:{}:{}:{}'.format(m[1], m[2], int(m[3]))
                        for m in members)
        lines.append('A {} {} -> {}'.format(own, text, bench.army(own, members)))
        count['A'] += 1

    # B: every effect/ability combination on a few bases, then random values.
    cases = []
    for mask in range(1 << 6):
        effects = [e for i, e in enumerate(MORALE_EFFECTS) if mask >> i & 1]
        abilities = [a for i, a in enumerate([UNDEAD, MECHANICAL]) if mask >> (4 + i) & 1]
        for base, influence in [(0, 0), (3, 2), (-4, -3), (12, 0), (-7, -8)]:
            cases.append((base, influence, abilities, effects))
    for _ in range(600):
        cases.append((rng.randint(-25, 25), rng.choice([0, 0, rng.randint(-10, 10)]),
                      [a for a in (UNDEAD, MECHANICAL) if rng.random() < 0.1],
                      [e for e in MORALE_EFFECTS if rng.random() < 0.2]))
    for base, influence, abilities, effects in cases:
        morale, plain = bench.morale(base, influence, abilities, effects)
        lines.append('B {} {} {} {} -> {} {}'.format(base, influence, hexes(abilities), hexes(effects), morale, plain))
        count['B'] += 1

    # C: strength.
    for _ in range(800):
        hp = rng.choice([1, 2, 3, 5, 10, 25, 40, 150, 300, rng.randint(1, 1000)])
        n = rng.choice([0, 1, 2, 7, 30, rng.randint(0, 500)])
        wounds = rng.randint(0, hp - 1) if n else 0
        effects = [e for e in STRENGTH_EFFECTS if rng.random() < 0.3]
        celestial, drain = rng.choice([0, rng.randint(1, 500)]), rng.choice([0, rng.randint(1, 500)])
        value = bench.strength(n, hp, wounds, effects, celestial, drain)
        lines.append('C {} {} {} {} {} {} -> {}'.format(n, hp, wounds, hexes(effects), celestial, drain, value))
        count['C'] += 1

    # D: loss penalties.
    for _ in range(500):
        initial = [rng.choice([1, 10, 100, rng.randint(1, 20000)]) for _ in range(2)]
        old = [rng.choice([0, 0, -rng.randint(0, 10)]) for _ in range(2)]
        units = []
        for _ in range(rng.randint(1, 8)):
            side = rng.randrange(2)
            units.append((side, rng.randint(0, max(1, initial[side] // 2)), rng.randint(-15, 15)))
        if rng.random() < 0.2:  # a side with more than it started with
            units.append((0, initial[0] * 2, 0))
        penalties, bases = bench.losses(initial, old, units)
        lines.append('D {} {} {} {} {} -> {} {} {}'.format(
            initial[0], initial[1], old[0], old[1],
            ';'.join('{}:{}:{}'.format(*u) for u in units), penalties[0], penalties[1],
            ' '.join(map(str, bases))))
        count['D'] += 1

    args.out.write_text('\n'.join(lines) + '\n')
    print('original-exe morale fixtures: ' + ', '.join('{} {}'.format(k, v) for k, v in count.items()))


if __name__ == '__main__':
    main()
