"""Decode HoMM4 paletted sprite layers (t_bitmap_layer_24) to PNG.

usage: h4_sprite.py layers <file.h4d> <out dir>        bitmap group: u16 count, layers
       h4_sprite.py sequence <file.h4d> <out dir>      actor_sequence: u16 ?, u16 ?, u16 count, layers, tail

t_bitmap_layer_24 (reader at rva 195570 in rus-3.0):
  u16 palette size, u16 has_block_alpha, palette BGR * size
  u16-prefixed name, u8 alpha depth, i32 left, top, right, bottom
  rows * (u16 left, u16 right, u32 offset)     rows = bottom - top, x relative to left
  pixels: u8 palette index, N = last row's offset + width
  if alpha depth >= 4: 4-bit alpha, (N + 1) >> 1 bytes
      if has_block_alpha: (N + 131) >> 7 more bytes (per-128-pixel flags, unused here)
"""
import os
import struct
import sys
import zlib


class Reader:
    def __init__(self, d, o=0):
        self.d, self.o = d, o

    def take(self, n):
        b = self.d[self.o:self.o + n]
        if len(b) != n:
            raise EOFError(f'need {n} bytes at {self.o:#x}')
        self.o += n
        return b

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack('<H', self.take(2))[0]

    def str16(self):
        return self.take(self.u16()).decode('cp1251', 'replace')


def read_layer(r):
    npal, block = r.u16(), r.u16()
    pal = r.take(3 * npal)
    name = r.str16()
    depth = r.u8()
    left, top, right, bottom = struct.unpack('<4i', r.take(16))
    h = bottom - top
    rows = [struct.unpack('<HHI', r.take(8)) for _ in range(max(h, 0))]
    n = rows[-1][2] + rows[-1][1] - rows[-1][0] if rows else 0
    pix = r.take(n)
    alpha = r.take((n + 1) >> 1) if depth >= 4 and n else b''
    if depth >= 4 and n and block:
        r.take((n + 131) >> 7)
    return dict(name=name, depth=depth, rect=(left, top, right, bottom), rows=rows,
                pal=pal, pix=pix, alpha=alpha)


def to_rgba(layer, canvas=None):
    """RGBA rows for the layer's rect, or for `canvas` = (left, top, right, bottom)."""
    cl, ct, cr, cb = canvas or layer['rect']
    w, h = cr - cl, cb - ct
    out = [bytearray(4 * w) for _ in range(h)]
    l, t = layer['rect'][:2]
    pal, pix, alpha = layer['pal'], layer['pix'], layer['alpha']
    for y, (x0, x1, off) in enumerate(layer['rows']):
        row = out[t + y - ct]
        for i in range(x1 - x0):
            k = off + i
            c = pix[k]
            if alpha:
                a = (alpha[k >> 1] >> (4 * (k & 1))) & 15
                a = a * 17
            else:
                a = 255
            x = l + x0 + i - cl
            row[4 * x:4 * x + 4] = bytes((pal[3 * c + 2], pal[3 * c + 1], pal[3 * c], a))
    return out


def write_png(path, rows):
    h, w = len(rows), len(rows[0]) // 4 if rows else 0
    raw = b''.join(b'\0' + bytes(r) for r in rows)

    def chunk(t, data):
        return struct.pack('>I', len(data)) + t + data + struct.pack('>I', zlib.crc32(t + data))
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
                + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def main():
    kind, src, out = sys.argv[1:4]
    d = open(src, 'rb').read()
    r = Reader(d)
    if kind == 'sequence':
        head = [r.u16(), r.u16()]
        count = r.u16()
    else:
        head, count = [], r.u16()
    layers = [read_layer(r) for _ in range(count)]
    print(f'header {head}, {count} layers, {len(d) - r.o} bytes left: {d[r.o:r.o + 32].hex()}')
    os.makedirs(out, exist_ok=True)
    rects = [l['rect'] for l in layers if l['rows']]
    canvas = (min(x[0] for x in rects), min(x[1] for x in rects),
              max(x[2] for x in rects), max(x[3] for x in rects)) if kind == 'sequence' else None
    base = os.path.splitext(os.path.basename(src))[0]
    for i, l in enumerate(layers):
        print(f"  {i:2d} {l['name']!r} depth {l['depth']} rect {l['rect']}")
        if l['rows']:
            write_png(os.path.join(out, f'{base}.{i:02d}.png'), to_rgba(l, canvas))


if __name__ == '__main__':
    main()
