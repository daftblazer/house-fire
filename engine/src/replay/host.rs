use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Instant;

use crate::bhav::{Node, Tree};
use crate::expression::{ExpressionOperand, Variables, expression};
use crate::interp::{Error, Host, Interpreter, Mode, PrimResult, Trees};
use crate::library::Library;
use crate::primitives::{balloon_or_string, change_suit, relationship_old, direction_to, distance_to, 
    call_named_tree, complete_goal, create_object, drop_onto, grab, place_made, remove_object, snap, dialog, idle, idle_for_input, idle_for_input_waiting, may_sleep, present, push_action, random, relationship, run_functional_tree, set_motive_delta, set_to_next, test_object_type, notify_stack_object,
};
use crate::routing::slot_places;
use crate::simulator::SimHost;
use crate::tree_log::{Log, StepRec};
use crate::world::{PORTAL_FUNCTION, SIT_FUNCTION, ObjectKey, ScriptVariables, World};
use crate::{FileId, TreeId};
use super::study::{env, has, atoi, file_name};
use super::counts::{Counts, Bench, Shown, us, shown_once};
use super::animate::{AnimReplay, animate_differs};
use super::variables::{sync_variables};
use super::routing::RoutingReturns;

/// The host the objects' scripts run with: the native primitives on the native world, the rest
/// from the log step being replayed.
pub(super) struct ReplayHost<'l> {
    pub(super) lib: &'l Library,
    pub(super) world: World<'l>,
    pub(super) log: Rc<Log>,
    /// The object whose stack is being stepped.
    pub(super) me: ObjectKey,
    /// The log's step being replayed.
    pub(super) step: usize,
    /// What each person was last told to look towards (LookTowards at the stack object): the
    /// tick and the place of the object, sixteenths.
    pub(super) looks: HashMap<i16, (u32, (i32, i32))>,
    pub(super) bench: Option<Bench>,
    pub(super) file_of_top: Option<FileId>,
    /// The log's steps of the checks a primitive runs (the interaction queue's), in order.
    pub(super) nested: Vec<usize>,
    pub(super) nested_pos: usize,
    pub(super) natively_delivered: BTreeSet<i16>,
    /// The objects on the lot this tick (the log's list).
    pub(super) present: BTreeSet<i16>,
    /// Nested records already used out of order (calls').
    pub(super) deferred: BTreeSet<usize>,
    /// The last evaluation's choice: the object, its interaction's action and file.
    pub(super) chosen: Option<(i16, TreeId, Option<FileId>)>,
    /// The next logged score ('A').
    pub(super) score_at: usize,
    /// While an evaluation runs: its checks' results against the log's (same, different).
    pub(super) on_check: Option<(usize, usize)>,
    pub(super) counts: Counts,
    pub(super) shown: Shown,
    /// The simulator's ticks run (for a study).
    pub(super) now: u32,
    /// The routing runs' native returns (a pre-pass of `routing`).
    pub(super) routing_returns: RoutingReturns,
    /// Objects made before their maker's step (its CreateObjectInstance runs the new object's
    /// init inside it, and the log has the init's steps first): the maker's step only places them.
    pub(super) pre_made: BTreeSet<i16>,
    /// The Animate primitive natively (`anim::Animator`), where the log has the frames.
    pub(super) anim: Option<AnimReplay>,
}

impl ReplayHost<'_> {
    pub(super) fn sync(&mut self, interp: &mut Interpreter, s: &StepRec) {
        self.sync_but(interp, s, None);
    }

    /// The same, but for a variable the step only writes (`sync_variables`).
    pub(super) fn sync_but(&mut self, interp: &mut Interpreter, s: &StepRec, written: Option<(u8, i16)>) {
        let t0 = Instant::now();
        sync_variables(&mut self.world, &mut self.counts, &mut self.shown, self.me, interp.top(), s, written);
        if let Some(b) = &mut self.bench {
            b.sync_us += us(t0);
        }
    }

    /// The primitive's result as the log has it.
    pub(super) fn logged(&mut self, s: &StepRec, n: &Node) -> PrimResult {
        match s.result {
            1 => PrimResult::Yield,
            // 2: a dialog waiting for the player (Dialog, opcode 36, the only one): the node
            // stays and the tick's pass stops after this turn (docs/simulation.md).
            2 => {
                self.world.stop_pass = true;
                PrimResult::Yield
            }
            _ if s.after == n.true_target as u16 => PrimResult::True,
            _ => PrimResult::False,
        }
    }

    pub(super) fn me_id(&self) -> i16 {
        self.world.get(self.me).id()
    }
}

impl Trees for ReplayHost<'_> {
    fn tree_at(&self, t: TreeId) -> &Tree {
        self.lib.tree_at(t)
    }
}

impl<'l> SimHost<'l> for ReplayHost<'l> {
    fn world(&mut self) -> &mut World<'l> {
        &mut self.world
    }
    fn set_me(&mut self, me: ObjectKey) {
        self.me = me;
    }
}

