use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Instant;

use crate::bhav::{TreeKind, tree_kind};
use crate::cfmt::g;
use crate::interp::{Error, Frame, Host, Step};
use crate::primitives::make_of_guid;
use crate::simulator::{Simulator};
use crate::tree_log::{Log, PassRec, same_shape};
use crate::world::{DATA_OBJECT_ID, Object, ObjectKey, World};
use crate::FileId;
use super::study::{env, has, atoi, file_name};
use super::counts::us;
use super::host::{ReplayHost};
use super::stacks::top_node;
use super::clock::{check_time_of_day};
use super::run::Replay;

impl<'l> Replay<'l> {
    /// Runs the next tick (the log's, from 1), and says whether there was one. The world is then
    /// the engine's at the end of that tick (`world`).
    pub fn step(&mut self) -> bool {
        if self.t >= self.log.ticks {
            return false;
        }
        self.t += 1;
        let t = self.t;
        let log_rc = Rc::clone(&self.log);
        let log: &Log = &log_rc;
        let lib = self.lib;
        let (use_native_places, new_observer, have_passes) = (self.use_native_places, self.new_observer, self.have_passes);
        let (mut place_at, mut motive_at, mut shown_motives, mut engine_list_at, mut time_at, mut streak, mut best_streak) = (self.place_at, self.motive_at, self.shown_motives, self.engine_list_at, self.time_at, self.streak, self.best_streak);
        let Self { h, sim, consumed, finder, unstarted, ids, native_places, by_tick, frames, first_seen, first_step, last_frame, unseen, delivered, frame_tree, engine_run, ev_env, .. } = self;
        'tick: {
            let mut tick_us = 0.0;
            while place_at < log.places.len() && log.places[place_at].tick < t {
                let p = log.places[place_at];
                // a walking Sim's place from the native walk where the routing pass has one
                let (x, y) = if use_native_places { native_places.get(&(p.id, p.tick)).copied().unwrap_or((p.x, p.y)) } else { (p.x, p.y) };
                if use_native_places {
                    if !native_places.contains_key(&(p.id, p.tick)) {
                        let person = h.world.find(p.id).is_some_and(|k| h.world.get(k).person);
                        if p.tick == 0 {
                            let loaded = h.world.find(p.id).map(|k| { let o = h.world.get(k); (o.x16, o.y16, o.level) });
                            h.counts.note(if loaded == Some((p.x, p.y, p.level)) {
                                "places: a tick-0 placement natively where the lot loads it"
                            } else if loaded.is_some_and(|(x, y, l)| (((x >> 4) << 4) + 8, ((y >> 4) << 4) + 8, l) == (p.x, p.y, p.level)) {
                                "places: a tick-0 placement natively the centre of the tile the lot loads it on"
                            } else if loaded.is_some_and(|(x, y, _)| (x < 0 || y < 0) && (p.x, p.y) == (-16, -16)) {
                                "places: a tick-0 placement natively (-16, -16), the place of an object off the lot"
                            } else if loaded.is_none() {
                                "places: a tick-0 placement of an object not in the native lot"
                            } else {
                                "places: a tick-0 placement NOT where the lot loads it"
                            });
                            if has("REPLAY_SHOW_PLACES") && !loaded.is_some_and(|(x, y, l)| (((x >> 4) << 4) + 8, ((y >> 4) << 4) + 8, l) == (p.x, p.y, p.level)) {
                                println!("tick-0 place of {}: logged {:?}, loaded {loaded:?}", p.id, (p.x, p.y, p.level));
                            }
                        }
                        h.counts.note(&if p.tick == 0 {
                            "places: the log's, no native walk: a tick-0 placement".to_string()
                        } else if !person {
                            if has("REPLAY_SHOW_PLACES") {
                                let file = h.world.find(p.id).and_then(|k| h.world.get(k).file).map_or("-".to_string(), |f| file_name(lib, f));
                                println!("moved object {} {file} at tick {} to {:?}", p.id, p.tick, (p.x, p.y, p.level));
                            }
                            "places: the log's, no native walk: not a person".to_string()
                        } else {
                            if has("REPLAY_SHOW_PLACES") {
                                println!("person place without a native walk: {} at tick {} {:?}", p.id, p.tick, (p.x, p.y));
                            }
                            format!("places: the log's, no native walk: person {}", p.id)
                        });
                    }
                    h.counts.note(if !native_places.contains_key(&(p.id, p.tick)) {
                        "places: the log's (no native walk there)"
                    } else if (x, y) == (p.x, p.y) {
                        "places: the native walk's, the same as the log's"
                    } else {
                        if has("REPLAY_SHOW_WALK_MISS") {
                            let prev = log.places.iter().rfind(|q| q.id == p.id && q.tick < p.tick).map(|q| (q.tick, q.x, q.y));
                            let nxt = log.places.iter().find(|q| q.id == p.id && q.tick > p.tick).map(|q| (q.tick, q.x, q.y));
                            let nat_prev = native_places.get(&(p.id, p.tick.saturating_sub(1))).copied();
                            let nat_next = native_places.get(&(p.id, p.tick + 1)).copied();
                            println!("walk miss {} tick {}: native {:?}, log {:?}; log before {prev:?} after {nxt:?}; native at t-1 {nat_prev:?} t+1 {nat_next:?}", p.id, p.tick, (x, y), (p.x, p.y));
                        }
                        "places: the native walk's, NOT the log's"
                    });
                }
                if let Some(k) = h.world.find(p.id) {
                    let o = h.world.get_mut(k);
                    o.x16 = x;
                    o.y16 = y;
                    o.level = p.level;
                }
                place_at += 1;
            }
            let sync_before = h.bench.as_ref().map_or(0.0, |b| b.sync_us);
            // A tick without a pass is a paused one: the native clock does not move in it.
            if have_passes && !log.passes.contains_key(&t) {
                h.counts.note("ticks: paused (no pass)");
                for _ in &by_tick[t as usize] {
                    h.counts.note("steps: outside the pass (the engine's own), not run");
                }
                break 'tick;
            }
            h.counts.note("ticks: run");
            h.now = sim.now();
            // The turns: the native simulator's against the lot objects the log stepped.
            let mut logged_turns: Vec<i16> = Vec::new();
            let mut seen: BTreeSet<u32> = BTreeSet::new();
            for &i in &by_tick[t as usize] {
                if engine_run[i] {
                    continue;
                }
                let s = &log.steps[i];
                let Some(&id) = ids.get(&s.sim) else { continue };
                if h.world.find(id).is_none() || !seen.insert(s.sim) {
                    continue;
                }
                logged_turns.push(id);
            }
            // The native pass: from its cursor (against where the log's started), over the objects
            // the log's reached (to where it stopped), sleepers passed over.
            let pass: PassRec = if have_passes { log.passes[&t] } else { PassRec::default() };
            if have_passes {
                h.counts.note(if sim.cursor() == pass.from as i16 { "passes: the native cursor where the log's pass starts" } else { "passes: DIFFERENT start" });
            }
            let mut native_turns: Vec<i16> = Vec::new();
            // The persons reached, in id order, and whether their turn ends with the motive update.
            let mut motives_due: Vec<(ObjectKey, bool)> = Vec::new();
            let mut motives_done = 0;
            let p0 = Instant::now();
            for (k, asleep) in sim.pass_order(&h.world) {
                let id = h.world.get(k).id();
                if pass.stopped_at != 0 && id > pass.stopped_at as i16 {
                    break;
                }
                if h.world.get(k).person {
                    let due = sim.begin_turn(&mut h.world, k);
                    motives_due.push((k, due));
                }
                if asleep {
                    sim.pass_over(&h.world, k);
                } else {
                    native_turns.push(id);
                }
            }
            if let Some(b) = &mut h.bench {
                let d = us(p0);
                b.pass_us += d;
                tick_us += d;
            }
            // Their updates, at the end of their turns: before the first step of an object with a
            // higher id (the pass goes up by id), or at the pass's end.
            let motives_before = |id: i16, sim: &Simulator, world: &mut World, done: &mut usize| {
                while *done < motives_due.len() {
                    // an object removed during its turn has no update to finish
                    let Some(o) = world.get_live(motives_due[*done].0) else {
                        *done += 1;
                        continue;
                    };
                    if o.id() >= id {
                        break;
                    }
                    sim.end_turn(world, motives_due[*done].0, motives_due[*done].1);
                    *done += 1;
                }
            };
            let (mut a, mut b) = (logged_turns.clone(), native_turns.clone());
            a.sort();
            b.sort();
            if a == b {
                h.counts.note("turns: the same objects");
            } else {
                let only_log: Vec<i16> = a.iter().copied().filter(|x| b.binary_search(x).is_err()).collect();
                let only_native: Vec<i16> = b.iter().copied().filter(|x| a.binary_search(x).is_err()).collect();
                h.counts.note("turns: different objects");
                h.counts.add("turns: objects the log ran, not the native", only_log.len());
                h.counts.add("turns: objects the native would run, not the log", only_native.len());
                for &id in &only_native {
                    let k = h.world.find(id).unwrap();
                    let i = sim.stack_of(&h.world, k);
                    let op = top_node(lib, i).map_or("?".to_string(), |n| n.opcode.to_string());
                    let slept = if i.sleep().woke() { " (slept)" } else { "" };
                    if has("REPLAY_SHOW_ONLY_NATIVE") && h.shown.next("only native", 6) {
                        println!("only native {id}: stack {:?} done {} error {:?}", i.stack().iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node)).collect::<Vec<_>>(), i.done(), i.error());
                    }
                    note!(h.counts, "turns: the native would run, at opcode {op}{slept}");
                }
                let show = has("REPLAY_SHOW_TURNS");
                let shown = *h.shown.0.get("turns").unwrap_or(&0);
                if show && shown < 10 {
                    for &id in &only_log {
                        let k = h.world.find(id).unwrap();
                        let i = sim.stack_of(&h.world, k);
                        println!("  {id}: done {} asleep {} depth {} error {}", i.done() as i32, i.asleep() as i32, i.stack().len(), i.error() as u16);
                    }
                }
                if show && h.shown.next("turns", 10) {
                    let mut l = format!("tick {t} turns differ: only the log's");
                    for id in &only_log {
                        l += &format!(" {id}");
                    }
                    l += "; only the native's";
                    for id in &only_native {
                        l += &format!(" {id}");
                    }
                    println!("{l}");
                }
            }

            // objects the original's engine made that no step of theirs made first: from the list
            // at the end of the tick before, placed by the log's places
            if !has("REPLAY_LOGGED_OBJECTS") {
                while engine_list_at < log.lists.len() && log.lists[engine_list_at].tick < t {
                    let l = &log.lists[engine_list_at];
                    engine_list_at += 1;
                    for &id in l.ids.iter().rev() {
                        if h.world.find(id).is_none() {
                            let mut o = Object::new();
                            o.data = vec![0; 72];
                            o.data[DATA_OBJECT_ID] = id;
                            (o.x16, o.y16) = log.places.iter().rfind(|p| p.id == id && p.tick < t).map_or((-16, -16), |p| (p.x, p.y));
                            h.world.add(o, true);
                            h.counts.note("objects: made by the engine, from the log's list");
                        }
                    }
                }
            }
            // The steps, in the log's order.
            for &i in &by_tick[t as usize] {
                let s = &log.steps[i];
                let traced = has("REPLAY_TRACE_MAKERS") && matches!((s.tree, s.node), (0x2007, 1) | (0x2022, 1) | (0x200f, 6)) && s.look == b'X';
                if traced {
                    println!("maker step {i} tick {} tree {:#x} node {}: engine's {}, consumed {}, id {:?}", s.tick, s.tree, s.node, engine_run[i], consumed.contains(&i), ids.get(&s.sim));
                }
                if engine_run[i] {
                    h.counts.note("steps: outside the pass (the engine's own), not run");
                    continue;
                }
                if consumed.contains(&i) {
                    continue;
                }
                if let Some(&id0) = ids.get(&s.sim) {
                    motives_before(id0, sim, &mut h.world, &mut motives_done);
                }
                {
                    // A Sim waiting at IdleForInput with an entry queued, whose next logged steps are
                    // the engine's checks on a frame above: the IdleForInput step (the log's next
                    // 281:4 of this Sim) is run natively, its checks stepping through those steps.
                    let w = ids.get(&s.sim).and_then(|&id| h.world.find(id));
                    if let Some(w) = w.filter(|&w| h.world.get(w).person && s.node == 0) {
                        // The evaluation (opcode 3 at the native top): its checks are this Sim's next
                        // logged steps up to the opcode-3 step itself (its tree and node, looked up).
                        let ein = sim.stack_of(&h.world, w);
                        let en = if ein.done() { None } else { top_node(lib, ein) };
                        let etop = ein.stack().last().and_then(|f| f.tree.map(|t| (lib.tree_at(t).id, f.node, f.file)));
                        // Native on every lot (the club's too since its guard limit, person data
                        // 36: docs/simulation.md, "Autonomy"). REPLAY_EVALUATION=0 turns it off.
                        let evaluation = ev_env.as_ref().is_none_or(|e| e.starts_with('1'));
                        if let (true, Some(en), Some((etree, enode, efile))) = (evaluation, en, etop)
                            && (en.opcode == 3 || (en.opcode == 14 && !has("REPLAY_LOGGED_FIND_BEST")))
                            && !(s.tree == etree as u16 && s.node == enode)
                        {
                            let mut k = i;
                            let mut nest: Vec<usize> = Vec::new();
                            for &j in &by_tick[t as usize] {
                                if j < i || log.steps[j].sim != s.sim {
                                    continue;
                                }
                                // The opcode-3 step itself: its tree and node, in a frame below its
                                // checks' (a guard can have the same tree id in another file: the
                                // bed's 8233).
                                if log.steps[j].tree == etree as u16 && log.steps[j].node == enode && log.steps[j].frame < s.frame {
                                    k = j;
                                    break; // (its lookup is its last check's)
                                }
                                nest.push(j);
                            }
                            // The checks of an object the native world does not have (made during the
                            // session: the replay does not make objects natively yet) are set aside:
                            // from its check's start in the bottom frame to the next check's start.
                            {
                                let bottom = log.steps[i].frame;
                                let mut kept = Vec::new();
                                let mut skipping = false;
                                let mut skipped = 0;
                                for &j in &nest {
                                    let ns = &log.steps[j];
                                    if ns.frame == bottom && ns.node == 0 {
                                        skipping = h.world.find(ns.stack_object).is_none();
                                    }
                                    if skipping {
                                        skipped += 1;
                                        consumed.insert(j);
                                        continue;
                                    }
                                    kept.push(j);
                                }
                                if skipped != 0 {
                                    h.counts.add("autonomy: an evaluation's steps of objects not in the native world, set aside", skipped);
                                }
                                nest = kept;
                            }
                            if k != i && (nest.len() >= 20 || en.opcode == 14) {
                                h.present.clear();
                                for l in &log.lists {
                                    if l.tick <= t {
                                        h.present = l.ids.iter().copied().collect();
                                    }
                                }
                                h.nested = nest.clone();
                                h.nested_pos = 0;
                                h.step = k;
                                h.file_of_top = efile;
                                h.me = w;
                                let ein = sim.stack_of(&h.world, w);
                                ein.reset_iterations();
                                ein.step(&mut *h);
                                let all_used = h.nested_pos == nest.len();
                                h.counts.note(if all_used {
                                    "autonomy: the evaluation used exactly the log's steps of it"
                                } else {
                                    "autonomy: the evaluation did NOT use the log's steps of it"
                                });
                                if !all_used && has("REPLAY_SHOW_AUTONOMY") {
                                    println!("evaluation of {} at tick {t}: used {} of {} steps", h.world.get(w).id(), h.nested_pos, nest.len());
                                }
                                consumed.extend(nest.iter().copied());
                                consumed.insert(k);
                                h.nested.clear();
                                continue;
                            }
                        }
                    }
                    // The engine checking a target's interactions on a Sim's stack (the player's menu
                    // of an object or the Sim itself, fn_80219A74's list; docs/simulation.md, "A Sim's
                    // own interactions re-checked"): the target's table's first tree, then, forward,
                    // the guards of the interactions flagged 0x400 (all, if the first tree passes),
                    // each with the target as stack object, above what the Sim runs (not over its
                    // IdleForInput or evaluation, whose checks are theirs)
                    if let Some(w) = w.filter(|&w| h.world.get(w).person && s.node == 0 && !has("REPLAY_NO_SELF_CHECKS"))
                        && let Some(target) = h.world.find(s.stack_object)
                    {
                        let o = h.world.get(target);
                        let (ofile, oid) = (o.file, o.id());
                        let tab = lib.table_of(ofile, o.person, h.world.definition(o));
                        let win = sim.stack_of(&h.world, w);
                        let below = win.stack().last().and_then(|f| f.tree).map(|t| lib.tree_at(t).id);
                        let below_op = if win.done() { None } else { top_node(lib, win).map(|n| n.opcode) };
                        // a table with no first tree starts at its first guard, and runs them all (the
                        // "go here" marker's six at tree.log's 741 and 1684)
                        let entry = match (tab.first, tab.file, tab.table) {
                            (0, Some(tf), Some(tid)) => lib.table(tf, tid).iter().find(|e| e.guard != 0 && e.flags & 0x80 == 0).map_or(0, |e| e.guard),
                            (f, _, _) => f,
                        };
                        if entry != 0 && s.tree == entry && below != Some(entry as i16) && !matches!(below_op, Some(3 | 17)) && let (Some(tf), Some(tid)) = (tab.file, tab.table) {
                            let nest: Vec<usize> = by_tick[t as usize].iter().copied().filter(|&j| j >= i && log.steps[j].sim == s.sim).take_while(|&j| log.steps[j].frame >= s.frame).collect();
                            let defaults = win.defaults().to_vec();
                            h.nested = nest.clone();
                            h.nested_pos = 0;
                            h.me = w;
                            let (mut same, mut diff) = (0, 0);
                            let want_of = |h: &mut ReplayHost, tree: u16| -> Option<bool> { lib.tree(ofile, tree).and_then(|t| h.run_check(t, ofile, oid, &defaults)) };
                            // once for each of the target's parts (a dance floor's 16 tiles: the
                            // first tree 16 times, on the leader), its own if it has none
                            let parts = h.world.objects().filter(|p| p.leader == oid && !p.person).count().max(1);
                            let mut first = None;
                            let mut checks = 0;
                            for _ in 0..parts {
                                if tab.first != 0 {
                                    first = want_of(&mut *h, tab.first);
                                    checks += 1;
                                }
                                for e in lib.table(tf, tid) {
                                    if e.guard == 0 || (first == Some(false) && e.flags & 0x400 == 0) || e.flags & 0x80 != 0 {
                                        continue;
                                    }
                                    if h.nested_pos >= h.nested.len() {
                                        break;
                                    }
                                    let _ = want_of(&mut *h, e.guard);
                                    checks += 1;
                                }
                            }
                            let all = h.nested_pos == nest.len();
                            if all { same += checks; } else { diff += 1; }
                            h.counts.add("self checks: run natively, all the log's steps used", same.min(1) * checks);
                            if diff != 0 {
                                h.counts.note("self checks: run natively, NOT the log's steps");
                                if has("REPLAY_SHOW_SELF_CHECKS") {
                                    println!("self checks: tick {t} object {oid} used {} of {} steps, {checks} checks, first {first:?}", h.nested_pos, nest.len());
                                }
                            }
                            h.counts.note("self checks: a run");
                            if has("REPLAY_SHOW_SELF_RUNS") {
                                let below_node = sim.stack_of(&h.world, w).stack().last().map(|f| f.node);
                                println!("self run: tick {t} object {oid} below {below:?}:{below_node:?} first {first:?} checks {checks}");
                            }
                            consumed.extend(nest.iter().copied());
                            h.nested.clear();
                            continue;
                        }
                    }
                    // The player's command (input the log does not record, as the dialogs' answers): a
                    // Sim at its IdleForInput with nothing queued natively, whose log runs checks and
                    // then, from 281 node 4, an interaction's tree a frame up on another object: that
                    // interaction queued as the player's (World::queue_player_command), for the native
                    // IdleForInput to deliver with its checks (the home log's 1579: Sim 237's "go here",
                    // the marker 197's 4108 check, then its 4107)
                    if let Some(w) = w.filter(|&w| h.world.get(w).person && h.world.get(w).queue.is_empty() && s.node == 0 && s.stack_object != h.world.get(w).id()) {
                        let win = sim.stack_of(&h.world, w);
                        let at_idle = !win.done() && top_node(lib, win).is_some_and(|n| n.opcode == 17);
                        let idle = by_tick[t as usize].iter().copied().find(|&j| j > i && log.steps[j].sim == s.sim && log.steps[j].tree == 281 && log.steps[j].node == 4);
                        let action = idle.and_then(|k| log.steps[k + 1..].iter().find(|x| x.sim == s.sim)).filter(|x| idle.is_some_and(|k| x.node == 0 && x.frame > log.steps[k].frame));
                        if has("REPLAY_SHOW_PLAYER_COMMAND") {
                            println!("player command? tick {t} sim {} at_idle {at_idle} idle {idle:?} action {:?} so {}", h.world.get(w).id(), action.map(|a| (a.tree, a.stack_object)), s.stack_object);
                            if let Some(o) = h.world.find_object(s.stack_object) {
                                let tab = lib.table_of(o.file, o.person, h.world.definition(o));
                                println!("  target file {:?} def {:?} table {:?} {:?} first {}", o.file.map(|f| lib.file(f).path.clone()), h.world.definition(o).map(|d| d.interaction_table()), tab.file.map(|f| lib.file(f).path.clone()), tab.table, tab.first);
                            }
                        }
                        // a target the player's click just made (the "go here" marker, a stand-in with
                        // no file yet): the file of the check run on it (a file of one definition)
                        if at_idle
                            && action.is_some_and(|a| a.stack_object == s.stack_object)
                            && let Some(tk) = h.world.find(s.stack_object)
                            && h.world.get(tk).file.is_none()
                            && let Some((_, cfile)) = finder.find(log, s)
                            && let [d] = &lib.file(cfile).definitions[..]
                            && let Some(def) = lib.definition(d.guid())
                        {
                            let o = h.world.get_mut(tk);
                            o.guid = d.guid();
                            o.definition = Some(def);
                            o.file = Some(cfile);
                            o.attributes = vec![0; lib.def(def).field(56) as usize];
                            unstarted.insert(tk);
                            h.counts.note("objects: a stand-in the player's command targets given its check's file");
                        }
                        if at_idle && let Some(a) = action && a.stack_object == s.stack_object && h.world.queue_player_command(w, a.stack_object, a.tree) {
                            h.counts.note("queue: the player's command (from the log) queued for IdleForInput");
                        }
                    }
                    if let Some(w) = w.filter(|&w| h.world.get(w).person && !h.world.get(w).queue.is_empty() && s.node == 0) {
                        let win = sim.stack_of(&h.world, w);
                        let wn = if win.done() { None } else { top_node(lib, win) };
                        let wfile = win.stack().last().and_then(|f| f.file);
                        let mut k = i;
                        let mut nest = Vec::new();
                        for &j in &by_tick[t as usize] {
                            if j < i || log.steps[j].sim != s.sim {
                                continue;
                            }
                            if log.steps[j].tree == 281 && log.steps[j].node == 4 {
                                k = j;
                                break;
                            }
                            nest.push(j);
                        }
                        if wn.is_some_and(|n| n.opcode == 17) && k != i {
                            h.nested = nest.clone();
                            h.nested_pos = 0;
                            h.step = k;
                            h.file_of_top = wfile;
                            h.me = w;
                            let win = sim.stack_of(&h.world, w);
                            win.reset_iterations();
                            win.step(&mut *h);
                            h.counts.note(if h.nested_pos == nest.len() {
                                "queue: the checks used exactly the log's steps of them"
                            } else {
                                "queue: the checks did NOT use the log's steps of them"
                            });
                            consumed.extend(nest.iter().copied());
                            consumed.insert(k);
                            h.nested.clear();
                            continue;
                        }
                    }
                }
                let fr = frames.entry(s.sim).or_default();
                if let Some(at) = fr.iter().position(|&f| f == s.frame) {
                    fr.truncate(at + 1);
                } else {
                    // A frame not seen yet: a call (above the others), or a return into one of the
                    // saved frames below the stack's first step, whose address the log had not shown
                    // (placeholders in `unseen`): the frames above it are gone, and it takes the
                    // place of the placeholder below them.
                    let mut popped = false;
                    // the first placeholder popped is the frame the return lands in (the one just
                    // below the frames that returned: placeholders' addresses are guesses, the
                    // frames' sizes vary)
                    let mut landed: Option<u32> = None;
                    while let Some(&b) = fr.last().filter(|&&b| b > s.frame) {
                        fr.pop();
                        popped = true;
                        if landed.is_none() && unseen.contains(&b) {
                            landed = Some(b);
                        }
                    }
                    if let Some(b) = landed {
                        unseen.remove(&b);
                        fr.push(s.frame);
                    } else if popped && fr.last().is_some_and(|b| unseen.contains(b)) {
                        unseen.remove(fr.last().unwrap());
                        *fr.last_mut().unwrap() = s.frame;
                    } else {
                        fr.push(s.frame);
                    }
                }
                first_seen.entry(s.sim).or_insert(t);
                // a step of an object its maker's step (later in the log) makes: the object made now
                // (its GUID the maker's), if the original's list has it at the tick's end
                if !has("REPLAY_LOGGED_OBJECTS")
                    && let Some(&id) = ids.get(&s.sim)
                    && h.world.find(id).is_none()
                    && log.lists.iter().find(|l| l.tick >= s.tick).is_some_and(|l| l.ids.contains(&id))
                    && let Some(guid) = by_tick[t as usize].iter().map(|&j| &log.steps[j]).find_map(|m| {
                        // the maker's step this tick: its stack object becomes the id, its node is
                        // opcode 42; the GUID in its operand
                        if m.stack_object_after != id || m.stack_object == id || m.look != b'X' {
                            return None;
                        }
                        let (tree, _) = finder.find(log, m)?;
                        let n = lib.tree_at(tree).nodes.get(m.node as usize).filter(|n| n.opcode == 42)?;
                        Some(u32::from_le_bytes([n.operand[0], n.operand[1], n.operand[2], n.operand[3]]))
                    }).or_else(|| {
                        // no maker this tick (the engine made it: the marker): the file its own
                        // step runs
                        let (_, file) = finder.find(log, s)?;
                        lib.file(file).definitions.first().map(|d| d.guid())
                    })
                    && let Some(k) = make_of_guid(&mut h.world, guid)
                {
                    let got = h.world.get(k).id();
                    h.counts.note(if got == id { "objects: made before its maker's step (its init's), the id the original's" } else { "objects: made before its maker's step, the id DIFFERENT" });
                    h.pre_made.insert(got);
                    h.present.insert(got);
                }
                let Some(ok) = ids.get(&s.sim).and_then(|&id| h.world.find(id)) else {
                    if has("REPLAY_SHOW_MADE") {
                        println!("not run: tick {} sim {:x} id {:?} tree {:#x}", s.tick, s.sim, ids.get(&s.sim), s.tree);
                    }
                    h.counts.note("steps: not a lot object's (not run)");
                    continue;
                };
                let (o_file, o_person, o_id) = {
                    let o = h.world.get(ok);
                    (o.file, o.person, o.id())
                };
                // A return step's nodes are its caller's (the lookup of where to go back to): its
                // tree is the frame's, from the frame's earlier steps.
                let key = (s.sim, s.frame, s.tree);
                let mut tf = if s.look == b'R' {
                    frame_tree.get(&key).copied()
                } else {
                    let r = finder.find(log, s);
                    if let Some(r) = r {
                        frame_tree.insert(key, r);
                    }
                    r
                };
                // Several files can hold a tree of the same id and shape (a Sim's template_person and
                // the character files): an own tree is taken from the object's file if it has it.
                if let (Some((_, tfile)), Some(of), Some(rec)) = (tf, o_file, log.tree_of(s))
                    && tree_kind(s.tree) == TreeKind::Own
                    && tfile != of
                    && rec.tree == s.tree
                    && let Some(&ot) = lib.file(of).trees.get(&s.tree)
                    && same_shape(lib.tree_at(ot), &rec.raw)
                {
                    tf = Some((ot, of));
                }
                // A step with no tree record (the observer writes one only for the node lookups it
                // hooks: a tree a CallNamedTree reached has none): the tree native's stack is in,
                // if it is that tree (its frame's file the one the call took it from). In logs of
                // today's observer only (its plans' 'r' records): the older logs' tree records miss
                // trees for another reason (its per-depth lookup they lack), native's not the log's.
                if tf.is_none() && s.t.is_none() && new_observer {
                    let i = sim.stack_of(&h.world, ok);
                    if let Some(f) = i.stack().last().filter(|f| f.tree.is_some_and(|t| lib.tree_at(t).id as u16 == s.tree))
                        && let (Some(t), Some(file)) = (f.tree, f.file)
                    {
                        tf = Some((t, file));
                        frame_tree.insert(key, (t, file));
                        h.counts.note("steps: no tree record, the tree native's stack has");
                    }
                }
                let Some((tree, file)) = tf else {
                    if has("REPLAY_TRACE_MAKERS") && matches!((s.tree, s.node), (0x2007, 1) | (0x2022, 1) | (0x200f, 6)) && s.look == b'X' {
                        println!("  maker step {i}: tree not found (tree record {:?}, the object's file {:?})", log.tree_of(s).map(|r| (r.tree, r.raw.len() / 12)), o_file.map(|f| &lib.file(f).path));
                    }
                    h.counts.note("steps: tree not found on the disc");
                    note!(h.counts, 
                        "steps: tree not found: {}{}",
                        if s.t.is_some() { "tree " } else { "no tree record, tree " },
                        s.tree
                    );
                    continue;
                };
                // A tree the engine runs on an object over its Idle (the counter's update of its
                // function scores, 8215 at ticks 165 and 844): pushed natively as an engine frame where
                // the log starts it, from node 0, in the object's own file (when the engine does it is
                // not native yet)
                if !o_person && s.node == 0 && !has("REPLAY_NO_ENGINE_FRAMES") && lib.tree(o_file, s.tree) == Some(tree) {
                    let i = sim.stack_of(&h.world, ok);
                    let top_idle = !i.done() && top_node(lib, i).is_some_and(|n| n.opcode == 0) && i.stack().last().and_then(|f| f.tree) != Some(tree);
                    if top_idle && i.push(lib, tree, &[]) {
                        i.top().engine = true;
                        i.top().file = Some(file);
                        i.top().stack_object = s.stack_object;
                        h.counts.note("frames: an engine frame pushed over an object's Idle (the log's tree)");
                    }
                }
                // A stand-in the engine made (no file yet: the club's "go here" markers) takes the
                // file and definition its first step's tree is in (a file of one definition) and
                // starts as a made object does (main and init, Simulator::stack_of)
                // A stand-in whose first step is a global tree (the club's markers: Global 280, "idle",
                // above their 4097): its file is that of its first own step below, and its stack the
                // calling frame (the node calling this tree whose return goes on to that step's node)
                // with this tree above
                if unstarted.remove(&ok) {
                    h.world.get_mut(ok).file = None; // its first own step: started as a stand-in is
                }
                if h.world.get(ok).file.is_none() && !has("REPLAY_NO_STANDIN_FILE") && tree_kind(s.tree) == TreeKind::Global {
                    let below = log.steps[i + 1..].iter().find(|x| x.sim == s.sim && x.frame < s.frame && tree_kind(x.tree) == TreeKind::Own);
                    if let Some(b) = below
                        && let Some((bt, bfile)) = finder.find(log, b)
                        && let [d] = &lib.file(bfile).definitions[..]
                        && let Some(def) = lib.definition(d.guid())
                        && let Some(c) = lib.tree_at(bt).nodes.iter().position(|n| n.opcode == s.tree && (n.true_target == b.node as u8 || n.false_target == b.node as u8))
                    {
                        let o = h.world.get_mut(ok);
                        o.guid = d.guid();
                        o.definition = Some(def);
                        o.file = Some(bfile);
                        o.attributes = vec![0; lib.def(def).field(56) as usize];
                        o.stack = vec![
                            Frame { tree: Some(bt), node: c as u16, params: b.params.clone(), locals: b.locals.clone(), stack_object: b.stack_object, file: Some(bfile), ..Default::default() },
                            Frame { tree: Some(tree), node: s.node, params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, file: Some(bfile), ..Default::default() },
                        ];
                        sim.forget(ok);
                        h.counts.note("objects: a stand-in first seen in a global tree, given its own step's file and the calling frame");
                    }
                }
                if h.world.get(ok).file.is_none() && !has("REPLAY_NO_STANDIN_FILE") {
                    let f = lib.file(file);
                    if let [d] = &f.definitions[..]
                        && let Some(def) = lib.definition(d.guid())
                    {
                        let o = h.world.get_mut(ok);
                        o.guid = d.guid();
                        o.definition = Some(def);
                        o.file = Some(file);
                        o.attributes = vec![0; lib.def(def).field(56) as usize];
                        // first seen mid-run (not at its init's first node: the club's markers 179,
                        // 180 and 181, an Idle returning into their 4097 at node 1): its start is
                        // not in the log, its frame the log's
                        let init = f.functions.values().next().and_then(|t| t.first()).map(|e| e.1);
                        if !(s.node == 0 && init == Some(s.tree)) {
                            o.stack = vec![Frame { tree: Some(tree), node: s.node, params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, file: Some(file), ..Default::default() }];
                            h.counts.note("objects: a stand-in first seen mid-run, its frame the log's");
                        }
                        sim.forget(ok);
                        h.counts.note("objects: a stand-in given its first step's file and started");
                    }
                }
                // study: the log's steps of one object in a tick range ("id:first:last")
                if let Some(v) = env("REPLAY_SHOW_LOG_STEPS") {
                    let p: Vec<i64> = v.split(':').map(atoi).collect();
                    if p.len() == 3 && (p[0] == o_id as i64 || p[0] == -1) && (p[1]..=p[2]).contains(&(t as i64)) {
                        let op = lib.tree_at(tree).nodes.get(s.node as usize).map_or(-1, |n| n.opcode as i32);
                        println!("log step: tick {t} object {o_id} {}:{} op {op} -> {} look {} res {} frame {:x} so {} params {:?}", s.tree, s.node, s.after, if s.look != 0 { s.look as char } else { '-' }, s.result, s.frame & 0xFFFF, s.stack_object, s.params);
                    }
                }
                if has("REPLAY_SHOW_MADE_DEFS") && matches!(o_id, 197 | 201 | 202) && h.shown.next(&format!("made def {o_id}"), 1) {
                    let o = h.world.get(ok);
                    if let Some(d) = h.world.definition(o) {
                        println!("made def {o_id} tick {t}: {:?} first step {}:{}; fields {:?}", o.file.map(|f| file_name(lib, f)), s.tree, s.node, (0..64).map(|i| d.field(i)).collect::<Vec<_>>());
                    }
                }
                if has("REPLAY_SHOW_STANDIN") && h.world.get(ok).file.is_none() && h.shown.next(&format!("standin {o_id}"), 6) {
                    println!("standin: tick {t} object {o_id} {}:{} -> {} file {} defs {:?}", s.tree, s.node, s.after, lib.file(file).path, lib.file(file).definitions.iter().map(|d| (d.guid(), (0..48).map(|i| d.field(i)).collect::<Vec<_>>())).collect::<Vec<_>>());
                }
                if has("REPLAY_FRAME_SIZES") {
                    // study: a call's frame address against the caller's: the size of a frame
                    if let Some(&(pf, pt)) = last_frame.get(&s.sim)
                        && s.frame > pf
                        && s.node == 0
                    {
                        let (ca, cl) = (lib.tree_at(pt).args, lib.tree_at(pt).locals);
                        let (ea, el) = (lib.tree_at(tree).args, lib.tree_at(tree).locals);
                        note!(h.counts, "study frame: delta {} caller args {ca} locals {cl} callee args {ea} locals {el}", s.frame - pf);
                    }
                    last_frame.insert(s.sim, (s.frame, tree));
                }
                if let Some(w) = env("REPLAY_SHOW_OPS") {
                    // study: the opcodes a stack runs in a tick range, "id:from:to"
                    let v: Vec<i64> = w.split(':').filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 3 && o_id as i64 == v[0] && (v[1]..=v[2]).contains(&(t as i64)) {
                        let op = lib.tree_at(tree).nodes.get(s.node as usize).map_or(-1, |n| n.opcode as i32);
                        note!(h.counts, "study ops: {}:{} op {op}", s.tree, s.node);
                    }
                }
                // The interaction queue: the first interaction pushed onto a waiting Sim (a tree at
                // node 0 above "Wait For Notify", Global 281, other than its checks) against the
                // first entry of the queue the lot saved for it.
                let before = (0..i).rev().map(|j| &log.steps[j]).find(|p| p.sim == s.sim);
                if o_person && s.node == 0 && before.is_some_and(|b| b.tree == 281 && b.node == 4) && delivered.insert(s.sim) {
                    if h.natively_delivered.contains(&o_id) {
                        // checked by the native delivery
                    } else if h.world.get(ok).queue.is_empty() {
                        h.counts.note("queue: a waiting Sim's first interaction, with nothing queued (autonomy's)");
                    } else {
                        let q = h.world.get(ok).queue[0];
                        let same = q.tree == s.tree as i16 && q.objects[1] == s.stack_object;
                        h.counts.note(if same {
                            "queue: a waiting Sim's first interaction the queue's first entry (tree, target)"
                        } else {
                            "queue: a waiting Sim's first interaction NOT its queue's first entry"
                        });
                        if !same && has("REPLAY_SHOW_PUSH") {
                            println!("queue: Sim {o_id} got {} with {}; queued {} with {}", s.tree, s.stack_object, q.tree, q.objects[1]);
                        }
                    }
                }
                let interp = sim.stack_of(&h.world, ok);
                let st_len = interp.stack().len();
                let st_top = interp.stack().last().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.file));
                if first_step.contains(&s.sim) {
                } else if has("REPLAY_SHOW_FIRST_TREE") {
                    let o = h.world.get(ok);
                    let fields: Vec<usize> = h.world.definition(o).map(|d| (0..d.fields.len()).filter(|&k| d.field(k) == s.tree as i32 as _).collect()).unwrap_or_default();
                    note!(h.counts, "study first tree: node {} person {} the definition's fields equal to it {fields:?}", s.node, o.person);
                    let saved: Vec<(i16, u16)> = o.stack.iter().map(|f| (f.tree.map_or(-1, |t| lib.tree_at(t).id), f.node)).collect();
                    let rel = match saved.iter().position(|&(t, _)| t == s.tree as i16) {
                        Some(k) if k + 1 == saved.len() => "the saved top's tree",
                        Some(0) => "the saved bottom's tree",
                        Some(_) => "a saved middle frame's tree",
                        None if saved.is_empty() => "no saved stack",
                        None => "no saved frame's tree",
                    };
                    note!(h.counts, "study first tree vs the saved stack: {rel}, node {}{}", s.node, if saved.last().is_some_and(|&(t, n)| t == s.tree as i16 && n == s.node) { " (the saved top itself)" } else { "" });
                    if fields.is_empty() && !o.person && h.shown.next("first tree", 4) {
                        println!("first tree {} of object {o_id} ({:?}): def fields {:?}", s.tree, o.file.map(|f| file_name(lib, f)), h.world.definition(o).map(|d| &d.fields[..d.fields.len().min(40)]));
                    }
                }
                if first_step.insert(s.sim) {
                    // Each lot object's first step against the stack the lot saved.
                    let same0 = st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.node);
                    h.counts.note(if same0 { "load: a stack's first step at its saved top frame" } else { "load: a stack's first step NOT at its saved top frame" });
                    // The log shows only the frames that step: the saved frames below the first
                    // step's are taken as the log's too (addresses below its, never seen).
                    let fr = frames.get_mut(&s.sim).unwrap();
                    if same0 && st_len > fr.len() {
                        // their addresses by the frames' sizes: 16 and the tree's parameters and
                        // locals, two bytes each, to a multiple of 8 (all but 12 of the logs' 191,030
                        // calls; study REPLAY_FRAME_SIZES)
                        let size = |f: &Frame| f.tree.map_or(16, |t| {
                            let tr = lib.tree_at(t);
                            16 + (2 * (u32::from(tr.args) + u32::from(tr.locals))).div_ceil(8) * 8
                        });
                        let saved = interp.stack();
                        let n = st_len - fr.len();
                        let mut below = Vec::new();
                        let mut at = s.frame;
                        for k in (0..n).rev() {
                            at = at.wrapping_sub(size(&saved[k]));
                            below.push(at);
                            unseen.insert(at);
                        }
                        below.reverse();
                        fr.splice(0..0, below);
                    }
                    if !same0 && has("REPLAY_SHOW_LOAD") && h.shown.next("load", 12) {
                        println!(
                            "load: object {o_id} tick {t}: saved {}:{} (depth {st_len}), first step {}:{} (depth {})",
                            st_top.and_then(|(t, _, _)| t).map_or(-1, |t| t as i32),
                            st_top.map_or(-1, |(_, n, _)| n as i32),
                            s.tree,
                            s.node,
                            fr.len()
                        );
                    }
                }
                let fr_len = frames[&s.sim].len();
                let temps = h.world.get(ok).temps.to_vec();
                let interp = sim.stack_of(&h.world, ok);
                // a call that gives no parameters passes the object's temps as they are now (the
                // interpreter keeps a pointer to them, +44)
                interp.set_defaults(temps);
                // Returns the log does not show (a tree whose return the observer missed, such as
                // 363 between 8283 and 8233 at the club): native deeper than the log and on a
                // return node runs its returns until it is not.
                let mut ran_returns = false;
                while !interp.done() && interp.stack().len() > fr_len && interp.stack().last().is_some_and(|f| f.node > 250) {
                    interp.step(&mut *h);
                    ran_returns = true;
                }
                if ran_returns {
                    h.counts.note("steps: native ran returns the log did not show");
                }
                // A CallNamedTree node whose record before its callee the log lacks (Global 337 node 3
                // at tick 1356: node 4's step, then 8241's node 0 a frame up, the call's record after):
                // native on the call node, the log a frame deeper at a tree's first node: the call
                // node runs first.
                if !interp.done()
                    && s.node == 0
                    && interp.stack().len() + 1 == fr_len
                    && top_node(lib, interp).is_some_and(|n| n.opcode == 28)
                {
                    h.step = i;
                    h.file_of_top = interp.stack().last().and_then(|f| f.file);
                    h.me = ok;
                    interp.reset_iterations();
                    interp.step(&mut *h);
                    h.counts.note("steps: a call node run ahead (its record before its callee's not in the log)");
                }
                // A CreateObjectInstance node the log has no record of (the dream controller's 4121 node
                // 1 at tree.log's tick 85, making the marker 179: node 0's step, then node 2): native on
                // it, the log in the same frame at one of its ways: the node runs first.
                if !interp.done()
                    && interp.stack().len() == fr_len
                    && let Some(n) = top_node(lib, interp).filter(|n| n.opcode == 42)
                    && interp.stack().last().and_then(|f| f.tree).is_some_and(|t| lib.tree_at(t).id as u16 == s.tree)
                    && (s.node == n.true_target as u16 || s.node == n.false_target as u16)
                    && interp.stack().last().is_some_and(|f| f.node != s.node)
                {
                    h.step = i;
                    h.file_of_top = interp.stack().last().and_then(|f| f.file);
                    h.me = ok;
                    interp.reset_iterations();
                    interp.step(&mut *h);
                    h.counts.note("steps: a CreateObjectInstance node run ahead (no record of it in the log)");
                }
                let st_len = interp.stack().len();
                let st_top = interp.stack().last().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.file));
                let same = !interp.done() && interp.stack().len() == fr_len && st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.node);
                if has("REPLAY_TRACE_MAKERS") && matches!((s.tree, s.node), (0x2007, 1) | (0x2022, 1) | (0x200f, 6)) && s.look == b'X' && !consumed.contains(&i) {
                    println!("  maker step {i}: tree found {:?}, native stack same {same} (native top {st_top:?}, depth {} of {fr_len}), node opcode {:?}", lib.file(file).path, interp.stack().len(), lib.tree_at(tree).nodes.get(s.node as usize).map(|n| n.opcode));
                }
                // A tree run again in a frame that has just returned (the previous step this
                // stack's, in the same frame, a return; this one at node 0): a new check of an
                // evaluation, which older logs show as the Sim's own steps (no nesting): not compared.
                let rerun = !same
                    && s.node == 0
                    && i > 0
                    && log.steps[..i].iter().rev().find(|p| p.sim == s.sim).is_some_and(|p| p.frame == s.frame && p.node > 250 && p.node == p.after);
                if rerun {
                    h.counts.note("steps: a tree run again in a frame just returned (a check, not compared)");
                    streak = 0;
                    // native takes the log's: the frames below kept, the top one the log's
                }
                if same {
                    h.counts.note("steps: the native stack as the log's");
                    streak += 1;
                    best_streak = best_streak.max(streak);
                } else {
                    streak = 0;
                    // (not a call's record after its callee's, which is skipped below, not counted)
                    let call_record = st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.after)
                        && lib.tree_at(tree).nodes.get(s.node as usize).is_some_and(|nd| nd.opcode == 28);
                    if has("REPLAY_SHOW_DIFFS") && !call_record {
                        let cat = if interp.done() {
                            0
                        } else if st_len > fr_len {
                            1
                        } else if st_len < fr_len {
                            2
                        } else if st_top.is_some_and(|(t, _, _)| t.is_some() && t != Some(s.tree as i16)) {
                            3
                        } else {
                            4
                        };
                        const NAMES: [&str; 5] = ["stopped", "deeper", "log deeper", "another tree", "another node"];
                        if h.shown.next(&format!("diffs {cat}"), env("REPLAY_DIFFS_LIMIT").map_or(6, |v| atoi(&v) as i32)) {
                            let prev = (0..i).rev().map(|j| &log.steps[j]).find(|p| p.sim == s.sim);
                            println!(
                                "{}: object {o_id} tick {t}: native {}:{} depth {st_len} (error {}), log {}:{} depth {fr_len}; its last step {}:{}->{} {} res {}",
                                NAMES[cat],
                                st_top.and_then(|(t, _, _)| t).map_or(-1, |t| t as i32),
                                st_top.map_or(-1, |(_, n, _)| n as i32),
                                interp.error() as u16,
                                s.tree,
                                s.node,
                                prev.map_or(-1, |p| p.tree as i32),
                                prev.map_or(-1, |p| p.node as i32),
                                prev.map_or(-1, |p| p.after as i32),
                                prev.map_or('-', |p| if p.look != 0 { p.look as char } else { '-' }),
                                prev.map_or(-1, |p| p.result as i32)
                            );
                        }
                    }
                    if interp.done() {
                        if !rerun { h.counts.note("steps: different: the native stack stopped"); }
                        if has("REPLAY_SHOW_STOPPED") {
                            let p = if i > 0 && log.steps[i - 1].sim == s.sim { format!("{}:{}->{}", log.steps[i - 1].tree, log.steps[i - 1].node, log.steps[i - 1].after) } else { "another stack's".into() };
                            note!(h.counts, "study stopped: error {:?} log {}:{} (before {p})", interp.error(), s.tree, s.node);
                            if interp.error() == Error::ReturnFromBottom && has("REPLAY_SHOW_RFB") {
                                let back: Vec<String> = (i.saturating_sub(6)..i).map(|j| &log.steps[j]).filter(|t| t.sim == s.sim).map(|t| format!("{}:{}->{} f{:x}", t.tree, t.node, t.after, t.frame & 0xFFF)).collect();
                                println!("rfb: object {o_id} tick {t} log {}:{} frame {:x} depth {fr_len}; before: {}", s.tree, s.node, s.frame & 0xFFF, back.join(" | "));
                            }
                            if interp.error() == Error::BadTransition && has("REPLAY_SHOW_BADT") {
                                let back: Vec<String> = (i.saturating_sub(4)..i).map(|j| &log.steps[j]).filter(|t| t.sim == s.sim).map(|t| format!("{}:{}->{}", t.tree, t.node, t.after)).collect();
                                let nodes: Vec<String> = interp.stack().last().and_then(|f| f.tree).map(|t| lib.tree_at(t)).unwrap_or(lib.tree_at(tree)).nodes.iter().take(4).map(|n| format!("op {} -> {}/{}", n.opcode, n.true_target, n.false_target)).collect();
                                println!("badt: object {o_id} tick {t} log {}:{}; before {}; tree {} nodes {:?}; native stack {:?}", s.tree, s.node, back.join(" | "), s.tree, nodes, interp.stack().iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node)).collect::<Vec<_>>());
                            }
                            if interp.error() == Error::Missing {
                                let o = h.world.get(ok);
                                let f = o.file.map(|f| lib.file(f));
                                println!("missing: object {o_id} file {:?} semi {:?}; frame file {:?}; log {}:{}", f.map(|f| f.path.clone()), f.and_then(|f| f.semi).map(|x| lib.file(x).path.clone()), st_top.and_then(|t| t.2).map(|x| lib.file(x).path.clone()), s.tree, s.node);
                            }
                        }
                    } else if st_len != fr_len && !rerun {
                        h.counts.note(if st_len < fr_len { "steps: different: the log's is deeper (a frame pushed)" } else { "steps: different: the native is deeper" });
                    }
                    if !interp.done() && st_len < fr_len && o_person && has("REPLAY_SHOW_PUSH") {
                        let wn = top_node(lib, interp);
                        let o = h.world.get(ok);
                        let mut l = format!(
                            "push: Sim {o_id} tick {t} onto {}:{} (op {}): {}:{} in {}, stack object {}, params",
                            st_top.and_then(|(t, _, _)| t).map_or(-1, |t| t as i32),
                            st_top.map_or(0, |(_, n, _)| n),
                            wn.map_or(-1, |n| n.opcode as i32),
                            s.tree,
                            s.node,
                            lib.file(file).path,
                            s.stack_object
                        );
                        for p in &s.params {
                            l += &format!(" {p}");
                        }
                        l += &format!(
                            "; saved current id {} objects {} {} {}; queue",
                            o.current.id, o.current.objects[0], o.current.objects[1], o.current.objects[2]
                        );
                        for q in &o.queue {
                            l += &format!(
                                " [id {} objects {} {} {} shorts {} {} {} {} a {} b {} c {} tree {}]",
                                q.id, q.objects[0], q.objects[1], q.objects[2], q.shorts[0], q.shorts[1], q.shorts[2], q.shorts[3], q.a, q.b, q.c, q.tree
                            );
                        }
                        println!("{l}");
                    } else if st_top.is_none_or(|(t, _, _)| t != Some(s.tree as i16)) {
                        if !rerun { h.counts.note("steps: different: another tree"); }
                        if !rerun && has("REPLAY_SHOW_CURRENT") && t < 5 {
                            let c = &h.world.get(ok).current;
                            println!("current: tick {t} object {o_id} id {} objects {:?} shorts {:?} a {} b {} c {} tree {}; log {}:{}", c.id, c.objects, c.shorts, c.a, c.b, c.c, c.tree, s.tree, s.node);
                        }
                        if !rerun && has("REPLAY_SHOW_QUEUE_AT") && !h.world.get(ok).queue.is_empty() {
                            let q: Vec<String> = h.world.get(ok).queue.iter().map(|q| format!("[id {} tree {} objects {:?} interaction {:?}]", q.id, q.tree, q.objects, q.interaction)).collect();
                            println!("queue at: tick {t} object {o_id} log {}:{} so {}; {}", s.tree, s.node, s.stack_object, q.join(" "));
                        }
                        if !rerun && has("REPLAY_SHOW_OTHER_TREE") {
                            println!("other tree: tick {t} object {o_id} queue {} native {:?} log {}:{} (op {})", h.world.get(ok).queue.len(), st_top.map(|(t, n, _)| (t, n)), s.tree, s.node, lib.tree_at(tree).nodes.get(s.node as usize).map_or(-1, |n| n.opcode as i32));
                        }
                        if has("REPLAY_SHOW_196") && o_id == 196 && h.shown.next("196", 2) {
                            let o = h.world.get(ok);
                            let f = o.file.map(|f| lib.file(f));
                            println!("196: file {:?} guid {:08x} def {:?}; has 4129 {:?} 4137 {:?} 4116 {:?} 4106 {:?}", f.map(|f| f.path.clone()), o.guid, o.definition, f.map(|f| f.trees.contains_key(&4129)), f.map(|f| f.trees.contains_key(&4137)), f.map(|f| f.trees.contains_key(&4116)), f.map(|f| f.trees.contains_key(&4106)));
                            if let Some(t) = lib.tree(o.file, 4116) {
                                for (k, n) in lib.tree_at(t).nodes.iter().enumerate() {
                                    println!("   4116 node {k}: op {} -> {} / {} operand {:02x?}", n.opcode, n.true_target, n.false_target, n.operand);
                                }
                            }
                            for f in lib.files().iter().filter(|f| f.trees.contains_key(&4116) && f.trees.contains_key(&4106) && !f.trees.contains_key(&4137)) {
                                println!("   a file with 4116 and 4106 but no 4137: {}", f.path);
                            }
                        }
                        if has("REPLAY_SHOW_TREES") && h.shown.next("trees", if has("REPLAY_SHOW_ALL") { 100_000 } else { 12 }) {
                            let p = if i > 0 && log.steps[i - 1].sim == s.sim {
                                format!("{}:{}->{}", log.steps[i - 1].tree, log.steps[i - 1].node, log.steps[i - 1].after)
                            } else {
                                "another stack's".into()
                            };
                            println!(
                                "tick {t} object {o_id}: native {}:{} (depth {st_len}), log {}:{} (depth {fr_len}), log step before: {p}",
                                st_top.and_then(|(t, _, _)| t).map_or(-1, |t| t as i32),
                                st_top.map_or(0, |(_, n, _)| n),
                                s.tree,
                                s.node
                            );
                        }
                    } else if st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.after)
                        && lib.tree_at(tree).nodes.get(s.node as usize).is_some_and(|nd| nd.opcode == 28)
                    {
                        // a CallNamedTree node's record, written when the call ends (after its
                        // callee's steps): native is past it, at the node it went on to
                        h.counts.note("steps: a call's record after its callee's (native past it)");
                        continue;
                    } else {
                        if !rerun { h.counts.note("steps: different: another node"); }
                        if has("REPLAY_SHOW_NODES") {
                            let p = if i > 0 && log.steps[i - 1].sim == s.sim { format!("{}:{}->{}", log.steps[i - 1].tree, log.steps[i - 1].node, log.steps[i - 1].after) } else { "another stack's".into() };
                            let op = lib.tree_at(tree).nodes.get(s.node as usize).map(|nd| nd.opcode);
                            note!(h.counts, "study another node: tree {} native node {} log node {} op {op:?} after {} (before: {p})", s.tree, st_top.map_or(0, |(_, n, _)| n), s.node, s.after);
                        }
                    }
                    // Set to the log's: the frames below kept, the top one the log's.
                    let keep = st_len.min(fr_len - 1);
                    let mut nf: Vec<Frame> = interp.stack()[..keep].to_vec();
                    while nf.len() < fr_len - 1 {
                        nf.push(Frame::default()); // frames the log has, not known natively
                    }
                    // A frame's file is the object file its trees are named from: for a global or
                    // semi-global tree, not the file the tree is in but one that names it (the
                    // object's own if it does, else any whose semi-global file it is).
                    let top_file = match tree_kind(s.tree) {
                        TreeKind::Global => st_top.and_then(|(_, _, f)| f).or(o_file),
                        TreeKind::SemiGlobal => {
                            if o_file.is_some_and(|f| lib.file(f).semi == Some(file)) {
                                o_file
                            } else {
                                // (none naming it: the tree's own file, as the C++ left it)
                                (0..lib.files().len() as u32).map(FileId).find(|&f| lib.file(f).semi == Some(file)).or(Some(file))
                            }
                        }
                        _ => Some(file),
                    };
                    nf.push(Frame {
                        tree: Some(tree),
                        node: s.node,
                        params: s.params.clone(),
                        locals: s.locals.clone(),
                        stack_object: s.stack_object,
                        file: top_file,
                        ..Default::default()
                    });
                    interp.restore(nf);
                    // another stack than the one it had: no sleep of the old one (nor a saved
                    // Idle's waking) carries over
                    interp.sleep().wake();
                }
                if let Some(w) = env("REPLAY_WATCH_ID")
                    && let Ok(w) = w.parse::<i16>()
                    && let Some(o) = h.world.find_object(w)
                {
                    let now = (o.data[2], o.data[3]);
                    let was = h.shown.0.insert(format!("watch {w}"), (now.0 as i32) << 16 | (now.1 as u16 as i32));
                    if was != Some((now.0 as i32) << 16 | (now.1 as u16 as i32)) {
                        let prev = i.checked_sub(1).map(|k| &log.steps[k]);
                        let ops: Vec<String> = h.world.objects().filter_map(|x| x.file).collect::<BTreeSet<_>>().into_iter().filter_map(|f| prev.and_then(|p| lib.tree(Some(f), p.tree).map(|t| lib.tree_at(t).nodes.get(p.node as usize).map(|n| (n.opcode, n.operand))))).flatten().map(|(op, o)| format!("op {op} {o:02x?}")).collect::<BTreeSet<_>>().into_iter().collect();
                        println!("watch {w}: container/slot now {now:?} at tick {} step {i} ({}:{}; before it {:?}: {ops:?})", s.tick, s.tree, s.node, prev.map(|p| (p.tree, p.node, p.after)));
                    }
                }
                if same {
                    // The frame's own state as the native kept it, against the log's at the step.
                    let f = interp.top();
                    h.counts.note(if f.params == s.params { "frames: parameters the same natively" } else { "frames: parameters different (set to the log's)" });
                    h.counts.note(if f.locals == s.locals { "frames: locals the same natively" } else { "frames: locals different (set to the log's)" });
                    h.counts.note(if f.stack_object == s.stack_object {
                        "frames: stack object the same natively"
                    } else {
                        "frames: stack object different (set to the log's)"
                    });
                    if f.stack_object != s.stack_object && i > 0 {
                        let p = &log.steps[i - 1];
                        let own = p.sim == s.sim;
                        let what = if !own {
                            "another stack's step"
                        } else if p.frame != s.frame {
                            if p.frame < s.frame { "a call" } else { "a return" }
                        } else {
                            "a step of the frame"
                        };
                        note!(h.counts, "frames: stack object different, after {what}");
                        if has("REPLAY_SHOW_SO") && h.shown.next("so", if has("REPLAY_SHOW_SO_NEAR") { 100_000 } else { 12 }) {
                            let mut l = format!(
                                "stack object: native {}, log {} at {}:{}; before: {}:{}->{} ({}) so {}->{}{}",
                                f.stack_object,
                                s.stack_object,
                                s.tree,
                                s.node,
                                p.tree,
                                p.node,
                                p.after,
                                if p.look != 0 { p.look as char } else { '-' },
                                p.stack_object,
                                p.stack_object_after,
                                if own { "" } else { " (another stack)" }
                            );
                            if let Some(e) = &p.expr {
                                l += &format!(
                                    " expr lhs {}/{} rhs {}/{} op {} lhs {}->{} rhs {}",
                                    e.lhs_scope, e.lhs_data, e.rhs_scope, e.rhs_data, e.op, e.lhs, e.lhs_after, e.rhs
                                );
                            }
                            for v in &p.lookups {
                                l += &format!(" [V {}/{}={} fl {}]", v.scope, v.data, v.value, v.flags);
                            }
                            if let Some(n) = f.tree.and_then(|t| lib.tree_at(t).nodes.get(p.node as usize)) {
                                l += &format!(" node {}: op {} operand {:02x?}", p.node, n.opcode, n.operand);
                            }
                            if has("REPLAY_SHOW_SO_NEAR") && let Some(me_o) = h.world.find_object(o_id) {
                                let (mx, my) = (me_o.x16 >> 4, me_o.y16 >> 4);
                                let order = h.world.order().to_vec();
                                for (k, &key) in order.iter().enumerate() {
                                    let o = h.world.get(key);
                                    if ((o.x16 >> 4) - mx).abs() <= 1 && ((o.y16 >> 4) - my).abs() <= 1 && o.id() != o_id {
                                        l += &format!(" | #{k} {}: ({},{}) guid {:08x} p {}", o.id(), o.x16 >> 4, o.y16 >> 4, o.guid, o.person);
                                    }
                                }
                                l += &format!(" | me {o_id} at ({mx},{my})");
                                if has("REPLAY_SHOW_SO_DATA") {
                                    for id in [102i16, 66, 57, 101, 109, 53, 139, 52, 65, 61, 62, 60, 56, 135, 136] {
                                        if let Some(o) = h.world.find_object(id) {
                                            println!("obj {id}: guid {:08x} level {} x16 {} y16 {} data {:?} defn {:?}", o.guid, o.level, o.x16, o.y16, &o.data[..o.data.len().min(80)], h.world.definition(o).map(|d| &d.fields[..d.fields.len().min(80)]));
                                        }
                                    }
                                }
                            }
                            if has("REPLAY_SHOW_SO_197") && let Some(o) = h.world.find_object(197) {
                                l += &format!(" | tick {}: 197 container {} slot {} at ({},{})", s.tick, o.data[2], o.data[3], o.x16 >> 4, o.y16 >> 4);
                                if let Some(c) = h.world.find_object(62) {
                                    l += &format!("; 62 at ({},{}) container {}", c.x16 >> 4, c.y16 >> 4, c.data[2]);
                                }
                            }
                            if has("REPLAY_SHOW_SO_59") {
                                let order = h.world.order().to_vec();
                                let hits: Vec<String> = order.iter().enumerate().map(|(k, &key)| (k, h.world.get(key))).filter(|(_, o)| o.data.get(59) == s.params.first().or(p.params.first())).map(|(k, o)| format!("#{k} {}", o.id())).collect();
                                l += &format!(" | params {:?} (before {:?}); field 59 = p0: {}", s.params, p.params, hits.join(" "));
                            }
                            if has("REPLAY_SHOW_SO_OBJECTS") {
                                for id in [p.stack_object, p.stack_object_after, o_id] {
                                    if let Some(o) = h.world.find_object(id) {
                                        l += &format!(
                                            " | {id}: guid {:08x} person {} leader {} at ({},{},{}) container {} slot {} d0..12 {:?}",
                                            o.guid, o.person, o.leader, o.x16 >> 4, o.y16 >> 4, o.level,
                                            o.data.get(2).copied().unwrap_or(0), o.data.get(3).copied().unwrap_or(0),
                                            &o.data[..o.data.len().min(12)]
                                        );
                                    }
                                }
                            }
                            println!("{l}");
                        }
                    }
                    f.params = s.params.clone();
                    f.locals = s.locals.clone();
                    f.stack_object = s.stack_object;
                }
                if interp.asleep() {
                    if has("REPLAY_SHOW_MADE") {
                        println!("asleep: tick {} id {} tree {:#x} node {}", s.tick, o_id, s.tree, s.node);
                    }
                    h.counts.note("steps: the log ran a stack the native has asleep");
                    interp.sleep().wake();
                }
                h.step = i;
                h.file_of_top = interp.stack().last().and_then(|f| f.file);
                h.me = ok;
                interp.reset_iterations();
                let s0 = Instant::now();
                let stepped = interp.step(&mut *h);
                // A Sim that spent money this turn (AlterBudget, not a test) shows it when its turn
                // ends: the engine pushes its "display money balloon" over the yielding frame, an
                // engine frame with no stack object, run in the same turn (docs/simulation.md, "The
                // money balloon")
                if stepped == Step::Yield
                    && h.world.get_live(ok).is_some_and(|o| o.spent)
                    && std::mem::take(&mut h.world.get_mut(ok).spent)
                    && let Some(t) = h.world.money_balloon_tree(ok)
                    && interp.push(lib, t, &[])
                {
                    let f = interp.top();
                    f.engine = true;
                    f.stack_object = 0;
                    f.file = h.world.get(ok).file;
                    h.counts.note("frames: the money balloon pushed natively when a Sim that spent yields");
                }
                if let Some(b) = &mut h.bench {
                    let d = us(s0);
                    b.steps_us += d;
                    tick_us += d;
                    b.steps += 1;
                }
            }
            motives_before(i16::MAX, sim, &mut h.world, &mut motives_done);
            if have_passes {
                h.counts.note(if sim.stop_asked(&h.world) == (pass.stopped_at != 0) {
                    "passes: stopped (or not) as the log's"
                } else {
                    "passes: DIFFERENT stop"
                });
            }
            let stopped = have_passes && sim.stop_asked(&h.world);
            sim.end_pass(&mut h.world, if have_passes { pass.stopped_at as i16 } else { 0 });
            // the clock: the minute every 30 passes that ran to their end
            h.world.clock_pass(stopped);
            check_time_of_day(log, t, &h.world, &mut time_at, &mut h.counts);
            // The persons' motives after the tick against the log's ('N' records), then set to the
            // log's (what the scripts not native yet wrote).
            while motive_at < log.motives.len() && log.motives[motive_at].tick < t {
                motive_at += 1;
            }
            while motive_at < log.motives.len() && log.motives[motive_at].tick == t {
                let m = &log.motives[motive_at];
                motive_at += 1;
                let Some(k) = h.world.find(m.id).filter(|&k| h.world.get(k).motives.len() >= 16) else {
                    h.counts.note("motives: a person not in the world");
                    continue;
                };
                let om = h.world.get(k).motives.clone();
                let same = (0..16).all(|i| i == 3 || i == 13 || om[i] == m.motives[i]);
                h.counts.note(if same {
                    "motives: the native motives as the log's after the tick (all but 3 and 13)"
                } else {
                    "motives: DIFFERENT after the tick (set to the log's)"
                });
                if !same {
                    for (i, (&a, &b)) in om.iter().zip(&m.motives).enumerate() {
                        if i != 3 && i != 13 && a != b {
                            note!(h.counts, 
                                "motives: different after the tick: person {} motive {i}{}",
                                m.id,
                                if (a - b).abs() < 1e-3 { " (by a last bit)" } else { "" }
                            );
                        }
                    }
                }
                if !same && has("REPLAY_SHOW_MOTIVES") && m.id == 237 && t >= 5020 && shown_motives < 6 {
                    shown_motives += 1;
                    let mut l = format!("tick {t} person {} (object #{}):", m.id, k.0);
                    for i in [5, 6, 7, 8, 9, 14, 15] {
                        l += &format!("  m{i} native {} log {}", g(om[i] as f64, 6), g(m.motives[i] as f64, 6));
                    }
                    println!("{l}");
                }
                h.world.get_mut(k).motives[..16].copy_from_slice(&m.motives);
            }
            if let Some(b) = &mut h.bench {
                tick_us -= b.sync_us - sync_before; // the replay's own, inside the steps
                if b.ticks.is_empty() || tick_us > b.ticks[b.worst_tick] {
                    b.worst_tick = b.ticks.len();
                }
                b.ticks.push(tick_us);
            }
        }
        self.place_at = place_at;
        self.motive_at = motive_at;
        self.shown_motives = shown_motives;
        self.engine_list_at = engine_list_at;
        self.time_at = time_at;
        self.streak = streak;
        self.best_streak = best_streak;
        self.turn_followers();
        true
    }
}
