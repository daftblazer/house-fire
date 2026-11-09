"""EA/EOR .arc archives.

Two layouts, both with the same index entries (offset, size, a NUL-terminated name, then
12 bytes: a hash of the name, a time stamp and a word), all big-endian:
- the index first: the word 0x04000000, the entry count, a word, the entries; the data
  from 0x800 (audiostr.arc in Bustin' Out);
- the index last: a little-endian word at 0 giving its offset; there the count, a word,
  then the entries (everything else).
"""
import struct

def _entries(data, p, n):
    out = []
    for _ in range(n):
        off, size = struct.unpack('>II', data[p:p + 8])
        q = data.index(b'\0', p + 8)
        out.append((data[p + 8:q].decode('latin-1'), off, size))
        p = q + 1 + 12
    return out

def entries(data):
    """[(name, offset, size)], or None if `data` is not an archive."""
    if len(data) >= 12 and struct.unpack('>I', data[:4])[0] == 0x04000000:
        return _entries(data, 12, struct.unpack('>I', data[4:8])[0])
    if len(data) >= 8:
        at = struct.unpack('<I', data[:4])[0]
        if 4 <= at < len(data) - 8:
            n = struct.unpack('>I', data[at:at + 4])[0]
            try:
                e = _entries(data, at + 8, n)
            except (ValueError, struct.error):
                return None
            if all(o + s <= at for _, o, s in e):
                return e
    return None
