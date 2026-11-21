#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for where a moving creature stops (16c4a0) and in
how many turns it reaches a cell (171340).

Both run whole on top of the path search bench (combat_path.py): the real search
(20d200) on its random maps, the real path rebuild with the vector of points (20e7b0),
and then the two functions themselves. Extra fixture boundary: the creature's path
finder (1f7680) returns the bench's finder; the battlefield's active creature (+0x28) is
the bench's creature. The targets are picked after the search: cells it reached (the
start among them sometimes) and cells of the occupied-cell phase or not reached at all.
16c4a0 is only asked about reached cells other than the start: for the others the exe
would read a point through a null pointer. See decomp/combat_path.md.
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('combat_path', Path(__file__).with_name('combat_path.py'))
path_bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(path_bench)
reference = path_bench.reference

BASE = path_bench.BASE
FINDER, CREATURE, FIELD, POINT = path_bench.FINDER, path_bench.CREATURE, path_bench.FIELD, path_bench.POINT
TARGET, STOP_OUT = path_bench.HEAP + 0x3400, path_bench.HEAP + 0x3410
REACHED, MARK = 0x01, 0x80


class Bench(path_bench.Bench):
    def __init__(self, exe):
        super().__init__(exe)
        self.uc.hook_add(UC_HOOK_CODE, self.finder, begin=BASE + 0x1f7680, end=BASE + 0x1f7680)

    def finder(self, uc, address, size, _):
        """get_path_finder(bool) of the creature: the bench's finder (no attack mode)."""
        o = self.o
        sp = uc.reg_read(UC_X86_REG_ESP)
        if uc.reg_read(UC_X86_REG_ECX) != CREATURE or o.read_word(sp + 4) & 0xff:
            raise RuntimeError('unexpected path finder request')
        uc.reg_write(UC_X86_REG_EAX, FINDER)
        uc.reg_write(UC_X86_REG_EIP, o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 8)

    def stops(self, world, rng):
        o, uc = self.o, self.uc
        self.build(world)
        uc.mem_write(reference.STACK, bytes(0x8000))
        sx, sy = world['start']
        uc.mem_write(POINT, struct.pack('<ii', sx, sy))
        o.call(0x20e3f0, (POINT,), this=FINDER)
        uc.mem_write(self.cell(sx, sy), struct.pack('<H', world['start_enter']))
        o.call(0x20d200, (), this=FINDER)
        # The creature stands at the start (subcells) and is the battlefield's active one.
        uc.mem_write(CREATURE + 0xc, struct.pack('<iii', sx * 16 + rng.randrange(16), sy * 16 + rng.randrange(16), 0))
        o.word(FIELD + 0x28, CREATURE)
        reached, other = [], []
        for x, (start, end) in enumerate(world['rows']):
            for y in range(start, end):
                flags = struct.unpack('<I', bytes(uc.mem_read(self.cell(x, y) + 8, 4)))[0]
                (reached if flags & REACHED else other).append((x, y))
        targets = rng.sample(reached, min(len(reached), rng.randint(1, 8)))
        targets += rng.sample(other, min(len(other), rng.randint(0, 2)))
        if rng.random() < 0.3:
            targets.append((sx, sy))
        answers = []
        for tx, ty in targets:
            uc.mem_write(TARGET, struct.pack('<ii', tx, ty))
            turns = int(o.call(0x171340, (CREATURE, TARGET), this=0))
            turns = turns - (1 << 32) if turns >= 1 << 31 else turns
            stop = '-'
            if (tx, ty) in reached and (tx, ty) != (sx, sy):
                uc.mem_write(STOP_OUT, bytes(8))
                o.call(0x16c4a0, (STOP_OUT, TARGET), this=FIELD)
                stop = '{},{}'.format(*struct.unpack('<ii', bytes(uc.mem_read(STOP_OUT, 8))))
            answers.append('{}:{}:{}:{}'.format(tx, ty, stop, turns))
        return answers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=400)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(1626)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# S rows mover start zones cells -> answers',
             '# rows, mover, start, zones, cells: as in combat_path_vectors.tsv (the search runs first);',
             '# answers: x:y:stop:turns per target; stop = x,y of 16c4a0 (- when not asked),',
             '#   turns = 171340 (-1 no path, 1 already there or reachable this turn, up to 4)']
    made = 0
    while made < args.count:
        world = path_bench.random_world(rng)
        if world is None:
            continue
        answers = bench.stops(world, rng)
        encoded = path_bench.encode(world).rsplit(' ', 1)[0]  # without the search's targets
        lines.append('S {} -> {}'.format(encoded, ' '.join(answers)))
        made += 1
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe path stop fixtures'.format(made))


if __name__ == '__main__':
    main()
