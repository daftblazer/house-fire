//! The native interpreter against the original's, step by step: a log of the recompiled Bustin'
//! Out's interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: lockstep LOG ISO [--card CARD]
//!
//! --card: the lots from a save on a memory card image (the log's run started from it), not the
//! disc's.
//!
//! The log has every step each tree stack took. For each step the native interpreter is given
//! the stack as the log has it (each frame's tree, found on the disc by the native lookup, its
//! node, parameters and locals), runs one step with the primitive answering what the
//! original's did (its true or false target, a yield, a stop), and its stack after is compared
//! with the original's next step for the same stack:
//!
//!   a primitive: the node moved to the same target, or stayed (a yield);
//!   a call: a frame on the same tree, found by the native lookup with the same nodes, with
//!     the same parameters and locals;
//!   a return: the caller's node moved to the same target.
//!
//! The lot's state: each object variable's first read (scopes 0, 3, 8, and a Sim's 14 and 18)
//! against the lot as the engine loads it; with LOCKSTEP_LOT_DIFFS set, each one that is not
//! the lot's is printed.
//!
//! Each object's file is found from the trees the original ran for it: the disc file whose own,
//! semi-global (its GLOB) and global (Global.iff) trees have those nodes.
//!
//! Its output is the C++ tool's, line for line, but for one diagnostic that showed the native
//! objects' addresses: it shows their keys.

mod anim;
mod lot;
mod set_to_next;
mod variables;

use lot::pick_lot;
use set_to_next::set_to_next_check;
use variables::variables;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::ExitCode;

use housefire::bhav::{Node, Tree, TreeKind, tree_kind};
use housefire::cfmt::{alt_hex, buf, g};
use housefire::disc::Disc;
use housefire::expression::{ExprResult, ExpressionOperand, Variable, Variables, expression};
use housefire::interp::{Frame, Host, Interpreter, Mode, PrimResult, Step, Trees};
use housefire::library::Library;
use housefire::primitives::{create_object, drop_onto, grab, remove_object, set_to_next, snap};
use housefire::save::{memory_card_file, read_lots};
use housefire::tree_log::{ExprRec, LookupRec, StepRec, read_log, same_shape};
use housefire::world::{DATA_FACING, DATA_CONTAINER, DATA_OBJECT_ID, Object, ObjectKey, ScriptVariables, Table, World, scope};
use housefire::{FileId, TreeId};

/// The variables as the original's lookup found them: the right side is asked for first, then
/// the left (as TryExpression does); a write is kept.
struct LoggedVariables<'a> {
    e: &'a ExprRec,
    asked: i32,
    wrote: bool,
    wrote_float: bool,
    written: i16,
    written_float: f32,
}

impl Variables for LoggedVariables<'_> {
    fn get(&mut self, _: u8, _: i16) -> Option<Variable> {
        let i = self.asked;
        self.asked += 1;
        if i == 0 {
            return (self.e.flags & 8 == 0).then_some(Variable { value: self.e.rhs, f: None });
        }
        if self.e.flags & 4 != 0 {
            return None;
        }
        let mut v = Variable { value: self.e.lhs, f: None };
        if self.e.flags & 3 == 2 {
            v.f = Some(f32::from_bits(self.e.lhs_float));
        }
        Some(v)
    }
    fn set(&mut self, _: u8, _: i16, v: i16) -> bool {
        if self.e.flags & 1 == 0 {
            return false; // the original's left side has no 16-bit pointer
        }
        self.wrote = true;
        self.written = v;
        true
    }
    fn set_float(&mut self, _: u8, _: i16, v: f32) -> bool {
        if self.e.flags & 2 == 0 {
            return false;
        }
        self.wrote_float = true;
        self.written_float = v;
        true
    }
}

/// Answers the one primitive of the step as the original did.
struct LogHost<'a> {
    lib: &'a Library,
    object: Option<FileId>,
    answer: PrimResult,
}

impl Trees for LogHost<'_> {
    fn tree_at(&self, t: TreeId) -> &Tree {
        self.lib.tree_at(t)
    }
}

