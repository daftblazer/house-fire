//! The driven run (`replay --drive`): the engine decides what runs, the log is only the input
//! the engine cannot compute and the answer key.
//!
//! `Simulator::tick` runs every object's stack natively, each tick, through `DriveHost`, whose
//! primitives are the engine's own (the ones `ReplayHost` takes natively too, `crate::primitives`).
//! Where a primitive is not native yet, the host takes a documented neutral answer (`True`, the
//! frame unchanged), never the log's value, and the parting shows it.
//!
//! What is read from the log (the inputs the engine cannot compute, docs/plan.md "Built for
//! multiplayer"): which ticks are paused ones (no pass), the player's commands to a Sim (an
//! interaction on another object queued before the tick), a dialog's answer and the number a
//! Random drew (read from the log's step of the object at that node, only while the run agrees
//! with the log up to it).
//!
//! The answer key: each native step (the node's tree and number, where it goes, its result, the
//! frame's parameters and locals after it, its stack object) is compared online with the log's
//! next step of that object in that tick; the first difference is the parting and the run stops.

use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use crate::anim::Poll;
use crate::bhav::Node;
use crate::interp::{Error, Host, Interpreter, Mode, PrimResult, Step, Trees};
use crate::library::Library;
use crate::primitives::{
    balloon_or_string, call_named_tree, find_best_object, change_suit, complete_goal, create_object, dialog, direction_to, distance_to, drop_onto, grab, idle, idle_for_input,
    idle_for_input_waiting, may_sleep, notify_stack_object, present, push_action, random, relationship, relationship_old, remove_object, run_functional_tree, set_motive_delta,
    set_to_next, snap, test_object_type,
};
use crate::expression::{ExpressionOperand, Variables, expression};
use crate::save::{LotChunks, Neighbour};
use crate::simulator::{SimHost, Simulator};
use crate::tree_log::{Log, StepRec};
use crate::world::{ObjectKey, ScriptVariables, World};
use crate::{FileId, TreeId};
use super::animate::{AnimReplay, animate, write_event};
use super::run::Replay;
use super::alone::Tally;
use super::start::RunWith;

/// What a driven run found.
pub struct DriveReport {
    /// The ticks of the log the run was the log's on, from tick 1.
    pub same: u32,
    /// The log's ticks.
    pub of: u32,
    pub parting: Option<Parting>,
}

/// The first step the native run and the log differ at.
pub struct Parting {
    pub tick: u32,
    pub object: i16,
    pub tree: i32,
    pub node: u16,
    pub text: String,
}

/// A native step, as the log records a step of its own kind.
struct Native {
    tree: i32,
    node: u16,
    after: u16,
    /// The node's opcode and operand.
    op: i32,
    operand: [u8; 8],
    /// 1: the node yielded.
    yielded: bool,
    params: Vec<i16>,
    locals: Vec<i16>,
    unset: u64,
    stack_object: i16,
    /// The stack object and the parameters before the step.
    so_before: i16,
    params_before: Vec<i16>,
}

/// The host a driven run's scripts run with.
pub(super) struct DriveHost<'l> {
    pub(super) lib: &'l Library,
    pub(super) world: World<'l>,
    log: Rc<Log>,
    pub(super) me: ObjectKey,
    file_of_top: Option<FileId>,
    pub(super) tick: u32,
    /// The log's steps of each object this tick still to be matched, by object id.
    pub(super) expected: BTreeMap<i16, VecDeque<usize>>,
    /// The log's step the native step now being taken should match (the inputs read from it).
    front: Option<usize>,
    pub(super) parting: Option<Parting>,
    /// A turn asked the pass to stop (a dialog waiting).
    pub(super) stopped: bool,
    /// The last native steps (tree:node, opcode, where it went), for the parting.
    trail: VecDeque<String>,
    /// The last expression's reads, natively: (scope, datum, value) of its two sides.
    last_expr: [(u8, i16, Option<i16>); 2],
    /// Primitives answered with the neutral default, by opcode (printed with the parting).
    neutral: BTreeMap<u16, u32>,
    /// The animator (Animate 44) and the frames it is stepped by; none where the log has no
    /// animation records.
    pub(super) anim: Option<AnimReplay>,
    /// Each object's animator key (the Sim's address in the log), by object id.
    sim_of: BTreeMap<i16, u32>,
    /// A run alone (no log): its counts of what was answered neutrally and of the errors.
    pub(super) tally: Option<Tally>,
    /// The action the last evaluation chose: its object, tree and the object's file.
    pub(super) chosen: Option<(i16, TreeId, Option<FileId>)>,
    /// The tree of the step being taken (a run alone, for the counts).
    at_tree: i32,
    /// A player is there to answer a Dialog (a run alone, played): the dialog opens in the world
    /// (`primitives::dialog`) and waits for `World::dialog_answer`, instead of waiting for ever.
    pub(super) player: bool,
    /// The dialog now open for the player: the object that asked and its node's operand.
    pub(super) asked: Option<(i16, [u8; 8])>,
    /// The objects the player's clicks made (the go-here markers), whose checks start with
    /// parameter 0 = 0 instead of 1.
    pub(super) player_targets: Vec<i16>,
}

impl Trees for DriveHost<'_> {
    fn tree_at(&self, t: TreeId) -> &crate::bhav::Tree {
        self.lib.tree_at(t)
    }
}

impl<'l> SimHost<'l> for DriveHost<'l> {
    fn world(&mut self) -> &mut World<'l> {
        &mut self.world
    }
    fn set_me(&mut self, me: ObjectKey) {
        self.me = me;
    }

    /// The turn, a step at a time, each compared with the log's. A Sim that spent money shows it
    /// when its turn ends: the engine's "display money balloon" over the yielding frame, run in
    /// the same turn (docs/simulation.md, "The money balloon").
    fn run_turn(&mut self, interp: &mut Interpreter) -> u32 {
        if self.tally.is_none() {
            return self.turn(interp);
        }
        // A run alone contains a panic in one object's script code to that stack.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.turn(interp))) {
            Ok(steps) => {
                if interp.done() {
                    self.note_error(interp);
                    self.restart(interp);
                }
                steps
            }
            Err(_) => {
                interp.abort(Error::Panicked);
                self.note_error(interp);
                self.restart(interp);
                1
            }
        }
    }
}

