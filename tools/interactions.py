#!/usr/bin/env python3
"""Interaction tables in quick data: a `ResFile` row's TreeTable slot (tools/resfile.py),
each element pointing to an array of entries, one an interaction (a TTAB entry). Entries
are 28 bytes in the v2.0 games, 32 in The Urbz, 36 in The Sims 2 and Pets; big-endian
words, a pair of 16-bit fields stored as one word where two share it:

  +0   guard (high half), action (low): trees
  +4   pointer to the motive entries (or null)
  +8   attenuation value (float)
  v2.0:   +12 attenuation code (high), flags (low)   +16 order (high), autonomy threshold (low)
          +20 joining index (high), string index (low)   +24 pointer to the name
  v3.0:   +12 attenuation code   +16 flags   +20 order (high), autonomy threshold (low)
          +24 joining index (high), string index (low)   +28 pointer to the name
          (+32 one more word in The Sims 2 and Pets, 0 so far)

A motive entry is four 16-bit fields, personality, minimum, delta and the motive's index,
in an array whose count is the word before it; only the motives an interaction changes are
there. The name pointer is to the table's list of names (its TTAs, in the language image),
indexed by the string index; names are UTF-16 (big-endian).

Checked on Bustin' Out (`check`): every quick interaction against the same table of its
object's IFF file, field by field, the motives and the name (the TTAs string).

Usage: interactions.py check IMAGE | trees IMAGE | list IMAGE

`trees` checks every game with scripts: each action and guard names a tree the object has.
"""
import sys, struct, pathlib, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import quickdata, resfile, objects

LAYOUT = {'GCIE69': 28, 'G4ME69': 28, 'GUBE69': 32, 'G4ZE69': 36, 'G4OE69': 36}
MAX_MOTIVES = 64


def motives(img, o):
    """{motive index: (min, delta, personality)}: only the motives the interaction changes."""
    if o not in img.pointers:
        return {}
    m = img.ptr(o)
    n = img.u32(m - 4)
    if n > MAX_MOTIVES:
        raise quickdata.QuickError(f'{n} motives')
    out = {}
    for k in range(n):
        pers, mn, delta, idx = struct.unpack_from('>HhhH', img.data, m + 8 * k)
        out[idx] = (mn, delta, pers)
    return out


def name(img, at, index):
    """The interaction's name: the pointer at `at` is to the table's name list (its TTAs)."""
    if at not in img.pointers:
        return None
    lst = img.ptr(at) + 4 * index
    if lst + 4 > len(img.data) or img.u32(lst) == 0:
        return None
    return img.wstr(img.u32(lst) - img.base)


def entry(img, x, size):
    w = lambda k: img.u32(x + k)
    e = {'guard': w(0) >> 16, 'action': w(0) & 0xFFFF, 'motives': motives(img, x + 4),
         'attenuation_value': struct.unpack('>f', img.data[x + 8:x + 12])[0]}
    if size == 28:
        e.update(attenuation_code=w(12) >> 16, flags=w(12) & 0xFFFF, order=w(16) >> 16, autonomy_threshold=w(16) & 0xFFFF,
                 joining_index=struct.unpack('>h', img.data[x + 20:x + 22])[0], tta_index=w(20) & 0xFFFF)
        name_at = 24
    else:
        e.update(attenuation_code=w(12), flags=w(16), order=w(20) >> 16, autonomy_threshold=w(20) & 0xFFFF,
                 joining_index=struct.unpack('>h', img.data[x + 24:x + 26])[0], tta_index=w(24) & 0xFFFF)
        name_at = 28
        if size == 36:
            e['extra'] = w(32)
    e['name'] = name(img, x + name_at, e['tta_index'])
    return e


def tables(img, game):
    """{(object file, table id): [entries]}"""
    size = LAYOUT[game]
    out = {}
    for name, o in resfile.rows(img):
        res, ids = resfile.resources(img, o)
        for el, tid in zip(res['TreeTable'], ids['TreeTable']):
            if el not in img.pointers:
                out[(name, tid)] = []
                continue
            p = img.ptr(el)
            n = img.u32(p - 4)
            if n > 256:
                raise quickdata.QuickError(f'{n} interactions')
            out[(name, tid)] = [entry(img, p + size * k, size) for k in range(n)]
    return out


