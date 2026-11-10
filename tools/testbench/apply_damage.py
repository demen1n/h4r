#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for applying damage to a stack.

Executes t_combat_creature::resolve_damage (1eee40) and ::kill (1f3860) whole,
and the pending-damage cap of ::add_damage (block 1f2a91..1f2ab8).
Fixture inputs: number, wounds, pending damage, hit points (virtual +0x30),
the permanent-death flag and counter, total hits (virtual +0x2c).
Stubbed: effect lookups (always absent), text formatting, the floating damage
text (its arguments are recorded), animation refresh, clear_influence.
Spell shields (effects 0x10, 0x44), poison (0x6f), luck and the martyr
redirect inside add_damage are excluded.
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

VBASE = 0xe00          # fixture offset of the virtual base inside the object
HP = 0x98 + 0x700      # fixture storage for the hit points accessor
SENTINEL = 0x7777      # "last killed" before the call: shows whether it was written

# address -> bytes of stack arguments the callee pops
STUBS = {
    0x71d190: 0,   # format a number into a string (cdecl, varargs)
    0x566010: 12,  # t_battlefield::add_damage_text(creature, damage, killed)
    0x563850: 4,   # refresh the stack's number label
    0x5f16a0: 0,   # t_combat_creature::clear_influence
    0x5f4ad0: 0,   # ability 0x35 follow-up
    0x5f1f80: 8,   # remove an effect
}


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.heap = 0x7000000
        self.uc.mem_map(self.heap, 0x20000)
        self.obj = self.heap + 0x3000
        self.table, self.vbtable, self.effects = self.heap + 0x1000, self.heap + 0x1200, self.heap + 0x1300
        code = self.heap + 0x1400
        # get_hit_points: ECX is object+0x98
        self.uc.mem_write(code, b'\x8b\x81' + struct.pack('<I', HP - 0x98) + b'\xc3')
        self.o.word(self.table + 0x30, code)
        self.o.word(self.vbtable + 8, VBASE)
        self.o.word(self.effects + 8, self.o.accessor(31, 0))  # no effect is present
        # the attackable-object vtable used by add_damage: +0x2c get_total_hits
        self.attackable = self.heap + 0x1600
        self.text = None
        self.cleared = False
        for address in STUBS:
            self.uc.hook_add(UC_HOOK_CODE, self.stub, begin=address, end=address)

    def stub(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        args = [self.o.read_word(sp + 4 * (i + 1)) for i in range(3)]
        if address == 0x71d190:
            uc.mem_write(args[0], bytes(16))  # an empty std::string
            uc.reg_write(UC_X86_REG_EAX, args[0])
        elif address == 0x566010:
            self.text = (args[1], args[2])
        elif address == 0x5f16a0:
            self.cleared = True
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + STUBS[address])

    def setup(self, count, wounds, pending, hp, permanent=0, deaths=0):
        o, obj = self.o, self.obj
        self.uc.mem_write(obj, bytes(0x1000))
        o.word(obj + 0x98, self.table)
        o.word(obj + 0x9c, self.vbtable)
        o.word(obj + VBASE + 0x9c, self.effects)
        o.word(obj + VBASE + 0xa0, wounds)
        o.word(obj + 0xb0, count)
        o.word(obj + 0xb8, deaths)
        o.word(obj + 0x9c0, pending)
        o.word(obj + 0xa68, SENTINEL)
        o.word(obj + HP, hp)
        self.uc.mem_write(obj + 0xafc, bytes((permanent,)))
        self.text = None
        self.cleared = False

    def signed(self, address):
        value = self.o.read_word(address)
        return value - 2**32 if value >= 2**31 else value

    def state(self):
        obj = self.obj
        last = self.signed(obj + 0xa68)
        return [self.signed(obj + 0xb0), self.signed(obj + VBASE + 0xa0),
                '-' if last == SENTINEL else last]

    def resolve(self, count, wounds, pending, hp, permanent, deaths):
        self.setup(count, wounds, pending, hp, permanent, deaths)
        self.o.call(0x1eee40, this=self.obj)
        text = '- -' if self.text is None else '{} {}'.format(*self.text)
        return self.state() + [self.signed(self.obj + 0x9c0), self.signed(self.obj + 0xb8),
                               self.uc.mem_read(self.obj + 0xafc, 1)[0], text, int(self.cleared)]

    def kill(self, count, wounds, number):
        self.setup(count, wounds, 0, 10)
        self.o.call(0x1f3860, (number,), this=self.obj)
        return self.state()

    def accept(self, damage, pending, total):
        uc, o = self.uc, self.o
        sub = self.obj + 0x9b4
        self.setup(1, 0, pending, 10)
        o.word(sub, self.attackable)
        o.word(self.attackable + 0x2c, o.accessor(30, total, pop=8))
        uc.reg_write(UC_X86_REG_ESI, sub)
        uc.reg_write(UC_X86_REG_EBX, damage)
        uc.reg_write(UC_X86_REG_ESP, reference.STACK + 0x8000)
        uc.emu_start(reference.BASE + 0x1f2a91, reference.BASE + 0x1f2ab8, count=100)
        if uc.reg_read(UC_X86_REG_EIP) != reference.BASE + 0x1f2ab8:
            raise RuntimeError('pending-damage block did not finish')
        accepted = uc.reg_read(UC_X86_REG_EBX)
        return [accepted - 2**32 if accepted >= 2**31 else accepted, self.signed(self.obj + 0x9c0)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bench = Bench(args.exe)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# R count wounds pending hp permanent deaths -> count wounds last-killed pending deaths permanent text-damage text-killed influence-cleared',
             '# K count wounds number -> count wounds last-killed',
             '# A damage pending total-hits -> accepted pending',
             '# last-killed "-" means the field was not written; text "- -" means no floating text']

    def fmt(kind, inputs, outputs):
        lines.append('{} {} {}'.format(kind, ' '.join(map(str, inputs)), ' '.join(map(str, outputs))))

    rng = random.Random(68)
    cases = set()
    for count in [0, 1, 2, 5, 100, 30000]:
        for hp in [1, 7, 10, 250]:
            for wounds in sorted({0, 1, hp // 2, hp - 1, hp, hp + 3}):
                total = max(count * hp - wounds, 0)
                for pending in sorted({0, 1, hp - 1, hp, hp + 1, 3 * hp, max(total - 1, 0), total, total + 1, total + 5 * hp}):
                    cases.add((count, wounds, pending, hp, 0, 0))
    for _ in range(600):
        hp = rng.choice([1, 3, 10, 35, 250, 1000])
        count = rng.choice([0, 1, 2, 3, 10, 50, 999])
        cases.add((count, rng.randrange(hp), rng.randrange(0, (count + 2) * hp + 1), hp,
                   rng.randrange(2), rng.choice([0, 0, 4])))
    for case in sorted(cases):
        fmt('R', case, bench.resolve(*case))
    for count in [0, 1, 2, 5, 100]:
        for wounds in [0, 3, 9]:
            for number in [0, 1, 2, count - 1, count, count + 1, 1000]:
                if number >= 0:
                    fmt('K', (count, wounds, number), bench.kill(count, wounds, number))
    for total in [0, 1, 26, 1000]:
        for pending in sorted({0, 1, total // 2, max(total - 1, 0), total}):
            for damage in sorted({1, 2, total // 2 + 1, max(total - pending - 1, 1), max(total - pending, 1), total - pending + 1, total + 1, 5000}):
                fmt('A', (damage, pending, total), bench.accept(damage, pending, total))
    lines = lines[:5] + sorted(set(lines[5:]), key=lambda s: (s[0], [int(v) if v.lstrip('-').isdigit() else -1 for v in s.split()[1:]]))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe damage application fixtures'.format(len(lines) - 5))


if __name__ == '__main__':
    main()
