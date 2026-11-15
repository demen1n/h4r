#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the retaliation and "can shoot" checks.

Executes the retaliation check of t_combat_creature (1f23c0) whole, including the
real "can shoot" check of t_battlefield (170780), footprint centers (f100, 28c60), the x87
distance and __ftol. Fixture boundaries: abilities and effects (virtual lookups),
footprint size (virtual +8), the effect eligibility check 1b6120 (a supplied
answer per spell; ported separately in part, decomp/damage.md) and the
battlefield threat map lookup 10d200 (a supplied count). See
decomp/retaliation.md.
"""
import argparse
import importlib.util
import itertools
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
SUB = 0x9b4
NO_RETALIATION, LONG_WEAPON, PANIC, RANGED, DEATH_PROTECTION = 0x2e, 0x25, 0x30, 0x33, 0x10


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x40000)
        self.a, self.d, self.field = HEAP + 0x10000, HEAP + 0x20000, HEAP + 0x30000
        self.threat = HEAP + 0x38000
        self.can_affect = {}
        self.hooks = {BASE + 0x1b6120: (self.effect_can_affect, 0),
                      BASE + 0x10d200: (self.threats, 4)}
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)

    def hook(self, uc, address, size, _):
        handler, pop = self.hooks[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        handler(uc, sp)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def effect_can_affect(self, uc, sp):
        spell, target = self.o.read_word(sp + 4), self.o.read_word(sp + 8)
        if target != self.d or spell not in self.can_affect:
            raise RuntimeError('unexpected effect eligibility call {:x} {:x}'.format(spell, target))
        uc.reg_write(UC_X86_REG_EAX, int(self.can_affect[spell]))

    def threats(self, uc, sp):
        if uc.reg_read(UC_X86_REG_ECX) != self.field + 0xe8 or self.o.read_word(sp + 4) != self.d:
            raise RuntimeError('unexpected threat lookup')
        uc.reg_write(UC_X86_REG_EAX, self.threat)

    def creature(self, obj, slot, abilities, effects, size, position, shots=0):
        o = self.o
        self.uc.mem_write(obj, bytes(0x1000))
        table, vbtable, base_vt = obj + 0xf00, obj + 0xf80, obj + 0xfc0
        o.word(obj + 0x98, table)
        o.word(table + 0x68, o.lookup(slot, abilities))
        o.word(table + 0x4c, o.accessor(slot, shots, pop=0))
        o.word(obj + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        effects_vt = obj + 0xe80
        o.word(obj + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(slot + 1, effects))
        # t_combat_object_base at +8: virtual +8 is the footprint size; +0xc the position.
        o.word(obj + 8, base_vt)
        o.word(base_vt + 8, o.accessor(slot + 2, size, pop=0))
        for i, v in enumerate(position):
            o.word(obj + 0xc + 4 * i, v)
        o.word(obj + 0xa18, self.field)

    def check(self, n, ranged, flag=0, ba=None, x9a=None,
              a_abilities=(), a_effects=(), a_size=1, a_pos=(0, 0),
              d_abilities=(), d_effects=(), d_size=1, d_pos=(0, 0), shots=0, threat=0,
              affect25=0, affect6b=0):
        effects = list(d_effects) + ([0xba] if ba is not None else []) + ([0x9a] if x9a is not None else [])
        self.creature(self.a, 10, list(a_abilities), list(a_effects), a_size, a_pos + (0,))
        self.creature(self.d, 20, list(d_abilities), effects, d_size, d_pos + (0,), shots=shots)
        self.uc.mem_write(self.d + 0xa9, bytes((flag,)))
        self.o.word(self.d + 0x990, 0 if ba is None else ba)
        self.o.word(self.d + 0x810, 0 if x9a is None else x9a)
        self.o.word(self.threat + 8, threat)
        self.can_affect = {0x25: affect25, 0x6b: affect6b}
        result = int(self.o.call(0x1f23c0, (self.a, ranged, n & 0xffffffff), this=self.d + SUB)) & 0xff
        return [result]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bench = Bench(args.exe)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# F n flag effect-0xba(value|-) effect-0x9a(value|-) -> can (melee, adjacent 1x1 stacks)',
             '# S n flag ranged-ability effect-0x2f shots threats -> can (ranged retaliation)',
             '# G attacker-long target-long attacker-size target-size ax ay tx ty -> can (melee reach; positions in subcells)',
             '# E no_retaliation effect-0x24 can-affect-0x25 panic can-affect-0x6b death_protection -> can']
    header = len(lines)

    def emit(kind, inputs, outputs):
        lines.append(' '.join([kind] + [str(v) for v in list(inputs) + list(outputs)]))

    near = dict(a_pos=(800, 784), d_pos=(800, 800))
    for n, flag, ba, x9a in itertools.product([-1, 0, 1, 2, 3], [0, 1], [None, 0, 1, 2, 5], [None, 0, 1, 2, 5]):
        emit('F', (n, flag, '-' if ba is None else ba, '-' if x9a is None else x9a),
             bench.check(n, 0, flag=flag, ba=ba, x9a=x9a, **near))
    for n, flag, ranged, x2f, shots, threat in itertools.product([1, 2], [0, 1], [0, 1], [0, 1], [0, 1, 3], [0, 1, 2]):
        emit('S', (n, flag, ranged, x2f, shots, threat),
             bench.check(n, 1, flag=flag, d_abilities=[RANGED] if ranged else [],
                         d_effects=[0x2f] if x2f else [], shots=shots, threat=threat, **near))
    offsets = [0, 5, 8, 15, 16, 23, 31, 32, 40, 47, 48, 63, 64, 72, 80, 95, 96, 101, 112, 128, 150]
    for a_long, d_long, a_size, d_size in itertools.product([0, 1], [0, 1], [1, 2, 3], [1, 2]):
        for t_shift in [0, 7]:
            tx = ty = 800 + t_shift
            for dx, dy in itertools.product(offsets, [0, 16, 37, 64]):
                ax, ay = 800 - dx, 800 + dy
                emit('G', (a_long, d_long, a_size, d_size, ax, ay, tx, ty), bench.check(
                    1, 0, a_abilities=[LONG_WEAPON] if a_long else [], d_abilities=[LONG_WEAPON] if d_long else [],
                    a_size=a_size, d_size=d_size, a_pos=(ax, ay), d_pos=(tx, ty)))
    for case in itertools.product([0, 1], repeat=6):
        no_ret, e24, aff25, panic, aff6b, death = case
        emit('E', case, bench.check(
            1, 0, a_abilities=([NO_RETALIATION] if no_ret else []) + ([PANIC] if panic else []),
            a_effects=[0x24] if e24 else [], d_abilities=[DEATH_PROTECTION] if death else [],
            affect25=aff25, affect6b=aff6b, **near))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe retaliation fixtures'.format(len(lines) - header))


if __name__ == '__main__':
    main()