def check(disc):
    import iff, chunks
    img = objects.sims_objects(disc)
    qt = tables(img, disc.id)
    files = {n.split('/')[-1].lower()[:-4]: n for n in disc.files if n.startswith('Runtime/') and n.endswith('.iff')}
    same = differ = 0
    fields = collections.Counter(); ex = []; flagdiff = collections.Counter()
    cache = {}
    for (name, tid), entries in qt.items():
        f = files.get(name.lower())
        if not f:
            continue
        if f not in cache:
            data = disc.read(f)
            cs = iff.chunks(data, 0)
            tt = {c[1]: chunks.ttab(data[c[4]:c[4] + c[5]]) for c in cs if c[0] == 'TTAB'}
            ta = {c[1]: chunks.strings(data[c[4]:c[4] + c[5]]) for c in cs if c[0] == 'TTAs'}
            cache[f] = (tt, ta)
        tt, ta = cache[f]
        ref = tt[tid]['interactions']
        names = ta.get(tid)
        if len(ref) != len(entries):
            differ += 1; fields['count'] += 1; ex.append((name, tid, 'count', len(entries), len(ref))); continue
        bad = []
        for q, r in zip(entries, ref):
            for k in ('action', 'guard', 'attenuation_code', 'autonomy_threshold', 'joining_index'):
                if q[k] != (r[k] & 0xFFFF if k == 'tta_index' else r[k]):
                    bad.append(k)
            if r['attenuation_value'] == r['attenuation_value'] and abs(q['attenuation_value'] - r['attenuation_value']) > 1e-6 \
                    and struct.pack('<f', r['attenuation_value']) != b'\xcd\xcd\xcd\xcd':
                bad.append('attenuation_value')
            want = {i: (m['min'], m['delta'], m['personality']) for i, m in enumerate(r['motives']) if any(m.values())}
            if q['motives'] != want:
                bad.append('motives')
            if q['flags'] != r['flags'] & 0xFFFF:
                flagdiff[q['flags'] ^ (r['flags'] & 0xFFFF)] += 1
            if names and q['name'] not in {x['value'] for x in names['strings']}:
                bad.append('name')
        if bad:
            differ += 1; fields.update(bad); ex.append((name, tid, sorted(set(bad))))
        else:
            same += 1
    print(f'{same + differ} quick interaction tables matched to the IFF: {same} identical, {differ} differ')
    print('   flags: quick XOR IFF where they differ:', [(hex(k), v) for k, v in flagdiff.most_common(6)])
    if fields:
        print('   differing:', fields.most_common(10))
        for e in ex[:6]:
            print('   e.g.', e)
    return differ


def main(argv):
    from disc import Disc
    d = Disc(argv[1])
    if argv[0] == 'check':
        sys.exit(1 if check(d) else 0)
    img = objects.sims_objects(d)
    for (name, tid), es in tables(img, d.id).items():
        for e in es:
            print(f'{name} {tid}: {e["name"]!r} action {e["action"]:#06x} guard {e["guard"]:#06x} flags {e["flags"]:#x}')


if __name__ == '__main__' and sys.argv[1] in ('check', 'list'):
    main(sys.argv[1:])


def check_trees(disc):
    """Every interaction's action and guard is a tree its object has: its own (4096-8191,
    in the object's script file), or a global or semi-global one (by range). Works where the
    scripts are on the disc as IFF (all but The Sims)."""
    import iff, arc
    from check_chunks import byte_order
    img = objects.sims_objects(disc)
    own = collections.defaultdict(set)   # file name -> own tree ids
    if disc.id == 'G4ME69':
        for n in disc.files:
            if n.startswith('Runtime/') and n.endswith('.iff'):
                data = disc.read(n)
                own[n.split('/')[-1][:-4].lower()] = {c[1] for c in iff.chunks(data, 0) if c[0] == 'BHAV'}
    else:
        name = next(n for n in disc.files if n.lower().endswith('edithtre.arc'))
        data = disc.read(name)
        for n, o, s in arc.entries(data):
            st = data.find(iff.MAGIC, o, o + s)
            if st >= 0:
                own[n.lower()] |= {c[1] for c in iff.chunks(data, st, o + s) if c[0] == 'BHAV'}
    total = missing = nofile = 0
    ex = []
    for (fname, tid), entries in tables(img, disc.id).items():
        trees = own.get(fname.lower())
        if trees is None:
            nofile += len(entries)
            continue
        for e in entries:
            for k in ('action', 'guard'):
                t = e[k]
                if t == 0 or t == 0xFFFF:
                    continue
                total += 1
                if 4096 <= t < 8192 and t not in trees:
                    missing += 1; ex.append((fname, tid, k, hex(t)))
    print(f'{disc.id}: {total} actions and guards, {missing} naming an own tree the object does not have '
          f'({nofile} interactions of objects without a script file)')
    for e in ex[:5]:
        print('   e.g.', e)
    return missing


if __name__ == '__main__' and sys.argv[1] == 'trees':
    from disc import Disc
    sys.exit(1 if check_trees(Disc(sys.argv[2])) else 0)
