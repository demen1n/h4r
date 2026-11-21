#!/usr/bin/env python3
"""Record Buka 3.0 reference vectors for the path map preparation of one creature (20c7f0).

Executes 20c7f0 whole: the copy of the template map (20cfb0), the walk over the
battlefield's object list with its filters, the marking of objects with the
expanded footprint (20e0f0, real constructor 1fc460 and overlap test 1fc810),
the start (20e3f0, or for a creature in a tower 20c6d0 / 20c400 with the direction
51ede0), the copy of the terrain map (20cef0), the gate decision and the entry cost
loop, and the vector/heap code they use.

Fixture boundaries, each a plain lookup in supplied data:
- the battlefield's caches: the template map of the size (10e4f0) and the terrain map
  (10e780) return supplied maps; the footprint cache 1fc590 returns footprints built by
  the real 1fc340; the expanded footprint cache 1fc8e0 returns one built by the real
  1fc460 from them;
- the terrain cost list of the creature (225320) and the terrain map refreshes
  (225260, 2252d0: no-ops, the masks are supplied after refreshing);
- dynamic_cast (8ed444), "which creature stands in this cell" (170d90), "which tower
  does this creature stand in" (1f60a0), hostility (cd80), cowardice (1f52f0);
- the gate: has_gate (177b90), the two state queries 177990 and 1778c0, its rect
  (177bf0), opening it for the terrain refresh (1779c0, recorded);
- virtual accessors of the objects and the creature.
See decomp/combat_path.md.
"""
import argparse
import importlib.util
import random
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
FINDER, VTABLE = HEAP, HEAP + 0x100
FIELD = HEAP + 0x1000
TEMPLATE, TERRAIN, INFO, RECORDS = HEAP + 0x2800, HEAP + 0x2900, HEAP + 0x2a00, HEAP + 0x2b00
MOVER = HEAP + 0x4000
TOWERS = HEAP + 0x6000  # 0x200 each: towers creatures stand in
FOOTPRINTS = HEAP + 0x7000  # 0x100 per size
EXPANDED = HEAP + 0x16000  # 0x40 per pair of sizes
OBJECTS = HEAP + 0x40000  # 0x1000 per object, occupants of towers 16 objects later
NODES = HEAP + 0x1c000
CELLS, MASKS = HEAP + 0x20000, HEAP + 0x30000
STATIONARY, CREATURE_TYPE = 0xa592d8, 0xa583a8
TYPES = {'o': 5, 'g': 0x22, 't': 0x23, 'w': 0x24}
MAX_SIZE = 8

# dynamic_cast is one of compare.py's guarded services; route it to the active bench.
_service = reference.Original.service
ACTIVE = []


def service(self, uc, address, size, user):
    if address == 0x8ed444 and ACTIVE:
        try:
            return ACTIVE[0].cast(uc)
        except Exception as error:  # noqa: BLE001 - reported after emu_stop
            ACTIVE[0].error = error
            return uc.emu_stop()
    return _service(self, uc, address, size, user)


reference.Original.service = service


