#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the template path map of a footprint size.

The battlefield keeps one template path map per footprint size (cache 10e4f0).
On a miss it builds the battlefield's blockage map once (20e900) and fills the
template from it (20dfd0). Both run whole here, with everything they call: the
footprint-fits check of t_battlefield (1650d0), the blockage query (20eb20), the
footprint marking (20ebf0), the point shift (10d000), the object type (477790),
the footprint getter (f0f0) and the virtual accessors of the objects.

Fixture boundaries: the footprint cache 1fc590 returns footprints built by the
real constructor 1fc340; objects answer their virtual queries from supplied
values (+0x10 of the object, +0x10 and the size +8 of its base at +8).
See decomp/combat_path.md.
"""
import argparse
import importlib.util
import random
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

BASE = reference.BASE
HEAP = 0x7000000
FIELD = HEAP  # 16-aligned: the blockage values take their spare bits from this address
INFO, RECORDS = HEAP + 0x1000, HEAP + 0x1100
BLOCKAGE, TEMPLATE = HEAP + 0x1800, HEAP + 0x1900
FIRST_LIST, SECOND_LIST = HEAP + 0x1a00, HEAP + 0x1a10
FOOTPRINTS = HEAP + 0x2000  # 0x100 per size
OBJECTS = HEAP + 0x8000  # 0x400 per object
NODES = HEAP + 0x10000
CELLS, VALUES, PATH = HEAP + 0x20000, HEAP + 0x40000, HEAP + 0x50000
MAX_SIZE = 8
TYPES = {'o': 5, 'g': 0x22, 't': 0x23, 'w': 0x24}


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x100000)
        self.uc.hook_add(UC_HOOK_CODE, self.footprint, begin=BASE + 0x1fc590, end=BASE + 0x1fc590)
        for size in range(MAX_SIZE):
            self.o.call(0x1fc340, (size,), this=FOOTPRINTS + 0x100 * size)
        # Constant virtual accessors shared by all objects: slot n returns n.
        self.constant = {n: self.o.accessor(n, n, pop=0) for n in range(MAX_SIZE)}

    def footprint(self, uc, address, size, _):
        sp = uc.reg_read(UC_X86_REG_ESP)
        wanted = self.o.read_word(sp + 4)
        if wanted >= MAX_SIZE:
            raise RuntimeError('unexpected footprint size {}'.format(wanted))
        uc.reg_write(UC_X86_REG_EAX, FOOTPRINTS + 0x100 * wanted)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def grid(self, obj, rows, cells):
        o = self.o
        o.word(obj, len(rows))
        o.word(obj + 0x14, INFO)
        o.word(obj + 0x20, cells)

    def object(self, k, item):
        o = self.o
        kind, blocks, fixed, size, position = item
        obj = OBJECTS + 0x400 * k
        self.uc.mem_write(obj, bytes(0x400))
        vt, base_vt, model = obj + 0x100, obj + 0x140, obj + 0x200
        o.word(obj, vt)
        o.word(vt + 0x10, self.constant[blocks])
        o.word(obj + 8, base_vt)
        o.word(base_vt + 0x10, self.constant[fixed])
        o.word(base_vt + 8, self.constant[size])
        for i, v in enumerate(position + (0,)):
            o.word(obj + 0xc + 4 * i, v)
        o.word(obj + 0xa0, model)
        o.word(model + 0x88, TYPES[kind])
        return obj

    def chain(self, head, objects, first_node):
        """A std::list: the head node's +0 is the first node; each node: next, prev, value."""
        o = self.o
        nodes = [NODES + 0x10 * (first_node + i) for i in range(len(objects))]
        ring = [head] + nodes
        for i, node in enumerate(ring):
            o.word(node, ring[(i + 1) % len(ring)])
            o.word(node + 4, ring[i - 1])
        for node, obj in zip(nodes, objects):
            o.word(node + 8, obj)

    def run(self, world):
        o, uc = self.o, self.uc
        rows = world['rows']
        uc.mem_write(FIELD, bytes(0x1000))
        index = 0
        bases = []
        for x, (start, end) in enumerate(rows):
            o.word(RECORDS + 12 * x, index - start)
            o.word(RECORDS + 12 * x + 4, start)
            o.word(RECORDS + 12 * x + 8, end)
            bases.append(index - start)
            index += end - start
        o.word(FIELD + 0x68, len(rows))
        o.word(FIELD + 0x7c, INFO)
        o.word(INFO + 8, RECORDS)
        o.word(FIELD + 0x88, CELLS)
        uc.mem_write(CELLS, bytes(28 * index))
        for x, y in world['blocked']:
            uc.mem_write(CELLS + 28 * (bases[x] + y) + 1, b'\x04')
        objects = [self.object(k, item) for k, item in enumerate(world['objects'])]
        first = objects[:len(world['first'])]
        o.word(FIELD + 0x44c, FIRST_LIST)
        o.word(FIELD + 0x538, SECOND_LIST)
        self.chain(FIRST_LIST, first, 0)
        self.chain(SECOND_LIST, objects[len(first):], len(first))
        uc.mem_write(VALUES, bytes(4 * index))
        uc.mem_write(BLOCKAGE, bytes(0x40))
        self.grid(BLOCKAGE, rows, VALUES)
        uc.mem_write(PATH, bytes(28 * index))
        uc.mem_write(TEMPLATE, bytes(0x40))
        self.grid(TEMPLATE, rows, PATH)
        o.call(0x20e900, (FIELD,), this=BLOCKAGE)
        o.call(0x20dfd0, (FIELD, BLOCKAGE, FOOTPRINTS + 0x100 * world['size']), this=TEMPLATE)
        if o.read_word(TEMPLATE + 0x2c) != world['size']:
            raise RuntimeError('template size not stored')
        values = struct.unpack('<{}I'.format(index), bytes(uc.mem_read(VALUES, 4 * index)))
        records = bytes(uc.mem_read(PATH, 28 * index))
        blockage, template = [], []
        k = 0
        for x, (start, end) in enumerate(rows):
            for y in range(start, end):
                # Only the low four bits mean something: 20e900 builds its values from a
                # register holding the battlefield's address (bits 4 and up come from it).
                if values[k] & 0xf:
                    blockage.append('{}:{}:{:x}'.format(x, y, values[k] & 0xf))
                record = records[28 * k:28 * k + 28]
                flags = struct.unpack_from('<I', record, 8)[0]
                if record[:8] != bytes(8) or record[12:] != bytes(16):
                    raise RuntimeError('template record has more than flags')
                if flags:
                    template.append('{}:{}:{:x}'.format(x, y, flags))
                k += 1
        return ';'.join(blockage) or '-', ';'.join(template) or '-'


