"""The engine's save streams: ReconBuffer (The Sims 2's names; Bustin' Out has the same).

Everything the engine saves goes through one stream: lots' objects (ObjM), families, the
neighbourhood, the memory card's saves. Read from The Sims 2's code (ReconBuffer::Recon8 /
16 / 32 / Float / Bool / CmprInt / Bits / Mark, and the schemes its static initialiser
builds):

- Bits are read from the top of each byte.
- With compression on (EnableCompression), a value is 0 (bit 0), or bit 1, a two-bit code
  and that many bits, sign-extended. The widths by code: 8-bit values 2, 4, 6, 8; 16-bit
  5, 8, 13, 16; 32-bit 6, 11, 21, 32. Floats go as their 32 bits; a bool as an 8-bit value.
- Without compression, values are plain bytes (big-endian, or swapped).
- A mark pads to the next byte and is a plain 32-bit word, written back with the length
  of what follows it, so that a reader can skip a part it does not know.
"""
import struct

SCHEMES = {8: (2, 4, 6, 8), 16: (5, 8, 13, 16), 32: (6, 11, 21, 32)}


class ReconError(ValueError):
    pass


class Reader:
    def __init__(self, data, pos=0, little=True):
        self.d, self.pos, self.mask, self.compressed, self.little = data, pos, 0, False, little

    # -- bits (the compressed form) --
    def bit(self):
        if self.mask == 0:
            if self.pos >= len(self.d): raise ReconError('stream ends')
            self.cur = self.d[self.pos]; self.pos += 1; self.mask = 128
        b = 1 if self.cur & self.mask else 0
        self.mask >>= 1
        return b

    def bits(self, n):
        v = 0
        for _ in range(n): v = v << 1 | self.bit()
        return v

    def pad(self):
        self.mask = 0

    def cmpr(self, size):
        if not self.bit(): return 0
        w = SCHEMES[size][self.bits(2)]
        v = self.bits(w)
        if v & (1 << (w - 1)): v -= 1 << w
        return v

    # -- values --
    def _raw(self, n, fmt):
        self.pad()
        if self.pos + n > len(self.d): raise ReconError('stream ends')
        v = struct.unpack_from(('<' if self.little else '>') + fmt, self.d, self.pos)[0]
        self.pos += n
        return v

    def r8(self): return self.cmpr(8) if self.compressed else self._raw(1, 'b')
    def r16(self): return self.cmpr(16) if self.compressed else self._raw(2, 'h')
    def r32(self): return self.cmpr(32) if self.compressed else self._raw(4, 'i')
    def rbool(self): return self.r8() != 0

    def rfloat(self):
        u = self.r32() & 0xFFFFFFFF
        return struct.unpack('<f', struct.pack('<I', u))[0]

    def mark(self):
        """A mark: the length of what follows it (a plain word)."""
        return self._raw(4, 'I')
