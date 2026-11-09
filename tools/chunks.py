"""Readers for the IFF chunk types the engine uses.

Each reader takes the chunk's bytes and its byte order ('<' for Bustin' Out's, which keeps
the PC's, '>' for the v3.0 games, whose converter swapped each field) and returns a dict.
A reader must account for every byte: what is left over may only be padding (0x00, or the
fill bytes 0xA3 and 0xCD that Maxis's tools left), otherwise it raises ChunkError. That is
the check. The editor's own chunks (TREE, TRCN: the layout of a tree's boxes and labels for
constants) are read only as far as their headers: the engine has no use for them.
"""
import struct


class ChunkError(ValueError):
    pass


class R:
    """A cursor over a chunk."""

    def __init__(self, b, end='<'):
        self.b, self.p, self.e = b, 0, end

    def left(self):
        return len(self.b) - self.p

    def take(self, n):
        if self.p + n > len(self.b):
            raise ChunkError(f'needs {n} bytes at {self.p}, has {self.left()}')
        v = self.b[self.p:self.p + n]
        self.p += n
        return v

    def u8(self): return self.take(1)[0]
    def s8(self): return struct.unpack('b', self.take(1))[0]
    def u16(self): return struct.unpack(self.e + 'H', self.take(2))[0]
    def s16(self): return struct.unpack(self.e + 'h', self.take(2))[0]
    def u32(self): return struct.unpack(self.e + 'I', self.take(4))[0]
    def s32(self): return struct.unpack(self.e + 'i', self.take(4))[0]
    def f32(self): return struct.unpack(self.e + 'f', self.take(4))[0]

    def cstr(self):
        q = self.b.find(b'\0', self.p)
        if q < 0:
            raise ChunkError(f'unterminated string at {self.p}')
        s = self.b[self.p:q].decode('latin-1')
        self.p = q + 1
        return s

    def pstr(self):
        return self.take(self.u8()).decode('latin-1')

    def magic(self, m):
        got = self.take(4)
        if got != m:
            raise ChunkError(f'expected {m!r}, found {got!r}')

    def done(self):
        rest = self.b[self.p:]
        if rest.strip(b'\0\xa3\xcd'):
            raise ChunkError(f'{len(rest)} bytes left over: {rest[:16].hex(" ")}')


# ---- strings: STR#, CTSS, TTAs (and the v3.0 games' equivalents) ----

def strings(b, end='<'):
    r = R(b, end)
    fmt = r.s16()
    out = []
    if fmt == 0:          # Pascal strings
        for _ in range(r.u16()):
            out.append({'value': r.pstr()})
    elif fmt == -1:       # C strings
        for _ in range(r.u16()):
            out.append({'value': r.cstr()})
    elif fmt == -2:       # pairs of C strings
        for _ in range(r.u16()):
            out.append({'value': r.cstr(), 'comment': r.cstr()})
    elif fmt == -3:       # a language code, then a value and a comment
        for _ in range(r.u16()):
            out.append({'lang': r.u8(), 'value': r.cstr(), 'comment': r.cstr()})
    elif fmt == -4:       # sets by language
        for _ in range(r.u8()):
            for _ in range(r.u16()):
                out.append({'lang': r.u8(), 'value': r.pstr(), 'comment': r.pstr()})
    else:
        raise ChunkError(f'string format {fmt}')
    r.done()
    return {'format': fmt, 'strings': out}


# ---- BHAV: a behaviour tree ----

def bhav(b, end='<'):
    """Header 12 bytes: signature (0x8000-0x8002), node count, tree type, argument count,
    local count (16 bits), tree version (32 bits); then 12 bytes a node: opcode, true and
    false targets, 8 operand bytes (kept raw: their fields' order depends on the opcode)."""
    r = R(b, end)
    sig = r.u16()
    if not 0x8000 <= sig <= 0x8002:
        raise ChunkError(f'BHAV signature {sig:#x}')
    n = r.u16()
    h = {'signature': sig, 'type': r.u8(), 'args': r.u8(), 'locals': r.u16(), 'version': r.u32()}
    nodes = []
    for _ in range(n):
        nodes.append({'opcode': r.u16(), 'true': r.u8(), 'false': r.u8(), 'operand': r.take(8)})
    r.done()
    h['nodes'] = nodes
    return h


