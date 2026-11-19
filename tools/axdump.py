"""Reads a GCN_AXDUMP file (the recomp runtime's ax.cpp: the AX mixer's inputs and frames) and
prints a summary: records by kind, frames, voices running a frame, ARAM uploaded, the output's
peak. Usage: axdump.py DUMP"""
import struct, sys, collections

def records(path):
    """(tag, fields) of each record of the dump, in order; a last record cut short (the run ends
    on a signal) is left out."""
    b = open(path, 'rb').read()
    p = 0
    while p < len(b):
        try:
            rec, p = _record(b, p)
        except struct.error:
            return
        yield rec

def _record(b, p):
    """The record at `p` and the offset after it (struct.error if it is cut short)."""
    t = b[p:p + 1]; p += 1
    def take(n):
        if p + n > len(b): raise struct.error('cut short')
        return b[p:p + n]
    if t == b'F':
        return ('F', struct.unpack_from('<I', b, p)), p + 4
    if t == b'C':
        n, = struct.unpack_from('<H', b, p); p += 2
        return ('C', struct.unpack_from('<%dH' % n, b, p)), p + 2 * n
    if t in (b'B', b'A'):
        a, n = struct.unpack_from('<II', b, p); p += 8
        return (t.decode(), (a, take(n))), p + n
    if t == b'V':
        a, = struct.unpack_from('<I', b, p); p += 4
        return ('V', (a, struct.unpack_from('<93H', b, p))), p + 186
    if t == b'O':
        lr = struct.unpack_from('<320h', b, p)
        sur = struct.unpack_from('<160i', b, p + 640)
        return ('O', (lr, sur)), p + 1280
    raise ValueError(f'unknown record {t!r} at {p - 1}')

def main(path):
    kinds = collections.Counter(); cmds = collections.Counter(); running = collections.Counter()
    aram = 0; peak = 0; nonzero = 0; frame_running = 0
    for t, f in records(path):
        kinds[t] += 1
        if t == 'C': cmds[f[0]] += 1
        elif t == 'V':
            if f[1][7]: frame_running += 1
        elif t == 'A': aram += len(f[1])
        elif t == 'O':
            m = max(abs(x) for x in f[0]); peak = max(peak, m); nonzero += m > 0
            running[frame_running] += 1; frame_running = 0
    print('records', dict(kinds))
    print('commands', {f'{k:02X}': v for k, v in sorted(cmds.items())})
    print('frames by running voices', dict(sorted(running.items())))
    print(f'ARAM uploaded {aram} bytes; output peak {peak}, frames not silent {nonzero}')

if __name__ == '__main__':
    main(sys.argv[1])
