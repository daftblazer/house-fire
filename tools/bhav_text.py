#!/usr/bin/env python3
"""The behaviour scripts as text, and the checks on them.

Usage: bhav_text.py [--dump] IMAGE...

For each disc: every BHAV is read and checked (its node targets are nodes of the tree or a
return; its primitives are ones the game's interpreter accepts; a call to the object's own
trees finds the tree in the same file), and the opcodes used are counted. With --dump the
scripts are written as text to work/scripts/<disc id>/ (not part of the repository).
Exits non-zero if a check fails.
"""
import sys, collections, pathlib, hashlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from disc import Disc
import chunks, primitives
from check_chunks import iff_files, byte_order

ROOT = pathlib.Path(__file__).resolve().parent.parent
RET = {253: 'false', 254: 'true', 255: 'error'}

# Primitives a game's scripts use that its interpreter no longer accepts: dead scripts.
KNOWN_DEAD = {
    ('G4OE69', 54): 'GbaCommunication: Pets dropped the Game Boy Advance link; one script still names it',
}


def target(t, n):
    return RET.get(t) or (str(t) if t < n else None)


def text(game, cid, label, b):
    out = [f'tree {cid:#06x} "{label}": {len(b["nodes"])} nodes, {b["args"]} args, {b["locals"]} locals, '
           f'type {b["type"]}, signature {b["signature"]:#06x}']
    for i, nd in enumerate(b['nodes']):
        out.append(f'  {i:3d}  {primitives.name(game, nd["opcode"]):26s} {nd["operand"].hex(" ")}'
                   f'   -> {target(nd["true"], len(b["nodes"])) or nd["true"]} / {target(nd["false"], len(b["nodes"])) or nd["false"]}')
    return '\n'.join(out)


def main(argv):
    dump = '--dump' in argv
    imgs = [a for a in argv if a != '--dump']
    failures = 0
    for img in imgs:
        d = Disc(img)
        game = d.id
        if game not in primitives.DISPATCH:
            print(f'== {game}: no dispatch table known, skipped'); continue
        ok_ops = primitives.accepted(game)
        seen = set()
        ops = collections.Counter(); calls = collections.Counter()
        bad_target = bad_op = bad_private = trees = from_semiglobal = 0
        dead = collections.Counter()
        ex = {}
        outdir = ROOT / 'work' / 'scripts' / game
        for name, data, cs in iff_files(d):
            end = byte_order(data, cs)
            bh = {}
            for t, cid, fl, lab, o, sz in cs:
                if t == 'BHAV':
                    bh[cid] = (lab, chunks.bhav(data[o:o + sz], end))
            key = hashlib.sha1(b''.join(data[o:o + sz] for t, cid, fl, lab, o, sz in cs)).digest()
            if not bh or key in seen:
                continue  # the same file is in more than one archive
            seen.add(key)
            lines = []
            for cid, (lab, b) in sorted(bh.items()):
                trees += 1
                n = len(b['nodes'])
                for i, nd in enumerate(b['nodes']):
                    op = nd['opcode']
                    if op < 256:
                        ops[op] += 1
                        if op not in ok_ops:
                            if (game, op) in KNOWN_DEAD:
                                dead[op] += 1
                            else:
                                bad_op += 1; ex.setdefault('opcode', (name, lab, i, op))
                    else:
                        kind = 'global' if op < 4096 else 'private' if op < 8192 else 'semiglobal'
                        calls[kind] += 1
                        if kind == 'private' and op not in bh:
                            if cid >= 8192:   # in a semi-global file: the calling object's tree
                                from_semiglobal += 1
                            else:
                                bad_private += 1; ex.setdefault('private', (name, lab, i, hex(op)))
                    for t in (nd['true'], nd['false']):
                        if target(t, n) is None:
                            bad_target += 1; ex.setdefault('target', (name, lab, i, t, n))
                if dump:
                    lines.append(text(game, cid, lab, b))
            if dump:
                p = outdir / (name.replace('/', '_') + f'.{key.hex()[:8]}.txt')
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text('\n\n'.join(lines) + '\n')
        print(f'== {game} {d.title}: {len(seen)} files, {trees} trees, {sum(ops.values())} primitive nodes, '
              f'calls {dict(calls)}')
        print(f'   bad targets {bad_target}, primitives the game does not accept {bad_op}, '
              f'calls to a missing own tree {bad_private}')
        print(f'   own-tree calls from semi-global trees (resolved in the caller): {from_semiglobal}; '
              f'known dead primitives: {dict(dead)}')
        for k, v in ex.items():
            print(f'   e.g. {k}: {v}')
        print('   opcodes used: ' + ' '.join(f'{op}:{c}' for op, c in sorted(ops.items())))
        never = sorted(ok_ops - set(ops))
        print(f'   accepted but never used: {never}')
        failures += bad_target + bad_op + bad_private
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
