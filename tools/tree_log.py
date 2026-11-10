#!/usr/bin/env python3
"""Read a log of the recompiled Bustin' Out's interpreter (its src/tree_log.cpp,
BUSTIN_TREE_LOG) and check it against the disc and docs/interpreter.md.

Usage: tree_log.py LOG IMAGE

- Each tree the game ran is found on the disc: the IFF file and BHAV whose nodes it has,
  and how the operand bytes in memory relate to the file's.
- Each step is checked against the node step: a primitive's node moves to its true or false
  target, or stays (a yield); a call starts a frame on the called tree; a return comes from
  254 or 255. What a called tree's parameters and locals hold on its first step is counted
  (the open questions of docs/interpreter.md).
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
from check_chunks import byte_order
import iff, chunks

SKIP = ('audiostr', 'movies', 'samples', 'textures', 'models', 'animatio')


def records(path):
    b = pathlib.Path(path).read_bytes()
    p, n = 0, len(b)
    while p < n:
        k = b[p:p + 1]; p += 1
        if k == b'K':
            if p + 4 > n: break
            yield 'K', struct.unpack_from('<I', b, p)[0]; p += 4
        elif k == b'L':
            if p + 6 > n: break
            tick, cnt = struct.unpack_from('<IH', b, p); p += 6
            if p + 6 * cnt > n: break
            items = [struct.unpack_from('<Ih', b, p + 6 * i) for i in range(cnt)]; p += 6 * cnt
            yield 'L', (tick, items)
        elif k in (b'Q', b'q'):  # the pass over the objects starts / returns: its cursor
            if p + 4 > n: break
            yield k.decode(), struct.unpack_from('<I', b, p)[0]; p += 4
        elif k == b'C':
            if p + 6 > n: break
            yield 'C', struct.unpack_from('<Ih', b, p); p += 6
        elif k == b'M':
            if p + 4 + 0x1100 > n: break
            obj = struct.unpack_from('<I', b, p)[0]
            yield 'M', (obj, b[p + 4:p + 4 + 0x1100]); p += 4 + 0x1100
        elif k == b'O':
            if p + 4 > n: break
            obj = struct.unpack_from('<I', b, p)[0]; p += 4
            rows = []
            while p + 2 <= n:
                off = struct.unpack_from('<h', b, p)[0]; p += 2
                if off == 0x7FFF: break
                inner, ptr = struct.unpack_from('<HI', b, p); p += 6
                rows.append(((off, inner), ptr, b[p:p + 196])); p += 196
            yield 'O', (obj, rows)
        elif k == b'B':
            if p + 8 > n: break
            yield 'B', (struct.unpack_from('<I', b, p)[0], struct.unpack_from('<f', b, p + 4)[0]); p += 8
        elif k == b'Y':  # autonomy's sorted list: (id, interaction, value, takes-best flag)
            if p + 2 > n: break
            cnt = struct.unpack_from('<H', b, p)[0]; p += 2
            if p + 16 * cnt > n: break
            rows = []
            for _ in range(cnt):
                oid, _x, i4, val, f12 = struct.unpack_from('<hHIfI', b, p); p += 16
                rows.append((oid, i4 & 0xFFFF, val, f12))
            yield 'Y', rows
        elif k == b'G':  # the route planner's vector: (state, result, [(word, word)])
            if p + 10 > n: break
            st, res, cnt = struct.unpack_from('<IIH', b, p); p += 10
            if p + 8 * cnt > n: break
            yield 'G', (st, res, [struct.unpack_from('<II', b, p + 8 * i) for i in range(cnt)]); p += 8 * cnt
        elif k == b'R':  # the random number autonomy's choice draws: (site, result)
            if p + 5 > n: break
            yield 'R', struct.unpack_from('<BI', b, p); p += 5
        elif k == b'W':
            if p + 6 > n: break
            ev, cnt = struct.unpack_from('<IH', b, p); p += 6
            curves = []
            for _ in range(cnt):
                motive, npts = struct.unpack_from('<II', b, p); p += 8
                pts = struct.unpack_from('<64f', b, p); p += 256
                slopes = struct.unpack_from('<32f', b, p); p += 128
                curves.append((motive, [(pts[2 * i], pts[2 * i + 1]) for i in range(min(npts, 32))], list(slopes[:min(npts, 32)])))
            yield 'W', (ev, curves)
        elif k == b'A':
            if p + 12 + 64 + 132 > n: break
            ev, ad = struct.unpack_from('<II', b, p)
            score = struct.unpack_from('<f', b, p + 8)[0]
            motives = struct.unpack_from('<16f', b, p + 12)
            adv = b[p + 76:p + 76 + 132]
            yield 'A', dict(evaluator=ev, adverts=ad, score=score, motives=motives, raw=adv); p += 12 + 64 + 132
        elif k == b'U':
            if p + 7 > n: break
            yield 'U', struct.unpack_from('<hhhB', b, p); p += 7
        elif k == b'N':
            if p + 66 > n: break
            yield 'N', (struct.unpack_from('<h', b, p)[0], struct.unpack_from('<16f', b, p + 2)); p += 66
        elif k == b'P':
            if p + 14 > n: break
            yield 'P', struct.unpack_from('<hiii', b, p); p += 14
        elif k == b'V':
            if p + 27 > n: break
            f = struct.unpack_from('<IIhhhBIII', b, p); p += 27
            yield 'V', dict(zip(('object', 'lr', 'scope', 'data', 'value', 'flags', 'ptr', 'fptr', 'fvalue'), f))
        elif k == b'E':
            if p + 28 > n: break
            f = struct.unpack_from('<hhhhBBhhhIII', b, p); p += 28
            yield 'E', dict(zip(('lhs_scope', 'lhs_data', 'rhs_scope', 'rhs_data', 'op', 'flags', 'rhs',
                                 'lhs', 'lhs_after', 'lhs_float', 'lhs_float_after', 'result'), f))
        elif k == b'T':
            if p + 12 > n: break
            nodes, tree, count, file = struct.unpack_from('<IHHI', b, p); p += 12
            if p + count * 12 > n: break
            yield 'T', (nodes, tree, file, b[p:p + count * 12]); p += count * 12
        elif k == b'S':
            if p + 28 > n: break
            sim, frame, file, nodes, tree, node, after, look, res, err, np_, nl = \
                struct.unpack_from('<IIIIHHHBBHBB', b, p); p += 28
            c = np_ + nl
            if p + 4 * c > n: break
            before = struct.unpack_from(f'<{c}h', b, p); p += 2 * c
            aft = struct.unpack_from(f'<{c}h', b, p); p += 2 * c
            if p + 20 > n: break
            defaults = struct.unpack_from('<8h', b, p); p += 16
            stack_object, stack_object_after = struct.unpack_from('<hh', b, p); p += 4
            yield 'S', dict(sim=sim, frame=frame, file=file, nodes=nodes, tree=tree & 0x7FFF,
                            flag=tree >> 15, node=node, after=after, look=look, res=res, err=err,
                            params=before[:np_], locals=before[np_:],
                            params_after=aft[:np_], locals_after=aft[np_:], defaults=defaults,
                            stack_object=stack_object, stack_object_after=stack_object_after)
        else:
            raise SystemExit(f'bad record {k!r} at {p - 1}')


def disc_trees(img):
    """(tree id, (opcode, true, false) of each node) -> [(file, label, bhav, order)]"""
    d = Disc(img)
    out = collections.defaultdict(list)
    for name, (off, size) in sorted(d.files.items()):
        if any(s in name.lower() for s in SKIP) or size > 200 << 20: continue
        data = d.read(name)
        starts = list(iff.find_all(data))
        for i, s in enumerate(starts):
            cs = iff.chunks(data, s, starts[i + 1] if i + 1 < len(starts) else None)
            end = byte_order(data, cs)
            label = next((lab for t, cid, fl, lab, o, sz in cs if t == 'OBJD'), '') or f'@{s:x}'
            for t, cid, fl, lab, o, sz in cs:
                if t != 'BHAV': continue
                try: bh = chunks.bhav(data[o:o + sz], end)
                except chunks.ChunkError: continue
                shape = tuple((x['opcode'], x['true'], x['false']) for x in bh['nodes'])
                out[(cid & 0xFFFF, shape)].append((f'{name}:{label}', lab, bh, end))
    return out


def mem_shape(raw):
    return tuple((struct.unpack_from('>H', raw, i)[0], raw[i + 2], raw[i + 3])
                 for i in range(0, len(raw), 12))


def operand_order(mem, disc):
    if mem == disc: return 'same'
    if mem == b''.join(disc[i:i + 2][::-1] for i in range(0, 8, 2)): return 'swapped16'
    if mem == b''.join(disc[i:i + 4][::-1] for i in range(0, 8, 4)): return 'swapped32'
    return 'other'


def main(argv):
    log, img = argv[1], argv[2]
    trees = {}          # nodes address -> (tree id, file, raw), as loaded at the time
    every = []          # each tree loaded
    steps = []
    ticks = 0
    for k, v in records(log):
        if k == 'K': ticks += 1
        elif k == 'T':
            trees[v[0]] = (v[1], v[2], v[3]); every.append((v[0], v[1], v[2], v[3]))
        else:
            v['t'] = trees.get(v['nodes']); steps.append(v)
    print(f'{ticks} ticks, {len(steps)} steps, {len(every)} trees loaded')

    # Trees found on the disc.
    dt = disc_trees(img)
    found = {}; orders = collections.Counter(); missing = []
    files_of = collections.defaultdict(set)   # memory file pointer -> disc files
    for addr, tid, file, raw in every:
        hits = dt.get((tid, mem_shape(raw)))
        if not hits:
            missing.append((tid, len(raw) // 12)); continue
        src, lab, bh, end = hits[0]
        found[addr] = (src, lab, bh)
        files_of[file].add(frozenset(h[0] for h in hits))
        for i, x in enumerate(bh['nodes']):
            if x['opcode'] < 256:
                orders[(x['opcode'], operand_order(raw[i * 12 + 4:i * 12 + 12], x['operand']))] += 1
    print(f'trees on the disc: {len(found)} of {len(every)}; not found: {missing[:10]}')
    by = collections.defaultdict(collections.Counter)
    for (op, o), n in orders.items(): by[o][op] += n
    for o, c in by.items():
        print(f'  operands {o}: {sum(c.values())} nodes, opcodes {sorted(c)[:30]}')

    # The steps against the node step.
    out = collections.Counter(); errs = collections.Counter(); ex = {}
    first = collections.Counter()   # what a called frame holds on its first step
    stacks = collections.defaultdict(list)
    for i, s in enumerate(steps):
        st = stacks[s['sim']]
        if s['frame'] in st:
            del st[st.index(s['frame']) + 1:]
        else:
            st.append(s['frame'])
        if s['err']: errs[s['err']] += 1
        t = s['t']
        if t and t[0] != s['tree']:
            out['lookup tree differs from the frame'] += 1; t = None
        if s['look'] == ord('X') and t:
            raw = t[2]; n = s['node']
            op = struct.unpack_from('>H', raw, n * 12)[0] & 0x7FFF
            tt, ft = raw[n * 12 + 2], raw[n * 12 + 3]
            if op < 256:
                a = s['after']
                if s['res'] == 1:   # the tree yields: the node stays, to be run again
                    kind = 'yield' if a == n else 'yield, moved'
                elif s['res'] == 2:
                    kind = 'stop'
                else:
                    kind = ('either' if a == tt == ft else 'true' if a == tt else
                            'false' if a == ft else 'other')
                out[(kind, s['res'])] += 1
                if kind == 'other': ex.setdefault('other', (hex(s['tree']), n, op, tt, ft, a, s['res']))
            else:
                out[('call', s['res'])] += 1
                nxt = next((x for x in steps[i + 1:i + 50] if x['sim'] == s['sim']), None)
                operand = raw[n * 12 + 4:n * 12 + 12]
                none = operand == b'\xff' * 8
                if nxt and nxt['tree'] == op and nxt['frame'] != s['frame']:
                    out['call: frame on the callee'] += 1
                    p = struct.unpack('>4h', operand)
                    if not none and tuple(nxt['params']) != p[:len(nxt['params'])] and 'params' not in ex:
                        ex['params'] = (hex(s['tree']), n, hex(op), operand.hex(), nxt['params'])
                    if none:
                        first[('params, none given', 'zero' if not any(nxt['params']) else 'caller' if tuple(nxt['params']) == tuple(s['params'][:len(nxt['params'])]) else 'other')] += 1
                    else:
                        first[('params given', 'operand' if tuple(nxt['params']) == p[:len(nxt['params'])] else 'other')] += 1
                    first[('locals', 'zero' if not any(nxt['locals']) else 'other')] += 1
                    if first[('locals', 'other')] == 1 and any(nxt['locals']):
                        ex.setdefault('locals', (hex(op), nxt['locals']))
                else:
                    out['call: no callee frame next'] += 1
                    ex.setdefault('nocallee', (hex(s['tree']), n, hex(op), nxt and (hex(nxt['tree']), nxt['node'])))
        elif s['look'] == ord('R'):
            out[('return', s['node'])] += 1
        elif s['look'] == 0:
            out[('no lookup', s['node'], s['res'])] += 1
    print('steps:')
    for k, n in sorted(out.items(), key=lambda kv: -kv[1]): print(f'  {k}: {n}')
    print('first step of a called tree:')
    for k, n in sorted(first.items()): print(f'  {k}: {n}')
    print('errors:', dict(errs))
    print('examples:', ex)
    ids = collections.Counter('global' if t < 4096 else 'own' if t < 8192 else 'semi-global' if t <= 9000 else 'other'
                              for a, t, f, r in every)
    print('trees by id range:', dict(ids))


if __name__ == '__main__':
    main(sys.argv)
