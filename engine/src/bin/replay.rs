//! The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
//! interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: replay LOG ISO [--card CARD] [--lot N] [--bench]
//!
//! --card: the lot from a save on a memory card image (its first lot, Mom's house in the user's
//! save, unless --lot says), not the disc's club; the neighbours are the save's too.
//!
//! The world is the disc's club lot, loaded natively (the log starts as the club loads): each
//! object's saved tree stack, attributes, temps, data, and the Sims' motives and person data.
//! The simulator says whose turn it is each tick; the turns are compared with the log's. A tick
//! without the pass over the objects is a paused one, whose clock does not move, and steps
//! outside the pass are the engine's own (the pie menu's), not run (the log's 'Q' records). Each
//! stack is then stepped natively for as many steps as the log has for it in that tick, and
//! before each step its top frame (depth, tree, node) is compared with the log's. The native
//! state is kept from step to step and tick to tick: frames, sleep, the variables.
//!
//! The primitives the engine has run natively (the expression, Idle, SetToNext, ...) on the
//! native world; each variable they read is compared with what the original's lookup found
//! before it is used. The others are answered from the log: their result, and their frame's
//! parameters, locals and stack object after them.
//!
//! Where the native state differs from the log's it is counted and then set to the log's, so
//! that the run goes on: a frame (an interaction the engine pushed, which is not native yet), a
//! variable (one a primitive that is not native wrote). Objects that are not the lot's (the
//! household, made as the lot loads, and objects made later) are not run.
//!
//! Its output is the C++ tool's, line for line; of its studies (REPLAY_SHOW_...), the two that
//! showed native addresses show object keys.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::ExitCode;
use std::time::Instant;

use housefire::autonomy::{
    Candidate, EvaluationCheck, Valued, attenuation, autonomy_candidates, autonomy_list, autonomy_pick, candidate_value, interaction_score,
    person_distance, run_evaluation, score_curves,
};
use housefire::bhav::{Node, Tree, TreeKind, tree_kind};
use housefire::cfmt::g;
use housefire::disc::Disc;
use housefire::expression::{ExpressionOperand, Variables, expression};
use housefire::interp::{Error, Frame, Host, Interpreter, Mode, PrimResult, Trees};
use housefire::library::{Advert, Library};
use housefire::motives::{MotiveRates, motive, update_motives};
use housefire::primitives::{
    call_named_tree, dialog, idle, idle_for_input, present, push_action, random, set_motive_delta, set_to_next, test_object_type,
};
use housefire::routing::{Tile, front, slot_places};
use housefire::save::{LotChunks, Neighbour, memory_card_file, read_lot_arrays, read_lots, read_neighbours};
use housefire::simulator::{SimHost, Simulator};
use housefire::tree_log::{Log, PassRec, StepRec, read_log, same_shape};
use housefire::world::{ObjectKey, ScriptVariables, World};
use housefire::{FileId, TreeId};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
fn has(name: &str) -> bool {
    std::env::var_os(name).is_some()
}
/// atoi: the leading integer of a string, 0 if none.
fn atoi(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    sign * digits.parse::<i64>().unwrap_or(0)
}
fn file_name(lib: &Library, f: FileId) -> String {
    let p = &lib.file(f).path;
    p[p.rfind('/').map_or(0, |s| s + 1)..].to_string()
}

/// A count whose kind is formatted, only when counting.
macro_rules! note {
    ($counts:expr, $($arg:tt)*) => {
        if !$counts.quiet {
            $counts.note(&format!($($arg)*));
        }
    };
}

/// What was seen, counted by kind (--bench: nothing, only the timings).
#[derive(Default)]
struct Counts {
    map: BTreeMap<String, usize>,
    quiet: bool,
}

impl Counts {
    fn note(&mut self, k: &str) {
        self.add(k, 1);
    }
    fn add(&mut self, k: &str, n: usize) {
        if !self.quiet {
            *self.map.entry(k.to_string()).or_default() += n;
        }
    }
}

/// --bench: the native work's time, per tick and per part. The replay's own work (finding trees,
/// comparing with the log, setting the native state to the log's) is not counted.
#[derive(Default, Clone)]
struct Bench {
    /// The native work of each tick that ran, in µs.
    ticks: Vec<f64>,
    steps_us: f64,
    expression_us: f64,
    set_to_next_us: f64,
    idle_us: f64,
    other_us: f64,
    pass_us: f64,
    sync_us: f64,
    steps: usize,
    worst_tick: usize,
}

fn us(a: Instant) -> f64 {
    a.elapsed().as_secs_f64() * 1e6
}

/// A logged tree on the disc: the file whose tree of that id has those nodes, by the nodes'
/// address and the tree's id. The game reuses an unloaded tree's memory for others, and the log
/// records a tree only the first time it is at an address, so a step's record can be another
/// tree's: only one of the step's own id is used.
struct TreeFinder<'l> {
    lib: &'l Library,
    cache: HashMap<(u32, u16), Option<(TreeId, FileId)>>,
    shown: i32,
}

impl TreeFinder<'_> {
    fn find(&mut self, log: &Log, s: &StepRec) -> Option<(TreeId, FileId)> {
        if let Some(&r) = self.cache.get(&(s.nodes, s.tree)) {
            return r;
        }
        let t = log.tree_of(s).filter(|t| t.tree == s.tree)?; // not cached: a later step may have its record
        let mut r = None;
        for (k, f) in self.lib.files().iter().enumerate() {
            if let Some(&id) = f.trees.get(&s.tree)
                && same_shape(self.lib.tree_at(id), &t.raw)
            {
                r = Some((id, FileId(k as u32)));
                break;
            }
        }
        if r.is_none()
            && let Some(want) = env("REPLAY_SHOW_SHAPE")
            && s.tree == atoi(&want) as u16
            && self.shown < 2
        {
            self.shown += 1;
            let mut line = format!("tree {} at {:08x}: {} nodes logged:", s.tree, s.nodes, t.raw.len() / 12);
            for (k, b) in t.raw.iter().enumerate() {
                line += &format!("{}{b:02x}", if k % 12 != 0 { "" } else { " " });
            }
            println!("{line}");
            for f in self.lib.files() {
                let Some(&id) = f.trees.get(&s.tree) else { continue };
                let mut l = format!("  {}:", f.path);
                for n in &self.lib.tree_at(id).nodes {
                    l += &format!(
                        " {:04x} {:02x} {:02x} {:04x}{:04x}{:04x}{:04x}",
                        n.opcode,
                        n.true_target,
                        n.false_target,
                        n.operand16(0),
                        n.operand16(1),
                        n.operand16(2),
                        n.operand16(3)
                    );
                }
                println!("{l}");
            }
        }
        self.cache.insert((s.nodes, s.tree), r);
        r
    }
}

/// The studies' "shown" counts (the C++'s static ones).
#[derive(Default)]
struct Shown(HashMap<String, i32>);

impl Shown {
    /// Whether a study of this kind may print once more (below `limit`), counting it.
    fn next(&mut self, kind: &str, limit: i32) -> bool {
        let n = self.0.entry(kind.to_string()).or_default();
        *n += 1;
        *n <= limit
    }
}

/// The host the objects' scripts run with: the native primitives on the native world, the rest
/// from the log step being replayed.
struct ReplayHost<'a, 'l> {
    lib: &'l Library,
    world: World<'l>,
    log: &'a Log,
    /// The object whose stack is being stepped.
    me: ObjectKey,
    /// The log's step being replayed.
    step: usize,
    bench: Option<Bench>,
    file_of_top: Option<FileId>,
    /// The log's steps of the checks a primitive runs (the interaction queue's), in order.
    nested: Vec<usize>,
    nested_pos: usize,
    natively_delivered: BTreeSet<i16>,
    /// The objects on the lot this tick (the log's list).
    present: BTreeSet<i16>,
    /// Nested records already used out of order (calls').
    deferred: BTreeSet<usize>,
    /// The lot is a save's (--card).
    card_lot: bool,
    /// The next logged score ('A').
    score_at: usize,
    /// While an evaluation runs: its checks' results against the log's (same, different).
    on_check: Option<(usize, usize)>,
    counts: Counts,
    shown: Shown,
    /// The simulator's ticks run (for a study).
    now: u32,
}

