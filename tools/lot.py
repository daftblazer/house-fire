#!/usr/bin/env python3
"""The lots in Bustin' Out's neighbourhood file (tools/ngh.py, section 3).

Usage: lot.py IMAGE

A lot block starts with a chunk table: n FourCCs (backwards), n offsets (from the block's
start in the file, so 28 more than in the unpacked data) and n sizes. The chunks are Arry
(three of them), objt and ObjM, as in the PC game's house files.

- objt, the object types: 0, the version (2), "tjbo", then for each type a GUID, six 16-bit
  fields and a name (zero-ended, padded to an even length), ended by a zero GUID.
- ObjM, the objects: 0, the version (80), "MjbO", a compression code (1), then the
  engine's own object stream (The Sims 2's ObjectModuleImpl::DoStream through ReconBuffer:
  each object's ReconStream). Not decoded yet.

For every lot this reads the table and the object types (every one must end exactly at its
zero GUID), and counts the types whose GUID is a definition (OBJD) on the disc.
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import ngh
from definitions import disc_objds


def chunk_table(b):
    n = 0
    while b[4 * n:4 * n + 4].isalpha(): n += 1
    tags = [b[4 * i:4 * i + 4][::-1].decode() for i in range(n)]
    offs = struct.unpack_from(f'<{n}I', b, 4 * n)
    sizes = struct.unpack_from(f'<{n}I', b, 8 * n)
    return [(t, b[o - ngh.HEADER:o - ngh.HEADER + s]) for t, o, s in zip(tags, offs, sizes)]


def object_types(c):
    """[(GUID, six fields, name)]; raises if the chunk does not end at its zero GUID."""
    if c[8:12] != b'tjbo': raise ValueError('not an objt')
    p, out = 12, []
    while True:
        guid = struct.unpack_from('<I', c, p)[0]
        if guid == 0:
            if p + 4 != len(c): raise ValueError(f'objt ends at {p + 4}, not {len(c)}')
            return out
        fields = struct.unpack_from('<6H', c, p + 4)
        q = c.index(b'\0', p + 16)
        out.append((guid, fields, c[p + 16:q].decode('latin-1')))
        p = q + 1
        if (p - 12) % 2: p += 1


def lots(d):
    data = d.read('Runtime/default.ngh')
    return [(off, dict(chunk_table(body))) for off, sec, count, ck, body, ok in ngh.blocks(data) if sec == 3]


def main(argv):
    d = Disc(argv[1])
    objds = disc_objds(argv[1])
    res = collections.Counter()
    for off, cs in lots(d):
        types = object_types(cs['objt'])
        on = sum(1 for g, f, n in types if g in objds)
        m = cs['ObjM']
        res['lots'] += 1
        res['types'] += len(types)
        res['types on the disc'] += on
        print(f'lot at {off:#08x}: {len(types)} types ({on} on the disc), ObjM version '
              f'{struct.unpack_from("<I", m, 4)[0]}, {len(m)} bytes')
    print(dict(res))


if __name__ == '__main__':
    main(sys.argv)
