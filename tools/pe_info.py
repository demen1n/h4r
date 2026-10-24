"""Dump what a MSVC-built PE gives away without a disassembler.

usage: pe_info.py <heroes4.exe> <out dir>

Writes rtti_types.txt (mangled type names), classes.txt (distinct base class names),
source_files.txt (E:\\HMM4\\SOURCE\\... paths leaked via anonymous namespaces and asserts),
imports.txt; prints sections and a rough function count (distinct E8 call targets).
"""
import datetime
import os
import re
import struct
import sys


def sections(d):
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    nsec = struct.unpack_from('<H', d, pe + 6)[0]
    osz = struct.unpack_from('<H', d, pe + 20)[0]
    opt = pe + 24
    secs = []
    for i in range(nsec):
        o = opt + osz + 40 * i
        vs, va, rs, ro = struct.unpack_from('<IIII', d, o + 8)
        secs.append((d[o:o + 8].rstrip(b'\0').decode(), va, vs, rs, ro))
    return pe, opt, secs


def imports(d, opt, secs):
    def r2o(rva):
        for _, va, vs, _, ro in secs:
            if va <= rva < va + vs:
                return rva - va + ro

    def cstr(o):
        return d[o:d.index(b'\0', o)].decode()

    o = r2o(struct.unpack_from('<I', d, opt + 96 + 8)[0])
    while True:
        oft, _, _, name, ft = struct.unpack_from('<IIIII', d, o)
        if not name:
            break
        t, fs = r2o(oft or ft), []
        while True:
            v = struct.unpack_from('<I', d, t)[0]
            if not v:
                break
            fs.append('#%d' % (v & 0xffff) if v & 0x80000000 else cstr(r2o(v) + 2))
            t += 4
        yield cstr(r2o(name)), fs
        o += 20


def main():
    src, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    d = open(src, 'rb').read()
    pe, opt, secs = sections(d)
    ts = struct.unpack_from('<I', d, pe + 8)[0]
    print(f'linker {d[opt + 2]}.{d[opt + 3]:02d}, built {datetime.datetime.fromtimestamp(ts, datetime.timezone.utc):%Y-%m-%d %H:%M} UTC')
    for name, va, vs, rs, ro in secs:
        print(f'{name:8s} va {va:#08x} vsize {vs:9d} raw {rs:9d}')

    _, va, vs, _, ro = secs[0]
    text = d[ro:ro + vs]
    targets = set()
    for m in re.finditer(rb'\xe8', text):
        i = m.start()
        if i + 5 <= len(text):
            t = i + 5 + struct.unpack_from('<i', text, i + 1)[0]
            if 0 <= t < len(text):
                targets.add(t)
    print(f'~{len(targets)} functions (distinct call targets)')

    rtti = sorted(set(m.decode('latin1') for m in re.findall(rb'\.\?A[VU]([A-Za-z0-9_@$?<>,%:\\.]+)@@', d)))
    classes = sorted(set(n.split('@')[0] for n in rtti))
    srcs = sorted(set(m.decode('latin1') for m in re.findall(rb'SOURCE\\([A-Za-z0-9_\\]+\.(?:cpp|h))', d)))
    with open(os.path.join(out, 'rtti_types.txt'), 'w') as f:
        f.write('\n'.join(rtti) + '\n')
    with open(os.path.join(out, 'classes.txt'), 'w') as f:
        f.write('\n'.join(classes) + '\n')
    with open(os.path.join(out, 'source_files.txt'), 'w') as f:
        f.write('\n'.join(srcs) + '\n')
    with open(os.path.join(out, 'imports.txt'), 'w') as f:
        for dll, fs in imports(d, opt, secs):
            f.write(f'{dll} ({len(fs)}): {" ".join(fs)}\n')
    print(f'{len(rtti)} rtti types, {len(classes)} classes, {len(srcs)} source files -> {out}')


if __name__ == '__main__':
    main()