impl DriveHost<'_> {
    /// The turn, a step at a time (each compared with the log's unless alone).
    fn turn(&mut self, interp: &mut Interpreter) -> u32 {
        let mut steps = 0;
        let me = self.me;
        loop {
            if self.parting.is_some() {
                break;
            }
            let r = if self.tally.is_some() { self.step_alone(interp) } else { self.step_checked(interp) };
            steps += 1;
            if self.tally.is_some() && super::study::env("REPLAY_ALONE_TRACE").is_some_and(|v| super::study::atoi(&v) == i64::from(self.me_id()) && self.tick >= 20000 && self.tick < 20003) {
                println!("trace tick {} object {} step {steps}: {:?} trail {:?} params {:?} queue {}", self.tick, self.me_id(), r, self.trail.back(), interp.stack().last().map(|f| f.params.clone()), self.world.get(self.me).queue.len());
            }
            match r {
                Step::Go => {}
                Step::Yield => {
                    if self.world.get_live(me).is_some_and(|o| o.spent)
                        && std::mem::take(&mut self.world.get_mut(me).spent)
                        && let Some(t) = self.world.money_balloon_tree(me)
                        && interp.push(self.lib, t, &[])
                    {
                        let f = interp.top();
                        f.engine = true;
                        f.stack_object = 0;
                        f.file = self.world.get(me).file;
                        continue;
                    }
                    break;
                }
                Step::Stop => break,
            }
        }
        self.stopped |= self.world.stop_pass;
        if self.parting.is_none() && self.tally.is_none() {
            let id = self.world.get_live(me).map(|o| o.id());
            if let Some(id) = id
                && let Some(&i) = self.expected.get(&id).and_then(|q| q.front())
            {
                let s = &self.log.steps[i];
                let text = format!("the native turn ended (yield or stop) with {} of the log's steps left", self.expected[&id].len());
                self.part(id, s.tree as i32, s.node, text);
            }
        }
        steps
    }

    /// A Sim whose stack ended (an error, a script stop) goes on from its bottom frame, its main
    /// tree from the start with no interaction: a Sim never stops living (a run alone). Other
    /// objects stay as they ended.
    fn restart(&mut self, interp: &mut Interpreter) {
        let Some(o) = self.world.get_live(self.me).filter(|o| o.person) else { return };
        let id = o.id();
        let Some(mut bottom) = interp.stack().first().cloned() else { return };
        bottom.node = 0;
        bottom.stack_object = 0;
        interp.restore(vec![bottom]);
        self.world.end_interaction(self.me);
        if let Some(t) = &mut self.tally {
            t.restarted += 1;
            t.walks.remove(&id);
            t.sim(id).open.clear();
        }
    }

    /// A stack that ended in an error, counted by its tree and the error.
    fn note_error(&mut self, interp: &Interpreter) {
        let tree = interp.stack().last().and_then(|f| f.tree).map_or(-1, |t| self.lib.tree_at(t).id as i32);
        let node = interp.stack().last().map_or(0, |f| f.node);
        // (a stack that ended with no error code was stopped by a primitive: its script error)
        let op = interp.stack().last().and_then(|f| f.tree.and_then(|t| self.lib.tree_at(t).nodes.get(f.node as usize))).map_or(-1, |n| n.opcode as i32);
        let kind = if interp.error() == Error::None { format!("a primitive stopped the stack (opcode {op})") } else { format!("{:?} ({})", interp.error(), interp.error() as i32) };
        let id = self.world.get_live(self.me).map_or(-1, |o| o.id());
        if super::study::has("REPLAY_ALONE_SHOW_ERRORS") {
            println!("error {kind} tick {} object {id} tree {tree} node {node}; trail {:?}", self.tick, self.trail);
        }
        if let Some(t) = &mut self.tally {
            t.error(tree, node, &kind, id);
        }
    }
}

