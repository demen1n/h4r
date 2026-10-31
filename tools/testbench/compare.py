#!/usr/bin/env python3
"""Compare original rus-3.0 x86 functions with h4-rules. No game startup or imports."""
import argparse
import hashlib
import itertools
import json
import random
from pathlib import Path
import struct
import subprocess

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW

ROOT = Path(__file__).resolve().parents[2]
SHA256 = '175eacb4838c86e5aee03f31e2406004a06dd238933517e2df40669cf17d36ff'
ARENA, ARENA_SIZE = 0x4000000, 0x2000000
RNG, FIELD, CREATURE, TARGET = 0x2004000, 0x2005000, 0x2006000, 0x2007000

BASE, HEAP, STACK, STOP = 0x400000, 0x2000000, 0x3000000, 0x200f000


class Original:
    def __init__(self, path):
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != SHA256:
            raise ValueError(f'unsupported exe SHA256: {digest}; expected rus-3.0 {SHA256}')
        pe = struct.unpack_from('<I', data, 60)[0]
        opt = pe + 24
        image_base = struct.unpack_from('<I', data, opt + 28)[0]
        if image_base != BASE:
            raise ValueError('unexpected image base')
        size = struct.unpack_from('<I', data, opt + 56)[0]
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.mem_map(BASE, (size + 4095) & ~4095)
        headers = struct.unpack_from('<I', data, opt + 60)[0]
        self.uc.mem_write(BASE, data[:headers])
        sections = opt + struct.unpack_from('<H', data, pe + 20)[0]
        for i in range(struct.unpack_from('<H', data, pe + 6)[0]):
            _, va, raw_size, raw = struct.unpack_from('<IIII', data, sections + 40*i + 8)
            self.uc.mem_write(BASE + va, data[raw:raw + raw_size])
        # Minimal single-thread SEH chain used by diagnostic helpers (FS:[0]).
        self.uc.mem_map(0, 0x1000)
        self.word(0, 0xffffffff)
        self.uc.mem_map(HEAP, 0x10000)
        self.uc.mem_map(STACK, 0x10000)
        self.uc.mem_map(ARENA, ARENA_SIZE)
        self.arena_next = ARENA
        # Only platform services and fixture boundaries are intercepted. The original
        # map, vector, key comparison, counters and checkpoint arithmetic all execute.
        for address in [0x8d98b0, 0x8dbda0, 0x8df945, 0x8df9e1, 0x8ed444, 0x5edf30, 0x5b6140, 0x8643a0, 0x40cb30, 0x5b3cf0, 0x5b5a80]:
            self.uc.hook_add(UC_HOOK_CODE, self.service, begin=address, end=address)
        self.dice_mode = False
        self.effect_mode = False
        self.gaze_mode = False
        # Virtual accessors return supplied fixture values. Arithmetic stays in exe code.
        self.uc.mem_write(STOP, b'\xd9\x1d' + struct.pack('<I', HEAP + 0xe000))  # fstp float
        self.uc.mem_write(HEAP, struct.pack('<I', HEAP + 0x1000))
        self.uc.mem_write(HEAP + 0x200, struct.pack('<I', HEAP + 0x1200))
        # Start with reset/default exception masks, then execute CRT precision init.
        # CRT startup -> 4ed10b -> 4f2ee8 selects 53-bit x87 precision.
        self.uc.reg_write(UC_X86_REG_FPCW, 0x037f)
        self.call(0x4f2ee8)
        if self.uc.reg_read(UC_X86_REG_FPCW) != 0x023f:
            raise RuntimeError(f'original CRT control word: {self.uc.reg_read(UC_X86_REG_FPCW):04x}')

    def read_word(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def service(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        pop = 0
        if address == 0x8d98b0:  # operator new(size), cdecl
            size = self.read_word(sp + 4)
            result = self.arena_next
            self.arena_next += (max(size, 1) + 15) & ~15
            if self.arena_next > ARENA + ARENA_SIZE:
                raise RuntimeError('fixture allocator exhausted')
            uc.mem_write(result, bytes(size))
            uc.reg_write(UC_X86_REG_EAX, result)
        elif address in (0x8dbda0, 0x8df945, 0x8df9e1):
            pass  # delete / single-threaded CRT lock / unlock
        elif address == 0x8ed444:
            if not (self.dice_mode or self.effect_mode):
                raise RuntimeError('unexpected dynamic_cast outside fixture')
            if self.read_word(sp + 4) != TARGET:
                raise RuntimeError('unexpected dynamic_cast input')
            uc.reg_write(UC_X86_REG_EAX, TARGET if not self.effect_mode or self.target_is_creature else 0)
        elif address in (0x40cb30, 0x5b3cf0, 0x5b5a80):
            if not self.gaze_mode:
                return  # outside this fixture, execute the native function
            expected = ([0x98, 0] if address == 0x40cb30
                        else [CREATURE + 0x98, 0x98, TARGET + 0x98, 0])
            actual = [self.read_word(sp + 4 * (i + 1)) for i in range(len(expected))]
            if actual != expected or (address == 0x40cb30 and uc.reg_read(UC_X86_REG_ECX) != TARGET + 0x98):
                raise RuntimeError('unexpected unported eligibility-boundary call')
            # Unported boundaries only: no artifact block, refresh allowed, and
            # the caller-supplied remainder represented by the side filter.
            result = self.remaining_eligibility if address == 0x5b3cf0 else address != 0x40cb30
            uc.reg_write(UC_X86_REG_EAX, int(result))
            if address == 0x40cb30:
                pop = 8
        elif address == 0x5b6140:
            if self.gaze_mode:
                return  # execute native eligibility, including the four real filters
            if not self.effect_mode:
                raise RuntimeError('unexpected effect eligibility call')
            if [self.read_word(sp + i) for i in (4, 8, 12)] != [CREATURE, 0x98, TARGET]:
                raise RuntimeError('unexpected effect eligibility arguments')
            uc.reg_write(UC_X86_REG_EAX, int(self.effect_can_affect))
        elif address == 0x8643a0:
            if not self.effect_mode or self.read_word(sp + 4) != 0x98:
                raise RuntimeError('unexpected spell record lookup')
            uc.reg_write(UC_X86_REG_EAX, HEAP + 0xd000)
        elif address == 0x5edf30:
            if not self.dice_mode:
                return  # execute the real modifier chain
            # Observe the base roll before modifiers; do not reproduce its formula.
            uc.reg_write(UC_X86_REG_EAX, self.read_word(sp + 4))
            pop = 20
        uc.reg_write(UC_X86_REG_EIP, self.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def combat_start(self, seed, modulus):
        self.arena_next = ARENA
        self.uc.mem_write(RNG, bytes(0x28))
        self.word(0xaa1c30, 0)  # shared std::map nil; constructor initializes it
        self.word(0xaa1c2c, 0)
        self.word(FIELD + 0x438, modulus + 1)
        self.call(0x2031b0, (seed, FIELD), this=RNG)

    def combat_roll(self, a, b, purpose):
        return self.call(0x203280, (a, b, purpose), this=RNG)

    def word(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xffffffff))

    def accessor(self, slot, value, pop=4, floating=False):
        address = HEAP + 0x2000 + slot * 32
        if floating:
            self.uc.mem_write(address, b'\xd9\x05' + struct.pack('<I', address + 16) + b'\xc2' + struct.pack('<H', pop))
            self.uc.mem_write(address + 16, struct.pack('<f', value))
        else:
            self.uc.mem_write(address, b'\xb8' + struct.pack('<I', value) + b'\xc2' + struct.pack('<H', pop))
        self.uc.ctl_remove_cache(address, address + 32)
        return address

    def lookup(self, slot, values):
        """Virtual bool accessor backed only by fixture bytes, not game logic."""
        table = HEAP + 0x8000 + slot * 256
        self.uc.mem_write(table, bytes(256))
        for value in values:
            self.uc.mem_write(table + value, b'\x01')
        address = HEAP + 0x3000 + slot * 32
        self.uc.mem_write(address, b'\x8b\x44\x24\x04\x0f\xb6\x80' + struct.pack('<I', table) + b'\xc2\x04\x00')
        self.uc.ctl_remove_cache(address, address + 32)
        return address

    def modify(self, v, setup_only=False):
        (base, offense, reduction, defense, bonus, school, abilities, resistances,
         wards, giant, effect, ranged, moved, luck, strength) = v
        abilities = [] if abilities == '-' else list(map(int, abilities.split(',')))
        resistances = [] if resistances == '-' else list(map(int, resistances.split(',')))
        self.word(CREATURE + 0x98, HEAP + 0x1400)
        self.word(CREATURE + 0x9c, HEAP + 0x1900)
        self.word(HEAP + 0x1900 + 8, 0x164)
        self.word(CREATURE + 0x200, HEAP + 0x1a00)
        self.word(HEAP + 0x1a00 + 8, self.lookup(0, [0x8c] if effect == '1' else []))
        self.word(HEAP + 0x1400 + 0x68, self.lookup(1, abilities))
        self.word(HEAP + 0x1400 + 4, self.accessor(8, int(school), pop=0))
        self.word(HEAP + 0x1400 + 0x28, self.accessor(9, float(reduction), floating=True))
        self.word(HEAP + 0x1400 + 0x44, self.accessor(10, float(offense), floating=True))
        self.word(CREATURE + 0xa18, FIELD)
        self.word(FIELD + 0x60, int(strength))
        self.word(TARGET, HEAP + 0x1500)
        self.word(HEAP + 0x1500 + 0x30, self.lookup(2, resistances))
        self.word(HEAP + 0x1500 + 0x40, self.accessor(11, int(giant), pop=0))
        self.word(TARGET + 4, HEAP + 0x1800)
        self.word(HEAP + 0x1800 + 4, 0x1fc)
        self.word(HEAP + 0x1800 + 8, 0x2fc)
        self.word(TARGET + 0x200, HEAP + 0x1b00)
        effects = [code for i, code in enumerate([0x77, 0x79, 0x76, 0x75, 0x78]) if (int(wards) >> i) & 1]
        self.word(HEAP + 0x1b00 + 8, self.lookup(3, effects))
        self.word(TARGET + 0x300, HEAP + 0x1c00)
        self.word(HEAP + 0x1c00 + 4, self.accessor(12, float(bonus), floating=True))
        self.word(HEAP + 0x1c00 + 8, self.accessor(13, float(defense), floating=True))
        if setup_only:
            return
        return self.call(0x1edf30, (int(base) & 0xffffffff, TARGET, int(ranged), int(moved) & 0xffffffff, int(luck)), this=CREATURE)

    def immunity_setup(self, v):
        flags, caster, target, effects, hostile, same, resistance = v
        flags, hostile, same, resistance = map(int, (flags, hostile, same, resistance))
        def values(value):
            return [] if value in ('-', 'none') else list(map(int, value.split(',')))
        self.effect_mode = True
        self.word(HEAP + 0xd000 + 0x3c, flags)
        self.word(CREATURE + 0x98, HEAP + 0x1400)
        self.word(TARGET + 0x98, HEAP + 0x1d00)
        self.word(HEAP + 0x1400 + 0x68, self.lookup(0, values(caster)))
        self.word(HEAP + 0x1d00 + 0x68, self.lookup(1, values(target)))
        self.word(HEAP + 0x1d00 + 0x7c, self.accessor(16, resistance & 0xffffffff))
        self.word(HEAP + 0x1400 + 0x74, self.accessor(18, 2, pop=0))
        for obj, slot in [(CREATURE, 19), (TARGET, 20)]:
            self.word(obj + 0x9c, HEAP + 0x5000)
            self.word(HEAP + 0x5008, 0x40)
            self.word(obj + 0xdc, HEAP + 0x5100 + slot * 16)
        target_vtable = HEAP + 0x5100 + 20 * 16
        self.word(target_vtable + 8, self.lookup(2, values(effects)))
        self.word(target_vtable, self.accessor(21, 0))  # not an allied side
        # +4(0) returns side id, +4(caster side=2) returns friendliness.
        address = HEAP + 0x2800
        code = b'\x83\x7c\x24\x04\x00\x75\x08\xb8'
        code += struct.pack('<I', 2 if same else 3) + b'\xc2\x04\x00\xb8'
        code += struct.pack('<I', 0 if hostile else 1) + b'\xc2\x04\x00'
        self.uc.mem_write(address, code)
        self.uc.ctl_remove_cache(address, address + 32)
        self.word(target_vtable + 4, address)
        self.word(HEAP + 0x5100 + 19 * 16 + 4, self.accessor(22, 2))
        # Mark localized diagnostic strings initialized; no diagnostic output requested.
        for offset in [0x69d0, 0x69e8, 0x6a00, 0x69a0, 0x69b8, 0x6970,
                       0x6988, 0x6910, 0x6928, 0x6898, 0x68f8, 0x68e0,
                       0x68c8, 0x6940, 0x6958]:
            self.uc.mem_write(0xaa0000 + offset, b'\x01')
        source = 0 if caster == 'none' else CREATURE + 0x98
        dest = TARGET + 0x98
        return source, dest

    def stone_gaze(self, v):
        if len(v) != 17 or v[1] == 'none':
            raise ValueError('stone-gaze fixture requires a caster and 17 fields')
        self.immunity_setup(v[:7])
        generic, remaining, creature, power, unit_hp, number, damage, hp, dealt, block = map(int, v[7:])
        self.remaining_eligibility = bool(remaining)
        self.target_is_creature = bool(creature)
        # Slot +0x40's initial eligibility check is an explicit allowed boundary.
        self.word(HEAP + 0x1d00 + 0x40, self.accessor(23, 1, pop=0))
        target_abilities = [] if v[2] == '-' else list(map(int, v[2].split(',')))
        self.word(TARGET, HEAP + 0x1500)
        self.word(HEAP + 0x1500 + 0x30, self.lookup(3, target_abilities + ([7] if block else [])))
        self.word(HEAP + 0x1500 + 0x2c, self.accessor(17, hp, pop=8))
        self.word(TARGET + 0xa4, HEAP + 0x1e00)
        self.word(HEAP + 0x1e00, HEAP + 0x1f00)
        self.word(HEAP + 0x1f00 + 0x90, self.accessor(14, power, pop=0))
        self.word(HEAP + 0x1d00 + 0x30, self.accessor(15, unit_hp, pop=0))
        # Query -1 separately from 0x98, without colliding with the side accessor.
        address = HEAP + 0x2a00
        code = b'\x83\x7c\x24\x04\xff\x75\x08\xb8' + struct.pack('<I', generic & 0xffffffff)
        code += b'\xc2\x04\x00\xb8' + struct.pack('<I', int(v[6]) & 0xffffffff) + b'\xc2\x04\x00'
        self.uc.mem_write(address, code)
        self.uc.ctl_remove_cache(address, address + 32)
        self.word(HEAP + 0x1d00 + 0x7c, address)
        self.gaze_mode = True
        try:
            eligible = int(self.call(0x1b6140, (CREATURE, 0x98, TARGET), callee_pop=False)) & 0xff
            effectiveness = int(self.call(0x1f1de0, (TARGET, 0x98), this=CREATURE))
            damage = int(self.final_call(damage, number, dealt))
            def signed(value):
                return value if value < 0x80000000 else value - 0x100000000
            return f'{eligible} {signed(effectiveness)} {signed(damage)}'
        finally:
            self.gaze_mode = False
            self.effect_mode = False

    def final_setup(self, v):
        # number, target HP, already dealt, creature HP, type, eligibility,
        # resolved effectiveness, target virtual power, excluded abilities, block
        number, hp, dealt, unit_hp, creature, eligible, effectiveness, power, excluded, block = map(int, v)
        self.target_is_creature = bool(creature)
        self.effect_can_affect = bool(eligible)
        self.word(CREATURE + 0xb0, number)
        self.word(TARGET + 0x98, HEAP + 0x1d00)
        self.word(TARGET + 0xa4, HEAP + 0x1e00)
        self.word(HEAP + 0x1e00, HEAP + 0x1f00)
        self.word(HEAP + 0x1f00 + 0x90, self.accessor(14, power, pop=0))
        self.word(HEAP + 0x1d00 + 0x30, self.accessor(15, unit_hp, pop=0))
        self.word(HEAP + 0x1d00 + 0x7c, self.accessor(16, 100 - effectiveness))
        self.word(HEAP + 0x1500 + 0x2c, self.accessor(17, hp, pop=8))
        # Explicit effect-98 flags: resisted; native effectiveness helper executes.
        self.word(HEAP + 0xd000 + 0x3c, 0x800000)
        return number, dealt, excluded, block

    def final_call(self, damage, number, dealt):
        return self.call(0x1ee540, (CREATURE, number, TARGET, dealt, int(damage)), callee_pop=False)

    def complete(self, kind, v):
        # Final fixtures: modifier fields (15), finalization fields (10).
        # Attack fixtures additionally prefix seed, modulus, low, high, hit count.
        if kind == 'attack':
            seed, modulus, low, high, hits = map(int, v[:5])
            v = v[5:]
            self.combat_start(seed, modulus)
        else:
            hits = 1
        modifiers, final = v[:15], v[15:]
        self.modify(modifiers, setup_only=True)
        number, dealt, excluded, block = self.final_setup(final)
        abilities = [] if modifiers[6] == '-' else list(map(int, modifiers[6].split(',')))
        # At this fixture boundary eligibility/resistance are already resolved;
        # bypass-resistance abilities are deliberately not supplied.
        if any(a in abilities for a in (0x52, 0x1f)):
            raise RuntimeError('unsupported resistance-bypass fixture')
        resistances = [] if modifiers[7] == '-' else list(map(int, modifiers[7].split(',')))
        resistances += [a for i, a in enumerate([0x4d, 0x2a, 0x11]) if excluded & (1 << i)]
        if block:
            resistances.append(7)
        self.word(HEAP + 0x1500 + 0x30, self.lookup(2, resistances))
        self.effect_mode = True
        try:
            result = []
            if kind == 'attack':
                self.word(HEAP + 0x1400 + 0x24, self.accessor(6, low, pop=0))
                self.word(HEAP + 0x1400 + 0x20, self.accessor(7, high, pop=0))
                self.word(CREATURE + 0xa30, 3)
                self.word(TARGET + 0xa30, 7)
                self.word(FIELD + 0x380, RNG)
                for _ in range(hits):
                    modified = self.call(0x1ee240, (TARGET, int(modifiers[11]), int(modifiers[12]), int(modifiers[13])), this=CREATURE)
                    # Same creature/target/RNG state; modifier interception is OFF.
                    result.append(self.final_call(modified, number, dealt))
                result.append(self.combat_roll(7 if self.target_is_creature else 0, 3, 0))
            else:
                result.append(self.final_call(modifiers[0], number, dealt))
            return ' '.join(result)
        finally:
            self.effect_mode = False

    def call(self, rva, args=(), this=HEAP, floating=False, callee_pop=True):
        sp = STACK + 0x8000
        self.uc.mem_write(sp, struct.pack('<' + 'I' * (len(args)+1), STOP, *args))
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_ECX, this)
        self.uc.emu_start(BASE + rva, STOP, timeout=30_000_000)
        if self.uc.reg_read(UC_X86_REG_EIP) != STOP:
            raise RuntimeError(f'execution timed out at RVA {rva:x}')
        expected = sp + 4 + (4 * len(args) if callee_pop else 0)
        if self.uc.reg_read(UC_X86_REG_ESP) != expected:
            raise RuntimeError('unexpected calling convention / stack imbalance')
        if floating:
            self.uc.emu_start(STOP, STOP + 6, count=1)
            return bytes(self.uc.mem_read(HEAP + 0xe000, 4)).hex()  # little endian below
        return str(self.uc.reg_read(UC_X86_REG_EAX))

    def evaluate(self, line):
        kind, *v = line.split()
        if kind == 'global':
            seed, count = map(int, v)
            self.word(HEAP + 0x400, seed)
            return ' '.join(self.call(0x36a70, this=HEAP + 0x400) for _ in range(count))
        if kind == 'combat':
            seed, modulus, *rolls = map(int, v)
            self.combat_start(seed, modulus)
            result = []
            for a, b, purpose in zip(rolls[::3], rolls[1::3], rolls[2::3]):
                if purpose == 44:  # protocol marker: reset the actual generator
                    self.call(0x203430, (a,), this=RNG)
                    continue
                result.append(self.combat_roll(a, b, purpose))
            return ' '.join(result)
        if kind == 'dice':
            seed, modulus, attacker, target, number, low, high, hits = map(int, v)
            self.combat_start(seed, modulus)
            self.word(CREATURE + 0x98, HEAP + 0x1400)
            self.word(HEAP + 0x1400 + 0x24, self.accessor(6, low, pop=0))
            self.word(HEAP + 0x1400 + 0x20, self.accessor(7, high, pop=0))
            self.word(CREATURE + 0xb0, number)
            self.word(CREATURE + 0xa30, attacker)
            self.word(CREATURE + 0xa18, FIELD)
            self.word(FIELD + 0x380, RNG)
            self.word(TARGET + 0xa30, target)
            self.dice_mode = True
            try:
                result = [self.call(0x1ee240, (TARGET, 0, 0, 0), this=CREATURE) for _ in range(hits)]
                # Verify how many RNG rolls the damage routine consumed, even low==high.
                result.append(self.combat_roll(target, attacker, 0))
            finally:
                self.dice_mode = False
            return ' '.join(result)
        if kind == 'stone_gaze':
            return self.stone_gaze(v)
        if kind == 'immunity':
            source, dest = self.immunity_setup(v)
            calls = [(0x1b3540, (0x98, dest, 0)),
                     (0x1b3a10, (0x98, source, dest, 0)),
                     (0x1b4770, (source, 0x98, dest, 0)),
                     (0x1b5750, (source, 0x98, dest, 0))]
            try:
                result = []
                for rva, args in calls:
                    try:
                        result.append(str(int(self.call(rva, args, callee_pop=False)) & 0xff))
                    except Exception as error:
                        raise RuntimeError(f'immunity RVA {rva:x}: {v}') from error
                return ' '.join(result)
            finally:
                self.effect_mode = False
        if kind == 'effectiveness':
            eligible, resisted, between, same, bypass, generic, specific_r, generic_r = map(int, v)
            self.effect_mode = True
            self.effect_can_affect = bool(eligible)
            self.word(CREATURE + 0x98, HEAP + 0x1400)
            abilities = ([0x52] if bypass else []) + ([0x1f] if generic else [])
            self.word(HEAP + 0x1400 + 0x68, self.lookup(0, abilities))
            self.word(TARGET + 0x98, HEAP + 0x1d00)
            # Native accessor dispatches -1 separately from the actual effect id.
            address = HEAP + 0x2800
            code = b'\x83\x7c\x24\x04\xff\x75\x08\xb8' + struct.pack('<I', generic_r & 0xffffffff)
            code += b'\xc2\x04\x00\xb8' + struct.pack('<I', specific_r & 0xffffffff) + b'\xc2\x04\x00'
            self.uc.mem_write(address, code)
            self.uc.ctl_remove_cache(address, address + 32)
            self.word(HEAP + 0x1d00 + 0x7c, address)
            for obj, side, slot in [(CREATURE, 0, 20), (TARGET, 0 if same else 1, 21)]:
                self.word(obj + 0x9c, HEAP + 0x5000)
                self.word(HEAP + 0x5008, 0x40)
                self.word(obj + 0xdc, HEAP + 0x5100 + slot * 16)
                self.word(HEAP + 0x5100 + slot * 16 + 4, self.accessor(slot, side))
            self.word(HEAP + 0xd000 + 0x3c, resisted * 0x800000 | between * 0x1000000)
            try:
                raw = int(self.call(0x1f1de0, (TARGET, 0x98), this=CREATURE))
                return str(raw if raw < 0x80000000 else raw - 0x100000000)
            finally:
                self.effect_mode = False
        if kind in ('ability42', 'attack'):
            return self.complete(kind, v)
        if kind == 'modify':
            return self.modify(v)
        if kind == 'ratio':
            offense, reduction, defense, bonus = map(float, v)
            self.word(HEAP + 0x1000 + 0x28, self.accessor(0, reduction, floating=True))
            self.word(HEAP + 0x1200 + 4, self.accessor(1, bonus, floating=True))
            self.word(HEAP + 0x1200 + 8, self.accessor(2, defense, floating=True))
            bits = struct.unpack('<I', struct.pack('<f', offense))[0]
            raw = self.call(0x250a60, (HEAP + 0x200, 0, bits), floating=True)
            return f'{int.from_bytes(bytes.fromhex(raw), "little"):08x}'
        damage, hp, dealt, block = map(int, v)
        self.word(HEAP + 0x98, HEAP + 0x1000)
        self.word(HEAP + 0x1000 + 0x68, self.accessor(3, 0))  # no ability 0x42
        self.word(HEAP + 0x1200 + 0x2c, self.accessor(4, hp, pop=8))
        self.word(HEAP + 0x1200 + 0x30, self.accessor(5, block))
        return self.call(0x1ee540, (HEAP, 1, HEAP + 0x200, dealt, damage), callee_pop=False)


def corpus():
    for flags, caster, target, effects, remaining in itertools.product(
            [0x800000, 0x1000000, 0x2000, 0x3ffffff],
            ['66', '66,31', '66,82', '66,31,82'],
            ['-', '5', '63', '38', '38,63', '42', '77', '17'],
            ['-', '3', '137'], [0, 1]):
        # Effect-specific 25%, generic 75% deliberately distinguish ignore_wards.
        yield f'stone_gaze {flags} {caster} {target} {effects} 1 0 25 75 {remaining} 1 100 37 11 15 10000 0 0'
    gaze_random = random.Random(98)
    for _ in range(128):
        flags = gaze_random.choice([0, 0x800000, 0x1000000, 0x1802000, 0x3ffffff])
        caster = gaze_random.choice(['-', '66', '66,31', '66,82', '66,31,82', '66,45'])
        target = gaze_random.choice(['-', '5', '7', '63', '38', '38,63', '42', '77', '17'])
        effects = gaze_random.choice(['-', '3', '137', '40', '20'])
        values = [gaze_random.randrange(2), gaze_random.randrange(2),
                  gaze_random.choice([0, 25, 99, 100, 101]), gaze_random.choice([0, 50, 100]),
                  gaze_random.randrange(2), gaze_random.randrange(2),
                  gaze_random.choice([1, 50, 100]), gaze_random.choice([1, 37, 400]),
                  gaze_random.choice([9, 10, 11, 1000]), 15,
                  gaze_random.choice([0, 30, 100000]), gaze_random.choice([0, 5, 35]),
                  gaze_random.randrange(2)]
        yield f'stone_gaze {flags} {caster} {target} {effects} ' + ' '.join(map(str, values))
    def ids(items):
        return ','.join(map(str, items)) or '-'
    immunity_flags = [0, 0x8000, 0x400, 0x4000, 0x10, 0x20, 2, 0x200000,
                      0x800000, 0x2000, 0x1000000, 0x3ffffff]
    # All subsets of the relevant target abilities at the two flag extremes.
    for bits, flags, caster in itertools.product(range(512), [0, 0x3ffffff], ['-', '45']):
        target = ids(a for i, a in enumerate([0x3f, 0x26, 0x15, 0x0d, 3, 0x2a, 0x4d, 0x11, 5]) if bits & (1 << i))
        yield f'immunity {flags} {caster} {target} - 1 0 100'
    for flags, target, effects, caster in itertools.product(
            immunity_flags, ['-', '63', '38', '21', '13', '3', '42', '77', '17', '5'],
            ['-', '137', '3', '40', '20', '137,3,40,20'], ['none', '-', '45', '82', '45,82']):
        yield f'immunity {flags} {caster} {target} {effects} 1 0 100'
    # Resistance gate: missing caster, non-hostile versus same-side, 0x52 vs 0x26.
    for flags, caster, target, hostile, same, resistance in itertools.product(
            [0, 0x800000, 0x2000, 0x1000000, 0x1802000],
            ['none', '-', '82'], ['-', '38', '63', '38,63'], [0, 1], [0, 1],
            [-1, 0, 99, 100, 101, 2147483647]):
        yield f'immunity {flags} {caster} {target} - {hostile} {same} {resistance}'

    for flags in itertools.product([0, 1], repeat=6):
        for specific, generic in [(0, 0), (25, 75), (100, 0), (-10, 120), (2147483647, -2147483648)]:
            yield 'effectiveness ' + ' '.join(map(str, (*flags, specific, generic)))
    for seed in [0, 1, 7, 12345, 32767, 0x7fffffff, 0x80000000, 0xffffffff]:
        yield f'global {seed} 256'
    for values in itertools.product([0, 0.1, 1, 11, 1000], [0.5, 1, 1.2, 2], [0, 0.1, 11, 1000], [0.5, 1, 1.5, 2]):
        yield 'ratio ' + ' '.join(map(str, values))
    for values in itertools.product([0, 1, 5, 15, 50, 10000], [0, 1, 30, 1000], [0, 10, 1000], [0, 1]):
        yield 'final ' + ' '.join(map(str, values))

    for seed, modulus in itertools.product([0, 1, 32767, 0xffffffff], [1, 2, 10, 20]):
        rolls = [(3, 7, 0), (3, 7, 0), (3 + modulus, 7 + modulus, 0),
                 (7, 3, 0), (3, 7, 1), (3, 7, 29), (250, 15, 30),
                 (50, 215, 30), (0xffffffff, 0x80000000, 43), (3, 7, 2), (3, 7, 3), (3, 7, 0),
                 (seed, 0, 44), (3, 7, 0), (250, 15, 30),
                 (seed ^ 0x1234, 0, 44), (3, 7, 0), (3, 7, 0)]
        yield f'combat {seed} {modulus} ' + ' '.join(str(v) for roll in rolls for v in roll)
    for seed, number, bounds in itertools.product([1, 32767], [0, 1, 5, 9, 10, 11, 19, 1000], [(1, 1), (1, 3), (7, 19)]):
        low, high = bounds
        yield f'dice {seed} 20 3 7 {number} {low} {high} 3'

    yield from modifier_corpus()
    yield from attack_corpus()


def modifier_corpus():
    # Protocol: base offense reduction defense bonus school attacker-abilities
    # defender-abilities ward-mask giant giant-effect ranged moved luck strength.
    def case(**changes):
        fields = dict(base=101, offense=11, reduction=1, defense=11, bonus=1,
                      school=5, abilities='-', resistances='-', wards=0, giant=0,
                      effect=0, ranged=0, moved=0, luck=0, strength=3)
        fields.update(changes)
        return 'modify ' + ' '.join(map(str, fields.values()))
    for base, moved, ranged in itertools.product([0, 1, 15, 101, 10001], [0, 499, 500, 501, 524, 525, 700, 1400], [0, 1]):
        yield case(base=base, abilities='11', moved=moved, ranged=ranged)
    for giant, effect, abilities, ranged in itertools.product([0, 1], [0, 1], ['-', '28', '43', '53', '28,43,53'], [0, 1]):
        yield case(giant=giant, effect=effect, abilities=abilities, ranged=ranged)
    for base, luck, strength in itertools.product([0, 1, 3, 15, 101, 99999], [0, 1, 2], range(4)):
        yield case(base=base, luck=luck, strength=strength)
    for school, wards, resistances in itertools.product(range(6), [0, 1, 2, 4, 8, 16, 31, 63], ['-', '34', '16', '10', '34,16,10']):
        yield case(school=school, wards=wards, resistances=resistances)
    for abilities, resistances, ranged in itertools.product(['-', '20', '9', '84', '83', '12', '20,12', '9,83,84,12'], ['-', '21', '13', '21,13'], [0, 1]):
        yield case(abilities=abilities, resistances=resistances, ranged=ranged)
    for base, stats in itertools.product([0, 1, 3, 15, 101, 10001], [(0.1, 1, 0.1, 1.5), (11, 1.2, 11, 1.5), (11, 2, 11, 1.5), (0, 1, 0, 1), (1000, 1, 0.1, 1), (0.1, 1, 1000, 1)]):
        offense, reduction, defense, bonus = stats
        yield case(base=base, offense=offense, reduction=reduction, defense=defense, bonus=bonus)
        yield case(base=base, offense=offense, reduction=reduction, defense=defense, bonus=bonus,
                   abilities='11,28,20,12', resistances='21,13', school=3, wards=8,
                   giant=1, effect=1, moved=700, luck=1, strength=1)

    # Exact-integer boundaries expose rounding of the coefficient before scaling.
    for offense, defense in itertools.product(range(1, 26), repeat=2):
        yield case(base=defense, offense=offense, defense=defense)
    rng = random.Random(0x483452)
    for _ in range(128):
        aa = [v for v in [11, 28, 43, 53, 20, 9, 84, 83, 12] if rng.randrange(2)]
        da = [v for v in [34, 16, 10, 21, 13] if rng.randrange(2)]
        yield case(base=rng.choice([0, 1, 3, 15, 101, 99999]),
                   offense=rng.choice([0.1, 5, 11, 15, 1000]),
                   defense=rng.choice([0.1, 6, 11, 19, 1000]),
                   reduction=rng.choice([0.5, 1, 1.2, 2]), bonus=rng.choice([0.5, 1, 1.5, 2]),
                   abilities=','.join(map(str, aa)) or '-', resistances=','.join(map(str, da)) or '-',
                   school=rng.randrange(6), wards=rng.randrange(64), giant=rng.randrange(2),
                   effect=rng.randrange(2), ranged=rng.randrange(2), moved=rng.choice([500, 501, 525, 700, 1400]),
                   luck=rng.randrange(3), strength=rng.randrange(4))


def attack_corpus():
    def modifier(ability='66', **changes):
        f = dict(base=15, offense=11, reduction=1, defense=11, bonus=1, school=5,
                 abilities=ability, resistances='-', wards=0, giant=0, effect=0,
                 ranged=0, moved=0, luck=0, strength=3)
        f.update(changes)
        return ' '.join(map(str, f.values()))
    def final(**changes):
        f = dict(number=10, hp=100000, dealt=0, unit_hp=37, creature=1,
                 eligible=1, effectiveness=100, power=50, excluded=0, block=0)
        f.update(changes)
        return ' '.join(map(str, f.values()))
    for number, power, effectiveness, unit_hp, block in itertools.product(
            [0, 1, 4, 5, 9, 10, 11, 49, 50, 499, 500], [1, 50, 100], [0, 50, 99, 100], [1, 37], [0, 1]):
        yield 'ability42 ' + modifier() + ' ' + final(number=number, power=power, effectiveness=effectiveness, unit_hp=unit_hp, block=block)
    for ability, creature, eligible, excluded, hp, dealt in itertools.product(
            ['-', '66'], [0, 1], [0, 1], range(8), [0, 30], [0, 10, 100]):
        yield 'ability42 ' + modifier(ability) + ' ' + final(creature=creature, eligible=eligible, excluded=excluded, hp=hp, dealt=dealt)
    for seed, number, ranged, luck, block in itertools.product([1, 32767], [0, 1, 9, 10, 11, 1000], [0, 1], [0, 1, 2], [0, 1]):
        m = modifier('11,28,20,12,66', offense=5, defense=6, school=3, wards=8,
                     resistances='21,13', giant=1, effect=1, moved=700, ranged=ranged, luck=luck, strength=1)
        yield f'attack {seed} 20 1 3 3 ' + m + ' ' + final(number=number, block=block, hp=300, dealt=10)
    for creature, eligible, excluded, ability in itertools.product([0, 1], [0, 1], [0, 1, 2, 4], ['-', '66']):
        yield 'attack 7 20 7 19 3 ' + modifier(ability) + ' ' + final(creature=creature, eligible=eligible, excluded=excluded)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT / 'target/testbench-report.json')
    parser.add_argument('--only', choices=['global', 'combat', 'dice', 'ratio', 'final', 'modify', 'ability42', 'attack', 'effectiveness', 'immunity', 'stone_gaze'])
    args = parser.parse_args()
    original = Original(args.exe)
    cases = [case for case in corpus() if args.only is None or case.split()[0] == args.only]
    subprocess.run(['cargo', 'build', '--locked', '-q', '-p', 'h4-rules', '--example', 'exe_reference'], cwd=ROOT, check=True)
    proc = subprocess.run([str(ROOT / 'target/debug/examples/exe_reference')], input='\n'.join(cases)+'\n', text=True, capture_output=True, check=True)
    results = proc.stdout.splitlines()
    if len(results) != len(cases):
        raise RuntimeError('reference returned wrong number of results')
    failures = []
    vectors = []
    for index, (case, rust) in enumerate(zip(cases, results)):
        if index % 32 == 0:
            print(f"checking {index + 1}/{len(cases)}", flush=True)
        exe = original.evaluate(case)
        if case.startswith(('combat ', 'dice ', 'modify ', 'ability42 ', 'attack ', 'effectiveness ', 'immunity ', 'stone_gaze ')):
            vectors.append(dict(input=case, exe=exe))
        if exe != rust:
            failures.append(dict(input=case, exe=exe, rust=rust))
    report = dict(exe_sha256=SHA256, unicorn=__import__('unicorn').__version__, x87_control=original.uc.reg_read(UC_X86_REG_FPCW), cases=len(cases), failures=failures, vectors=vectors)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2)+'\n')
    print(f'{len(cases)} cases, {len(failures)} differences; {args.report}')
    for failure in failures[:10]:
        print(failure)
    return bool(failures)


if __name__ == '__main__':
    raise SystemExit(main())
