#!/usr/bin/env python3
"""Read every chunk of the types tools/chunks.py knows on each disc; count what fails.
Usage: check_chunks.py IMAGE..."""
import sys, collections, pathlib, struct
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import iff, chunks

SKIP = ('audiostr', 'movies', 'samples', 'textures', 'models', 'animatio', '.tpl', '.bnr', '.elf', '.map', '.log', '.nd')

def iff_files(d):
    for name, (off, size) in sorted(d.files.items()):
        if any(s in name.lower() for s in SKIP) or size > 200 << 20: continue
        data = d.read(name)
        starts = list(iff.find_all(data))
        for i, s in enumerate(starts):
            yield name, data, iff.chunks(data, s, starts[i + 1] if i + 1 < len(starts) else None)

def byte_order(data, chunks_):
    for t, cid, fl, lab, o, sz in chunks_:
        if t == 'BHAV':
            return '<' if data[o + 1] == 0x80 else '>'
    return '<'

def main():
    total = 0
    for img in sys.argv[1:]:
        d = Disc(img)
        ok, bad, ex = collections.Counter(), collections.Counter(), {}
        tta_bad = 0
        for name, data, cs in iff_files(d):
            end = byte_order(data, cs)
            parsed = {}
            for t, cid, fl, lab, o, sz in cs:
                fn = chunks.READERS.get(t)
                if not fn: continue
                try:
                    parsed[(t, cid)] = fn(data[o:o + sz], end); ok[t] += 1
                except (chunks.ChunkError, struct.error, UnicodeDecodeError) as e:
                    bad[t] += 1; ex.setdefault(t, (name, cid, lab, sz, str(e)))
            # each interaction's string is in the TTAs of the same id
            for (t, cid), v in parsed.items():
                if t == 'TTAB':
                    names = parsed.get(('TTAs', cid))
                    for e in v['interactions']:
                        if names is not None and not 0 <= e['tta_index'] < len(names['strings']): tta_bad += 1
        print(f'== {d.id} {d.title}')
        for t in sorted(set(ok) | set(bad)):
            print(f'   {t}: {ok[t]} read, {bad[t]} failed' + (f'   e.g. {ex[t]}' if t in ex else ''))
        print(f'   interactions naming a string their TTAs does not have: {tta_bad}')
        total += sum(bad.values()) + tta_bad
    return 1 if total else 0

if __name__ == '__main__':
    sys.exit(main())