impl DriveHost<'_> {
    fn part(&mut self, object: i16, tree: i32, node: u16, text: String) {
        if self.parting.is_none() {
            self.parting = Some(Parting { tick: self.tick, object, tree, node, text });
        }
    }

    fn me_id(&self) -> i16 {
        self.world.get(self.me).id()
    }

    /// One native step of a run alone: no comparison. A Sim's interactions are counted: one
    /// starts when a frame is pushed over its IdleForInput (a delivery) or over its Find Best
    /// Action (autonomy's choice), and is over when that frame is gone.
    fn step_alone(&mut self, interp: &mut Interpreter) -> Step {
        let lib = self.lib;
        let opcode = |i: &Interpreter| i.stack().last().and_then(|f| f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize))).map_or(-1, |n| n.opcode as i32);
        let (depth, op) = (interp.stack().len(), opcode(interp));
        if self.trail.len() >= 8 {
            self.trail.pop_front();
        }
        let at = interp.stack().last().map_or((-1, 0), |f| (f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node));
        self.at_tree = at.0;
        if super::study::has("REPLAY_ALONE_SHOW_ERRORS") {
            self.trail.push_back(format!("{}:{} op {op} so {}", at.0, at.1, interp.stack().last().map_or(0, |f| f.stack_object)));
        }
        // (the trees a call names are the top frame's file's)
        self.file_of_top = interp.stack().last().and_then(|f| f.file);
        let r = interp.step(self);
        let now = interp.stack().len();
        // (the object may have removed itself)
        let who = self.world.get_live(self.me).filter(|o| o.person).map(|o| o.id());
        if let Some(t) = &mut self.tally
            && let Some(id) = who
        {
            let s = t.sim(id);
            if now == depth + 1 && (op == 17 || op == 30) {
                s.started += 1;
                s.open.push(now);
                if super::study::has("REPLAY_ALONE_SHOW_STARTS") {
                    let f = interp.stack().last();
                    println!("start tick {} sim {id}: tree {} on object {}", self.tick, f.and_then(|f| f.tree).map_or(-1, |t| lib.tree_at(t).id as i32), f.map_or(0, |f| f.stack_object));
                }
            }
            while s.open.last().is_some_and(|&d| d > now) {
                s.open.pop();
                s.finished += 1;
            }
        }
        r
    }

    /// One native step of `interp` (the object's stack or one of its checks), compared with the
    /// log's next step of the object.
    fn step_checked(&mut self, interp: &mut Interpreter) -> Step {
        let lib = self.lib;
        let id = self.me_id();
        let depth = interp.stack().len();
        let top = interp.stack().last();
        let (tree, node, file) = top.map_or((-1, 0, None), |f| (f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node, f.file));
        self.front = self.expected.get(&id).and_then(|q| q.front()).copied();
        self.file_of_top = file;
        let op = top.and_then(|f| f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize))).map_or(-1, |x| x.opcode as i32);
        let operand = top.and_then(|f| f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize))).map_or([0; 8], |x| x.operand);
        let before = top.map(|f| (f.params.clone(), f.locals.clone(), f.unset, f.stack_object));
        let r = interp.step(self);
        let after = interp.stack().get(depth.wrapping_sub(1)).map(|f| (f.node, f.params.clone(), f.locals.clone(), f.unset, f.stack_object));
        // (a frame that returned: its node, parameters and locals as it left them)
        let so_before = before.as_ref().map_or(0, |b| b.3);
        let params_before = before.as_ref().map(|b| b.0.clone()).unwrap_or_default();
        let (anode, params, locals, unset, so) = match (after, before) {
            (Some(a), _) => a,
            (None, Some(b)) => (node, b.0, b.1, b.2, b.3),
            _ => (node, vec![], vec![], 0, 0),
        };
        let n = Native { tree, node, after: anode, op, operand, yielded: r == Step::Yield, params, locals, unset, stack_object: so, so_before, params_before };
        if self.trail.len() >= 6 {
            self.trail.pop_front();
        }
        self.trail.push_back(format!("{}:{} op {} -> {}", n.tree, n.node, n.op, n.after));
        self.compare(id, &n, r);
        r
    }

    fn compare(&mut self, id: i16, n: &Native, r: Step) {
        if self.parting.is_some() {
            return;
        }
        let Some(i) = self.expected.get_mut(&id).and_then(|q| q.pop_front()) else {
            let text = format!("native step {}:{} -> {} ({r:?}), the log has no more steps of it this tick", n.tree, n.node, n.after);
            self.part(id, n.tree, n.node, text);
            return;
        };
        let s: &StepRec = &self.log.steps[i];
        let mut diffs: Vec<String> = Vec::new();
        if n.tree != s.tree as i32 || n.node != s.node {
            diffs.push(format!("step: native {}:{}, log {}:{}", n.tree, n.node, s.tree, s.node));
        } else {
            if n.after != s.after {
                diffs.push(format!("goes to: native {}, log {}", n.after, s.after));
            }
            if n.yielded != (s.result != 0) {
                diffs.push(format!("result: native {}, log {}", if n.yielded { "yield" } else { "no yield" }, s.result));
            }
            if n.params != s.params_after {
                diffs.push(format!("params after: native {:?}, log {:?}", n.params, s.params_after));
            }
            // (a local never written is zero here and whatever the stack's memory held in the
            // original: not owed, docs/interpreter.md "Locals are not cleared")
            let locals_same = n.locals.len() == s.locals_after.len() && n.locals.iter().zip(&s.locals_after).enumerate().all(|(k, (a, b))| a == b || (k < 64 && n.unset >> k & 1 != 0));
            if !locals_same {
                diffs.push(format!("locals after: native {:?}, log {:?}", n.locals, s.locals_after));
            }
            if n.stack_object != s.stack_object_after {
                diffs.push(format!("stack object after: native {}, log {}", n.stack_object, s.stack_object_after));
            }
        }
        if diffs.is_empty() {
            return;
        }
        let op = n.op;
        let expr = if op == 2 { format!(" native reads (scope/datum=value) {:?}", self.last_expr) } else { String::new() };
        let neutral: Vec<_> = self.neutral.iter().map(|(o, c)| format!("{o}x{c}")).collect();
        let reads: Vec<String> = s.lookups.iter().map(|v| format!("{}/{}={} fl{} obj{:x} off{:#x}", v.scope, v.data, v.value, v.flags, v.object, v.ptr.wrapping_sub(v.object) as i32)).collect();
        let sos = format!("stack object before: native {}, log {}; params before: native {:?}, log {:?};", n.so_before, s.stack_object, n.params_before, s.params);
        let text = format!("opcode {op} {sos} operand {:02x?}: {}; the log's reads {reads:?};{expr} log step {i}, tick {}; native trail {:?}; neutral answers so far (opcode x count): {}", n.operand, diffs.join("; "), s.tick, self.trail, neutral.join(" "));
        self.part(id, s.tree as i32, s.node, text);
    }

    /// The log's step the native step now taken is, if it is the one at the node.
    fn log_step_here(&self, n: &Node, interp: &mut Interpreter) -> Option<&StepRec> {
        let s = &self.log.steps[self.front?];
        let f = interp.top();
        let here = f.tree.map(|t| (self.lib.tree_at(t).id as u16, f.node));
        let _ = n;
        (here == Some((s.tree, s.node))).then_some(s)
    }

    /// A primitive the engine cannot answer yet, the frame as it is (named in the parting):
    /// an animation or a walk (Animate 44, GotoRelativePosition 27, GotoRoutingSlot 45) is still
    /// going on, a yield; any other is true.
    pub(super) fn neutral(&mut self, n: &Node, why: &'static str) -> PrimResult {
        *self.neutral.entry(n.opcode).or_default() += 1;
        if let Some(t) = &mut self.tally {
            t.neutral(n.opcode, why, self.at_tree);
        }
        match n.opcode {
            // (a run alone has no animation to wait for where the archive lacks it)
            44 if self.tally.is_some() => PrimResult::True,
            44 | 27 | 45 => PrimResult::Yield,
            _ => PrimResult::True,
        }
    }
}

