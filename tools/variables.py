#!/usr/bin/env python3
"""The variable scopes, from a log of the recompiled game's variable lookups.

Usage: variables.py LOG

The log (BUSTIN_TREE_LOG, with 'V' records) has every lookup the original's primitives made:
the object it was made for, the scope and datum, the value, and the address the variable
was found at (16-bit or float). For each scope this prints:

- how often it is looked up, by which primitives (the caller), and how often the lookup fails;
- where the variable is: its address relative to the object asked (a fixed offset plus the
  datum times a size, if one fits every lookup), or relative to another object;
- whether its value changes between the scripts' own accesses: a read that differs from the
  last value read or written there means the engine changed it by itself;
- which other scopes reach the same addresses.
"""
import sys, struct, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from tree_log import records

# The PC game's names for the scopes (FreeSO's VMVariableScope), as working labels only.
PC_NAMES = {
    0: 'my attribute', 1: "stack object's attribute", 2: "target's attribute", 3: 'my object data',
    4: "stack object's data", 5: "target's data", 6: 'global', 7: 'literal', 8: 'temp',
    9: 'parameter', 10: 'stack object id', 11: 'temp by temp', 12: 'check tree ad range',
    13: "stack object's temp", 14: 'my motive', 15: "stack object's motive", 16: "stack object's slot",
    17: "stack object's motive by temp", 18: 'my person data', 19: "stack object's person data",
    20: 'my slot', 21: "stack object's definition", 22: "stack object's attribute by parameter",
    23: 'room by temp 0', 24: 'neighbour in stack object', 25: 'local', 26: 'constant',
    27: 'dynamic sprite flag of stack object', 28: 'tree ad personality', 29: 'tree ad min',
    30: 'my person data by temp', 31: "stack object's person data by temp", 32: 'neighbour',
    33: 'job data', 34: 'neighbourhood data',
}


def main(argv):
    lookups = []
    for k, v in records(argv[1]):
        if k == 'V': lookups.append(v)
    print(f'{len(lookups)} lookups')
    objects = sorted({v['object'] for v in lookups})

    by_scope = collections.defaultdict(list)
    for v in lookups: by_scope[v['scope']].append(v)

    # Where each address is reached from: (scope, datum) pairs.
    reached = collections.defaultdict(set)
    for v in lookups:
        a = v['ptr'] or v['fptr']
        if a: reached[a].add(v['scope'])

    # Values over time at each address, for the reads (V) and the expressions' writes (E).
    last = {}
    lhs_addr = e_float = None
    changed = collections.Counter(); kept = collections.Counter()
    for k, v in records(argv[1]):
        if k == 'V':
            a = v['ptr'] or v['fptr']
            if not a: continue
            val = v['fvalue'] if v['fptr'] and not v['ptr'] else v['value']
            if a in last:
                (kept if last[a] == val else changed)[v['scope']] += 1
            last[a] = val
        elif k == 'E':
            # An expression wrote its left side: the value after is the last known one. Its
            # left lookup is the 'V' just before; that address now holds the value after.
            if lhs_addr is not None and e_float is not None:
                last[lhs_addr] = v['lhs_float_after'] if e_float else v['lhs_after']
        if k == 'V':
            lhs_addr = (v['ptr'] or v['fptr']) or None
            e_float = bool(v['fptr'] and not v['ptr'])

    for scope in sorted(by_scope):
        vs = by_scope[scope]
        found = [v for v in vs if v['flags'] & 1]
        kinds = collections.Counter('float' if v['flags'] & 4 and not v['flags'] & 2 else '16-bit' if v['flags'] & 2 else 'value only' for v in found)
        callers = collections.Counter(hex(v['lr']) for v in vs)
        # Address relative to the object asked: offset = base + size * datum?
        fits = collections.Counter()
        offs = collections.defaultdict(set)
        for v in found:
            a = v['ptr'] or v['fptr']
            if not a: continue
            offs[v['data']].add(a - v['object'])
        line = ''
        if offs:
            samples = sorted((d, min(o)) for d, o in offs.items())
            same_off = all(len(o) == 1 for o in offs.values())
            if len(samples) >= 2:
                (d0, o0), (d1, o1) = samples[0], samples[-1]
                size = (o1 - o0) // (d1 - d0) if d1 != d0 else 0
                base = o0 - size * d0
                ok = all(o == base + size * d for d, o in samples) and same_off
                line = f'object + {base:#x} + {size} x datum' if ok else 'not a fixed table on the object'
            else:
                line = f'object + {samples[0][1]:#x}' if same_off else 'varies by object'
        others = collections.Counter()
        for v in found:
            a = v['ptr'] or v['fptr']
            for s2 in reached.get(a, ()):
                if s2 != scope: others[s2] += 1
        name = PC_NAMES.get(scope, '?')
        print(f'\nscope {scope} ({name}): {len(vs)} lookups, {len(vs) - len(found)} failed; {dict(kinds)}')
        print(f'  data: {sorted(offs)[:12]}{" ..." if len(offs) > 12 else ""} ({len(offs)} distinct)')
        if line: print(f'  where: {line}')
        print(f'  changed by the engine between accesses: {changed[scope]} of {changed[scope] + kept[scope]}')
        if others: print(f'  same addresses as scopes: {dict(others)}')
        print(f'  callers: {dict(callers.most_common(6))}')


if __name__ == '__main__':
    main(sys.argv)