# ---- BCON: constants ----

def bcon(b, end='<'):
    r = R(b, end)
    n = r.u8()
    flags = r.u8()
    vals = [r.s16() for _ in range(n)]
    r.done()
    return {'flags': flags, 'values': vals}


# ---- GLOB: the semi-global file an object's scripts may call into ----

def glob(b, end='<'):
    r = R(b, end)
    # Pascal string, padded; some writers give a C string instead.
    if b and b[0] and b[0] + 1 <= len(b) and all(32 <= c < 127 for c in b[1:1 + b[0]]):
        name = r.pstr()
    else:
        name = r.cstr()
    r.done()
    return {'name': name}


# ---- OBJf: an object's function table (a check and an action tree per entry) ----

def objf(b, end='<'):
    r = R(b, end)
    r.u32()
    version = r.u32()
    r.magic(b'fJBO' if end == '<' else b'OBJf')
    n = r.u32()
    fns = [{'guard': r.u16(), 'action': r.u16()} for _ in range(n)]
    r.done()
    return {'version': version, 'functions': fns}


# ---- OBJD: an object definition, kept as its 16-bit fields ----

def objd(b, end='<'):
    """Version (32 bits), then 16-bit fields; their names are to be checked against how the
    engine reads them before they are given (plan: chunk 5)."""
    r = R(b, end)
    version = r.u32()
    fields = []
    while r.left() >= 2:
        fields.append(r.u16())
    r.done()
    return {'version': version, 'fields': fields}


# ---- TTAB: an object's interactions ----

class FieldBits:
    """The packed fields of TTAB versions 9 and up: a field is 0 (bit 0), or bit 1, a
    two-bit width code and that many bits, signed (widths 5, 8, 13, 16 for 16-bit fields,
    6, 11, 21, 32 for 32-bit ones); bits are read from the top of each byte."""
    W16 = (5, 8, 13, 16)
    W32 = (6, 11, 21, 32)

    def __init__(self, r):
        self.r, self.cur, self.bit = r, r.u8(), 0

    def bitv(self):
        if self.cur is None:
            raise ChunkError('packed fields run past the end')
        v = (self.cur >> (7 - self.bit)) & 1
        self.bit += 1
        if self.bit == 8:
            self.bit = 0
            self.cur = self.r.u8() if self.r.left() else None
        return v

    def bits(self, n):
        v = 0
        for _ in range(n):
            v = v << 1 | self.bitv()
        return v

    def field(self, big):
        if not self.bitv():
            return 0
        w = (self.W32 if big else self.W16)[self.bits(2)]
        v = self.bits(w)
        if v & (1 << (w - 1)):
            v -= 1 << w
        return v


MAX_MOTIVES = 64  # the games have 8-16; more means the data was misread