impl Host for DriveHost<'_> {
    fn interaction_returned(&mut self) {
        let me = self.me;
        self.world.end_interaction(me);
    }
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        let f = self.file_of_top.or(self.world.get_live(self.me).and_then(|o| o.file));
        self.lib.tree(f, id)
    }

    fn ad_range_mut(&mut self) -> Option<&mut Vec<i16>> {
        Some(&mut self.world.ad_range)
    }

    fn default_params(&mut self) -> Option<[i16; 8]> {
        self.world.get_live(self.me).map(|o| o.temps)
    }

    /// A check tree the engine runs, stepped natively and compared with the log's steps of it
    /// like any other.
    fn run_check(&mut self, t: TreeId, file: Option<FileId>, stack_object: i16, defaults: &[i16]) -> Option<bool> {
        if let Some(v) = self.world.get_mut(self.me).data.get_mut(crate::world::DATA_HIDE_INTERACTION) {
            *v = 0;
        }
        let mut chk = Interpreter::new(Mode::Check, 10000);
        chk.set_defaults(defaults.to_vec());
        // (a check tree starts with parameter 0 = 1; on a target the player's click made, 0:
        // `Alone::make_object`)
        chk.start(self.lib, Some(t), &[i16::from(!self.player_targets.contains(&stack_object))], file);
        if chk.done() {
            return None;
        }
        chk.top().stack_object = stack_object;
        let outer = self.file_of_top;
        while !chk.done() && self.parting.is_none() {
            // (a run alone keeps the iteration limit: a check that loops ends in an error)
            if self.tally.is_some() {
                self.step_alone(&mut chk);
            } else {
                chk.reset_iterations();
                self.step_checked(&mut chk);
            }
        }
        if self.tally.is_some() && chk.done() && chk.error() != Error::None {
            self.note_error(&chk);
        }
        self.file_of_top = outer;
        (chk.done() && chk.error() == Error::None && self.parting.is_none()).then(|| chk.result())
    }

    fn primitive(&mut self, interp: &mut Interpreter, n: &Node) -> PrimResult {
        let lib = self.lib;
        let me = self.me;
        // (an object that removed itself has nothing left to run)
        let Some(object) = self.world.get_live(me) else { return PrimResult::Stop };
        let (me_id, person) = (object.id(), object.person);
        match n.opcode {
            // Animate: natively, the answer the animator gives (yield, true, false with an event);
            // where the archive lacks the animation, neutral
            44 if self.anim.is_some() => {
                let sim = self.sim_of.get(&me_id).copied().unwrap_or(if self.tally.is_some() { 0x8000_0000 + me_id as u32 } else { 0 });
                let ar = self.anim.as_mut().expect("checked");
                if self.tally.is_none() {
                    ar.animating.entry(self.tick).or_default().insert(sim);
                }
                match animate(ar, lib, &self.world, me, interp, n, (sim, self.tick)).2 {
                    Some(Poll::Pending) => PrimResult::Yield,
                    Some(Poll::Done) => PrimResult::True,
                    Some(Poll::Event(v)) => {
                        write_event(interp.top(), n, v);
                        PrimResult::False
                    }
                    None => self.neutral(n, "animation not modelled"),
                }
            }
            // the autonomy of a run alone: the evaluation and the action it chose
            3 if self.tally.is_some() && person => self.evaluate(interp),
            30 if self.tally.is_some() => match self.chosen.take() {
                Some((id, t, file)) if interp.push(lib, t, &[]) => {
                    interp.top().stack_object = id;
                    interp.top().file = file;
                    PrimResult::Continue
                }
                _ => self.neutral(n, "no action chosen"),
            },
            22 if self.tally.is_some() => self.look_towards(interp, n),
            14 if self.tally.is_some() => find_best_object(interp, self, me, n),
            // the walks of a run alone: the engine's own (the log has no say)
            27 | 45 if self.tally.is_some() => self.go(interp, n),
            // Idle
            0 => {
                let may = may_sleep(self.world.get(me), false);
                let (sleep, f) = interp.sleep_and_top();
                idle(sleep, f, n, may)
            }
            // the expression, SetToNext
            2 => {
                let e = ExpressionOperand::of(n);
                if (e.lhs_scope == 23 || e.rhs_scope == 23) && super::study::has("REPLAY_DRIVE_SHOW_ROOM") {
                    let at = |id: i16| self.world.find_object(id).map(|o| (id, o.x16 >> 4, o.y16 >> 4, o.level, self.world.walls.room(o.x16 >> 4, o.y16 >> 4), o.file.map(|f| self.lib.file(f).path.rsplit('/').next().unwrap_or("").to_string())));
                    println!("room read: me {:?} temps {:?}; so {:?}; rooms {}", at(me_id), self.world.get(me).temps, at(interp.top().stack_object), self.world.walls.rooms());
                }
                let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
                self.last_expr = [(e.lhs_scope, e.lhs_data, vars.get(e.lhs_scope, e.lhs_data).map(|v| v.value)), (e.rhs_scope, e.rhs_data, vars.get(e.rhs_scope, e.rhs_data).map(|v| v.value))];
                expression(&e, &mut vars).result
            }
            31 => set_to_next(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            // Random: the engine's own draw; the original's number is the input, read from its
            // step where it is in a parameter or a local
            8 => {
                let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
                let got = random(n, &mut vars);
                let (dscope, datum) = (n.operand[2], n.operand[0] as usize | (n.operand[1] as usize) << 8);
                let node = vars.frame.node;
                let at = self.front.filter(|&i| self.log.steps[i].node == node);
                let given = at.and_then(|i| {
                    let s = &self.log.steps[i];
                    match dscope {
                        25 => s.locals_after.get(datum).copied(),
                        9 => s.params_after.get(datum).copied(),
                        // a temp or an attribute of mine (not in the step record): its first read
                        // by this stack after the step, as `replay` takes it (primitives.md)
                        8 | 0 => self.log.steps[i + 1..]
                            .iter()
                            .filter(|t| t.sim == s.sim)
                            .take(64)
                            .find_map(|t| t.lookups.iter().find(|v| v.scope as u8 == dscope && v.data as usize == datum && v.flags & 1 != 0).map(|v| v.value)),
                        _ => None,
                    }
                });
                if let Some(v) = given {
                    if matches!(dscope, 8 | 0) {
                        vars.set(dscope, datum as i16, v);
                    } else {
                        let f = interp.top();
                        let slot = if dscope == 25 { f.locals.get_mut(datum) } else { f.params.get_mut(datum) };
                        if let Some(x) = slot {
                            *x = v;
                        }
                        if dscope == 25 && datum < 64 {
                            f.unset &= !(1u64 << datum);
                        }
                    }
                }
                got
            }
            // RunFunctionalTree, PushAction
            20 => run_functional_tree(interp, self, n),
            13 => push_action(interp, self, n),
            29 => set_motive_delta(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())).unwrap_or_else(|| self.neutral(n, "form not modelled")),
            49 => notify_stack_object(&mut ScriptVariables::new(&mut self.world, me, interp.top())),
            11 => distance_to(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            12 => direction_to(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            24 => relationship_old(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            26 => relationship(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            56 => complete_goal(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())).unwrap_or_else(|| self.neutral(n, "new form not modelled")),
            32 => test_object_type(n, &mut ScriptVariables::new(&mut self.world, me, interp.top())),
            // IdleForInput: a Sim waits, or delivers what is queued for it
            17 if person && (self.world.get(me).queue.is_empty() || n.operand16(1) == 0) => { let mut fl = self.world.get(me).data.get(crate::world::DATA_FLAGS).copied().unwrap_or(0); let r = idle_for_input_waiting(n, interp.top(), &mut fl, !self.world.get(me).queue.is_empty()); if let Some(d) = self.world.get_mut(me).data.get_mut(crate::world::DATA_FLAGS) { *d = fl; } r },
            17 if person => idle_for_input(interp, self, me),
            // Dialog: the player's answer is the input (the log's, where its step is this one);
            // 1 a wait the pass goes on after, 2 a wait that stops it
            36 if self.player => {
                let opening = self.world.dialog_object != me_id;
                let got = dialog(n, &mut self.world, me_id);
                if opening {
                    self.asked = Some((me_id, n.operand));
                }
                if got == PrimResult::True {
                    self.asked = None;
                }
                self.world.presentation.clear();
                got
            }
            36 if self.tally.is_some() => {
                // no player to answer: the object waits at its Dialog for ever, and the pass goes
                // on (counted when it begins to wait)
                let at = self.at_tree;
                if self.tally.as_mut().is_some_and(|t| t.waiting.insert(me_id)) {
                    self.tally.as_mut().unwrap().neutral(36, "no player to answer (waits)", at);
                }
                PrimResult::Yield
            }
            36 => {
                let s = self.log_step_here(n, interp).map(|s| (s.result, s.after == n.true_target as u16));
                if let Some((0, yes)) = s {
                    self.world.dialog_answer = Some(yes);
                }
                let stop_before = self.world.stop_pass;
                let got = dialog(n, &mut self.world, me_id);
                if matches!(s, Some((1, _))) {
                    self.world.stop_pass = stop_before;
                }
                self.world.presentation.clear();
                got
            }
            18 => {
                let so = interp.top().stack_object;
                remove_object(n, &mut self.world, me_id, so);
                PrimResult::True
            }
            42 => {
                let so = interp.top().stack_object;
                let made = create_object(n, &mut self.world, me_id, so).map(|k| self.world.get(k).id());
                if let Some(id) = made {
                    interp.top().stack_object = id;
                }
                PrimResult::True
            }
            4 | 43 | 46 => {
                let so = interp.top().stack_object;
                match n.opcode {
                    4 => grab(&mut self.world, me_id, so).is_some(),
                    43 => drop_onto(&mut self.world, me_id, so).is_some(),
                    _ => snap(n, &mut self.world, me_id, so).is_some(),
                };
                PrimResult::True
            }
            6 | 41 | 50 => {
                let so = interp.top().stack_object;
                let got = if n.opcode == 6 { change_suit(n, &mut self.world, me_id) } else { Some(balloon_or_string(n, &mut self.world, me_id, so)) };
                self.world.presentation.clear();
                got.unwrap_or_else(|| self.neutral(n, "form not modelled"))
            }
            23 | 7 | 48 => {
                let got = present(n, &mut self.world, me_id);
                self.world.presentation.clear();
                got
            }
            28 => {
                // (the object's temps as they are now are the parameters it gives none of)
                interp.copy_defaults(&self.world.get(me).temps);
                call_named_tree(interp, n, &self.world)
            }
            _ => {
                let _ = lib;
                self.neutral(n, "not native")
            }
        }
    }
}

/// What a run has once its start is done: the host, the simulator and the log's indexes.
pub(super) struct Driven<'l> {
    pub(super) host: DriveHost<'l>,
    pub(super) sim: Simulator,
    pub(super) log: Rc<Log>,
    ids: BTreeMap<u32, i16>,
    by_tick: Vec<Vec<usize>>,
    engine_run: Vec<bool>,
    have_passes: bool,
}

/// The start both runs share: the lot loaded (`Replay::new`), the host made, the load pass run
/// (the log's tick-0 calls: which function of which object, see `load_init`) and the sleeps the
/// save lacks put in (`load_sleeps`). A parting in the load is in `host.parting`.
pub(super) fn setup<'l>(log: &Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], with: RunWith, tally: Option<Tally>) -> Option<Driven<'l>> {
    let rep = Replay::new(log, lib, lot, globals, neighbours, None, with)?;
    let Replay { log, h, ids, by_tick, engine_run, have_passes, mut sim, mut finder, .. } = rep;
    let mut host = DriveHost {
        lib,
        world: h.world,
        log: Rc::clone(&log),
        me: ObjectKey(0),
        file_of_top: None,
        tick: 0,
        expected: BTreeMap::new(),
        front: None,
        parting: None,
        stopped: false,
        last_expr: [(0, 0, None); 2],
        trail: VecDeque::new(),
        neutral: BTreeMap::new(),
        anim: h.anim,
        sim_of: ids.iter().map(|(&sim, &id)| (id, sim)).collect(),
        tally,
        chosen: None,
        at_tree: -1,
        player: false,
        asked: None,
        player_targets: Vec::new(),
    };
    if super::study::has("REPLAY_DRIVE_SHOW_FIRSTS") {
        let mut first: BTreeMap<i16, (u32, u16, u16)> = BTreeMap::new();
        for s in &log.steps {
            if let Some(&id) = ids.get(&s.sim) {
                first.entry(id).or_insert((s.tick, s.tree, s.node));
            }
        }
        for o in host.world.objects() {
            let top = o.stack.last().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.params.clone()));
            println!("firsts: object {} person {} saved top {top:?} first step {:?}", o.id(), o.person, first.get(&o.id()));
        }
    }
    if super::study::has("REPLAY_DRIVE_SHOW_PERSONS") {
        // study: each person's data as loaded, and its neighbour's (data 31) where differing
        for o in host.world.objects().filter(|o| o.person) {
            let nid = o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied().unwrap_or(0);
            let n = host.world.neighbours.iter().find(|n| n.id == nid);
            let diff: Vec<(usize, i16, i16)> = n.map(|n| o.person_data.iter().zip(&n.person_data).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).collect()).unwrap_or_default();
            println!("person {} neighbour {nid} ({:?}) data {:?}; differs from the neighbour's at {diff:?}", o.id(), n.map(|n| n.name.clone()), o.person_data);
        }
    }
    if let Some(id) = super::study::env("REPLAY_DRIVE_SHOW_MOTIVES").map(|v| super::study::atoi(&v) as i16) {
        // study: a person's motives as loaded, and the log's 'N' records of it to tick 3
        if let Some(o) = host.world.find_object(id) {
            println!("motives loaded {id}: {:?} / b {:?}", o.motives, o.motives_b);
            let nid = o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied().unwrap_or(0);
            if let Some(n) = host.world.neighbours.iter().find(|n| n.id == nid) {
                println!("motives neighbour {nid}: {:?} / b {:?}", n.motives.iter().map(|m| format!("{:08x}", m.to_bits())).collect::<Vec<_>>(), n.motives_b.iter().map(|m| format!("{:08x}", m.to_bits())).collect::<Vec<_>>());
            }
        }
        for m in log.motives.iter().filter(|m| m.id == id && m.tick <= 3) {
            println!("motives log tick {} {id}: {:?}", m.tick, m.motives);
        }
    }
    if let Some(v) = super::study::env("REPLAY_DRIVE_SHOW_LOG") {
        // study: the log's steps of one object in a tick range, "id:from:to"
        let p: Vec<i64> = v.split(':').map(super::study::atoi).collect();
        for (k, s) in log.steps.iter().enumerate() {
            if ids.get(&s.sim).is_some_and(|&id| id as i64 == p[0]) && (p[1]..=p[2]).contains(&(s.tick as i64)) {
                println!("log {k}: tick {} {}:{} -> {} look {} res {} frame {:x} so {} params {:?}->{:?} in_pass {} expr {:?}", s.tick, s.tree, s.node, s.after, if s.look != 0 { s.look as char } else { '-' }, s.result, s.frame & 0xFFFF, s.stack_object, s.params, s.params_after, s.in_pass, s.expr.as_ref().map(|e| (e.lhs_scope, e.lhs_data, e.lhs, e.lhs_after, e.rhs_scope, e.rhs_data, e.rhs, e.op)));
            }
        }
    }
    if super::study::has("REPLAY_DRIVE_SHOW_TICK0") {
        // study: the log's tick-0 steps (the load's), a line each time the stack changes
        let mut last = None;
        let mut n = 0;
        for s in log.steps.iter().filter(|s| s.tick == 0) {
            if last != Some(s.sim) {
                if let Some(l) = last {
                    println!("  ({n} steps)");
                    let _ = l;
                }
                n = 0;
                last = Some(s.sim);
                let entry = ids.get(&s.sim).and_then(|&id| host.world.find_object(id)).and_then(|o| o.file).and_then(|f| lib.file(f).functions.values().next()).and_then(|t| t.iter().position(|e| e.1 == s.tree));
                println!("tick 0: stack {:?} first {}:{} in_pass {} frame {:x} function entry {entry:?}", ids.get(&s.sim), s.tree, s.node, s.in_pass, s.frame & 0xFFFF);
            }
            n += 1;
        }
    }
    if let Some(v) = super::study::env("REPLAY_DRIVE_SHOW_LOOKUPS") {
        // study: the log's reads of one variable, "scope:datum:count"
        let p: Vec<i64> = v.split(':').map(super::study::atoi).collect();
        let mut shown = 0;
        for (si, s) in log.steps.iter().enumerate() {
            for l in s.lookups.iter().filter(|l| l.scope as i64 == p[0] && l.data as i64 == p[1]) {
                if shown < p[2] {
                    shown += 1;
                    if super::study::has("REPLAY_DRIVE_SHOW_BEFORE") {
                        for b in log.steps[..si].iter().rev().filter(|b| b.sim == s.sim).take(8).collect::<Vec<_>>().into_iter().rev() {
                            println!("   before: {}:{} -> {} reads {:?}", b.tree, b.node, b.after, b.lookups.iter().map(|v| (v.scope, v.data, v.value)).collect::<Vec<_>>());
                        }
                    }
                    let at = |id: i16| host.world.find_object(id).map(|o| (id, o.x16 >> 4, o.y16 >> 4, o.level, host.world.walls.room(o.x16 >> 4, o.y16 >> 4), o.file.map(|f| lib.file(f).path.rsplit('/').next().unwrap_or("").to_string())));
                    println!("read: tick {} {}:{} me {:?} so {} value {} flags {} obj {:x} ptr {:x}; me {:?}; so {:?}", s.tick, s.tree, s.node, ids.get(&s.sim), s.stack_object, l.value, l.flags, l.object, l.ptr, ids.get(&s.sim).and_then(|&i| at(i)), at(s.stack_object));
                }
            }
        }
        for o in host.world.objects().filter(|o| o.person) {
            println!("person {}: data[{}] {:?}", o.id(), p[1], o.person_data.get(p[1] as usize));
        }
    }
    // The load: what the engine calls on every object before the first tick (the log's tick 0).
    host.tick = 0;
    load_init(&mut host, &log, &ids, &mut finder);
    if host.parting.is_none()
        && let Some((&id, q)) = host.expected.iter().find(|(_, q)| !q.is_empty())
    {
        let s = &log.steps[q[0]];
        let text = format!("the load's native init did not run it; {} log steps", q.len());
        host.part(id, s.tree as i32, s.node, text);
    }
    if host.parting.is_none() {
        load_sleeps(&mut host, &mut sim, &log, &ids, &engine_run);
    }
    Some(Driven { host, sim, log, ids, by_tick, engine_run, have_passes })
}

