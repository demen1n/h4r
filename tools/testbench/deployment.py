#!/usr/bin/env python3
"""Record native open-field deployment and compaction (Buka 3.0).

Runs initializer 224270, coordinate block 22dbb6..22dc07, native compaction
dispatch 22dfbc..22e0cb and full pull helper
22e3b0. Model loading/virtual sizes and field occupancy are fixture boundaries.
Footprint masks execute original 1fc340; collision inputs use those masks.
Height and move-object bookkeeping are intercepted (no renderer). Army slot
reordering/splitting, siege, bridge and towers are outside this fixture.
"""
import argparse
import importlib.util
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.heap = 0x7000000
        self.uc.mem_map(self.heap, 0x10000)
        self.o.call(0x224270)
        self.masks = {}
        for size in range(1, 8):
            shape = self.heap
            self.o.call(0x1fc340, (size,), this=shape)
            mask = self.uc.mem_read(self.o.read_word(shape + 8), size * size)
            self.masks[size] = [(x, y) for x in range(size) for y in range(size) if mask[x * size + y]]
        # Virtual size and auxiliary flag accessors, with no game rule substituted.
        table, code = self.heap + 0x100, self.heap + 0x200
        self.uc.mem_write(code, b'\x8b\x41\x18\xc3')
        self.o.word(table + 8, code)
        self.o.word(table + 12, self.o.accessor(31, 0, pop=0))
        self.table = table
        for rva, handler in [(0x1651e0, self.fit), (0x163290, self.height), (0x163ec0, self.move)]:
            self.uc.hook_add(UC_HOOK_CODE, self.dispatch(handler), begin=reference.BASE + rva, end=reference.BASE + rva)

    def dispatch(self, handler):
        def hook(uc, address, count, data):
            handler(uc.reg_read(UC_X86_REG_ESP))
        return hook

    def ret(self, sp, value, pop):
        self.uc.reg_write(UC_X86_REG_EAX, value)
        self.uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)
        self.uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))

    def fit(self, sp):
        obj = self.o.read_word(sp + 4) - 8
        x, y = struct.unpack('<ii', self.uc.mem_read(self.o.read_word(sp + 8), 8))
        size = self.objects[obj][0]
        occupied = set(self.blocked)
        for other, (diameter, at) in self.objects.items():
            if other != obj:
                occupied.update((at[0] + dx, at[1] + dy) for dx, dy in self.masks[diameter])
        fits = all(0 <= x + dx < 102 and max(37-x-dx, x+dx-37, 0) <= y+dy < min(38+x+dx, 166-x-dx, 102)
                   and (x+dx, y+dy) not in occupied for dx, dy in self.masks[size])
        self.ret(sp, int(fits), 8)

    def height(self, sp):
        self.ret(sp, 0, 8)

    def move(self, sp):
        obj, point = self.o.read_word(sp+4), self.o.read_word(sp+8)
        xyz = bytes(self.uc.mem_read(point, 12))
        self.uc.mem_write(obj+12, xyz)
        size = self.objects[obj][0]
        x, y, _ = struct.unpack('<iii', xyz)
        self.objects[obj] = (size, (x//16, y//16))
        self.ret(sp, 0, 8)

    def start(self, formation, side, slot, size):
        sp = reference.STACK + 0x8000
        for off, value in [(0xd8, formation), (0xe0, side), (0x2c, slot)]:
            self.o.word(sp+off, value)
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_EAX, size)
        self.uc.emu_start(reference.BASE+0x22dbb6, reference.BASE+0x22dc07, count=100)
        if self.uc.reg_read(UC_X86_REG_EIP) != reference.BASE+0x22dc07:
            raise RuntimeError('coordinate block did not finish')
        return (self.uc.reg_read(UC_X86_REG_ECX), self.uc.reg_read(UC_X86_REG_EDX))

    def run(self, formation, side, sizes, blocked):
        self.objects, self.blocked = {}, blocked
        ids = [self.heap+0x1000+i*0x100 if size else 0 for i,size in enumerate(sizes)]
        for slot, (obj, size) in enumerate(zip(ids, sizes)):
            if not obj:
                continue
            at = self.start(formation, side, slot, size)
            self.o.word(obj+8, self.table)
            self.o.word(obj+0x20, size)
            self.uc.mem_write(obj+12, struct.pack('<iii', at[0]*16, at[1]*16, 0))
            self.objects[obj] = (size, at)
        # Execute the native dispatch block too: its original call order and
        # short-circuit behavior must not be reimplemented by the fixture.
        sp = reference.STACK + 0x8000
        for slot, obj in enumerate(ids):
            self.o.word(sp + 0x44 + slot * 4, obj)
        self.o.word(sp + 0xd8, formation)
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_EDI, self.heap)
        self.uc.emu_start(reference.BASE + 0x22dfbc, reference.BASE + 0x22e0cb, timeout=30_000_000)
        if self.uc.reg_read(UC_X86_REG_EIP) != reference.BASE + 0x22e0cb or self.uc.reg_read(UC_X86_REG_ESP) != sp:
            raise RuntimeError('native compaction dispatch did not finish cleanly')
        return [self.objects[obj][1] if obj else None for obj in ids]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bench = Bench(args.exe)
    patterns = [[s]*7 for s in range(1,8)] + [[3,4,5,6,7,4,3], [0,4,0,7,0,3,0], [4,0,0,0,0,0,0], [0,0,4,0,0,0,0], [0,0,4,0,7,0,0], [0,0,0,0,0,0,4]]
    lines = ['# Buka 3.0 SHA-256 '+reference.SHA256, '# formation side sizes obstacle(-/x,y) positions(-/x,y per slot)']
    for formation in range(3):
        for side in range(2):
            for sizes in patterns:
                for blocked in [set(), {(78,64)} if side == 0 else {(30,46)}]:
                    positions = bench.run(formation, side, sizes, blocked)
                    point = lambda p: '{},{}'.format(*p)
                    lines.append('{} {} {} {} {}'.format(formation, side, ','.join(map(str,sizes)), point(next(iter(blocked))) if blocked else '-', ' '.join(point(p) if p else '-' for p in positions)))
    args.out.write_text('\n'.join(lines)+'\n')
    print('{} native deployment fixtures'.format(len(lines)-2))

if __name__ == '__main__':
    main()
