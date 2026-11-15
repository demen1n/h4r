#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the melee attack order and retaliation flag.

Executes, from the original image:
- number of attacks 1f1d10 (melee and ranged);
- set_retaliated 1f6b20 with the real time stamp getter 177db0;
- retaliation flag clearing 1f6b60;
- the retaliation block of the round reset 1ecfb0 (1ed09b..1ed0c3);
- t_battlefield::begin_attack 16ede0 whole, including the real handler binding
  (17f5b0/17f610/154df0, 17f7f0/17f850) and the real "can act" check 155ea0.
  The queued continuation handlers are then invoked through their own vtables,
  so the bound arguments of end_melee_attack / begin_melee_retaliation are observed.

Fixture boundaries (stubs that only record or return supplied values):
begin_melee_attack 16e520 (the animated strike), the continuation queue push
175a70, get_adjusted_speed 1f2cc0 (the priority, verified in initiative.py),
dynamic_cast, the resolve_attacks handler, end_melee_attack 16ea60,
begin_melee_retaliation 16f130, and virtual accessors: abilities, effects,
can_retaliate, alive, slot +0x38, no-op slot +0x14.
See decomp/melee_attack.md.
"""
import argparse
import importlib.util
import itertools
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

BASE = reference.BASE
HEAP = 0x7000000
VB = 0xe00                 # fixture offset of the virtual base inside a creature
SUB = 0x9b4                # t_attackable_object subobject of t_combat_creature
FIRST_STRIKE, FIRST_STRIKE_IMMUNITY = 0x17, 0x18
STRIKES_TWICE, EXTRA_MELEE, SHOOTS_TWICE, EXTRA_RANGED = 0x46, 0x12, 0x3b, 0x13
UNLIMITED_RETALIATION = 0x4e


class Original(reference.Original):
    """The shared harness, with dynamic_cast answering for this fixture only."""
    casts = {}

    def service(self, uc, address, size, data):
        if address == 0x8ed444:
            sp = uc.reg_read(UC_X86_REG_ESP)
            source = self.read_word(sp + 4)
            if source not in self.casts:
                raise RuntimeError('unexpected dynamic_cast input {:x}'.format(source))
            uc.reg_write(UC_X86_REG_EAX, self.casts[source])
            uc.reg_write(UC_X86_REG_EIP, self.read_word(sp))
            uc.reg_write(UC_X86_REG_ESP, sp + 4)
            return
        super().service(uc, address, size, data)


class Bench:
    def __init__(self, exe):
        self.o = Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x40000)
        self.field = HEAP + 0x30000
        self.a, self.d = HEAP + 0x10000, HEAP + 0x20000
        self.code = HEAP + 0x8000           # fixture code addresses (hooked or tiny stubs)
        self.calls, self.queue, self.handled = [], [], []
        self.stamp = 0
        # Hooked fixture boundaries: address -> (handler, bytes of arguments popped).
        self.hooks = {
            BASE + 0x16e520: (self.begin_melee_attack, 16),
            BASE + 0x175a70: (self.push_continuation, 8),
            BASE + 0x1f2cc0: (self.adjusted_speed, 0),
            BASE + 0x16ea60: (self.end_melee_attack, 8),
            BASE + 0x16f130: (self.begin_melee_retaliation, 4),
            self.code + 0x00: (self.resolve_attacks, 0),
            self.code + 0x10: (self.can_retaliate, 12),
            self.code + 0x20: (lambda uc, sp: None, 0),   # target/attacker slot +0x14
            self.code + 0x30: (self.this_object, 0),      # slot +0x24: the object itself
        }
        for address in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=address, end=address)
        # A handler object for resolve_attacks: [vptr][refs][vptr of the callable part].
        self.resolve_handler = HEAP + 0x9000
        self.o.word(self.resolve_handler + 4, 1000)
        self.o.word(self.resolve_handler + 8, HEAP + 0x9100)
        self.o.word(HEAP + 0x9100 + 4, self.code)

    # --- hooks -----------------------------------------------------------------
    def hook(self, uc, address, size, _):
        handler, pop = self.hooks[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        handler(uc, sp)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def args(self, sp, n):
        return [self.o.read_word(sp + 4 * (i + 1)) for i in range(n)]

    def name(self, pointer):
        return {self.a: 'A', self.d: 'D', self.a + SUB: 'a', self.d + SUB: 'd', 0: '0'}.get(pointer, '?')

    def begin_melee_attack(self, uc, sp):
        if uc.reg_read(UC_X86_REG_ECX) != self.field:
            raise RuntimeError('begin_melee_attack on another object')
        who, target, sequential, retaliation = self.args(sp, 4)
        self.calls.append('{}>{}:{}{}'.format(self.name(who), self.name(target), sequential & 0xff, retaliation & 0xff))

    def push_continuation(self, uc, sp):
        handler, front = self.args(sp, 2)
        self.queue.append((handler, front & 0xff))

    def adjusted_speed(self, uc, sp):
        if uc.reg_read(UC_X86_REG_ECX) != self.o.read_word(self.field + 0x28):
            raise RuntimeError('time stamp read from a creature that is not acting')
        uc.reg_write(UC_X86_REG_EAX, self.stamp & 0xffffffff)

    def resolve_attacks(self, uc, sp):
        self.handled.append('resolve')

    def end_melee_attack(self, uc, sp):
        n, flag = self.args(sp, 2)
        n = n - 2**32 if n >= 2**31 else n
        self.handled.append('end({}:{})'.format(n, flag & 0xff))

    def begin_melee_retaliation(self, uc, sp):
        self.handled.append('retaliation({})'.format(self.args(sp, 1)[0] & 0xff))

    def can_retaliate(self, uc, sp):
        attacker, ranged, n = self.args(sp, 3)
        if uc.reg_read(UC_X86_REG_ECX) != self.d + SUB or attacker != self.a or ranged & 0xff or n != 1:
            raise RuntimeError('unexpected can_retaliate call')
        uc.reg_write(UC_X86_REG_EAX, self.can_ret)

    def this_object(self, uc, sp):
        uc.reg_write(UC_X86_REG_EAX, uc.reg_read(UC_X86_REG_ECX))

    # --- fixtures --------------------------------------------------------------
    def creature(self, obj, slot, abilities, effects, shots=0, alive=1, slot38=0):
        """A t_combat_creature with only the fields these functions read."""
        o = self.o
        self.uc.mem_write(obj, bytes(0x1000))
        table, vbtable, subtable, subvb = obj + 0xf00, obj + 0xf80, obj + 0xfa0, obj + 0xff0
        o.word(obj + 0x98, table)
        o.word(table + 0x68, o.lookup(slot, abilities))
        o.word(table + 0x4c, o.accessor(slot, shots, pop=0))
        # Effects live behind the virtual base: [obj+0x9c] -> vbtable, vbtable[2] = offset.
        o.word(obj + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        effects_vt = obj + 0xe80
        o.word(obj + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(slot + 1, effects))
        # The attackable subobject reaches the same effects through its own vbtable.
        o.word(obj + SUB, subtable)
        o.word(obj + SUB + 4, subvb)
        o.word(subvb + 4, (obj + 0x9c + VB) - (obj + SUB + 4))
        o.word(subtable + 0x10, self.code + 0x10)
        o.word(subtable + 0x14, self.code + 0x20)
        o.word(subtable + 0x24, self.code + 0x30)
        o.word(subtable + 0x30, o.lookup(slot, abilities))
        o.word(subtable + 0x34, o.accessor(slot + 2, alive, pop=0))
        o.word(subtable + 0x38, o.accessor(slot + 3, slot38, pop=0))
        o.word(obj + 0xa18, self.field)

    def ret(self, register):
        value = self.uc.reg_read(register)
        return value - 2**32 if value >= 2**31 else value

    def signed(self, address):
        value = self.o.read_word(address)
        return value - 2**32 if value >= 2**31 else value

    # --- scenarios -------------------------------------------------------------
    def attacks(self, ranged, effects, abilities, shots):
        self.creature(self.a, 40, abilities, effects, shots=shots)
        return [int(self.o.call(0x1f1d10, (ranged,), this=self.a))]

    def set_retaliated(self, old, old_stamp, value, stamp):
        self.creature(self.d, 50, [], [])
        self.uc.mem_write(self.d + 0xa9, bytes((old,)))
        self.o.word(self.d + 0xb70, old_stamp)
        self.o.word(self.field + 0x28, self.a)
        self.stamp = stamp
        self.o.call(0x1f6b20, (value,), this=self.d)
        return [self.uc.mem_read(self.d + 0xa9, 1)[0], self.signed(self.d + 0xb70)]

    def clear(self, flag, saved, stamp):
        self.creature(self.d, 50, [], [])
        self.uc.mem_write(self.d + 0xa9, bytes((flag,)))
        self.o.word(self.d + 0xb70, saved)
        self.o.call(0x1f6b60, (stamp & 0xffffffff,), this=self.d)
        return [self.uc.mem_read(self.d + 0xa9, 1)[0], self.signed(self.d + 0xb70)]

    def round_reset(self, flag, saved):
        self.creature(self.d, 50, [], [])
        self.uc.mem_write(self.d + 0xa9, bytes((flag,)))
        self.o.word(self.d + 0xb70, saved)
        uc = self.uc
        uc.reg_write(UC_X86_REG_EBX, self.d)
        uc.reg_write(UC_X86_REG_EDX, 0)
        uc.reg_write(UC_X86_REG_ESP, reference.STACK + 0x8000)
        uc.emu_start(BASE + 0x1ed09b, BASE + 0x1ed0c3, count=100)
        if uc.reg_read(UC_X86_REG_EIP) != BASE + 0x1ed0c3:
            raise RuntimeError('round reset block did not finish')
        return [uc.mem_read(self.d + 0xa9, 1)[0], self.signed(self.d + 0xb70)]

    def begin_attack(self, can_ret, a_abilities, d_abilities, d_state, a38, d38, moved, return_after):
        self.creature(self.a, 60, a_abilities, [], slot38=a38)
        self.creature(self.d, 70, d_abilities, [0x0b] if d_state == 'stunned' else [],
                      alive=0 if d_state == 'dead' else 1, slot38=d38)
        f = self.field
        self.uc.mem_write(f, bytes(0x600))
        self.o.word(f + 0x28, self.a)
        self.o.word(f + 0x504, self.d + SUB)
        self.o.word(f + 0x478, self.resolve_handler)
        self.o.word(f + 0x314, moved)
        self.uc.mem_write(f + 0x2f0, b'\x07')   # shows whether begin_attack writes it
        Original.casts = {self.d + SUB: self.d}
        self.can_ret = can_ret
        self.stamp = 1234
        self.calls, self.queue, self.handled = [], [], []
        self.o.call(0x16ede0, (self.a, return_after), this=f)
        for handler, front in self.queue:
            if front:
                raise RuntimeError('continuation pushed to the front')
            callable_part = handler + 8
            vtable = self.o.read_word(callable_part)
            self.o.call(self.o.read_word(vtable + 4) - BASE, this=callable_part)
        flag = self.uc.mem_read(self.d + 0xa9, 1)[0]
        stamp = self.signed(self.d + 0xb70)
        return [','.join(self.calls) or '-', ','.join(self.handled) or '-',
                self.uc.mem_read(f + 0x2f0, 1)[0], flag, stamp if flag else '-']


def bits(mask, values):
    return [v for i, v in enumerate(values) if (mask >> i) & 1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bench = Bench(args.exe)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# N ranged effects(0x7a,0x82 mask) abilities(0x46,0x12,0x3b,0x13 mask) shots -> attacks',
             '# S old-flag old-stamp value stamp -> flag stamp',
             '# C flag stamp now -> flag stamp',
             '# R flag stamp -> flag stamp',
             '# B can-retaliate attacker(first_strike,immunity,strikes_twice mask) '
             'defender(first_strike,immunity,unlimited mask) defender(ok|stunned|dead) '
             'attacker+0x38 defender+0x38 moved return -> strikes handlers bf+0x2f0 flag stamp',
             '# strikes: who>target:sequential retaliation (A/D creatures, a/d attackable objects); '
             'handlers run in queue order; stamp is - when the flag is clear']
    header = len(lines)

    def emit(kind, inputs, outputs):
        lines.append(' '.join([kind] + [str(v) for v in list(inputs) + list(outputs)]))

    for ranged, effects, abilities, shots in itertools.product([0, 1], range(4), range(16), [0, 1, 2, 3, 5]):
        emit('N', (ranged, effects, abilities, shots),
             bench.attacks(ranged, bits(effects, [0x7a, 0x82]),
                           bits(abilities, [STRIKES_TWICE, EXTRA_MELEE, SHOOTS_TWICE, EXTRA_RANGED]), shots))
    stamps = [-3_000_000, -2_999_000, -1000, 0, 1, 1006, 2006, 3_000_000]
    for old, old_stamp, value, stamp in itertools.product([0, 1], [0, 77], [0, 1], [-2006, 0, 6, 1006, 2006]):
        emit('S', (old, old_stamp, value, stamp), bench.set_retaliated(old, old_stamp, value, stamp))
    for flag, saved, now in itertools.product([0, 1], stamps, [-2006, -6, 0, 6, 1006, 2006]):
        emit('C', (flag, saved, now), bench.clear(flag, saved, now))
    for flag, saved in itertools.product([0, 1], stamps):
        emit('R', (flag, saved), bench.round_reset(flag, saved))
    for case in itertools.product([0, 1], range(8), range(8), ['ok', 'stunned', 'dead'], [0, 1], [0, 1], [3, 4], [0, 1]):
        can_ret, a_mask, d_mask, d_state, a38, d38, moved, return_after = case
        emit('B', case, bench.begin_attack(
            can_ret, bits(a_mask, [FIRST_STRIKE, FIRST_STRIKE_IMMUNITY, STRIKES_TWICE]),
            bits(d_mask, [FIRST_STRIKE, FIRST_STRIKE_IMMUNITY, UNLIMITED_RETALIATION]),
            d_state, a38, d38, moved, return_after))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe melee attack fixtures'.format(len(lines) - header))


if __name__ == '__main__':
    main()
