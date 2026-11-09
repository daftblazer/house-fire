#!/usr/bin/env python3
"""Which scripts read a local before writing it?

Usage: locals_check.py IMAGE [LOG]

The engine does not clear a new frame's locals (docs/interpreter.md, "The call"); the
native one starts them at zero. That only changes what a script does if some path through
it reads a local before writing it. For each tree this finds, by data flow over its nodes,
every read of a local that is not written on every path to it.

Reads and writes are taken from the primitives' operands, as measured against the
recompiled game (which primitives changed locals in a logged session, and where their
operands name the local):

  Expression (2): left side bytes 0-1 and scope byte 6; right side bytes 2-3 and scope
    byte 7; operator byte 5. Assignment (5) only writes the left side; comparisons only
    read it; the rest read and write it.
  Random (8): destination bytes 0-1, scope bytes 2-3; maximum bytes 4-5, scope byte 6.
  Relationship2 (26): scope byte 4, the local bytes 6-7 (written).
  SetToNext (31): with flag 0x80 in byte 4, scope byte 5, the local byte 6 (written).
  Animate (44): with flag 0x20 in byte 5, an event is stored in local 0 (written).

Other primitives are taken to neither read nor write locals; a report on a path through
one of the primitives not decoded here is marked uncertain. With LOG (a BUSTIN_TREE_LOG of
the recompiled game), each report the session ran is checked: did the original read a
left-over value there that was not zero?

Bustin' Out only for now (its operands are in the PC's little-endian order).
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
from check_chunks import iff_files, byte_order
import chunks, primitives

LOCAL = 25
ASSIGN = 5
COMPARE = {0, 1, 2, 8, 14, 15, 16}   # >, <, ==, flag set?, >=, <=, !=
DECODED = {0, 2, 8, 26, 31, 44}


def u16(b, i): return b[i] | b[i + 1] << 8


def effects(nd):
    """(reads, writes) of locals by one node, as sets of local numbers."""
    op, o = nd['opcode'], nd['operand']
    r, w = set(), set()
    if op == 2:
        oper = o[5]
        if o[6] == LOCAL:
            if oper != ASSIGN: r.add(u16(o, 0))
            if oper not in COMPARE: w.add(u16(o, 0))
        if o[7] == LOCAL: r.add(u16(o, 2))
    elif op == 8:
        if o[6] == LOCAL: r.add(u16(o, 4))
        if u16(o, 2) == LOCAL: w.add(u16(o, 0))
    elif op == 26:
        if o[4] == LOCAL: w.add(u16(o, 6))
    elif op == 31:
        if o[4] & 0x80 and o[5] == LOCAL: w.add(o[6])
    elif op == 44:
        if o[5] & 0x20: w.add(0)
    return r, w


def check_tree(b):
    """Reports: (node, local, uncertain) for each read not preceded by a write on all paths."""
    nodes = b['nodes']; n = len(nodes)
    if not n or not b['locals']: return []
    full = (1 << b['locals']) - 1
    eff = [effects(x) for x in nodes]
    starts = range(n) if b['type'] == 4 else [0]   # a type-4 tree starts where its parameter says
    IN = [full] * n
    UNK = [False] * n                              # an undecoded primitive on some path here
    for s in starts: IN[s] = 0
    work = list(starts)
    while work:
        i = work.pop()
        r, w = eff[i]
        out = IN[i]
        for k in w:
            if k < b['locals']: out |= 1 << k
        unk = UNK[i] or (nodes[i]['opcode'] < 256 and nodes[i]['opcode'] not in DECODED)
        for t in (nodes[i]['true'], nodes[i]['false']):
            if t >= n: continue
            new, newu = IN[t] & out, UNK[t] or unk
            if (new, newu) != (IN[t], UNK[t]):
                IN[t], UNK[t] = new, newu
                work.append(t)
    out = []
    for i in range(n):
        for k in eff[i][0]:
            if k >= b['locals'] or not IN[i] >> k & 1:
                out.append((i, k, UNK[i]))
    return out


def main(argv):
    d = Disc(argv[1])
    if d.id != 'G4ME69': raise SystemExit('Bustin\' Out only for now')
    seen = set(); reports = []; trees = with_locals = 0
    for name, data, cs in iff_files(d):
        end = byte_order(data, cs)
        for t, cid, fl, lab, o, sz in cs:
            if t != 'BHAV': continue
            key = data[o:o + sz]
            if key in seen: continue
            seen.add(key)
            try: b = chunks.bhav(key, end)
            except chunks.ChunkError: continue
            trees += 1
            with_locals += b['locals'] > 0
            for i, k, unk in check_tree(b):
                reports.append((name.split('/')[-1], cid & 0xFFFF, lab, i, k, unk, b['locals'],
                                tuple((x['opcode'], x['true'], x['false']) for x in b['nodes'])))
    sure = [r for r in reports if not r[5]]
    print(f'{trees} trees, {with_locals} with locals; reads of a local that may not be written yet: '
          f'{len(reports)} ({len(sure)} certain, {len(reports) - len(sure)} past an undecoded primitive)')
    by_tree = collections.Counter((r[0], r[1], r[2]) for r in sure)
    for (f, cid, lab), c in by_tree.most_common(25):
        print(f'  {f} {cid:#x} {lab!r}: {c}')

    if len(argv) > 2:
        from tree_log import records, mem_shape
        want = collections.defaultdict(set)
        for f, cid, lab, i, k, unk, nl, shape in reports:
            want[(cid, shape)].add((i, k))
        trees_mem = {}; ran = collections.Counter(); ex = {}
        for kind, v in records(argv[2]):
            if kind == 'T': trees_mem[v[0]] = (v[1], mem_shape(v[3]))
            elif kind == 'S' and v['look'] == ord('X'):
                t = trees_mem.get(v['nodes'])
                if not t or t[0] != v['tree']: continue
                for i, k in want.get(t, ()):
                    if i == v['node'] and k < len(v['locals']):
                        val = v['locals'][k]
                        ran['nonzero' if val else 'zero'] += 1
                        if val: ex.setdefault((t[0], i, k), val)
        print(f'reported reads the session ran: {sum(ran.values())}; the value read was zero {ran["zero"]}, '
              f'not zero {ran["nonzero"]}')
        for (cid, i, k), val in list(ex.items())[:15]:
            print(f'  tree {cid:#x} node {i} local {k}: read {val}')


if __name__ == '__main__':
    main(sys.argv)