/// Brings the native world's variables to what the original's lookups found, counting those
/// that were already the same.
fn sync_variables(world: &mut World, counts: &mut Counts, shown: &mut Shown, me: ObjectKey, f: &mut Frame, s: &StepRec) {
    let mut vars = ScriptVariables::new(world, me, f);
    for v in &s.lookups {
        if v.flags & 1 == 0 {
            continue; // the lookup found nothing
        }
        let Some(got) = vars.get(v.scope as u8, v.data) else {
            note!(counts, "variables: scope {}: not modelled", v.scope);
            continue;
        };
        if v.flags & 4 != 0 {
            let lf = f32::from_bits(v.fvalue);
            if got.f == Some(lf) {
                note!(counts, "variables: scope {}: the same natively", v.scope);
                continue;
            }
            note!(counts, "variables: scope {}: different (set to the log's)", v.scope);
            vars.set_float(v.scope as u8, v.data, lf);
            continue;
        }
        if got.value == v.value {
            note!(counts, "variables: scope {}: the same natively", v.scope);
            continue;
        }
        if env("REPLAY_SHOW_SCOPE").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope", 10) {
            println!(
                "scope {} datum {}: native {}, log {}; the step's stack object {} -> {}, tree {}:{}",
                v.scope, v.data, got.value, v.value, s.stack_object, s.stack_object_after, s.tree, s.node
            );
        }
        note!(counts, "variables: scope {}: different (set to the log's)", v.scope);
        vars.set(v.scope as u8, v.data, v.value);
    }
}

impl ReplayHost<'_, '_> {
    fn sync(&mut self, interp: &mut Interpreter, s: &StepRec) {
        let t0 = Instant::now();
        sync_variables(&mut self.world, &mut self.counts, &mut self.shown, self.me, interp.top(), s);
        if let Some(b) = &mut self.bench {
            b.sync_us += us(t0);
        }
    }

    /// The primitive's result as the log has it.
    fn logged(&mut self, s: &StepRec, n: &Node) -> PrimResult {
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

    fn me_id(&self) -> i16 {
        self.world.get(self.me).id()
    }
}

impl Trees for ReplayHost<'_, '_> {
    fn tree_at(&self, t: TreeId) -> &Tree {
        self.lib.tree_at(t)
    }
}

impl<'l> SimHost<'l> for ReplayHost<'_, 'l> {
    fn world(&mut self) -> &mut World<'l> {
        &mut self.world
    }
    fn set_me(&mut self, me: ObjectKey) {
        self.me = me;
    }
}

