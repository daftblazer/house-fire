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
use housefire::interp::{Error, Frame, Host, Interpreter, Mode, PrimResult, Step, Trees};
use housefire::library::{Advert, Library};
use housefire::motives::{MotiveRates, motive, update_motives};
use housefire::primitives::{
    call_named_tree, create_object, drop_onto, grab, make_of_guid, place_made, remove_object, snap, dialog, idle, idle_for_input, present, push_action, random, run_functional_tree, function_entry, set_motive_delta, set_to_next, test_object_type,
};
use housefire::routing::{BLOCKS, FrameClock, OFF_PAVED_WEIGHT, Tile, turn_frames, WALK_FRAME, weighed_path, WALK_TOP_ADULT, WALK_TOP_CHILD, walk_top, slot_area, slot_goal_score, slot_pick, run_on_point, walk_frames, routing_return_tick, along_line, walk_rates, walk_step, footprint, front, obstacle, overlaps, segment_enters, slot_places, ungrown};
use housefire::planner::{choose_goal, coarse_way, point_after_goal, refine, Growth, MapRect};
use housefire::library::Slot;
use housefire::save::{LotChunks, Neighbour, memory_card_file, read_lot_arrays, read_lot_globals, read_lots, read_neighbours};
use housefire::simulator::{SimHost, Simulator};
use housefire::tree_log::{Log, PassRec, PlanRec, StepRec, read_log, same_shape};
use housefire::world::{DATA_OBJECT_ID, PORTAL_FUNCTION, SIT_FUNCTION, Object, ObjectKey, ScriptVariables, World};
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
/// A person's own part among the planner's tested objects ('J'): its offset from the listed
/// object the observer files it under.
const PERSON_PART_OFFSET: i32 = -104;

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
    /// The last evaluation's choice: the object, its interaction's action and file.
    chosen: Option<(i16, TreeId, Option<FileId>)>,
    /// The next logged score ('A').
    score_at: usize,
    /// While an evaluation runs: its checks' results against the log's (same, different).
    on_check: Option<(usize, usize)>,
    counts: Counts,
    shown: Shown,
    /// The simulator's ticks run (for a study).
    now: u32,
    /// The routing runs' native returns (a pre-pass of `routing`).
    routing_returns: RoutingReturns,
    /// Objects made before their maker's step (its CreateObjectInstance runs the new object's
    /// init inside it, and the log has the init's steps first): the maker's step only places them.
    pre_made: BTreeSet<i16>,
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
            if env("REPLAY_SHOW_SCOPE").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope", 40) {
                let so = s.stack_object;
                println!(
                    "scope {} datum {} (not modelled): log {} (flags {}, object {:08x} ptr {:08x}, offset {:#x}); me {}, stack object {so}, tree {}:{}",
                    v.scope,
                    v.data,
                    v.value,
                    v.flags,
                    v.object,
                    v.ptr,
                    v.ptr.wrapping_sub(v.object) as i32,
                    vars.world.get(vars.me).id(),
                    s.tree,
                    s.node
                );
                if let Some(o) = vars.world.find_object(so) {
                    let pd = o.person_data.get(v.data as usize).copied();
                    let d = o.data.get(v.data as usize).copied();
                    let room = vars.world.walls.room(o.x16 >> 4, o.y16 >> 4);
                    println!("   so {so}: data[{}] {d:?} person_data {pd:?} tile ({},{},{}) room {room} file {:?}", v.data, o.x16 >> 4, o.y16 >> 4, o.level, o.file.map(|f| vars.world.library().file(f).path.clone()));
                }
            }
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
            if env("REPLAY_SHOW_SCOPE_SAME").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope same", 20) {
                println!("scope {} datum {}: the same, {}", v.scope, v.data, v.value);
            }
            note!(counts, "variables: scope {}: the same natively", v.scope);
            continue;
        }
        if env("REPLAY_SHOW_SCOPE").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope", env("REPLAY_SHOW_N").map_or(10, |n| atoi(&n) as i32)) {
            println!(
                "scope {} datum {}: native {}, log {}; the step's stack object {} -> {}, tree {}:{} (me {} {:?}, tick {})",
                v.scope,
                v.data,
                got.value,
                v.value,
                s.stack_object,
                s.stack_object_after,
                s.tree,
                s.node,
                vars.world.get(me).id(),
                vars.world.get(me).file.map(|f| vars.world.library().file(f).path.rsplit('/').next().unwrap_or("").to_string()),
                s.tick
            );
        }
        // a local this frame has not written: zero here, the original's left-over memory there
        // (the listed deliberate difference, plan.md), counted apart
        if v.scope == 25 && (0..64).contains(&v.data) && (vars.frame.unset >> v.data & 1) != 0 {
            note!(counts, "variables: scope 25: an unwritten local, the original's left over (zero here, deliberately)");
            // study REPLAY_ZERO_LOCALS: keep the engine's zero, to see what the scripts do with it
            vars.set(v.scope as u8, v.data, if has("REPLAY_ZERO_LOCALS") { 0 } else { v.value });
            continue;
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

    fn ad_range_mut(&mut self) -> Option<&mut Vec<i16>> {
        Some(&mut self.world.ad_range)
    }

    fn default_params(&mut self) -> Option<Vec<i16>> {
        Some(self.world.get(self.me).temps.to_vec())
    }

    /// A check tree the engine runs, stepped natively through the log's steps of it.
    fn run_check(&mut self, t: TreeId, file: Option<FileId>, stack_object: i16, defaults: &[i16]) -> Option<bool> {
        let log = self.log;
        let lib = self.lib;
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
            if trace && std::env::var("REPLAY_TRACE_CHECK").ok().and_then(|v| v.parse::<i16>().ok()).is_none_or(|w| w == self.me_id()) {
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
        if n.opcode == 27 || n.opcode == 45 {
            // GotoRelativePosition, GotoRoutingSlot: natively a yield until its run's return tick
            // (its walker's walk, turn and frames: `routing_return_tick`), where the run is
            // modelled; the log's result taken
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
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0))?)))
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
                    && let Some(chair) = self.world.seat_at_routing_slot(n, s.stack_object, &pl.points)
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
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0))?)))
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
                self.counts.note("primitives: GotoRoutingSlot/GotoRelativePosition: its run not modelled (the log's taken)");
                if has("REPLAY_SHOW_NO_WAY") && n.opcode == 45 {
                    let w = &self.world;
                    let places: Vec<(i32, i32)> = w
                        .find_object(s.stack_object)
                        .and_then(|o| Some((o, lib.routing_slot(o.file?, w.definition(o), n.operand16(1), n.operand16(0))?)))
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
            self.sync(interp, s);
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
            if let Ok(w) = std::env::var("REPLAY_WATCH_ID")
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
            let got = push_action(interp, self, n);
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
        if n.opcode == 0 {
            // Idle
            let was = interp.sleep().woke();
            let t0 = Instant::now();
            // (a stack saved at an Idle wakes there: the simulator's `stack_of` marks it)
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

impl ReplayHost<'_, '_> {
    /// FindBestObjectForFunction (opcode 14), as the original's fn_8020CC8C (docs/primitives.md):
    /// the function's (rating datum, function-table entry); each object whose function table
    /// (OBJf) has an action for the entry and whose rating (its data, a: -1 for 50) is not 0,
    /// its test tree (if any) run as a check on it; the score the rating less three times the
    /// distance, at least 1, the best (the first of equals) the stack object, true; none, false.
    fn find_best_object(&mut self, interp: &mut Interpreter, s: &StepRec, n: &Node) -> PrimResult {
        let lib = self.lib;
        let me = self.me;
        let Some((a, b)) = function_entry(n.operand[0]) else { return self.logged(s, n) };
        let (mx, my) = (self.world.get(me).x16 as f32 / 16.0, self.world.get(me).y16 as f32 / 16.0);
        let defaults = interp.defaults().to_vec();
        let candidates: Vec<(i16, Option<FileId>, u16, f32, f32, f32)> = self
            .world
            .objects()
            .filter(|o| o.id() != self.world.get(me).id())
            // in use (its data 25, the int16 at +88, above 0) left out
            .filter(|o| o.data.get(25).is_none_or(|&u| u <= 0))
            .filter_map(|o| {
                let f = o.file?;
                let &(test, action) = lib.file(f).functions.values().next()?.get(b)?;
                if action == 0 {
                    return None;
                }
                let rating = if a < 0 { 50 } else { *o.data.get(a as usize)? };
                (rating != 0).then_some((o.id(), Some(f), test, f32::from(rating), o.x16 as f32 / 16.0, o.y16 as f32 / 16.0))
            })
            .collect();
        let mut best: Option<(i16, f32)> = None;
        for (id, file, test, rating, x, y) in candidates {
            if test != 0 {
                let ok = lib.tree(file, test).and_then(|t| self.run_check(t, file, id, &defaults));
                if ok != Some(true) {
                    continue;
                }
            }
            let dist = ((x - mx) * (x - mx) + (y - my) * (y - my)).sqrt();
            let score = (rating - 3.0 * dist).max(1.0);
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((id, score));
            }
        }
        let want = self.logged(s, n);
        let got = match best {
            Some((id, _)) => {
                interp.top().stack_object = id;
                PrimResult::True
            }
            None => PrimResult::False,
        };
        self.counts.note(if got == want && best.map(|b| b.0) == Some(s.stack_object_after).or(if got == PrimResult::False { Some(s.stack_object_after) } else { None }) {
            "primitives: FindBestObjectForFunction natively, the original's object"
        } else {
            "primitives: FindBestObjectForFunction natively, DIFFERENT"
        });
        if has("REPLAY_SHOW_FIND_BEST") {
            println!("find best natively: tick {} function {} found {best:?}, log {} {:?}", s.tick, n.operand[0], s.stack_object_after, want);
        }
        got
    }

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
        if has("REPLAY_SHOW_CAND_FIELDS") && self.shown.next("cand fields", 1) {
            let pd = &self.world.get(me).person_data;
            println!("evaluator {me_id} pd32 {:?} pd36 {:?}", pd.get(32), pd.get(36));
            for c in &cands {
                let e = c.interaction(lib);
                println!("  cand {} guard {} threshold {} flags {:#x}", c.id, e.guard, e.autonomy_threshold, e.flags);
            }
        }
        if has("REPLAY_SHOW_GUARD_RUNS") {
            let pd = &self.world.get(me).person_data;
            let guards: BTreeSet<(i16, u16)> = cands.iter().map(|c| (c.id, c.interaction(lib).guard)).collect();
            let ran = self.nested.iter().any(|&i| guards.contains(&(log.steps[i].stack_object, log.steps[i].tree)));
            note!(self.counts, "study guards run by the original: Sim {me_id} pd32 {:?} pd36 {:?} pd29 {:?} ran {ran}", pd.get(32), pd.get(36), pd.get(29));
        }
        if has("REPLAY_SHOW_CAND_IDS") && self.shown.next("cand ids", 1) {
            let ids: BTreeSet<i16> = cands.iter().map(|c| c.id).collect();
            println!("candidate ids at tick {} by {me_id}: {ids:?}", s.tick);
        }
        if has("REPLAY_SHOW_EVAL_ORDER") && self.shown.next("eval order", 2) {
            let mut l = format!("evaluation order at tick {} by {me_id} ({} nested steps):", s.tick, self.nested.len());
            let mut prev = None;
            for &i in &self.nested {
                let t = &log.steps[i];
                let key = (t.tree, t.stack_object);
                if t.node == 0 && prev != Some((key, t.frame)) {
                    l += &format!(" {}/{}@{}", t.tree, t.stack_object, t.frame);
                }
                prev = Some((key, t.frame));
            }
            println!("{l}");
            for id in [3i16, 18, 33, 150, 135] {
                if let Some(o) = self.world.find_object(id) {
                    let d = self.world.definition(o);
                    println!("   {id}: {} at ({},{},{}) leader {} data 0..12 {:?} def 6 {:?} 7 {:?} 8 {:?} 11 {:?}", o.file.map_or("?".into(), |f| file_name(lib, f)), o.x16 >> 4, o.y16 >> 4, o.level, o.leader, &o.data[..12], d.map(|d| d.field(6)), d.map(|d| d.field(7)), d.map(|d| d.field(8)), d.map(|d| d.field(11)));
                }
            }
            let visited: BTreeSet<i16> = self.nested.iter().map(|&i| log.steps[i].stack_object).collect();
            let mut seen = BTreeSet::new();
            for c in &cands {
                if seen.insert(c.id) && let Some(o) = self.world.find_object(c.id) {
                    let tab = lib.table_of(c.object_file, c.person, c.def.map(|d| lib.def(d)));
                    println!("   cand {} visited {}: first {} file {}", c.id, visited.contains(&c.id), tab.first, o.file.map_or("?".into(), |f| file_name(lib, f)));
                }
            }
            if let Some(m) = self.world.find_object(me_id) {
                println!("   me {me_id} person data {:?} global 10 {:?}", m.person_data, self.world.globals.get(10));
                println!("   me {me_id} at ({},{},{}) room {:?}", m.x16 >> 4, m.y16 >> 4, m.level, self.world.walls.room(m.x16 >> 4, m.y16 >> 4));
            }
            let mut l = "   native candidates:".to_string();
            for c in cands.iter().take(40) {
                l += &format!(" {}/{}", c.id, c.interaction(lib).guard);
            }
            println!("{l}");
        }
        if has("REPLAY_SHOW_EVAL_OPERAND") {
            println!("evaluation node at tick {} by {me_id}: tree {}:{} operand {:02x?} so {} params {:?}", s.tick, s.tree, s.node, n.operand, s.stack_object, s.params);
            for id in [me_id, 150] {
                if let Some(o) = self.world.find_object(id) {
                    println!("   {id}: {} at ({},{},{}) container {} person {} data 0..12 {:?} person data 0..20 {:?}", o.file.map_or("?".into(), |f| file_name(lib, f)), o.x16 >> 4, o.y16 >> 4, o.level, o.data[2], o.person, &o.data[..12], o.person_data.get(..20));
                }
            }
        }
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
        for c in checks.iter().filter(|c| c.candidate.is_none()) {
            note!(self.counts, "autonomy study: a first tree's check {:?}", c.passed);
            if has("REPLAY_SHOW_FIRST_TREES") { note!(self.counts, "study first tree {} {:?}", c.tree, c.passed); }
        }
        let mut native_choice: Option<i16> = None;
        {
            let curves = score_curves(lib);
            let person = self.world.get(me);
            let base = interaction_score(&curves, None, person, &person.motives);
            if has("REPLAY_SCORE_GATE") {
                // each candidate's score natively against the base, by whether the original ran
                // its guard (a step of the guard tree with the candidate as the stack object)
                let ran: BTreeSet<(i16, u16)> = self.nested.iter().map(|&i| (log.steps[i].stack_object, log.steps[i].tree)).collect();
                for c in &cands {
                    let e = c.interaction(lib);
                    let sc = interaction_score(&curves, Some(&e.adverts), person, &person.motives);
                    let checked = ran.contains(&(c.id, e.guard));
                    let rel = if sc > base { "above base" } else if sc == base { "at base" } else { "below base" };
                    note!(self.counts, "study score gate: {} score {rel}, guard run by the original {checked}", if self.world.globals.get(10) == Some(&1) { "home" } else { "club" });
                }
            }
            let mut values: Vec<(f32, usize)> = Vec::new();
            let (mut scores_same, mut scores_diff) = (0, 0);
            let mut values_scores: Vec<f32> = Vec::new();
            for c in &checks {
                let Some(ci) = c.candidate else { continue };
                if c.passed != Some(true) {
                    continue;
                }
                let cand = &cands[ci];
                // The log's score of this one (the scores come in the order of the passing
                // guards).
                // its advertisement as its guard left the ad range (scope 12): the deltas
                let mut adverts = cand.interaction(lib).adverts.to_vec();
                for (m, a) in adverts.iter_mut().enumerate() {
                    if let Some(&d) = c.ad_range.get(m) {
                        a.delta = d;
                    }
                }
                let native_score = interaction_score(&curves, Some(&adverts), person, &person.motives);
                values_scores.push(native_score);
                while self.score_at < log.scores.len() && log.scores[self.score_at].tick < s.tick {
                    self.score_at += 1;
                }
                if self.score_at < log.scores.len() && log.scores[self.score_at].tick == s.tick {
                    let tick_scores = log.scores.iter().filter(|x| x.tick == s.tick);
                    if native_score == log.scores[self.score_at].score {
                        scores_same += 1;
                    } else if tick_scores.clone().any(|x| x.score == native_score) {
                        // the log's order of the tick's scores is not the candidates' here (one
                        // more of its calls before): one of the tick's, bit for bit
                        self.counts.note("autonomy: a candidate's score the same natively as one of the tick's (out of the log's order)");
                    } else {
                        scores_diff += 1;
                        // with motive 14's advertisement as the original scored it (its logged
                        // entry read as personality, min, delta): made at run time, not native
                        {
                            let mut adv = cand.interaction(lib).adverts.to_vec();
                            adv.resize(16, Default::default());
                            let lg = log.scores[self.score_at].adverts[14];
                            adv[14] = housefire::library::Advert { min: lg[1], delta: lg[2], personality: lg[0] as u16 };
                            self.counts.note(if interaction_score(&curves, Some(&adv), person, &person.motives) == log.scores[self.score_at].score {
                                "autonomy: a differing score the log's with motive 14's advertisement as the original made it"
                            } else {
                                "autonomy: a differing score NOT the log's with motive 14's advertisement as the original made it"
                            });
                        }
                        if has("REPLAY_SHOW_SCORES") {
                            let a = &cand.interaction(lib).adverts;
                            let mut l = format!("score DIFFERENT tick {} by {} on {} (interaction {}): native {native_score:e}, log {:e}; adverts (motive: native min/delta/personality | log)", s.tick, person.id(), cand.id, cand.interaction(lib).string_index, log.scores[self.score_at].score);
                            for (m, lg) in log.scores[self.score_at].adverts.iter().enumerate() {
                                let n = a.get(m).copied().unwrap_or_default();
                                if (n.min, n.delta, n.personality as i16) != (lg[0], lg[1], lg[2]) {
                                    l += &format!(" {m}: {}/{}/{} | {}/{}/{}", n.min, n.delta, n.personality, lg[0], lg[1], lg[2]);
                                }
                            }
                            // the native score with the log's adverts: are they the original's input?
                            let logged: Vec<housefire::library::Advert> = log.scores[self.score_at].adverts.iter().map(|a| housefire::library::Advert { min: a[0], delta: a[1], personality: a[2] as u16 }).collect();
                            let with_logged = interaction_score(&curves, Some(&logged), person, &person.motives);
                            l += &format!("; with the log's adverts natively {with_logged:e} ({})", if with_logged == log.scores[self.score_at].score { "the log's" } else { "not the log's" });
                            // motive 14's advertisement that gives the log's score natively
                            let mut found = Vec::new();
                            let mut tryv = a.to_vec();
                            tryv.resize(16, Default::default());
                            for delta in -100i16..=100 {
                                for min in [0i16, tryv[14].min] {
                                    for pers in [0u16, tryv[14].personality] {
                                        tryv[14] = housefire::library::Advert { min, delta, personality: pers };
                                        if interaction_score(&curves, Some(&tryv), person, &person.motives) == log.scores[self.score_at].score {
                                            found.push((min, delta, pers));
                                        }
                                    }
                                }
                            }
                            l += &format!("; motive 14 that gives it: {found:?}; the person's motive 14 {}", person.motives.get(14).copied().unwrap_or(0.0));
                            // the log's motive-14 entry read as (personality, min, delta)
                            let lg = log.scores[self.score_at].adverts[14];
                            tryv[14] = housefire::library::Advert { min: lg[1], delta: lg[2], personality: lg[0] as u16 };
                            let turned = interaction_score(&curves, Some(&tryv), person, &person.motives);
                            l += &format!("; with its entry as (personality, min, delta): {}", if turned == log.scores[self.score_at].score { "the log's" } else { "not" });
                            println!("{l}");
                        }
                    }
                    self.score_at += 1;
                }
                // a person's place as logged at the tick (the walk is not native yet; the save's
                // place is where it stood before it walked): the chooser's and a person
                // candidate's, for the distance in the value
                let placed = |o: &housefire::world::Object| {
                    let mut o = o.clone();
                    if o.person
                        && let Some(p) = log.places.iter().rfind(|r| r.id == o.id() && r.tick <= s.tick)
                    {
                        (o.x16, o.y16) = (p.x, p.y);
                    }
                    o
                };
                let (chooser, object) = (placed(person), placed(self.world.get(cand.object)));
                values.push((candidate_value(lib, &curves, &chooser, &object, cand, &adverts, base), ci));
            }
            if has("REPLAY_SHOW_SCORES") {
                // the tick's logged scores that no native candidate's matches
                let natives: Vec<f32> = values_scores.clone();
                let logged: Vec<f32> = log.scores.iter().filter(|x| x.tick == s.tick).map(|x| x.score).collect();
                let order: Vec<String> = natives.iter().enumerate().map(|(i, v)| {
                    let at = logged.iter().position(|x| x == v).map_or("-".to_string(), |p| p.to_string());
                    let c = &cands[values[i].1];
                    format!("{}/{}:{at}", c.id, c.interaction(lib).string_index)
                }).collect();
                println!("score order tick {}: native candidate:logged position {}", s.tick, order.join(" "));
                for (i, x) in log.scores.iter().filter(|x| x.tick == s.tick).enumerate() {
                    if !natives.contains(&x.score) {
                        println!("logged score {i} of tick {} with no native candidate's: {:e} (motive 14's entry {:?})", s.tick, x.score, x.adverts[14]);
                    }
                }
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
                let rnd = log.randoms.iter().rfind(|x| x.tick == s.tick && x.site <= 1);
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
                        let c = &cands[list[at.unwrap()].candidate];
                        native_choice = Some(c.id);
                        // what GosubFoundAction (opcode 30) runs: the interaction's action, in
                        // the chosen object's file
                        self.chosen = lib.tree(c.object_file, c.interaction(lib).action).map(|t| (c.id, t, c.object_file));
                    }
                }
            }
            values.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let rank = values.iter().position(|v| cands[v.1].id == s.stack_object_after).map_or(0, |p| p + 1);
            if values.is_empty() {
                // nothing passed (the club's evaluations: no guard runs there): no choice, the
                // stack object as it was
                let kept = interp.top().stack_object;
                native_choice = Some(kept);
                self.counts.note(if kept == s.stack_object_after {
                    "autonomy: no interaction passed, the stack object kept, as the log's"
                } else {
                    "autonomy: no interaction passed, the stack object kept, DIFFERENT"
                });
            } else {
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
/// A plan's points natively (logs with 'r' and 'h'): the planner's search on the rects and
/// settings the plan's last search had, from its coarse way's start to its end, refined, and the
/// point after the goal (none if the plan took no goal); (x, y) like the plan's points.
fn native_plan_points(pl: &PlanRec, world: &World) -> Option<Vec<(i32, i32)>> {
    // a request with no goals (logs with 'h', which every plan of such a log has) plans nothing
    if pl.goals.is_empty() && pl.approach.is_some() {
        return Some(Vec::new());
    }
    if pl.coarse.len() < 2 || pl.lookup_regions[0].is_empty() {
        return None;
    }
    let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
    let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
    // from the Sim's place (the search's start) to the request's goal: its only one, or the one
    // the backward search takes of several
    let &(sx, sy) = pl.coarse.first()?;
    let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
    let pick = if goals.len() == 1 { 0 } else { choose_goal(&rects, growth, (sy, sx), &goals)? };
    let (gy, gx, _) = *goals.get(pick)?;
    let way = coarse_way(&rects, growth, (sy, sx), (gy, gx))?;
    let mut points: Vec<(i32, i32)> = refine(&way).iter().map(|&(y, x)| (x, y)).collect();
    if let (Some(a), true) = (pl.approach, pl.chosen >= 0) {
        let facing = world.find_object(a[5] as i16).and_then(|o| o.data.get(1)).map(|&d| d as i32);
        points.extend(point_after_goal(a[0], a[1], (a[3], a[4]), facing, a[2], &points));
    }
    Some(points)
}

/// `lot`: the lot as loaded, and the disc's copy of it (its house) if a save is loaded.
fn run(log: &Log, lib: &Library, lot: (&LotChunks, Option<&LotChunks>), globals: &[i16], neighbours: &[Neighbour], bench: Option<Bench>, card_lot: bool) -> Option<(Counts, Option<Bench>)> {
    let (club, house) = lot;
    let mut world = World::new(lib);
    world.load_lot(club).expect("the lot");
    let took_house = house.is_some_and(|h| world.take_house_from(h).unwrap_or(false));
    if has("REPLAY_SHOW_LOW_TREES") {
        for f in lib.files().iter().filter(|f| f.path.to_ascii_lowercase().contains("global")) {
            let t462 = f.trees.get(&462).map(|&t| lib.tree_at(t).nodes.first().map(|n| (n.opcode, n.operand)));
            println!("file {}: {} trees, 462 node 0 {t462:?}", f.path, f.trees.len());
        }
        let files: BTreeSet<FileId> = world.objects().filter_map(|o| o.file).collect();
        for f in files {
            let low: Vec<u16> = lib.file(f).trees.keys().copied().filter(|&id| id < 0x1000).collect();
            if !low.is_empty() {
                println!("low trees in {}: {low:?}", lib.file(f).path);
            }
        }
    }
    if has("REPLAY_SHOW_SLOTTED") {
        for id in [197i16, 62] {
            if let Some(o) = world.find_object(id) {
                println!("loaded {id}: person {} container {} slot {} at ({},{},{}) guid {:08x}", o.person, o.data[2], o.data[3], o.x16 >> 4, o.y16 >> 4, o.level, o.guid);
            }
        }
        for o in world.objects() {
            if o.data.get(3).is_some_and(|&s| s >= 0) && o.data.get(2) == Some(&0) {
                let t = (o.x16 >> 4, o.y16 >> 4, o.level);
                let mates: Vec<String> = world.objects().filter(|x| x.id() != o.id() && (x.x16 >> 4, x.y16 >> 4, x.level) == t).map(|x| format!("{}{}", x.id(), if x.person { "P" } else { "" })).collect();
                println!("slotted, no container: {}{} slot {} at {t:?} x16 {} y16 {}; on its tile {mates:?}", o.id(), if o.person { "P" } else { "" }, o.data[3], o.x16, o.y16);
            }
        }
    }
    // the lot's globals as saved (its SIMI): the clock among them
    world.globals = globals.to_vec();
    if let Some(id) = env("REPLAY_SHOW_STACK").and_then(|v| v.parse::<i16>().ok())
        && let Some(o) = world.find_object(id)
    {
        {
            println!("file {:?} semi {:?}", o.file.map(|f| file_name(lib, f)), o.file.and_then(|f| lib.file(f).semi).map(|s| file_name(lib, s)));
            for f in &o.stack {
                if let Some(t) = f.tree {
                    let tr = lib.tree_at(t);
                    println!("  frame {} node {}: node op {:?} stack object {}", tr.id, f.node, tr.nodes.get(f.node as usize).map(|n| (n.opcode, n.true_target, n.false_target)), f.stack_object);
                }
            }
            println!("stack of {id}: {:?}", o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.file.map(|x| file_name(lib, x)))).collect::<Vec<_>>());
        }
    }
    if has("REPLAY_SHOW_REQUEUE") {
        for o in world.objects().filter(|o| o.person) {
            let diff: Vec<(usize, i16)> = o.person_data.iter().enumerate().filter(|(i, _)| *i < 80).map(|(i, &v)| (i, v)).collect();
            println!("requeue data {}: {:?}", o.id(), diff);
            println!("requeue entry {}: {:?}", o.id(), o.current);
            println!("requeue: {} current tree {} on {:?} stack {:?} pd32 {:?} pd29 {:?} data4 {:?} saved queue {}", o.id(), o.current.tree, o.current.objects, o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node)).collect::<Vec<_>>(), o.person_data.get(32), o.person_data.get(29), o.data.get(4), o.queue.len());
        }
    }
    world.requeue_current_on_community_lot();
    // The plans' points natively wherever the log carries the planner's inputs ('r', 'h'), so the
    // walk checks run on the native planner's ways (study REPLAY_LOGGED_PLANS: the log's instead)
    let patched;
    let mut native_plans = (0, 0);
    // --bench: the native planner's cost, each plan's whole request (goal choice, search,
    // refinement, the point after the goal) timed 21 times, the rounds interleaved over the plans
    // (a busy PC), each plan's median
    if bench.is_some() {
        let plans: Vec<&PlanRec> = log.plans.iter().filter(|pl| native_plan_points(pl, &world).is_some()).collect();
        let mut times: Vec<Vec<f64>> = vec![Vec::new(); plans.len()];
        for _ in 0..21 {
            for (k, pl) in plans.iter().enumerate() {
                let t0 = Instant::now();
                std::hint::black_box(native_plan_points(pl, &world));
                times[k].push(us(t0));
            }
        }
        let mut medians: Vec<(f64, usize)> = times
            .iter_mut()
            .enumerate()
            .map(|(k, t)| {
                t.sort_by(f64::total_cmp);
                (t[t.len() / 2], k)
            })
            .collect();
        medians.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f64 = medians.iter().map(|m| m.0).sum();
        if let Some(&(worst, k)) = medians.last() {
            let pl = plans[k];
            println!(
                "bench: the planner on {} plans, us a plan (medians of 21): median {:.1}, worst {:.1} (tick {}, {} goals, {} points), all {:.1}",
                plans.len(),
                medians[medians.len() / 2].0,
                worst,
                pl.tick,
                pl.goals.len(),
                pl.points.len(),
                total
            );
        }
    }
    let log = if has("REPLAY_LOGGED_PLANS") {
        log
    } else {
        let mut l = log.clone();
        for pl in &mut l.plans {
            if let Some(p) = native_plan_points(pl, &world) {
                native_plans.1 += usize::from(p == pl.points);
                if p != pl.points && has("REPLAY_SHOW_PLAN_DIFFS") {
                    let a = pl.approach;
                    let facing = a.and_then(|a| world.find_object(a[5] as i16)).and_then(|o| o.data.get(1)).copied();
                    let sim = a.and_then(|a| world.find_object(a[7] as i16)).map(|o| (o.person, o.facing(), o.data.get(1).copied()));
                    let place = a.and_then(|a| log.places.iter().rfind(|q| q.id == a[7] as i16 && q.tick < pl.tick).map(|q| (q.x, q.y, q.level)));
                    println!("plan tick {}: native {p:?}, the log's {:?}; approach {a:?} chosen {} facing {facing:?} sim {sim:?} place {place:?}", pl.tick, pl.points, pl.chosen);
                }
                native_plans.0 += 1;
                pl.points = p;
            }
        }
        patched = l;
        &patched
    };
    if has("REPLAY_SHOW_SIMI") {
        println!("lot globals: {:?}", &globals[..globals.len().min(40)]);
    }
    if has("REPLAY_SHOW_RELATIONSHIPS") {
        for o in world.objects().filter(|o| !o.relationships.is_empty()) {
            for r in &o.relationships {
                println!("relationship of {} with {}: {:?}", o.id(), r.with, r.values);
            }
        }
    }
    if has("REPLAY_SHOW_SAVED_STACKS") {
        for o in world.objects().filter(|o| o.person || !o.stack.is_empty() && env("REPLAY_SHOW_SAVED_STACKS").is_some_and(|v| v == "all")) {
            let fr: Vec<_> = o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.stack_object, f.engine, f.params.clone())).collect();
            println!("saved stack {}: {fr:?}", o.id());
        }
    }
    if has("REPLAY_SHOW_OBJECTS") {
        for o in world.objects() {
            let mut l = format!("object {}: guid {:08x}, {} at {},{} facing {}; data", o.id(), o.guid, o.file.map_or("-".to_string(), |f| lib.file(f).path.clone()), o.x16, o.y16, o.data[1]);
            for v in &o.data {
                l += &format!(" {v}");
            }
            println!("{l}");
        }
    }
    if has("REPLAY_SHOW_PAVED") {
        println!("paved regions natively: {:?}", world.walls.paved_regions());
    }
    if has("REPLAY_SHOW_WALL_BYTES") {
        // the second array's bytes (each tile's element) round the front yard, for study
        if let Ok(arrays) = read_lot_arrays(club) {
            for (n, a) in arrays.iter().enumerate() {
                let es = a.element_size as usize;
                println!("array {n}: {}x{}, {es} bytes a tile", a.width, a.height);
                for y in 5..16 {
                    let row: Vec<String> = (15..25).map(|x| {
                        let i = (y * a.width + x) as usize * es;
                        a.bytes[i..i + es].iter().map(|b| format!("{b:02x}")).collect::<String>()
                    }).collect();
                    println!("  y {y:2}: {}", row.join(" "));
                }
            }
        }
    }
    if let Some(id) = env("REPLAY_DUMP_TREE").and_then(|v| v.parse::<u16>().ok()) {
        for f in lib.files() {
            if let Some(&t) = f.trees.get(&id) {
                println!("tree {id} in {}:", f.path);
                for (n, node) in lib.tree_at(t).nodes.iter().enumerate() {
                    println!("  {n}: op {} -> {}/{} operand {:?}", node.opcode, node.true_target, node.false_target, &node.operand[..8]);
                }
            }
        }
    }
    if let Some(v) = env("REPLAY_SHOW_STACK_DUMPS") {
        // study: the interpreters' first-step dumps ('k') of those whose first step is this tree
        println!("log steps: {}; dumps {}; first sims {:x?}", log.steps.len(), log.stack_dumps.len(), log.stack_dumps.iter().take(3).map(|d| (d.tick, d.sim, d.frame)).collect::<Vec<_>>());
        let want = atoi(&v) as u16;
        // or the interpreter of the step whose frame's low half is `want`
        let by_frame: Option<u32> = log.steps.iter().find(|x| x.frame & 0xFFFF == want as u32).map(|x| x.sim);
        if let Some(sm) = by_frame {
            println!("the step with frame ..{want:04x}: sim {sm:08x}; its dump {}", log.stack_dumps.iter().any(|d| d.sim == sm));
        }
        for d in &log.stack_dumps {
            let (tick, sim, frame, words, below) = (&d.tick, &d.sim, &d.frame, &d.words, &d.below);
            if by_frame.is_some_and(|sm| sm == *sim) {
                println!("dump tick {tick} sim {sim:08x} frame {frame:08x}; words {words:08x?}");
                for row in below.chunks(32).skip(8) {
                    println!("  {:02x?}", row);
                }
            }
            let first = log.steps.iter().find(|x| x.sim == *sim);
            if has("REPLAY_SHOW_STACK_DUMP_FIRSTS") {
                println!("dump sim {sim:08x} frame {frame:08x} first {:?}", first.map(|x| (x.tick, x.tree, x.node, x.frame)));
            }
            let Some(first) = first else { continue };
            if first.tree != want && first.frame & 0xFFFF != want as u32 {
                continue;
            }
            println!("dump tick {tick} sim {sim:08x} frame {frame:08x} first {}:{}; words {:08x?}", first.tree, first.node, words);
            for row in below.chunks(32).skip(8) {
                println!("  {:02x?}", row);
            }
        }
    }
    if let Some(pat) = env("REPLAY_SHOW_FILE") {
        // study: a file's function table and tree labels
        for f in lib.files().iter().filter(|f| f.path.contains(&pat)) {
            println!("file {}: functions {:?}; semi {:?}", f.path, f.functions, f.semi.map(|s| &lib.file(s).path));
            for (id, sl) in &f.slots {
                println!("  slots {id}: {sl:?}");
            }
            for (id, t) in &f.tables {
                println!("  table {id}: first {:?}; (action, guard, flags) {:?}", f.table_first.get(id), t.iter().map(|e| (e.action, e.guard, e.flags)).collect::<Vec<_>>());
            }
            for d in &f.definitions {
                let hits: Vec<(usize, u16)> = (0..80).map(|i| (i, d.field(i))).filter(|(_, v)| f.functions.contains_key(v)).collect();
                println!("  def {:08x}: fields naming a function table {hits:?}", d.guid());
            }
            let mut names: Vec<_> = f.tree_by_name.iter().map(|(n, id)| (*id, n.clone())).collect();
            names.sort();
            for (id, n) in names {
                println!("  tree {id} {n:?}");
            }
        }
    }
    if let Some(pat) = env("REPLAY_TREE_NAMES") {
        // study: the BHAV labels matching a pattern, by file
        for f in lib.files() {
            for (name, id) in &f.tree_by_name {
                if name.to_lowercase().contains(&pat.to_lowercase()) {
                    println!("tree name: {} {id} {name:?}", f.path);
                }
            }
        }
    }
    if has("REPLAY_SHOW_PERSON_TABLE") {
        for f in lib.files().iter().filter(|f| f.path.ends_with("person_sg.iff")) {
            for (&id, t) in &f.tables {
                println!("person table {id}: first {:?}; guards {:?}", f.table_first.get(&id), t.iter().map(|e| (e.guard, e.autonomy_threshold, e.flags)).collect::<Vec<_>>());
            }
        }
    }
    if let Some(dump) = env("REPLAY_DUMP_ARRAYS") {
        // the lot's three arrays, raw, for study
        let bytes: Vec<u8> = read_lot_arrays(club).unwrap_or_default().iter().flat_map(|a| a.bytes.iter().copied()).collect();
        let _ = std::fs::write(dump, bytes);
    }
    // The disc's club: its motives do not move by themselves (measured; World::motives_run).
    world.motives_run = card_lot || has("REPLAY_MOTIVES_RUN");
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
        chosen: None,
        score_at: 0,
        on_check: None,
        counts: Counts { quiet, ..Default::default() },
        shown: Shown::default(),
        now: 0,
        routing_returns: BTreeMap::new(),
        pre_made: BTreeSet::new(),
    };
    for i in 0..native_plans.0 {
        h.counts.note(if i < native_plans.1 {
            "planner: a plan's points natively, in the log's place, the same as the log's"
        } else {
            "planner: a plan's points natively, in the log's place, NOT the log's"
        });
    }
    let mut consumed: BTreeSet<usize> = BTreeSet::new(); // log steps a native primitive's checks have stepped
    let mut sim = Simulator::new(lib);
    if took_house {
        h.counts.note("load: the lot's house (walls, floors) from the disc's copy, the save keeping none");
    }
    let mut finder = TreeFinder { lib, cache: HashMap::new(), shown: 0 };
    // stand-ins given a file before their first own step (the target of a player's command):
    // their first step still starts them as a stand-in's does
    let mut unstarted: BTreeSet<ObjectKey> = BTreeSet::new();
    let mut ids = stack_ids(log);
    place_quiet_stacks(log, &h.world, &mut ids, &mut h.counts, lib);
    let mut native_places: BTreeMap<(i16, u32), (i32, i32)> = BTreeMap::new();
    // a walking Sim's place from the native walk where there is one (study REPLAY_LOGGED_PLACES:
    // every place the log's)
    let use_native_places = !has("REPLAY_LOGGED_PLACES");
    // the routing runs' native returns, for the routing primitives' steps (its own counts dropped:
    // the full pass after the steps counts them)
    if !quiet && !log.places.is_empty() {
        let native = routing(log, lib, &h.world, &h.world, &mut Counts { quiet: true, ..Default::default() }, &ids);
        h.routing_returns = native.returns;
        native_places = native.places;
    }

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
    let mut last_frame: HashMap<u32, (u32, TreeId)> = HashMap::new();
    let mut unseen: BTreeSet<u32> = BTreeSet::new(); // placeholder addresses of saved frames below a first step
    let mut delivered: BTreeSet<u32> = BTreeSet::new(); // Sims whose first interaction was seen
    if has("REPLAY_SHOW_OFFLIST") {
        let want = [0x80ff7f78u32, 0x80ff8350, 0x80ff8728];
        for st in log.steps.iter().filter(|st| want.contains(&st.sim) && st.tick == 260) {
            println!("offlist {:x} tick {} {}:{} -> {} so {} reads {:?} params {:?}", st.sim, st.tick, st.tree, st.node, st.after, st.stack_object, st.lookups.iter().map(|v| (v.scope, v.data, v.value, v.object == st.sim)).collect::<Vec<_>>(), st.params);
        }
        println!("offlist ids: {:?}", want.iter().map(|p| (format!("{p:x}"), ids.get(p))).collect::<Vec<_>>());
    }
    let mut frame_tree: HashMap<(u32, u32, u16), (TreeId, FileId)> = HashMap::new();
    // a log of today's observer: its plans carry the search's region lists ('r')
    let new_observer = log.plans.iter().any(|p| !p.lookup_regions[0].is_empty());
    let (mut streak, mut best_streak) = (0usize, 0usize);

    // Which steps are turns: those in the tick's pass over the objects ('Q' records). The others
    // are the engine's own (the pie menu's checks on a Sim's stack: docs/simulation.md). A log
    // without them is taken as every step a turn.
    let have_passes = !log.passes.is_empty();
    let engine_run: Vec<bool> = log.steps.iter().map(|s| have_passes && !s.in_pass).collect();
    if !have_passes {
        h.counts.note("log: no pass records (an older log): every step taken as a turn, no tick as paused");
    }

    let mut place_at = 0; // the next 'P' record (places: the native walk's where it has one, else the log's)
    let mut motive_at = 0; // the next 'N' record
    let mut shown_motives = 0;
    let ev_env = env("REPLAY_EVALUATION");
    let mut engine_list_at = 1;
    // The time of day's light ('3' records, a light log's) from the engine's own clock: the
    // hour (global 0), the minute (global 5) and two seconds a pass counted (`clock_pass`), each
    // tick's calls against the clock after the tick (tick 0's before the first).
    let mut time_at = 0;
    check_time_of_day(log, 0, &h.world, &mut time_at, &mut h.counts);
    for t in 1..=log.ticks {
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
                                first = want_of(&mut h, tab.first);
                                checks += 1;
                            }
                            for e in lib.table(tf, tid) {
                                if e.guard == 0 || (first == Some(false) && e.flags & 0x400 == 0) || e.flags & 0x80 != 0 {
                                    continue;
                                }
                                if h.nested_pos >= h.nested.len() {
                                    break;
                                }
                                let _ = want_of(&mut h, e.guard);
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
                let fields: Vec<usize> = h.world.definition(o).map(|d| (0..d.fields.len()).filter(|&k| d.field(k) == s.tree as u16 as i32 as _).collect()).unwrap_or_default();
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
                interp.step(&mut h);
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
                interp.step(&mut h);
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
                interp.step(&mut h);
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
            if let Ok(w) = std::env::var("REPLAY_WATCH_ID")
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
            let stepped = interp.step(&mut h);
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
        motives_before(i16::MAX, &sim, &mut h.world, &mut motives_done);
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
                if has("REPLAY_SHOW_CAND_FLAGS") {
                    let mut by_flag: BTreeMap<(u32, bool), Vec<i16>> = BTreeMap::new();
                    for c in autonomy_candidates(world, me).iter().filter(|c| present.is_empty() || present.contains(&c.id)) {
                        let e = c.interaction(lib);
                        by_flag.entry((e.flags, logged.contains(&(c.id, e.guard)))).or_default().push(c.id);
                    }
                    for ((f, inlog), ids) in by_flag {
                        println!("   flags {f:08x} checked by the original {inlog}: {} candidates, e.g. {:?}", ids.len(), &ids[..ids.len().min(6)]);
                    }
                    println!("   logged {logged:?}");
                    for &(id, _) in logged.iter().take(1) {
                        if let Some(o) = world.find_object(id) {
                            let d = world.definition(o);
                            println!("   {id}: file {:?} def {:?} field 6 {:?} field 7 {:?} tables {:?}", o.file.map(|f| file_name(lib, f)), o.definition, d.map(|d| d.field(6)), d.map(|d| d.field(7)), d.and_then(|d| o.file.and_then(|f| lib.interactions_of(d, f))));
                            if let (Some(d), Some(f)) = (d, o.file) && let Some(t) = lib.interactions_of(d, f) {
                                for e in lib.table(f, t) {
                                    println!("      action {} guard {} threshold {} flags {:x}", e.action, e.guard, e.autonomy_threshold, e.flags);
                                }
                            }
                        }
                    }
                }
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
        // the walkers' bodies from the lot as it loaded (a walker's id may be another object's,
        // or none, by the run's end)
        let mut loaded = World::new(lib);
        loaded.load_lot(club).expect("the lot");
        if let Some(h) = house {
            let _ = loaded.take_house_from(h);
        }
        let _ = routing(log, lib, world, &loaded, counts, &ids);
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
/// The routing runs' native returns: (sim, tree, node) of the routing step, its run's first tick,
/// the tick it returns natively (its walk, turn and frames as `routing_return_tick` gives them).
type RoutingReturns = BTreeMap<(u32, u16, u16), Vec<(u32, u32)>>;

/// What the routing pass gives the host: each routing run's native return tick, and each walking
/// Sim's native place at the ticks the log has one (by id and tick).
#[derive(Default)]
struct NativeRouting {
    returns: RoutingReturns,
    places: BTreeMap<(i16, u32), (i32, i32)>,
}

fn routing(log: &Log, lib: &Library, world: &World, loaded: &World, counts: &mut Counts, ids: &BTreeMap<u32, i16>) -> NativeRouting {
    let mut returns: RoutingReturns = BTreeMap::new();
    let mut native_places: BTreeMap<(i16, u32), (i32, i32)> = BTreeMap::new();
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
    // study (REPLAY_SHOW_ROUTE_OPS): each routing step that walks (opcode 27 or 45, result 1), its
    // operand bytes
    if has("REPLAY_SHOW_ROUTE_OPS") {
        for s in log.steps.iter().filter(|s| s.result == 1) {
            if let Some(t) = log.tree_of(s) {
                let at = s.node as usize * 12;
                if at + 12 <= t.raw.len() {
                    let op = u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF;
                    if matches!(op, 27 | 45) {
                        println!("route op tick {} sim {} opcode {op} operands {:02x?} stack {}", s.tick, ids.get(&s.sim).copied().unwrap_or(-1), &t.raw[at + 4..at + 12], s.stack_object);
                    }
                }
            }
        }
    }
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
        // the plan this step's Sim made (by its id where the log names the planner, 'h')
        let sim_id = ids.get(&s.sim).copied();
        let Some(pl) = log.plans.iter().find(|p| {
            p.tick == s.tick && p.points.first() == Some(&start) && p.points.len() >= 2 && p.approach.is_none_or(|a| Some(a[7] as i16) == sim_id)
        }) else {
            continue;
        };
        let Some(o) = world.find_object(s.stack_object) else {
            // An object not on the lot natively: its goals (logs with 'H') checked by their shape
            // alone, round the plan's end (its object) as some facing and proximity give them.
            if let Some(&center) = pl.points.last().filter(|_| !pl.goals.is_empty()) {
                let bits: i32 = pl.goals.iter().map(|g| (g[3] & 0xFF) as i32).fold(0, |a, b| a | b);
                let want: Vec<(u8, (i32, i32))> =
                    pl.goals.iter().map(|g| ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32))).collect();
                let fits = (0..8).any(|f| {
                    (1..=64).any(|p| {
                        let slot = Slot { flags: bits, optimal_proximity: p, ..Slot::default() };
                        slot_places(&slot, center.0, center.1, f) == want
                    })
                });
                counts.note(if fits {
                    "routing: a request's goals round an object not on the lot natively as a slot's (some facing and proximity)"
                } else {
                    "routing: a request's goals round an object not on the lot natively NOT a slot's"
                });
            }
            continue;
        };
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
        // a person faces the way it last stepped (its last two logged places before the step,
        // eighths of a turn: 0 to -x, 2 to +y, 4 to +x, 6 to -y), else as the save has it
        let facing = {
            let mut seen = log.places.iter().filter(|p| p.id == o.id() && p.tick <= s.tick).rev();
            match (o.person, seen.next(), seen.next()) {
                (true, Some(b), Some(a)) => match ((b.x - a.x).signum(), (b.y - a.y).signum()) {
                    (-1, 0) => 0,
                    (-1, 1) => 1,
                    (0, 1) => 2,
                    (1, 1) => 3,
                    (1, 0) => 4,
                    (1, -1) => 5,
                    (0, -1) => 6,
                    (-1, -1) => 7,
                    _ => o.facing(),
                },
                _ => o.facing(),
            }
        };
        let offered = slot_places(slot, target.0, target.1, facing);
        let mut places = offered.clone();
        // A place across a wall from its object (another room, though a door may lead there) is
        // not offered, nor one another object (not a person) stands on.
        let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
        places.retain(|&(_, p)| !occupied(p) && room(p) == room(target));
        // The request's goals (logs with 'H'): a slot of one proximity offers exactly the open
        // places, in order, each with its direction's bit; one of a range offers an area (not yet).
        if !pl.goals.is_empty() {
            // a goal's score (its third word), for the goals that are the slot's own places: the
            // slot's, with its random part
            let score = |counts: &mut Counts, g: &[u32; 4]| {
                counts.note(if slot_goal_score(slot, g[3] >> 16 != 0).contains(&g[2]) {
                    "routing: a slot goal's score natively the original's (135, 90 more for a named object, the random part when flagged)"
                } else {
                    "routing: a slot goal's score natively NOT the original's"
                });
            };
            if slot.min_proximity == slot.max_proximity {
                let same = places.len() == pl.goals.len()
                    && places.iter().zip(&pl.goals).all(|(&(k, p), g)| (g[0] as i32, g[1] as i32) == p && g[3] & 0xFF == 1 << k);
                if same {
                    pl.goals.iter().for_each(|g| score(counts, g));
                }
                counts.note(if same {
                    "routing: a request's goals natively the open places of its slot, in order, with their bits"
                } else {
                    "routing: a request's goals natively NOT the open places of its slot"
                });
                if !same && has("REPLAY_SHOW_GOALS") {
                    let theirs: Vec<_> = pl.goals.iter().map(|g| ((g[3] & 0xFF).trailing_zeros(), g[0] as i32, g[1] as i32)).collect();
                    println!("slot goals tick {}: object {} at {target:?} facing {}: native places {places:?} (offered {offered:?}); the request's {theirs:?}", pl.tick, o.id(), o.facing());
                    println!("   the step: sim {} tree {} number {number} scope {scope} stack object {} raw {raw:?}; the plan's planner {:?}", ids.get(&s.sim).copied().unwrap_or(-1), s.tree, s.stack_object, pl.approach.map(|a| a[7]));
                    println!("   object 60's places: {:?}", log.places.iter().filter(|p| p.id == 60).map(|p| (p.tick, p.x, p.y)).collect::<Vec<_>>());
                    for id in [12i16, 197, 198, 202, 237] {
                        println!("   person {id} at tick {}: {:?}", pl.tick, log.places.iter().rfind(|p| p.id == id && p.tick <= pl.tick).map(|p| (p.x, p.y, p.tick)));
                    }
                    // which slot's area holds the request's goals (the object's table and the global one)
                    let tables = [world.definition(o).and_then(|d| lib.file(file).slots.get(&d.slot_table())), lib.global_file().and_then(|g| g.slots.values().next())];
                    for (t, table) in tables.iter().enumerate() {
                        for (n, sl) in table.iter().flat_map(|t| t.iter()).filter(|s| s.kind == 3).enumerate() {
                            let area = slot_area(sl, target.0, target.1, o.facing());
                            let inside = pl.goals.iter().filter(|g| area.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count();
                            if inside > 0 {
                                println!("   table {t} slot {n}: {inside} of {} goals in its area of {} ({sl:?})", pl.goals.len(), area.len());
                            }
                            // a range slot: the centre and facing whose area holds the most goals
                            if inside > 1 && sl.min_proximity != sl.max_proximity {
                                let mut best = (0, (0, 0), 0);
                                for dx in (-64..=64).step_by(8) {
                                    for dy in (-64..=64).step_by(8) {
                                        for f in 0..8 {
                                            let a = slot_area(sl, target.0 + dx, target.1 + dy, f);
                                            let k = pl.goals.iter().filter(|g| a.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count();
                                            if k > best.0 {
                                                best = (k, (target.0 + dx, target.1 + dy), f);
                                            }
                                        }
                                    }
                                }
                                println!("   table {t} slot {n}: best {} of {} goals with the area round {:?} facing {}", best.0, pl.goals.len(), best.1, best.2);
                                let at_target: Vec<usize> = (0..8).map(|f| {
                                    let a = slot_area(sl, target.0, target.1, f);
                                    pl.goals.iter().filter(|g| a.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count()
                                }).collect();
                                println!("   table {t} slot {n}: round the target {target:?}, goals in the area by facing 0..7: {at_target:?}");
                            }
                        }
                    }
                }
            } else {
                // a slot of a range: each goal one of its area's places, with its direction's bit
                // (by the object's native facing, else by the facing the goals' bits give: the
                // paper girl's save facing is stale)
                let fits = |f: i16| {
                    let area = slot_area(slot, target.0, target.1, f);
                    pl.goals.iter().filter(|g| area.contains(&((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32)))).count()
                };
                let native = fits(o.facing());
                let (facing, n) = if native == pl.goals.len() { (o.facing(), native) } else { (0..8).map(|f| (f, fits(f))).max_by_key(|x| x.1).unwrap() };
                for i in 0..pl.goals.len() {
                    counts.note(match (i < n, facing == o.facing()) {
                        (true, true) => "routing: a goal of a range slot natively one of its area's places (its bit's grid)",
                        (true, false) => "routing: a goal of a range slot one of its area's places (its bit's grid; the facing the bits give)",
                        _ => "routing: a goal of a range slot NOT one of its area's places",
                    });
                }
                // the open places among them, which the original draws its goals from at random:
                // those in the object's room, strictly inside no blocking obstacle of the plan (as
                // logged; an obstacle's edge is open) and on or in no wall's rectangle
                let on_or_in = |p: (i32, i32), r: &[i32]| p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3];
                let strictly_in = |p: (i32, i32), r: &[i32]| p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3];
                let wall_rects = walls.wall_rects(3);
                let open: Vec<(u8, (i32, i32))> = slot_area(slot, target.0, target.1, facing)
                    .into_iter()
                    .filter(|&(_, p)| room(p) == room(target))
                    .filter(|&(_, p)| !pl.obstacles.iter().any(|r| r[4] == BLOCKS && strictly_in(p, &r[..4])) && !wall_rects.iter().any(|r| on_or_in(p, r)))
                    .collect();

                for g in &pl.goals {
                    let goal = ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32));
                    if open.contains(&goal) {
                        score(counts, g);
                    }
                    counts.note(if open.contains(&goal) {
                        "routing: a goal of a range slot among its area's open places (its room, clear of the obstacles and walls)"
                    } else {
                        "routing: a goal of a range slot NOT among its area's open places"
                    });
                }
                // the scores by the builder's draws (logs with 'R' site 2): each place in the area's
                // order takes the next draw, a direction perhaps stopping short (its classifier,
                // fn_80248F4C, not yet read: the one short direction's count taken from the draws'
                // total); each goal's score that of its place
                let draws: Vec<u32> = log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 2).map(|r| r.value).collect();
                if slot.flags & 0x2000 != 0 && !draws.is_empty() {
                    let area = slot_area(slot, target.0, target.1, facing);
                    let per: Vec<usize> = (0..8u8).map(|k| area.iter().filter(|x| x.0 == k).count()).collect();
                    let short = draws.len() as i64 - per.iter().sum::<usize>() as i64;
                    // which direction stops short: the one whose shortening fits the goals best
                    let score_with = |sk: usize, sn: usize| {
                        let mut it = draws.iter();
                        // (direction, place, score)
                        let mut scored: Vec<(u8, (i32, i32), u32)> = Vec::new();
                        for (k, &count) in per.iter().enumerate() {
                            let n = if k == sk { sn } else { count };
                            for &(d, place) in area.iter().filter(|x| x.0 as usize == k).take(n) {
                                if let Some(&r) = it.next() {
                                    scored.push((d, place, 135 + r % 20 - 10));
                                }
                            }
                        }
                        pl.goals
                            .iter()
                            .filter(|g| scored.iter().any(|&(d, p, sc)| (d, p) == ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32)) && sc == g[2]))
                            .count()
                    };
                    let best = (0..8)
                        .filter(|&k| short <= 0 && per[k] as i64 + short >= 0)
                        .map(|k| (score_with(k, (per[k] as i64 + short) as usize), k))
                        .max();
                    if let Some((hits, sk)) = best {
                        // the pick (logs with 'R' site 3): the goals, in order, the places the draws
                        // take (`slot_pick`, the cap the goals' count) among the open ones the
                        // builder reached
                        let picks: Vec<u32> = log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 3).map(|r| r.value).collect();
                        let reached: Vec<(u8, (i32, i32))> = (0..8usize)
                            .flat_map(|k| {
                                let n = if k == sk { (per[k] as i64 + short) as usize } else { per[k] };
                                area.iter().filter(move |x| x.0 as usize == k).take(n).copied()
                            })
                            .filter(|x| open.contains(x))
                            .collect();
                        if !picks.is_empty() {
                            let taken = slot_pick(reached.len(), &picks, pl.goals.len());
                            for (i, g) in pl.goals.iter().enumerate() {
                                let goal = ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32));
                                counts.note(if taken.get(i).map(|&j| reached[j]) == Some(goal) {
                                    "routing: a range slot's goal natively the place the pick's draw takes, in order"
                                } else {
                                    "routing: a range slot's goal NOT the place the pick's draw takes"
                                });
                            }
                        }
                        if has("REPLAY_SHOW_GOALS") {
                            println!("area draws at tick {}: {} draws, direction {sk} short by {}", pl.tick, draws.len(), -short);
                        }
                        // study (REPLAY_DUMP_AREA): the scored places in order, open or not
                        if has("REPLAY_DUMP_AREA") {
                            let mut it = draws.iter();
                            for (k, &count) in per.iter().enumerate() {
                                let n = if k == sk { (count as i64 + short) as usize } else { count };
                                for &(d, p) in area.iter().filter(|x| x.0 as usize == k).take(n) {
                                    if let Some(&r) = it.next() {
                                        println!("scored\t{d}\t{}\t{}\t{}\t{}", p.0, p.1, 135 + r % 20 - 10, open.contains(&(d, p)) as u8);
                                    }
                                }
                            }
                            for r in log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 3) {
                                println!("pick\t{}", r.value);
                            }
                            for g in &pl.goals {
                                println!("goal\t{}\t{}\t{}\t{}", (g[3] & 0xFF).trailing_zeros(), g[0], g[1], g[2]);
                            }
                        }
                        for i in 0..pl.goals.len() {
                            counts.note(if i < hits {
                                "routing: a range slot goal's score its place's draw, in the builder's order (one direction's count from the draws)"
                            } else {
                                "routing: a range slot goal's score NOT its place's draw in the builder's order"
                            });
                        }
                    }
                }
                if has("REPLAY_SHOW_GOALS") {
                    println!("area at tick {}: facing {facing} (native {}), {n} of {} goals among {} places, {} open; slot {slot:?}", pl.tick, o.facing(), pl.goals.len(), slot_area(slot, target.0, target.1, facing).len(), open.len());
                }
            }
        }
        let d2 = |p: &(i32, i32)| (p.0 - start.0).pow(2) + (p.1 - start.1).pow(2);
        // a slot of one proximity: the nearest open place; one of a range (its goals the request's,
        // checked above): the one the backward search takes of them
        let got = if slot.min_proximity != slot.max_proximity && pl.goals.len() > 1 && !pl.lookup_regions[0].is_empty() {
            let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
            let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
            let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
            choose_goal(&rects, growth, (start.1, start.0), &goals).map(|i| (goals[i].1, goals[i].0))
        } else {
            places.iter().map(|x| x.1).min_by_key(d2)
        };
        counts.note(if got == Some(want) {
            "routing: a routing step's goal natively the plan's (the nearest open place of its slot)"
        } else {
            "routing: a routing step's goal natively NOT the plan's"
        });
        if let Some(g) = usize::try_from(pl.chosen).ok().and_then(|i| pl.goals.get(i)) {
            counts.note(if got == Some((g[0] as i32, g[1] as i32)) {
                "routing: a routing step's goal natively the one the planner took"
            } else {
                "routing: a routing step's goal natively NOT the one the planner took"
            });
        }
        if has("REPLAY_SHOW_GOALS") {
            for &(k, p) in &offered {
                let on: Vec<String> = world
                    .objects()
                    .filter(|x| tile((x.x16, x.y16)) == tile(p))
                    .map(|x| format!("{} {} level {} person {} lot {}", x.id(), x.file.map_or("-".to_string(), |f| file_name(lib, f)), x.level, x.person, x.on_lot()))
                    .collect();
                println!("   place {k} {p:?} tile {:?} room {}: {on:?}", tile(p), walls.room(tile(p).0, tile(p).1));
            }
            let logged: Vec<(u32, (i32, i32))> = pl.goals.iter().map(|g| (g[3] & 0xFF, (g[0] as i32, g[1] as i32))).collect();
            println!(
                "goals at tick {}: slot {number} ({scope}) flags {:x} proximity {}..{} of {} facing {}: offered {offered:?}, open {places:?}, logged {logged:?} took {}; native {got:?}, the plan's {want:?}",
                s.tick,
                slot.flags,
                slot.min_proximity,
                slot.max_proximity,
                o.id(),
                o.facing(),
                pl.chosen
            );
            if let Some(t) = world.definition(o).and_then(|d| lib.file(file).slots.get(&d.slot_table())) {
                for (i, sl) in t.iter().enumerate() {
                    println!("   table slot {i}: {sl:?}");
                }
            }
            println!("   slot fields {slot:?}; scores {:?}; last words {:x?}", pl.goals.iter().map(|g| g[2]).collect::<Vec<_>>(), pl.goals.iter().map(|g| g[3]).collect::<Vec<_>>());
        }
    }
    // The obstacles the planner gathered for each plan (logs with 'X'): their shapes (a cost of
    // 0x7FFFFFFF grown by half the width, a cost of 10 not), and how they meet the native
    // objects (study: REPLAY_SHOW_OBSTACLES, each with the objects whose place lies in it).
    // a request of several goals: the goal it takes natively (the backward search)
    for pl in log.plans.iter().filter(|p| p.goals.len() > 1 && p.coarse.len() >= 2 && !p.lookup_regions[0].is_empty()) {
        let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
        let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
        let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
        let (sx, sy) = pl.coarse[0];
        let ours = choose_goal(&rects, growth, (sy, sx), &goals);
        counts.note(if ours.map(|i| i as i32) == Some(pl.chosen) {
            "planner: a request of several goals natively takes the planner's (the backward search)"
        } else {
            "planner: a request of several goals natively NOT the planner's"
        });
        if ours.map(|i| i as i32) != Some(pl.chosen) && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
            println!("goal choice tick {}: native {ours:?}, the planner's {} of {}", pl.tick, pl.chosen, goals.len());
        }
    }
    for pl in log.plans.iter().filter(|p| p.approach.is_some()) {
        if let Some(p) = native_plan_points(pl, world) {
            counts.note(if p == pl.points {
                "planner: a plan natively, search to its point after the goal, all its points the original's"
            } else {
                "planner: a plan natively NOT all its points the original's"
            });
        }
    }
    // The planner's search natively (logs with 'r'): on the rects and settings the plan's last
    // search had, from its coarse way's start to its end, the coarse way point by point.
    for pl in log.plans.iter().filter(|p| p.coarse.len() >= 2 && !p.lookup_regions[0].is_empty()) {
        let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
        let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
        let turned = |&(x, y): &(i32, i32)| (y, x);
        let theirs: Vec<(i32, i32)> = pl.coarse.iter().map(turned).collect();
        let way = coarse_way(&rects, growth, theirs[0], theirs[theirs.len() - 1]);
        let ours = way.as_ref().map(|w| w.points.clone());
        let same = ours.as_ref() == Some(&theirs);
        // and the plan's points ('G', in (x, y)) from it: the refined way, then the one the
        // plan adds after the goal
        if let Some(w) = &way {
            let refined: Vec<(i32, i32)> = refine(w).iter().map(|&(y, x)| (x, y)).collect();
            let fits = pl.points.len() >= refined.len() && pl.points[..refined.len()] == refined[..];
            counts.note(if fits {
                "planner: a plan's points ('G') natively the refined way, point by point (but the one after the goal)"
            } else {
                "planner: a plan's points ('G') natively NOT the refined way"
            });
            if !fits && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
                println!("refine tick {}: theirs {:?}; native {refined:?}", pl.tick, pl.points);
            }
            // the point the plan adds after its goal (logs with 'h')
            if let (true, Some(a)) = (fits, pl.approach) {
                let facing = world.find_object(a[5] as i16).and_then(|o| o.data.get(1)).map(|&d| d as i32);
                // a plan that took no goal (the request's +88 left at -1) adds none
                let ours = if pl.chosen < 0 { None } else { point_after_goal(a[0], a[1], (a[3], a[4]), facing, a[2], &refined) };
                let theirs = pl.points.get(refined.len()).copied();
                counts.note(if ours == theirs {
                    "planner: a plan's point after its goal (or none) natively its own"
                } else {
                    "planner: a plan's point after its goal (or none) natively NOT its own"
                });
                if ours != theirs && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
                    println!("after goal tick {}: kind {} facer {} facing {facing:?} theirs {theirs:?}; native {ours:?}", pl.tick, a[0], a[5]);
                }
            }
        }
        counts.note(if same {
            "planner: a plan's search ('I') natively its coarse way, point by point"
        } else {
            "planner: a plan's search ('I') natively NOT its coarse way"
        });
        if !same && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
            println!("search tick {}: theirs {theirs:?}; native {ours:?}", pl.tick);
            if let Some(&(sx, sy)) = pl.coarse.first() {
                // the regions (y0, x0, y1, x1, cost) that hold the start, in each set
                for (set, rs) in [("regions", &pl.regions), ("lookup 0", &pl.lookup_regions[0]), ("lookup 1", &pl.lookup_regions[1])] {
                    for (k, r) in rs.iter().enumerate().filter(|(_, r)| r[0] <= sy && sy < r[2] && r[1] <= sx && sx < r[3]) {
                        println!("   start ({sy}, {sx}) in {set} {k} of {}: {r:?}", rs.len());
                    }
                }
            }
        }
    }
    let show_obstacles = has("REPLAY_SHOW_OBSTACLES");
    for pl in log.plans.iter().filter(|p| !p.obstacles.is_empty()) {
        if show_obstacles {
            println!("obstacles tick {} width {} ({} rects)", pl.tick, pl.obstacle_width, pl.obstacles.len());
        }
        let half = pl.obstacle_width / 2;
        let inside = |o: &housefire::world::Object, r: &[i32; 5]| o.x16 >= r[0] && o.x16 < r[2] && o.y16 >= r[1] && o.y16 < r[3];
        let solid = |o: &&housefire::world::Object| o.on_lot() && o.level == 1 && !o.person;
        for r in &pl.obstacles {
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            let grown = r[4] == i32::MAX;
            let core = if grown { (w - 2 * half, h - 2 * half) } else { (w, h) };
            counts.note(if core.0 > 0 && core.1 > 0 && core.0 <= 16 && core.1 <= 16 {
                "routing: an obstacle a part of a tile (blocking: grown by half the width)"
            } else {
                "routing: an obstacle NOT a part of a tile grown as its cost says"
            });
            let objs: Vec<String> = world
                .objects()
                .filter(solid)
                .filter(|o| inside(o, r))
                .map(|o| format!("{} {} f{}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.facing()))
                .collect();
            if show_obstacles {
                println!("   {:?} {w}x{h} cost {}: {objs:?}", &r[..4], r[4]);
            }
        }
        // Each obstacle's object (logs with 'J'): the objects the loop kept, in order, one a
        // rectangle (one may be dropped, as overlapping the Sim: the pairing that best fits).
        let kept: Vec<i16> = pl.tested.iter().filter(|t| !t.1).map(|t| t.0).collect();
        if !kept.is_empty() {
            let core = |r: &[i32; 5]| if r[4] == i32::MAX { [r[0] + half, r[1] + half, r[2] - half, r[3] - half] } else { [r[0], r[1], r[2], r[3]] };
            // an object's place: a person's as logged at the plan's tick (the walk is not yet
            // native), any other's natively
            let place = |id: i16| world.find_object(id).map(|o| if o.person { place_at(id, pl.tick).unwrap_or((o.x16, o.y16)) } else { (o.x16, o.y16) });
            let on_tile = |id: i16, r: &[i32; 5]| {
                let c = core(r);
                place(id).is_some_and(|p| {
                    let person = world.find_object(id).is_some_and(|o| o.person);
                    // a person's core is 6 on a side, centred on its place; another's within its tile
                    if person { c == [p.0 - 3, p.1 - 3, p.0 + 3, p.1 + 3] } else { p.0 >> 4 == c[0] >> 4 && p.1 >> 4 == c[1] >> 4 }
                })
            };
            let skips: Vec<Option<usize>> = if kept.len() == pl.obstacles.len() + 1 { (0..kept.len()).map(Some).collect() } else { vec![None] };
            let pairing = |skip: Option<usize>| -> Vec<i16> { kept.iter().enumerate().filter(|&(i, _)| Some(i) != skip).map(|(_, &id)| id).collect() };
            let best = skips
                .iter()
                .map(|&sk| pairing(sk))
                .max_by_key(|ids| ids.iter().zip(&pl.obstacles).filter(|(id, r)| on_tile(**id, r)).count())
                .unwrap_or_default();
            counts.add("routing: obstacles paired with the objects the loop kept (as many, or one dropped)", (best.len() == pl.obstacles.len()) as usize);
            for (&id, r) in best.iter().zip(&pl.obstacles) {
                let c = core(r);
                // its footprint natively (not a person's): the tile less the definition's mask
                if let Some(o) = world.find_object(id).filter(|o| !o.person) {
                    let mask = world.definition(o).map_or(0, |d| d.field(75));
                    let width = world.definition(o).map_or(0, |d| d.field(65));
                    if show_obstacles && footprint(mask, width, o.x16, o.y16, o.facing()) != c {
                        println!("   footprint miss: object {id} mask {mask:04x} facing {}: native {:?}, the log's {c:?}", o.facing(), footprint(mask, width, o.x16, o.y16, o.facing()));
                    }
                    counts.note(if footprint(mask, width, o.x16, o.y16, o.facing()) == c {
                        "routing: an obstacle's footprint natively its object's (the tile less its OBJD mask and width)"
                    } else {
                        "routing: an obstacle's footprint natively NOT its object's"
                    });
                }
                let what = match world.find_object(id) {
                    None => "routing: an obstacle's object not in the native world",
                    Some(_) if on_tile(id, r) => "routing: an obstacle's object natively on its tile (the footprint within it)",
                    Some(_) => "routing: an obstacle's object natively NOT on its tile",
                };
                counts.note(what);
                if show_obstacles {
                    let o = world.find_object(id);
                    println!(
                        "   obstacle {:?} cost {} core {}x{} at +{},+{} of tile {},{}: object {id} {} native at {:?} defn version {:?} f75 {:04x}",
                        &r[..4],
                        r[4],
                        c[2] - c[0],
                        c[3] - c[1],
                        c[0] & 15,
                        c[1] & 15,
                        c[0] >> 4,
                        c[1] >> 4,
                        o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                        o.map(|o| (o.x16, o.y16, o.level, o.facing())),
                        o.and_then(|o| world.definition(o)).map(|d| (d.version, d.fields.len())),
                        o.and_then(|o| world.definition(o)).map_or(0, |d| d.field(75))
                    );
                }
            }
        }
        // The obstacles natively, in the loop's order (the objects it asked about, as logged):
        // each not a person by `routing::obstacle`, a person as the log keeps it (6 on a side
        // round its logged place, grown); against the logged list where it has as many (not
        // dropping one that overlaps the Sim: not yet native), rectangle by rectangle.
        if !kept.is_empty() {
            // the Sim that plans: the person nearest the plan's start (its place as logged, within
            // a tile); its footprint 6 on a side round its place
            let start = pl.points.first().copied();
            let sim_rect = start.and_then(|s| {
                world
                    .objects()
                    .filter(|o| o.person)
                    .filter_map(|o| place_at(o.id(), pl.tick))
                    .map(|p| ((p.0 - s.0).pow(2) + (p.1 - s.1).pow(2), p))
                    .filter(|&(d, _)| d <= 256)
                    .min()
                    .map(|(_, p)| [p.0 - 3, p.1 - 3, p.0 + 3, p.1 + 3])
            });
            let native: Vec<[i32; 5]> = pl
                .tested
                .iter()
                .filter_map(|&(id, out, _)| {
                    // an object not in the native world holds its place (as the log keeps it)
                    let Some(o) = world.find_object(id) else { return (!out).then_some([0; 5]) };
                    if o.person {
                        // its place as the planner sees it, mid-tick: the one logged before the
                        // plan's tick ('P' is written as a tick starts; the club's tick 56)
                        let p = log.places.iter().rfind(|q| q.id == id && q.tick < pl.tick).map(|q| (q.x, q.y)).unwrap_or((o.x16, o.y16));
                        return (!out).then_some([p.0 - 3 - half, p.1 - 3 - half, p.0 + 3 + half, p.1 + 3 + half, BLOCKS]);
                    }
                    // an object replay's world holds without its definition (one the engine made,
                    // taken from the log's list: the club's smoke-puff effect) has no footprint
                    // here: none, as the original's loop left it out
                    world.definition(o)?;
                    obstacle(o, world.definition(o).map_or(0, |d| d.field(75)), world.definition(o).map_or(0, |d| d.field(65)), half)
                })
                // one whose footprint (before it is grown) overlaps the Sim's is dropped
                .filter(|r| *r == [0; 5] || !sim_rect.is_some_and(|s| overlaps(&ungrown(r, half), &s)))
                .collect();
            // an object not in the native world ([0; 5]) holds its place, unless the planner made
            // no rectangle for it (the next native one is the planner's here): then it is dropped
            let native: Vec<[i32; 5]> = {
                let (mut kept_rects, mut j) = (Vec::new(), 0);
                for (i, r) in native.iter().enumerate() {
                    if *r == [0; 5] && native.len() > pl.obstacles.len() && native.get(i + 1).is_some_and(|n| pl.obstacles.get(j) == Some(n)) {
                        continue;
                    }
                    kept_rects.push(*r);
                    j += 1;
                }
                kept_rects
            };
            let hit = native.iter().zip(&pl.obstacles).filter(|(a, b)| a == b).count();
            let unknown = native.iter().filter(|a| **a == [0; 5]).count();
            let miss = native.len().min(pl.obstacles.len()) - hit - unknown;
            if show_obstacles {
                for (k, (a, b)) in native.iter().zip(&pl.obstacles).enumerate().filter(|(_, (a, b))| a != b && **a != [0; 5]) {
                    println!("      rectangle {k} of tick {}: native {a:?}, the planner's {b:?}, object {:?}", pl.tick, pl.tested.iter().filter(|t| !t.1).nth(k).map(|t| t.0));
                }
                for (a, b) in native.iter().zip(&pl.obstacles).filter(|(a, b)| a != b && **a != [0; 5]) {
                    println!("   native obstacle {a:?}, the planner's {b:?}");
                }
            }
            // A person among the obstacles natively: left out when it is what the plan goes to
            // (the stack object of the tick's routing step that walks: opcode 27 or 45, result
            // 1) or more than 3 tiles from the planning Sim (the constant at 802CFAAC, 3.0; the
            // logged places, the walk not native); against the loop's answers.
            let targets: Vec<i16> = log
                .steps
                .iter()
                .filter(|s| s.tick == pl.tick && s.result == 1)
                .filter(|s| {
                    log.tree_of(s).is_some_and(|t| {
                        let at = s.node as usize * 12;
                        t.tree == s.tree && at + 12 <= t.raw.len() && matches!(u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, 27 | 45)
                    })
                })
                .map(|s| s.stack_object)
                .collect();
            if let Some(sr) = sim_rect {
                let c = (sr[0] + 3, sr[1] + 3);
                // only a person's own part (104 bytes before its listed object, in the log): an
                // entry the observer files under a person by nearness alone (the paper girl's
                // other object, 356 past hers, another class) is no person
                for &(id, out, _) in pl.tested.iter().filter(|t| t.2 == PERSON_PART_OFFSET) {
                    let Some(p) = world.find_object(id).filter(|o| o.person).and_then(|_| place_at(id, pl.tick)) else { continue };
                    let d2 = (p.0 - c.0).pow(2) + (p.1 - c.1).pow(2);
                    let native_out = targets.contains(&id) || d2 >= 48 * 48;
                    counts.note(if native_out == out {
                        "routing: a person natively kept or left out as the loop did (its target, or 3 tiles off)"
                    } else {
                        "routing: a person natively NOT kept or left out as the loop did"
                    });
                    if show_obstacles && native_out != out {
                        println!("   person {id} at {p:?}, {:.1} from the Sim: the loop {}, natively {}; targets {targets:?}", (d2 as f64).sqrt(), if out { "left out" } else { "kept" }, if native_out { "left out" } else { "kept" });
                    }
                }
            }
            // study: each person the loop asked about, its distance from the planning Sim and
            // whether it was kept
            if show_obstacles {
                for &(id, out, _) in pl.tested.iter().filter(|t| t.2 == PERSON_PART_OFFSET) {
                    if let Some(o) = world.find_object(id).filter(|o| o.person)
                        && let (Some(p), Some(s)) = (place_at(id, pl.tick), sim_rect)
                    {
                        let c = (s[0] + 3, s[1] + 3);
                        let d = (((p.0 - c.0).pow(2) + (p.1 - c.1).pow(2)) as f64).sqrt();
                        println!("   person {id} ({}) at {p:?}, {d:.1} from the Sim at {c:?} (start {:?}, goal {:?}): {}", o.id(), pl.points.first(), pl.points.last(), if out { "left out" } else { "kept" });
                    }
                }
            }
            if show_obstacles {
                println!("   native list tick {}: {} rects, the planner's {}, kept {}, the Sim's {sim_rect:?}, {hit} the same", pl.tick, native.len(), pl.obstacles.len(), kept.len());
                if native.len() != pl.obstacles.len() {
                    let ids: Vec<String> = pl
                        .tested
                        .iter()
                        .map(|&(id, out, off)| {
                            let o = world.find_object(id);
                            let nat = o.map(|o| {
                                if o.person { "P".to_string() } else { format!("f{:x}c{}", o.data.get(8).copied().unwrap_or(0), o.data.get(2).copied().unwrap_or(0)) }
                            });
                            format!("{id}{}{}{}", if out { "-" } else { "+" }, if off == PERSON_PART_OFFSET { "p" } else { "" }, nat.unwrap_or("?".into()))
                        })
                        .collect();
                    println!("      tested (id, + kept / - left out, native flags and container): {}", ids.join(" "));
                }
                let only_native: Vec<_> = native.iter().filter(|r| !pl.obstacles.contains(r)).collect();
                let only_theirs: Vec<_> = pl.obstacles.iter().filter(|r| !native.contains(r)).collect();
                if !only_native.is_empty() || !only_theirs.is_empty() {
                    println!("   tick {}: only natively {only_native:?}; only the planner's {only_theirs:?}", pl.tick);
                }
            }
            counts.add("routing: obstacles of objects not in the native world (the log's kept)", unknown);
            // The plan's waypoints between its ends, by what they are: a corner of one of the
            // native obstacles (as the planner has them where natively unknown), or not
            let corners: Vec<(i32, i32)> = native
                .iter()
                .zip(&pl.obstacles)
                .map(|(a, b)| if *a == [0; 5] { b } else { a })
                .flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])])
                .collect();
            // The plan within one room natively (`corner_path`) against the planner's: the corners
            // of the obstacles it passes, in order (its other points apart), to the place it
            // walks to (before the object it then faces).
            let obstacles_here: Vec<[i32; 5]> = native.iter().zip(&pl.obstacles).map(|(a, b)| if *a == [0; 5] { *b } else { *a }).collect();
            if pl.points.len() >= 2 && native.len() == pl.obstacles.len() {
                let start = pl.points[0];
                let end = *pl.points.last().unwrap();
                let in_blocking = |p: (i32, i32)| obstacles_here.iter().any(|r| r[4] == BLOCKS && p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3]);
                let goal = if in_blocking(end) && pl.points.len() >= 3 { pl.points[pl.points.len() - 2] } else { end };
                let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
                let same_room = room(start) == room(goal);
                {
                    // weighed by the paved regions natively from the floor (`OFF_PAVED_WEIGHT`, a
                    // fitted weight; study HF_PAVED_WEIGHT to try another)
                    let weight: f64 = env("HF_PAVED_WEIGHT").and_then(|v| v.parse().ok()).unwrap_or(OFF_PAVED_WEIGHT);
                    let path = weighed_path(start, goal, &obstacles_here, walls, half, &walls.paved_regions(), weight);
                    let is_corner = |p: &(i32, i32)| obstacles_here.iter().filter(|r| r[4] == BLOCKS).any(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])].contains(p));
                    let theirs: Vec<(i32, i32)> = pl.points.iter().copied().filter(is_corner).collect();
                    let ours: Vec<(i32, i32)> = path.as_ref().map_or(Vec::new(), |p| p.iter().copied().filter(is_corner).collect());
                    counts.note(match (same_room, path.is_some() && ours == theirs) {
                        (true, true) => "routing: a plan within a room natively by the obstacles' corners as the planner's",
                        (true, false) => "routing: a plan within a room natively NOT by the planner's corners",
                        (false, true) => "routing: a plan through a door natively by the obstacles' corners as the planner's",
                        (false, false) => "routing: a plan through a door natively NOT by the planner's corners",
                    });
                    if show_obstacles {
                        println!("   corner path tick {}: native {path:?}; the planner's corners {theirs:?}, points {:?}", pl.tick, pl.points);
                    }
                    // against the planner's own coarse way through its map (logs with 'I')
                    if !pl.coarse.is_empty() {
                        let same = path.as_ref().is_some_and(|p| *p == pl.coarse);
                        counts.note(if same {
                            "routing: a plan's coarse way (the search's, 'I') natively the corner path, point by point"
                        } else {
                            "routing: a plan's coarse way (the search's, 'I') NOT the native corner path"
                        });
                        // study: each coarse point moved to the nearest corner of a logged
                        // obstacle (any cost) within 12, against the plan's points
                        if has("REPLAY_SHOW_COARSE") && !counts.quiet {
                            let corners: Vec<(i32, i32)> = pl.obstacles.iter().flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])]).collect();
                            let snapped: Vec<String> = pl.coarse.iter().map(|&p| {
                                let near = corners.iter().min_by_key(|c| (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2)).copied();
                                match near {
                                    Some(c) if (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2) <= 144 => format!("{p:?}->{c:?}{}", if pl.points.contains(&c) { "+" } else { "" }),
                                    _ => format!("{p:?}{}", if pl.points.contains(&p) { "+" } else { "" }),
                                }
                            }).collect();
                            println!("snap tick {}: {}", pl.tick, snapped.join(" "));
                            // and against the search's own rectangles (logs with 'D'): the nearest
                            // of their corners to each coarse point
                            let dc: Vec<(i32, i32)> = pl.regions.iter().flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])]).collect();
                            let near: Vec<String> = pl.coarse.iter().map(|&p| {
                                dc.iter().min_by_key(|c| (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2)).map_or(format!("{p:?}"), |c| format!("{p:?}~{c:?}"))
                            }).collect();
                            println!("regions tick {} ({} rects): {}", pl.tick, pl.regions.len(), near.join(" "));
                        }
                        if has("REPLAY_SHOW_COARSE") && !counts.quiet {
                            println!("coarse tick {}: the search's {:?}; native {path:?}; the plan {:?}", pl.tick, pl.coarse, pl.points);
                        }
                    }
                }
            }
            // Each leg of the plan keeps out of the blocking obstacles' insides (natively)
            // (but the last into the object it ends at: the Sim turns to face it there)
            for (i, leg) in pl.points.windows(2).enumerate() {
                let (a, b) = (leg[0], leg[1]);
                let through = native.iter().filter(|r| r[4] == BLOCKS).find(|r| segment_enters(a, b, &[r[0], r[1], r[2], r[3]]));
                let last = i + 2 == pl.points.len();
                counts.note(match through {
                    None => "routing: a plan's leg clear of the native blocking obstacles",
                    Some(r) if last && b.0 > r[0] && b.0 < r[2] && b.1 > r[1] && b.1 < r[3] => {
                        "routing: a plan's last leg into the obstacle it ends in (the object faced)"
                    }
                    Some(_) => "routing: a plan's leg through a native blocking obstacle",
                });
                if show_obstacles && let Some(r) = through {
                    println!("   leg of tick {} {a:?} to {b:?} through {r:?}", pl.tick);
                }
            }
            let goals: Vec<(i32, i32)> = pl.goals.iter().map(|g| (g[0] as i32, g[1] as i32)).collect();
            for (i, w) in pl.points.iter().enumerate().skip(1).take(pl.points.len().saturating_sub(2)) {
                let corner = corners.contains(w);
                let (prev, next) = (pl.points[i - 1], pl.points[i + 1]);
                // on the line between its neighbours (to within a sixteenth)
                let cross = (w.0 - prev.0) as i64 * (next.1 - prev.1) as i64 - (w.1 - prev.1) as i64 * (next.0 - prev.0) as i64;
                let len = (((next.0 - prev.0).pow(2) + (next.1 - prev.1).pow(2)) as f64).sqrt();
                let between = len > 0.0 && (cross.abs() as f64) / len <= 1.0;
                let tile_centre = w.0 & 15 == 8 && w.1 & 15 == 8;
                counts.note(if corner {
                    "routing: a waypoint (not an end) a corner of an obstacle"
                } else if goals.contains(w) {
                    "routing: a waypoint (not an end) the request's goal"
                } else if between {
                    "routing: a waypoint (not an end) on the line between its neighbours"
                } else if tile_centre {
                    "routing: a waypoint (not an end) a tile's centre (a door's sides)"
                } else {
                    "routing: a waypoint (not an end) none of these"
                });
                let corner = corner || goals.contains(w) || between || tile_centre;
                if show_obstacles && !corner {
                    let prev = pl.points[i - 1];
                    let next = pl.points.get(i + 1).copied().unwrap_or(*w);
                    println!("   waypoint {i} of tick {} {w:?} (from {prev:?} to {next:?}): not a corner", pl.tick);
                }
            }
            counts.add("routing: obstacles natively the planner's, rectangle by rectangle", hit);
            counts.add("routing: obstacles natively NOT the planner's", miss + native.len().abs_diff(pl.obstacles.len()));
            counts.note(if native.len() == pl.obstacles.len() && hit + unknown == native.len() {
                "routing: a plan's obstacles natively all the planner's (but those of objects not native)"
            } else {
                "routing: a plan's obstacles natively NOT all the planner's (an object not native, or a footprint)"
            });
        }
        // study (REPLAY_DUMP_TESTED): each object the loop asked about, its label (left out, or
        // kept with its rectangle's cost) and its definition's fields, for the rules' search
        if has("REPLAY_DUMP_TESTED") && !kept.is_empty() && kept.len() == pl.obstacles.len() {
            let mut costs = std::collections::BTreeMap::new();
            let mut k = 0;
            for &(id, out, _) in &pl.tested {
                if out {
                    continue;
                }
                if let Some(r) = pl.obstacles.get(k) {
                    costs.entry(id).or_insert(r[4]);
                }
                k += 1;
            }
            for &(id, out, _) in &pl.tested {
                let Some(o) = world.find_object(id) else { continue };
                let label = if out { "out".to_string() } else { costs.get(&id).map_or("kept?".to_string(), |c| format!("cost{c}")) };
                let fields = world.definition(o).map(|d| d.fields.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default();
                let data = o.data.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                println!("tested\t{}\t{id}\t{}\t{label}\t{}\t{fields}\t{data}", pl.tick, o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.person as u8);
            }
        }
        // the native objects within the obstacles' extent that none covers
        let (x0, y0) = pl.obstacles.iter().fold((i32::MAX, i32::MAX), |a, r| (a.0.min(r[0]), a.1.min(r[1])));
        let (x1, y1) = pl.obstacles.iter().fold((i32::MIN, i32::MIN), |a, r| (a.0.max(r[2]), a.1.max(r[3])));
        for o in world.objects().filter(solid).filter(|o| o.x16 >= x0 && o.x16 < x1 && o.y16 >= y0 && o.y16 < y1) {
            let covered = pl.obstacles.iter().any(|r| inside(o, r));
            counts.note(if covered {
                "routing: a native object in the obstacles' extent covered by one"
            } else {
                "routing: a native object in the obstacles' extent covered by none"
            });
            if show_obstacles && !covered {
                println!("   none for {} {} at {},{}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.x16, o.y16);
            }
        }
    }
    // The walk (study: REPLAY_SHOW_WALKS): the planning Sim's logged places from its plan's tick
    // to its next plan (or 600 ticks), each against the plan's line: its distance from it and
    // how far along it (sixteenths).
    let show_walks = has("REPLAY_SHOW_WALKS") && !counts.quiet;
    for (k, pl) in log.plans.iter().enumerate() {
        let Some(&start) = pl.points.first() else { continue };
        let Some(sim) = world
            .objects()
            .filter(|o| o.person)
            .filter_map(|o| place_at(o.id(), pl.tick).map(|p| ((p.0 - start.0).pow(2) + (p.1 - start.1).pow(2), o.id())))
            .filter(|&(d, _)| d <= 256)
            .min()
            .map(|(_, id)| id)
        else {
            continue;
        };
        let until = log.plans[k + 1..].iter().find(|q| q.points.first().is_some_and(|&a| place_at(sim, q.tick) == Some(a))).map_or(pl.tick + 600, |q| q.tick);
        // the line's legs and their lengths
        let legs: Vec<((f64, f64), (f64, f64))> = pl.points.windows(2).map(|w| ((w[0].0 as f64, w[0].1 as f64), (w[1].0 as f64, w[1].1 as f64))).collect();
        let along = |p: (i32, i32)| -> (f64, f64) {
            // (distance from the line, length along it to the nearest point)
            let (mut best, mut at, mut before) = (f64::INFINITY, 0.0, 0.0);
            for &(a, b) in &legs {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = (dx * dx + dy * dy).sqrt();
                let t = if len == 0.0 { 0.0 } else { (((p.0 as f64 - a.0) * dx + (p.1 as f64 - a.1) * dy) / (len * len)).clamp(0.0, 1.0) };
                let q = (a.0 + t * dx, a.1 + t * dy);
                let d = ((p.0 as f64 - q.0).powi(2) + (p.1 as f64 - q.1).powi(2)).sqrt();
                if d < best {
                    best = d;
                    at = before + t * len;
                }
                before += len;
            }
            (best, at)
        };
        let mut last: Option<f64> = None;
        let mut line = String::new();
        for r in log.places.iter().filter(|r| r.id == sim && r.tick > pl.tick && r.tick <= until) {
            let (d, s) = along((r.x, r.y));
            counts.note(if d <= 1.5 {
                "routing: a walking Sim's place on its plan's line (within a sixteenth and a half)"
            } else {
                "routing: a walking Sim's place NOT on its plan's line"
            });
            if show_walks {
                line += &format!(" {}:{:.1}{}", r.tick, last.map_or(s, |l| s - l), if d > 1.5 { format!("(off {d:.1})") } else { String::new() });
            }
            last = Some(s);
        }
        if show_walks {
            println!("walk of {sim} on the plan of tick {} ({} points) to tick {until}:{line}", pl.tick, pl.points.len());
            // its speed: the length along the line over the ticks, from its first move to its
            // last (sixteenths a tick)
            let moves: Vec<(u32, f64)> = log.places.iter().filter(|r| r.id == sim && r.tick > pl.tick && r.tick <= until).map(|r| (r.tick, along((r.x, r.y)).1)).collect();
            if let (Some(a), Some(b)) = (moves.first(), moves.last())
                && b.0 > a.0
            {
                println!("   speed {:.3} a tick over {} ticks ({:.1} sixteenths), first move at tick {} ({} after the plan)", (b.1 - a.1) / (b.0 - a.0) as f64, b.0 - a.0, b.1 - a.1, a.0, a.0 - pl.tick);
                let mid: Vec<&(u32, f64)> = moves.iter().filter(|m| m.0 >= a.0 + 10 && m.0 + 10 <= b.0).collect();
                if let (Some(c), Some(d)) = (mid.first(), mid.last())
                    && d.0 > c.0 + 20
                {
                    println!("   cruising {:.4} a tick over {} ticks", (d.1 - c.1) / (d.0 - c.0) as f64, d.0 - c.0);
                }
            }
        }
    }
    // study (REPLAY_SHOW_PERSONS): the persons' data, for the child's
    if has("REPLAY_SHOW_PERSONS") {
        for o in world.objects().filter(|o| o.person) {
            println!("person {} {}: {:?}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.person_data);
        }
    }
    // The walker's steps natively (`walk_step`), each from its logged place along the line it
    // walks (the latest plan whose line it is on, less the last leg into a faced object when the
    // plan has one), against the logged place after it. Its top and growth from the DOL's
    // constants (`walk_rates`); its largest logged step only tells an adult from a child.
    let mut speed: BTreeMap<u32, f32> = BTreeMap::new();
    let mut tops: BTreeMap<u32, f32> = BTreeMap::new();
    // which person a follower walks: the one whose logged place at a tick it moves in is its own
    // (to a sixteenth); its top natively from its file's body strings (`walk_top`)
    let mut walker_of: BTreeMap<u32, i16> = BTreeMap::new();
    {
        let mut at = 0;
        for f in &log.follows {
            if walker_of.contains_key(&f.follower) || (f.before.x, f.before.y) == (f.after.x, f.after.y) {
                continue;
            }
            while at < log.places.len() && log.places[at].tick < f.tick {
                at += 1;
            }
            let here = log.places[at..].iter().take_while(|p| p.tick == f.tick);
            let found: Vec<i16> = here.filter(|p| ((p.x as f32) - f.after.x).abs() < 1.0 && ((p.y as f32) - f.after.y).abs() < 1.0).map(|p| p.id).collect();
            if let [id] = found[..] {
                walker_of.insert(f.follower, id);
            }
        }
    }
    let native_top: BTreeMap<u32, f32> = walker_of
        .iter()
        .filter_map(|(&fo, &id)| {
            let o = loaded.objects().find(|o| o.id() == id)?;
            let age = o.file.and_then(|fi| lib.file(fi).strings.get(&200)).and_then(|t| t.first()).map(String::as_str);
            Some((fo, walk_top(age)))
        })
        .collect();
    for f in log.follows.iter().filter(|f| f.before.factor == 1.0) {
        let d = ((f.after.x - f.before.x).powi(2) + (f.after.y - f.before.y).powi(2)).sqrt();
        let t = tops.entry(f.follower).or_insert(0.0);
        *t = t.max(d);
    }
    // checked: each walker's native top against the logged steps' (its largest step at normal
    // speed nearest the adult's or the child's)
    for (fo, &seen) in &tops {
        let logged = [WALK_TOP_ADULT, WALK_TOP_CHILD].into_iter().min_by(|a, b| (a * WALK_FRAME - seen).abs().total_cmp(&(b * WALK_FRAME - seen).abs())).unwrap();
        counts.note(match native_top.get(fo) {
            Some(&n) if n == logged => "routing: a walker's top natively the original's (its body's age, STR# 200)",
            Some(_) => "routing: a walker's top natively NOT the original's",
            None => "routing: a walker whose person is not found (its top not checked)",
        });
    }
    let mut plan_at = 0;
    // the walk run free: a native place a follower, set at its walk's start (state 1) and then
    // moved only by the native steps (with its own native speed), against the logged place after
    // each frame that moves
    let mut free: BTreeMap<u32, ((f32, f32), f32)> = BTreeMap::new();
    // the native speed each walk starts from (by follower and tick), for the whole walks below
    let mut start_speed: BTreeMap<(u32, u32), f32> = BTreeMap::new();
    // Natively, whether a walk runs on: the walker's object data 9 (Route_Follow's test, fn_801F5F4C)
    // as its scripts set it (an expression "my object data 9 := literal": scope 3, data 9, op 5,
    // its value from the node's operands) since its last routing primitive ended (GotoRoutingSlot
    // or GotoRelativePosition returning: the primitive clears it, Route_Run at 80220918).
    let mut data9: BTreeMap<i16, Vec<(u32, i16)>> = BTreeMap::new();
    for st in &log.steps {
        let Some(t) = log.tree_of(st) else { continue };
        let at = st.node as usize * 12;
        if at + 12 > t.raw.len() {
            continue;
        }
        let raw = &t.raw[at..at + 12];
        let op = u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF;
        let Some(&who) = ids.get(&st.sim) else { continue };
        let be = |i: usize| i16::from_be_bytes([raw[i], raw[i + 1]]);
        if op == 2 && be(4) == 9 && raw[9] == 5 && raw[10] == 3 && raw[11] == 7 {
            data9.entry(who).or_default().push((st.tick, be(6)));
        } else if matches!(op, 27 | 45) && st.result != 1 {
            data9.entry(who).or_default().push((st.tick, 0));
        }
    }
    let data9_at = |who: i16, tick: u32| data9.get(&who).and_then(|v| v.iter().rfind(|e| e.0 <= tick)).is_some_and(|e| e.1 != 0);
    // for each walker frame, whether its walk is followed by state 5 (the first frame from state
    // 4 after it going to 5): found backwards through the log
    let mut runs_on = vec![false; log.follows.len()];
    {
        let mut next4: BTreeMap<u32, u32> = BTreeMap::new();
        for (i, f) in log.follows.iter().enumerate().rev() {
            if f.before.state == 4 {
                next4.insert(f.follower, f.after.state);
            }
            runs_on[i] = next4.get(&f.follower) == Some(&5);
        }
    }
    // checked: at each walk's end (state 2 to 3), the native data 9 against the log's run on
    for (i, f) in log.follows.iter().enumerate().filter(|(_, f)| f.before.state == 2 && f.after.state == 3) {
        let Some(&who) = walker_of.get(&f.follower) else {
            counts.note("routing: a walk's end whose walker is not found (its run on not checked)");
            continue;
        };
        counts.note(if data9_at(who, f.tick) == runs_on[i] {
            "routing: a walk's run on natively the original's (its walker's object data 9 as its scripts set it)"
        } else {
            "routing: a walk's run on natively NOT the original's"
        });
    }
    // state 5's point natively (`run_on_point`), from the latest plan whose last waypoint but
    // one is where the walker enters it, against the follower's +116/+120 after
    for f in log.follows.iter().filter(|f| f.before.state == 4 && f.after.state == 5) {
        let at = (f.before.x, f.before.y);
        let plan = log.plans.iter().rev().filter(|p| p.tick <= f.tick && p.points.len() >= 2).find(|p| {
            let q = p.points[p.points.len() - 2];
            (q.0 as f32 - at.0).abs() < 0.5 && (q.1 as f32 - at.1).abs() < 0.5
        });
        let Some(p) = plan else {
            counts.note("routing: a walker's state 5 with no plan ending there (not checked)");
            continue;
        };
        let (q, l) = (p.points[p.points.len() - 2], p.points[p.points.len() - 1]);
        let nat = run_on_point((q.0 as f32, q.1 as f32), (l.0 as f32, l.1 as f32), f.before.mode == 1);
        let got = f.after.point;
        counts.note(if (nat.0 - got.0).abs() < 0.01 && (nat.1 - got.1).abs() < 0.01 {
            "routing: a walker's state-5 point natively the original's (towards the plan's last waypoint, 3 or 9 at most)"
        } else {
            "routing: a walker's state-5 point natively NOT the original's"
        });
        if show_walks {
            println!("state 5 at tick {} {:x}: at {at:?}, plan {:?}, native {nat:?}, the original's {got:?}, +132 {}", f.tick, f.follower, p.points, f.before.mode);
        }
    }
    for f in &log.follows {
        while plan_at < log.plans.len() && log.plans[plan_at].tick <= f.tick {
            plan_at += 1;
        }
        let (b, a) = (f.before, f.after);
        if b.state == 1 {
            // its speed the native one the follower's last walk left (step by step)
            let v = speed.get(&f.follower).copied().unwrap_or(0.0);
            free.insert(f.follower, ((b.x, b.y), v));
            start_speed.insert((f.follower, f.tick), v);
        }
        // (a walk starts at the speed the follower's last one stopped at: its +144 is not reset)
        if (b.x, b.y) == (a.x, a.y) {
            continue;
        }
        // a frame of state 5 (the move on to the follower's +116/+120 after a walk): natively the
        // walk's own step (`walk_step`) towards its point, braking for it unless its object data 9
        // is still set; its speed the one the next walk starts from
        if b.state == 5 || a.state == 5 {
            let step = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            let carried = speed.get(&f.follower).copied().unwrap_or(0.0);
            let left = ((a.point.0 - b.x).powi(2) + (a.point.1 - b.y).powi(2)).sqrt();
            let Some(&per_second) = native_top.get(&f.follower) else { continue };
            let (top, growth) = walk_rates(per_second);
            let runs_on = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
            let (v, s) = walk_step(carried, top, growth, b.factor, if runs_on { f32::INFINITY } else { left });
            let native = s.min(left);
            counts.note(if (native - step).abs() < 0.001 {
                "routing: a walker's state-5 step natively the original's (the walk's step to its point)"
            } else {
                "routing: a walker's state-5 step natively NOT the original's"
            });
            if show_walks && (native - step).abs() >= 0.001 {
                println!("state 5 step tick {} {:x}: native {native:.4} (carried {carried:.4}, left {left:.3}, runs on {runs_on}), the original's {step:.4}", f.tick, f.follower);
            }
            speed.insert(f.follower, v);
            continue;
        }
        // the walker's top a second: the DOL's adult or child constant (which: the one nearest
        // its logged full step; the original's test, through the follower's +8, not yet read)
        let Some(&per_second) = native_top.get(&f.follower) else { continue };
        let (top, growth) = walk_rates(per_second);
        // the line it is on
        let runs_on_now = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
        let found = log.plans[..plan_at].iter().rev().take(6).find_map(|pl| {
            let pts: Vec<(f32, f32)> = pl.points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
            // the walk ends at the request's chosen goal: a plan's last leg past it, into what the
            // Sim then faces in the same room, is not walked (one through a door is)
            let goal = usize::try_from(pl.chosen).ok().and_then(|i| pl.goals.get(i)).map(|g| (g[0] as i32, g[1] as i32));
            let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
            let last = *pl.points.last().unwrap_or(&(0, 0));
            let faced = pl.points.len() >= 3 && goal.is_some_and(|g| pl.points[pl.points.len() - 2] == g && room(last) == room(g));
            // a walk that runs on (its walker's data 9 set) ends at the plan's last point but one
            // too: state 5 goes on from there towards the last
            let walk = if pts.len() >= 3 && (faced || pl.obstacles.is_empty() || runs_on_now) { &pts[..pts.len() - 1] } else { &pts[..] };
            let mut best: Option<(f32, usize, f32)> = None;
            for i in 0..walk.len().saturating_sub(1) {
                let (p, q) = (walk[i], walk[i + 1]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 0.0 { (((b.x - p.0) * dx + (b.y - p.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = ((b.x - p.0 - t * dx).powi(2) + (b.y - p.1 - t * dy).powi(2)).sqrt();
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, i, t));
                }
            }
            best.filter(|x| x.0 < 0.05).map(|(_, i, t)| (walk.to_vec(), i, t))
        });
        // the walk runs on without braking when its walker goes straight on into state 5 (the
        // move to its +116/+120): when its object data 9 is set, natively from its scripts
        let on_past = runs_on_now;
        let Some((walk, i, t)) = found else {
            counts.note("routing: a walker's step not on a logged plan's line (set aside)");
            continue;
        };
        let leg = |j: usize| ((walk[j + 1].0 - walk[j].0).powi(2) + (walk[j + 1].1 - walk[j].1).powi(2)).sqrt();
        let left = (i..walk.len() - 1).map(leg).sum::<f32>() - t * leg(i);
        // no brake when it runs on (the step still ends at the walk's end)
        let brake_left = if on_past { f32::INFINITY } else { left };
        let (v, s) = walk_step(speed.get(&f.follower).copied().unwrap_or(0.0), top, growth, b.factor, brake_left);
        let s = s.min(left);
        speed.insert(f.follower, v);
        let at = along_line(&walk, i, t, s);
        // the free-running place: on the same line, from where the native walk has it
        if let Some(((fx, fy), fv)) = free.get(&f.follower).copied() {
            let loc = (0..walk.len().saturating_sub(1))
                .map(|j| {
                    let (p, q) = (walk[j], walk[j + 1]);
                    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                    let l2 = dx * dx + dy * dy;
                    let t = if l2 > 0.0 { (((fx - p.0) * dx + (fy - p.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    (((fx - p.0 - t * dx).powi(2) + (fy - p.1 - t * dy).powi(2)).sqrt(), j, t)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((d, j, t)) = loc.filter(|l| l.0 < 0.05) {
                let _ = d;
                let fleft = (j..walk.len() - 1).map(leg).sum::<f32>() - t * leg(j);
                let (nv, ns) = walk_step(fv, top, growth, b.factor, if on_past { f32::INFINITY } else { fleft });
                let ns = ns.min(fleft);
                let nat = along_line(&walk, j, t, ns);
                free.insert(f.follower, (nat, nv));
                let off = ((nat.0 - a.x).powi(2) + (nat.1 - a.y).powi(2)).sqrt();
                if show_walks && off >= 0.01 && ((fx - b.x).powi(2) + (fy - b.y).powi(2)).sqrt() < 0.01 {
                    println!("free walk parts tick {} {:x} state {}->{} factor {}: native {nat:?} speed {nv:.4} (was {fv:.4}), the original's ({:.3},{:.3}) step {:.4}", f.tick, f.follower, b.state, a.state, b.factor, a.x, a.y, ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt());
                }
                counts.note(if off < 0.01 {
                    "routing: a walk run free natively at the original's place (to a hundredth of a sixteenth)"
                } else {
                    "routing: a walk run free natively NOT at the original's place"
                });
            }
        }
        let off = ((at.0 - a.x).powi(2) + (at.1 - a.y).powi(2)).sqrt();
        if show_walks && off >= 0.003 {
            println!(
                "walk miss runs on {on_past} tick {} {:x} state {}->{} factor {} at ({:.3},{:.3}) to ({:.3},{:.3}): native ({:.3},{:.3}) step {s:.4} speed {v:.4} left {left:.3} leg {i}/{}",
                f.tick, f.follower, b.state, a.state, b.factor, b.x, b.y, a.x, a.y, at.0, at.1, walk.len() - 1
            );
        }
        counts.note(if off < 0.003 {
            "routing: a walker's step natively the original's (to a thousandth of a sixteenth)"
        } else {
            "routing: a walker's step natively NOT the original's"
        });
    }
    let mut native_frame_ticks: Vec<u32> = Vec::new();
    // Frames and ticks (`FrameClock`): the frames each tick holds natively against a walker's
    // frames in each tick of the log, from the first tick it has any; the clock's phase there is
    // not known (the log starts in play): the best of a thousandth's steps
    {
        let mut per: BTreeMap<u32, u32> = BTreeMap::new();
        // the walker with the most frames
        let mut by: BTreeMap<u32, usize> = BTreeMap::new();
        for f in &log.follows {
            *by.entry(f.follower).or_default() += 1;
        }
        let first_follower = by.iter().max_by_key(|e| e.1).map(|e| *e.0);
        for f in log.follows.iter().filter(|f| Some(f.follower) == first_follower) {
            *per.entry(f.tick).or_default() += 1;
        }
        // the game's speed each tick (the walkers' factor as logged, an input): the clock's ticks
        // run that much faster
        let mut speed_at: BTreeMap<u32, f32> = BTreeMap::new();
        for f in &log.follows {
            speed_at.insert(f.tick, f.before.factor);
        }
        let speed_from = |t: u32| speed_at.range(..=t).next_back().map_or(1.0, |e| *e.1);
        if let (Some((&first, _)), Some((&last, _))) = (per.first_key_value(), per.last_key_value()) {
            let best = (0..42)
                .map(|ph| {
                    let mut clock = FrameClock { time: ph as f32 / 1000.0 };
                    let n = (first..=last)
                        .filter(|&t| {
                            let frames = clock.frames_to_tick_at(speed_from(t));
                            per.get(&t) == Some(&frames)
                        })
                        .count();
                    (n, ph)
                })
                .max()
                .unwrap_or_default();
            counts.add("routing: ticks a walker moves in", per.len());
            counts.add("routing: ticks whose walker frames are natively the original's (the frame clock)", best.0);
            // the tick of each frame natively, from the first (the clock at its phase)
            let mut clock = FrameClock { time: best.1 as f32 / 1000.0 };
            for t in first..=last + 200 {
                for _ in 0..clock.frames_to_tick_at(speed_from(t)) {
                    native_frame_ticks.push(t);
                }
            }
        }
    }
    // The routing primitives' runs (opcode 27 or 45): a Sim's step at one tree and node that
    // yields (result 1) tick after tick, then returns: (sim, tree, node, first tick, return tick,
    // its result)
    let routing_runs: Vec<(u32, u16, u16, u32, u32, u8)> = {
        let mut open: BTreeMap<(u32, u16, u16), u32> = BTreeMap::new();
        let mut out = Vec::new();
        for s in &log.steps {
            let Some(t) = log.tree_of(s) else { continue };
            let at = s.node as usize * 12;
            if t.tree != s.tree || at + 12 > t.raw.len() || !matches!(u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, 27 | 45) {
                continue;
            }
            let key = (s.sim, s.tree, s.node);
            if s.result == 1 {
                open.entry(key).or_insert(s.tick);
            } else if let Some(first) = open.remove(&key) {
                out.push((s.sim, s.tree, s.node, first, s.tick, s.result));
            }
        }
        out
    };
    if show_walks {
        for r in &routing_runs {
            println!("routing run: sim {:x} tree {} node {} yields from tick {} to its return at {} (result {}) ({} steps)", r.0, r.1, r.2, r.3, r.4, r.5,
                log.steps.iter().filter(|st| (st.sim, st.tree, st.node) == (r.0, r.1, r.2) && st.tick >= r.3 && st.tick <= r.4).count());
        }
    }
    // A routing primitive's return: after its walker is done (state 4) the follower runs one
    // frame more, and the primitive returns on the tick after that frame's (the frames as logged:
    // the frame clock's); against the routing runs' returns
    for (k, f) in log.follows.iter().enumerate() {
        if !(f.before.state == 3 && f.after.state == 4) {
            continue;
        }
        let Some(next) = log.follows[k + 1..].iter().find(|g| g.follower == f.follower) else { continue };
        let predicted = next.tick + 1;
        let run = routing_runs.iter().filter(|r| r.3 <= f.tick && r.4 >= f.tick).min_by_key(|r| r.4.abs_diff(predicted));
        let Some(run) = run else { continue };
        counts.note(if run.4 == predicted {
            "routing: a routing primitive's return natively the original's tick (a frame after its walker is done)"
        } else {
            "routing: a routing primitive's return natively NOT the original's tick"
        });
        if show_walks && run.4 != predicted {
            println!("return: walker done at tick {}, its next frame at {}, predicted {predicted}, the run's return {}", f.tick, next.tick, run.4);
        }
    }
    // The turn at a walk's end (`turn_frames`): the frames a follower stays arrived (state 3)
    // against the angle from its last step's way to what it faces (the plan's last point past its
    // end), each walk's end that turns (a door's crossing walks on: set aside, its next plan at once)
    {
        let mut last_way: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
        let mut arrived: BTreeMap<u32, (u32, u32, (f32, f32))> = BTreeMap::new();
        // each follower's walk: its start's tick and place, its speed then, and its frames to arrive
        let mut walk_starts: BTreeMap<u32, (u32, (f32, f32), f32, u32)> = BTreeMap::new();
        // the ticks of a follower's frames from its walk's start (the frame clock as it ran)
        if has("REPLAY_SHOW_PLACE_OFFSETS") {
            // study: each Sim's logged place against its walker's last float that tick (the walker
            // mapped by its places: the follower whose position rounds to the place most often)
            let mut last: BTreeMap<(u32, u32), (f32, f32)> = BTreeMap::new();
            for f in &log.follows {
                last.insert((f.follower, f.tick), (f.after.x, f.after.y));
            }
            let mut hist: BTreeMap<i32, usize> = BTreeMap::new();
            for p in &log.places {
                let best = last.range((0, p.tick)..).filter(|((_, t), _)| *t == p.tick).map(|(_, &(x, y))| ((p.x as f32 - x).powi(2) + (p.y as f32 - y).powi(2)).sqrt()).fold(f32::INFINITY, f32::min);
                if best.is_finite() {
                    *hist.entry((best * 10.0) as i32).or_default() += 1;
                }
            }
            println!("place - nearest walker that tick, in tenths: {hist:?}");
        }
        if has("REPLAY_SHOW_FOLLOWS") {
            for f in log.follows.iter().filter(|f| (2036..=2041).contains(&f.tick) || (4920..=4935).contains(&f.tick)) {
                println!("follow tick {} {:x}: state {} -> {}, at ({:.3}, {:.3}) -> ({:.3}, {:.3}) factor {} -> {}", f.tick, f.follower, f.before.state, f.after.state, f.before.x, f.before.y, f.after.x, f.after.y, f.before.factor, f.after.factor);
            }
            for p in log.places.iter().filter(|p| (4920..=4925).contains(&p.tick)) {
                println!("any place tick {} {} ({}, {})", p.tick, p.id, p.x, p.y);
            }
            for p in log.places.iter().filter(|p| p.id == 237 && ((2036..=2041).contains(&p.tick) || (4920..=4935).contains(&p.tick))) {
                println!("place tick {} 237 ({}, {})", p.tick, p.x, p.y);
            }
        }
        let mut frame_ticks: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let all_frames: Vec<(u32, u32)> = log.follows.iter().map(|g| (g.follower, g.tick)).collect();
        // the game's speed each frame of a follower's walk (the follower's +124: 0 paused, 1, or 5
        // fast; the player's, the same for every walker at once), an input to the walk
        let mut speeds: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
        let mut speed_now: BTreeMap<u32, f32> = BTreeMap::new();
        for (at, f) in log.follows.iter().enumerate() {
            let (b, a) = (f.before, f.after);
            if b.state == 1 {
                // the native speed it starts from (that the walker check carried), else its last step
                let v = start_speed.get(&(f.follower, f.tick)).copied().unwrap_or_else(|| speed_now.get(&f.follower).copied().unwrap_or(0.0));
                walk_starts.insert(f.follower, (f.tick, (b.x, b.y), v, 0));
                speeds.insert(f.follower, Vec::new());
                let me = f.follower;
                frame_ticks.insert(me, std::iter::once(f.tick).chain(all_frames[at + 1..].iter().filter(|g| g.0 == me).map(|g| g.1)).collect());
            }
            if b.state <= 2
                && let Some(w) = walk_starts.get_mut(&f.follower)
            {
                w.3 += 1;
                speeds.entry(f.follower).or_default().push(b.factor);
            }
            if (a.x, a.y) != (b.x, b.y) {
                speed_now.insert(f.follower, ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt());
            }
            if (a.x, a.y) != (b.x, b.y) {
                last_way.insert(f.follower, (a.x - b.x, a.y - b.y));
            }
            if b.state == 2 && a.state == 3 {
                arrived.insert(f.follower, (f.tick, 0, (a.x, a.y)));
            }
            if b.state == 3
                && let Some(e) = arrived.get_mut(&f.follower)
            {
                e.1 += 1;
            }
            if b.state == 3 && a.state == 4
                && let Some((t0, n, end)) = arrived.remove(&f.follower)
            {
                let faced = log.plans.iter().rev().filter(|p| p.tick <= t0 && p.points.len() >= 3).find_map(|p| {
                    let q = p.points[p.points.len() - 2];
                    ((q.0 as f32 - end.0).abs() < 1.0 && (q.1 as f32 - end.1).abs() < 1.0).then(|| p.points[p.points.len() - 1])
                });
                // a walk that runs on (its walker's data 9 set) does not turn: one frame arrived,
                // then state 5 (as a turn of no angle)
                let runs_on_end = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
                let angle = if runs_on_end {
                    0.0
                } else {
                    let (Some(faced), Some(w)) = (faced, last_way.get(&f.follower).copied()) else {
                        if show_walks {
                            println!("walk end at tick {t0} {:x} set aside: no plan with its end as its last point but one (end {end:?})", f.follower);
                        }
                        continue;
                    };
                    let a1 = w.1.atan2(w.0);
                    let a2 = (faced.1 as f32 - end.1).atan2(faced.0 as f32 - end.0);
                    let pi = std::f32::consts::PI;
                    let angle = ((a2 - a1 + pi).rem_euclid(2.0 * pi) - pi).abs();
                    // a door's crossing: a plan within 5 ticks while the same routing run goes on
                    let crossing = log.plans.iter().any(|p| p.tick > t0 && p.tick <= t0 + 5 && routing_runs.iter().any(|r| r.3 <= t0 && r.4 > p.tick + 5));
                    if crossing {
                        if show_walks {
                            println!("walk end at tick {t0} {:x} set aside: a plan follows within 5 ticks (end {end:?})", f.follower);
                        }
                        continue;
                    }
                    angle
                };
                counts.note(match (runs_on_end, turn_frames(angle) == n) {
                    (false, true) => "routing: a walk's end turning natively the original's frames (3 degrees a frame)",
                    (false, false) => "routing: a walk's end turning natively NOT the original's frames",
                    (true, true) => "routing: a run-on walk's end natively the original's frames (one arrived)",
                    (true, false) => "routing: a run-on walk's end natively NOT the original's frames",
                });
                if show_walks {
                    println!("turn at tick {t0}: {:.1} degrees, {n} frames, natively {}; done at tick {}", angle.to_degrees(), turn_frames(angle), f.tick);
                }
                // the whole walk natively: from its start (state 1) along its line at the native
                // speed to its end, then the turn, against the original's frames from 1 to 4
                if let Some(&(start_tick, start, v0, walked)) = walk_starts.get(&f.follower) {
                    let line = log.plans.iter().rev().filter(|p| p.tick <= start_tick).find_map(|p| {
                        let pts: Vec<(f32, f32)> = p.points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
                        let j = pts.iter().position(|q| (q.0 - end.0).abs() < 0.5 && (q.1 - end.1).abs() < 0.5)?;
                        if let Some(i) = pts.iter().position(|q| (q.0 - start.0).abs() < 0.5 && (q.1 - start.1).abs() < 0.5) {
                            // from where the walker is (a run on can leave it a little past
                            // the plan's point: 237 at 2038, 377.309 for 377)
                            return (j > i).then(|| std::iter::once(start).chain(pts[i + 1..=j].iter().copied()).collect::<Vec<_>>());
                        }
                        // a walk begun where a run on left its walker, past the plan's start: from
                        // there through the plan's points after its start, less the sixteenth the
                        // planner nudged off the start (start + the sign of the third's way)
                        let s0 = *pts.first()?;
                        let here = ((start.0 - s0.0).powi(2) + (start.1 - s0.1).powi(2)).sqrt();
                        let sign = |v: f32| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
                        let nudged = pts.len() >= 3 && pts[1] == (s0.0 + sign(pts[2].0 - s0.0), s0.1 + sign(pts[2].1 - s0.1));
                        let k = if nudged { 2 } else { 1 };
                        (here < 5.0 && j >= k).then(|| std::iter::once(start).chain(pts[k..=j].iter().copied()).collect())
                    });
                    if line.is_none() && show_walks {
                        println!("walk end at tick {t0} {:x}: no plan line from its start {start:?} (tick {start_tick}) to its end {end:?}", f.follower);
                    }
                    if let Some(line) = line {
                        let total: f32 = line.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
                        let per_second = native_top.get(&f.follower).copied().unwrap_or(WALK_TOP_ADULT);
                        let (wtop, growth) = walk_rates(per_second);
                        let runs_on = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
                        let game = speeds.get(&f.follower).cloned().unwrap_or_default();
                        let (frames, _) = walk_frames(total, v0, wtop, growth, runs_on, &game);
                        let native = frames + turn_frames(angle);
                        // the Sim's place each tick natively: the walker's distance along the line
                        // after the tick's last frame, its point rounded, against the logged places
                        if let (Some(&id), Some(ticks)) = (walker_of.get(&f.follower), frame_ticks.get(&f.follower)) {
                            let (mut v, mut gone) = (v0, 0.0f32);
                            let mut gone_after: Vec<f32> = Vec::with_capacity(frames as usize);
                            for k in 0..frames as usize {
                                let factor = game.get(k).copied().unwrap_or(1.0);
                                if factor > 0.0 {
                                    let left = total - gone;
                                    let (nv, st) = walk_step(v, wtop, growth, factor, if runs_on { f32::INFINITY } else { left });
                                    v = nv;
                                    gone += st.min(left);
                                }
                                gone_after.push(gone);
                            }
                            if show_walks {
                                // study: each Sim with places during the walk, and how many the walk gives
                                let mut by: BTreeMap<i16, (usize, usize)> = BTreeMap::new();
                                for r in log.places.iter().filter(|r| r.tick > start_tick && r.tick <= f.tick) {
                                    let e = by.entry(r.id).or_default();
                                    e.1 += 1;
                                    if let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                        e.0 += usize::from(((x + 0.5).floor() as i32, (y + 0.5).floor() as i32) == (r.x, r.y));
                                    }
                                }
                                println!("   walk from {start_tick} (follower {:x}, mapped to {id}): places by Sim (fit, of) {by:?}; frames {} ticks {:?}..; line {:?}", f.follower, gone_after.len(), &ticks[..ticks.len().min(3)], &line[..line.len().min(6)]);
                            }
                            // the Sims it walks: the follower's, and any other whose places during the
                            // walk the walk gives at least half the time (two Sims logged moving as one)
                            let walked: BTreeSet<i16> = {
                                let mut by: BTreeMap<i16, (usize, usize)> = BTreeMap::new();
                                for r in log.places.iter().filter(|r| r.tick > start_tick && r.tick <= f.tick) {
                                    let e = by.entry(r.id).or_default();
                                    e.1 += 1;
                                    if let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                        e.0 += usize::from(((x + 0.5).floor() as i32, (y + 0.5).floor() as i32) == (r.x, r.y));
                                    }
                                }
                                by.into_iter().filter(|&(sim, (fit, of))| sim == id || 2 * fit >= of).map(|(sim, _)| sim).collect()
                            };
                            for r in log.places.iter().filter(|r| walked.contains(&r.id) && r.tick > start_tick && r.tick <= f.tick) {
                                let id = r.id;
                                let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) else { continue };
                                let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                // to the nearest (truncating puts 816 of 1,186 off)
                                let ours = ((x + 0.5).floor() as i32, (y + 0.5).floor() as i32);
                                if has("REPLAY_SHOW_WALK_MISS") && ours != (r.x, r.y) {
                                    let around: Vec<String> = (k.saturating_sub(3)..(k + 4).min(ticks.len()).min(gone_after.len())).map(|j| format!("{}{}:{}@{:.2}", if j == k { "*" } else { "" }, j, ticks[j], gone_after[j])).collect();
                                    println!("walk float {id} tick {}: native ({x:.3}, {y:.3}), log {:?}, walked {:.3}; start {start_tick}; frames {}", r.tick, (r.x, r.y), gone_after[k], around.join(" "));
                                }
                                native_places.insert((id, r.tick), ours);
                                counts.note(if ours == (r.x, r.y) {
                                    "routing: a walking Sim's place each tick natively the original's (the walker's, rounded)"
                                } else {
                                    "routing: a walking Sim's place each tick natively NOT the original's"
                                });
                                if ours != (r.x, r.y) && show_walks {
                                    let at = |k: usize| {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k.min(gone_after.len() - 1)]);
                                        ((x, y), ((x + 0.5).floor() as i32, (y + 0.5).floor() as i32))
                                    };
                                    println!("   place of {id} at tick {}: native {ours:?} (exact {:?}), the original's {:?}; a frame before {:?}, after {:?}", r.tick, at(k).0, (r.x, r.y), at(k.saturating_sub(1)).1, at(k + 1).1);
                                }
                            }
                        }
                        let original = walked + n;
                        // the routing primitive natively: its return the tick after its walker's
                        // frame after done (frame `native`, from the walk's start), against the run
                        // that holds the walk
                        if let Some(ticks) = frame_ticks.get(&f.follower)
                            && let Some(ret) = routing_return_tick(ticks, frames, turn_frames(angle))
                            && let Some(run) = routing_runs.iter().filter(|r| r.3 <= start_tick + 1 && r.4 >= f.tick).min_by_key(|r| r.4 - r.3)
                        {
                            returns.entry((run.0, run.1, run.2)).or_default().push((run.3, ret));
                            counts.note(if run.4 == ret {
                                "routing: a routing primitive natively returning on the original's tick (its walk and turn native)"
                            } else {
                                "routing: a routing primitive natively NOT returning on the original's tick"
                            });
                            // with the frame clock native too: the walker's frames from its start's
                            // (its tick and its place among that tick's frames as logged)
                            let within = ticks.iter().take_while(|&&t| t == start_tick).count();
                            let before = log.follows.iter().filter(|g| g.follower == f.follower && g.tick == start_tick).count() - within;
                            if let Some(first_at) = native_frame_ticks.iter().position(|&t| t == start_tick) {
                                let from = &native_frame_ticks[(first_at + before).min(native_frame_ticks.len())..];
                                let nat = routing_return_tick(from, frames, turn_frames(angle));
                                if show_walks && nat != Some(run.4) {
                                    let logged = &ticks[..ticks.len().min((frames + turn_frames(angle) + 2) as usize)];
                                    let native = &from[..from.len().min(logged.len())];
                                    let diff: Vec<(usize, u32, u32)> = logged.iter().zip(native).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).take(6).collect();
                                    println!("clock run from tick {start_tick}: native return {nat:?}, the original's {}; frames (index, logged tick, native tick) differing {diff:?}", run.4);
                                }
                                counts.note(if nat == Some(run.4) {
                                    "routing: a routing primitive natively returning on the original's tick (the frame clock native too)"
                                } else {
                                    "routing: a routing primitive natively NOT returning on the original's tick (the frame clock native)"
                                });
                            }
                            // each of the run's steps: natively a yield before its return tick, the
                            // return on it
                            for st in log.steps.iter().filter(|st| (st.sim, st.tree, st.node) == (run.0, run.1, run.2) && st.tick >= run.3 && st.tick <= run.4) {
                                let native_yield = st.tick < ret;
                                counts.note(if native_yield == (st.result == 1) {
                                    "routing: a routing primitive's step natively as the original's (a yield, or its return)"
                                } else {
                                    "routing: a routing primitive's step natively NOT as the original's"
                                });
                            }
                        }
                        counts.note(if native == original {
                            "routing: a whole walk natively the original's frames (walk and turn)"
                        } else {
                            "routing: a whole walk natively NOT the original's frames"
                        });
                        if show_walks {
                            println!("whole walk from tick {start_tick}: {total:.1} long, natively {frames} + {} frames, the original's {walked} + {n}", turn_frames(angle));
                        }
                    }
                }
            }
        }
    }
    // Each plan with the routing primitives its Sim ran in its tick (study: REPLAY_SHOW_PLANS):
    // opcode 27 or 45, the stack object and its place, the operand, the result.
    if has("REPLAY_SHOW_PLANS") {
        for pl in &log.plans {
            let (Some(a), Some(b)) = (pl.points.first(), pl.points.last()) else { continue };
            let who = world.objects().find(|o| o.person && (o.x16, o.y16) == *a).map_or(-1, |o| o.id() as i32);
            println!("plan tick {} from {},{} to {},{} ({} points), Sim at its start {who}", pl.tick, a.0, a.1, b.0, b.1, pl.points.len());
            let pts: Vec<String> = pl.points.iter().map(|&(x, y)| format!("{}/{}", x as f32 / 16.0, y as f32 / 16.0)).collect();
            println!("   points {}", pts.join(" "));
            let gs: Vec<String> = pl.goals.iter().map(|g| format!("{}/{} {:x} {:x}", g[0] as i32 as f32 / 16.0, g[1] as i32 as f32 / 16.0, g[2], g[3])).collect();
            println!("   goals (took {}) {}", pl.chosen, gs.join(", "));
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
    if show_walks {
        for r in &routing_runs {
            if !returns.get(&(r.0, r.1, r.2)).is_some_and(|v| v.iter().any(|x| x.0 == r.3)) {
                println!("routing run not modelled: sim {:x} tree {} node {} from tick {} to {}", r.0, r.1, r.2, r.3, r.4);
            }
        }
    }
    NativeRouting { returns, places: native_places }
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
    let lot_globals = read_lot_globals(&ngh).expect("the lots' globals");
    let the_globals: &[i16] = if lot_index < 0 || lot_index as usize >= lot_globals.len() { lot_globals.last().map_or(&[], |g| g) } else { &lot_globals[lot_index as usize] };
    // (study: the lot's globals from the disc's neighbourhood file, as the club visit's clock
    // shows the original takes them: 18:15:24, the disc's lot 15, where the save's says 19:27:40)
    let disc_globals = if has("REPLAY_DISC_GLOBALS") { read_lot_globals(&d.read("Runtime/default.ngh").unwrap_or_default()).unwrap_or_default() } else { Vec::new() };
    let the_globals: &[i16] = match usize::try_from(lot_index).ok().and_then(|i| disc_globals.get(i)) {
        Some(g) => g,
        None => the_globals,
    };
    let card_lot = card_path.is_some();
    if has("REPLAY_SHOW_LOT_CLOCKS") {
        // study: each lot's clock (globals 0, 5, 6: the hour, minute and second)
        for (k, g) in lot_globals.iter().enumerate() {
            println!("lot {k}: {:?}", [0, 5, 6].map(|i| g.get(i).copied()));
        }
    }
    // the disc's copy of the lot (its house: walls and floors a save may not keep)
    let disc_lots = if card_lot && !has("REPLAY_NO_DISC_HOUSE") { read_lots(&d.read("Runtime/default.ngh").unwrap_or_default()).unwrap_or_default() } else { Vec::new() };
    let house = usize::try_from(lot_index).ok().and_then(|i| disc_lots.get(i));
    if has("REPLAY_SHOW_LOT_CHUNKS") {
        // study: the lot's chunks and its arrays as read
        let kinds: Vec<(String, usize)> = the_lot.chunks.iter().map(|(k, c)| (k.clone(), c.len())).collect();
        println!("lot chunks: {kinds:?}");
        match read_lot_arrays(the_lot) {
            Ok(a) => {
                println!("lot arrays: {:?}", a.iter().map(|a| (a.width, a.height, a.element_size, a.bytes.iter().filter(|&&b| b != 0).count())).collect::<Vec<_>>());
                if let Some(w) = a.get(1) {
                    let per: Vec<usize> = (0..w.element_size as usize).map(|k| w.bytes.iter().skip(k).step_by(w.element_size as usize).filter(|&&b| b != 0).count()).collect();
                    let low: usize = w.bytes.iter().step_by(w.element_size as usize).filter(|&&b| b & 0x0F != 0).count();
                    println!("array 1 nonzero by byte {per:?}; byte 0 with wall bits {low}");
                }
            }
            Err(e) => println!("lot arrays: error {e:?}"),
        }
    }
    if !bench {
        let Some((counts, _)) = run(&log, &lib, (the_lot, house), the_globals, &neighbours, None, card_lot) else { return ExitCode::from(1) };
        for (k, n) in &counts.map {
            println!("  {k:<66} {n}");
        }
        return ExitCode::SUCCESS;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    let mut runs: Vec<Bench> = Vec::new();
    for _ in 0..5 {
        let Some((_, Some(b))) = run(&log, &lib, (the_lot, house), the_globals, &neighbours, Some(Bench::default()), card_lot) else { return ExitCode::from(1) };
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


/// The '3' records of tick `t` against the engine's clock and the light it gives
/// (`light::time_of_day`, `light::sun_direction`).
fn check_time_of_day(log: &housefire::tree_log::Log, t: u32, world: &housefire::world::World, at: &mut usize, counts: &mut Counts) {
    while *at < log.times_of_day.len() && log.times_of_day[*at].0 < t {
        *at += 1;
    }
    let g = |k: usize| world.globals.get(k).copied().unwrap_or(0) as i32;
    let (hour, minute, second) = (g(0), g(5), g(6));
    if std::env::var_os("REPLAY_CLOCK").is_some() && t < 3 {
        println!("  tick {t}: globals 0-8 {:?}", &world.globals[..world.globals.len().min(9)]);
    }
    while *at < log.times_of_day.len() && log.times_of_day[*at].0 == t {
        let (_, r, _, o) = &log.times_of_day[*at];
        *at += 1;
        if [r[2] as i32, r[3] as i32, r[4] as i32] != [hour, minute, second] {
            counts.note("light: time of day: DIFFERENT clock");
            if std::env::var_os("REPLAY_CLOCK").is_some() {
                println!("  tick {t}: the engine's clock {hour}:{minute}:{second}, the log's {}:{}:{} (globals {:?})", r[2] as i32, r[3] as i32, r[4] as i32, world.globals);
            }
            continue;
        }
        counts.note("light: time of day: the clock as the log's");
        let d = housefire::light::time_of_day(hour, minute, second);
        let dir = housefire::light::sun_direction(hour, minute, second).unwrap_or([f32::NAN; 3]);
        let mine = [d.sun_factor, d.sun[0], d.sun[1], d.sun[2], dir[0], dir[1], dir[2], d.extra, d.ambient_factor, d.ambient[0], d.ambient[1], d.ambient[2]].map(f32::to_bits);
        let theirs = [o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11], o[12]];
        counts.note(if mine == theirs { "light: time of day: the sun and ambient as the log's" } else { "light: time of day: DIFFERENT sun or ambient" });
    }
}
