#!/usr/bin/env python3
"""Object resource files in quick data (`SimsObjects`, table `ResFile`, rows named after the
object's file): what an object's IFF file holds besides its definition and scripts.

A row is 72 bytes of pointers to arrays, one per resource type; an array's element count
is the word before it (as C++ `new[]` leaves it). The types and element sizes are from the
engine's readers, `FindRes<T>` and its callers in The Sims 2's map:

  +4 AnimRefTable 8      +8 PropRefTable 8         +12 SpriteIdToResIdNode 12
  +16 AStringSet 12      +20 WStringSet 12         +24 SndInfo 8
  +28 CatalogData 16     +32 BehaviorConstants 16  +36 TreeTable 12
  +40 SlotDescList 8     +44 ObjFnData 124         +48 FloatConstantsData 8

A string set's element is a pointer to the strings, the type tag `STR#` and the IFF chunk's
id in the top half of the third word; an interaction table's (TreeTable) a pointer to its
entries (36 bytes each), then a word whose low half is the TTAB chunk's id.

Checked against Bustin' Out (`check`): every quick resource of those kinds is the IFF
chunk of the same id in the object's file, and the counts of constants, slots, function
tables and sounds equal the file's BCON, SLOT, OBJf and FWAV counts. What the tool left
out is listed.

Usage: resfile.py check IMAGE
"""
import sys, pathlib, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import quickdata

ROW = 72
SLOTS = {
    4: ('AnimRefTable', 8), 8: ('PropRefTable', 8), 12: ('SpriteIdToResIdNode', 12),
    16: ('AStringSet', 12), 20: ('WStringSet', 12), 24: ('SndInfo', 8), 28: ('CatalogData', 16),
    32: ('BehaviorConstants', 16), 36: ('TreeTable', 12), 40: ('SlotDescList', 8),
    44: ('ObjFnData', 124), 48: ('FloatConstantsData', 8),
}
COUNTED = {'BehaviorConstants': 'BCON', 'SlotDescList': 'SLOT', 'ObjFnData': 'OBJf', 'SndInfo': 'FWAV'}
MAX_ELEMENTS = 4096


def array(img, o, off):
    """Offsets of the elements of the slot at `off` of the row at `o`."""
    if o + off not in img.pointers:
        return []
    p = img.ptr(o + off)
    n = img.u32(p - 4)
    if n > MAX_ELEMENTS:
        raise quickdata.QuickError(f'{n} elements')
    size = SLOTS[off][1]
    return [p + k * size for k in range(n)]


def rows(img):
    t = next(t for t in img.tables() if t['name'] == 'ResFile')
    if t['row_size'] != ROW:
        raise quickdata.QuickError(f'ResFile rows of {t["row_size"]} bytes')
    for i in range(t['count']):
        o = t['rows'] + i * ROW
        name = img.cstr(img.ptr(t['row_names'] + 4 * i))
        yield name, o


def resources(img, o):
    """{type: [element offsets]} and the ids of the string sets and interaction tables."""
    out = {name: array(img, o, off) for off, (name, size) in SLOTS.items()}
    ids = {
        'AStringSet': [img.u32(e + 8) >> 16 for e in out['AStringSet']],
        'WStringSet': [img.u32(e + 8) >> 16 for e in out['WStringSet']],
        'TreeTable': [img.u32(e + 4) & 0xFFFF for e in out['TreeTable']],
    }
    return out, ids


def check(disc):
    import iff
    from objects import sims_objects
    img = sims_objects(disc)
    files = {n.split('/')[-1].lower()[:-4]: n for n in disc.files if n.startswith('Runtime/') and n.endswith('.iff')}
    seen = bad = 0
    kept = collections.Counter(); dropped = collections.Counter(); wrong = []
    for name, o in rows(img):
        f = files.get(name.lower())
        if not f:
            continue
        seen += 1
        data = disc.read(f)
        have = collections.defaultdict(set)
        for t, cid, fl, lab, co, sz in iff.chunks(data, 0):
            have[t].add(cid)
        res, ids = resources(img, o)
        for kind, types in (('AStringSet', ('STR#',)), ('WStringSet', ('STR#', 'CTSS')), ('TreeTable', ('TTAB',))):
            for x in ids[kind]:
                kept[kind] += 1
                if not any(x in have[t] for t in types):
                    bad += 1; wrong.append((name, kind, x))
        for kind, chunk in COUNTED.items():
            kept[kind] += len(res[kind])
            if len(res[kind]) != len(have[chunk]):
                bad += 1; wrong.append((name, kind, len(res[kind]), len(have[chunk])))
        for cid in have['STR#'] - set(ids['AStringSet']) - set(ids['WStringSet']):
            dropped[f'STR# {cid}'] += 1
        for cid in have['TTAB'] - set(ids['TreeTable']):
            dropped['TTAB'] += 1
    print(f'{seen} objects with an IFF file: quick resources {dict(kept)}; {bad} not matching the file')
    for w in wrong[:5]:
        print('   e.g.', w)
    print('   left out of quick data:', dropped.most_common(10))
    return bad


if __name__ == '__main__':
    from disc import Disc
    if sys.argv[1] == 'check':
        sys.exit(1 if check(Disc(sys.argv[2])) else 0)