impl Host for ReplayHost<'_, '_> {
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        let f = self.file_of_top.or(self.world.get(self.me).file);
        self.lib.tree(f, id)
    }

    /// A check tree the engine runs, stepped natively through the log's steps of it.
    fn run_check(&mut self, t: TreeId, file: Option<FileId>, stack_object: i16, defaults: &[i16]) -> Option<bool> {
        let log = self.log;
        let lib = self.lib;
        let mut chk = Interpreter::new(Mode::Check, 10000);
        chk.set_defaults(defaults.to_vec());
        chk.start(lib, Some(t), &[], file);
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
            if trace && self.me_id() == 169 {
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
        let log = self.log;
        let lib = self.lib;
        let s = &log.steps[self.step];
        let me = self.me;
        let me_id = self.me_id();
        if n.opcode == 8 {
            // Random: the engine's own numbers; the result's range checked, the log's value taken
            self.sync(interp, s);
            let mut vars = ScriptVariables::new(&mut self.world, me, interp.top());
            let got = random(n, &mut vars);
            // The original's number, where the log shows it (a parameter or local destination):
            // within the range, and then set natively so the run goes on with it.
            let dscope = n.operand[2];
            let datum = n.operand[0] as i32 | (n.operand[1] as i32) << 8;
            let range = vars.get(n.operand[6], (n.operand[4] as u16 | (n.operand[5] as u16) << 8) as i16);
            let after = match dscope {
                25 => Some(&s.locals_after),
                9 => Some(&s.params_after),
                _ => None,
            };
            let note_range = match (after, range) {
                (Some(after), Some(range)) if (datum as usize) < after.len() => {
                    let v = after[datum as usize];
                    let ok = v >= 0 && if range.value <= 0 { v == 0 } else { v < range.value };
                    vars.set(dscope, datum as i16, v);
                    if ok { "primitives: Random: the original's number within the range" } else { "primitives: Random: the original's number OUT of the range" }
                }
                _ => "primitives: Random: the original's number not in the log (a temp or attribute)",
            };
            let want = self.logged(s, n);
            self.counts.note(if got == want { "primitives: Random: the result the same natively" } else { "primitives: Random: DIFFERENT result" });
            self.counts.note(note_range);
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
        if n.opcode == 36 {
            // Dialog: the replay plays the player, answering when the original's did
            if s.result != 2 {
                self.world.dialog_answer = Some(s.after == n.true_target as u16);
            }
            let got = dialog(n, &mut self.world, me_id);
            self.world.presentation.clear();
            let same = if s.result == 2 { got == PrimResult::Yield && self.world.stop_pass } else { got == self.logged(s, n) };
            self.counts.note(if same {
                if s.result == 2 { "primitives: Dialog: waiting, as the log's" } else { "primitives: Dialog: answered, as the log's" }
            } else {
                "primitives: Dialog: DIFFERENT"
            });
            return got;
        }
        if n.opcode == 13 {
            // PushAction
            let got = push_action(interp, self, n);
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
        if matches!(n.opcode, 23 | 7 | 48) {
            // the presentation's requests
            let got = present(n, &mut self.world, me_id);
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
            let got = call_named_tree(interp, n, &self.world);
            if got == PrimResult::Continue {
                self.counts.note("primitives: CallNamedTree: a tree called natively");
                return got;
            }
            let want = self.logged(s, n);
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
            let k = || if n.opcode == 2 { "primitives: expression".to_string() } else { format!("primitives: SetToNext kind {}", n.operand[4] & 0x7F) };
            if got == want {
                note!(self.counts, "{}: the same natively", k());
            } else {
                note!(self.counts, "{}: DIFFERENT (the log's taken)", k());
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
        if n.opcode == 0 {
            // Idle
            let was = interp.sleep().woke();
            let t0 = Instant::now();
            let got = {
                let (sleep, f) = interp.sleep_and_top();
                idle(sleep, f, n)
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
            self.counts.note(if got == want { "primitives: Idle: the same natively" } else { "primitives: Idle: DIFFERENT" });
            return got;
        }
        self.counts.note("primitives: from the log");
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
        note!(self.counts, "primitives: from the log, opcode {}", n.opcode);
        let f = interp.top();
        f.params = s.params_after.clone();
        f.locals = s.locals_after.clone();
        f.unset = 0;
        f.stack_object = s.stack_object_after;
        self.logged(s, n)
    }
}

impl ReplayHost<'_, '_> {
    /// The autonomy evaluation (opcode 3), natively: its checks against the log's, its scores,
    /// the list and the choice.
    fn evaluation(&mut self, interp: &mut Interpreter, s: &StepRec, n: &Node) -> PrimResult {
        let log = self.log;
        let lib = self.lib;
        let me = self.me;
        let me_id = self.me_id();
        let cands: Vec<Candidate> = autonomy_candidates(&self.world, me)
            .into_iter()
            .filter(|c| self.present.is_empty() || self.present.contains(&c.id))
            .collect();
        // The engine's evaluation, each check's result against the log's: the step that ends it
        // (its bottom frame's return, 254 true or 255 false) is the last nested step used.
        self.on_check = Some((0, 0));
        if has("REPLAY_SHOW_AUTONOMY") {
            let mut l = format!("evaluation by {me_id}: {} candidates, present {}; objects", cands.len(), self.present.len());
            let mut lastid = -1;
            for c in &cands {
                if c.id != lastid {
                    lastid = c.id;
                    if lastid > 190 {
                        l += &format!(" {lastid}");
                    }
                }
            }
            println!("{l}; 197 present {}, in world {}", self.present.contains(&197) as i32, self.world.find(197).is_some() as i32);
        }
        let defaults = interp.defaults().to_vec();
        let checks: Vec<EvaluationCheck> = run_evaluation(lib, self, &cands, &defaults);
        let (same, different) = self.on_check.take().unwrap();
        let unfinished = checks.iter().filter(|c| c.passed.is_none()).count();
        let mut native_choice: Option<i16> = None;
        {
            let curves = score_curves(lib);
            let person = self.world.get(me);
            let base = interaction_score(&curves, None, person, &person.motives);
            let mut values: Vec<(f32, usize)> = Vec::new();
            let (mut scores_same, mut scores_diff) = (0, 0);
            for c in &checks {
                let Some(ci) = c.candidate else { continue };
                if c.passed != Some(true) {
                    continue;
                }
                let cand = &cands[ci];
                // The log's score of this one (the scores come in the order of the passing
                // guards).
                let native_score = interaction_score(&curves, Some(&cand.interaction(lib).adverts), person, &person.motives);
                while self.score_at < log.scores.len() && log.scores[self.score_at].tick < s.tick {
                    self.score_at += 1;
                }
                if self.score_at < log.scores.len() && log.scores[self.score_at].tick == s.tick {
                    if native_score == log.scores[self.score_at].score {
                        scores_same += 1;
                    } else {
                        scores_diff += 1;
                    }
                    self.score_at += 1;
                }
                values.push((candidate_value(lib, &curves, person, self.world.get(cand.object), cand, base), ci));
            }
            self.counts.add("autonomy: a candidate's score the same natively (its own adverts, the native motives)", scores_same);
            if scores_diff != 0 {
                self.counts.add("autonomy: a candidate's score DIFFERENT", scores_diff);
            }
            // The list and the choice natively (autonomy_list, autonomy_pick), against the
            // original's sorted list ('Y', the tick's last) and with its random number ('R').
            {
                let vs: Vec<Valued> = values.iter().map(|&(value, candidate)| Valued { value, candidate }).collect();
                let list = autonomy_list(lib, vs, false);
                let logged_list = log.choice_lists.iter().rfind(|l| l.tick == s.tick);
                let rnd = log.randoms.iter().rfind(|x| x.tick == s.tick);
                // The list's interaction number is the interaction's string index.
                let index_of = |c: &Candidate| c.interaction(lib).string_index;
                if let Some(ll) = logged_list {
                    let mut same = ll.entries.len() == list.len();
                    let (mut flags_same, mut flags_diff) = (0, 0);
                    for (i, e) in ll.entries.iter().enumerate().take(list.len()) {
                        let c = &cands[list[i].candidate];
                        same = same && e.id == c.id && e.interaction as i32 == index_of(c) && e.value == list[i].value;
                        if e.takes_best != 0 {
                            flags_diff += 1;
                        } else {
                            flags_same += 1;
                        }
                    }
                    self.counts.note(if same {
                        "autonomy: the choice's list the same natively (objects, interactions, values bit for bit)"
                    } else {
                        "autonomy: the choice's list DIFFERENT"
                    });
                    self.counts.add("autonomy: the list's entries without the takes-best flag", flags_same);
                    if flags_diff != 0 {
                        self.counts.add("autonomy: the list's entries WITH the takes-best flag (not built)", flags_diff);
                    }
                    if !same && has("REPLAY_SHOW_AUTONOMY") {
                        println!("list at tick {}: log {}, native {}", s.tick, ll.entries.len(), list.len());
                        for i in 0..list.len().max(ll.entries.len()) {
                            let mut l = match ll.entries.get(i) {
                                Some(e) => format!("  log {}/{}={}", e.id, e.interaction, g(e.value as f64, 9)),
                                None => "  log -".to_string(),
                            };
                            if i < list.len() && i < ll.entries.len() && has("REPLAY_SHOW_DISTANCE") {
                                let c = &cands[list[i].candidate];
                                let e = c.interaction(lib);
                                let o = self.world.get(c.object);
                                let sc = interaction_score(&curves, Some(&e.adverts), person, &person.motives);
                                let att = attenuation(lib, e, false);
                                let implied = if att > 0.0 { ((sc - base) / ll.entries[i].value - 1.0) / att } else { -1.0 };
                                l += &format!(
                                    " [d {:.4} implied {:.4} att {:.2} me {},{},{} obj {},{},{}]",
                                    person_distance(person, o),
                                    implied,
                                    att,
                                    person.x16,
                                    person.y16,
                                    person.level,
                                    o.x16,
                                    o.y16,
                                    o.level
                                );
                            }
                            match list.get(i) {
                                Some(v) => {
                                    let c = &cands[v.candidate];
                                    let e = c.interaction(lib);
                                    l += &format!(
                                        "   native {}/{}(s{},f{:x})={}",
                                        c.id,
                                        index_of(c),
                                        e.string_index,
                                        e.flags,
                                        g(v.value as f64, 9)
                                    );
                                }
                                None => l += "   native -",
                            }
                            println!("{l}");
                        }
                    }
                }
                if let Some(rnd) = rnd {
                    let at = autonomy_pick(lib, &list, rnd.value);
                    let ok = at.is_some_and(|a| cands[list[a].candidate].id == s.stack_object_after);
                    self.counts.note(if ok {
                        "autonomy: the native choice (the original's random number) the log's"
                    } else {
                        "autonomy: the native choice (the original's random number) NOT the log's"
                    });
                    if has("REPLAY_SHOW_AUTONOMY") {
                        println!(
                            "pick at tick {}: random {}, index {}, native {}, log {}",
                            s.tick,
                            rnd.value,
                            at.map_or(-1, |a| a as i64),
                            at.map_or(-1, |a| cands[list[a].candidate].id as i64),
                            s.stack_object_after
                        );
                    }
                    if ok {
                        native_choice = Some(cands[list[at.unwrap()].candidate].id);
                    }
                }
            }
            values.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let rank = values.iter().position(|v| cands[v.1].id == s.stack_object_after).map_or(0, |p| p + 1);
            self.counts.note(if (1..=4).contains(&rank) {
                "autonomy: the log's choice among the native best four"
            } else {
                "autonomy: the log's choice NOT among the native best four"
            });
            if has("REPLAY_SHOW_AUTONOMY") {
                let mut l = format!("choice of {me_id}: object {}, native rank {rank} of {}; best:", s.stack_object_after, values.len());
                for v in values.iter().take(4) {
                    l += &format!(" {}/{}={}", cands[v.1].id, cands[v.1].interaction(lib).action, g(v.0 as f64, 6));
                }
                println!("{l}");
            }
        }
        self.counts.add("autonomy: an evaluation's checks, result as the log's", same);
        self.counts.add("autonomy: an evaluation's checks, result DIFFERENT", different);
        if unfinished != 0 {
            self.counts.add("autonomy: an evaluation's checks, not finished", unfinished);
        }
        // The choice: the native one, made with the original's random number (the engine's own
        // numbers differ, docs/plan.md); the log's where there is none.
        interp.top().stack_object = native_choice.unwrap_or(s.stack_object_after);
        if native_choice.is_some() {
            self.counts.note("autonomy: an evaluation's result (the stack object) made natively");
        }
        self.logged(s, n)
    }
}

/// The log's stacks: which object each is, from the stack's own variable lookups (scopes 0, 3,
/// 8) and the logged object lists.
fn stack_ids(log: &Log) -> BTreeMap<u32, i16> {
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
fn place_quiet_stacks(log: &Log, world: &World, ids: &mut BTreeMap<u32, i16>, counts: &mut Counts, lib: &Library) {
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
fn top_node(lib: &Library, i: &Interpreter) -> Option<Node> {
    let f = i.stack().last()?;
    lib.tree_at(f.tree?).nodes.get(f.node as usize).copied()
}

/// One run of the club over the log: with bench, timed and not counted. The counts, or `None`
/// if the log does not start with the lot loaded.
fn run(log: &Log, lib: &Library, club: &LotChunks, neighbours: &[Neighbour], bench: Option<Bench>, card_lot: bool) -> Option<(Counts, Option<Bench>)> {
    let mut world = World::new(lib);
    world.load_lot(club).expect("the lot");
    if has("REPLAY_SHOW_OBJECTS") {
        for o in world.objects() {
            let mut l = format!("object {}: guid {:08x}, {}; data", o.id(), o.guid, o.file.map_or("-".to_string(), |f| lib.file(f).path.clone()));
            for v in &o.data {
                l += &format!(" {v}");
            }
            println!("{l}");
        }
    }
    if let Some(dump) = env("REPLAY_DUMP_ARRAYS") {
        // the lot's three arrays, raw, for study
        let bytes: Vec<u8> = read_lot_arrays(club).unwrap_or_default().iter().flat_map(|a| a.bytes.iter().copied()).collect();
        let _ = std::fs::write(dump, bytes);
    }
    // The disc's club: its motives do not move by themselves (measured; World::motives_run).
    world.motives_run = card_lot;
    world.neighbours = neighbours.to_vec();
    if log.lists.first().is_none_or(|l| l.ids.len() != world.len()) {
        println!(
            "the log does not start with the lot loaded ({} objects in its first list, {} in the lot)",
            log.lists.first().map_or(0, |l| l.ids.len()),
            world.len()
        );
        return None;
    }
    let quiet = bench.is_some();
    let mut h = ReplayHost {
        lib,
        world,
        log,
        me: ObjectKey(0),
        step: 0,
        bench,
        file_of_top: None,
        nested: Vec::new(),
        nested_pos: 0,
        natively_delivered: BTreeSet::new(),
        present: BTreeSet::new(),
        deferred: BTreeSet::new(),
        card_lot,
        score_at: 0,
        on_check: None,
        counts: Counts { quiet, ..Default::default() },
        shown: Shown::default(),
        now: 0,
    };
    let mut consumed: BTreeSet<usize> = BTreeSet::new(); // log steps a native primitive's checks have stepped
    let mut sim = Simulator::new(lib);
    let mut finder = TreeFinder { lib, cache: HashMap::new(), shown: 0 };
    let mut ids = stack_ids(log);
    place_quiet_stacks(log, &h.world, &mut ids, &mut h.counts, lib);

    // The log's steps by tick, in order.
    let mut by_tick: Vec<Vec<usize>> = vec![Vec::new(); log.ticks as usize + 1];
    for (i, s) in log.steps.iter().enumerate() {
        if s.tick >= 1 && s.tick <= log.ticks {
            by_tick[s.tick as usize].push(i);
        }
    }
    let mut frames: BTreeMap<u32, Vec<u32>> = BTreeMap::new(); // the log's stacks: frame addresses, bottom first
    let mut first_seen: BTreeMap<u32, u32> = BTreeMap::new(); // stack -> tick of its first step
    let mut first_step: BTreeSet<u32> = BTreeSet::new(); // stacks whose first step was seen
    let mut unseen: BTreeSet<u32> = BTreeSet::new(); // placeholder addresses of saved frames below a first step
    let mut delivered: BTreeSet<u32> = BTreeSet::new(); // Sims whose first interaction was seen
    let mut frame_tree: HashMap<(u32, u32, u16), (TreeId, FileId)> = HashMap::new();
    let (mut streak, mut best_streak) = (0usize, 0usize);

    // Which steps are turns: those in the tick's pass over the objects ('Q' records). The others
    // are the engine's own (the pie menu's checks on a Sim's stack: docs/simulation.md). A log
    // without them is taken as every step a turn.
    let have_passes = !log.passes.is_empty();
    let engine_run: Vec<bool> = log.steps.iter().map(|s| have_passes && !s.in_pass).collect();
    if !have_passes {
        h.counts.note("log: no pass records (an older log): every step taken as a turn, no tick as paused");
    }

    let mut place_at = 0; // the next 'P' record (places: routing is not native, so set to the log's)
    let mut motive_at = 0; // the next 'N' record
    let mut shown_motives = 0;
    let ev_env = env("REPLAY_EVALUATION");
    for t in 1..=log.ticks {
        let mut tick_us = 0.0;
        while place_at < log.places.len() && log.places[place_at].tick < t {
            let p = log.places[place_at];
            if let Some(k) = h.world.find(p.id) {
                let o = h.world.get_mut(k);
                o.x16 = p.x;
                o.y16 = p.y;
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
            continue;
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
            while *done < motives_due.len() && world.get(motives_due[*done].0).id() < id {
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

        // The steps, in the log's order.
        for &i in &by_tick[t as usize] {
            let s = &log.steps[i];
            if engine_run[i] {
                h.counts.note("steps: outside the pass (the engine's own), not run");
                continue;
            }
            if consumed.contains(&i) {
                continue;
            }
            if let Some(&id0) = ids.get(&s.sim) {
                motives_before(id0, &sim, &mut h.world, &mut motives_done);
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
                    // Native where it is checked: the save's lot (Mom's house, all three
                    // evaluations exact); not the disc's club, whose evaluations look otherwise
                    // (docs/simulation.md, "Autonomy"). REPLAY_EVALUATION=0 or 1 overrides.
                    let evaluation = match &ev_env {
                        Some(e) => e.starts_with('1'),
                        None => h.card_lot,
                    };
                    if let (true, Some(en), Some((etree, enode, efile))) = (evaluation, en, etop)
                        && en.opcode == 3
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
                        if k != i && nest.len() >= 20 {
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
                            ein.step(&mut h);
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
                        win.step(&mut h);
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
                while fr.last().is_some_and(|&b| b > s.frame) {
                    fr.pop();
                    popped = true;
                }
                if popped && fr.last().is_some_and(|b| unseen.contains(b)) {
                    unseen.remove(fr.last().unwrap());
                    *fr.last_mut().unwrap() = s.frame;
                } else {
                    fr.push(s.frame);
                }
            }
            first_seen.entry(s.sim).or_insert(t);
            let Some(ok) = ids.get(&s.sim).and_then(|&id| h.world.find(id)) else {
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
            let Some((tree, file)) = tf else {
                h.counts.note("steps: tree not found on the disc");
                note!(h.counts, 
                    "steps: tree not found: {}{}",
                    if s.t.is_some() { "tree " } else { "no tree record, tree " },
                    s.tree
                );
                continue;
            };
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
            if first_step.insert(s.sim) {
                // Each lot object's first step against the stack the lot saved.
                let same0 = st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.node);
                h.counts.note(if same0 { "load: a stack's first step at its saved top frame" } else { "load: a stack's first step NOT at its saved top frame" });
                // The log shows only the frames that step: the saved frames below the first
                // step's are taken as the log's too (addresses below its, never seen).
                let fr = frames.get_mut(&s.sim).unwrap();
                if same0 && st_len > fr.len() {
                    let mut below = Vec::new();
                    for k in (1..=st_len - fr.len()).rev() {
                        below.push(s.frame.wrapping_sub(16 * k as u32));
                        unseen.insert(*below.last().unwrap());
                    }
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
            let interp = sim.stack_of(&h.world, ok);
            let same = !interp.done() && interp.stack().len() == fr_len && st_top.is_some_and(|(t, n, _)| t == Some(s.tree as i16) && n == s.node);
            if same {
                h.counts.note("steps: the native stack as the log's");
                streak += 1;
                best_streak = best_streak.max(streak);
            } else {
                streak = 0;
                if has("REPLAY_SHOW_DIFFS") {
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
                    if h.shown.next(&format!("diffs {cat}"), 6) {
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
                    h.counts.note("steps: different: the native stack stopped");
                } else if st_len != fr_len {
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
                    h.counts.note("steps: different: another tree");
                    if has("REPLAY_SHOW_TREES") && h.shown.next("trees", 12) {
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
                } else {
                    h.counts.note("steps: different: another node");
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
                if interp.asleep() {
                    interp.sleep().wake();
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
                    if has("REPLAY_SHOW_SO") && h.shown.next("so", 12) {
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
                        println!("{l}");
                    }
                }
                f.params = s.params.clone();
                f.locals = s.locals.clone();
                f.stack_object = s.stack_object;
            }
            if interp.asleep() {
                h.counts.note("steps: the log ran a stack the native has asleep");
                interp.sleep().wake();
            }
            h.step = i;
            h.file_of_top = interp.stack().last().and_then(|f| f.file);
            h.me = ok;
            interp.reset_iterations();
            let s0 = Instant::now();
            interp.step(&mut h);
            if let Some(b) = &mut h.bench {
                let d = us(s0);
                b.steps_us += d;
                tick_us += d;
                b.steps += 1;
            }
        }
        motives_before(i16::MAX, &sim, &mut h.world, &mut motives_done);
        if have_passes {
            h.counts.note(if sim.stop_asked(&h.world) == (pass.stopped_at != 0) {
                "passes: stopped (or not) as the log's"
            } else {
                "passes: DIFFERENT stop"
            });
        }
        sim.end_pass(&mut h.world, if have_passes { pass.stopped_at as i16 } else { 0 });
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

    let world = &h.world;
    let counts = &mut h.counts;
    if has("REPLAY_SHOW_FILES") {
        let mut f: BTreeMap<String, i32> = BTreeMap::new();
        for s in &log.steps {
            let Some(o) = ids.get(&s.sim).and_then(|&id| world.find_object(id)) else { continue };
            if !o.person {
                continue;
            }
            if let Some((_, file)) = finder.find(log, s)
                && tree_kind(s.tree) == TreeKind::Own
            {
                *f.entry(lib.file(file).path.clone()).or_default() += 1;
            }
        }
        for (p, n) in &f {
            println!("a lot Sim's own trees in {p}: {n} steps");
        }
    }
    let bench_on = h.bench.is_some();
    if !bench_on {
        counts.add("steps: longest run with the native stack as the log's", best_streak);
    }

    // Autonomy's evaluations: a person's many guard checks in one tick (each at node 0 in the
    // bottom frame of its checks), against the native candidates (autonomy_candidates), the
    // persons' interactions and a version-11 table's first tree set aside.
    if !bench_on {
        let mut starts: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new(); // (tick, stack) -> node-0 starts
        for (i, s) in log.steps.iter().enumerate() {
            if s.node == 0 {
                starts.entry((s.tick, s.sim)).or_default().push(i);
            }
        }
        for (key, v) in &starts {
            if v.len() < 40 {
                continue;
            }
            let mut per_frame: BTreeMap<u32, usize> = BTreeMap::new(); // the checks' bottom frame: the one with the most starts
            for &i in v {
                *per_frame.entry(log.steps[i].frame).or_default() += 1;
            }
            let mut bottom = (0u32, 0usize);
            for (&f, &n) in &per_frame {
                if n > bottom.1 || bottom.1 == 0 {
                    bottom = (f, n);
                }
            }
            let bottom = bottom.0;
            let Some(me) = ids.get(&key.1).and_then(|&id| world.find(id)).filter(|&k| world.get(k).person) else { continue };
            // An evaluation goes over the lot: many objects (a pie menu's checks are of one).
            let over: BTreeSet<i16> = v.iter().map(|&i| &log.steps[i]).filter(|s| s.frame == bottom).map(|s| s.stack_object).collect();
            if over.len() < 10 || per_frame[&bottom] < 20 {
                continue;
            }
            let mut logged: Vec<(i16, u16)> = Vec::new();
            for &i in v {
                let s = &log.steps[i];
                let Some(so) = world.find(s.stack_object) else { continue };
                if s.frame != bottom || so == me {
                    continue;
                }
                let o = world.get(so);
                let tf = if o.person { o.file.and_then(|f| lib.file(f).semi) } else { o.file };
                let prefix = tf.is_some_and(|f| lib.file(f).prefix_of.values().any(|&p| p == s.tree));
                if !prefix {
                    logged.push((s.stack_object, s.tree));
                }
            }
            // The objects on the lot then (the log's list at that tick: the replay does not
            // remove objects natively yet, as the paper girl leaves).
            let mut present: BTreeSet<i16> = BTreeSet::new();
            for l in &log.lists {
                if l.tick <= key.0 {
                    present = l.ids.iter().copied().collect();
                }
            }
            let native: Vec<(i16, u16)> = autonomy_candidates(world, me)
                .iter()
                .filter(|c| present.is_empty() || present.contains(&c.id))
                .map(|c| (c.id, c.interaction(lib).guard))
                .collect();
            counts.note(if logged == native {
                "autonomy: an evaluation's candidates the same natively (objects and persons)"
            } else {
                "autonomy: an evaluation's candidates DIFFERENT"
            });
            counts.add("autonomy: candidates checked in evaluations", logged.len());
            if logged != native && has("REPLAY_SHOW_AUTONOMY") {
                let lo: BTreeSet<i16> = logged.iter().map(|x| x.0).collect();
                let na: BTreeSet<i16> = native.iter().map(|x| x.0).collect();
                let mut l = "objects only native:".to_string();
                for i in na.difference(&lo) {
                    let o = world.find_object(*i);
                    l += &format!(
                        " {i}({}, stack {})",
                        o.and_then(|o| o.file).map_or("?".to_string(), |f| file_name(lib, f)),
                        o.map_or(0, |o| o.stack.len())
                    );
                }
                println!("{l}");
                let mut l = "only logged:".to_string();
                for i in lo.difference(&na) {
                    l += &format!(" {i}");
                }
                println!("{l}");
                let mut k = 0;
                while k < logged.len() && k < native.len() && logged[k] == native[k] {
                    k += 1;
                }
                println!(
                    "evaluation at tick {} by {}: {} logged, {} native, first difference at {k}: log {}/{}, native {}/{}",
                    key.0,
                    world.get(me).id(),
                    logged.len(),
                    native.len(),
                    logged.get(k).map_or(-1, |x| x.0 as i32),
                    logged.get(k).map_or(-1, |x| x.1 as i32),
                    native.get(k).map_or(-1, |x| x.0 as i32),
                    native.get(k).map_or(-1, |x| x.1 as i32)
                );
            }
        }
    }

    // Autonomy's scores ('A', 'B' records): each recomputed natively (interaction_score, the
    // curves from STR# 501) from the motives and adverts it read; the person is the one whose
    // evaluation it was (the Sim of that tick's evaluation: Sim 12's or Sim 237's motives).
    if !bench_on && !log.scores.is_empty() {
        let curves = score_curves(lib);
        counts.add("autonomy: score curves from the global file", curves.len());
        for br in &log.bases {
            let Some(sr) = log.scores.iter().find(|x| x.tick == br.tick) else { continue };
            let Some(any) = world.objects().find(|o| o.person && o.definition.is_none()) else { continue };
            let got = interaction_score(&curves, None, any, &sr.motives);
            counts.note(if got == br.score { "autonomy: a base score the same natively (no advert)" } else { "autonomy: a base score DIFFERENT" });
        }
        for sr in &log.scores {
            // The person: the Sim whose motives these are (person data for the personality).
            let mut person = None;
            for o in world.objects() {
                if o.person && o.definition.is_none() && o.motives.len() >= 16 {
                    let near = [5, 7, 9].iter().all(|&i| (o.motives[i] - sr.motives[i]).abs() <= 60.0);
                    if near && person.is_none_or(|p: &housefire::world::Object| (o.motives[5] - sr.motives[5]).abs() < (p.motives[5] - sr.motives[5]).abs()) {
                        person = Some(o);
                    }
                }
            }
            let Some(person) = person else {
                counts.note("autonomy: a score's person not found");
                continue;
            };
            let ads: Vec<Advert> = (0..16).map(|m| Advert { min: sr.adverts[m][1], delta: sr.adverts[m][2], personality: sr.adverts[m][0] as u16 }).collect();
            let got = interaction_score(&curves, Some(&ads), person, &sr.motives);
            counts.note(if got == sr.score {
                "autonomy: a score the same natively (bit for bit)"
            } else if (got - sr.score).abs() < 1e-5 {
                "autonomy: a score the same natively but for the last bits"
            } else {
                "autonomy: a score DIFFERENT"
            });
        }
    }

    // Routing: each Sim's logged step from one tile to the next ('P'), against the lot's walls
    // and the doors' openings (Walls). Measured on the save's lot.
    if !bench_on && !log.places.is_empty() {
        routing(log, lib, world, counts, &ids);
    }

    // The motives' own update (update_motives), on each person's logged motives ('N' records):
    // the native update of one record against the next. A pair it does not reproduce is a change
    // the scripts made, or a tick the original did not update it.
    if has("REPLAY_SHOW_TUNING")
        && let Some(gf) = lib.global_file()
    {
        for (id, t) in &gf.tuning {
            for (name, v) in t {
                println!("tuning {id}: {name} = {}", g(*v as f64, 6));
            }
        }
    }
    if !bench_on && !log.motives.is_empty() {
        let (rates, ok) = MotiveRates::read(lib);
        if !ok {
            counts.note("motives: a rate missing from the tuning tables");
        }
        let mut by_person: BTreeMap<i16, Vec<usize>> = BTreeMap::new();
        for (k, m) in log.motives.iter().enumerate() {
            by_person.entry(m.id).or_default().push(k);
        }
        for (id, recs) in &by_person {
            let Some(o) = world.find_object(*id) else { continue };
            if !o.person || o.person_data.len() < 7 || o.motives_b.len() < 16 {
                continue;
            }
            let mut last: [f32; 16] = o.motives_b[..16].try_into().unwrap();
            for k in 1..recs.len() {
                let (prev, cur) = (&log.motives[recs[k - 1]], &log.motives[recs[k]]);
                let mut now = prev.motives;
                let mut copy = last;
                update_motives(&rates, &mut now, &mut copy, o.person_data[3] as i32, o.person_data[6] as i32, 12);
                let same = (0..16).all(|i| i == 3 || i == 13 || now[i] == cur.motives[i]);
                if !same && has("REPLAY_SHOW_MOTIVES") {
                    let near = (0..16).filter(|&i| i != 3 && i != 13 && (now[i] - cur.motives[i]).abs() < 1e-3).count();
                    if near == 14 {
                        for (i, (&a, &b)) in now.iter().zip(&cur.motives).enumerate() {
                            if i != 3 && i != 13 && a != b {
                                println!(
                                    "motive {i} of {id} at tick {}: native {}, log {} (before {})",
                                    cur.tick,
                                    g(a as f64, 9),
                                    g(b as f64, 9),
                                    g(prev.motives[i] as f64, 9)
                                );
                            }
                        }
                    }
                }
                counts.note(if same {
                    "motives: an update the same natively (bit for bit, all but 3 and 13)"
                } else {
                    "motives: a change not the update's (a script's, or no update)"
                });
                // An update step (energy moves only by the update): the copy is the motives after it.
                let update_step = now[motive::ENERGY] == cur.motives[motive::ENERGY] && cur.motives[motive::ENERGY] != prev.motives[motive::ENERGY];
                if update_step {
                    last = cur.motives;
                }
                if !same && update_step {
                    counts.note("motives: an update step with a script's change besides");
                }
            }
        }
    }
    let bench = h.bench.take();
    Some((std::mem::take(&mut h.counts), bench))
}

/// The routing checks: the Sims' steps, the original's plans, its routing steps' slots, and the
/// walks against the native route.
fn routing(log: &Log, lib: &Library, world: &World, counts: &mut Counts, ids: &BTreeMap<u32, i16>) {
    let walls = &world.walls;
    let show = has("REPLAY_SHOW_ROUTING");
    counts.add("routing: the lot's wall edges (each seen from both tiles)", walls.wall_edges());
    counts.add("routing: the doors' openings", walls.openings());
    counts.add("routing: the rooms (the regions the walls close)", walls.rooms());
    if show {
        for d in walls.doors() {
            println!("door {},{} +{},{}: room {} to {}", d.x, d.y, d.dx, d.dy, d.from, d.to);
        }
    }
    let mut tile: BTreeMap<i16, Tile> = BTreeMap::new();
    for p in &log.places {
        let Some(o) = world.find_object(p.id) else { continue };
        if !o.person || p.x < 0 || p.y < 0 {
            continue;
        }
        let (x, y) = (p.x >> 4, p.y >> 4);
        if let Some(&(tx, ty)) = tile.get(&p.id)
            && (tx, ty) != (x, y)
        {
            let (dx, dy) = (x - tx, y - ty);
            if dx.abs() + dy.abs() != 1 {
                counts.note("routing: a Sim's move not to a neighbouring tile (not a step)");
                if show {
                    println!("Sim {} moved {tx},{ty} -> {x},{y} (tick {})", p.id, p.tick);
                }
            } else {
                let ok = walls.can_step(tx, ty, dx, dy);
                counts.note(if ok { "routing: a Sim's step between tiles open natively" } else { "routing: a Sim's step through a WALL natively" });
                if ok && walls.wall_between(tx, ty, dx, dy) {
                    counts.note("routing: a Sim's step through a door's opening");
                }
                if !ok && show {
                    println!("Sim {} through a wall: {tx},{ty} -> {x},{y} (tick {})", p.id, p.tick);
                }
            }
        }
        tile.insert(p.id, (x, y));
    }
    // The original's own planned routes ('G': waypoints in sixteenths): every tile change along
    // their lines open natively (a corner crossed: either way round), and the objects whose
    // tiles they cross.
    let mut crossed: BTreeMap<String, usize> = BTreeMap::new();
    for pl in &log.plans {
        if pl.points.len() < 2 {
            continue;
        }
        let mut open = true;
        let mut i = 1;
        while i < pl.points.len() && open {
            let (x0, y0) = pl.points[i - 1];
            let (x1, y1) = pl.points[i];
            let n = (x1 - x0).abs().max((y1 - y0).abs());
            let (mut tx, mut ty) = (x0 >> 4, y0 >> 4);
            let mut k = 1;
            while k <= n && open {
                let (nx, ny) = ((x0 + (x1 - x0) * k / n) >> 4, (y0 + (y1 - y0) * k / n) >> 4);
                k += 1;
                if nx == tx && ny == ty {
                    continue;
                }
                let (dx, dy) = (nx - tx, ny - ty);
                open = if dx.abs() + dy.abs() == 1 {
                    walls.can_step(tx, ty, dx, dy)
                } else {
                    (walls.can_step(tx, ty, dx, 0) && walls.can_step(tx + dx, ty, 0, dy)) || (walls.can_step(tx, ty, 0, dy) && walls.can_step(tx, ty + dy, dx, 0))
                };
                if !open && show {
                    println!("plan at tick {} through a wall: {tx},{ty} -> {nx},{ny}", pl.tick);
                }
                tx = nx;
                ty = ny;
                for o in world.objects() {
                    if !o.person && o.level == 1 && o.on_lot() && (o.x16 >> 4) == nx && (o.y16 >> 4) == ny
                        && let Some(f) = o.file
                    {
                        *crossed.entry(file_name(lib, f)).or_default() += 1;
                    }
                }
            }
            i += 1;
        }
        counts.note(if open { "routing: an original's planned route open natively (walls, doors)" } else { "routing: an original's planned route through a WALL natively" });
        // The rooms along it (Walls::room): each change of room between two waypoints a single
        // step through a door's opening (the route broken at its doors), each piece between
        // within one room (its waypoints' tiles; a diagonal wall's tile, 0, either).
        let mut at_doors = true;
        let mut changes = 0;
        for i in 1..pl.points.len() {
            let (x0, y0) = (pl.points[i - 1].0 >> 4, pl.points[i - 1].1 >> 4);
            let (x1, y1) = (pl.points[i].0 >> 4, pl.points[i].1 >> 4);
            let (r0, r1) = (walls.room(x0, y0), walls.room(x1, y1));
            if r0 == r1 || r0 == 0 || r1 == 0 {
                continue;
            }
            changes += 1;
            let (dx, dy) = (x1 - x0, y1 - y0);
            let door = walls.doors().iter().any(|d| (d.x == x0 && d.y == y0 && d.dx == dx && d.dy == dy) || (d.x == x1 && d.y == y1 && d.dx == -dx && d.dy == -dy));
            if !door {
                at_doors = false;
                if show {
                    println!("plan at tick {} changes room {r0} -> {r1} not at a door: {x0},{y0} -> {x1},{y1}", pl.tick);
                }
            } else {
                counts.note("routing: a plan's change of room at a door, a waypoint either side");
            }
        }
        if changes != 0 {
            counts.note(if at_doors { "routing: a plan through rooms broken at each door" } else { "routing: a plan changing rooms NOT at a door" });
        } else {
            counts.note("routing: a plan within one room");
        }
    }
    // Every routing step (GotoRoutingSlot, opcode 45): the routing slot its operand names (the
    // number, where from), found natively in the stack object's SLOT or the global one.
    let raw45 = |s: &StepRec| -> Option<[u8; 12]> {
        let t = log.tree_of(s)?;
        let at = s.node as usize * 12;
        if at + 12 > t.raw.len() {
            return None;
        }
        let raw: [u8; 12] = t.raw[at..at + 12].try_into().unwrap();
        ((u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF) == 45).then_some(raw)
    };
    for s in &log.steps {
        let Some(raw) = raw45(s) else { continue };
        let number = u16::from_be_bytes([raw[4], raw[5]]);
        let scope = u16::from_be_bytes([raw[6], raw[7]]);
        let o = world.find_object(s.stack_object);
        if scope == 0 {
            counts.note("routing: a routing step's slot from a stack variable (not modelled)");
            continue;
        }
        let Some((o, file)) = o.and_then(|o| o.file.map(|f| (o, f))) else {
            counts.note("routing: a routing step's stack object not on the lot natively");
            continue;
        };
        let slot = lib.routing_slot(file, world.definition(o), scope, number);
        counts.note(match (slot.is_some(), scope == 1) {
            (true, true) => "routing: a routing step's slot found natively (the object's own)",
            (true, false) => "routing: a routing step's slot found natively (the global one)",
            _ => "routing: a routing step's slot NOT found natively",
        });
    }
    // A plan that ends at its routing step's object (the step's stack object, in the plan's
    // tick, at the plan's last point): the waypoint before it the tile in front of the object
    // (`front`, by its facing), one tile away; the Sim walks there, then turns to the object.
    for pl in &log.plans {
        if pl.points.len() < 3 {
            continue;
        }
        let end = pl.points[pl.points.len() - 1];
        let stand = pl.points[pl.points.len() - 2];
        let target = log.steps.iter().filter(|s| s.tick == pl.tick).find_map(|s| {
            let t = log.tree_of(s).filter(|t| t.tree == s.tree)?;
            let at = s.node as usize * 12;
            let op = u16::from_be_bytes([*t.raw.get(at)?, *t.raw.get(at + 1)?]) & 0x7FFF;
            let o = world.find_object(s.stack_object)?;
            (matches!(op, 27 | 45) && (o.x16, o.y16) == end).then_some(o)
        });
        let Some(o) = target else {
            counts.note("routing: a plan not ending at its routing step's object");
            continue;
        };
        let want = front(o.facing()).map(|(dx, dy)| (o.x16 + 16 * dx, o.y16 + 16 * dy));
        counts.note(if want == Some(stand) {
            "routing: a plan ending at its object, from the tile in front of it (its facing)"
        } else {
            "routing: a plan ending at its object, NOT from the tile in front of it"
        });
        if want != Some(stand) && show {
            println!(
                "plan at tick {} to object {} (facing {}): from {},{}, in front {:?}",
                pl.tick,
                o.id(),
                o.facing(),
                stand.0,
                stand.1,
                want
            );
        }
    }
    // A routing step's goal natively (GotoRoutingSlot, a literal or global slot): of the places
    // its slot offers (`slot_places`), the nearest the Sim (from the plan's start) whose tile
    // the walls let it reach and no other object stands on; against the plan the step made in
    // its tick (its start the Sim's place) by the step that walks: its end, or the tile before
    // the object where it ends at the object.
    let place_at = |id: i16, tick: u32| log.places.iter().rfind(|p| p.id == id && p.tick <= tick).map(|p| (p.x, p.y));
    for s in &log.steps {
        let Some(raw) = raw45(s) else { continue };
        // The step that walks (yields, result 1) made the plan; one that ends (a return to its
        // tree's true or false target) only shares the tick.
        if s.result != 1 {
            continue;
        }
        let (number, scope) = (u16::from_be_bytes([raw[4], raw[5]]), u16::from_be_bytes([raw[6], raw[7]]));
        let Some(sim) = ids.get(&s.sim).copied() else { continue };
        let Some(start) = place_at(sim, s.tick) else { continue };
        let Some(pl) = log.plans.iter().find(|p| p.tick == s.tick && p.points.first() == Some(&start) && p.points.len() >= 2) else { continue };
        let Some(o) = world.find_object(s.stack_object) else { continue };
        let Some(file) = o.file else { continue };
        let Some(slot) = lib.routing_slot(file, world.definition(o), scope, number) else { continue };
        let target = place_at(o.id(), s.tick).unwrap_or((o.x16, o.y16));
        let end = pl.points[pl.points.len() - 1];
        let want = if end == target { pl.points[pl.points.len() - 2] } else { end };
        let tile = |p: (i32, i32)| (p.0 >> 4, p.1 >> 4);
        let occupied = |p: (i32, i32)| {
            world.objects().any(|x| {
                !x.person && x.id() != o.id() && x.on_lot() && x.level == 1 && tile((x.x16, x.y16)) == tile(p)
            })
        };
        let mut places = slot_places(slot, target.0, target.1, o.facing());
        places.retain(|&p| !occupied(p) && !walls.route(tile(start), tile(p), None).is_empty());
        let d2 = |p: &(i32, i32)| (p.0 - start.0).pow(2) + (p.1 - start.1).pow(2);
        let got = places.iter().min_by_key(|p| d2(p)).copied();
        counts.note(if got == Some(want) {
            "routing: a routing step's goal natively the plan's (the nearest open place of its slot)"
        } else {
            "routing: a routing step's goal natively NOT the plan's"
        });
        if show {
            println!(
                "goal at tick {}: slot {number} ({scope}) flags {:x} of {} facing {}: native {got:?}, the plan's {want:?}; places {places:?}",
                s.tick,
                slot.flags,
                o.id(),
                o.facing()
            );
        }
    }
    // Each plan with the routing primitives its Sim ran in its tick (study: REPLAY_SHOW_PLANS):
    // opcode 27 or 45, the stack object and its place, the operand, the result.
    if has("REPLAY_SHOW_PLANS") {
        for pl in &log.plans {
            let (Some(a), Some(b)) = (pl.points.first(), pl.points.last()) else { continue };
            let who = world.objects().find(|o| o.person && (o.x16, o.y16) == *a).map_or(-1, |o| o.id() as i32);
            println!("plan tick {} from {},{} to {},{} ({} points), Sim at its start {who}", pl.tick, a.0, a.1, b.0, b.1, pl.points.len());
            for s in log.steps.iter().filter(|s| s.tick == pl.tick) {
                let Some(t) = log.tree_of(s) else { continue };
                let at = s.node as usize * 12;
                if at + 12 > t.raw.len() || t.tree != s.tree {
                    continue;
                }
                let raw = &t.raw[at..at + 12];
                let op = u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF;
                if op != 27 && op != 45 {
                    continue;
                }
                let o = world.find_object(s.stack_object);
                let mut l = format!(
                    "   op {op} by {} in {}:{} on {} ({} at {},{}) operand",
                    ids.get(&s.sim).map_or(-1, |&i| i as i32),
                    s.tree,
                    s.node,
                    s.stack_object,
                    o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                    o.map_or(0, |o| o.x16),
                    o.map_or(0, |o| o.y16)
                );
                for x in &raw[4..12] {
                    l += &format!(" {x:02x}");
                }
                l += &format!(" -> {} result {}; params", s.after, s.result);
                for p in &s.params {
                    l += &format!(" {p}");
                }
                println!("{l}");
            }
        }
    }
    // The routing steps (GotoRoutingSlot, opcode 45): the slot they name, of the stack object's
    // SLOT, against the next plan's end (study: REPLAY_SHOW_SLOTS).
    if has("REPLAY_SHOW_SLOTS") {
        let mut gi = 0;
        for s in &log.steps {
            let Some(raw) = raw45(s) else { continue };
            while gi < log.plans.len() && log.plans[gi].tick < s.tick {
                gi += 1;
            }
            if gi >= log.plans.len() || log.plans[gi].tick != s.tick {
                continue; // a plan made in this step's tick
            }
            let o = world.find_object(s.stack_object);
            let mut l = format!(
                "route step tick {}: object {} ({} at {},{}), operand",
                s.tick,
                s.stack_object,
                o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                o.map_or(0, |o| o.x16),
                o.map_or(0, |o| o.y16)
            );
            for b in &raw[4..12] {
                l += &format!(" {b:02x}");
            }
            l += ", params";
            for p in &s.params {
                l += &format!(" {p}");
            }
            let pl = &log.plans[gi];
            if let (Some(a), Some(b)) = (pl.points.first(), pl.points.last()) {
                l += &format!("; next plan tick {} from {},{} to {},{}", pl.tick, a.0, a.1, b.0, b.1);
            }
            l += &format!(" result {}; lookups", s.result);
            for v in &s.lookups {
                l += &format!(" ({},{})={}", v.scope, v.data, v.value);
            }
            println!("{l}");
            if let Some(f) = o.and_then(|o| o.file) {
                for v in lib.file(f).slots.values() {
                    for (k, sl) in v.iter().enumerate() {
                        if sl.kind == 3 {
                            println!(
                                "   slot {k}: flags {:x}, proximity {}-{} ({}), facing {}, offset {},{}",
                                sl.flags,
                                sl.min_proximity,
                                sl.max_proximity,
                                sl.optimal_proximity,
                                sl.facing,
                                g(sl.x as f64, 6),
                                g(sl.y as f64, 6)
                            );
                        }
                    }
                }
            }
        }
    }
    if show {
        for (name, n) in &crossed {
            println!("planned routes cross a tile of {name}: {n}");
        }
    }
    // Each walk (a Sim's tile changes, a pause of 40 ticks or more ending one) against the native
    // route between its ends (Walls::route, no object blocking yet).
    let mut seq: BTreeMap<i16, Vec<(u32, Tile)>> = BTreeMap::new();
    for p in &log.places {
        let Some(o) = world.find_object(p.id) else { continue };
        if !o.person || p.x < 0 || p.y < 0 {
            continue;
        }
        let t = (p.x >> 4, p.y >> 4);
        let v = seq.entry(p.id).or_default();
        if v.last().is_none_or(|l| l.1 != t) {
            v.push((p.tick, t));
        }
    }
    for (id, v) in &seq {
        let mut start = 0;
        for i in 1..=v.len() {
            if i < v.len() && v[i].0 - v[i - 1].0 < 40 {
                continue;
            }
            // A walk: v[start - 1 or start] .. v[i - 1] (from the tile it stood on).
            let from = if start > 0 { start - 1 } else { start };
            if i - 1 > from {
                let logged: Vec<Tile> = v[from..i].iter().map(|x| x.1).collect();
                let jump = logged.windows(2).any(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs() != 1);
                if !jump {
                    let r = walls.route(logged[0], logged[logged.len() - 1], None);
                    let same_length = r.len() == logged.len();
                    counts.note(if same_length { "routing: a walk as long as the native route (walls only)" } else { "routing: a walk NOT as long as the native route (walls only)" });
                    if r == logged {
                        counts.note("routing: a walk's tiles the native route's");
                    }
                    if !same_length && show {
                        let mut l = String::new();
                        for t in &r {
                            l += &format!(" {},{}", t.0, t.1);
                        }
                        println!("{l}");
                    }
                    if show {
                        println!(
                            "walk of {id} from tick {}: {} steps, native {} ({},{} -> {},{})",
                            v[from].0,
                            logged.len() - 1,
                            r.len().saturating_sub(1),
                            logged[0].0,
                            logged[0].1,
                            logged[logged.len() - 1].0,
                            logged[logged.len() - 1].1
                        );
                    }
                }
            }
            start = i;
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: replay LOG ISO [--card CARD] [--lot N] [--bench]");
        return ExitCode::from(2);
    }
    let Some(log) = read_log(&args[1]) else {
        println!("cannot read {}", args[1]);
        return ExitCode::from(1);
    };
    let Ok(d) = Disc::open(&args[2]) else {
        println!("cannot read {}", args[2]);
        return ExitCode::from(1);
    };
    let lib = Library::bustin_out(&d);
    let mut bench = false;
    let mut card_path: Option<String> = None;
    let mut lot_index: i64 = -1;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--bench" => bench = true,
            "--card" if i + 1 < args.len() => {
                i += 1;
                card_path = Some(args[i].clone());
            }
            "--lot" if i + 1 < args.len() => {
                i += 1;
                lot_index = atoi(&args[i]);
            }
            a => {
                println!("unknown option {a}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let ngh = match &card_path {
        Some(p) => {
            let mut card = std::fs::read(p).unwrap_or_default();
            card.truncate(1 << 22);
            let ngh = memory_card_file(&card, "G4ME69").unwrap_or_default();
            if ngh.is_empty() {
                println!("no Bustin' Out save on {p}");
                return ExitCode::from(1);
            }
            if lot_index < 0 {
                lot_index = 0;
            }
            ngh
        }
        None => d.read("Runtime/default.ngh").unwrap_or_default(),
    };
    let lots = read_lots(&ngh).expect("the lots");
    let neighbours = read_neighbours(&ngh).expect("the neighbours");
    if lots.is_empty() {
        println!("no lots");
        return ExitCode::from(1);
    }
    let the_lot = if lot_index < 0 || lot_index as usize >= lots.len() { lots.last().unwrap() } else { &lots[lot_index as usize] };
    let card_lot = card_path.is_some();
    if !bench {
        let Some((counts, _)) = run(&log, &lib, the_lot, &neighbours, None, card_lot) else { return ExitCode::from(1) };
        for (k, n) in &counts.map {
            println!("  {k:<66} {n}");
        }
        return ExitCode::SUCCESS;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    let mut runs: Vec<Bench> = Vec::new();
    for _ in 0..5 {
        let Some((_, Some(b))) = run(&log, &lib, the_lot, &neighbours, Some(Bench::default()), card_lot) else { return ExitCode::from(1) };
        runs.push(b);
    }
    let median = |f: &dyn Fn(&Bench) -> f64| {
        let mut v: Vec<f64> = runs.iter().map(f).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let pct = |v: &[f64], q: f64| {
        let mut v = v.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[(q * (v.len() - 1) as f64) as usize]
    };
    let ticks = runs[0].ticks.len();
    println!(
        "bench: {ticks} ticks, {} native steps ({:.1} a tick), 5 runs, medians",
        runs[0].steps,
        runs[0].steps as f64 / ticks as f64
    );
    println!(
        "  per tick:  mean {:.2} us, median {:.2} us, 99th percentile {:.2} us, worst {:.2} us",
        median(&|b| b.ticks.iter().sum::<f64>() / b.ticks.len() as f64),
        median(&|b| pct(&b.ticks, 0.5)),
        median(&|b| pct(&b.ticks, 0.99)),
        median(&|b| b.ticks[b.worst_tick])
    );
    println!(
        "  the worst tick: tick {}; the worst from tick 10 on (past the load): {:.2} us",
        runs[0].worst_tick + 1,
        median(&|b| b.ticks[9.min(b.ticks.len() - 1)..].iter().copied().fold(f64::MIN, f64::max))
    );
    println!(
        "  per part, us a tick: scripts (steps) {:.2}, of which expression {:.2}, SetToNext {:.2}, Idle {:.2}; the pass {:.2}",
        median(&|b| (b.steps_us - b.sync_us) / b.ticks.len() as f64),
        median(&|b| b.expression_us / b.ticks.len() as f64),
        median(&|b| b.set_to_next_us / b.ticks.len() as f64),
        median(&|b| b.idle_us / b.ticks.len() as f64),
        median(&|b| b.pass_us / b.ticks.len() as f64)
    );
    println!("  not native yet (not in these figures): routing, autonomy, animation, the other primitives");
    ExitCode::SUCCESS
}
