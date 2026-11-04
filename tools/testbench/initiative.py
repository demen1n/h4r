#!/usr/bin/env python3
"""Record Buka 3.0 initiative, morale, wait and defense reference vectors.

Executes selection 16f250, comparison 1f2d30, adjusted speed 1f2cc0,
round morale RNG 1f2c30 and morale classification branches in 1f32c0.
Virtual count/speed/side and effect/ability accessors are fixture inputs.
Defense executes stance block 1f1b08..1f1b46; wait/defend execute flag blocks
172d9d..172db5 / 172b9d..172bb9. Three-round traces also run full
round reset 1ecfb0, selection-time stance clearing and side update blocks.
UI, the full action pipeline and spell/hero-stat resolution are excluded.
"""
import argparse
import importlib.util
import itertools
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EFLAGS

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.heap = 0x7000000
        self.uc.mem_map(self.heap, 0x20000)
        self.field = self.heap
        self.table, self.vbtable, self.effects = self.heap+0x1000, self.heap+0x1200, self.heap+0x1300
        code = self.heap+0x1400
        # ECX points at unit+0x98. Effective stats and side are fixture data.
        self.uc.mem_write(code, b'\x8b\x81\xe8\x08\x00\x00\xc3')
        self.uc.mem_write(code+16, b'\x8b\x41\x18\xc3')
        self.uc.mem_write(code+32, b'\x0f\xb6\x41\x08\xc3')
        for slot, ptr in [(0x54,code),(0x40,code+16),(0x74,code+32),(0x68,self.o.accessor(30,0))]:
            self.o.word(self.table+slot, ptr)
        self.o.word(self.vbtable+8, 0x800)
        self.effect_stub = self.o.accessor(31,0)
        self.o.word(self.effects+8,self.effect_stub)
        self.classifying = False
        for rva, result in [(0x1f330d,-1),(0x1f33bd,1),(0x1f3482,0)]:
            def capture(uc, address, size, data, result=result):
                if self.classifying:
                    self.result = result
                    uc.emu_stop()
            self.uc.hook_add(UC_HOOK_CODE,capture,begin=reference.BASE+rva,end=reference.BASE+rva)

    def obj(self, i):
        return self.heap+0x3000+i*0x1000

    def setup(self, i, side, speed, morale, roll, wait, alive=1, done=0):
        obj = self.obj(i)
        self.uc.mem_write(obj, bytes(0xc40))
        self.o.word(obj+0x98,self.table)
        self.o.word(obj+0x9c,self.vbtable)
        self.o.word(obj+0x89c,self.effects)
        self.o.word(obj+0x980,speed)
        self.o.word(obj+0xb0,alive)
        self.o.word(obj+0xae8,morale)
        self.o.word(obj+0xaf0,roll)
        self.uc.mem_write(obj+0xa0,bytes((side,)))
        self.uc.mem_write(obj+0xa8,bytes((done,)))
        self.uc.mem_write(obj+0xc08,bytes((wait,)))
        # Native branches decide the outcome. Freeze flags as the animation
        # layer would, so comparison no longer re-evaluates current morale.
        outcome = self.classify(obj)
        self.uc.mem_write(obj+0xa49,bytes((outcome == -1,)))
        self.uc.mem_write(obj+0xa4b,bytes((outcome == 1,)))
        return obj

    def classify(self, obj):
        self.uc.mem_write(obj+0xaf4,b'\x00')
        self.uc.reg_write(UC_X86_REG_ESI,obj)
        self.uc.reg_write(UC_X86_REG_ECX,obj)
        self.uc.reg_write(UC_X86_REG_EBX,0)
        self.uc.reg_write(UC_X86_REG_ESP,reference.STACK+0x8000)
        self.classifying=True
        self.result=None
        self.uc.emu_start(reference.BASE+0x1f32ea,reference.STOP,count=1000)
        self.classifying=False
        if self.result is None:
            raise RuntimeError('morale classification did not reach a native branch')
        return self.result

    def select(self, entries, last_side):
        sentinel=self.heap+0x1800
        self.o.word(self.field+0xd4,sentinel)
        nodes=[self.heap+0x1900+16*i for i in range(len(entries))]
        self.o.word(sentinel,nodes[0] if nodes else sentinel)
        for i,(node,entry) in enumerate(zip(nodes,entries)):
            obj=self.setup(i,*entry)
            self.o.word(node,nodes[i+1] if i+1<len(nodes) else sentinel)
            self.o.word(node+8,obj)
        selected=int(self.o.call(0x16f250,(last_side,),this=self.field))
        return -1 if selected==0 else (selected-self.obj(0))//0x1000

    def block(self, begin, end, obj):
        self.o.word(self.field+0x28,obj)
        self.uc.reg_write(UC_X86_REG_ESI,self.field)
        self.uc.reg_write(UC_X86_REG_EBX,0)
        self.uc.emu_start(reference.BASE+begin,reference.BASE+end,count=100)
        if self.uc.reg_read(UC_X86_REG_EIP)!=reference.BASE+end:
            raise RuntimeError('action flag block did not finish')


    def trace(self, seed, entries):
        count = len(entries)
        o, uc = self.o, self.uc
        o.combat_start(seed, count + 1)
        o.word(reference.FIELD + 0x380, reference.RNG)
        o.accessor(31, 0)
        sentinel = self.heap + 0x1800
        o.word(self.field + 0xd4, sentinel)
        o.word(sentinel, self.heap + 0x1900)
        uc.mem_write(self.field + 0x37c, b'\x01')
        for i, (side, speed, morale, plan) in enumerate(entries):
            obj = self.setup(i, side, speed, morale, 0, 0)
            node = self.heap + 0x1900 + 16 * i
            o.word(node, node + 16 if i + 1 < count else sentinel)
            o.word(node + 8, obj)
            o.word(obj + 0xa18, reference.FIELD)
            o.word(obj + 0xa30, i + 1)
            o.word(obj + 0xb4, -1)  # no artifact slot updates in this fixture
            o.word(obj + 0x9f8, self.heap + 0x1c00)
        result = []
        for round_num in range(1, 4):
            for i in range(count):
                obj = self.obj(i)
                o.call(0x1ecfb0, this=obj)  # full native round reset + morale RNG
                outcome = self.classify(obj)
                uc.mem_write(obj + 0xa49, bytes((outcome == -1,)))
                uc.mem_write(obj + 0xa4b, bytes((outcome == 1,)))
            for _ in range(2 * count + 1):
                last = uc.mem_read(self.field + 0x37c, 1)[0]
                obj = int(o.call(0x16f250, (last,), this=self.field))
                if not obj:
                    break
                i = (obj - self.obj(0)) // 0x1000
                priority = int(o.call(0x1f2cc0, this=obj))
                if priority >= 2**31:
                    priority -= 2**32
                # Native selected-unit defense expiry and last-side update.
                uc.reg_write(UC_X86_REG_EAX, obj)
                uc.emu_start(reference.BASE + 0x167e2f, reference.STOP, count=1)
                uc.reg_write(UC_X86_REG_EBX, obj + 0x98)
                uc.reg_write(UC_X86_REG_ESI, self.field)
                uc.emu_start(reference.BASE + 0x1682e5, reference.BASE + 0x1682f4, count=20)
                plan = entries[i][3]
                action = 'w' if plan == 'w' and not uc.mem_read(obj + 0xc08, 1)[0] else ('d' if plan in ('w', 'd') else 'f')
                if action == 'w':
                    self.block(0x172d9d, 0x172db5, obj)
                else:
                    if action == 'd':
                        self.block(0x172b9d, 0x172bb9, obj)
                    uc.reg_write(UC_X86_REG_EAX, obj)
                    uc.emu_start(reference.BASE + 0x16b34a, reference.STOP, count=1)
                defense = sum(1 << j for j in range(count) if uc.mem_read(self.obj(j) + 0xa38, 1)[0])
                result.append('{},{},{},{},{}'.format(round_num, i, action, priority, defense))
            else:
                raise RuntimeError('native trace did not finish the round')
        return ';'.join(result)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    bench=Bench(args.exe)
    o,uc=bench.o,bench.uc
    lines=['# Buka 3.0 SHA-256 '+reference.SHA256,
           '# M morale roll outcome; P last-side [side speed morale roll wait]x2 before rank0 rank1',
           '# S last-side [side,speed,morale,roll,wait,alive,done;...] selected-index',
           '# R seed modulus unit-count rounds rolls; D float-bits defending effect-1f result-bits; A wait-flag defend-flag field-action']
    for morale,roll in itertools.product(range(-10,11),range(10)):
        obj=bench.setup(0,0,5,morale,roll,0)
        lines.append('M {} {} {}'.format(morale,roll,bench.classify(obj)))
    rng=random.Random(60)
    values=[-2147483648,-1001,-1,0,1,5,10,20,999,2147483647]
    for i in range(1200):
        entries=[]
        for _ in range(2):
            entries.append((rng.randrange(2),rng.choice(values),rng.choice([-10,-3,0,4,10]),rng.randrange(10),rng.randrange(2)))
        last=rng.randrange(2)
        a,b=[bench.setup(i,*entry) for i,entry in enumerate(entries)]
        result=int(o.call(0x1f2d30,(b,last),this=a))&255
        ranks=[int(o.call(0x1f2cc0,this=obj)) for obj in (a,b)]
        ranks=[v if v<2**31 else v-2**32 for v in ranks]
        lines.append('P {} {} {} {}'.format(last,' '.join(str(v) for entry in entries for v in entry),result,' '.join(map(str,ranks))))
    for i in range(160):
        entries=[(rng.randrange(2),rng.choice([3,5,5,5,10]),rng.choice([-10,0,0,10]),rng.randrange(10),rng.randrange(2),rng.randrange(2),rng.randrange(2)) for _ in range(rng.randrange(1,8))]
        last=rng.randrange(2)
        lines.append('S {} {} {}'.format(last,';'.join(','.join(map(str,e)) for e in entries),bench.select(entries,last)))
    for seed,modulus in itertools.product([0,1,1234,0xffffffff],[2,8,15]):
        count=min(modulus-1,7)
        o.combat_start(seed,modulus)
        o.word(reference.FIELD+0x380,reference.RNG)
        rolls=[]
        for _ in range(3):
            for i in range(count):
                obj=bench.obj(i)
                o.word(obj+0xa18,reference.FIELD)
                o.word(obj+0xa30,i+1)
                o.call(0x1f2c30,this=obj)
                rolls.append(o.read_word(obj+0xaf0))
        lines.append('R {} {} {} 3 {}'.format(seed,modulus,count,' '.join(map(str,rolls))))
    for base,defending,effect in itertools.product([0.,1.,3.25,7.,99.,1000.125],range(2),range(2)):
        obj=bench.setup(0,0,5,0,0,0)
        o.accessor(31,effect)
        uc.mem_write(obj+0xa38,bytes((defending,)))
        sp=reference.STACK+0x8000
        uc.mem_write(sp+12,struct.pack('<f',base))
        uc.reg_write(UC_X86_REG_ESP,sp)
        uc.reg_write(UC_X86_REG_ESI,obj+0xc20)
        uc.reg_write(UC_X86_REG_EFLAGS,2 if defending else 0x42)
        uc.emu_start(reference.BASE+0x1f1b08,reference.BASE+0x1f1b46,count=100)
        if uc.reg_read(UC_X86_REG_EIP)!=reference.BASE+0x1f1b46:
            raise RuntimeError('defense block did not finish')
        lines.append('D {} {} {} {}'.format(struct.pack('<f',base).hex(),defending,effect,bytes(uc.mem_read(sp+12,4)).hex()))
    obj=bench.setup(0,0,5,0,0,0)
    bench.block(0x172d9d,0x172db5,obj)
    wait=uc.mem_read(obj+0xc08,1)[0]
    bench.block(0x172b9d,0x172bb9,obj)
    lines.append('A {} {} {}'.format(wait,uc.mem_read(obj+0xa38,1)[0],o.read_word(bench.field+0x500)))
    for seed in [0, 1, 1234, 0xffffffff]:
        for n in [3, 5, 7]:
            entries = [(0 if i < n//2 else 1, rng.choice([3,5,5,10]), rng.choice([-10,-3,0,4,10]), rng.choice(['w','d','f'])) for i in range(n)]
            trace = bench.trace(seed, entries)
            lines.append('T {} {} {}'.format(seed, ';'.join(','.join(map(str,e)) for e in entries), trace))
    args.out.write_text('\n'.join(lines)+'\n')
    print('{} original-exe initiative fixtures'.format(sum(not line.startswith('#') for line in lines)))

if __name__=='__main__':
    main()
