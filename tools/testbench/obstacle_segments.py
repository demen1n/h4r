#!/usr/bin/env python3
"""Record full native RVA 238690 segmentation at scale 1 using synthetic layers."""
import argparse
import importlib.util
import itertools
import struct
from pathlib import Path

spec = importlib.util.spec_from_file_location('h4_reference', Path(__file__).with_name('compare.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def pixels(mode, width=96, height=64):
    # Deliberately includes fully transparent rows, holes and isolated pixels.
    return bytes((1 + (x + y) % 2) if (
        mode == 0 or (mode == 1 and (x + 3*y) % 11 < 4)
        or (mode == 2 and 32 <= x < 48 and 24 <= y < 32)
        or (mode == 3 and (x, y) in ((0, 0), (95, 63), (40, 32)))
    ) else 0 for y in range(height) for x in range(width))


def signature(original, segment):
    u = original.uc
    read = original.read_word
    size, x, y = (read(segment + i) for i in (0x90, 0x94, 0x98))
    ox, oy = struct.unpack('<2i', u.mem_read(segment + 0x2c, 8))
    parts = [str(v) for v in (size, x//16, y//16, ox, oy)]
    for p in range(read(segment + 0x64), read(segment + 0x68), 12):
        layer = read(p + 8)
        left, top, right, bottom = struct.unpack('<4i', u.mem_read(layer + 0x18, 16))
        digest = 0xcbf29ce484222325
        for yy in range(max(0, bottom - top)):
            a, b, offset = struct.unpack('<HHI', u.mem_read(read(layer + 0x30) + yy*8, 8))
            for xx in range(max(0, right-left)):
                index = alpha = 0
                if a <= xx < b:
                    k = offset + xx - a
                    index = u.mem_read(read(layer + 0x54) + k, 1)[0]
                    if read(layer + 0x14) >= 4:
                        alpha = (u.mem_read(read(layer + 0x3c) + k//2, 1)[0] >> (4*(k&1))) & 15
                    else:
                        alpha = 0 if read(layer + 0x14) and index == 0 else 15
                # Transparent colours have no rendered meaning.
                for value in (index if alpha else 0, alpha):
                    digest = ((digest ^ value) * 0x100000001b3) & 0xffffffffffffffff
        parts.extend(map(str, (left, top, right, bottom)))
        parts.append(f'{digest:016x}')
    return ','.join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    o = reference.Original(args.exe)
    u, heap = o.uc, reference.HEAP
    root, layer, out = heap+0x4000, heap+0x5000, heap+0x6000
    rows, data, alpha = heap+0x7000, heap+0x8000, heap+0xa000
    results = ['# Full native 238690, scale 1.0, SHA-256 ' + reference.SHA256,
               '# width height attr flag type elevation left top offset-x offset-y depth pattern layers -> segments(size,x,y,ox,oy,rect,coverage-fnv64)']
    cases = itertools.product(((2,4), (4,2), (3,5), (2,2), (0,3)), (0,1), (1,4), range(5), (1,2))
    for number, ((w,h), attr, depth, mode, count) in enumerate(cases):
        flag = int(number % 7 == 0)
        kind = (0, 0x23, 0x25)[number % 3]
        elevation = (0, 7, 23)[number % 3]
        left, top = (-48, -24) if number % 2 else (0, 0)
        ox, oy = ((13, -9) if number % 3 else (0, 0))
        o.arena_next = reference.ARENA
        u.mem_write(root, bytes(0x100)); u.mem_write(layer, bytes(0xd0)); u.mem_write(out, bytes(0x20))
        for address, value in ((root+4, attr), (root+8, elevation), (root+0x1c, flag),
            (root+0x18, kind), (root+0x20,w), (root+0x24,h), (root+0x4c,ox), (root+0x50,oy),
            (root+0x80,layer), (root+0x84,layer+count*0x68), (root+0x88,layer+count*0x68)):
            o.word(address,value)
        for index in range(count):
            item = layer + index*0x68
            row_data, pixel_data, alpha_data = (rows, data, alpha) if index == 0 else (heap+0xb000, heap+0xc000, heap+0xe000)
            dx,dy = -9*index,7*index
            for address,value in (
            (item+0x14,depth), (item+0x18,left+dx), (item+0x1c,top+dy), (item+0x20,left+dx+96), (item+0x24,top+dy+64)):
                o.word(address,value)
            u.mem_write(row_data,b''.join(struct.pack('<HHI',0,96,y*96) for y in range(64)))
            values = pixels((mode+2*index)%5)
            u.mem_write(pixel_data,values)
            for off, pointer in ((0x28,row_data),(0x4c,pixel_data)):
                o.word(item+off,item+off); o.word(item+off+4,item+off); o.word(item+off+8,pointer)
            if depth == 4:
                # A nonzero palette index can still be transparent.
                alphas = bytes(0 if (k//96 + k%96)%7 == 0 else (v*5) for k,v in enumerate(values))
                u.mem_write(alpha_data,bytes(alphas[k] | (alphas[k+1]<<4) for k in range(0,len(alphas),2)))
                for off,value in ((0x34,item+0x34),(0x38,item+0x34),(0x3c,alpha_data)):o.word(item+off,value)
        o.call(0x238690,(root,0,0x3ff00000),this=out)
        segments = [signature(o,o.read_word(p)) for p in range(o.read_word(out+12),o.read_word(out+16),4)]
        inputs = (w,h,attr,flag,kind,elevation,left,top,ox,oy,depth,mode,count)
        results.append(' '.join(map(str,inputs))+' -> '+'/'.join(segments))
    args.output.write_text('\n'.join(results)+'\n')
    print(f'{len(results)-2} full native segmentation cases')


if __name__ == '__main__':
    main()
