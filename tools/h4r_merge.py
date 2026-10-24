"""Merge .h4r archives: entries of the first archive win over later ones with the same name.

usage: h4r_merge.py <out.h4r> <a.h4r> [b.h4r ...]
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h4r import read_dir  # noqa: E402


def main():
    out, srcs = sys.argv[1], sys.argv[2:]
    body = bytearray(b'H4R\x05' + b'\0\0\0\0')
    seen, entries = set(), []
    for src in srcs:
        d = open(src, 'rb').read()
        for e in read_dir(d):
            key = e['name'].lower()
            if key in seen:
                continue
            seen.add(key)
            blob = d[e['off']:e['off'] + e['csz']]
            entries.append(dict(e, off=len(body), csz=len(blob)))
            body += blob
    struct.pack_into('<I', body, 4, len(body))
    body += struct.pack('<I', len(entries))
    for e in entries:
        body += struct.pack('<IIII', e['off'], e['csz'], e['sz'], e['tm'])
        for s in (e['name'], e['path'], e['alias']):
            body += struct.pack('<H', len(s)) + s
        body += struct.pack('<I', e['typ'])
    open(out, 'wb').write(body)
    print(f'{len(entries)} entries -> {out} ({len(body)} bytes)')


if __name__ == '__main__':
    main()
