"""Carry homm4-symbols names from a sibling build (complete-3.0) onto our exe by function shape.

usage: transfer_names.py <symbols repo> <source target> <our target>

Our exe is the same 3.0WoW code relinked: functions keep their order, with small shifts that
drift across the file. Matching uses only what both sides have: size, instruction count and
`ret N` (the source side's count and cleanup come from its evidence column).

1. anchors: source functions of >= 48 bytes with exactly one same-shape function in ours
   within +-32K; the longest chain increasing on both sides is kept.
2. bands: every other source function is looked up between its neighbouring anchors and
   accepted only if exactly one same-shape function sits there.
Vftables are paired by their map name from RTTI (exact).

Writes maps/<our>/names.tsv in the repo's format (tier kept from the source, method prefixed
with "shape-anchor" / "shape-band"), ready for tools/ghidra/ApplyNames.java.
"""
import bisect
import csv
import os
import re
import sys

csv.field_size_limit(1 << 30)
MIN_ANCHOR = 48
WINDOW = 0x8000


def tsv(path):
    with open(path, newline='') as f:
        return list(csv.DictReader(f, delimiter='\t', quoting=csv.QUOTE_NONE))


def lis(pairs):
    """Longest chain of (a, b) pairs strictly increasing in both; pairs sorted by a."""
    tails, tail_idx, prev = [], [], [-1] * len(pairs)
    for i, (_, b) in enumerate(pairs):
        k = bisect.bisect_left(tails, b)
        if k == len(tails):
            tails.append(b)
            tail_idx.append(i)
        else:
            tails[k] = b
            tail_idx[k] = i
        prev[i] = tail_idx[k - 1] if k else -1
    out, i = [], tail_idx[-1] if tail_idx else -1
    while i >= 0:
        out.append(pairs[i])
        i = prev[i]
    return out[::-1]


def main():
    repo, src_t, dst_t = sys.argv[1:4]
    src_rows = tsv(os.path.join(repo, 'maps', src_t, 'names.tsv'))
    feats = tsv(os.path.join(repo, 'work', dst_t, 'features', 'functions.tsv'))
    vfts = tsv(os.path.join(repo, 'work', dst_t, 'features', 'vftables.tsv'))

    ours = sorted((int(f['rva'], 16), int(f['size'], 16), int(f['ninsn']), int(f['retn'])) for f in feats)
    our_rva = [o[0] for o in ours]
    by_rva = {o[0]: o for o in ours}

    src = []
    for r in src_rows:
        if r['kind'] != 'func':
            continue
        m = re.search(r'instructions=\d+/(\d+)', r['evidence'])
        n = re.search(r'retn=(\d+)/(\d+)', r['evidence'])
        src.append((int(r['rva'], 16), int(r['size'], 16), int(m.group(1)) if m else None,
                    int(n.group(2)) if n else None, r))
    src.sort(key=lambda s: s[0])

    def same(s, o):
        return (o[1] == s[1] and (s[2] is None or o[2] == s[2])
                and (s[3] is None or o[3] == s[3]))

    def between(s, lo, hi):
        i, j = bisect.bisect_left(our_rva, lo), bisect.bisect_right(our_rva, hi)
        return [ours[k] for k in range(i, j) if same(s, ours[k])]

    cand = []
    for s in src:
        if s[1] < MIN_ANCHOR:
            continue
        c = between(s, s[0] - WINDOW, s[0] + WINDOW)
        if len(c) == 1:
            cand.append((s[0], c[0][0]))
    anchors = lis(cand)
    a_src = [a for a, _ in anchors]
    amap = dict(anchors)

    matched = {}
    for a, b in anchors:
        matched[a] = (b, 'shape-anchor')
    used = set(b for _, b in anchors)
    for s in src:
        if s[0] in matched:
            continue
        k = bisect.bisect_left(a_src, s[0])
        lo = amap[a_src[k - 1]] + 1 if k else 0
        hi = amap[a_src[k]] - 1 if k < len(a_src) else our_rva[-1]
        c = [o for o in between(s, lo, hi) if o[0] not in used]
        if len(c) == 1:
            matched[s[0]] = (c[0][0], 'shape-band')
            used.add(c[0][0])

    out = []
    for s in src:
        if s[0] not in matched:
            continue
        rva, how = matched[s[0]]
        r = dict(s[4])
        r['evidence'] = f"{how};{src_t}-rva={s[0]:x};" + r['evidence']
        r['method'] = f"{how}+{r['method']}"
        r['via'] = f'{s[0]:x}'
        r['rva'] = f'{rva:x}'
        out.append(r)

    vft_by_name = {}
    for v in vfts:
        vft_by_name.setdefault(v['map_name'], []).append(v['rva'])
    nv = 0
    for r in src_rows:
        if r['kind'] == 'vftable' and len(vft_by_name.get(r['name'], [])) == 1:
            r = dict(r)
            r['via'] = r['rva']
            r['rva'] = vft_by_name[r['name']][0]
            r['method'] = 'rtti-name+' + r['method']
            out.append(r)
            nv += 1

    out.sort(key=lambda r: int(r['rva'], 16))
    path = os.path.join(repo, 'maps', dst_t, 'names.tsv')
    with open(path, 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=list(src_rows[0].keys()), delimiter='\t',
                           quoting=csv.QUOTE_NONE, escapechar='\\', lineterminator='\n')
        w.writeheader()
        w.writerows(out)
    nf = len(out) - nv
    tiers = {}
    for r in out:
        if r['kind'] == 'func':
            tiers[r['tier']] = tiers.get(r['tier'], 0) + 1
    print(f'{len(src)} source functions: {len(anchors)} anchors, {nf - len(anchors)} band matches, '
          f'{len(src) - nf} unmatched; tiers {dict(sorted(tiers.items()))}; {nv} vftables -> {path}')


if __name__ == '__main__':
    main()
