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
        self.uc.mem_map(HEAP, 0x10000)
        self.uc.mem_map(STACK, 0x10000)
        self.uc.mem_map(ARENA, ARENA_SIZE)
        self.arena_next = ARENA
        # Only platform services and fixture boundaries are intercepted. The original
        # map, vector, key comparison, counters and checkpoint arithmetic all execute.
        for address in [0x8d98b0, 0x8dbda0, 0x8df945, 0x8df9e1, 0x8ed444, 0x5edf30]:
            self.uc.hook_add(UC_HOOK_CODE, self.service, begin=address, end=address)
        self.dice_mode = False
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
            if not self.dice_mode:
                raise RuntimeError('unexpected dynamic_cast outside dice fixture')
            if self.read_word(sp + 4) != TARGET:
                raise RuntimeError('unexpected dynamic_cast input')
            uc.reg_write(UC_X86_REG_EAX, TARGET)
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

    def modify(self, v):
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
        return self.call(0x1edf30, (int(base) & 0xffffffff, TARGET, int(ranged), int(moved) & 0xffffffff, int(luck)), this=CREATURE)

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT / 'target/testbench-report.json')
    parser.add_argument('--only', choices=['global', 'combat', 'dice', 'ratio', 'final', 'modify'])
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
        if case.startswith(('combat ', 'dice ', 'modify ')):
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
