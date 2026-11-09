#!/usr/bin/env python3
"""Where an object keeps its definition, from a log of the recompiled game ('O' records).

Usage: definitions.py LOG IMAGE

For each object the log describes the words near it that point at a definition row (the
OBJD's contents: version 138, then 96 fields). This finds the offset at which every object
has one, and matches each object's row to the OBJD chunks on the disc by GUID (fields 12
and 13) and by every field.
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from tree_log import records
from disc import Disc
from check_chunks import iff_files, byte_order
import chunks


def disc_objds(img):
    """GUID -> [(file, label, fields)] for every OBJD on the disc."""
    out = collections.defaultdict(list)
    d = Disc(img)
    for name, data, cs in iff_files(d):
        end = byte_order(data, cs)
        for t, cid, fl, lab, o, sz in cs:
            if t != 'OBJD': continue
            try: od = chunks.objd(data[o:o + sz], end)
            except chunks.ChunkError: continue
            f = od['fields']
            out[f[12] | f[13] << 16].append((name.split('/')[-1], lab, f))
    return out


def main(argv):
    objects = [v for k, v in records(argv[1]) if k == 'O']
    offsets = collections.Counter()
    for obj, rows in objects:
        for off in {r[0] for r in rows}: offsets[off] += 1
    print(f'{len(objects)} objects described; offsets with a definition row: {offsets.most_common(8)}')
    if not offsets: return
    best = offsets.most_common(1)[0][0]
    objds = disc_objds(argv[2])
    res = collections.Counter(); ex = []
    for obj, rows in objects:
        row = next((r for r in rows if r[0] == best), None)
        if not row: res['no row at the offset'] += 1; continue
        f = list(struct.unpack_from('>96H', row[2], 4))
        # The 32-bit fields (12, 68, 70) have their halves the other way round in memory.
        for a in (12, 68, 70): f[a], f[a + 1] = f[a + 1], f[a]
        f = tuple(f)
        guid = f[12] | f[13] << 16
        hits = objds.get(guid, [])
        if not hits: res['GUID not on the disc'] += 1; ex.append(hex(guid)); continue
        same = [h for h in hits if tuple(h[2][:96]) == f]
        res['GUID on the disc, every field the same' if same else 'GUID on the disc, fields differ'] += 1
    print(f'at offset {best}:', dict(res), ex[:8])


if __name__ == '__main__':
    main(sys.argv)