/// The run driven by the engine over the log's inputs, to its end or the first parting.
pub fn drive<'l>(log: &Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], with: RunWith) -> Option<DriveReport> {
    let Driven { mut host, mut sim, log, ids, by_tick, engine_run, have_passes } = setup(log, lib, lot, globals, neighbours, with, None)?;
    let ticks = log.ticks;
    if host.parting.is_some() {
        return Some(DriveReport { same: 0, of: ticks, parting: host.parting });
    }
    let mut same = 0;
    // The animation frames due before a tick's first step (its place in the log, `tick_steps`):
    // the native frame clock's, or the log's track records before its first tick.
    let frames_before = |host: &mut DriveHost, t: u32| {
        if let Some(ar) = &mut host.anim {
            ar.frames_to(log.tick_steps.get(t as usize - 1).copied().unwrap_or(log.steps.len()));
        }
    };
    for t in 1..=ticks {
        host.tick = t;
        // A tick without a pass is a paused one: nothing runs and the clock does not move.
        if have_passes && !log.passes.contains_key(&t) {
            same = t;
            continue;
        }
        host.expected.clear();
        for &i in &by_tick[t as usize] {
            if engine_run[i] {
                continue;
            }
            let Some(&id) = ids.get(&log.steps[i].sim) else { continue };
            if host.world.find(id).is_some() {
                host.expected.entry(id).or_default().push_back(i);
            }
        }
        if let Some(id) = super::study::env("REPLAY_DRIVE_TEMPS").map(|v| super::study::atoi(&v) as i16)
            && let Some(o) = host.world.find_object(id)
        {
            println!("tick {t} start: object {id} temps {:?}", o.temps);
        }
        frames_before(&mut host, t);
        player_commands(&mut host, &mut sim, &log, t);
        host.stopped = false;
        host.world.stop_pass = false;
        host.neutral.clear();
        sim.tick(&mut host);
        if host.parting.is_none()
            && let Some((&id, q)) = host.expected.iter().find(|(_, q)| !q.is_empty())
        {
            let s = &log.steps[q[0]];
            let text = format!("the native turn did not run it (asleep, done or not reached); {} log steps", q.len());
            host.part(id, s.tree as i32, s.node, text);
        }
        if host.parting.is_some() {
            break;
        }
        host.world.clock_pass(host.stopped);
        same = t;
    }
    Some(DriveReport { same, of: ticks, parting: host.parting })
}

