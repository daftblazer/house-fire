#!/usr/bin/env python3
"""Quick data: the engine's tables, baked into images (`quickdat.arc` entries such as
`SimsObjects`, in all five games). Read as the engine's `ERQuickdata` reads them
(LoadXImage, applyFixups; names from The Sims 2's map).

File: records of 24 bytes, all big-endian words: the record size (24), the data's offset
and size, the fix-ups' offset and size, and the address the image was built for. The first
record's offset field is also the header's size, so there are offset/24 records: the first
is the main image, the others one each for a language (the engine loads only the current
one, after the main image).

Fix-ups: words, read in order; the low two bits say what follows. 0: one pointer at that
offset; 1: a count in the rest of the word and an offset in the next, that many pointers
from there; 2 and 3: two or three pointers at that offset (low bits cleared). Each pointer
had the build address added; here pointers are read back as offsets into the image.

Image: the table directory, in one of two layouts. The Sims and Bustin' Out: a pointer to
the path of the tool's source file, the table count, then the descriptors from 8. The v3.0
games: a version word (1), a pointer to the image's name, the count, three zero words, the
descriptors from 24. A descriptor is 20 bytes a table: name, rows, row names (pointers), row count, row size. In the v3.0 games the
tables are sorted by name (the engine finds them by binary search) and rows by their names;
The Sims' and Bustin' Out's SimsObjects are not (ObjDefinition::ResData before
ObjDefinition).

Usage: quickdata.py IMAGE [ENTRY]   lists the tables of each quick data entry.
"""
import sys, struct, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))

MAX_TABLES = 4096


class QuickError(ValueError):
    pass


def be32(b, o):
    return struct.unpack_from('>I', b, o)[0]


def records(b):
    if len(b) < 24 or be32(b, 0) != 24:
        raise QuickError('not quick data')
    n = be32(b, 4) // 24
    if not 1 <= n <= 64:
        raise QuickError(f'{n} records')
    out = []
    for i in range(n):
        size, off, dsize, foff, fsize, base = struct.unpack_from('>6I', b, i * 24)
        if size != 24 or off + dsize > len(b) or foff + fsize > len(b):
            raise QuickError(f'record {i} out of range')
        out.append({'offset': off, 'size': dsize, 'fixups': (foff, fsize), 'base': base})
    return out


def fixup_offsets(b, rec):
    """The image offsets that hold pointers, from the record's fix-up stream."""
    foff, fsize = rec['fixups']
    words = struct.unpack_from(f'>{fsize // 4}I', b, foff)
    out = []
    i = 0
    while i < len(words):
        w = words[i]; i += 1
        kind = w & 3
        if kind == 0:
            out.append(w)
        elif kind == 1:
            if i >= len(words):
                raise QuickError('fix-up count without an offset')
            n, at = w >> 2, words[i]; i += 1
            if n > rec['size'] // 4:
                raise QuickError(f'{n} pointers in one fix-up')
            out.extend(at + 4 * k for k in range(n))
        else:
            at = w & ~3
            out.extend(at + 4 * k for k in range(kind))
    return out


class Image:
    """One image, its pointers read back as offsets into it."""

    def __init__(self, b, rec):
        self.data = b[rec['offset']:rec['offset'] + rec['size']]
        self.base = rec['base']
        self.pointers = set(fixup_offsets(b, rec))
        for p in self.pointers:
            if p + 4 > len(self.data):
                raise QuickError(f'pointer at {p:#x} outside the image')

    def u32(self, o):
        return be32(self.data, o)

    def ptr(self, o):
        """The offset a pointer at `o` points to, or None for a null pointer."""
        v = self.u32(o)
        if v == 0 and o not in self.pointers:
            return None
        if o not in self.pointers:
            raise QuickError(f'{o:#x} is not a pointer')
        t = v - self.base
        if not 0 <= t <= len(self.data):
            raise QuickError(f'pointer at {o:#x} leaves the image')
        return t

    def cstr(self, o):
        e = self.data.index(b'\0', o)
        return self.data[o:e].decode('latin-1')

    def layout(self):
        """(header version, count offset, first descriptor offset)"""
        if 0 in self.pointers:
            return 0, 4, 8
        if self.u32(0) == 1 and 4 in self.pointers:
            return 1, 8, 24
        raise QuickError(f'unknown directory layout ({self.u32(0):#x})')

    def tables(self):
        _, at, first = self.layout()
        n = self.u32(at)
        if not 0 <= n <= MAX_TABLES or first + 20 * n > len(self.data):
            raise QuickError(f'{n} tables')
        out = []
        for i in range(n):
            d = first + 20 * i
            name = self.cstr(self.ptr(d))
            rows = self.ptr(d + 4)
            names = self.ptr(d + 8)
            out.append({'name': name, 'rows': rows, 'row_names': names,
                        'count': self.u32(d + 12), 'row_size': self.u32(d + 16)})
        return out


def entries_of(disc):
    """(name, bytes) of each entry of the disc's quickdat.arc."""
    import arc
    name = next(n for n in disc.files if n.lower().endswith('quickdat.arc'))
    data = disc.read(name)
    for n, o, s in arc.entries(data):
        yield n, data[o:o + s]


def main(argv):
    from disc import Disc
    d = Disc(argv[0])
    want = argv[1] if len(argv) > 1 else None
    for n, b in entries_of(d):
        if want and n != want:
            continue
        recs = records(b)
        img = Image(b, recs[0])
        ts = img.tables()
        print(f'== {d.id} {n}: {len(b)} bytes, {len(recs) - 1} language images, '
              f'{len(img.pointers)} pointers, {len(ts)} tables')
        ordered = [t['name'] for t in ts] == sorted((t['name'] for t in ts))
        print(f'   tables sorted by name: {ordered}')
        for t in ts:
            print(f'   {t["name"]:40s} {t["count"]:6d} rows of {t["row_size"]:5d} bytes'
                  f'{"" if t["row_names"] is None else ", named"}')


if __name__ == '__main__':
    main(sys.argv[1:])
