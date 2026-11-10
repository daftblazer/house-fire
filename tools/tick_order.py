#!/usr/bin/env python3
"""Which objects' tree stacks the recompiled Bustin' Out runs each tick, and in what order
(docs/simulation.md), from its BUSTIN_TREE_LOG.

Usage: tick_order.py LOG

A step's 'sim' is the object's simulation part, not the object: it is tied to its object by
the object's own variable lookups (scopes 0, 3 and 8) in its steps, and the object to its id
by the logged object lists. Per tick, a run is a stretch of steps of one stack. The first run
of each stack in a tick is its turn in the pass, unless the engine started it: the stack had
yielded (its last step stayed on its node) and the run starts a tree at node 0 on a frame
above that one, with no call (the pie menu's checks, run on a Sim's stack).

Prints: the stacks and how many have an id; the ticks whose turns are in id order; the
later runs and the trees they start with; and whether a tick's pass ever starts after where
the previous one ended (a pass the original stopped partway and went on with).
"""
import sys, collections, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import tree_log


def main(argv):
    own = collections.defaultdict(collections.Counter)
    ptr_id = {}
    pending, tick = [], None
    runs = collections.defaultdict(list)  # tick -> [(sim, first tree, engine-started)]
    last = {}  # sim -> (frame, node, node after) of its last step in its own turns
    in_engine = {}  # sim -> whether its current run is one the engine started
    for k, v in tree_log.records(argv[1]):
        if k == 'K': tick = v
        elif k == 'L':
            for p, i in v[1]: ptr_id[p] = i
        elif k == 'V':
            if v['scope'] in (0, 3, 8): pending.append(v['object'])
        elif k == 'S':
            for p in pending: own[v['sim']][p] += 1
            pending = []
            if tick is None: continue
            r = runs[tick]
            if not r or r[-1][0] != v['sim']:
                # Started by the engine: the stack had yielded (its last step stayed on its
                # node), and this run starts a tree at node 0 on a frame above it.
                lf = last.get(v['sim'])
                engine = bool(lf) and lf[1] == lf[2] and v['frame'] > lf[0] and v['node'] == 0
                r.append((v['sim'], v['tree'], engine))
                in_engine[v['sim']] = engine
            if not in_engine[v['sim']]: last[v['sim']] = (v['frame'], v['node'], v['after'])
    sim_id = {s: ptr_id.get(c.most_common(1)[0][0]) for s, c in own.items()}
    print(f'{len(own)} stacks, {sum(1 for c in own.values() if len(c) == 1)} with one own object, '
          f'{sum(i is not None for i in sim_id.values())} with an id')
    res = collections.Counter()
    later = collections.Counter()
    nested = []
    prev = None
    for t in sorted(runs):
        seen, turns = set(), []
        for s, first, engine in runs[t]:
            if engine:
                later[first] += 1
                continue
            if s in seen:
                res['a stack run twice in its tick'] += 1
                continue
            seen.add(s)
            if sim_id.get(s) is not None: turns.append(sim_id[s])
        if not turns: continue
        ordered = all(a < b for a, b in zip(turns, turns[1:]))
        res['ticks with turns in id order' if ordered else 'ticks with turns out of id order'] += 1
        if not ordered: nested.append((t, turns[:6]))
        if prev and turns[0] > prev[-1]: res['passes going on from the last tick'] += 1
        prev = turns
    print(dict(res))
    print('runs the engine started, by first tree:', later.most_common(8))
    print('out of order (first ones):', nested[:6])


if __name__ == '__main__':
    main(sys.argv)