class Bench:
    def __init__(self, exe):
        self.o = reference.Original(exe)
        self.uc = self.o.uc
        self.uc.mem_map(HEAP, 0x100000)
        ACTIVE.append(self)
        o = self.o
        for size in range(MAX_SIZE):
            o.call(0x1fc340, (size,), this=FOOTPRINTS + 0x100 * size)
        self.constant = {n: o.accessor(n, n, pop=0) for n in range(MAX_SIZE)}
        # Expanded footprints for every pair of sizes, built up front by the real 1fc460
        # (emulation can't be started again from inside a hook).
        self.expanded_shapes = {}
        for a in range(MAX_SIZE):
            for b in range(MAX_SIZE):
                shape = EXPANDED + 0x40 * (MAX_SIZE * a + b)
                o.call(0x1fc460, (FOOTPRINTS + 0x100 * a, FOOTPRINTS + 0x100 * b), this=shape)
                self.expanded_shapes[(a, b)] = shape
        self.arena_start = o.arena_next
        self.hooks = {
            0x10e4f0: (self.template, 4), 0x10e780: (self.terrain, 4), 0x1fc590: (self.footprint, None),
            0x1fc8e0: (self.expanded, None), 0x225320: (self.costs, 4), 0x225260: (self.noop, 4),
            0x2252d0: (self.noop, 8), 0x170d90: (self.stands_in_cell, 4), 0x1f60a0: (self.tower_of, 0),
            0xcd80: (self.hostile, 4), 0x1f52f0: (self.cowardly, 4), 0x177b90: (self.has_gate, 0),
            0x177990: (self.gate_state_a, 0), 0x1778c0: (self.gate_state_b, 0), 0x177bf0: (self.gate_rect, 4),
            0x1779c0: (self.open_gate, 4),
        }
        for rva in self.hooks:
            self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=BASE + rva, end=BASE + rva)

    def hook(self, uc, address, size, _):
        # An exception escaping a Unicorn hook aborts the process; stop and report instead.
        try:
            self.handle(uc, address)
        except Exception as error:  # noqa: BLE001 - reported after emu_stop
            self.error = error
            uc.emu_stop()

    def handle(self, uc, address):
        handler, pop = self.hooks[address - BASE]
        sp = uc.reg_read(UC_X86_REG_ESP)
        result = handler(uc.reg_read(UC_X86_REG_ECX), lambda i: self.o.read_word(sp + 4 * i))
        uc.reg_write(UC_X86_REG_EAX, result & 0xffffffff)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + (pop or 0))  # None: cdecl

    # --- boundaries -------------------------------------------------------
    def template(self, this, arg):
        assert this == FIELD + 0xe8 and arg(1) == self.world['size'], 'template lookup'
        return TEMPLATE

    def terrain(self, this, arg):
        assert this == FIELD + 0xe8 and arg(1) == self.world['size'], 'terrain lookup'
        return TERRAIN

    def footprint(self, this, arg):
        assert arg(1) < MAX_SIZE, 'footprint size'
        return FOOTPRINTS + 0x100 * arg(1)

    def expanded(self, this, arg):
        return self.expanded_shapes[(arg(1), arg(2))]

    def costs(self, this, arg):
        assert arg(1) == MOVER, 'cost list owner'
        entries = self.world['costs']
        table = self.o.arena_next
        self.o.arena_next += 8 * len(entries) + 16
        for i, (mask, cost) in enumerate(entries):
            self.uc.mem_write(table + 8 * i, struct.pack('<IHH', mask, cost, 0))
        self.o.word(this + 4, table if entries else 0)
        self.o.word(this + 8, table + 8 * len(entries) if entries else 0)
        self.o.word(this + 12, table + 8 * len(entries) if entries else 0)
        self.calls.append('costs')
        return 0

    def noop(self, this, arg):
        return 0

    def stands_in_cell(self, this, arg):
        assert this == FIELD, 'cell lookup'
        x, y = struct.unpack('<ii', bytes(self.uc.mem_read(arg(1), 8)))
        return self.occupants.get((x, y), 0)

    def tower_of(self, this, arg):
        return self.tower_address[this]

    def hostile(self, this, arg):
        return self.facts[this - 0x98][0]

    def cowardly(self, this, arg):
        assert this == MOVER, 'cowardice of the mover'
        return self.facts[arg(1)][1]

    def has_gate(self, this, arg):
        return self.world['gate'][0]

    def gate_state_a(self, this, arg):
        return self.world['gate'][1]

    def gate_state_b(self, this, arg):
        return self.world['gate'][2]

    def gate_rect(self, this, arg):
        self.uc.mem_write(arg(1), bytes(16))
        return arg(1)

    def open_gate(self, this, arg):
        self.calls.append('open{}'.format(arg(1)))
        return 0

    def cast(self, uc):
        sp = uc.reg_read(UC_X86_REG_ESP)
        pointer, wanted = self.o.read_word(sp + 4), self.o.read_word(sp + 16)
        obj = self.base_of.get(pointer, pointer)
        kind = self.kind.get(obj)
        if kind is None:
            raise RuntimeError('unexpected dynamic_cast of {:x}'.format(pointer))
        if wanted == STATIONARY:
            result = obj if kind in TYPES else 0
        elif wanted == CREATURE_TYPE:
            result = obj if kind in 'cm' else 0
        else:
            raise RuntimeError('unexpected dynamic_cast target {:x}'.format(wanted))
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_EIP, self.o.read_word(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    # --- building ---------------------------------------------------------
    def grid(self, obj, cells):
        o = self.o
        self.uc.mem_write(obj, bytes(0x40))
        o.word(obj, len(self.world['rows']))
        o.word(obj + 0x14, INFO)
        o.word(obj + 0x20, cells)
        o.word(obj + 0x24, cells + self.count * self.item[obj])
        o.word(obj + 0x28, cells + self.count * self.item[obj])
        o.word(obj + 0x2c, self.world['size'])

    def creature(self, obj, slot, size, position, effects, in_tower):
        o = self.o
        self.uc.mem_write(obj, bytes(0x1000))
        table, vbtable, base_vt, effects_vt, vt = obj + 0xf00, obj + 0xf80, obj + 0xfc0, obj + 0xe80, obj + 0xd00
        o.word(obj, vt)
        o.word(obj + 0x98, table)
        o.word(obj + 0x9c, vbtable)
        o.word(vbtable + 8, VB)
        o.word(obj + 0x9c + VB, effects_vt)
        o.word(effects_vt + 8, o.lookup(slot, effects))
        o.word(obj + 8, base_vt)
        o.word(base_vt + 8, self.constant[size])
        for i, v in enumerate(position + (0,)):
            o.word(obj + 0xc + 4 * i, v)
        self.uc.mem_write(obj + 0x9bc, bytes((int(in_tower is not None),)))
        o.word(obj + 0xa18, FIELD)
        return table, effects_vt, vt, base_vt

    def tower(self, k, size, position):
        o = self.o
        obj = TOWERS + 0x200 * k
        self.uc.mem_write(obj, bytes(0x200))
        o.word(obj + 8, obj + 0x100)
        o.word(obj + 0x108, self.constant[size])
        for i, v in enumerate(position + (0,)):
            o.word(obj + 0xc + 4 * i, v)
        return obj

    def run(self, world):
        o, uc = self.o, self.uc
        self.world = world
        o.arena_next = self.arena_start  # keep the expanded footprints' masks
        self.calls, self.occupants = [], {}
        self.base_of, self.kind, self.facts, self.tower_address = {}, {}, {}, {}
        rows = world['rows']
        # rows and cells shared by the template, the terrain map and the finder
        index, bases = 0, []
        o.word(INFO, 1000)  # reference count of the shared row info
        o.word(INFO + 8, RECORDS)
        for x, (start, end) in enumerate(rows):
            o.word(RECORDS + 12 * x, index - start)
            o.word(RECORDS + 12 * x + 4, start)
            o.word(RECORDS + 12 * x + 8, end)
            bases.append(index - start)
            index += end - start
        self.count = index
        self.item = {TEMPLATE: 28, TERRAIN: 4}
        records = bytearray(28 * index)
        masks = bytearray(4 * index)
        for (x, y), flags in world['template'].items():
            struct.pack_into('<I', records, 28 * (bases[x] + y) + 8, flags)
        for (x, y), mask in world['masks'].items():
            struct.pack_into('<I', masks, 4 * (bases[x] + y), mask)
        uc.mem_write(CELLS, bytes(records))
        uc.mem_write(MASKS, bytes(masks))
        self.grid(TEMPLATE, CELLS)
        self.grid(TERRAIN, MASKS)

        uc.mem_write(FIELD, bytes(0x800))
        o.word(FIELD + 0x64, world['gate'][3])
        # the mover
        m = world['mover']
        table, effects_vt, vt, base_vt = self.creature(MOVER, 1, world['size'], m['position'], m['effects'], m['tower'])
        # in the object list the creature is alive and not fixed scenery; it is skipped as itself
        o.word(vt + 0x10, self.constant[1])
        o.word(base_vt + 0x10, self.constant[0])
        o.word(table + 0x68, o.lookup(2, m['abilities']))
        o.word(effects_vt + 4, o.lookup(3, [1] if m['defender'] else []))
        self.kind[MOVER] = 'm'
        self.base_of[MOVER + 8] = MOVER
        if m['tower']:
            self.tower_address[MOVER] = self.tower(0, *m['tower'])
        # the battlefield's object list
        objects = []
        for k, item in enumerate(world['objects']):
            if item['kind'] == 'm':
                objects.append(MOVER)
                continue
            obj = OBJECTS + 0x1000 * k
            _, _, vt, base_vt = self.creature(obj, 10 + k, item['size'], item['position'], item['effects'],
                                              item.get('tower'))
            o.word(vt + 0x10, self.constant[item['alive']])
            o.word(base_vt + 0x10, self.constant[item['fixed']])
            o.word(base_vt + 4, self.constant[item['skip']])
            o.word(obj + 0xa0, obj + 0xc00)
            o.word(obj + 0xc00 + 0x88, TYPES.get(item['kind'], 5))
            self.kind[obj] = item['kind']
            self.base_of[obj + 8] = obj
            self.facts[obj] = (item['hostile'], item['coward'])
            if item.get('tower'):
                self.tower_address[obj] = self.tower(1 + k, *item['tower'])
            if item['kind'] == 't' and item.get('occupant'):
                # The creature standing at the tower's center: a separate fixture object.
                occupant = OBJECTS + 0x1000 * (k + 16)
                hostile, coward, effects = item['occupant']
                self.creature(occupant, 40 + k, 1, (0, 0), effects, None)
                self.kind[occupant] = 'c'
                self.facts[occupant] = (hostile, coward)
                self.occupants[self.center(item)] = occupant
            objects.append(obj)
        # The exe asks for the creature in the tower's center cell, not for the tower's
        # own creature: towers sharing a center see the same one (the last placed). Record
        # what each tower actually sees.
        seen = {}
        for item in world['objects']:
            if item['kind'] == 't' and item.get('occupant'):
                seen[self.center(item)] = item['occupant']
        for item in world['objects']:
            if item['kind'] == 't':
                item['occupant'] = seen.get(self.center(item))
        head = NODES
        o.word(FIELD + 0x448 + 4, head)
        ring = [head] + [NODES + 0x10 * (i + 1) for i in range(len(objects))]
        for i, node in enumerate(ring):
            o.word(node, ring[(i + 1) % len(ring)])
            o.word(node + 4, ring[i - 1])
        for node, obj in zip(ring[1:], objects):
            o.word(node + 8, obj)
        # the path finder
        uc.mem_write(FINDER, bytes(0x100))
        o.word(FINDER, VTABLE)
        o.word(FINDER + 0x4c, MOVER)
        uc.mem_write(FINDER + 0x50, bytes((int(m['teleports']), int(world['attack_mode']))))
        self.error = None
        try:
            o.call(0x20c7f0, (), this=FINDER)
        except Exception:
            if self.error:
                raise self.error
            raise

        # results: the finder's own records and queue
        cells = o.read_word(FINDER + 0x3c)
        after = bytes(uc.mem_read(cells, 28 * index))
        out = []
        k = 0
        for x, (start, end) in enumerate(rows):
            for y in range(start, end):
                record = after[28 * k:28 * k + 28]
                # Most cells are the template's flags with entry cost 100; list the others.
                plain = struct.pack('<HHIIiiii', 100, 0, 0, self.world['template'].get((x, y), 0), 0, 0, 0, 0)
                if record != plain:
                    out.append(self.record(x, y, record))
                k += 1
        begin, end = o.read_word(FINDER + 0xc), o.read_word(FINDER + 0x10)
        queue = [self.record(*struct.unpack_from('<ii', bytes(uc.mem_read(p + 28, 8))), bytes(uc.mem_read(p, 28)))
                 for p in range(begin, end, 36)] if begin else []
        return ';'.join(out) or '-', ';'.join(queue) or '-', ','.join(self.calls) or '-'

    @staticmethod
    def center(item):
        size = item['size']
        return ((item['position'][0] >> 4) + (size >> 1), (item['position'][1] >> 4) + (size >> 1))

    def record(self, x, y, record):
        enter, over, cost, flags, fx, fy, ox, oy = struct.unpack('<HHIIiiii', record)
        # Seeds are built in locals that are never fully initialized: the start's bits
        # 13 and up (20e3f0) and a tower exit's bits 8-12 and 18 and up (20c400) are
        # leftovers of earlier calls. Keep only the bits the exe sets.
        if fx == -1 and fy == -1 and cost == 0 and not self.world['mover']['tower']:
            flags &= 0x1fff
        elif self.world['mover']['tower'] and cost == 400 and (fx, fy) == self.mover_cell():
            flags &= 0x3e0ff
        return '{}:{}:{}:{}:{}:{:x}:{}:{}:{}:{}'.format(x, y, enter, over, cost, flags, fx, fy, ox, oy)

    def mover_cell(self):
        p = self.world['mover']['position']
        return (p[0] >> 4, p[1] >> 4)


def fits(rows, size, x, y):
    for dx in range(size):
        if x + dx >= len(rows):
            return False
        start, end = rows[x + dx]
        if y < start or y + size > end:
            return False
    return True


def random_world(rng):
    height = rng.randint(4, 14)
    rows = []
    for x in range(height):
        if rng.random() < 0.6:
            inset = int(abs(x - height / 2) * rng.choice([0.5, 1]))
            rows.append((inset, max(inset + 1, rng.randint(10, 14) - inset)))
        else:
            start = rng.choice([0, 0, 1, 2])
            rows.append((start, start + rng.randint(4, 14)))
    size = rng.choice([1, 1, 2, 2, 3, 3, 4, 5, 6, 7])
    template = {}
    cells = []
    for x, (start, end) in enumerate(rows):
        for y in range(start, end):
            if not fits(rows, size, x, y):
                template[(x, y)] = 0x14
                continue
            cells.append((x, y))
            if rng.random() < 0.15:
                template[(x, y)] = rng.choice([0x04, 0x0c, 0x20, 0x40, 0x44, 0x24, 0x08])
    if not cells:
        return None

    def position(shift=True):
        # Objects may stick out of the field; the moving creature always stands on it.
        x, y = rng.choice(cells)
        off = 16 if shift and rng.random() < 0.1 else 0
        return (16 * x + rng.randrange(16) - off, 16 * y + rng.randrange(16), rng.randrange(-2, 3))

    def effects():
        return [e for e in (0x89,) if rng.random() < 0.2]

    def tower_place():
        return (rng.choice([1, 2, 3, 4]), position())

    mover_tower = tower_place() if rng.random() < 0.25 else None
    mover = {'position': position(shift=False), 'tower': mover_tower,
             'effects': [e for e, p in ((7, 0.1), (0x2e, 0.1)) if rng.random() < p],
             'abilities': [0x19] if rng.random() < 0.25 else [], 'defender': int(rng.random() < 0.5),
             'teleports': int(rng.random() < 0.15)}
    objects = []
    for _ in range(rng.randint(0, 10)):
        kind = rng.choice('ccccooggttwwn')
        item = {'kind': kind, 'alive': int(rng.random() < 0.9), 'fixed': int(rng.random() < 0.25),
                'skip': int(rng.random() < 0.3), 'size': rng.choice([0, 1, 1, 2, 3, 4, 5]),
                'position': position(), 'effects': effects(),
                'hostile': int(rng.random() < 0.6), 'coward': int(rng.random() < 0.2)}
        if kind == 'c' and rng.random() < 0.15:
            item['tower'] = tower_place()
        if kind == 't' and rng.random() < 0.6:
            item['occupant'] = (int(rng.random() < 0.6), int(rng.random() < 0.2), effects())
        objects.append(item)
    if rng.random() < 0.3:
        objects.insert(rng.randrange(len(objects) + 1), {'kind': 'm'})
    masks = {}
    for (x, y) in cells:
        if rng.random() < 0.3:
            masks[(x, y)] = rng.choice([1, 2, 4, 8, 0x10, 3, 0x12])
    costs = [(rng.choice([1, 2, 4, 8, 0x20, 6]), rng.choice([0, 50, 150, 200, 300, 1000])) for _ in range(rng.choice([0, 0, 1, 2, 3, 4]))]
    gate = (int(rng.random() < 0.4), int(rng.random() < 0.3), int(rng.random() < 0.3), rng.randrange(height + 2))
    return {'rows': rows, 'size': size, 'template': template, 'mover': mover, 'objects': objects,
            'masks': masks, 'costs': costs, 'gate': gate, 'attack_mode': int(rng.random() < 0.3)}


def encode(world):
    def nums(items):
        return ','.join(map(str, items)) or '-'
    m = world['mover']
    tower = '-' if not m['tower'] else '{}/{}/{}/{}'.format(m['tower'][0], *m['tower'][1])
    mover = '{}/{}/{}:{}:{}:{}:{}:{}:{}'.format(*m['position'], tower, nums(m['effects']), nums(m['abilities']),
                                                m['defender'], m['teleports'], world['attack_mode'])
    objects = []
    for item in world['objects']:
        if item['kind'] == 'm':
            objects.append('m')
            continue
        tower = '-' if not item.get('tower') else '{}/{}/{}/{}'.format(item['tower'][0], *item['tower'][1])
        occupant = '-' if not item.get('occupant') else '{}/{}/{}'.format(item['occupant'][0], item['occupant'][1], nums(item['occupant'][2]))
        objects.append('{}:{}:{}:{}:{}:{}/{}/{}:{}:{}:{}:{}:{}'.format(
            item['kind'], item['alive'], item['fixed'], item['skip'], item['size'], *item['position'],
            nums(item['effects']), item['hostile'], item['coward'], tower, occupant))
    return ' '.join([
        ';'.join('{}:{}'.format(*r) for r in world['rows']), str(world['size']),
        ';'.join('{}:{}:{:x}'.format(x, y, f) for (x, y), f in sorted(world['template'].items())) or '-',
        mover, ';'.join(objects) or '-',
        ';'.join('{}:{}:{:x}'.format(x, y, v) for (x, y), v in sorted(world['masks'].items())) or '-',
        ';'.join('{:x}:{}'.format(*c) for c in world['costs']) or '-',
        ':'.join(map(str, world['gate']))])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--count', type=int, default=800)
    args = parser.parse_args()
    bench = Bench(args.exe)
    rng = random.Random(6200)
    lines = ['# Buka 3.0 SHA-256 ' + reference.SHA256,
             '# R rows size template mover objects masks costs gate -> cells queue calls',
             '# template: x:y:flags hex; mover: x/y/z subcells:tower:effects:abilities:defender:teleports:attack mode',
             '#   (tower = size/x/y/z or -); objects: m for the mover itself, or',
             '#   kind:alive:fixed:skip:size:x/y/z:effects:hostile:coward:tower:occupant',
             '#   kind c creature, o obstacle, g castle_gate, t castle_tower, w castle_wall, n other object;',
             '#   occupant of a tower: hostile/coward/effects; masks: x:y:terrain bits hex; costs: mask hex:cost;',
             '#   gate: has gate:state 177990:state 1778c0:gate x (+0x64);',
             '# cells: every record other than (entry 100, template flags, zeros) x:y:entry:over:cost:flags hex:from x:from y:origin x:origin y;',
             '# queue: the same per queued point in vector order; calls: costs (225320), open1/open0 (1779c0)']
    made = 0
    while made < args.count:
        world = random_world(rng)
        if world is None:
            continue
        cells, queue, calls = bench.run(world)
        lines.append('R {} -> {} {} {}'.format(encode(world), cells, queue, calls))
        made += 1
    # The direction from one cell to another (51ede0: x87 atan2, eight 45-degree sectors),
    # exhaustively over offsets -20..20; a tower exit records it.
    lines.append('# D dx dy -> direction of (dx, dy) seen from (0, 0)')
    origin, target = HEAP + 0xff00, HEAP + 0xff10
    bench.o.word(origin, 0)
    bench.o.word(origin + 4, 0)
    for dx in range(-20, 21):
        for dy in range(-20, 21):
            bench.o.word(target, dx)
            bench.o.word(target + 4, dy)
            direction = int(bench.o.call(0x11ede0, (origin, target), callee_pop=False))
            lines.append('D {} {} -> {}'.format(dx, dy, direction))
    args.out.write_text('\n'.join(lines) + '\n')
    print('{} original-exe path preparation fixtures'.format(made))


if __name__ == '__main__':
    main()
