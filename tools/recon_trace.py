#!/usr/bin/env python3
"""A trace of the recompiled game's save stream (its BUSTIN_RECON_LOG): which function
streamed what, in order.

Usage: recon_trace.py LOG SYMBOLS [--buffer N] [--limit N]

SYMBOLS is the recomp's analysis/symbols.txt, to name the caller of each call. Without
--buffer it lists the buffers (each stream the game read or wrote) with their call counts;
with it, that buffer's calls in order: the caller, the kind, the byte position, the values.
"""
import sys, struct, bisect, re, collections

KINDS = {1: 'r8', 2: 'bool', 3: 'r16', 4: 'int', 5: 'r32', 6: 'float', 7: 'mark', 8: 'to-mark',
         9: 'str1', 10: 'str2', 11: 'str3', 12: 'str4'}


def records(path):
    b = open(path, 'rb').read()
    p = 0
    while p + 19 <= len(b):
        kind, lr, buf, before, after, n = struct.unpack_from('<BIIIIH', b, p)
        p += 19
        vals = struct.unpack_from(f'<{n}i', b, p)
        p += 4 * n
        yield dict(kind=kind, lr=lr, buf=buf, before=before, after=after, values=vals)


def symbols(path):
    out = []
    for line in open(path):
        m = re.match(r'(\S+) = \.text:0x([0-9A-Fa-f]+);', line)
        if m: out.append((int(m.group(2), 16), m.group(1)))
    out.sort()
    return [a for a, n in out], [n for a, n in out]


def caller_name(syms, lr):
    addrs, names = syms
    i = bisect.bisect_right(addrs, lr) - 1
    return f'{names[i]}+{lr - addrs[i]:#x}' if i >= 0 else hex(lr)


def main(argv):
    log, syms = argv[1], symbols(argv[2])
    want = int(argv[argv.index('--buffer') + 1], 0) if '--buffer' in argv else None
    limit = int(argv[argv.index('--limit') + 1]) if '--limit' in argv else 400
    recs = list(records(log))
    if want is None:
        bufs = collections.Counter(r['buf'] for r in recs)
        first = {}
        for i, r in enumerate(recs): first.setdefault(r['buf'], i)
        for buf, n in sorted(bufs.items(), key=lambda kv: first[kv[0]]):
            fns = collections.Counter(caller_name(syms, r['lr']).split('+')[0] for r in recs if r['buf'] == buf)
            print(f'buffer {buf:#x}: {n} calls, from {fns.most_common(4)}')
        return
    shown = 0
    for r in recs:
        if r['buf'] != want: continue
        print(f"{r['before']:6} {KINDS.get(r['kind'], r['kind']):7} {caller_name(syms, r['lr']):42} {list(r['values'])[:12]}")
        shown += 1
        if shown >= limit: break


if __name__ == '__main__':
    main(sys.argv)
