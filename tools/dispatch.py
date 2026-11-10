#!/usr/bin/env python3
"""The opcodes each game's interpreter accepts, read from its executable.

Usage: dispatch.py IMAGE...   (prints DISPATCH entries for tools/primitives.py)

The switches are `cXObjectImpl::TryElement` and `cXPersonImpl::TryElement` (names from The
Sims 2's map; the others found by the same switch, and by their code)."""
import sys, pathlib, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import ppc

# disc id: (object switch, its size, person switch, its size)
SWITCHES = {
    'G4ME69': (0x8021242C, 0x107C, 0x802246B0, 0x210),
    'GUBE69': (0x80165964, 0x444, 0x80180E1C, 0x2C0),
    'G4ZE69': (0x80109770, 0x47C, 0x80124ADC, 0x244),
    'G4OE69': (0x800EC050, 0x600, 0x80105D48, 0x2C0),
    'R42E69': (0x80154C1C, 0x388, 0x800C8744, 0x18C),  # Castaway (Wii): from its ELF's symbols
}


def real_cases(M, a, size):
    """The switch's cases without those that land on its default (the target most cases
    share: a value narrowed to by the search but not handled). A switch compiled as a jump
    table (Castaway's) is read from its table."""
    jt = ppc.jump_table_cases(M, a, a + size)
    if jt:
        targets = collections.Counter(jt.values())
        default, n = targets.most_common(1)[0]
        return sorted(v for v, t in jt.items() if not (n >= 3 and t == default))
    c = ppc.switch_cases(M, a, a + size)
    targets = collections.Counter(c.values())
    default, n = targets.most_common(1)[0] if targets else (None, 0)
    return sorted(v for v, t in c.items() if not (n >= 3 and t == default))


def cases(img):
    d = Disc(img)
    oa, os_, pa, ps = SWITCHES[d.id]
    M = ppc.load_dol(d.dol(), data=True)
    return d.id, real_cases(M, oa, os_), real_cases(M, pa, ps)


if __name__ == '__main__':
    for img in sys.argv[1:]:
        gid, o, p = cases(img)
        print(f"    '{gid}': {{'object': {o},\n               'person': {p}}},")
