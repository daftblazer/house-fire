"""Which IFF chunk types each disc carries, and where. Usage: survey.py IMAGE..."""
import sys, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import iff

SKIP = ('audiostr', 'movies', 'samples', 'textures', 'models', 'animatio', '.tpl', '.bnr', '.elf', '.map', '.log', '.nd')

for img in sys.argv[1:]:
    d = Disc(img)
    types = collections.Counter(); where = collections.Counter(); nfiles = 0
    bhav_sig = collections.Counter()
    for name, (off, size) in sorted(d.files.items()):
        if any(s in name.lower() for s in SKIP) or size > 200 << 20: continue
        data = d.read(name)
        starts = list(iff.find_all(data))
        for i, s in enumerate(starts):
            nfiles += 1
            for t, cid, fl, lab, o, sz in iff.chunks(data, s, starts[i + 1] if i + 1 < len(starts) else None):
                types[t] += 1; where[name.split('/')[0] if name.startswith('Runtime') else name] += 1
                if t == 'BHAV' and sz >= 2: bhav_sig[data[o:o + 2].hex()] += 1
    print(f'== {d.id} {d.title}: {nfiles} IFF files')
    print('   in:', dict(where.most_common(8)))
    print('   chunks:', ' '.join(f'{t}={c}' for t, c in types.most_common(40)))
    print('   BHAV first bytes:', dict(bhav_sig.most_common(6)))