impl Host for ReplayHost<'_> {
    fn interaction_returned(&mut self) {
        let me = self.me;
        self.world.end_interaction(me);
    }
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        let f = self.file_of_top.or(self.world.get(self.me).file);
        self.lib.tree(f, id)
    }

    fn ad_range_mut(&mut self) -> Option<&mut Vec<i16>> {
        Some(&mut self.world.ad_range)
    }

    fn default_params(&mut self) -> Option<[i16; 8]> {
        Some(self.world.get(self.me).temps)
    }

    /// A check tree the engine runs, stepped natively through the log's steps of it.
    fn run_check(&mut self, t: TreeId, file: Option<FileId>, stack_object: i16, defaults: &[i16]) -> Option<bool> {
        let log_rc = Rc::clone(&self.log);
        let log: &Log = &log_rc;
        let lib = self.lib;
        if let Some(v) = self.world.get_mut(self.me).data.get_mut(crate::world::DATA_HIDE_INTERACTION) {
            *v = 0;
        }
        let mut chk = Interpreter::new(Mode::Check, 10000);
        chk.set_defaults(defaults.to_vec());
        // a check tree starts with parameter 0 = 1 (every logged check reads it so: 103 reads of
        // the home log that were 0 with none)
        chk.start(lib, Some(t), &[1], file);
        if chk.done() {
            return None;
        }
        chk.top().stack_object = stack_object;
        let (outer, outer_file) = (self.step, self.file_of_top);
        let trace = has("REPLAY_TRACE_CHECK");
        let show_push = has("REPLAY_SHOW_PUSH");
        let top_of = |chk: &Interpreter| -> (i32, u16, Option<FileId>, Option<Node>) {
            let f = chk.stack().last().unwrap();
            match f.tree {
                Some(t) => {
                    let tr = lib.tree_at(t);
                    (tr.id as i32, f.node, f.file, tr.nodes.get(f.node as usize).copied())
                }
                None => (-1, f.node, f.file, None),
            }
        };
        while !chk.done() {
            // A record for a call that ran its callee's steps first is past them: skip it here.
            while self.nested_pos < self.nested.len() && self.deferred.contains(&self.nested[self.nested_pos]) {
                self.nested_pos += 1;
            }
            // A node whose step calls a tree (CallNamedTree, opcode 28) is logged when it ends,
            // after its callee's steps: its record is the next one with its tree and node.
            let (tid, tnode, tfile, tn) = top_of(&chk);
            if tn.is_some_and(|n| n.opcode == 28) {
                let mut k = self.nested_pos;
                while k < self.nested.len() && !(log.steps[self.nested[k]].tree == tid as u16 && log.steps[self.nested[k]].node == tnode) {
                    k += 1;
                }
                if k < self.nested.len() && k != self.nested_pos {
                    self.deferred.insert(self.nested[k]);
                    self.counts.note("queue: a check's call step as the log's (its record after its callee's)");
                    self.step = self.nested[k];
                    self.file_of_top = tfile;
                    chk.reset_iterations();
                    chk.step(self);
                    continue;
                }
            }
            if self.nested_pos >= self.nested.len() {
                self.counts.note("queue: a check ran past the log's steps of it");
                if show_push {
                    println!("past: Sim {} at {tid}:{tnode} depth {}\\n", self.me_id(), chk.stack().len());
                }
                break;
            }
            let ni = self.nested[self.nested_pos];
            self.nested_pos += 1;
            let ns = &log.steps[ni];
            if trace && env("REPLAY_TRACE_CHECK").and_then(|v| v.parse::<i16>().ok()).is_none_or(|w| w == self.me_id()) {
                println!(
                    "  check {}: native {tid}:{tnode} depth {}; log {}:{}->{} {}",
                    lib.tree_at(t).id,
                    chk.stack().len(),
                    ns.tree,
                    ns.node,
                    ns.after,
                    if ns.look != 0 { ns.look as char } else { '-' }
                );
            }
            let same = tid != -1 && tid as i16 == ns.tree as i16 && tnode == ns.node;
            self.counts.note(if same { "queue: a check's step as the log's" } else { "queue: a check's step DIFFERENT" });
            if !same && show_push {
                println!(
                    "check of Sim {}: native {tid}:{tnode} depth {}, log {}:{} (so {}, look {}, res {}, after {})",
                    self.me_id(),
                    chk.stack().len(),
                    ns.tree,
                    ns.node,
                    ns.stack_object,
                    if ns.look != 0 { ns.look as char } else { '-' },
                    ns.result,
                    ns.after
                );
            }
            self.step = ni;
            self.file_of_top = tfile;
            chk.reset_iterations();
            chk.step(self);
        }
        self.step = outer;
        self.file_of_top = outer_file;
        if trace {
            println!(
                "  check {} done {} error {} result {}",
                lib.tree_at(t).id,
                chk.done() as i32,
                chk.error() as u16,
                chk.result() as i32
            );
        }
        let result = (chk.done() && chk.error() == Error::None).then(|| chk.result());
        if let (Some(got), Some((same, different))) = (result, self.on_check.as_mut())
            && self.nested_pos > 0
        {
            let end = &log.steps[self.nested[self.nested_pos - 1]];
            if got == (end.node == 254) {
                *same += 1;
            } else {
                *different += 1;
            }
        }
        result
    }

    fn primitive(&mut self, interp: &mut Interpreter, n: &Node) -> PrimResult {
        let log_rc = Rc::clone(&self.log);
        let log: &Log = &log_rc;
        let lib = self.lib;
        let s = &log.steps[self.step];
        let me = self.me;
        let me_id = self.me_id();
        if n.opcode == 44
            && let Some(ar) = &mut self.anim
        {
            // Animate: natively through `anim::Animator`, against the log's (yield, true, or false
            // with an event in local 0); the log's result taken
            ar.frames_to(self.step);
            ar.animating.entry(s.tick).or_default().insert(s.sim);
            use crate::anim::Poll;
            let (file, table, native) = super::animate::animate(ar, lib, &self.world, me, interp, n, (s.sim, s.tick));
            let logged_true = s.result != 1 && s.after == n.true_target as u16;
            if env("REPLAY_TRACE_ANIM_SIM").is_some_and(|v| v == "all" || u32::from_str_radix(&v, 16).ok() == Some(s.sim)) && s.tick <= env("REPLAY_TRACE_ANIM_TO").and_then(|v| v.parse().ok()).unwrap_or(u32::MAX) {
                let st = ar.m.play(s.sim).map(|(id, p)| format!("{id:x} {p:?}"));
                let name = file.and_then(|f| lib.file(f).strings.get(&table)).and_then(|t| t.get(n.operand16(0) as usize)).cloned();
                println!("  anim trace name: {:?} table {table} {name:?}; tree file {:?}", file.map(|f| &lib.file(f).path), interp.top().tree.and_then(|t| ar.tree_file.get(&t)).map(|f| (&lib.file(*f).path, lib.file(*f).strings.get(&129).map(|v| v.len()))));
                println!("  anim trace t {} step {} tree {} node {} op {:02x?} result {} after {} (t {} f {}): native {native:?}; play {}", s.tick, self.step, s.tree, s.node, n.operand, s.result, s.after, n.true_target, n.false_target, st.map(|x| x.chars().take(200).collect::<String>()).unwrap_or_default());
            }
            if native.is_none() && has("REPLAY_SHOW_ANIMATE_UNKNOWN") {
                let raw = file.and_then(|f| lib.file(f).strings.get(&table)).and_then(|t| t.get(n.operand16(0) as usize)).cloned();
                let len = file.and_then(|f| lib.file(f).strings.get(&table)).map(|t| t.len());
                note!(self.counts, "study animate unknown: {}:{} by {me_id} source {} flags {:#04x} file {:?} (table {table}: {len:?} names) name {:?} operand {:02x?}", s.tree, s.node, n.operand[4], n.operand[5], file.map(|f| lib.file(f).path.rsplit('/').next().unwrap_or("").to_string()), raw, n.operand);
                if has("REPLAY_SHOW_ANIMATE_TABLES") && self.shown.next(&format!("anim unknown {}:{}", s.tree, s.node), 1) {
                    // study: every string table of the files around that has the index
                    let index = n.operand16(0) as usize;
                    let semi = self.world.get(me).file.and_then(|f| lib.file(f).semi);
                    let stack_file = self.world.by_id().get(&interp.top().stack_object).and_then(|k| self.world.get(*k).file);
                    let tree_file = interp.top().tree.and_then(|t| ar.tree_file.get(&t).copied());
                    for (k, f) in [("global", lib.global()), ("me", self.world.get(me).file), ("my semi-global", semi), ("tree", tree_file), ("stack object", stack_file)] {
                        if let Some(f) = f {
                            let t: Vec<_> = lib.file(f).strings.iter().filter_map(|(id, v)| v.get(index).map(|x| (*id, v.len(), x.clone()))).collect();
                            println!("  tables of {k} {} with index {index}: {t:?}", lib.file(f).path);
                        }
                    }
                }
                if n.operand[4] == 3 && shown_once(&mut self.shown, "anim tables") {
                    for (k, f) in [("global", lib.global()), ("me", self.world.get(me).file), ("stack", file)] {
                        if let Some(f) = f {
                            let t: Vec<_> = lib.file(f).strings.iter().filter(|(_, v)| v.iter().any(|x| x.to_ascii_lowercase().contains("a2o") || x.contains(".max") || x.to_ascii_lowercase().starts_with("a-") || x.to_ascii_lowercase().starts_with("a2a"))).map(|(id, v)| (*id, v.len(), v.first().cloned())).collect();
                            println!("  anim tables of {k} {}: {t:?}", lib.file(f).path);
                            if k == "global" {
                                for tb in [130u16, 156, 157] {
                                    let v = lib.file(f).strings.get(&tb);
                                    let at: Vec<_> = [0x0a, 0x0b, 0x14, 0x19, 0x24, 0x39, 0x48, 0x49, 0xbb, 0x11a, 0x11c].iter().map(|&i| v.and_then(|v| v.get(i)).cloned()).collect();
                                    println!("    STR# {tb}: {at:?}");
                                }
                            }
                        }
                    }
                }
            }
            self.counts.note(match native {
                None => "primitives: Animate: not modelled (no archive animation), the log's",
                Some(Poll::Pending) if s.result == 1 => "primitives: Animate: yields natively as the log's",
                Some(Poll::Done) if logged_true => "primitives: Animate: true natively as the log's",
                Some(Poll::Event(_)) if s.result != 1 && !logged_true => "primitives: Animate: false (an event) natively as the log's",
                _ => "primitives: Animate: DIFFERENT (the log's taken)",
            });
            // An event's value goes to the frame: the local operand byte 2 names with flag 0x20,
            // else parameter 0 (primitives.md, "counted events"); against the frame after the step.
            if s.result != 1 && !logged_true {
                let f = interp.top();
                if let Some(Poll::Event(v)) = native {
                    super::animate::write_event(f, n, v);
                }
                let same = f.locals == s.locals_after && f.params == s.params_after;
                self.counts.note(if same { "primitives: Animate: the event's value in the frame as the log's" } else { "primitives: Animate: the event's value DIFFERENT (the log's taken)" });
                if !same {
                    f.locals = s.locals_after.clone();
                    f.params = s.params_after.clone();
                }
            }
            if has("REPLAY_ANIMATE_BY_FLAGS") {
                note!(self.counts, "study animate flags {:#04x}: log {} native {}", n.operand[5], if s.result == 1 { "yield" } else if logged_true { "true" } else { "false" }, match native { Some(Poll::Pending) => "yield", Some(Poll::Done) => "true", Some(Poll::Event(_)) => "false", None => "-" });
            }
            if has("REPLAY_SHOW_ANIMATE") && native.is_some() && animate_differs(&native, s, logged_true) {
                let name = file.and_then(|f| lib.file(f).strings.get(&table).map(|t| (lib.file(f).path.rsplit('/').next().unwrap_or("").to_string(), t.get(n.operand16(0) as usize).map(|p| p.rsplit('\\').next().unwrap_or("").to_string()))));
                let o = self.world.get(me);
                println!("   queue: current {:?}; queued {:?}", (o.current.id, o.current.flags, o.current.tree), o.queue.iter().map(|q| (q.id, q.flags, q.tree, q.e)).collect::<Vec<_>>());
                println!("animate different: tick {} sim {:x} tree {} node {} result {} local0 {:?}: native {native:?}; source {} {name:?}; targets {} {}; operand {:02x?}", s.tick, s.sim, s.tree, s.node, s.result, s.locals_after.first(), n.operand[4], n.true_target, n.false_target, n.operand);
            }
            return self.logged(s, n);
        }
        if n.opcode == 27 || n.opcode == 45 {
            // GotoRelativePosition, GotoRoutingSlot: natively a yield until its run's return tick
            // (its walker's walk, turn and frames: `routing_return_tick`), where the run is
            // modelled; the log's result taken
            let params = interp.top().params.clone();
            let run = self.routing_returns.get(&(s.sim, s.tree, s.node)).and_then(|v| v.iter().filter(|r| r.0 <= s.tick).max_by_key(|r| r.0)).copied();
            if let Some((_, ret)) = run.filter(|r| s.tick <= r.1) {
                let native_yield = s.tick < ret;
                self.counts.note(if native_yield == (s.result == 1) {
                    "primitives: GotoRoutingSlot/GotoRelativePosition: yields or returns natively as the log's"
                } else {
                    "primitives: GotoRoutingSlot/GotoRelativePosition: DIFFERENT (the log's taken)"
                });
            } else if let Some(pl) = log.plans.iter().rfind(|p| p.tick == s.tick && p.approach.is_some_and(|a| a[7] as i16 == me_id)) {
                // the step that plans (its Sim's plan made this tick): with a way, it comes back to
                // its own node and walks from the next; with none, it goes on at once
                let native_again = !pl.points.is_empty();
                if native_again {
                    self.world.get_mut(me).asked_to_move = None; // a way: no one in it
                }
                // A route through a door: the step pushes the door's portal function (its leader's
                // function table entry 15, the door its stack object) and stays at its node, which
                // plans again when that returns (docs/simulation.md, "A route through a door")
                if let Some(door) = self.world.portal_on_route(&pl.points)
                    && let Some(tree) = self.world.function_tree(door, PORTAL_FUNCTION)
                {
                    let (did, dfile) = (self.world.get(door).id(), self.world.get(door).file);
                    let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                    let same = s.result == 0 && s.after == s.node && next.is_some_and(|x| x.frame > s.frame && x.node == 0 && x.tree == lib.tree_at(tree).id as u16 && x.stack_object == did);
                    self.counts.note(if same {
                        "primitives: GotoRoutingSlot/GotoRelativePosition: a route through a door, its portal pushed natively as the log's"
                    } else {
                        "primitives: GotoRoutingSlot/GotoRelativePosition: a route through a door, its portal pushed natively, NOT the log's"
                    });
                    if interp.push(lib, tree, &[]) {
                        interp.top().stack_object = did;
                        interp.top().file = dfile;
                        return PrimResult::Continue;
                    }
                }
                // A slot route with no way (a request of no goals) and a person on one of the slot's
                // places: the router asks them to move (its "route ask to move animation", the
                // person its stack object) and stays at its node (docs/simulation.md, "Asked to move")
                if pl.points.is_empty() && n.opcode == 45 {
                    let w = &self.world;
                    let places: Vec<(i32, i32)> = w
                        .find_object(s.stack_object)
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0), &params)?)))
                        .map(|(o, slot)| slot_places(slot, o.x16, o.y16, o.facing()).into_iter().map(|p| p.1).collect())
                        .unwrap_or_default();
                    if let Some(p) = w.person_on_places(me, &places)
                        && let Some(tree) = w.ask_to_move_tree(me)
                    {
                        let (pid, mfile) = (w.get(p).id(), w.get(me).file);
                        let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                        let same = s.result == 0 && s.after == s.node && next.is_some_and(|x| x.frame > s.frame && x.node == 0 && x.tree == lib.tree_at(tree).id as u16 && x.stack_object == pid);
                        self.counts.note(if same {
                            "primitives: GotoRoutingSlot: no way, a person on the slot asked to move natively as the log's"
                        } else {
                            "primitives: GotoRoutingSlot: no way, a person on the slot asked to move natively, NOT the log's"
                        });
                        self.world.get_mut(me).asked_to_move = Some(pid);
                        if interp.push(lib, tree, &[]) {
                            interp.top().stack_object = pid;
                            interp.top().file = mfile;
                            return PrimResult::Continue;
                        }
                    }
                }
                if n.opcode == 45
                    && let Some(chair) = self.world.seat_at_routing_slot(n, &params, s.stack_object, &pl.points)
                    && let Some(tree) = self.world.function_tree(chair, SIT_FUNCTION)
                {
                    let (cid, cfile) = (self.world.get(chair).id(), self.world.get(chair).file);
                    // the Sim's temps, as a call with default operands passes them (the log's 25, 198, 0, 0;
                    // the caller's own were 25, 198, 2, 0)
                    let params: Vec<i16> = interp.defaults().iter().take(4).copied().collect();
                    if has("REPLAY_SHOW_SIT") {
                        println!("sit push: tick {} sim {me_id} chair {cid}; params {params:?}; the log's {:?}", s.tick, log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim).map(|x| (x.tree, x.stack_object, x.params.clone())));
                    }
                    let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                    let same = s.result == 0 && s.after == s.node && next.is_some_and(|x| x.frame > s.frame && x.node == 0 && x.tree == lib.tree_at(tree).id as u16 && x.stack_object == cid && x.params[..] == params[..x.params.len().min(params.len())]);
                    self.counts.note(if same {
                        "primitives: GotoRoutingSlot: a sitting slot's seat, its sit function pushed natively as the log's"
                    } else {
                        "primitives: GotoRoutingSlot: a sitting slot's seat, its sit function pushed natively, NOT the log's"
                    });
                    if interp.push(lib, tree, &params) {
                        interp.top().stack_object = cid;
                        interp.top().file = cfile;
                        return PrimResult::Continue;
                    }
                }
                self.counts.note(if s.result == 0 && native_again == (s.after == s.node) {
                    "primitives: GotoRoutingSlot/GotoRelativePosition: the planning step natively as the log's (again with a way, on without one)"
                } else {
                    "primitives: GotoRoutingSlot/GotoRelativePosition: the planning step NOT as the log's"
                });
            } else {
                // Still in the way of a slot's places after being asked to move: the router does not
                // plan, it waits ("route waiting animation", on itself) and stays at its node
                // (docs/simulation.md, "Asked to move")
                if n.opcode == 45 {
                    let w = &self.world;
                    let places: Vec<(i32, i32)> = w
                        .find_object(s.stack_object)
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0), &params)?)))
                        .map(|(o, slot)| slot_places(slot, o.x16, o.y16, o.facing()).into_iter().map(|p| p.1).collect())
                        .unwrap_or_default();
                    if let Some(p) = w.person_on_places(me, &places)
                        && w.get(me).asked_to_move == Some(w.get(p).id())
                        && let Some(tree) = w.route_wait_tree(me)
                    {
                        let mfile = w.get(me).file;
                        let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                        let same = s.result == 0 && s.after == s.node && next.is_some_and(|x| x.frame > s.frame && x.node == 0 && x.tree == lib.tree_at(tree).id as u16 && x.stack_object == me_id);
                        self.counts.note(if same {
                            "primitives: GotoRoutingSlot: still in the way after asked, the wait natively as the log's"
                        } else {
                            "primitives: GotoRoutingSlot: still in the way after asked, the wait natively, NOT the log's"
                        });
                        if interp.push(lib, tree, &[]) {
                            interp.top().stack_object = me_id;
                            interp.top().file = mfile;
                            return PrimResult::Continue;
                        }
                    }
                }
                // A slot route whose Sim stands on one of the slot's places already: true at once,
                // with no plan (home-full tick 5066, Sim 12 at the dining table's slot)
                if n.opcode == 45 {
                    let w = &self.world;
                    let at = (w.get(me).x16, w.get(me).y16);
                    let on_slot = w
                        .find_object(s.stack_object)
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0), &params)?)))
                        .is_some_and(|(o, slot)| slot_places(slot, o.x16, o.y16, o.facing()).iter().any(|p| p.1 == at));
                    if on_slot {
                        let logged = self.logged(s, n);
                        self.counts.note(if logged == PrimResult::True {
                            "primitives: GotoRoutingSlot: its Sim on the slot's place already, true at once natively as the log's"
                        } else {
                            "primitives: GotoRoutingSlot: its Sim on the slot's place already, true at once natively, NOT the log's (the log's taken)"
                        });
                        return logged;
                    }
                }
                self.counts.note("primitives: GotoRoutingSlot/GotoRelativePosition: its run not modelled (the log's taken)");
                if has("REPLAY_SHOW_NO_WAY") {
                    let w = &self.world;
                    let places: Vec<(i32, i32)> = w
                        .find_object(s.stack_object)
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0), &params)?)))
                        .map(|(o, slot)| slot_places(slot, o.x16, o.y16, o.facing()).into_iter().map(|p| p.1).collect())
                        .unwrap_or_default();
                    let by = w.person_on_places(me, &places).map(|p| w.get(p).id());
                    let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim).map(|x| (x.tree, x.node, x.stack_object, x.frame > s.frame));
                    let me_at = (w.get(me).x16, w.get(me).y16);
                    println!("no plan: tick {} sim {me_id} {}:{} on {} res {} after {}; places {places:?}; person on them {by:?}; me at {me_at:?}; next {next:?}", s.tick, s.tree, s.node, s.stack_object, s.result, s.after);
                }
                if has("REPLAY_ROUTE_PUSHES") {
                    // study: what the run pushes (the stack's next step a frame up, at node 0)
                    let next = self.log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                    let what = match next {
                        Some(x) if x.frame > s.frame && x.node == 0 => format!("pushes {} on {}", x.tree, if x.stack_object == s.stack_object { "the same".to_string() } else { "another".to_string() }),
                        Some(x) if x.frame == s.frame => format!("goes on in its frame (after {})", s.after),
                        _ => "returns or other".to_string(),
                    };
                    note!(self.counts, "study route run: opcode {} result {} {what}", n.opcode, s.result);
                }
                if has("REPLAY_SHOW_ROUTE_RUNS") {
                    let plans: Vec<_> = log.plans.iter().filter(|p| p.tick + 5 >= s.tick && p.tick <= s.tick + 5).map(|p| (p.tick, p.approach.map(|a| a[7]), p.chosen, p.points.len())).collect::<Vec<_>>();
                    println!("   plans within 5 ticks of {}: {plans:?}; log ends at tick {}", s.tick, log.ticks);
                    let next: Vec<_> = log.steps[self.step + 1..].iter().filter(|x| x.sim == s.sim && x.tree == s.tree).take(4).map(|x| (x.tick, x.node, x.result, x.after)).collect();
                    println!("   the stack's next steps on the tree (tick, node, result, after): {next:?}");
                    let plans: Vec<_> = log.plans.iter().filter(|p| p.tick == s.tick).map(|p| (p.approach.map(|a| a[7]), p.chosen, p.points.len(), p.points.first().copied())).collect();
                    println!("route run not modelled: tick {} sim {me_id:?} tree {} node {} opcode {} result {} after {}; plans then (planner, chosen, points, start): {plans:?}", s.tick, s.tree, s.node, n.opcode, s.result, s.after);
                }
            }
            return self.logged(s, n);
        }
        if n.opcode == 25 && n.operand[4] == 0 {
            // AlterBudget that is not a test: the Sim's spending, shown when its turn ends (the
            // funds themselves the log's for now)
            self.world.get_mut(me).spent = true;
        }
        if n.opcode == 20 {
            // RunFunctionalTree: natively (primitives::run_functional_tree), against the log's next
            // step (that tree's node 0, a frame up, on the same stack object, with these parameters)
            let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
            let got = run_functional_tree(interp, self, n);
            let same = got == PrimResult::Continue
                && next.is_some_and(|x| {
                    let f = interp.top();
                    x.frame > s.frame && x.node == 0 && f.tree.is_some_and(|t| lib.tree_at(t).id as u16 == x.tree) && x.stack_object == f.stack_object && x.params[..] == f.params[..x.params.len().min(f.params.len())]
                });
            self.counts.note(if same { "primitives: RunFunctionalTree natively as the log's" } else { "primitives: RunFunctionalTree natively, NOT the log's" });
            if got == PrimResult::Continue {
                return got;
            }
            return self.logged(s, n);
        }
        if n.opcode == 8 {
            // Random: the engine's own numbers; the result's range checked, the log's value taken
            self.sync_but(interp, s, Some((n.operand[2], i16::from_le_bytes([n.operand[0], n.operand[1]]))));
            let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
            let got = random(n, &mut vars);
            // The original's number, where the log shows it (a parameter or local destination):
            // within the range, and then set natively so the run goes on with it.
            let dscope = n.operand[2];
            let datum = n.operand[0] as i32 | (n.operand[1] as i32) << 8;
            let range = vars.get(n.operand[6], (n.operand[4] as u16 | (n.operand[5] as u16) << 8) as i16);
            let after = match dscope {
                25 => s.locals_after.get(datum as usize).copied(),
                9 => s.params_after.get(datum as usize).copied(),
                // a temp or an attribute of mine: its first read by this stack after the step
                // (before anything else of this stack writes it)
                8 | 0 => log.steps[self.step + 1..]
                    .iter()
                    .filter(|t| t.sim == s.sim)
                    .take(64)
                    .find_map(|t| t.lookups.iter().find(|v| v.scope as u8 == dscope && v.data as i32 == datum && v.flags & 1 != 0).map(|v| v.value)),
                _ => None,
            };
            let note_range = match (after, range) {
                (Some(v), Some(range)) => {
                    let ok = v >= 0 && if range.value <= 0 { v == 0 } else { v < range.value };
                    vars.set(dscope, datum as i16, v);
                    match (ok, dscope) {
                        (true, 8 | 0) => "primitives: Random: the original's number within the range (a temp or attribute, from its next read)",
                        (true, _) => "primitives: Random: the original's number within the range",
                        _ => "primitives: Random: the original's number OUT of the range",
                    }
                }
                _ => "primitives: Random: the original's number not in the log (a temp or attribute)",
            };
            let want = self.logged(s, n);
            self.counts.note(if got == want { "primitives: Random: the result the same natively" } else { "primitives: Random: DIFFERENT result" });
            self.counts.note(note_range);
            return want;
        }
        if n.opcode == 17 && has("REPLAY_SHOW_IDLE_QUEUE") && self.world.get(me).person {
            let q: Vec<_> = self.world.get(me).queue.iter().map(|q| (q.tree, q.objects[1], q.interaction, q.rank)).collect();
            let next = log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim).map(|x| (x.tree, x.node, x.stack_object, x.frame > s.frame));
            println!("idle queue: tick {} sim {me_id} nested {}; queue (tree, target, interaction, rank) {q:?}; the log's next {next:?}", s.tick, self.nested.len());
        }
        if n.opcode == 17 && self.world.get(me).person && (self.world.get(me).queue.is_empty() || n.operand16(1) == 0) {
            // IdleForInput with nothing queued, or a wait that does not deliver: the wait's count
            let got = { let mut fl = self.world.get(me).data.get(crate::world::DATA_FLAGS).copied().unwrap_or(0); let r = idle_for_input_waiting(n, interp.top(), &mut fl, self.world.should_interrupt(me)); if let Some(d) = self.world.get_mut(me).data.get_mut(crate::world::DATA_FLAGS) { *d = fl; } r };
            let want = self.logged(s, n);
            if got == want {
                note!(self.counts, "primitives: IdleForInput waiting: the same natively, {}", if got == PrimResult::True { "true" } else { "a tick more" });
                return got;
            }
            self.counts.note("primitives: IdleForInput waiting: DIFFERENT (the log's taken)");
            if has("REPLAY_SHOW_DIFF") && self.shown.next("ifi", 8) {
                println!("IdleForInput waiting at {}:{} by {me_id}: native {}, log {} (result {}), params {:?}", s.tree, s.node, got as i32, want as i32, s.result, s.params);
            }
            return want;
        }
        if n.opcode == 17 && self.world.get(me).person && !self.world.get(me).queue.is_empty() && !self.nested.is_empty() {
            // IdleForInput delivering an entry
            let trace = has("REPLAY_TRACE_CHECK");
            if trace {
                println!("IdleForInput natively for Sim {me_id}, {} nested steps, at {}", self.nested.len(), self.nested_pos);
            }
            let got = idle_for_input(interp, self, me);
            if trace {
                println!("  -> {}, used {}", got as i32, self.nested_pos);
            }
            self.counts.note(if got == PrimResult::Continue {
                "queue: IdleForInput delivered the queued interaction natively"
            } else {
                "queue: IdleForInput natively did NOT deliver"
            });
            if got == PrimResult::Continue {
                self.natively_delivered.insert(me_id);
                return got;
            }
            return self.logged(s, n);
        }
        if n.opcode == 17 && self.world.get(me).person && !self.world.get(me).queue.is_empty() && self.nested.is_empty() {
            // IdleForInput with an entry whose interaction has no checks: delivered in the step
            // itself (the party controller's 4106 onto Sim 178 at the club's tick 156), where the
            // log's next step of this stack starts a tree at node 0 above it
            let next = log.steps[self.step + 1..].iter().find(|t| t.sim == s.sim);
            if next.is_some_and(|t| t.node == 0 && t.frame > s.frame) {
                let first = self.world.get(me).queue[0];
                let got = idle_for_input(interp, self, me);
                if got == PrimResult::Continue {
                    let nt = next.unwrap();
                    self.counts.note(if first.tree == nt.tree as i16 && first.objects[1] == nt.stack_object {
                        "queue: IdleForInput delivered natively with no checks, the log's interaction"
                    } else {
                        "queue: IdleForInput delivered natively with no checks, DIFFERENT"
                    });
                    self.natively_delivered.insert(me_id);
                    return got;
                }
            }
        }
        if n.opcode == 30 && let Some((id, t, file)) = self.chosen.take() {
            // GosubFoundAction: the evaluation's choice run, its action pushed with the chosen
            // object as stack object (the step goes on into it)
            if interp.push(self.lib, t, &[]) {
                interp.top().stack_object = id;
                interp.top().file = file;
                self.world.get_mut(me).current.c = self.world.push_priority(0, me);
                self.world.get_mut(me).current_at = Some(interp.top_index());
                let next = self.log.steps[self.step + 1..].iter().find(|x| x.sim == s.sim);
                self.counts.note(if next.is_some_and(|x| x.tree == self.lib.tree_at(t).id as u16 && x.node == 0 && x.stack_object == id) {
                    "primitives: GosubFoundAction natively, the log's action"
                } else {
                    "primitives: GosubFoundAction natively, DIFFERENT"
                });
                return PrimResult::Continue;
            }
        }
        if n.opcode == 14 && !has("REPLAY_LOGGED_FIND_BEST") {
            return self.find_best_object(interp, s, n);
        }
        if n.opcode == 3 && self.world.get(me).person && !self.nested.is_empty() {
            return self.evaluation(interp, s, n);
        }
        if n.opcode == 29 {
            // SetMotiveDelta
            let got = set_motive_delta(n, &mut ScriptVariables::new(&mut self.world, me, interp.top()));
            if let Some(got) = got {
                let want = self.logged(s, n);
                self.counts.note(if got == want { "primitives: SetMotiveDelta the same natively" } else { "primitives: SetMotiveDelta: DIFFERENT" });
                return got;
            }
            self.counts.note("primitives: SetMotiveDelta: an influence, the log's taken");
        }
        if n.opcode == 49 {
            // NotifyStackObject: the stack object's Idle or IdleForInput ends
            self.sync(interp, s);
            let got = notify_stack_object(&mut ScriptVariables::new(&mut self.world, me, interp.top()));
            let want = self.logged(s, n);
            if got == want {
                self.counts.note("primitives: NotifyStackObject: the same natively");
                return got;
            }
            self.counts.note("primitives: NotifyStackObject: DIFFERENT (the log's taken)");
            return want;
        }
        if n.opcode == 11 || n.opcode == 12 {
            // DistanceTo, DirectionTo: from the two places
            self.sync_but(interp, s, (n.opcode == 12).then(|| (n.operand[2], i16::from_le_bytes([n.operand[0], n.operand[1]]))));
            let name = if n.opcode == 11 { "DistanceTo" } else { "DirectionTo" };
            let got = {
                let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
                if n.opcode == 11 { distance_to(n, &mut vars) } else { direction_to(n, &mut vars) }
            };
            let want = self.logged(s, n);
            // (the direction goes to a variable, a local in every use logged: the frame after)
            let f = interp.top();
            if got == want && f.locals == s.locals_after && f.params == s.params_after {
                note!(self.counts, "primitives: {name}: the same natively");
                return got;
            }
            note!(self.counts, "primitives: {name}: DIFFERENT (the log's taken)");
            if has("REPLAY_SHOW_DIFF") {
                println!("{name} at {}:{} by {me_id} so {} operand {:02x?}: native {got:?} locals {:?}, log {want:?} locals {:?}", s.tree, s.node, f.stack_object, n.operand, f.locals, s.locals_after);
            }
            f.locals = s.locals_after.clone();
            f.params = s.params_after.clone();
            return want;
        }
        if n.opcode == 24 {
            // Relationship, the older form: the value to or from a parameter
            self.sync(interp, s);
            let got = relationship_old(n, &mut ScriptVariables::new(&mut self.world, me, interp.top()));
            let want = self.logged(s, n);
            let f = interp.top();
            if got == want && f.params == s.params_after {
                self.counts.note("primitives: Relationship: the same natively");
                return got;
            }
            self.counts.note("primitives: Relationship: DIFFERENT (the log's taken)");
            if has("REPLAY_SHOW_DIFF") {
                println!("Relationship at {}:{} by {me_id} so {} operand {:02x?}: native {got:?} params {:?}, log {want:?} params {:?}", s.tree, s.node, f.stack_object, n.operand, f.params, s.params_after);
            }
            f.params = s.params_after.clone();
            return want;
        }
        if n.opcode == 26 {
            // Relationship2: the saved relationships, the objects' and the neighbours'
            self.sync(interp, s);
            let got = relationship(n, &mut ScriptVariables::new(&mut self.world, me, interp.top()));
            let want = self.logged(s, n);
            if has("REPLAY_SHOW_RELOPS") {
                let f = interp.top();
                println!("rel op {}:{} by {me_id} so {} operand {:02x?} locals {:?}: native {}, log {}", s.tree, s.node, f.stack_object, n.operand, f.locals, got as i32, want as i32);
            }
            // (a node whose two ends are one: the log cannot tell true from false)
            let either = n.true_target == n.false_target && matches!(got, PrimResult::True | PrimResult::False);
            if got == want || either {
                note!(self.counts, "primitives: Relationship2: the same natively, {}", if got == PrimResult::True { "true" } else { "false" });
                return got;
            }
            self.counts.note("primitives: Relationship2: DIFFERENT (the log's taken)");
            if has("REPLAY_SHOW_DIFF") && self.shown.next("rel", 12) {
                let f = interp.top();
                println!("Relationship2 at {}:{} by {me_id} so {} operand {:02x?}: native {}, log {}, params {:?}", s.tree, s.node, f.stack_object, n.operand, got as i32, want as i32, f.params);
            }
            return want;
        }
        if n.opcode == 56 {
            // CompleteGoal: the goal's mark in the save's progress
            self.sync(interp, s);
            let got = complete_goal(n, &mut ScriptVariables::new(&mut self.world, me, interp.top()));
            if let Some(got) = got {
                let want = self.logged(s, n);
                if got == want {
                    note!(self.counts, "primitives: CompleteGoal: the same natively, {}", if got == PrimResult::True { "true" } else { "false" });
                    return got;
                }
                self.counts.note("primitives: CompleteGoal: DIFFERENT (the log's taken)");
                if has("REPLAY_SHOW_DIFF") && self.shown.next("goal", 8) {
                    println!("CompleteGoal at {}:{} operand {:02x?}: native {}, log {}", s.tree, s.node, n.operand, got as i32, want as i32);
                }
                return want;
            }
            self.counts.note("primitives: CompleteGoal: the new form, the log's taken");
        }
        if n.opcode == 36 {
            // Dialog: the replay plays the player, answering when the original's did
            // (result 1: a wait the pass goes on after, the scenario controller 194's at the
            // home log's ticks 4 to 14; 2: a wait that stops the pass)
            if s.result == 0 {
                self.world.dialog_answer = Some(s.after == n.true_target as u16);
            }
            let stop_before = self.world.stop_pass;
            let got = dialog(n, &mut self.world, me_id);
            if s.result == 1 {
                self.world.stop_pass = stop_before;
            }
            self.world.presentation.clear();
            let same = match s.result {
                2 => got == PrimResult::Yield && self.world.stop_pass,
                1 => got == PrimResult::Yield,
                _ => got == self.logged(s, n),
            };
            self.counts.note(if same {
                if s.result == 2 { "primitives: Dialog: waiting, as the log's" } else { "primitives: Dialog: answered, as the log's" }
            } else {
                "primitives: Dialog: DIFFERENT"
            });
            return got;
        }
        if n.opcode == 18 && !has("REPLAY_LOGGED_OBJECTS") {
            // RemoveObjectInstance natively (the object the log's next list lacks); the result
            // the log's
            let so = interp.stack().last().map_or(0, |f| f.stack_object);
            let gone = remove_object(n, &mut self.world, me_id, so);
            let next = self.log.lists.iter().find(|l| l.tick >= s.tick);
            self.counts.note(match gone {
                Some(id) if next.is_some_and(|l| !l.ids.contains(&id)) => "objects: RemoveObjectInstance natively, the object the original removed",
                Some(_) => "objects: RemoveObjectInstance natively, NOT an object the original removed",
                None => "objects: RemoveObjectInstance: its object not found natively",
            });
            return self.logged(s, n);
        }
        if n.opcode == 42 && !has("REPLAY_LOGGED_OBJECTS") {
            // CreateObjectInstance natively (study REPLAY_LOGGED_OBJECTS: not made); an object
            // its init's steps made already (they come first in the log) is only placed
            let so = interp.stack().last().map_or(0, |f| f.stack_object);
            let made = if self.pre_made.remove(&s.stack_object_after)
                && let Some(k) = self.world.find(s.stack_object_after)
            {
                place_made(n, &mut self.world, k, me_id, so);
                Some(s.stack_object_after)
            } else {
                create_object(n, &mut self.world, me_id, so).map(|k| self.world.get(k).id())
            };
            self.counts.note(if made == Some(s.stack_object_after) {
                "objects: CreateObjectInstance natively, the id the original's"
            } else {
                "objects: CreateObjectInstance natively, the id DIFFERENT"
            });
            if let Some(id) = made {
                self.present.insert(id);
            }
            let f = interp.top();
            f.stack_object = made.unwrap_or(s.stack_object_after);
            return self.logged(s, n);
        }
        if matches!(n.opcode, 4 | 43 | 46) {
            // Grab, DropOnto, Snap natively on the world (who holds what, and where); the result
            // the log's. A held object's or a person's place is routing's (the log's 'P' records
            // set it each tick), so only who holds what is checked here, by the variables
            // scripts read afterwards (scopes 16, 20, data 2)
            let so = interp.stack().last().map_or(0, |f| f.stack_object);
            let done = match n.opcode {
                4 => grab(&mut self.world, me_id, so).is_some(),
                43 => drop_onto(&mut self.world, me_id, so).is_some(),
                _ => snap(n, &mut self.world, me_id, so).is_some(),
            };
            let name = match n.opcode {
                4 => "Grab",
                43 => "DropOnto",
                _ => "Snap",
            };
            note!(self.counts, "objects: {name} natively{}", if done { "" } else { ": its objects not found (or not modelled)" });
            if !done && has("REPLAY_SHOW_UNDONE") {
                let at = |id: i16| self.world.find_object(id).map(|o| (o.x16, o.y16, o.level, o.data.get(1).copied(), o.data[2], o.data[3]));
                let next = |id: i16| self.log.places.iter().find(|p| p.id == id && p.tick >= s.tick).map(|p| (p.tick, p.x, p.y, p.level));
                println!("undone {name} tick {} by {me_id} so {so} operand {:02x?} -> {}: me {:?} next {:?}; so {:?} next {:?}", s.tick, n.operand, s.after, at(me_id), next(me_id), at(so), next(so));
            }
            if let Some(w) = env("REPLAY_WATCH_ID")
                && let Ok(w) = w.parse::<i16>()
                && (me_id == w || so == w)
            {
                let c = self.world.find_object(w).map(|o| (o.data[2], o.data[3]));
                println!("watch {w}: tick {} {name} by {me_id} so {so} operand {:02x?} done {done}: now container/slot {c:?}", s.tick, n.operand);
            }
            return self.logged(s, n);
        }
        if n.opcode == 13 {
            // PushAction
            let got = push_action(interp, self, me, n);
            if has("REPLAY_SHOW_PUSHACTION") {
                let so = interp.top().stack_object;
                let q: Vec<(i16, i16, i32)> = self.world.find(so).map(|k| self.world.get(k).queue.iter().map(|q| (q.tree, q.objects[1], q.c)).collect()).unwrap_or_default();
                println!("pushaction: tick {} by {me_id} {}:{} onto {so} operand {:02x?}; queue now {q:?}", s.tick, s.tree, s.node, &n.operand[..8]);
            }
            let want = self.logged(s, n);
            self.counts.note(if got == want {
                "queue: PushAction the same natively"
            } else if got == PrimResult::Stop {
                "queue: PushAction not modelled (the log's taken)"
            } else {
                "queue: PushAction DIFFERENT"
            });
            return want;
        }
        if matches!(n.opcode, 6 | 41 | 50) {
            // the presentation's: an accessory, a balloon, the action's name
            let so = interp.top().stack_object;
            let got = if n.opcode == 6 { change_suit(n, &mut self.world, me_id) } else { Some(balloon_or_string(n, &mut self.world, me_id, so)) };
            self.world.presentation.clear(); // no front end here
            if let Some(got) = got {
                let want = self.logged(s, n);
                let name = match n.opcode {
                    6 => "ChangeSuit",
                    41 => "SetBalloon",
                    _ => "MakeActionString",
                };
                let f = interp.top();
                if got == want && f.params == s.params_after && f.locals == s.locals_after && f.stack_object == s.stack_object_after {
                    note!(self.counts, "primitives: {name}: the same natively");
                    return got;
                }
                note!(self.counts, "primitives: {name}: DIFFERENT (the log's taken)");
                f.params = s.params_after.clone();
                f.locals = s.locals_after.clone();
                f.stack_object = s.stack_object_after;
                return want;
            }
        }
        if matches!(n.opcode, 23 | 7 | 48) {
            // the presentation's requests
            let got = present(n, &mut self.world, me_id);
            if n.opcode == 23 && !self.counts.quiet {
                let ask = super::sound::ask(self.lib, &self.world, self.file_of_top, me_id, interp.top().stack_object, n.operand, self.counts.tick);
                self.counts.sound_asks.push(ask);
            }
            self.world.presentation.clear(); // no front end here
            let want = self.logged(s, n);
            self.counts.note(if got == want {
                "primitives: PlaySound/Update/KillSounds: the same natively"
            } else {
                "primitives: PlaySound/Update/KillSounds: DIFFERENT (the log's taken)"
            });
            return want;
        }
        if n.opcode == 28 {
            // CallNamedTree
            let so_before = interp.top().stack_object;
            let got = call_named_tree(interp, n, &self.world);
            if got == PrimResult::Continue {
                if has("REPLAY_SHOW_CNT") && self.shown.next(&format!("cnt {me_id}"), 3) {
                    let f = interp.top();
                    let t = f.tree.map(|t| self.lib.tree_at(t).id);
                    println!("CallNamedTree tick {} by {me_id} at {}:{}: stack object {so_before}, called {t:?} from {:?}; the log's step {}:{} -> {}", s.tick, s.tree, s.node, f.file.map(|x| self.lib.file(x).path.clone()), s.tree, s.node, s.after);
                }
                self.counts.note("primitives: CallNamedTree: a tree called natively");
                return got;
            }
            let want = self.logged(s, n);
            // the same node either way (true and false both to it) is the same step
            let goes = |r: PrimResult| match r {
                PrimResult::True => Some(n.true_target),
                PrimResult::False => Some(n.false_target),
                _ => None,
            };
            let got = if goes(got).is_some() && goes(got) == goes(want) { want } else { got };
            if has("REPLAY_SHOW_CNT") && got != want {
                let so = self.world.find_object(so_before);
                println!("CallNamedTree DIFFERENT tick {} by {me_id} at {}:{} operand {:?}: stack object {so_before} (person {:?}, file {:?}); native {got:?}, the log's {}", s.tick, s.tree, s.node, &n.operand[..8], so.map(|o| o.person), so.and_then(|o| o.file).map(|f| self.lib.file(f).path.clone()), s.after);
            }
            self.counts.note(if got == want {
                "primitives: CallNamedTree: the same natively (no tree)"
            } else {
                "primitives: CallNamedTree: DIFFERENT (the log's taken)"
            });
            return want;
        }
        if n.opcode == 32 {
            // TestObjectType
            self.sync(interp, s);
            let t0 = Instant::now();
            let mut got = test_object_type(n, &mut ScriptVariables::new(&mut self.world, me, interp.top()));
            if let Some(b) = &mut self.bench {
                b.other_us += us(t0);
            }
            let want = self.logged(s, n);
            if got == want {
                note!(self.counts, 
                    "primitives: TestObjectType: the same natively, {}",
                    if got == PrimResult::True { "true" } else { "false" }
                );
            } else {
                self.counts.note("primitives: TestObjectType: DIFFERENT (the log's taken)");
                if has("REPLAY_SHOW_DIFF") && self.shown.next("tot", 8) {
                    let mut l = format!("TestObjectType at {}:{}:", s.tree, s.node);
                    for v in &s.lookups {
                        l += &format!(" [{}/{}={}]", v.scope, v.data, v.value);
                    }
                    println!("{l} native {}, log {}", got as i32, want as i32);
                }
                got = want;
            }
            return got;
        }
        if n.opcode == 2 || n.opcode == 31 {
            // SetToNext's read of its target variable is logged as 0 whatever it holds (seen
            // with the stack object), so only the expression's reads are taken; SetToNext starts
            // from the frame as synced.
            if n.opcode == 2 {
                self.sync(interp, s);
            }
            let t0 = Instant::now();
            let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
            let mut got = if n.opcode == 2 { expression(&ExpressionOperand::of(n), &mut vars).result } else { set_to_next(n, &mut vars) };
            if let Some(b) = &mut self.bench {
                let d = us(t0);
                if n.opcode == 2 {
                    b.expression_us += d;
                } else {
                    b.set_to_next_us += d;
                }
            }
            let want = self.logged(s, n);
            if n.opcode == 2 && (s.tree == 462 || s.tree == 463) && has("REPLAY_SHOW_462") && self.shown.next(&format!("462 {}", self.world.globals.get(10).copied().unwrap_or(0)), 40) {
                let e = ExpressionOperand::of(n);
                let x = s.expr.as_ref().map(|x| (x.lhs, x.lhs_after, x.rhs));
                println!("462/463 (global 10 {:?}) {}:{} -> {}: lhs {}/{} op {} rhs {}/{}: logged lhs, after, rhs {x:?}; reads {:?}", self.world.globals.get(10), s.tree, s.node, s.after, e.lhs_scope, e.lhs_data, e.op, e.rhs_scope, e.rhs_data, s.lookups.iter().map(|v| (v.scope, v.data, v.value)).collect::<Vec<_>>());
            }
            let k = || if n.opcode == 2 { "primitives: expression".to_string() } else { format!("primitives: SetToNext kind {}", n.operand[4] & 0x7F) };
            // a node whose two ways are one node: the log's next node does not tell true from
            // false (logged as true), so the same where native's way leads there too
            let one_way = n.true_target == n.false_target && matches!(got, PrimResult::True | PrimResult::False) && matches!(want, PrimResult::True | PrimResult::False);
            // native at another node than the log's step (a check tree run natively off the
            // original's path, the log's next step not this node's): no comparison
            let f = interp.top();
            let here = f.tree.map(|t| (self.lib.tree_at(t).id as u16, f.node));
            if here.is_some_and(|h| h != (s.tree, s.node)) {
                note!(self.counts, "{}: native at another node than the log's step (not compared)", k());
                return want;
            }
            if got == want {
                note!(self.counts, "{}: the same natively", k());
            } else if one_way {
                note!(self.counts, "{}: the same way natively (both its ways one node)", k());
            } else {
                note!(self.counts, "{}: DIFFERENT (the log's taken)", k());
                if n.opcode == 2 && has("REPLAY_SHOW_EXPR_DIFFS") && self.shown.next("expr diff", if has("REPLAY_SHOW_EXPR_ALL") { 1_000_000 } else { 200 }) {
                    let e = ExpressionOperand::of(n);
                    let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
                    let lhs = vars.get(e.lhs_scope, e.lhs_data).map(|v| v.value);
                    let rhs = vars.get(e.rhs_scope, e.rhs_data).map(|v| v.value);
                    let lhs_f = vars.get(e.lhs_scope, e.lhs_data).and_then(|v| v.f);
                    let logged: Vec<String> = s.lookups.iter().map(|v| format!("{}/{}={}", v.scope, v.data, v.value)).collect();
                    println!("expression differs: tick {} me {me_id} tree {}:{} operand {:02x?}: native {:?}, log {:?}; native lhs {lhs:?} ({lhs_f:?}) rhs {rhs:?}; logged {logged:?}", s.tick, s.tree, s.node, n.operand, got, want);
                }
                let kind = n.operand[4] & 0x7F;
                if n.opcode == 31 && kind == 6 && has("REPLAY_KIND6") {
                    // which data fields of the object found equal the frame's first parameter
                    if let Some(x) = self.world.find_object(s.stack_object_after)
                        && want == PrimResult::True
                        && !s.params.is_empty()
                    {
                        let steps = self.shown.0.entry("kind6 steps".into()).or_default();
                        *steps += 1;
                        let steps = *steps;
                        for (k, &v) in x.data.iter().enumerate() {
                            if v == s.params[0] {
                                *self.shown.0.entry(format!("kind6 hit {k:05}")).or_default() += 1;
                            }
                        }
                        if steps % 200 == 0 || steps == 1 {
                            let mut l = format!("kind 6 after {steps} true steps; fields equal to param 0:");
                            let mut hits: Vec<(i32, i32)> = self
                                .shown
                                .0
                                .iter()
                                .filter_map(|(k, &c)| k.strip_prefix("kind6 hit ").map(|f| (f.parse().unwrap(), c)))
                                .collect();
                            hits.sort();
                            for (k, c) in hits {
                                if c > steps / 2 {
                                    l += &format!(" {k}:{c}");
                                }
                            }
                            println!("{l}");
                        }
                    }
                }
                if n.opcode == 31 && has("REPLAY_SHOW_STN") && self.shown.next(&format!("stn {kind}"), 6) {
                    let mut l = format!("kind {kind} at {}:{} by {me_id} operand", s.tree, s.node);
                    for b in n.operand {
                        l += &format!(" {b:02x}");
                    }
                    l += &format!(
                        ": so {} -> {}, {};",
                        s.stack_object,
                        s.stack_object_after,
                        if want == PrimResult::True { "true" } else { "false" }
                    );
                    for id in [s.stack_object, s.stack_object_after] {
                        if let Some(x) = self.world.find_object(id) {
                            l += &format!(
                                " [{id} guid {:08x} person {} data 0..2 {} {} {}]",
                                x.guid, x.person as i32, x.data[0], x.data[1], x.data[2]
                            );
                        }
                    }
                    println!("{l}");
                }
                got = want;
            }
            // The frame as the original's after it (a SetToNext target, a local the expression
            // wrote are checked through the next step's lookups).
            return got;
        }
        if n.opcode == 22 && n.operand16(0) == 2 && self.world.get(me).person {
            // LookTowards the stack object: the person's follower is given a route that turns
            // it to face the object (`Replay::turn_followers`); the log's result taken
            if let Some(o) = self.world.find_object(interp.top().stack_object) {
                self.looks.insert(me_id, (s.tick, (o.x16, o.y16)));
            }
            // its turn-only route natively (the routing pass's return: its follower turned to the
            // object's place): a yield until then, and then true (the walker's result none: done)
            let run = self.routing_returns.get(&(s.sim, s.tree, s.node)).and_then(|v| v.iter().filter(|r| r.0 <= s.tick).max_by_key(|r| r.0)).copied();
            if let Some((_, ret)) = run.filter(|r| s.tick <= r.1) {
                let native = if s.tick < ret { PrimResult::Yield } else { PrimResult::True };
                let logged = self.logged(s, n);
                self.counts.note(if native == logged {
                    "primitives: LookTowards: its turn's yield or true natively as the log's"
                } else {
                    "primitives: LookTowards: its turn's yield or true DIFFERENT (the log's taken)"
                });
                return logged;
            }
        }
        if n.opcode == 0 {
            // Idle
            let was = interp.sleep().woke();
            let t0 = Instant::now();
            // (a stack saved at an Idle wakes there: the simulator's `stack_of` marks it)
            let may = may_sleep(self.world.get(me), false);
            if has("REPLAY_SHOW_IDLES") {
                let o = self.world.get(me);
                println!("idle t {} id {me_id} {}:{} d25 {:?} person {} params {:?} woke {was} may {may} log result {} after {}", s.tick, s.tree, s.node, o.data.get(25), o.person, interp.top().params, s.result, s.after);
            }
            let got = {
                let (sleep, f) = interp.sleep_and_top();
                idle(sleep, f, n, may)
            };
            if let Some(b) = &mut self.bench {
                b.idle_us += us(t0);
            }
            if !was && interp.sleep().woke() && has("REPLAY_SHOW_IDLE") {
                let c = self.shown.0.entry("idle".into()).or_default();
                *c += 1;
                let c = *c;
                let op = n.operand16(0) as usize;
                let f = interp.stack().last().unwrap();
                if c <= 15 {
                    let mut l = format!(
                        "Idle: object {me_id} at tick {}, {}:{}, parameter {}, operand {}, params",
                        self.now,
                        s.tree,
                        s.node,
                        f.params.get(op).map_or(-999, |&p| p as i32),
                        n.operand16(0)
                    );
                    for p in &s.params {
                        l += &format!(" {p}");
                    }
                    println!("{l}");
                }
            }
            let want = self.logged(s, n);
            if has("REPLAY_TRACE_251") && me_id == 251 && (250..=500).contains(&s.tick) {
                println!("idle 251 tick {} {}:{} -> {} result {}: native {got:?}, log {want:?}, params {:?}", s.tick, s.tree, s.node, s.after, s.result, interp.top().params);
            }
            self.counts.note(if got == want { "primitives: Idle: the same natively" } else { "primitives: Idle: DIFFERENT" });
            if got != want && has("REPLAY_SHOW_IDLE_DIFF") {
                let f = interp.stack().last().unwrap();
                let op = n.operand16(0) as usize;
                note!(self.counts, "study Idle diff: tree {}:{} native {got:?} log {want:?} param {}", s.tree, s.node, f.params.get(op).copied().unwrap_or(-999));
                let o = self.world.get(me);
                println!("idle diff tick {} by {me_id} (person {}) param {}: queue {} entries, operand {:02x?}, sleep woke {}", s.tick, o.person, f.params.get(op).copied().unwrap_or(-999), o.queue.len(), n.operand, interp.sleep().woke());
            }
            return got;
        }
        self.counts.note("primitives: from the log");
        note!(self.counts, "primitives by opcode: answered from the log, {}", n.opcode);
        if s.params != s.params_after || s.locals != s.locals_after || s.stack_object != s.stack_object_after {
            note!(self.counts, "primitives: from the log, opcode {}: changes its frame", n.opcode);
        }
        if env("REPLAY_SHOW_OP").is_some_and(|e| n.opcode == atoi(&e) as u16)
            && self.shown.next("op", if has("REPLAY_SHOW_ALL") { 100000 } else { 14 })
        {
            let f = interp.stack().last().unwrap();
            let mut l = format!(
                "op {} by {me_id} at {}:{} ({}) operand",
                n.opcode,
                s.tree,
                s.node,
                f.file.map_or("?".to_string(), |f| lib.file(f).path.clone())
            );
            for b in n.operand {
                l += &format!(" {b:02x}");
            }
            l += &format!(" -> {}; so {}; lookups", if s.after == n.true_target as u16 { "true" } else { "false" }, s.stack_object);
            for v in &s.lookups {
                l += &format!(" [{}/{}={}]", v.scope, v.data, v.value);
            }
            l += " params";
            for (k, p) in s.params.iter().enumerate() {
                l += &format!(" {p}>{}", s.params_after[k]);
            }
            l += " locals";
            for (k, p) in s.locals.iter().enumerate() {
                l += &format!(" {p}>{}", s.locals_after[k]);
            }
            println!("{l}");
        }
        if n.opcode == 14 && has("REPLAY_SHOW_FIND_BEST") {
            // study: FindBestObjectForFunction, the checks the log ran above this step (the
            // steps of this stack between its previous step and this one, at node 0)
            let prev = (0..self.step).rev().find(|&j| self.log.steps[j].sim == s.sim && self.log.steps[j].frame == s.frame).unwrap_or(0);
            let checks: Vec<(u16, i16, u16)> = (prev + 1..self.step).map(|j| &self.log.steps[j]).filter(|x| x.sim == s.sim && x.node == 0).map(|x| (x.tree, x.stack_object, x.after)).collect();
            println!("find best: tick {} by {me_id} {}:{} operand {:?} -> {} so after {}; checks {checks:?}", s.tick, s.tree, s.node, &n.operand[..4], s.after, s.stack_object_after);
            if has("REPLAY_SHOW_FIND_DEFS") {
                for id in [61i16, 47, 54, 55, 57] {
                    if let Some(o) = self.world.find_object(id) && let Some(f) = o.file {
                        println!("   functions of {id} ({}): {:?}", file_name(self.lib, f), self.lib.file(f).functions);
                    }
                }
            }

        }
        note!(self.counts, "primitives: from the log, opcode {}", n.opcode);
        let f = interp.top();
        f.params = s.params_after.clone();
        f.locals = s.locals_after.clone();
        f.unset = 0;
        f.stack_object = s.stack_object_after;
        self.logged(s, n)
    }
}
