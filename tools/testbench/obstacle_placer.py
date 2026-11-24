#!/usr/bin/env python3
"""Run the original obstacle placer (RVA 20a270) and compare with h4-rules.

Executes natively: field geometry (34db60), cell constructors (183560), edge
blocking (162cb0), the placer constructor (208ec0) and `execute` with start
position clearing, path cutting, the breadth-first walk, model weighing,
weighted choice and placement bookkeeping, all on the original combat RNG.

The weights table is parsed by the original parser (2089a0) from the installed
`table.combat_obstacles` cells and compared with h4-rules' parse; only the
table cache getter (452100) is replaced.

The final obstruction pass (20a462..20a6ac) runs natively too: the finder
t_combat_obstruction_finder (20b480, its search 20b670 and start 20b4b0,
20ba60, 20b8f0, 20bc10, 20bcf0), the blockage map and template (20e900,
20dfd0), the heap push (20e550) and pop (20c170), the cover marking (20e640),
the "could stand here" query (1651e0 with the real potential creature and
183a10) and clear_rect (209040).

Fixture boundaries (not executed): the header table cache getter (208640)
returns the installed headers, model loading (206f50/17e950) returns the
chosen header's size and type, the image scale (164b20) is 1.0, the profiler
is skipped. Object creation (1772e0) is replaced: a blocking model becomes
square segments of side gcd(w, h) (the split of 238690; the exe drops segments
whose cropped image is empty, synthetic models have none), each a fixture
object in the field's list +0x44c and in the cells of its round footprint;
non-blocking models add nothing (nothing in the pass reads them). Removing an
object (164080) unlinks it from the cells and the list and records it; the
dynamic_cast in clear_rect returns the object itself. The direction offsets
(305860 over the lazily built table 3054b0) are supplied, and the footprint
cache (1fc590, a std::map set up by a static initializer) returns footprints
built by the real constructor 1fc340.
`atexit` registrations of lazily initialized statics are ignored, and the
critical-section imports do nothing (one thread).
Siege battles are not covered.
"""
import argparse
import importlib.util
import math
import struct
import subprocess
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