def random_world(rng):
    height = rng.randint(1, 20)
    rows = []
    for x in range(height):
        if rng.random() < 0.6:
            middle = height / 2
            inset = int(abs(x - middle) * rng.choice([0.5, 1]))
            rows.append((inset, max(inset + 1, rng.randint(12, 20) - inset)))
        else:
            start = rng.choice([0, 0, 1, 3])
            rows.append((start, start + rng.randint(1, 20)))
    blocked = []
    density = rng.choice([0, 0.03, 0.1, 0.3])
    for x, (start, end) in enumerate(rows):
        for y in range(start, end):
            if rng.random() < density:
                blocked.append((x, y))

    def item(kinds):
        size = rng.choice([0, 1, 1, 2, 2, 3, 4, 5, 6, 7])
        # positions in subcells: low bits, a little outside the field, negatives
        position = (rng.randrange(-40, 16 * height + 16), rng.randrange(-40, 16 * 22), rng.randrange(-3, 3))
        return (rng.choice(kinds), int(rng.random() < 0.8), int(rng.random() < 0.7), size, position)

    first = [item('o') for _ in range(rng.randint(0, 8))]
    second = [item('ttwwgo') for _ in range(rng.randint(0, 6))]
    return {'rows': rows, 'blocked': blocked, 'objects': first + second, 'first': first,
            'size': rng.choice([0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 7])}


def encode(world):
    rows = ';'.join('{}:{}'.format(*r) for r in world['rows'])
    blocked = ';'.join('{}:{}'.format(*c) for c in world['blocked']) or '-'
    def objects(items):
        return ';'.join('{}:{}:{}:{}:{}:{}:{}'.format(kind, blocks, fixed, size, *position)
                        for kind, blocks, fixed, size, position in items) or '-'
    n = len(world['first'])
    return ' '.join([rows, blocked, objects(world['objects'][:n]), objects(world['objects'][n:]), str(world['size'])])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=600)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(6262)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# M rows blocked objects towers size -> blockage template',
             '# rows: start:end per row; blocked: x:y of cells with the live flag 0x04;',
             '# objects (list +0x44c) and towers (list +0x538): kind:blocks:fixed:size:x:y:z, position in',
             '#   subcells; kind o obstacle (type 5), g castle_gate, t castle_tower, w castle_wall;',
             '# size: the mover footprint; blockage: x:y:value & 0xf hex (20e900); template: x:y:flags hex (20dfd0)']
    for _ in range(args.count):
        world = random_world(rng)
        blockage, template = bench.run(world)
        lines.append('M {} -> {} {}'.format(encode(world), blockage, template))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe path template fixtures'.format(args.count))


if __name__ == '__main__':
    main()
