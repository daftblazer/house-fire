#!/usr/bin/env python3
"""A lot's objects (its ObjM chunk), decoded from the engine's save stream (tools/recon.py).

Usage: objm.py IMAGE

Layout, as Bustin' Out's loader (fn_801FDE80) reads it; traced on the recompiled game
(BUSTIN_RECON_LOG, tools/recon_trace.py):

- 12 bytes: 0, the version (80), "MjbO". Stream positions below count from after them.
- A bool: compression on.
- The object table, in the world's object list order (the order SetToNext walks), each
  entry: object id (16), two 32-bit values, a count (8) and that many 32-bit values, the
  object's type (16, streamed by fn_801F92D0). Ended by an object id of 0.
- A mark (a plain 32-bit word: the position of the next mark), then each object's section,
  ended by a mark. Each section begins with the object's header (fn_801F1E30: six 32-bit
  values, an int, a 16-bit value) and goes on with its data, its tree stack and, for Sims,
  the rest; the marks let a reader step from object to object. After the last object's
  section comes one more: the lot's own.

section() reads an object's section up to the end of the base object's part: header, tables,
tree stack and the rest (docs/lots.md, "An object's section").

For every lot this reads the table and walks the mark chain, which must step forward and
end at the stream's end, with one section per object of the table and one more.
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import lot, recon

START = 12


def read(m):
    if m[8:12] != b'MjbO': raise ValueError('not an ObjM')
    r = recon.Reader(m, START)
    r.compressed = r.rbool()
    table = []
    while True:
        oid = r.r16()
        if oid == 0: break
        a, b, n = r.r32(), r.r32(), r.r8()
        extra = [r.r32() for _ in range(n & 0xFF)]
        table.append(dict(id=oid, a=a, b=b, extra=extra, type=r.r16()))
    sections = []
    mark_at = r.pos - START
    nxt = r.mark()
    while True:
        start = mark_at + 4
        if nxt <= mark_at or START + nxt + 4 > len(m):
            sections.append((start, len(m) - START))  # the last: to the stream's end
            break
        sections.append((start, nxt))
        mark_at = nxt
        nxt = struct.unpack_from('<I', m, START + nxt)[0]
    return r.compressed, table, sections


def header(m, start):
    """A section's object header (fn_801F1E30)."""
    r = recon.Reader(m, START + start)
    r.compressed = True
    return [r.r32() for _ in range(6)] + [r.r32(), r.r16()]


VERSION = 80  # ObjM's version on the disc; the conditions below are the loader's


def frame(r, v):
    """A tree stack frame (fn_8023E9A8)."""
    f = {}
    if v > 25: f['stack_object'] = r.r16()
    f['tree'] = r.r16()
    f['node'] = r.r16()
    nlocals, nparams = r.r8() & 0xFF, r.r8() & 0xFF
    f['params'] = [r.r16() for _ in range(nparams)]
    f['locals'] = [r.r16() for _ in range(nlocals)]
    f['engine'] = r.r32()  # +8: set for a frame the engine started
    if v > 46: f['file'] = r.r16()  # fn_801F961C: an index into the game's file list
    return f


def tree_stack(r, v):
    """The object's tree stack (fn_8023F928): from version 24, a frame count, a capacity,
    then the frames, bottom first."""
    if v < 24: raise ValueError('old tree stacks are not read')
    n, capacity = r.r32(), r.r32()
    if not 0 <= n <= 64: raise ValueError(f'a tree stack of {n} frames')
    return dict(capacity=capacity, frames=[frame(r, v) for _ in range(n)])


def int_list(r):
    """fn_80233084: a count, then that many 32-bit values."""
    n = r.r32()
    if not 0 <= n <= 4096: raise ValueError(f'a list of {n}')
    return [r.r32() for _ in range(n)]


def slot_entry(r, v):
    """fn_802326CC."""
    e = {'value': r.r32()} if v > 0 else {}
    e['list'] = int_list(r)
    return e


def slots(r, v):
    """The object's +192 member (fn_8023243C, then fn_80232E2C on its +36). Its meaning is
    not known yet; on the disc's lots `a` is -1 and both lists are empty or short."""
    out = {'a': r.r32()}
    if out['a'] < 0:
        if v > 78: out['c'] = r.r32()
        n = r.r32()
        if not 0 <= n <= 4096: raise ValueError(f'{n} slot entries')
        out['entries'] = [slot_entry(r, v) if r.r32() else None for _ in range(n)]  # an int: present
    else:
        out['entries'] = [slot_entry(r, v) for _ in range(out['a'])]
    return out


def section(m, start, v=VERSION):
    """An object's section up to the end of the base object's part (fn_801F1F20). The class's
    own part (a Sim's motives and so on) follows, then an int, then the next mark."""
    r = recon.Reader(m, START + start)
    r.compressed = True
    o = {'header': [r.r32() for _ in range(6)] + [r.r32(), r.r16()]}
    n = r.r16()
    o['attributes'] = [r.r16() for _ in range(n)]
    o['temps'] = [r.r16() for _ in range(8)]
    o['data'] = [r.r16() for _ in range(72)]
    r.r16()  # always 0
    o['stack'] = tree_stack(r, v)
    if v > 4: o['slots'] = slots(r, v if v > 6 else 0)
    if v > 4:  # fn_801F22D0: a count of 36-byte entries, two 16-bit values each
        n = r.r16()
        o['pairs'] = [(r.r16(), r.r16()) for _ in range(n)]
    if v > 65: o['b'] = r.r32()
    if v > 26:
        n = r.r16()
        o['shorts'] = [r.r16() for _ in range(n)]
    o['base_end'] = r.pos - START
    o['_pos'], o['_mask'], o['_cur'] = r.pos, r.mask, getattr(r, 'cur', 0)
    return o


