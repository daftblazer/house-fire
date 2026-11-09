#!/usr/bin/env python3
"""Bustin' Out's neighbourhood file (Runtime/default.ngh): the lots and Sims the game starts
from. The game reads it, not the Houses/*.iff files, which it never loads.

Usage: ngh.py IMAGE [--dump]

The file is 4 KB blocks (0xCD where unused). A block that holds data starts with a header of
seven 32-bit little-endian words:

    section, 28 (the header's size), unpacked size, packed size, checksum, 4, count

then the packed data: LZSS (a flag byte, low bit first, 1 for a literal byte, 0 for a
two-byte reference: 12-bit position, the high nibble of the second byte its top, and a
length of the low nibble + 3; a 4096-byte ring starting at 0xFEE). Section 0 is a
directory: its FourCCs are stored backwards ("TTAT", then pairs of "IMIS" "SUOH": SIMI and
HOUS).

Every block must unpack to exactly its stated size using exactly its packed size; the
tool says so for each. With --dump the unpacked blocks go to work/ngh/ (not part of the
repository).
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc

ROOT = pathlib.Path(__file__).resolve().parent.parent
BLOCK = 0x1000
HEADER = 28


def lzss(src, size):
    """Unpack `size` bytes; returns (data, bytes of src used)."""
    ring = bytearray(4096)
    r = 0xFEE
    out = bytearray()
    i = 0
    flags = 0
    while len(out) < size:
        flags >>= 1
        if not flags & 0x100:
            if i >= len(src): raise ValueError('packed data ends early')
            flags = src[i] | 0xFF00
            i += 1
        if flags & 1:
            c = src[i]; i += 1
            out.append(c); ring[r] = c; r = (r + 1) & 4095
        else:
            lo, hi = src[i], src[i + 1]; i += 2
            pos = lo | (hi & 0xF0) << 4
            for k in range((hi & 0x0F) + 3):
                c = ring[(pos + k) & 4095]
                out.append(c); ring[r] = c; r = (r + 1) & 4095
    return bytes(out[:size]), i


def blocks(data):
    """(offset, section, count, checksum, unpacked) for each block with a header."""
    out = []
    for off in range(0, len(data) - HEADER, BLOCK):
        section, hs, un, pk, ck, four, count = struct.unpack_from('<7I', data, off)
        if hs != HEADER or four != 4 or not 0 < pk <= un < 1 << 24 or off + HEADER + pk > len(data):
            continue
        body, used = lzss(data[off + HEADER:off + HEADER + pk], un)
        out.append((off, section, count, ck, body, used == pk))
    return out


def main(argv):
    d = Disc(argv[1])
    data = d.read('Runtime/default.ngh')
    bs = blocks(data)
    exact = sum(1 for b in bs if b[5])
    by = collections.Counter(b[1] for b in bs)
    print(f'{len(data)} bytes, {len(bs)} blocks with a header; unpacked exactly: {exact} of {len(bs)}')
    print('blocks by section:', dict(sorted(by.items())))
    for off, sec, count, ck, body, ok in bs[:6]:
        print(f'  {off:#08x} section {sec} count {count} -> {len(body)} bytes: {body[:24]!r}')
    if '--dump' in argv:
        out = ROOT / 'work' / 'ngh'
        out.mkdir(parents=True, exist_ok=True)
        for off, sec, count, ck, body, ok in bs:
            (out / f'{off:08x}_s{sec}.bin').write_bytes(body)
        print('unpacked blocks in', out)
    return 0 if exact == len(bs) else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv))