spec = importlib.util.spec_from_file_location('h4_reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

BASE = reference.BASE
POINT = 0xaaad38          # (1180, 1024), set by the static initializer 208570
WEIGHTS, WEIGHTS_READY = 0xaa7a38, 0xaaad49
PLACER_GUARD = 0xaa7a30   # static-init guard of the "Place obstacles" profiler marker
ENTRY_SIZE = 0x38
# (row, column) offsets of the eight directions, as 3054b0 stores them.
OFFSETS = [(-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1)]


class Original(reference.Original):
    """The shared harness, with the one dynamic_cast of clear_rect (209040) answered:
    every object in a cell list is an obstacle object, so the cast returns it."""

    def service(self, uc, address, size, _):
        if address == 0x8ed444:
            sp = uc.reg_read(UC_X86_REG_ESP)
            uc.reg_write(UC_X86_REG_EAX, self.read_word(sp + 4))
            uc.reg_write(UC_X86_REG_EIP, self.read_word(sp))
            uc.reg_write(UC_X86_REG_ESP, sp + 4)
            return
        super().service(uc, address, size, _)


def segments(w, h, blocks):
    """Segments of a placed model as (dx, dy, size): the bench's stand-in for 238690."""
    if not blocks or w == 0 or h == 0:
        return []
    g = math.gcd(w, h)
    return [(i * g, j * g, g) for i in range(w // g) for j in range(h // g)]
BIG_ARENA, BIG_ARENA_SIZE = 0x10000000, 0x20000000


class Bench:
    def __init__(self, exe, models):
        self.original = Original(exe)
        self.uc = self.original.uc
        # The exe caches field geometry in statics (34db60), so memory is never
        # reused between scenarios: a larger arena that is not reset.
        self.uc.mem_map(BIG_ARENA, BIG_ARENA_SIZE)
        reference.ARENA, reference.ARENA_SIZE = BIG_ARENA, BIG_ARENA_SIZE
        self.original.arena_next = BIG_ARENA
        self.models = models
        o = self.original
        o.call(0x224270)  # start positions table
        o.call(0x208570)  # screen point of the placer's map
        o.call(0x20b3e0)  # the same point for the obstruction finder's blockage map
        o.call(0x20ddf0)  # and for the path finder's own map
        self.uc.mem_write(WEIGHTS_READY, b'\x01')
        self.uc.mem_write(PLACER_GUARD, bytes((self.uc.mem_read(PLACER_GUARD, 1)[0] | 1,)))
        # fld1; ret — the image scale of the battlefield.
        self.stub = reference.HEAP + 0xc000
        self.uc.mem_write(self.stub, b'\xd9\xe8\xc3')
        # Single thread: Initialize/Delete/Enter/LeaveCriticalSection do nothing
        # (stdcall, one argument). Used by the resource cache locks.
        nothing = reference.HEAP + 0xc010
        self.uc.mem_write(nothing, b'\xc2\x04\x00')
        for slot in (0x97005c, 0x970060, 0x970064, 0x970068):
            self.original.word(slot, nothing)
        hooks = {
            0x608640: self.header_table,
            0x564b20: self.scale,
            0x606f50: self.model_cache,
            0x57e950: self.model,
            0x5772e0: self.create_object,
            0x7e7a30: lambda uc, sp: self.ret(sp, 0, 8),
            0x7e7a60: lambda uc, sp: self.ret(sp, 0, 0),
            0x8ed013: lambda uc, sp: self.ret(sp, 0, 0),  # atexit of lazy statics
            0x564080: self.remove_object,
            0x705860: self.direction,
            0x5fc590: self.footprint,
        }
        for address, handler in hooks.items():
            self.uc.hook_add(UC_HOOK_CODE, self.dispatch(handler), begin=address, end=address)
        # Observers only (the code runs on): the finder's construction starts the pass;
        # each clear_rect of the pass is one path point; 20bcf0 ends a path.
        observers = {0x60b480: self.pass_starts, 0x609040: self.cleared, 0x60bcf0: self.path_ends}
        for address, handler in observers.items():
            self.uc.hook_add(UC_HOOK_CODE, self.dispatch(handler), begin=address, end=address)
        self.directions = self.alloc(64)
        uc_table = b''.join(struct.pack('<ii', *d) for d in OFFSETS)
        self.uc.mem_write(self.directions, uc_table)
        # Constant virtual accessors (no arguments): slot n returns n.
        self.constant = {n: o.accessor(10 + n, n, pop=0) for n in range(2)}
        # Round footprints of sizes 0..7, built by the real constructor 1fc340.
        self.footprints = self.alloc(0x100 * 8)
        for size in range(8):
            o.call(0x1fc340, (size,), this=self.footprints + 0x100 * size)
        # CRT static initializers of the keyword maps the parser looks words up in:
        # frequencies (never..usually) and obstacle type names.
        o.call(0x208600)
        o.call(0x3c8aa0)

    def parse_weights(self, rows):
        """Run the original table parser (2089a0) on `rows`; return its 38x85 ints."""
        o, uc = self.original, self.uc

        def string(text):
            data = text.encode('ascii')
            obj = self.alloc(16)
            if data:
                # MSVC 6 copy-on-write string: reference count byte, text, NUL.
                body = self.alloc(len(data) + 2)
                uc.mem_write(body + 1, data)
                o.word(obj + 4, body + 1)
                o.word(obj + 8, len(data))
                o.word(obj + 12, len(data))
            return obj

        def vector(items):
            # Elements are 16-byte objects; copy them contiguously.
            data = self.alloc(16 * max(len(items), 1))
            for i, item in enumerate(items):
                uc.mem_write(data + 16 * i, bytes(uc.mem_read(item, 16)))
            obj = self.alloc(16)
            o.word(obj + 4, data)
            o.word(obj + 8, data + 16 * len(items))
            o.word(obj + 12, data + 16 * len(items))
            return obj

        table = vector([vector([string(cell) for cell in row]) for row in rows])
        hook = self.uc.hook_add(UC_HOOK_CODE, self.dispatch(
            lambda uc, sp: self.cached_table(sp, table)), begin=0x452100, end=0x452100)
        uc.mem_write(WEIGHTS, bytes(4 * 38 * 85))
        o.call(0x2089a0, (WEIGHTS,), callee_pop=False)
        self.uc.hook_del(hook)
        return list(struct.unpack('<%di' % (38 * 85), self.uc.mem_read(WEIGHTS, 4 * 38 * 85)))

    def cached_table(self, sp, table):
        # t_abstract_cache<t_table>::get(out, progress): {no owner, table}.
        out = self.original.read_word(sp + 4)
        self.original.word(out, 0)
        self.original.word(out + 4, table)
        self.ret(sp, out, 8)

    def dispatch(self, handler):
        def run(uc, address, size, _):
            handler(uc, uc.reg_read(UC_X86_REG_ESP))
        return run

    def ret(self, sp, value, pop):
        self.uc.reg_write(UC_X86_REG_EAX, value)
        self.uc.reg_write(UC_X86_REG_EIP, self.original.read_word(sp))
        self.uc.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def alloc(self, size):
        o = self.original
        result = o.arena_next
        o.arena_next += (size + 15) & ~15
        self.uc.mem_write(result, bytes(size))
        return result

    def header_table(self, uc, sp):
        self.ret(sp, self.table, 0)

    def scale(self, uc, sp):
        uc.reg_write(UC_X86_REG_EIP, self.stub)

    def model_cache(self, uc, sp):
        out, entry = self.original.read_word(sp + 4), self.original.read_word(sp + 8)
        self.current = (entry - self.entries) // ENTRY_SIZE
        uc.mem_write(out, bytes(16))
        self.ret(sp, out, 0)  # cdecl

    def model(self, uc, sp):
        out = self.original.read_word(sp + 4)
        w, h, kind, _, _ = self.models[self.current]
        uc.mem_write(self.model_root, struct.pack('<7i', w, h, 0, 0, 0, 0, kind))
        uc.mem_write(out, struct.pack('<II', 0, self.model_root))
        self.ret(sp, out, 8)

    def create_object(self, uc, sp):
        x, y = struct.unpack('<ii', uc.mem_read(self.original.read_word(sp + 8), 8))
        index = len(self.placed)
        self.placed.append((self.current, x >> 4, y >> 4))
        w, h, kind, blocks, skip = self.models[self.current]
        for s, (dx, dy, size) in enumerate(segments(w, h, blocks)):
            self.add_segment((index, s), kind, skip, size, ((x >> 4) + dx, (y >> 4) + dy))
        self.ret(sp, 0, 8)

    def add_segment(self, name, kind, skip, size, corner):
        """A fixture obstacle object, laid out as the template bench lays them out:
        +0 vtable (+0x10 blocks, +0x5c skips blocking), +4 reference count,
        +8 base with its own vtable (+8 size, +0x10 fixed), +0xc position in subcells,
        +0xa0 model (+0x88 type)."""
        o = self.original
        obj = self.alloc(0x400)
        vt, base_vt, model = obj + 0x100, obj + 0x140, obj + 0x200
        o.word(obj, vt)
        o.word(vt + 0x10, self.constant[1])
        o.word(vt + 0x5c, self.constant[skip])
        o.word(obj + 4, 1000)
        o.word(obj + 8, base_vt)
        o.word(base_vt + 0x10, self.constant[1])
        o.word(base_vt + 8, o.accessor(20 + size, size, pop=0))
        o.word(obj + 0xc, corner[0] * 16)
        o.word(obj + 0x10, corner[1] * 16)
        o.word(model + 0x88, kind)
        o.word(obj + 0xa0, model)
        nodes = [self.link(self.field_list, obj)]
        for cx in range(size):
            for cy in range(size):
                d = 2 * cx + 1 - size, 2 * cy + 1 - size
                if d[0] * d[0] + d[1] * d[1] <= size * size + 2:
                    nodes.append(self.link(self.cell_list(corner[0] + cx, corner[1] + cy), obj))
        self.objects[obj] = (name, nodes)

    def link(self, head, obj):
        """Append obj to a std::list (node: next, prev, value) before its head node."""
        o = self.original
        node = self.alloc(16)
        last = o.read_word(head + 4)
        o.word(node, head)
        o.word(node + 4, last)
        o.word(node + 8, obj)
        o.word(last, node)
        o.word(head + 4, node)
        return node

    def cell_list(self, x, y):
        offset, start, end = self.bounds[x]
        if not start <= y < end:
            raise RuntimeError('segment outside the field')
        return self.original.read_word(self.cells + (offset + y) * 0x1c + 8)

    def remove_object(self, uc, sp):
        """t_battlefield::remove_object (164080): unlink and record."""
        o = self.original
        obj = o.read_word(sp + 4)
        name, nodes = self.objects.pop(obj)
        for node in nodes:
            nxt, prev = o.read_word(node), o.read_word(node + 4)
            o.word(prev, nxt)
            o.word(nxt + 4, prev)
        self.removed.append(name)
        self.ret(sp, 0, 4)

    def pass_starts(self, uc, sp):
        self.finding = True

    def cleared(self, uc, sp):
        if self.finding:
            x, y, x1, y1 = struct.unpack('<iiii', uc.mem_read(self.original.read_word(sp + 4), 16))
            if (x1 - x, y1 - y) != (7, 7):
                raise RuntimeError('unexpected rectangle in the obstruction pass')
            self.carved.append('{},{}'.format(x, y))

    def path_ends(self, uc, sp):
        self.carved.append('|')

    def footprint(self, uc, sp):
        size = self.original.read_word(sp + 4)
        if size >= 8:
            raise RuntimeError('unexpected footprint size {}'.format(size))
        self.ret(sp, self.footprints + 0x100 * size, 0)  # cdecl

    def direction(self, uc, sp):
        d = self.original.read_word(sp + 4)
        self.ret(sp, self.directions + 8 * d, 0)  # cdecl

    def run(self, seed, all_water, on_bridge, surface):
        """surface: {(x, y): (terrain, flags)} for every stored cell; returns placements."""
        o, uc = self.original, self.uc
        # compare.Original.combat_start without its arena reset (see __init__).
        uc.mem_write(reference.RNG, bytes(0x28))
        o.word(0xaa1c30, 0)
        o.word(0xaa1c2c, 0)
        o.word(reference.FIELD + 0x438, 2)
        o.call(0x2031b0, (seed, reference.FIELD), this=reference.RNG)
        field = self.alloc(0x600)
        o.call(0x34db60, (102, 16, POINT), this=field + 0x68)
        rows = o.read_word(field + 0x68)
        table = o.read_word(o.read_word(field + 0x7c) + 8)
        bounds = [struct.unpack('<iii', uc.mem_read(table + 12 * x, 12)) for x in range(rows)]
        count = max(offset + end for offset, _, end in bounds)
        cells = self.alloc(count * 0x1c)
        o.word(field + 0x88, cells)
        self.cells, self.bounds = cells, bounds
        stored = 0
        for x, (offset, start, end) in enumerate(bounds):
            for y in range(start, end):
                cell = cells + (offset + y) * 0x1c
                o.call(0x183560, this=cell)
                uc.mem_write(cell, bytes(surface[(x, y)]))
                stored += 1
        if stored != len(surface):
            raise RuntimeError('field geometry differs from h4-rules')
        o.call(0x162cb0, this=field)
        uc.mem_write(field + 0x36c, bytes((all_water,)))
        o.word(field + 0x380, reference.RNG)
        o.word(field + 100, 0)
        # The field's object list (+0x44c) and castle parts (+0x538): empty std::lists.
        self.field_list, castle = self.alloc(16), self.alloc(16)
        for head in (self.field_list, castle):
            o.word(head, head)
            o.word(head + 4, head)
        o.word(field + 0x44c, self.field_list)
        o.word(field + 0x538, castle)
        self.objects, self.removed = {}, []
        self.finding, self.carved = False, []
        window, context = self.alloc(0x200), self.alloc(0x40)
        o.word(field + 0xc0, window)
        o.word(window + 0x140, context)
        o.word(context + 4, 1000)  # reference count: never released here
        o.word(context + 0x20, on_bridge)
        self.entries = self.alloc(len(self.models) * ENTRY_SIZE)
        for i, (w, h, kind, blocks, _) in enumerate(self.models):
            root = self.entries + i * ENTRY_SIZE + 0x10
            uc.mem_write(root + 4, bytes((blocks,)))
            o.word(root + 0x18, kind)
            o.word(root + 0x20, w)
            o.word(root + 0x24, h)
        self.table = self.alloc(16)
        o.word(self.table + 4, self.entries)
        o.word(self.table + 8, self.entries + len(self.models) * ENTRY_SIZE)
        self.model_root = self.alloc(0x40)
        placer = self.alloc(0x60)
        o.call(0x208ec0, (field, 0, 0), this=placer)
        self.placed = []
        sp = reference.STACK + 0x8000
        uc.mem_write(sp, struct.pack('<I', reference.STOP))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, placer)
        try:
            uc.emu_start(BASE + 0x20a270, reference.STOP, timeout=600_000_000)
        except Exception as error:
            eip = uc.reg_read(UC_X86_REG_EIP)
            raise RuntimeError('execute failed at RVA {:x}: {}'.format(eip - BASE, error)) from None
        if uc.reg_read(UC_X86_REG_EIP) != reference.STOP or uc.reg_read(UC_X86_REG_ESP) != sp + 4:
            raise RuntimeError('execute did not return cleanly')
        removed = ['{}.{}'.format(*r) for r in sorted(self.removed)]
        return self.placed, 'carved {} removed {}'.format(' '.join(self.carved), ' '.join(removed))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--inputs', choices=('synthetic', 'installed'), default='synthetic')
    parser.add_argument('--data', default='', help='game data directory for --inputs installed')
    parser.add_argument('--count', type=int, default=24)
    parser.add_argument('--vectors', type=Path, help='write the original placements here')
    parser.add_argument('--carving-vectors', type=Path, help='write the original obstruction pass answers here')
    args = parser.parse_args()
    command = ['cargo', 'run', '--release', '-q', '-p', 'h4-rules', '--example', 'obstacle_reference', '--',
               args.data, args.inputs]

    def reference_output(mode):
        return subprocess.run(command + [mode, str(args.count)], cwd=reference.ROOT, check=True,
                              capture_output=True, text=True).stdout.splitlines()

    lines = iter(reference_output('dump'))
    count = int(next(lines).split()[1])
    models = [tuple(int(v) for v in next(lines).split()) for _ in range(count)]
    weights = [int(v) for v in next(lines).split()[1:]]
    rows = [next(lines).split('\t')[1:] for _ in range(int(next(lines).split()[1]))]
    scenarios = []
    for line in lines:
        words = line.split()
        if words[0] == 'scenario':
            scenarios.append((tuple(int(v) for v in words[1:]), {}))
        else:
            x, y, terrain, flags = (int(v) for v in words[1:])
            scenarios[-1][1][(x, y)] = (terrain, flags)

    bench = Bench(args.exe, models)
    native = bench.parse_weights(rows)
    differences = [i for i, (a, b) in enumerate(zip(native, weights)) if a != b]
    print('weights table: {} values, {} differ from h4-rules'.format(len(native), len(differences)))
    for i in differences[:5]:
        print('  type {} slot {}: exe {} ours {}'.format(i // 85, i % 85, native[i], weights[i]))
    expected, carvings = [], []
    for (seed, all_water, on_bridge), surface in scenarios:
        placed, carving = bench.run(seed, all_water, on_bridge, surface)
        expected.append('placed ' + ' '.join('{} {} {}'.format(*p) for p in placed))
        carvings.append(carving)
        print('seed {:5} water {} bridge {}: {} obstacles, {} removed by the obstruction pass'.format(
            seed, all_water, on_bridge, len(placed), len(carving.split('removed')[1].split())))
    mismatches = []
    for name, theirs, ours in (('placements', expected, reference_output('place')),
                               ('carving', carvings, reference_output('carve'))):
        wrong = [i for i, (a, b) in enumerate(zip(theirs, ours)) if a.split() != b.split()]
        if len(ours) != len(theirs):
            wrong.append('count')
        print('{}: {} scenarios, {} mismatches'.format(name, len(theirs), len(wrong)))
        for i in wrong[:3]:
            if isinstance(i, int):
                print('exe :', theirs[i][:300])
                print('ours:', ours[i][:300])
        mismatches += wrong
    if differences:
        mismatches.append('weights')
    print('{} placements'.format(sum(len(e.split()) // 3 for e in expected)))
    origin = '# inputs: {} {}; one line per scenario of support/obstacle_case.rs'.format(args.inputs, args.count)
    if args.vectors:
        header = ['# Native RVA 2089a0 (weights) + 34db60/183560/162cb0 (field) + 208ec0/20a270 (placer);',
                  '# SHA-256 ' + reference.SHA256, origin + ': model x y ...']
        args.vectors.write_text('\n'.join(header + expected) + '\n')
    if args.carving_vectors:
        header = ['# Native RVA 20a270 (placer) with its obstruction pass 20a462..20a6ac (20b480..20bcf0, 209040);',
                  '# SHA-256 ' + reference.SHA256,
                  origin + ': carved, the corner of every cleared 7x7 square',
                  '# with | after each path, then removed, placement.segment of every removed segment (sorted)']
        args.carving_vectors.write_text('\n'.join(header + carvings) + '\n')
    raise SystemExit(1 if mismatches else 0)


if __name__ == '__main__':
    main()