/// The player's command (input the log does not record as such): a Sim waiting at its
/// IdleForInput with nothing queued, whose log runs the checks of an interaction on another
/// object and then, from 281 node 4, that interaction's tree a frame up: the interaction is
/// queued as the player's, for the native IdleForInput to deliver with its checks (the same
/// reading as `Replay::step`'s).
fn player_commands(host: &mut DriveHost, sim: &mut Simulator, log: &Log, t: u32) {
    let ids: Vec<i16> = host.expected.keys().copied().collect();
    for id in ids {
        let Some(w) = host.world.find(id).filter(|&w| host.world.get(w).person && host.world.get(w).queue.is_empty()) else { continue };
        let q = &host.expected[&id];
        let Some(&i) = q.front() else { continue };
        let s = &log.steps[i];
        if s.node != 0 || s.stack_object == id {
            continue;
        }
        let win = sim.stack_of(&host.world, w);
        let lib = host.lib;
        let at_idle = !win.done() && win.stack().last().and_then(|f| f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize))).is_some_and(|n| n.opcode == 17);
        if !at_idle {
            continue;
        }
        let idle = q.iter().copied().find(|&j| j > i && log.steps[j].tree == 281 && log.steps[j].node == 4);
        let action = idle.and_then(|k| q.iter().copied().find(|&j| j > k)).map(|j| &log.steps[j]).filter(|x| x.node == 0 && idle.is_some_and(|k| x.frame > log.steps[k].frame));
        if let Some(a) = action
            && a.stack_object == s.stack_object
        {
            host.world.queue_player_command(w, a.stack_object, a.tree);
        }
    }
    let _ = t;
}

