"""Convert table.*.h4d (spreadsheet-like game data) to TSV.

usage: h4d_table.py <table.h4d> [out.tsv]

Format: u32 row count; each row is a u16 cell count followed by that many u16-prefixed
cp1251 strings (rows can differ in length). The first rows are headers in merged-cell style,
so many cells are empty.
strings.*.h4d use a different layout and are not handled here.
"""
import struct
import sys


def read_table(d):
    rows = struct.unpack_from('<I', d, 0)[0]
    o = 4
    out = []
    for _ in range(rows):
        cols = struct.unpack_from('<H', d, o)[0]
        o += 2
        row = []
        for _ in range(cols):
            l = struct.unpack_from('<H', d, o)[0]
            o += 2
            row.append(d[o:o + l].decode('cp1251', 'replace'))
            o += l
        out.append(row)
    return out


def main():
    rows = read_table(open(sys.argv[1], 'rb').read())
    text = '\n'.join('\t'.join(c.replace('\t', ' ').replace('\r', '').replace('\n', '\\n') for c in r) for r in rows) + '\n'
    if len(sys.argv) > 2:
        open(sys.argv[2], 'w', encoding='utf-8').write(text)
        print(f'{len(rows)} rows -> {sys.argv[2]}')
    else:
        sys.stdout.write(text)


if __name__ == '__main__':
    main()
