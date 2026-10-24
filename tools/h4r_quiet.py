"""Build an .h4r with attenuated copies of loud PCM sounds.

usage: h4r_quiet.py <src.h4r> <out.h4r> <name prefix> <target rms dBFS>
"""
import array
import gzip
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h4r import read_dir, unpack  # noqa: E402

HDR = 19  # per-resource sound header: format, bits, channels, rate, length...


def rms_db(samples):
    if not samples:
        return -200.0
    r = math.sqrt(sum(v * v for v in samples) / len(samples))
    return 20 * math.log10(max(r, 1) / 32768)


def main():
    src, out, prefix, target = sys.argv[1], sys.argv[2], sys.argv[3].encode(), float(sys.argv[4])
    d = open(src, 'rb').read()
    picked = []
    for e in read_dir(d):
        if not e['name'].lower().startswith(prefix.lower()) or e['alias'] or not e['csz']:
            continue
        data = unpack(d, e)
        if data[0] != 0:  # 0 = PCM, 1 = MP3
            continue
        bits, pcm = data[3], data[HDR:]
        if bits == 16:
            s = array.array('h', pcm[:len(pcm) // 2 * 2])
            vals = list(s)
        elif bits == 8:
            vals = [(b - 128) * 256 for b in pcm]
        else:
            continue
        level = rms_db(vals)
        if level <= target:
            continue
        g = 10 ** ((target - level) / 20)
        if bits == 16:
            s = array.array('h', (int(round(v * g)) for v in s))
            new_pcm = s.tobytes() + pcm[len(pcm) // 2 * 2:]
        else:
            new_pcm = bytes(max(0, min(255, int(round((b - 128) * g)) + 128)) for b in pcm)
        blob = gzip.compress(data[:HDR] + new_pcm, compresslevel=9, mtime=0)
        blob = blob[:8] + b'\x00\x0b' + blob[10:]  # same XFL/OS bytes as the game's own entries
        picked.append((e, blob))
        print(f"{e['name'].decode('latin1'):50s} {level:6.1f} -> {target:.1f} dBFS")

    body = bytearray(b'H4R\x05' + b'\0\0\0\0')
    offsets = []
    for e, blob in picked:
        offsets.append(len(body))
        body += blob
    struct.pack_into('<I', body, 4, len(body))
    body += struct.pack('<I', len(picked))
    for (e, blob), off in zip(picked, offsets):
        body += struct.pack('<IIII', off, len(blob), e['sz'], e['tm'])
        for s in (e['name'], e['path'], e['alias']):
            body += struct.pack('<H', len(s)) + s
        body += struct.pack('<I', e['typ'])
    open(out, 'wb').write(body)
    print(f'{len(picked)} sounds -> {out} ({len(body)} bytes)')


if __name__ == '__main__':
    main()