/// The sleeps the save lacks (docs/simulation.md, "A saved stack asleep at its Idle"): a stack
/// saved at an Idle was asleep there, for how long the save does not say (the log's first runs
/// of such stacks are at ticks 1, 2, 90, 1744, ... whatever their count). That is a load input
/// the engine cannot compute yet: each such stack is asleep until the tick of its first step in
/// the log's passes, none for ever if the log never runs it (a run alone, which has no log after
/// the start, wakes those at its first tick).
fn load_sleeps(host: &mut DriveHost, sim: &mut Simulator, log: &Log, ids: &BTreeMap<u32, i16>, engine_run: &[bool]) {
    let mut first: BTreeMap<i16, u32> = BTreeMap::new();
    for (i, s) in log.steps.iter().enumerate() {
        if s.tick >= 1
            && !engine_run[i]
            && let Some(&id) = ids.get(&s.sim)
        {
            first.entry(id).or_insert(s.tick);
        }
    }
    let lib = host.lib;
    let keys: Vec<ObjectKey> = host.world.objects().map(|o| host.world.find(o.id()).unwrap()).collect();
    for k in keys {
        let id = host.world.get(k).id();
        let i = sim.stack_of(&host.world, k);
        let at_idle = i.stack().last().and_then(|f| f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize))).is_some_and(|n| n.opcode == 0);
        if at_idle && !i.done() {
            i.sleep_and_top().0.sleep(first.get(&id).map_or(if host.tally.is_some() { 0 } else { u32::MAX }, |&t| t - 1), usize::MAX);
        }
    }
}