def ttab(b, end='<'):
    r = R(b, end)
    n = r.u16()
    if n * 8 > len(b):
        raise ChunkError(f'{n} interactions in {len(b)} bytes')
    if n == 0:  # possibly followed by a version and a packing code
        r.take(min(r.left(), 3))
        r.done()
        return {'version': None, 'interactions': []}
    version = r.u16()
    out = []
    if version < 9:
        # Versions 2-3: 16-bit count and flags, a delta a motive, no string index or
        # autonomy fields. 4-6: 32-bit fields, no attenuation code, a delta a motive.
        # 7-8: the attenuation code, and a minimum, delta and personality a motive.
        for _ in range(n):
            e = {'action': r.u16(), 'guard': r.u16()}
            if version <= 3:
                motives = r.u16()
                e['flags'] = r.u16()
                e['tta_index'] = len(out)
            else:
                motives = r.u32()
                e['flags'] = r.u32()
                e['tta_index'] = r.u32()
            if motives > MAX_MOTIVES:
                raise ChunkError(f'{motives} motive entries')
            if version > 6:
                e['attenuation_code'] = r.u32()
            e['attenuation_value'] = r.f32()
            if version > 3:
                e['autonomy_threshold'] = r.u32()
                e['joining_index'] = r.s32()
            if version > 6:
                e['motives'] = [{'min': r.s16(), 'delta': r.s16(), 'personality': r.u16()} for _ in range(motives)]
            else:
                e['motives'] = [{'min': 0, 'delta': r.s16(), 'personality': 0} for _ in range(motives)]
            out.append(e)
        r.done()
    else:
        # Versions 9-11: the fields packed (FieldBits), in the order of versions 7-8;
        # version 11 opens with one more field for the whole table.
        code = r.u8()
        if code != 1:
            raise ChunkError(f'TTAB packing code {code}')
        f = FieldBits(r)
        table_field = f.field(False) if version >= 11 else None
        for _ in range(n):
            e = {'action': f.field(False) & 0xFFFF, 'guard': f.field(False) & 0xFFFF}
            motives = f.field(True)
            if not 0 <= motives <= MAX_MOTIVES:
                raise ChunkError(f'{motives} motive entries')
            e['flags'] = f.field(True) & 0xFFFFFFFF
            e['tta_index'] = f.field(True)
            e['attenuation_code'] = f.field(True)
            e['attenuation_value'] = struct.unpack('<f', struct.pack('<i', f.field(True)))[0]
            e['autonomy_threshold'] = f.field(True)
            e['joining_index'] = f.field(True)
            e['motives'] = [{'min': f.field(False), 'delta': f.field(False), 'personality': f.field(False) & 0xFFFF}
                            for _ in range(motives)]
            out.append(e)
        rest = r.b[r.p:]
        if rest.strip(b'\0\xa3\xcd'):
            raise ChunkError(f'{len(rest)} bytes after the packed fields')
        if table_field is not None:
            return {'version': version, 'table_field': table_field, 'interactions': out}
    return {'version': version, 'interactions': out}


# ---- the editor's chunks: TPRP (labels), TREE (layout), TRCN (constant labels) ----

def _header(r, magic):
    r.u32()
    version = r.u32()
    r.magic(magic)
    return version


def tprp(b, end='<'):
    r = R(b, end)
    version = _header(r, b'PRPT' if end == '<' else b'TPRP')
    np_, nl = r.u32(), r.u32()
    params = [r.cstr() for _ in range(np_)]
    locals_ = [r.cstr() for _ in range(nl)]
    flags = r.take(r.left())  # per-parameter flags and a version byte, editor only
    return {'version': version, 'params': params, 'locals': locals_, 'rest': len(flags)}


def tree(b, end='<'):
    r = R(b, end)
    version = _header(r, b'EERT' if end == '<' else b'TREE')
    return {'version': version, 'boxes': r.u32()}


def trcn(b, end='<'):
    r = R(b, end)
    version = _header(r, b'NCRT' if end == '<' else b'TRCN')
    return {'version': version, 'labels': r.u32()}


def slot(b, end='<'):
    """Routing slots: header, then a slot each, of a size given by the version."""
    r = R(b, end)
    version = _header(r, b'TOLS' if end == '<' else b'SLOT')
    n = r.u32()
    size = r.left()
    if n and size % n:
        raise ChunkError(f'{size} bytes for {n} slots')
    return {'version': version, 'slots': n, 'slot_size': size // n if n else 0}


READERS = {
    'STR#': strings, 'CTSS': strings, 'TTAs': strings,
    'BHAV': bhav, 'BCON': bcon, 'GLOB': glob, 'OBJf': objf, 'OBJD': objd, 'TTAB': ttab,
    'TPRP': tprp, 'TREE': tree, 'TRCN': trcn, 'SLOT': slot,
}
