"""Maxis IFF 2.5 (the PC Sims' format, kept by the console games).

A 60-byte header "IFF FILE 2.5:TYPE FOLLOWED BY SIZE\\0 JAMIE DOORNBOS & MAXIS 1",
a big-endian word (the rsmp chunk's offset), then chunks: a type (4 bytes), the size
(big-endian, header included), an id (2 bytes), flags (2) and a 64-byte label, all
big-endian here; the chunks' contents keep their own byte order."""
import struct, re

MAGIC = b'IFF FILE 2.5:TYPE FOLLOWED BY SIZE\0 JAMIE DOORNBOS & MAXIS 1'
_TYPE = re.compile(rb'^[A-Za-z0-9#$ ]{4}$')

def find_all(data):
    i = data.find(MAGIC)
    while i >= 0:
        yield i
        i = data.find(MAGIC, i + 1)

def chunks(data, at=0, end=None):
    """Chunks of the IFF file starting at `at`, as (type, id, flags, label, offset, size)."""
    end = len(data) if end is None else end
    p = at + 64
    out = []
    while p + 76 <= end:
        t = data[p:p + 4]
        if not _TYPE.match(t): break
        size = struct.unpack('>I', data[p + 4:p + 8])[0]
        if size < 76 or p + size > end: break
        cid, flags = struct.unpack('>hH', data[p + 8:p + 12])
        label = data[p + 12:p + 76].split(b'\0')[0].decode('latin-1')
        out.append((t.decode('latin-1'), cid, flags, label, p + 76, size - 76))
        p += size
    return out