/// One call the engine made at the load: a function of an object run to its end.
struct LoadCall {
    /// The object it runs on, the tree, its parameters and its stack object (the call's own).
    me: i16,
    tree: u16,
    params: Vec<i16>,
    stack_object: i16,
    /// The log's steps of it, in order (the answer key).
    steps: Vec<usize>,
}

/// The load pass: what the engine calls on each object before the first tick (the log's tick 0,
/// not in a pass): an OBJf function of the object (Load, entry 2, for most; RoomChanged, entry 7,
/// for the lamps), run to its end, no stack object unless the call gives one. Which call is made
/// on which object, in what order and with what, is read from the log's runs (a load input: the
/// rule is not derived yet); everything that runs inside them is native and compared. A run's
/// object is the one its variable reads were made on (their object pointer against the log's
/// object list: the engine's temporary stacks are not an object's).
fn load_init(host: &mut DriveHost, log: &Log, ids: &BTreeMap<u32, i16>, finder: &mut super::finder::TreeFinder) {
    let lib = host.lib;
    let by_address: BTreeMap<u32, i16> = log.lists.first().map(|l| l.objects.iter().copied().zip(l.ids.iter().copied()).collect()).unwrap_or_default();
    let mut calls: Vec<LoadCall> = Vec::new();
    let mut open: BTreeMap<u32, (u32, usize)> = BTreeMap::new();
    for (i, s) in log.steps.iter().enumerate().filter(|(_, s)| s.tick == 0) {
        let Some(&sim_id) = ids.get(&s.sim) else { continue };
        if let std::collections::btree_map::Entry::Vacant(e) = open.entry(s.sim) {
            calls.push(LoadCall { me: sim_id, tree: s.tree, params: s.params.clone(), stack_object: s.stack_object, steps: Vec::new() });
            e.insert((s.frame, calls.len() - 1));
        }
        let (bottom, c) = open[&s.sim];
        let call = &mut calls[c];
        call.steps.push(i);
        if let Some(&id) = s.lookups.first().and_then(|l| by_address.get(&l.object))
            && call.steps.len() <= 2
        {
            call.me = id;
        }
        if bottom == s.frame && s.node > 250 {
            open.remove(&s.sim);
        }
    }
    for c in &calls {
        host.expected.entry(c.me).or_default().extend(c.steps.iter().copied());
    }
    for c in calls {
        let Some(k) = host.world.find(c.me) else { continue };
        // (a call with a stack object names its trees from that object's file: the home log's
        // tick-0 call of 8218 on Sim 12 with the paper girl 197 is NPC_SG's, not person_sg's)
        let file = host.world.find_object(c.stack_object).and_then(|o| o.file).or(host.world.get(k).file);
        let temps = host.world.get(k).temps.to_vec();
        let temps_before = temps.clone();
        host.me = k;
        let Some(t) = lib.tree(file, c.tree) else {
            host.part(c.me, c.tree as i32, 0, format!("the load's call: tree {} not found in the object's files", c.tree));
            return;
        };
        if super::study::has("REPLAY_DRIVE_SHOW_LOAD")
            && let Some((lt, lf)) = c.steps.first().and_then(|&i| finder.find(log, &log.steps[i]))
            && lt != t
        {
            println!("load call {} on {}: native tree in {:?} ({} args), the log's in {} ({} args); object's file {:?}, stack object {}", c.tree, c.me, file.map(|f| &lib.file(f).path), lib.tree_at(t).args, lib.file(lf).path, lib.tree_at(lt).args, file.map(|f| &lib.file(f).path), c.stack_object);
        }
        let mut chk = Interpreter::new(Mode::Check, 1 << 20);
        chk.set_defaults(temps);
        chk.start(lib, Some(t), &c.params, file);
        chk.top().stack_object = c.stack_object;
        while !chk.done() && host.parting.is_none() {
            chk.reset_iterations();
            host.step_checked(&mut chk);
        }
        if host.parting.is_some() {
            return;
        }
        // A run that is not one of the object's OBJf functions is a check: the original saves the
        // object's 8 temps before it and writes them back after (fn_8021944C, from the context's
        // +12 to the object at its +28; the club's 57: 8344 sets temp 1 to 57, the store at
        // 0x801F5CDC puts back 0)
        let is_function = host.world.get(k).file.and_then(|f| lib.file(f).functions.values().next()).is_some_and(|fs| fs.iter().any(|e| e.1 == c.tree));
        if !is_function {
            let saved = &temps_before;
            let o = host.world.get_mut(k);
            for (t, &v) in o.temps.iter_mut().zip(saved.iter()) {
                *t = v;
            }
        }
    }
}

