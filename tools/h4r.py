"""List or extract .h4r archives (Heroes of Might and Magic IV resource packs).

usage: h4r.py list <file.h4r> [prefix]
       h4r.py extract <file.h4r> <out dir> [prefix]

Format: "H4R\\x05", u32 directory offset, gzip blobs. Directory: u32 count, then per entry
u32 offset, compressed size, size, time; three u16-prefixed strings (name, source path, alias);
u32 type. Entries with an alias have no data of their own.
"""
import os
import struct
import sys
import zlib


def read_dir(d):
    do = struct.unpack_from('<I', d, 4)[0]
    n = struct.unpack_from('<I', d, do)[0]
    o = do + 4
    for _ in range(n):
        off, csz, sz, tm = struct.unpack_from('<IIII', d, o)
        o += 16
        strs = []
        for _ in range(3):
            l = struct.unpack_from('<H', d, o)[0]
            o += 2
            strs.append(d[o:o + l])
            o += l
        typ = struct.unpack_from('<I', d, o)[0]
        o += 4
        yield dict(off=off, csz=csz, sz=sz, tm=tm, name=strs[0], path=strs[1], alias=strs[2], typ=typ)


def unpack(d, e):
    return zlib.decompressobj(16 + zlib.MAX_WBITS).decompress(d[e['off']:e['off'] + e['csz']])


def main():
    cmd, src = sys.argv[1], sys.argv[2]
    d = open(src, 'rb').read()
    if cmd == 'list':
        prefix = sys.argv[3].lower() if len(sys.argv) > 3 else ''
        for e in read_dir(d):
            n = e['name'].decode('latin1')
            if n.lower().startswith(prefix):
                alias = ' -> ' + e['alias'].decode('latin1') if e['alias'] else ''
                print(f"{e['sz']:10d} {e['typ']:3d} {n}{alias}")
    elif cmd == 'extract':
        out = sys.argv[3]
        prefix = sys.argv[4].lower() if len(sys.argv) > 4 else ''
        os.makedirs(out, exist_ok=True)
        n = 0
        for e in read_dir(d):
            name = e['name'].decode('latin1')
            if not name.lower().startswith(prefix) or e['alias'] or not e['csz']:
                continue
            with open(os.path.join(out, name.replace('/', '_').replace('\\', '_')), 'wb') as f:
                f.write(unpack(d, e))
            n += 1
        print(f'{n} files -> {out}')


if __name__ == '__main__':
    main()
