#!/usr/bin/env python3
"""The Sims 2: Castaway's lots' objects (R42E69's default.ngh, ObjM versions 120, 129, 130),
read as its loader does. Read from its ELF's named code (docs/lots.md, "Castaway").

Usage: castaway_objm.py [NGH]   (default: work/cache/R42E69/default.ngh)

- The table: a bool (compression), then an object id, and for each object its type (16 bits,
  ObjectFolder::ReconSelector: an index into the objt from version 47) and the next id, ended
  by an id of 0. Then the mark chain, one section per object and one for the lot.
- An object's section (cXObject::ReconStream): Bustin' Out's layout with two more counted
  arrays (from versions 99 and 118), the object data stored sparsely (EdithVariableSet: a
  72-bit set of the fields saved, then one 16-bit value for each), the tree stack
  (TreeStack, StackElem), the relationships (RelMatrixImpl), the slots, a counted array. A
  multi-tile object's part (cXMTObject) follows for its parts, then the loader's int (from
  version 54).
- The lot's part: an object, a counted list of objects, a wide string, as Bustin' Out's.

The class of each object comes from its definition in the original (ConstructObject: a
master id makes a multi-tile part, type 8 then a portal, type 2 a person). Castaway's object
files hold no OBJD chunks, so here it is found from the data: each type takes the classes
whose reading ends its objects' sections exactly at their marks, across all lots. A portal
or a person (cXPortal, cXPerson) is not read: no saved lot has one.

Checks: every section ends at its mark under its type's class; every lot's part ends the
stream. Exits non-zero otherwise.
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import ngh, lot, recon, objm

ROOT = pathlib.Path(__file__).resolve().parent.parent
START = 12


def lots(path):
    data = open(path, 'rb').read()
    return [(off, dict(lot.chunk_table(body))) for off, sec, count, ck, body, ok in ngh.blocks(data) if sec == 3]


def read_table(m):
    r = recon.Reader(m, START)
    r.compressed = r.rbool()
    table, oid = [], r.r16()
    while oid:
        t = r.r16()
        table.append(dict(id=oid, type=t))
        oid = r.r16()
    mark_at, nxt, secs = r.pos - START, r.mark(), []
    while True:
        start = mark_at + 4
        if nxt <= mark_at or START + nxt + 4 > len(m):
            secs.append((start, len(m) - START))
            break
        secs.append((start, nxt))
        mark_at = nxt
        nxt = struct.unpack_from('<I', m, START + nxt)[0]
    return r.compressed, table, secs


def counted16(r):
    n=r.r16()
    if not 0<=n<=4096: raise ValueError(f'count {n}')
    return [r.r16() for _ in range(n)]
def count32(r,lim=4096):
    n=r.r32()
    if not 0<=n<=lim: raise ValueError(f'count {n}')
    return n
def edith(r):
    w0,w1,w2=r.r32()&0xFFFFFFFF,r.r32()&0xFFFFFFFF,r.r8()&0xFF
    bits=w0|(w1<<32)|(w2<<64)
    keys=[k for k in range(72) if bits>>k&1]
    return {k:r.r16() for k in keys}
def frame(r,v):
    f={}
    if v>25: f['so']=r.r16()
    f['tree']=r.r16(); f['node']=r.r16()
    nl,np_=r.r8()&0xFF,r.r8()&0xFF
    f['params']=[r.r16() for _ in range(np_)]; f['locals']=[r.r16() for _ in range(nl)]
    f['engine']=r.r32(); f['file']=r.r16() if v>=47 else r.r32()
    return f
def tree_stack(r,v):
    n=count32(r,64); cap=r.r32()
    return [frame(r,v) for _ in range(n)]
def rel_array(r,v):
    a={'id':r.r32()} if v>=1 else {}
    a['values']=[r.r32() for _ in range(count32(r))]
    return a
def rel_matrix(r,v):
    a=r.r32(); out={'a':a}
    if 78<v<84: out['c']=r.r32()
    if a<0: out['arrays']=[rel_array(r,v) if r.r32() else None for _ in range(count32(r))]
    else: out['arrays']=[rel_array(r,v) for _ in range(a)]
    return out
def base(r,v):
    o={'header':[r.r32() for _ in range(6)]}
    if v>=25: o['header'].append(r.r32())
    if v<47: r.r16()
    o['header'].append(r.r16())
    if v==55: r.r32()
    o['attributes']=counted16(r)
    if v>=99: o['array2']=counted16(r)
    if v>=118: o['array3']=counted16(r)
    o['temps']=[r.r16() for _ in range(8)]
    o['data']=edith(r)
    if v<=105: r.r16()
    o['stack']=tree_stack(r,v)
    o['relations']=rel_matrix(r,v)
    if v>=5:
        n=r.r16(); o['slots']=[(r.r16(),r.r16()) for _ in range(n)]
    o['shorts']=counted16(r)
    return o
def mt_part(r,v):
    o={'a':r.r32(),'b':r.r32()}
    if v>=58: o['c']=r.r32()
    if v>=40: o['offset']=[r.r32() for _ in range(3)]
    o['leader']=r.r16(); return o
def portal_part(r,v):
    o=mt_part(r,v)
    if v>=15:
        n=count32(r); o['values']=[r.rfloat() for _ in range(n)] if v>=37 else [r.r32() for _ in range(n)]
    return o
PARTS={'object':None,'mt':mt_part,'portal':portal_part}
def section(m,a,b,v,cls):
    r=recon.Reader(m,START+a); r.compressed=True
    o=base(r,v); f=PARTS[cls]
    if f: o[cls]=f(r,v)
    if v>=54: o['loader_int']=r.r32()
    r.pad()
    if r.pos-START!=b: raise ValueError(f'ends at {r.pos-START} not {b}')
    return o
def try_classes(m,a,b,v):
    ok=[]
    for c in PARTS:
        try: section(m,a,b,v,c); ok.append(c)
        except Exception as e: pass
    return ok


def main(argv):
    path = argv[1] if len(argv) > 1 else str(ROOT / 'work' / 'cache' / 'R42E69' / 'default.ngh')
    res = collections.Counter()
    per_type = collections.defaultdict(lambda: set(PARTS))
    objects = []
    for off, cs in lots(path):
        m = cs['ObjM']
        v = struct.unpack_from('<I', m, 4)[0]
        comp, table, secs = read_table(m)
        types = {f[4]: g for g, f, n in lot.object_types(cs['objt'])}
        res['lots'] += 1
        res[f'version {v}'] += 1
        res['sections, one per object and the lot'] += len(secs) == len(table) + 1
        for x, (a, b) in zip(table, secs):
            g = types[x['type']]
            per_type[g] &= set(try_classes(m, a, b, v))
            objects.append(g)
        try:
            objm.lot_section(m, secs[-1][0])
            res["lot parts ending the stream"] += 1
        except ValueError:
            pass
    by_class = collections.Counter(' or '.join(sorted(per_type[g])) or 'NONE' for g in objects)
    print(dict(res))
    print('objects by their type\'s class:', dict(by_class))
    ok = res['lot parts ending the stream'] == res['lots'] == res['sections, one per object and the lot'] and 'NONE' not in by_class
    print('castaway_objm:', 'ok' if ok else 'FAILED')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv))
