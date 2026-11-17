use std::collections::{BTreeMap, BTreeSet};

use crate::bhav::Node;
use crate::interp::{Interpreter};
use crate::library::Library;
use crate::tree_log::Log;
use crate::world::World;
use super::study::has;
use super::counts::Counts;

/// The log's stacks: which object each is, from the stack's own variable lookups (scopes 0, 3,
/// 8) and the logged object lists.
pub(super) fn stack_ids(log: &Log) -> BTreeMap<u32, i16> {
    let mut ptr_id: BTreeMap<u32, i16> = BTreeMap::new();
    for l in &log.lists {
        for (i, &o) in l.objects.iter().enumerate() {
            ptr_id.insert(o, l.ids[i]);
        }
    }
    let mut own: BTreeMap<u32, BTreeMap<u32, i32>> = BTreeMap::new();
    for s in &log.steps {
        for v in &s.lookups {
            if matches!(v.scope, 0 | 3 | 8) {
                *own.entry(s.sim).or_default().entry(v.object).or_default() += 1;
            }
        }
    }
    let mut out = BTreeMap::new();
    for (sim, c) in &own {
        // std::max_element: the first of the most.
        let mut best: Option<(&u32, &i32)> = None;
        for e in c {
            if best.is_none_or(|b| b.1 < e.1) {
                best = Some(e);
            }
        }
        if let Some(&id) = best.and_then(|b| ptr_id.get(b.0)) {
            out.insert(*sim, id);
        }
    }
    out
}

/// The stacks that never read their own variables, placed: each is a lot object not placed yet,
/// between the placed ones it runs after and before in each tick (turns are in id order); a
/// group of stacks that can be the same objects, as many as they, takes them in the order the
/// group runs in a tick. Repeated until nothing more is placed.
pub(super) fn place_quiet_stacks(log: &Log, world: &World, ids: &mut BTreeMap<u32, i16>, counts: &mut Counts, lib: &Library) {
    let mut free: BTreeSet<i16> = world.objects().map(|o| o.id()).collect();
    for id in ids.values() {
        free.remove(id);
    }
    let mut cand: BTreeMap<u32, BTreeSet<i16>> = BTreeMap::new();
    let mut first: BTreeMap<u32, usize> = BTreeMap::new();
    let mut order: BTreeMap<u32, Vec<u32>> = BTreeMap::new(); // tick -> stacks in their order
    for (i, s) in log.steps.iter().enumerate() {
        first.entry(s.sim).or_insert(i);
        let o = order.entry(s.tick).or_default();
        if !o.contains(&s.sim) {
            o.push(s.sim);
        }
    }
    for sim in first.keys() {
        if !ids.contains_key(sim) {
            cand.insert(*sim, free.clone());
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for o in order.values() {
            for j in 0..o.len() {
                if ids.contains_key(&o[j]) {
                    continue;
                }
                let Some(c) = cand.get_mut(&o[j]) else { continue };
                let mut lo = 0i32;
                let mut hi = 1i32 << 20;
                for k in &o[..j] {
                    if let Some(&id) = ids.get(k) {
                        lo = lo.max(id as i32);
                    }
                }
                for k in &o[j + 1..] {
                    if let Some(&id) = ids.get(k) {
                        hi = hi.min(id as i32);
                    }
                }
                c.retain(|&x| (x as i32) > lo && (x as i32) < hi);
            }
        }
        for (sim, c) in cand.iter_mut() {
            if ids.contains_key(sim) {
                continue;
            }
            c.retain(|x| free.contains(x));
            if c.len() == 1 {
                let id = *c.iter().next().unwrap();
                ids.insert(*sim, id);
                free.remove(&id);
                changed = true;
            }
        }
        if changed {
            continue;
        }
        // A group of stacks with the same candidates, as many as they: in id order, as they take
        // their turns in a tick where all of them run.
        let mut groups: BTreeMap<Vec<i16>, Vec<u32>> = BTreeMap::new();
        for (sim, c) in &cand {
            if !ids.contains_key(sim) && c.len() > 1 {
                groups.entry(c.iter().copied().collect()).or_default().push(*sim);
            }
        }
        for (c, gr) in &groups {
            if gr.len() != c.len() {
                continue;
            }
            for o in order.values() {
                let seen: Vec<u32> = o.iter().copied().filter(|s| gr.contains(s)).collect();
                if seen.len() != gr.len() {
                    continue;
                }
                for (sim, &id) in seen.iter().zip(c) {
                    ids.insert(*sim, id);
                    free.remove(&id);
                }
                changed = true;
                break;
            }
        }
    }
    if has("REPLAY_SHOW_QUIET") {
        let mut n = 0;
        for (sim, c) in &cand {
            if !ids.contains_key(sim) {
                n += 1;
                if n <= 8 {
                    let f = &log.steps[first[sim]];
                    println!("unplaced stack: first step {}:{} at tick {}, {} candidates", f.tree, f.node, f.tick, c.len());
                }
            }
        }
        for sim in cand.keys() {
            if let Some(&id) = ids.get(sim) {
                let f = &log.steps[first[sim]];
                let same = world.find_object(id).and_then(|o| o.stack.last()).is_some_and(|b| {
                    b.tree.is_some_and(|t| lib.tree_at(t).id == f.tree as i16) && b.node == f.node
                });
                counts.note(if same { "stacks: placed, first step the saved top frame" } else { "stacks: placed, first step not the saved top frame" });
            }
        }
        let mut n = 0;
        for &id in &free {
            if let Some(o) = world.find_object(id)
                && let Some(b) = o.stack.last()
            {
                n += 1;
                if n <= 8 {
                    println!(
                        "free object {id}: saved top {}:{} ({} frames)",
                        b.tree.map_or(-1, |t| lib.tree_at(t).id as i32),
                        b.node,
                        o.stack.len()
                    );
                }
            }
        }
    }
    let (mut placed, mut left) = (0, 0);
    for sim in cand.keys() {
        if ids.contains_key(sim) {
            placed += 1;
        } else {
            left += 1;
        }
    }
    counts.add("stacks: placed by their own variables", ids.len() - placed);
    counts.add("stacks: placed by turn order", placed);
    counts.add("stacks: not placed", left);
}

/// The top frame's node of an interpreter's stack, if it has one.
pub(super) fn top_node(lib: &Library, i: &Interpreter) -> Option<Node> {
    let f = i.stack().last()?;
    lib.tree_at(f.tree?).nodes.get(f.node as usize).copied()
}
