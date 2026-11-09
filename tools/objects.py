#!/usr/bin/env python3
"""Object definitions from quick data (`SimsObjects`, table `ObjDefinition`), in every game.

A row (196 bytes) is an OBJD chunk's content: the version (32 bits, 138 in Bustin' Out),
then its first 96 fields as big-endian 16-bit words, except that the three pairs that make
32-bit values (fields 12-13, the object's GUID, 68-69 and 70-71) are stored as 32-bit
words, so their halves are swapped against the IFF's order. Three fields are the tool's
own: 92 an id it assigns, 94-95 a pointer to the object's `ObjDefinition::ResData` row (or
null); and it writes 0xFFFF in field 6 of some parts of multi-tile objects where the IFF
has 0 or 1 (twelve in Bustin' Out).

Checked against Bustin' Out, which has both: `check` compares every row with the OBJD of
the same GUID in its IFF files.

Usage: objects.py check IMAGE      objects.py list IMAGE
"""
import sys, struct, pathlib, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import quickdata

ROW = 196
FIELDS = 96
PAIRS = (12, 68, 70)      # 32-bit values: the GUID, and two more
TOOL_FIELDS = (92, 94, 95)


def from_row(img, o):
    r = img.data[o:o + ROW]
    version = struct.unpack_from('>I', r, 0)[0]
    f = list(struct.unpack_from(f'>{FIELDS}H', r, 4))
    for k in PAIRS:
        f[k], f[k + 1] = f[k + 1], f[k]
    resdata = img.ptr(o + 4 + 2 * 94) if (o + 4 + 2 * 94) in img.pointers else None
    return {'version': version, 'fields': f, 'guid': f[12] | f[13] << 16, 'resdata': resdata}


def definitions(img):
    t = next(t for t in img.tables() if t['name'] == 'ObjDefinition')
    if t['row_size'] != ROW:
        raise quickdata.QuickError(f'ObjDefinition rows of {t["row_size"]} bytes')
    return [from_row(img, t['rows'] + i * ROW) for i in range(t['count'])]


def sims_objects(disc):
    b = dict(quickdata.entries_of(disc))['SimsObjects']
    return quickdata.Image(b, quickdata.records(b)[0])


def check(disc):
    """Every quick definition against the IFF's OBJD with its GUID."""
    import iff, chunks
    defs = {d['guid']: d for d in definitions(sims_objects(disc))}
    iffs = {}
    for name, (o, s) in sorted(disc.files.items()):
        if name.startswith('Runtime/') and name.endswith('.iff'):
            data = disc.read(name)
            for t, cid, fl, lab, co, sz in iff.chunks(data, 0):
                if t == 'OBJD':
                    f = chunks.objd(data[co:co + sz])
                    iffs.setdefault(f['fields'][12] | f['fields'][13] << 16, (name, f))
    same = differ = field6 = 0
    diff_fields = collections.Counter()
    for g, d in defs.items():
        if g not in iffs:
            continue
        name, f = iffs[g]
        bad = [i for i in range(FIELDS) if i not in TOOL_FIELDS and f['fields'][i] != d['fields'][i]
               and not (i == 6 and d['fields'][i] == 0xFFFF)]
        if f['fields'][6] != d['fields'][6]:
            field6 += 1
        if f['version'] != d['version']:
            bad.append('version')
        if bad:
            differ += 1
            diff_fields.update(bad)
        else:
            same += 1
    print(f'{len(defs)} quick definitions, {len(iffs)} IFF OBJDs; {same + differ} with the same GUID: '
          f'{same} identical (fields 92, 94, 95 aside; {field6} with the tool\'s 0xFFFF in field 6), {differ} differ')
    if diff_fields:
        print('   differing fields:', diff_fields.most_common(12))
    return differ


def main(argv):
    from disc import Disc
    cmd, img = argv[0], argv[1]
    d = Disc(img)
    if cmd == 'check':
        sys.exit(1 if check(d) else 0)
    for o in definitions(sims_objects(d)):
        print(f'{o["guid"]:08x} version {o["version"]} resdata {"-" if o["resdata"] is None else hex(o["resdata"])}')


if __name__ == '__main__':
    main(sys.argv[1:])