def empty_list(r, what):
    """A counted list the disc's Sims never fill (fn_8022933C, fn_8022989C, fn_80229D5C):
    its elements are not read yet."""
    n = r.r32()
    if n: raise ValueError(f'{what}: {n} elements')


def queue_entry(r, v):
    """An 88-byte entry of a Sim's queue (fn_801CFB68)."""
    e = {'a': r.r32(), 'id': r.r32(), 'objects': [r.r16() for _ in range(3)],  # fn_801FDD0C x3
         'b': r.r32(), 'shorts': [r.r16() for _ in range(4)], 'c': r.r32(), 'd': r.r16(),
         'e': r.rfloat()}
    if v > 61: e['flags'] = r.r32()
    else: raise ValueError('old queue entries are not read')
    return e


def sim_part(r, v):
    """A Sim's own part (fn_80224C88), for the versions after 63."""
    if v <= 63: raise ValueError('old Sims are not read')
    o = {}
    # The +1136 member (fn_800AD6E8): a count of (bool, 32-bit) pairs, then five values.
    n = r.r16()
    o['member'] = [(r.rbool(), r.r32()) for _ in range(n)]
    o['member_rest'] = [r.rbool(), r.rfloat(), r.r32(), r.r32(), r.r32()]
    o['motives'] = [r.rfloat() for _ in range(16)]       # +168
    o['motives_b'] = [r.rfloat() for _ in range(16)]     # +232
    o['person_data'] = [r.r16() for _ in range(80 if v > 40 else 64)]  # +8
    empty_list(r, 'the +1160 list')
    o['current'] = queue_entry(r, v)                     # +1032
    n = r.r32()
    if not 0 <= n <= 8: raise ValueError(f'a queue of {n}')
    o['queue'] = [queue_entry(r, v) for _ in range(n)]   # a ring of 8 at +320
    empty_list(r, 'the +1180 list')
    empty_list(r, 'the +1120 list')
    return o


def part_part(r, v):
    """A multi-tile object's part (fn_801D3F50)."""
    o = {'a': r.r32(), 'b': r.r32()}
    if v > 57: o['c'] = r.r32()
    if v > 39: o['offset'] = [r.r32() for _ in range(3)]
    o['d'] = r.r16()
    return o


def door_part(r, v):
    """A multi-tile door's part (fn_8022DE18): a part's, then a count and that many floats."""
    o = part_part(r, v)
    if v > 14:
        n = r.r32()
        if not 0 <= n <= 4096: raise ValueError(f'{n} door values')
        o['values'] = [r.rfloat() for _ in range(n)] if v > 37 else [r.r32() for _ in range(n)]
    return o


def object_class(objd_fields):
    """The class the loader makes an object of, from its definition (OBJD) fields: no
    definition on the disc, a Sim; a master id (field 8), a part of a multi-tile object,
    and a door's (type, field 7, 8) part if a door; otherwise the plain object."""
    if objd_fields is None: return 'sim'
    if objd_fields[8]: return 'door' if objd_fields[7] == 8 else 'part'
    return 'object'


CLASS_PARTS = {'sim': sim_part, 'part': part_part, 'door': door_part, 'object': None}


def whole_section(m, start, end, cls, v=VERSION):
    """An object's whole section: the base part, the class's, then the loader's int, which
    must end at the section's mark (end)."""
    o = section(m, start, v)
    r = recon.Reader(m, START + o['base_end'])
    r.compressed = True
    # base_end is a byte position; the base part may end inside a byte
    r.pos, r.mask, r.cur = o['_pos'], o['_mask'], o['_cur']
    f = CLASS_PARTS[cls]
    if f: o[cls] = f(r, v)
    o['loader_int'] = r.r32()
    r.pad()
    if r.pos - START != end: raise ValueError(f'ends at {r.pos - START}, not {end}')
    return o


def main(argv):
    d = Disc(argv[1])
    res = collections.Counter()
    for off, cs in lot.lots(d):
        m = cs['ObjM']
        comp, table, sections = read(m)
        ok = len(sections) == len(table) + 1  # one per object, then the lot's own part
        res['lots'] += 1
        res['sections match the table'] += ok
        types = lot.object_types(cs['objt'])
        by4 = {f[4] for g, f, n in types}
        res['types found as objt field 4'] += sum(1 for t in table if t['type'] in by4)
        res['types found as objt position'] += sum(1 for t in table if 0 <= t['type'] < len(types))
        res['objects'] += len(table)
        print(f'lot {off:#08x}: {len(table)} objects, {len(sections)} sections '
              f'({"same" if ok else "DIFFERENT"}), first ids {[t["id"] for t in table[:6]]}, '
              f'first header {header(m, sections[0][0])}')
    print(dict(res))


if __name__ == '__main__':
    main(sys.argv)