impl Host for LogHost<'_> {
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        self.lib.tree(self.object, id)
    }
    fn primitive(&mut self, _: &mut Interpreter, _: &Node) -> PrimResult {
        self.answer
    }
}

#[derive(Clone, Default)]
struct FrameState {
    tree: u16,
    node: u16,
    params: Vec<i16>,
    locals: Vec<i16>,
    /// Each frame its own: a Sim runs an object's interaction on its stack.
    object: Option<FileId>,
}

/// The tally of what was seen, with an example of each kind.
#[derive(Default)]
struct Notes {
    out: BTreeMap<String, i64>,
    example: BTreeMap<String, String>,
}

impl Notes {
    fn note(&mut self, k: &str) {
        *self.out.entry(k.to_string()).or_default() += 1;
    }
    fn note_ex(&mut self, k: &str, ex: &str) {
        self.note(k);
        if !ex.is_empty() && !self.example.contains_key(k) {
            self.example.insert(k.to_string(), ex.to_string());
        }
    }
}

fn vals(v: &[i16]) -> String {
    let mut o = String::from("(");
    for x in v {
        o += &format!("{x} ");
    }
    o + ")"
}

/// A variable cell's table (`None`: an object's that the world does not have) and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CellKey(Option<Table>, i32);

const EXPR_LHS: u32 = 0x80213900;
const EXPR_RHS: u32 = 0x802138C8;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: lockstep LOG ISO");
        return ExitCode::from(2);
    }
    let Some(log) = read_log(&args[1]) else {
        println!("cannot read {}", args[1]);
        return ExitCode::from(1);
    };
    let d = match Disc::open(&args[2]) {
        Ok(d) if d.id() == "G4ME69" => d,
        _ => {
            println!("{} is not Bustin' Out", args[2]);
            return ExitCode::from(1);
        }
    };
    // The neighbourhood: the card's save (--card) or the disc's.
    let ngh = match args.iter().position(|a| a == "--card").and_then(|i| args.get(i + 1)) {
        Some(p) => {
            let mut card = std::fs::read(p).unwrap_or_default();
            card.truncate(1 << 22);
            let ngh = memory_card_file(&card, "G4ME69").unwrap_or_default();
            if ngh.is_empty() {
                println!("no Bustin' Out save on {p}");
                return ExitCode::from(1);
            }
            ngh
        }
        None => d.read("Runtime/default.ngh").unwrap_or_default(),
    };
    let lib = Library::bustin_out(&d);
    // study (LOCKSTEP_SLOTS=name): the slots of the files whose path holds the name
    if let Ok(name) = std::env::var("LOCKSTEP_SLOTS") {
        for f in lib.files().iter().filter(|f| f.path.contains(&name)) {
            for d in &f.definitions {
                println!("{}: definition {} slot table {}", f.path, d.id, d.slot_table());
            }
            for (id, t) in &f.slots {
                for s in t {
                    println!("  SLOT {id}: {s:?}");
                }
            }
        }
        return ExitCode::SUCCESS;
    }
    // study (LOCKSTEP_SURVEY_OP=N): the disc's nodes of primitive N by operand bytes 4 and 5
    if let Some(op) = std::env::var("LOCKSTEP_SURVEY_OP").ok().and_then(|v| v.parse::<u16>().ok()) {
        let mut by: BTreeMap<(u8, u8), (usize, BTreeSet<String>)> = BTreeMap::new();
        for f in lib.files() {
            for &t in f.trees.values() {
                for n in lib.tree_at(t).nodes.iter().filter(|n| n.opcode == op) {
                    let e = by.entry((n.operand[4], n.operand[5])).or_default();
                    e.0 += 1;
                    if e.1.len() < 4 {
                        e.1.insert(f.path.rsplit('/').next().unwrap_or("").to_string());
                    }
                }
            }
        }
        for ((b4, b5), (n, files)) in &by {
            println!("opcode {op} byte 4 {b4} byte 5 {b5}: {n} nodes, e.g. {files:?}");
        }
        return ExitCode::SUCCESS;
    }
    let all: Vec<FileId> = (0..lib.files().len() as u32).map(FileId).collect();
    let path = |f: FileId| lib.file(f).path.as_str();
    let tree = |s: &StepRec| log.tree_of(s);
    // Each object's file: the disc file under which every tree the original ran for it is found
    // with the same nodes. Memory is reused, so a file pointer's trees are taken in order and
    // split where no one disc file has them all (another object at that address); each part is
    // an object.
    let fits = |f: FileId, id: u16, raw: &[u8]| lib.tree(Some(f), id).is_some_and(|t| same_shape(lib.tree_at(t), raw));
    // The constants a step's expression read, as (datum, value).
    let constants_read = |s: &StepRec| {
        let mut out = Vec::new();
        if let Some(e) = &s.expr {
            if e.rhs_scope == 26 && e.flags & 8 == 0 {
                out.push((e.rhs_data, e.rhs));
            }
            if e.lhs_scope == 26 && e.flags & 4 == 0 {
                out.push((e.lhs_data, e.lhs));
            }
        }
        out
    };

    // The sightings are the steps: each names its object's file pointer and the tree run (a tree
    // is logged once, under the first object that ran it, but every step has its own).
    let mut parts: Vec<Vec<FileId>> = Vec::new();
    let mut part_now: BTreeMap<u32, usize> = BTreeMap::new(); // file pointer -> its current part
    let mut step_part: Vec<Option<usize>> = vec![None; log.steps.len()];
    for (i, s) in log.steps.iter().enumerate() {
        let now = part_now.get(&s.file).copied();
        let Some(t) = tree(s).filter(|t| !matches!(tree_kind(t.tree), TreeKind::Global | TreeKind::None)) else {
            step_part[i] = now;
            continue;
        };
        if let Some(p) = now {
            let keep: Vec<FileId> = parts[p].iter().copied().filter(|&f| fits(f, t.tree, &t.raw)).collect();
            if !keep.is_empty() {
                parts[p] = keep;
                step_part[i] = Some(p);
                continue;
            }
        }
        part_now.insert(s.file, parts.len());
        step_part[i] = Some(parts.len());
        parts.push(all.iter().copied().filter(|&f| fits(f, t.tree, &t.raw)).collect());
    }
    // Variants of an object share their trees but not always their constants: the constants
    // each part's steps read narrow it further (where some file has them all).
    for (i, s) in log.steps.iter().enumerate() {
        let Some(p) = step_part[i] else { continue };
        for (data, value) in constants_read(s) {
            if (data as u16 >> 7) >= 64 {
                continue; // only the object's own tables tell variants apart
            }
            let keep: Vec<FileId> = parts[p].iter().copied().filter(|&f| lib.constant(Some(f), data) == Some(value)).collect();
            if !keep.is_empty() {
                parts[p] = keep;
            }
        }
    }
    let step_object: Vec<Option<FileId>> = step_part.iter().map(|p| p.and_then(|p| parts[p].first().copied())).collect();
    let found_parts = parts.iter().filter(|p| !p.is_empty()).count();
    let unresolved = parts.len() - found_parts;
    println!(
        "{} ticks, {} steps, {} trees loaded; objects {}, found on the disc {}",
        log.ticks,
        log.steps.len(),
        log.trees.len(),
        parts.len(),
        found_parts
    );

    let mut notes = Notes::default();
    // The stack's memory as far as the log shows it: each frame's values where they are (+16
    // on), as last seen. For telling whether a new frame's values are left over.
    let mut memory: BTreeMap<u32, i16> = BTreeMap::new();
    let remember = |memory: &mut BTreeMap<u32, i16>, frame: u32, p: &[i16], l: &[i16]| {
        for (k, &v) in p.iter().enumerate() {
            memory.insert(frame.wrapping_add(16 + 2 * k as u32), v);
        }
        for (k, &v) in l.iter().enumerate() {
            memory.insert(frame.wrapping_add(16 + 2 * (p.len() + k) as u32), v);
        }
    };
    let mut stacks: BTreeMap<u32, Vec<u32>> = BTreeMap::new(); // sim -> frame pointers, bottom first
    let mut frames: BTreeMap<u32, FrameState> = BTreeMap::new(); // frame pointer -> state
    let mut next_of: BTreeMap<usize, usize> = BTreeMap::new(); // index -> next step of its sim
    {
        let mut last: BTreeMap<u32, usize> = BTreeMap::new();
        for (i, s) in log.steps.iter().enumerate() {
            if let Some(&l) = last.get(&s.sim) {
                next_of.insert(l, i);
            }
            last.insert(s.sim, i);
        }
    }
    for (i, s) in log.steps.iter().enumerate() {
        {
            let st = stacks.entry(s.sim).or_default();
            // Frames are end to end, bottom first: a frame at or below the top replaces what was
            // above it (the engine also pops and resets stacks outside the node step).
            while st.last().is_some_and(|&b| b > s.frame) {
                st.pop();
            }
            if st.last() != Some(&s.frame) {
                st.push(s.frame);
            }
        }
        frames.insert(s.frame, FrameState { tree: s.tree, node: s.node, params: s.params.clone(), locals: s.locals.clone(), object: step_object[i] });
        remember(&mut memory, s.frame, &s.params, &s.locals);
        // The header's words the log has: +0 the tree, +2 the node, +6 the counts, +12 the file.
        memory.insert(s.frame, s.tree as i16); // (its flag bit is not kept)
        memory.insert(s.frame.wrapping_add(2), s.node as i16);
        memory.insert(s.frame.wrapping_add(6), ((s.locals.len() << 8) | s.params.len()) as i16);
        memory.insert(s.frame.wrapping_add(12), (s.file >> 16) as i16);
        memory.insert(s.frame.wrapping_add(14), s.file as i16);
        'step: {
            let st = &stacks[&s.sim];
            let Some(object) = step_object[i] else {
                notes.note("object not found");
                break 'step;
            };
            if s.look == 0 {
                notes.note("step without a node lookup (frame end at the bottom, or no file)");
                break 'step;
            }

            // The native stack, as the log has it.
            let mut h = LogHost { lib: &lib, object: Some(object), answer: PrimResult::Stop };
            let mut native: Vec<Frame> = Vec::new();
            let mut ok = true;
            for fp in st {
                let fs = &frames[fp];
                let Some(t) = fs.object.and_then(|o| lib.tree(Some(o), fs.tree)) else {
                    ok = false;
                    break;
                };
                native.push(Frame { tree: Some(t), node: fs.node, params: fs.params.clone(), locals: fs.locals.clone(), ..Default::default() });
            }
            if !ok {
                let mut e = format!("object {}, stack:", path(object));
                for fp in st {
                    e += &buf(format!(" {}@{:x}", alt_hex(frames[fp].tree as u64), fp), 32);
                }
                notes.note_ex("a frame's tree not found by the native lookup", &e);
                break 'step;
            }

            let top_tree = lib.tree_at(native.last().unwrap().tree.unwrap());
            let is_return = s.node > 250;
            let n: Option<Node> = if is_return { None } else { top_tree.nodes.get(s.node as usize).copied() };
            if let Some(n) = n.filter(|n| n.opcode < 256) {
                h.answer = match s.result {
                    1 => PrimResult::Yield,
                    2 => PrimResult::Stop,
                    // a primitive that runs a tree (RunFunctionalTree 20, CallNamedTree 28,
                    // GosubFoundAction 30, and the routing primitives 27 and 45, whose next step
                    // is a pushed tree's node 0) and stayed on its node: it pushed the tree and
                    // goes on into it (Continue), its node unchanged
                    _ if matches!(n.opcode, 20 | 27 | 28 | 30 | 45) && s.after == s.node && n.true_target as u16 != s.node && n.false_target as u16 != s.node => PrimResult::Continue,
                    _ if s.after == n.true_target as u16 => PrimResult::True,
                    _ => PrimResult::False,
                };
            }
            let mut interp = Interpreter::new(if native.len() == 1 { Mode::Check } else { Mode::Main }, 10000);
            interp.restore(native.clone());
            interp.set_defaults(s.defaults.to_vec());
            let step = interp.step(&mut h);
            let next: Option<&StepRec> = next_of.get(&i).map(|&n| &log.steps[n]);
            let back_node = |interp: &Interpreter| interp.stack().last().map_or(0, |f| f.node);
            if is_return {
                if native.len() == 1 {
                    notes.note("return from the bottom (a check tree's end)");
                    break 'step;
                }
                let caller = st[st.len() - 2];
                let Some(next) = next else {
                    notes.note("no next step");
                    break 'step;
                };
                let ex = buf(
                    format!(
                        "tree {}@{:x} returns {}; caller {}@{:x} node {}: native {}, original {}@{:x} node {}; depth {}",
                        alt_hex(s.tree as u64),
                        s.frame,
                        s.node,
                        alt_hex(frames[&caller].tree as u64),
                        caller,
                        frames[&caller].node,
                        back_node(&interp),
                        alt_hex(next.tree as u64),
                        next.frame,
                        next.node,
                        st.len()
                    ),
                    256,
                );
                if next.frame == caller && interp.stack().len() == native.len() - 1 && back_node(&interp) == next.node {
                    notes.note("return: same");
                } else {
                    notes.note_ex("return: DIFFERENT", &ex);
                }
            } else if let Some(n) = n.filter(|n| n.opcode >= 256) {
                let Some(next) = next else {
                    notes.note("no next step");
                    break 'step;
                };
                let callee = interp.stack().last().unwrap();
                let ex = buf(
                    format!(
                        "tree {} node {} calls {}: params native {}, original {}",
                        alt_hex(s.tree as u64),
                        s.node,
                        alt_hex(n.opcode as u64),
                        callee.params.len(),
                        next.params.len()
                    ),
                    256,
                );
                if interp.stack().len() == native.len() + 1
                    && next.frame > s.frame
                    && next.node == 0
                    && next.tree != n.opcode
                    && next.look != b'R'
                {
                    // The callee's frame was replaced before it ran, from outside the node step
                    // (the stack reset for a new action).
                    notes.note("call: callee replaced from outside before its first step");
                    break 'step;
                }
                if interp.stack().len() != native.len() + 1 || next.frame == s.frame || next.tree != n.opcode {
                    notes.note_ex("call: DIFFERENT (no callee frame)", &ex);
                    break 'step;
                }
                let callee_tree = lib.tree_at(callee.tree.unwrap());
                let same = tree(next).is_some_and(|t| same_shape(callee_tree, &t.raw));
                notes.note_ex(if same { "call: callee tree the same" } else { "call: callee tree DIFFERENT" }, &ex);
                let op = buf(format!(" operand {:04x} {:04x} {:04x} {:04x};", n.operand16(0), n.operand16(1), n.operand16(2), n.operand16(3)), 64);
                let caller = native.last().unwrap();
                let ev = format!(
                    "{ex}{op} caller params {} locals {}; callee params native {} original {}, locals native {} original {}",
                    vals(&caller.params),
                    vals(&caller.locals),
                    vals(&callee.params),
                    vals(&next.params),
                    vals(&callee.locals),
                    vals(&next.locals)
                );
                notes.note_ex(if callee.params == next.params { "call: parameters the same" } else { "call: parameters DIFFERENT" }, &ev);
                // A new frame's locals are zero here and left-over memory in the original: by
                // design (docs/interpreter.md, "The call"), so counted but not a difference.
                let zero = next.locals.iter().all(|&v| v == 0);
                notes.note_ex(
                    if callee.locals == next.locals {
                        "call: locals the same"
                    } else if zero {
                        "call: locals DIFFERENT"
                    } else {
                        "call: locals left over in the original (zero here, by design)"
                    },
                    &ev,
                );
                // Are the original's values what the stack's memory held there before?
                let left_over = |first: usize, v: &[i16]| {
                    let (mut known, mut same) = (0, 0);
                    for (k, &x) in v.iter().enumerate() {
                        let Some(&m) = memory.get(&next.frame.wrapping_add(16 + 2 * (first + k) as u32)) else { continue };
                        known += 1;
                        same += (m == x) as i32;
                    }
                    if known == 0 {
                        "unknown"
                    } else if same == known {
                        "left over"
                    } else {
                        "not left over"
                    }
                };
                let none = n.operand.iter().all(|&b| b == 0xFF);
                if none && callee.params != next.params {
                    let mut where_ = left_over(0, &next.params).to_string();
                    // Or some frame's parameters further down this stack?
                    for k in (0..native.len()).rev() {
                        let pp = &native[k].params;
                        let m = pp.len().min(next.params.len());
                        if !pp.is_empty() && next.params[..m] == pp[..m] {
                            where_ = format!("a frame {} down", native.len() - k);
                            break;
                        }
                    }
                    let mut stack_vals = String::new();
                    for f in &native {
                        let t = lib.tree_at(f.tree.unwrap());
                        stack_vals += &format!(" {}{}{}", t.id, vals(&f.params), vals(&f.locals));
                    }
                    notes.note_ex(&format!("  parameters (none given, differ): {where_}"), &format!("{ev}; stack{stack_vals}"));
                }
                if callee.locals != next.locals {
                    notes.note_ex(&format!("  locals left over: {}", left_over(next.params.len(), &next.locals)), &ev);
                }
            } else if let Some(n) = n {
                if n.opcode == 2 {
                    // The native expression on the values the original read.
                    match &s.expr {
                        None => notes.note("expression: no record"),
                        Some(e) => {
                            let mut vars = LoggedVariables { e, asked: 0, wrote: false, wrote_float: false, written: 0, written_float: 0.0 };
                            let op = ExpressionOperand::of(&n);
                            for (data, value) in constants_read(s) {
                                let mut where_ = buf(format!("datum {} in {}", alt_hex(data as u16 as u64), path(object)), 96);
                                let found = lib.constant(Some(object), data);
                                where_ += &format!(
                                    ": native {}, original {value}",
                                    found.map_or_else(|| "none".to_string(), |f| f.to_string())
                                );
                                notes.note_ex(
                                    match found {
                                        None => "constant: not found by the native lookup",
                                        Some(f) if f == value => "constant: same",
                                        _ => "constant: DIFFERENT",
                                    },
                                    &where_,
                                );
                            }
                            let er: ExprResult = expression(&op, &mut vars);
                            let r = er.result;
                            let dd = buf(
                                format!(
                                    "op {} lhs {}:{}={} rhs {}:{}={} flags {}: original result {} after {}, native result {} error {} wrote {}({})",
                                    e.op,
                                    e.lhs_scope,
                                    e.lhs_data,
                                    e.lhs,
                                    e.rhs_scope,
                                    e.rhs_data,
                                    e.rhs,
                                    e.flags,
                                    h.answer as i32,
                                    e.lhs_after,
                                    r as i32,
                                    er.error as i16,
                                    vars.wrote as i32,
                                    vars.written
                                ),
                                220,
                            );
                            let key = format!("expression op {}", e.op);
                            let either = n.true_target == n.false_target;
                            if op.op != e.op
                                || op.lhs_scope != e.lhs_scope as u8
                                || op.rhs_scope != e.rhs_scope as u8
                                || op.lhs_data != e.lhs_data
                                || op.rhs_data != e.rhs_data
                            {
                                notes.note_ex("expression: operand read differently", &dd);
                            } else {
                                // The result, unless both targets are the same node (then the
                                // step does not show it), and the value written.
                                let same_result = either || r == h.answer;
                                let same_value = if e.flags & 3 == 2 {
                                    let bits = if vars.wrote_float { vars.written_float.to_bits() } else { e.lhs_float };
                                    bits == e.lhs_float_after
                                } else {
                                    e.flags & 1 == 0 || if vars.wrote { vars.written == e.lhs_after } else { e.lhs == e.lhs_after }
                                };
                                let kind = if e.flags & 3 == 2 { " (float)" } else { "" };
                                let verdict = if same_result && same_value {
                                    ": same"
                                } else if same_result {
                                    ": value DIFFERENT"
                                } else {
                                    ": result DIFFERENT"
                                };
                                notes.note_ex(&format!("{key}{kind}{verdict}"), &dd);
                            }
                        }
                    }
                }
                let what = match s.result {
                    1 => "yield",
                    2 => "stop",
                    _ => "transition",
                };
                let ex = buf(
                    format!(
                        "tree {} node {} op {} result {}: native node {}, original {}",
                        alt_hex(s.tree as u64),
                        s.node,
                        n.opcode,
                        s.result,
                        back_node(&interp),
                        s.after
                    ),
                    256,
                );
                let same = if s.result == 2 {
                    step == Step::Stop
                } else {
                    !interp.stack().is_empty() && back_node(&interp) == s.after && (s.result != 1 || step == Step::Yield)
                };
                notes.note_ex(&format!("primitive {what}{}", if same { ": same" } else { ": DIFFERENT" }), &ex);
                if !same && std::env::var_os("LOCKSTEP_BY_OPCODE").is_some() {
                    notes.note_ex(&format!("  primitive {what} DIFFERENT, opcode {} result {}", n.opcode, s.result), &ex);
                }
            } else {
                notes.note_ex("node not in the tree", &s.node.to_string());
            }
        }
        // After the step: its frame's values as they were left.
        remember(&mut memory, s.frame, &s.params_after, &s.locals_after);
        if s.after <= 255 {
            memory.insert(s.frame.wrapping_add(2), s.after as i16);
        }
    }

    // (LOCKSTEP_ANIM_CHANNELS: every logged scale or translation read ('%') through the
    // engine's `anim::channel3` on its logged window: the same bits, close, or different)
    if std::env::var_os("LOCKSTEP_ANIM_CHANNELS").is_some() {
        anim::channels(&log, &d);
    }

    // (LOCKSTEP_ANIM_RUNS: each Animate (opcode 44) run, a Sim's steps on one node and frame
    // yielding until a transition; its animation by the operand's first 16 bits, the AnimRef
    // row whose id is the animatio.arc entry's; the run's ticks against the animation's frames)
    if std::env::var_os("LOCKSTEP_ANIM_RUNS").is_some() {
        anim::runs(&lib, &log, &d, &step_object);
    }
    // (LOCKSTEP_ANIM_ROTATIONS: each rotation read ('!') against the stream decoded from the
    // animation's bits alone: its header, its window, and the read from the place alone)
    if std::env::var_os("LOCKSTEP_ANIM_ROTATIONS").is_some() {
        anim::rotations(&log, &d);
    }
    // (LOCKSTEP_ANIM_FLAGS: each flag read ('~') against `anim::FlagStream` from the animation
    // and the place alone)
    if std::env::var_os("LOCKSTEP_ANIM_FLAGS").is_some() {
        anim::flags(&log, &d);
    }
    // (LOCKSTEP_ANIM_BONES: the bone matrices from the logged final node states, and the logged
    // skeletons found on the disc)
    if std::env::var_os("LOCKSTEP_ANIM_BONES").is_some() {
        anim::bones(&log, &d);
    }
    // (LOCKSTEP_ANIM_POSES: each pose's node states ('=') against the native local pose from
    // the channels and its fold into the state before)
    if std::env::var_os("LOCKSTEP_ANIM_POSES").is_some() {
        anim::poses(&log, &d);
    }
    // (LOCKSTEP_ANIM_LOADS: the engine's animations in memory, `anim::Loads`, from the lot's
    // preload sets (house n, LOCKSTEP_HOUSE, default 1: House1Preload and housepre<n>), against
    // each of the original's Animate load asks ('9', from TryAnimate at 800AC7E4) in turn)
    if std::env::var_os("LOCKSTEP_ANIM_LOADS").is_some() && !log.loaded.is_empty() {
        anim::loads(&log, &d);
    }
    // study (LOCKSTEP_SHOW_MAKERS): the node a made object came from, the step whose stack
    // object becomes an id the list gains that tick (or the one before)
    if std::env::var_os("LOCKSTEP_SHOW_MAKERS").is_some() {
        let mut have: BTreeSet<i16> = BTreeSet::new();
        let mut made: BTreeMap<u32, Vec<i16>> = BTreeMap::new();
        for l in &log.lists {
            let now: BTreeSet<i16> = l.ids.iter().copied().collect();
            if !have.is_empty() {
                made.insert(l.tick, now.difference(&have).copied().collect());
            }
            have = now;
        }
        for (i, s) in log.steps.iter().enumerate() {
            let node = step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
            if let Some(ticks) = std::env::var("LOCKSTEP_SHOW_TICKS").ok()
                && ticks.split(',').any(|t| t.parse::<u32>().ok().is_some_and(|t| t == s.tick || t == s.tick + 1))
                && let Some(n) = node.filter(|n| n.opcode < 256 && !matches!(n.opcode, 0 | 2 | 8 | 31))
                && s.look == b'X'
            {
                println!("tick {} sim {:x} tree {:#x} node {} of {:?}: opcode {} operand {:02x?}, stack object {} -> {}", s.tick, s.sim, s.tree, s.node, step_object[i].map(|f| &lib.file(f).path), n.opcode, n.operand, s.stack_object, s.stack_object_after);
            }
            if node.is_some_and(|n| n.opcode == 43 || n.opcode == 18) {
                let n = node.unwrap();
                println!(
                    "opcode {} at tick {} by tree {:#x} node {} of {:?}: operand {:02x?}, stack object {} -> {}, {}",
                    n.opcode,
                    s.tick,
                    s.tree,
                    s.node,
                    step_object[i].map(|f| &lib.file(f).path),
                    n.operand,
                    s.stack_object,
                    s.stack_object_after,
                    if s.after == n.true_target as u16 { "true" } else { "false" }
                );
            }
            let new = made.get(&s.tick).or(made.get(&(s.tick + 1)));
            if s.stack_object_after == s.stack_object || !new.is_some_and(|n| n.contains(&s.stack_object_after)) {
                continue;
            }
            let node = step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
            println!(
                "made {} at tick {} by tree {:#x} node {} of {:?}: opcode {:?} operand {:02x?}",
                s.stack_object_after,
                s.tick,
                s.tree,
                s.node,
                step_object[i].map(|f| &lib.file(f).path),
                node.map(|n| n.opcode),
                node.map(|n| n.operand)
            );
        }
    }
    let lot_world = {
        let (lots, _, _, best, _) = pick_lot(&lib, &ngh, &log);
        let mut w = World::new(&lib);
        w.load_lot(&lots[best]).unwrap();
        w.neighbours = housefire::save::read_neighbours(&ngh).unwrap_or_default();
        w
    };
    variables(&lib, &log, &step_object, &lot_world, &mut notes);
    let disc_ngh = d.read("Runtime/default.ngh").unwrap_or_default();
    let quick = d.read("quickdat.arc").unwrap_or_default();
    let tiles = housefire::arc::entries(&quick)
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.name == "SimsTiles")
        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
        .map(|img| (housefire::quickdata::fence_styles(&img), housefire::quickdata::wall_patterns(&img), housefire::quickdata::floor_patterns(&img)))
        .unwrap_or_default();
    // (and the objects' models, SimsObjects: for the window lights' kinds)
    let objects = housefire::arc::entries(&quick)
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.name == "SimsObjects")
        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
        .map(|img| housefire::quickdata::object_resources(&img))
        .unwrap_or_default();
    let tiles = (tiles.0, tiles.1, tiles.2, objects);
    set_to_next_check(&lib, &ngh, &disc_ngh, &tiles, &log, &step_object, &mut notes);

    for (k, v) in &notes.out {
        println!("  {k:<62} {v}");
        if (k.contains("DIFFERENT") || k.starts_with(' '))
            && let Some(e) = notes.example.get(k)
        {
            println!("      e.g. {e}");
        }
    }
    let diff: i64 = notes.out.iter().filter(|(k, _)| k.contains("DIFFERENT")).map(|(_, v)| v).sum();
    println!("lockstep: {} ({diff} differences, {unresolved} objects not found)", if diff != 0 { "DIFFERENT" } else { "same" });
    if diff != 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
