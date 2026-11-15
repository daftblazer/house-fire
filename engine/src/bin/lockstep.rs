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

    // (LOCKSTEP_ANIM_RUNS: each Animate (opcode 44) run, a Sim's steps on one node and frame
    // yielding until a transition; its animation by the operand's first 16 bits, the AnimRef
    // row whose id is the animatio.arc entry's; the run's ticks against the animation's frames)
    if std::env::var_os("LOCKSTEP_ANIM_RUNS").is_some() {
        let anims = d.read("animatio.arc").unwrap_or_default();
        let quick = d.read("quickdat.arc").unwrap_or_default();
        let by_id: BTreeMap<u32, housefire::anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, housefire::anim::read(housefire::arc::bytes(&anims, e))?))).collect();
        let refs: Vec<u32> = housefire::arc::entries(&quick)
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.name == "SimsObjects")
            .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
            .map(|img| img.rows("AnimRef").filter_map(|r| img.u32(r)).collect())
            .unwrap_or_default();
        let _ = &refs;
        let by_name: BTreeMap<String, &housefire::anim::Anim> = by_id.values().map(|a| (a.name.to_ascii_lowercase(), a)).collect();
        // a tree's file (the one whose trees hold it)
        let mut tree_file: BTreeMap<housefire::TreeId, housefire::FileId> = BTreeMap::new();
        for (k, f) in lib.files().iter().enumerate() {
            for t in f.trees.values() {
                tree_file.insert(*t, housefire::FileId(k as u32));
            }
        }
        // an anim table's entry: the file's STR# (129 adult), its index'th string's base name
        let name_in = |f: Option<housefire::FileId>, table: u16, k: usize| -> Option<String> {
            let path = lib.file(f?).strings.get(&table)?.get(k)?;
            let base = path.rsplit(['\\', '/']).next()?;
            Some(base.trim_end_matches(".max").trim_end_matches(".MAX").to_ascii_lowercase())
        };
        let (mut named, mut unnamed, mut by_source) = (0, 0, BTreeMap::<u8, (usize, usize)>::new());
        // per Sim: (tree, node, frame, first tick, the animation's name)
        let mut open: BTreeMap<u32, (u16, u16, u32, u32, String)> = BTreeMap::new();
        let mut runs: BTreeMap<String, BTreeMap<u32, usize>> = BTreeMap::new();
        // each run: (name, first tick, end tick)
        let mut each: Vec<(String, u32, u32)> = Vec::new();
        // each run's Sim, in the same order
        let mut each_sim: Vec<u32> = Vec::new();
        // each run's first step (its index among the steps)
        let mut first_step: BTreeMap<(String, u32), usize> = BTreeMap::new();
        let mut operand_of: BTreeMap<(String, u32), [u8; 8]> = BTreeMap::new();
        // each run's steps: (tick, step index)
        let mut steps_of: BTreeMap<(String, u32), Vec<(u32, usize)>> = BTreeMap::new();
        let mut ends: BTreeMap<u8, usize> = BTreeMap::new();
        for (i, s) in log.steps.iter().enumerate() {
            let node = step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
            let Some(n) = node.filter(|n| n.opcode == 44) else { continue };
            let index = n.operand16(0) as usize;
            let source = n.operand[4];
            let tree = step_object[i].and_then(|o| lib.tree(Some(o), s.tree));
            let file = match source {
                0 => tree.and_then(|t| tree_file.get(&t).copied()),
                1 => lib.global(),
                2 => step_object[i],
                _ => None,
            };
            let name = if n.operand[5] & 4 != 0 { None } else { name_in(file, 129, index) };
            let e = by_source.entry(source).or_default();
            match &name {
                Some(nm) if by_name.contains_key(nm) => {
                    named += 1;
                    e.0 += 1;
                }
                _ => {
                    unnamed += 1;
                    e.1 += 1;
                }
            }
            let name = name.unwrap_or_else(|| format!("(source {source} index {index})"));
            let key = (s.tree, s.node, s.frame);
            let start = match open.get(&s.sim) {
                Some((t, nd, f, first, r)) if (*t, *nd, *f) == key && *r == name => *first,
                _ => s.tick,
            };
            if start == s.tick {
                first_step.entry((name.clone(), start)).or_insert(i);
                operand_of.entry((name.clone(), start)).or_insert([n.operand[0], n.operand[1], n.operand[2], n.operand[3], n.operand[4], n.operand[5], n.operand[6], n.operand[7]]);
            }
            steps_of.entry((name.clone(), start)).or_default().push((s.tick, i));
            if s.result == 1 {
                open.insert(s.sim, (s.tree, s.node, s.frame, start, name));
            } else {
                open.remove(&s.sim);
                *ends.entry(s.result).or_default() += 1;
                each.push((name.clone(), start, s.tick));
                each_sim.push(s.sim);
                *runs.entry(name).or_default().entry(s.tick - start).or_default() += 1;
            }
        }
        let (mut fixed, mut total) = (0, 0);
        let mut fits: BTreeMap<String, usize> = BTreeMap::new();
        let mut rule: BTreeMap<&str, usize> = BTreeMap::new();
        let mut all: BTreeMap<&str, usize> = BTreeMap::new();
        println!("animate steps: named an animation {named}, not {unnamed}; by source (named, not) {by_source:?}");
        for (row, ticks) in &runs {
            let Some(a) = by_name.get(row) else { continue };
            total += 1;
            // the frames the track takes to pass its last frame (the original's track step a
            // frame, fn_8015708C: its place += the frame's time x the animation's rate, the
            // speeds 1), and the ticks those frames span by the frame clock at any phase
            let frames_needed = {
                let (mut pos, mut n) = (0f32, 0u32);
                while pos <= (a.frames as f32 - 1.0) && n < 100_000 {
                    pos += housefire::routing::WALK_FRAME * a.rate;
                    n += 1;
                }
                n
            };
            let spans: BTreeSet<u32> = (0..42)
                .map(|ph| {
                    let mut clock = housefire::routing::FrameClock { time: ph as f32 / 1000.0 };
                    let (mut t, mut f) = (0u32, 0u32);
                    while f < frames_needed {
                        f += clock.frames_to_tick();
                        t += 1;
                    }
                    t
                })
                .collect();
            if ticks.len() == 1 {
                let t = *ticks.keys().next().unwrap();
                *rule.entry(if spans.contains(&t) { "in the spans" } else if t < *spans.first().unwrap() { "shorter" } else { "longer" }).or_default() += 1;
                if std::env::var_os("LOCKSTEP_ANIM_RUNS_FIXED").is_some() {
                    println!("  fixed {row}: {t} ticks; {frames_needed} frames, spans {spans:?}");
                }
            }
            let longest = *spans.last().unwrap();
            for (t, k) in ticks {
                *all.entry(if *t <= longest { "within" } else { "beyond" }).or_default() += k;
            }
            if ticks.len() == 1 {
                fixed += 1;
                let t = *ticks.keys().next().unwrap();
                *fits.entry(format!("{:.3}", t as f32 / a.frames as f32 * a.rate / 30.0)).or_default() += 1;
            }
            if std::env::var_os("LOCKSTEP_ANIM_RUNS_EACH").is_some() {
                println!("  row {row} {}: {} frames at {}, runs {ticks:?}", a.name, a.frames, a.rate);
            }
        }
        // against the original's tracks ('8'): a track's play (its animation unchanged, its
        // place not going back) from its first step to the one that sets it done; each run's
        // nearest play of its rate starting within two ticks of it
        if !log.tracks.is_empty() {
            // a play: a track's steps from one start frame (+52); (step, first tick, done tick, frames)
            let mut plays: Vec<(f32, u32, u32, u32)> = Vec::new();
            let mut at: BTreeMap<(u32, u32), usize> = BTreeMap::new();
            for t in &log.tracks {
                let k = *at.entry((t.track, t.ran[1])).or_insert_with(|| {
                    plays.push((0.0, t.tick, u32::MAX, 0));
                    plays.len() - 1
                });
                let step = t.place[1] - t.place[0];
                let p = &mut plays[k];
                if p.2 == u32::MAX {
                    if step > 0.0 && p.0 == 0.0 {
                        p.0 = step;
                    }
                    p.3 += 1;
                    if t.done[1] != 0 {
                        p.2 = t.tick;
                    }
                }
            }
            let mut by: BTreeMap<String, usize> = BTreeMap::new();
            let mut hits: BTreeMap<(i32, i32), usize> = BTreeMap::new();
            let mut starts: BTreeMap<(bool, bool), usize> = BTreeMap::new();
            for (name, first, end) in &each {
                let Some(a) = by_name.get(name) else { continue };
                if end == first {
                    continue;
                }
                let want = housefire::routing::WALK_FRAME * a.rate;
                let near = plays
                    .iter()
                    .filter(|p| (p.0 - want).abs() < 0.01 && p.2 != u32::MAX && p.1 + 2 >= *first && p.1 <= first + 2)
                    .min_by_key(|p| (p.1 as i64 - *first as i64).abs());
                match near {
                    Some(p) => {
                        *by.entry("found".into()).or_default() += 1;
                        let predicted = ((a.frames as f32 - 1.0) / want).floor() as u32 + 1;
                        if p.3 == predicted && std::env::var_os("LOCKSTEP_ANIM_TRACKS_START").is_some() {
                            // the frames of the run's first tick and the next, against the play's first
                            let of = |tick: u32| log.tracks.iter().filter(|t| t.tick == tick).map(|t| t.frame).collect::<BTreeSet<u32>>();
                            let first_frame = log.tracks.iter().find(|t| t.tick == p.1 && t.place[0] == 0.0).map(|t| t.frame);
                            if let Some(t) = log.tracks.iter().find(|t| t.tick == p.1 && t.place[0] == 0.0) {
                                println!("  anim pointer {name}: {:08x} (tick {})", t.anim, t.tick);
                            }
                            // the first frame whose records come after the run's first step
                            let step_at = first_step.get(&(name.clone(), *first)).copied().unwrap_or(0);
                            let next_frame = log.tracks.iter().find(|t| t.after_steps > step_at).map(|t| t.frame);
                            let first_use = !each.iter().any(|e| e.0 == *name && e.1 < *first);
                            if !log.loaded.is_empty() {
                                let id = by_id.iter().find(|(_, a)| a.name.eq_ignore_ascii_case(name)).map(|e| *e.0);
                                let asks: Vec<String> = log.loaded.iter().filter(|l| Some(l.id) == id && l.tick + 40 >= *first && l.tick <= first + 2).map(|l| format!("t{} {} from {:08x}", l.tick, if l.loaded { "loaded" } else { "NOT" }, l.caller)).collect();
                                let ever_before = log.loaded.iter().filter(|l| Some(l.id) == id && l.tick < *first).count();
                                let reqs: Vec<String> = log.res_asks.iter().filter(|q| Some(q.id) == id && q.tick <= *first).take(4).map(|q| format!("t{} {} from {:08x} -> {:08x}", q.tick, if q.kind == 0 { "async" } else { "getref" }, q.caller, q.result)).collect();
                                println!("  load {name} {first} late {}: asks near {asks:?}; asked before {ever_before}; requests {reqs:?}", next_frame != first_frame);
                            }
                            *by.entry(format!("start: (first use {first_use}, late {})", next_frame != first_frame)).or_default() += 1;
                            *by.entry(if next_frame == first_frame { "first frame: the first after the step".into() } else { format!("first frame: not the first after the step ({next_frame:?} against {first_frame:?})") }).or_default() += 1;
                            let chained = each.iter().any(|e| e.2 == *first);
                            let late = first_frame.is_some_and(|f| !of(*first).contains(&f));
                            *starts.entry((chained, late)).or_default() += 1;
                            println!("  start {name} run {first}..{end} (after a run's end that tick: {chained}): frames of its tick {:?}, of the next {:?}; the play's first frame {first_frame:?} (start frame {:?}); done at tick {}", of(*first), of(first + 1), log.tracks.iter().find(|t| t.tick == p.1).map(|t| t.ran[1]), p.2);
                        }
                        if p.3 == predicted {
                            *by.entry("played whole (the predicted frames)".into()).or_default() += 1;
                            *hits.entry((p.1 as i32 - *first as i32, *end as i32 - p.2 as i32)).or_default() += 1;
                        }
                        if std::env::var_os("LOCKSTEP_ANIM_TRACKS_EACH").is_some() {
                            println!("  run {name} {first}..{end}: track play {}..{} ({} frames, predicted {})", p.1, p.2, p.3, ((a.frames as f32 - 1.0) / want).floor() as u32 + 1);
                        }
                    }
                    None => *by.entry("no track".into()).or_default() += 1,
                }
            }
            if std::env::var_os("LOCKSTEP_ANIM_SPEEDS").is_some() {
                // (the tracks' +72 speed by tick: its spans per value, all tracks)
                let mut spans: Vec<(u32, u32, String)> = Vec::new();
                let mut by_tick: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
                for t in &log.tracks {
                    if t.speeds[1] != 0.0 {
                        by_tick.entry(t.tick).or_default().insert(format!("{}", t.speeds[1]));
                    }
                }
                for (tick, v) in by_tick {
                    let v = v.into_iter().collect::<Vec<_>>().join(",");
                    match spans.last_mut() {
                        Some(l) if l.2 == v => l.1 = tick,
                        _ => spans.push((tick, tick, v)),
                    }
                }
                for (a, b, v) in spans {
                    println!("  speeds {a}..{b}: {v}");
                }
                // the primitive steps (not 0/2/8/31) of the ticks LOCKSTEP_ANIM_SPEEDS names
                let want: Vec<u32> = std::env::var("LOCKSTEP_ANIM_SPEEDS").unwrap_or_default().split(',').filter_map(|t| t.parse().ok()).collect();
                for (i, st) in log.steps.iter().enumerate() {
                    if !want.contains(&st.tick) {
                        continue;
                    }
                    let Some(n) = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|t| lib.tree_at(t).nodes.get(st.node as usize)).copied() else { continue };
                    if n.opcode < 256 && !matches!(n.opcode, 0 | 2 | 8 | 31) {
                        println!("  step tick {} sim {:x} tree {:#x} node {} of {:?}: opcode {} operand {:02x?} result {} locals {:?}", st.tick, st.sim, st.tree, st.node, step_object[i].map(|f| &lib.file(f).path), n.opcode, n.operand, st.result, &st.locals_after[..st.locals_after.len().min(3)]);
                    }
                }
            }
            if std::env::var_os("LOCKSTEP_ANIM_SPEEDREC").is_some() {
                // (the animators' speed updates, '#': the speeds seen; each tick with a speed other
                // than 1, its updates in order with that tick's load asks among them)
                let mut seen: BTreeMap<String, usize> = BTreeMap::new();
                for s in &log.speeds {
                    *seen.entry(format!("{}", s.after)).or_default() += 1;
                }
                println!("  log: {} ticks, {} steps, {} speed updates {seen:?}", log.ticks, log.steps.len(), log.speeds.len());
                // each animator's spans of one speed (first tick..last tick: speed, updates)
                let mut spans: BTreeMap<u32, Vec<(u32, u32, String, usize)>> = BTreeMap::new();
                for s in &log.speeds {
                    let v = spans.entry(s.animator).or_default();
                    let sp = format!("{}", s.after);
                    match v.last_mut() {
                        Some(l) if l.2 == sp => {
                            l.1 = s.tick;
                            l.3 += 1;
                        }
                        _ => v.push((s.tick, s.tick, sp, 1)),
                    }
                }
                for (a, v) in &spans {
                    println!("  animator {a:08x} (owner {:08x}): {}", log.speeds.iter().find(|s| s.animator == *a).map_or(0, |s| s.owner), v.iter().map(|e| format!("{}..{}:{}({})", e.0, e.1, e.2, e.3)).collect::<Vec<_>>().join(" "));
                }
                let odd: BTreeSet<u32> = log.speeds.iter().filter(|s| s.after != 1.0).map(|s| s.tick).collect();
                let want: Vec<u32> = std::env::var("LOCKSTEP_ANIM_SPEEDREC_TICKS").unwrap_or_default().split(',').filter_map(|t| t.parse().ok()).collect();
                for &t in odd.iter().filter(|t| want.iter().any(|w| *w == **t || *w + 1 == **t || **t + 1 == *w)) {
                    let mut line: Vec<(usize, String)> = log.speeds.iter().filter(|s| s.tick == t).map(|s| (s.after_steps, format!("{:x}:{}", s.animator & 0xffff, s.after))).collect();
                    line.extend(log.loaded.iter().filter(|l| l.tick == t && l.caller == 0x800A_C7E4).map(|l| (l.after_steps, format!("ask {:08x} {}", l.id, l.loaded))));
                    // the frames' track steps as markers
                    let mut fr: BTreeMap<u32, usize> = BTreeMap::new();
                    for tr in log.tracks.iter().filter(|r| r.tick == t) {
                        fr.entry(tr.frame).or_insert(tr.after_steps);
                    }
                    line.extend(fr.iter().map(|(f, a)| (*a, format!("[frame {f}]"))));
                    line.sort_by_key(|e| e.0);
                    println!("  tick {t}: {}", line.iter().map(|e| e.1.as_str()).collect::<Vec<_>>().join(", "));
                }
            }
            if std::env::var_os("LOCKSTEP_ANIM_PAUSES").is_some() {
                // (each controller's paused frames: all its tracks with a rate at speed 0 on a
                // frame while some had moved the frame before; with that tick's load asks)
                let mut by_frame: BTreeMap<(u32, u32), (u32, usize, usize)> = BTreeMap::new();
                for t in log.tracks.iter().filter(|t| t.speeds[0] != 0.0 && t.anim != 0x7e04_f99c) {
                    let e = by_frame.entry((t.controller, t.frame)).or_insert((t.tick, 0, 0));
                    e.1 += 1;
                    if t.speeds[1] == 0.0 {
                        e.2 += 1;
                    }
                }
                let mut was: BTreeMap<u32, bool> = BTreeMap::new();
                for (&(ctl, frame), &(tick, n, zero)) in &by_frame {
                    let paused = zero == n;
                    if paused && was.get(&ctl) == Some(&false) {
                        let asks: Vec<String> = log.loaded.iter().filter(|l| l.tick + 1 >= tick && l.tick <= tick).map(|l| format!("t{} {:08x} {} from {:08x}", l.tick, l.id, l.loaded, l.caller)).collect();
                        let reqs: Vec<String> = log.res_asks.iter().filter(|q| q.tick + 1 >= tick && q.tick <= tick).map(|q| format!("t{} {} {:08x} from {:08x}", q.tick, q.kind, q.id, q.caller)).collect();
                        println!("  pause: controller {ctl:08x} frame {frame} tick {tick} ({n} tracks); asks {asks:?}; requests {reqs:?}");
                    }
                    was.insert(ctl, paused);
                }
                // the Animate load asks that miss (800AC7E4, false), with and without a pause that tick
                let pause_ticks: BTreeSet<u32> = {
                    let mut w: BTreeMap<u32, bool> = BTreeMap::new();
                    let mut t = BTreeSet::new();
                    for (&(ctl, _), &(tick, n, zero)) in &by_frame {
                        if zero == n && w.get(&ctl) == Some(&false) {
                            t.insert(tick);
                        }
                        w.insert(ctl, zero == n);
                    }
                    t
                };
                let misses: BTreeSet<u32> = log.loaded.iter().filter(|l| l.caller == 0x800A_C7E4 && !l.loaded).map(|l| l.tick).collect();
                println!("  pauses: {} ticks; load misses {} ticks, {} of them with a pause; pauses without a miss {:?}", pause_ticks.len(), misses.len(), misses.intersection(&pause_ticks).count(), pause_ticks.difference(&misses).collect::<Vec<_>>());
                println!("  misses without a pause (with that tick's track records): {:?}", misses.difference(&pause_ticks).map(|t| (*t, log.tracks.iter().filter(|r| r.tick == *t).count(), log.tracks.iter().filter(|r| r.tick == *t + 1).count())).collect::<Vec<_>>());
            }
            if let Ok(want) = std::env::var("LOCKSTEP_ANIM_TRACK_STEPS") {
                // (a track's steps whose place moves other than its usual step: the tracks
                // set done at tick LOCKSTEP_ANIM_TRACK_STEPS)
                let at: u32 = want.parse().unwrap_or(0);
                for tr in log.tracks.iter().filter(|t| t.tick == at && t.done[0] == 0 && t.done[1] != 0).map(|t| t.track).collect::<BTreeSet<_>>() {
                    let steps: Vec<_> = log.tracks.iter().filter(|t| t.track == tr && t.tick + 200 > at && t.tick <= at).collect();
                    let usual = steps.iter().map(|t| ((t.place[1] - t.place[0]) * 100.0).round() as i32).fold(BTreeMap::<i32, usize>::new(), |mut m, d| { *m.entry(d).or_default() += 1; m });
                    println!("  track {tr:08x}: steps x100 {usual:?}");
                    for t in steps.iter().filter(|t| ((t.place[1] - t.place[0]) * 100.0).round() as i32 != usual.iter().max_by_key(|e| e.1).map_or(0, |e| *e.0)) {
                        let same: Vec<_> = log.tracks.iter().filter(|u| u.frame == t.frame && u.track != t.track).collect();
                        let moved = same.iter().filter(|u| u.place[1] != u.place[0]).count();
                        let paused = same.iter().filter(|u| u.speeds[1] == 0.0).count();
                        println!("    tick {} frame {} place {:.3}->{:.3} speeds {:?} ran {:?} (that frame's other tracks: {} moved, {} at speed 0, of {}; its controller's: {:?})", t.tick, t.frame, t.place[0], t.place[1], t.speeds, t.ran, moved, paused, same.len(), same.iter().filter(|u| u.controller == t.controller).map(|u| (u.track, u.speeds[1])).collect::<Vec<_>>());
                    }
                }
            }
            if let Ok(want) = std::env::var("LOCKSTEP_ANIM_TRACKS_NEAR") {
                for (name, first, end) in each.iter().filter(|e| e.0 == want) {
                    println!("  {name} {first}..{end}:");
                    for t in log.tracks.iter().filter(|t| t.tick + 3 >= *first && t.tick <= first + 3) {
                        println!("    tick {} ctl {:08x} track {:08x} anim {:08x} place {:.3}->{:.3} done {}->{} speeds {:?} frame {} ran {:?} head {:08x?}", t.tick, t.controller, t.track, t.anim, t.place[0], t.place[1], t.done[0], t.done[1], t.speeds, t.frame, t.ran, t.head);
                    }
                }
            }
            if !starts.is_empty() {
                println!("animate plays' starts: (after a run's end that tick, first frame in the next tick): {starts:?}");
            }
            // natively: the step that finds the animation loaded (`anim::Loads` from the lot's
            // sets), the first frame after it, the frames to the last (the place's step a frame
            // 0.033 x the rate), and the return the tick after that frame's
            if std::env::var_os("LOCKSTEP_ANIM_NATIVE").is_some() {
                let house: u32 = std::env::var("LOCKSTEP_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
                let ids: std::collections::HashSet<u32> = by_id.keys().copied().collect();
                let ds = d.read("datasets.arc").unwrap_or_default();
                let sets: Vec<Vec<u8>> = housefire::arc::entries(&ds).unwrap_or_default().iter().filter(|e| e.name == "House1Preload" || e.name == format!("housepre{house:02}")).map(|e| housefire::arc::bytes(&ds, e).to_vec()).collect();
                let refs: Vec<&[u8]> = sets.iter().map(|v| v.as_slice()).collect();
                let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
                // the frames in order: (frame, its tick, the steps before its first record)
                let mut frames: Vec<(u32, u32, usize)> = Vec::new();
                for t in &log.tracks {
                    if frames.last().is_none_or(|f| f.0 != t.frame) {
                        frames.push((t.frame, t.tick, t.after_steps));
                    }
                }
                let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
                let mut early: BTreeMap<(String, u8, &str, i16), Vec<usize>> = BTreeMap::new();
                let mut runs_in_order: Vec<&(String, u32, u32)> = each.iter().collect();
                runs_in_order.sort_by_key(|e| e.1);
                for (name, first, end) in runs_in_order {
                    let (Some(a), Some(steps)) = (by_name.get(name), steps_of.get(&(name.clone(), *first))) else { continue };
                    let Some(id) = by_id.iter().find(|(_, x)| x.name == a.name).map(|e| *e.0) else { continue };
                    // the first of the run's steps whose ask finds it loaded (one ask a tick)
                    let mut seen = BTreeSet::new();
                    let Some(&(_, at)) = steps.iter().filter(|(t, _)| seen.insert(*t)).find(|(t, _)| loads.ask(id, *t)) else { continue };
                    let want = housefire::routing::WALK_FRAME * a.rate;
                    // played backward (operand byte 5's 0x02: the controller's speed -1, from the
                    // last frame), the first frame stays put: one frame more
                    let backward = operand_of.get(&(name.clone(), *first)).is_some_and(|o| o[5] & 2 != 0);
                    let n = ((a.frames as f32 - 1.0) / want).floor() as usize + 1 + usize::from(backward);
                    let Some(k) = frames.iter().position(|f| f.2 > at) else { continue };
                    let Some(last) = frames.get(k + n - 1) else { continue };
                    let predicted = last.1 + 1;
                    *tally.entry(match predicted.cmp(end) {
                        std::cmp::Ordering::Equal => "returns on the native tick",
                        std::cmp::Ordering::Greater => "ends before the animation's end (an event or an interruption)",
                        std::cmp::Ordering::Less => "ends after it (NOT native)",
                    }).or_default() += 1;
                    if predicted > *end {
                        // an early end: the frames it played (from its first to its last step), the
                        // branch, local 0 after it
                        let last_i = steps.last().map(|e| e.1).unwrap_or(0);
                        let played = frames.iter().filter(|f| f.2 > at && f.2 <= last_i).count();
                        let ls = &log.steps[last_i];
                        let node = step_object[last_i].and_then(|o| lib.tree(Some(o), ls.tree)).and_then(|t| lib.tree_at(t).nodes.get(ls.node as usize)).copied();
                        let branch = node.map_or("?", |nd| if ls.after == nd.true_target as u16 { "true" } else if ls.after == nd.false_target as u16 { "false" } else { "other" });
                        let flags = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[5]);
                        early.entry((name.clone(), flags, branch, ls.locals_after.first().copied().unwrap_or(-1))).or_default().push(played);
                    }
                    if predicted == *end && std::env::var_os("LOCKSTEP_ANIM_NATIVE_EACH").is_some() {
                        println!("  native {name} {first}..{end}; operand {:02x?}", operand_of.get(&(name.clone(), *first)));
                    }
                    if predicted < *end && std::env::var_os("LOCKSTEP_ANIM_NATIVE_EACH").is_some() {
                        println!("  later {name} {first}..{end}: native {predicted}; operand {:02x?}", operand_of.get(&(name.clone(), *first)));
                    }
                }
                println!("animate runs natively (the load, the frames, the tick after): {tally:?}");
                // (LOCKSTEP_ANIM_NOTES: event ends. A run ends on the tick after the frame whose
                // place passes its play's next note (the AnimRef row's
                // notes, ms); the script's next Animate of the same animation from that tick goes
                // on with the same play)
                if std::env::var_os("LOCKSTEP_ANIM_NOTES").is_some() {
                    let notes = housefire::arc::entries(&quick)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|e| e.name == "SimsObjects")
                        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
                        .map(|img| {
                            img.rows("AnimRef")
                                .filter_map(|r| {
                                    let id = img.u32(r)?;
                                    let p = img.ptr(r + 8)?;
                                    let n = img.u32(p - 4)? as usize;
                                    Some((id, (0..n.min(64)).filter_map(|k| Some((img.u32(p + 12 * k)?, img.u32(p + 12 * k + 4)?, img.u32(p + 12 * k + 8)? as i32))).collect::<Vec<_>>()))
                                })
                                .collect::<BTreeMap<u32, Vec<(u32, u32, i32)>>>()
                        })
                        .unwrap_or_default();
                    let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
                    // per Sim: the play (animation, its first frame's index, the notes passed, its end tick)
                    let mut play: BTreeMap<u32, (String, usize, usize, u32)> = BTreeMap::new();
                    let mut order: Vec<usize> = (0..each.len()).collect();
                    order.sort_by_key(|&k| each[k].1);
                    let mut t2: BTreeMap<String, usize> = BTreeMap::new();
                    // per Sim: the counted events handed out, and whether its play is at its end
                    // owing more
                    let mut counted: BTreeMap<u32, (usize, bool)> = BTreeMap::new();
                    let use_track_speed = std::env::var_os("LOCKSTEP_ANIM_TRACK_SPEED").is_some();
                    let mut started: BTreeMap<u32, Vec<(u32, f32)>> = BTreeMap::new();
                    let mut track_speed: BTreeMap<(u32, u32), f32> = BTreeMap::new();
                    for t in &log.tracks {
                        if track_speed.insert((t.track, t.frame), t.speeds[1].abs()).is_none() && t.ran[1] + 1 >= t.frame && t.ran[1] <= t.frame {
                            started.entry(t.ran[1]).or_default().push((t.track, t.speeds[0]));
                        }
                    }
                    // the frames' ticks in fast-forward (the tracks stepping at 5: The Sims 2's
                    // SAnimator2::Update sets +252 from the game's speed; a frame every ~4 ticks)
                    let fast_ticks: BTreeSet<u32> = log.tracks.iter().filter(|t| t.speeds[1].abs() == 5.0).map(|t| t.tick).collect();
                    if let (Some(a), Some(b)) = (fast_ticks.first(), fast_ticks.last()) {
                        println!("  fast-forward frames: ticks {a}..{b} ({} frames)", fast_ticks.len());
                    }
                    let mut no_note: BTreeMap<(u8, &str, u8, i16, String), usize> = BTreeMap::new();
                    let mut no_note_by: BTreeMap<(String, String, &str), usize> = BTreeMap::new();
                    for k in order {
                        let (name, first, end) = &each[k];
                        let sim = each_sim[k];
                        let (Some(a), Some(steps)) = (by_name.get(name), steps_of.get(&(name.clone(), *first))) else { continue };
                        let Some(id) = by_id.iter().find(|(_, x)| x.name == a.name).map(|e| *e.0) else { continue };
                        let flags = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[5]);
                        let step = housefire::routing::WALK_FRAME * a.rate;
                        // going on with the Sim's play, or a new one
                        // (the play goes on when the Sim's play is of this animation and was left
                        // at a note, not at its end)
                        // (LOCKSTEP_ANIM_SKIP_ONCE: a run that returns on its first tick leaves the
                        // play alone: whether a restart's burst uses up the new play's notes)
                        if end == first && std::env::var_os("LOCKSTEP_ANIM_SKIP_ONCE").is_some() {
                            *t2.entry("at once, left out".to_string()).or_default() += 1;
                            continue;
                        }
                        let go_on = play.get(&sim).is_some_and(|p| p.0 == *name && p.3 != u32::MAX);
                        // the counted events (notes of value 0..99) the operand's byte 6 asks for
                        let wanted = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[6] as i8 as i32);
                        if !go_on {
                            counted.insert(sim, (0, false));
                        }
                        let backward = flags & 2 != 0;
                        // a play at its end still owing counted events: one at once to each call,
                        // then true at once
                        if counted.get(&sim).is_some_and(|c| c.1) {
                            let c = counted.get_mut(&sim).unwrap();
                            let key = if end == first { "at the end, at once, on the native tick" } else { "at the end, at once (NOT native)" };
                            *t2.entry(key.to_string()).or_default() += 1;
                            if (c.0 as i32) < wanted {
                                c.0 += 1;
                            } else if let Some(p) = play.get_mut(&sim) {
                                p.3 = u32::MAX;
                            }
                            continue;
                        }
                        let (k0, passed) = if go_on {
                            let p = &play[&sim];
                            (p.1, p.2)
                        } else {
                            let mut seen = BTreeSet::new();
                            let Some(&(_, at)) = steps.iter().filter(|(t, _)| seen.insert(*t)).find(|(t, _)| loads.ask(id, *t)) else { continue };
                            let Some(k0) = frames.iter().position(|f| f.2 > at) else { continue };
                            (k0, 0)
                        };
                        // the script's notes: type 6 (type 1, 5 and the rest are other notes:
                        // the approach's steps, sounds)
                        let list: Vec<(u32, u32, i32)> = notes.get(&id).cloned().unwrap_or_default().into_iter().filter(|n| n.1 == 6).collect();
                        let last = a.frames as f32 - 1.0;
                        // the frame (from the play's first) whose place passes the next note, if
                        // this run waits for notes
                        // (every note ends the run, whatever the flags: iteration 128)
                        // (The Sims 2's SAnimator2::TryAnimate: a note over 99 ends the run; one of
                        // 0..99 only when it is the next counted event, else it is passed over)
                        let mut counter = counted.get(&sim).map_or(0, |c| c.0);
                        let mut idx = passed;
                        let next_idx = loop {
                            let Some(n) = list.get(idx) else { break None };
                            if n.0 as f32 * a.rate / 1000.0 > last {
                                break None;
                            }
                            if n.2 > 99 {
                                break Some(idx);
                            }
                            let expect = if backward { wanted - counter as i32 - 1 } else { counter as i32 };
                            if (0..wanted).contains(&expect) && n.2 == expect {
                                counter += 1;
                                break Some(idx);
                            }
                            idx += 1;
                        };
                        let next = next_idx.map(|k| list[k].0 as f32 * a.rate / 1000.0);
                        let passed_after = next_idx.map_or(list.len(), |k| k + 1);
                        // at the end: a counted event still owed goes to this run (false), the
                        // rest at once to the next calls; none owed, true
                        let mut at_end = false;
                        if next.is_none() && (counter as i32) < wanted {
                            counter += 1;
                            at_end = true;
                        }
                        counted.insert(sim, (counter, at_end));
                        // forward the place runs 0 → last; backward (0x02) last → 0 after a still
                        // first frame
                        let gone = |t: f32| if backward { last - t } else { t };
                        // the frames whose place stays at or below the target, the place stepping
                        // by the step × the game's speed (5 in fast-forward, from the frame before)
                        let target = match next {
                            Some(t) => gone(t),
                            None => last,
                        };
                        // the play's track: one whose start frame (+52) is the play's first frame,
                        // at the animation's rate
                        let play_track = frames.get(k0).and_then(|f| started.get(&(f.0 - 1)).or(started.get(&f.0))).and_then(|v| v.iter().find(|(_, r)| *r == a.rate)).map(|e| e.0);
                        let mut place = 0.0f32;
                        let mut m = 0usize;
                        while frames.get(k0 + m + 1).is_some() {
                            // (the speed the frame before set: the animator's +252, read by the
                            // next frame's track step)
                            let speed = if fast_ticks.contains(&frames[k0 + m].1) { 5.0 } else { 1.0 };
                            // (LOCKSTEP_ANIM_TRACK_SPEED: the play's own track's logged speed at
                            // that frame instead: its Sim's pauses as well)
                            let speed = match play_track.filter(|_| use_track_speed).and_then(|tr| track_speed.get(&(tr, frames[k0 + m].0))) {
                                Some(&v) => v,
                                None => speed,
                            };
                            if place + step * speed > target + 1e-3 {
                                break;
                            }
                            place += step * speed;
                            m += 1;
                        }
                        let j = m + 1 + usize::from(backward);
                        let Some(f) = frames.get(k0 + j - 1) else { continue };
                        let predicted = f.1 + 1;
                        let what = if end == first { "at once" } else if next.is_some() { "a note's" } else { "the end's" };
                        let key = match predicted.cmp(end) {
                            std::cmp::Ordering::Equal => format!("{what} end on the native tick"),
                            std::cmp::Ordering::Greater => format!("{what}: ended before (NOT native)"),
                            std::cmp::Ordering::Less => format!("{what}: ended after (NOT native)"),
                        };
                        if predicted != *end && std::env::var_os("LOCKSTEP_ANIM_NOTES_EACH").is_some() {
                            println!("  note end {name} {first}..{end} sim {sim:x} (note {passed}, place {:?}, wanted {wanted}, counted {counter}, flags {flags:02x}): native {predicted}", next);
                        }
                        if predicted > *end && end != first {
                            // an early end with no note due: its flags, branch and local 0, and
                            // whether the next of the Sim's steps (after the run) is in another tree
                            let last_i = steps.last().map(|e| e.1).unwrap_or(0);
                            let ls = &log.steps[last_i];
                            let node = step_object[last_i].and_then(|o| lib.tree(Some(o), ls.tree)).and_then(|t| lib.tree_at(t).nodes.get(ls.node as usize)).copied();
                            let branch = node.map_or("?", |nd| if ls.after == nd.true_target as u16 { "true" } else if ls.after == nd.false_target as u16 { "false" } else { "other" });
                            let next_same_sim = log.steps[last_i + 1..].iter().find(|x| x.sim == sim);
                            let after = next_same_sim.map_or("none".to_string(), |x| if x.frame == ls.frame && x.tree == ls.tree { "the same tree".into() } else if x.frame < ls.frame { "a popped frame".into() } else { format!("frame {} (was {})", x.frame, ls.frame) });
                            *no_note_by.entry((name.clone(), format!("{:02x?}", operand_of.get(&(name.clone(), *first)).copied().unwrap_or_default()), branch)).or_default() += 1;
                            *no_note.entry((flags, branch, ls.result, ls.locals_after.first().copied().unwrap_or(-1), if after.starts_with("frame") { "another frame".to_string() } else { after })).or_default() += 1;
                        }
                        *t2.entry(key).or_default() += 1;
                        play.insert(sim, (name.clone(), k0, passed_after, if next.is_some() || at_end { *end } else { u32::MAX }));
                    }
                    println!("animate runs with the notes: {t2:?}");
                    for (k, v) in &no_note_by {
                        println!("  early, no note due, by: {} {} {}: {v}", k.0, k.1, k.2);
                    }
                    for (k, v) in &no_note {
                        println!("  early, no note due: flags {:02x}, {}, result {}, local0 {}, then {}: {v}", k.0, k.1, k.2, k.3, k.4);
                    }
                }
                if std::env::var_os("LOCKSTEP_ANIM_EARLY").is_some() {
                    for ((name, flags, branch, l0), played) in &early {
                        println!("  early {name} flags {flags:02x} {branch} local0 {l0}: frames played {played:?}");
                    }
                }
            }
            println!("animate runs against the tracks: {by:?}; (track start - run start, run end - track done): runs {hits:?}");
        }
        println!("animate runs by the frames: fixed runs against the frame clock's spans {rule:?}; all runs against the longest span {all:?}");
        println!("animate runs: {} animations ({fixed} always the same ticks), ends by result {ends:?}; ticks over frames (at 30) of the fixed ones {fits:?}", total);
    }
    // (LOCKSTEP_ANIM_LOADS: the engine's animations in memory, `anim::Loads`, from the lot's
    // preload sets (house n, LOCKSTEP_HOUSE, default 1: House1Preload and housepre<n>), against
    // each of the original's Animate load asks ('9', from TryAnimate at 800AC7E4) in turn)
    if std::env::var_os("LOCKSTEP_ANIM_LOADS").is_some() && !log.loaded.is_empty() {
        let house: u32 = std::env::var("LOCKSTEP_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let anims = d.read("animatio.arc").unwrap_or_default();
        let ids: std::collections::HashSet<u32> = housefire::arc::entries(&anims).unwrap_or_default().iter().map(|e| e.id).collect();
        let ds = d.read("datasets.arc").unwrap_or_default();
        let sets: Vec<Vec<u8>> = housefire::arc::entries(&ds)
            .unwrap_or_default()
            .iter()
            .filter(|e| e.name == "House1Preload" || e.name == format!("housepre{house:02}"))
            .map(|e| housefire::arc::bytes(&ds, e).to_vec())
            .collect();
        let refs: Vec<&[u8]> = sets.iter().map(|v| v.as_slice()).collect();
        let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
        let (mut same, mut differ) = (0, 0);
        for l in log.loaded.iter().filter(|l| l.caller == 0x800A_C7E4) {
            if loads.ask(l.id, l.tick) == l.loaded {
                same += 1;
            } else {
                differ += 1;
                println!("  load ask differs: tick {} id {:08x}: the original's {}", l.tick, l.id, l.loaded);
            }
        }
        println!("animate loads natively (sets {}): {same} asks answered as the original's, {differ} not", sets.len());
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

/// The variables: the native object model against the original's lookups.
///
/// A native object for each object the original asks for or names as a stack object. Each
/// variable cell takes its value from the log the first time it is seen; after that the native
/// value must be what the log reads, unless something other than a script's expression wrote it
/// (the engine, or a primitive not modelled yet: counted apart). And each cell must always be
/// the same address in the original, and each address the same cell: that checks what the
/// scopes name.
fn variables(lib: &Library, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], lot: &World, notes: &mut Notes) {
    let mut world = World::new(lib);
    // the neighbourhood's neighbours (scopes 24 and 32 look them up)
    world.neighbours = lot.neighbours.clone();
    let mut native: BTreeMap<u32, ObjectKey> = BTreeMap::new(); // the original's object pointer -> native object
    let mut cell_addr: HashMap<CellKey, u32> = HashMap::new();
    let mut addr_cell: BTreeMap<u32, CellKey> = BTreeMap::new();
    let mut seen: HashMap<CellKey, bool> = HashMap::new();
    let mut last_writer: BTreeMap<u32, u32> = BTreeMap::new(); // address -> the caller that last had its pointer
    let mut addr_used: BTreeMap<u32, usize> = BTreeMap::new(); // address -> the last step that reached it
    let mut bound_at: HashMap<CellKey, u32> = HashMap::new(); // a cell's binding -> the tick it was made
    // the ticks each id was made again (it appears in the original's list after being absent):
    // a cell of that id bound before is another object's memory now
    let mut remade: BTreeMap<i16, Vec<u32>> = BTreeMap::new();
    for w in log.lists.windows(2) {
        for id in w[1].ids.iter().filter(|id| !w[0].ids.contains(id)) {
            remade.entry(*id).or_default().push(w[1].tick);
        }
    }
    let mut why: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new(); // a difference -> (tick band, caller) counts
    let mut loaded_at = u32::MAX; // the tick the current lot started loading
    let mut shown = 0;
    let differ = |notes: &mut Notes, why: &mut BTreeMap<String, BTreeMap<String, i64>>, loaded_at: u32, k0: &str, s: &StepRec, v: &LookupRec| {
        // While a lot loads the engine sets its objects up itself (their ids among them):
        // counted apart.
        let loading = loaded_at != u32::MAX && s.tick >= loaded_at && s.tick < loaded_at.wrapping_add(25);
        let k = if loading { format!("{} (while a lot loads)", &k0[..k0.find(" DIFFERENT").unwrap_or(k0.len())]) } else { k0.to_string() };
        notes.note(&k);
        let b = buf(format!("tick {}-{} caller {:x}", s.tick / 20 * 20, s.tick / 20 * 20 + 19, v.caller), 64);
        *why.entry(k).or_default().entry(b).or_default() += 1;
    };
    let object_at = |world: &mut World, native: &mut BTreeMap<u32, ObjectKey>, ptr: u32, file: Option<FileId>| -> ObjectKey {
        if let Some(&k) = native.get(&ptr) {
            return k;
        }
        let mut o = Object::new();
        o.file = file;
        let k = world.add(o, false);
        native.insert(ptr, k);
        k
    };
    // Where a stack-object scope's variable is in the original: back from its address to the
    // object's (the layouts in docs/objects.md).
    let owner_of = |v: &LookupRec| -> Option<u32> {
        let a = if v.ptr != 0 { v.ptr } else { v.fptr };
        if a == 0 {
            return None; // the value alone was asked for
        }
        let d = v.data as i32 as u32;
        match v.scope {
            4 => Some(a.wrapping_sub(0x26).wrapping_sub(d.wrapping_mul(2))),
            15 => Some(a.wrapping_add(0x598).wrapping_sub(d.wrapping_mul(4))),
            19 => Some(a.wrapping_add(0x638).wrapping_sub(d.wrapping_mul(2))),
            _ => None,
        }
    };
    // A lot loaded: the ticks at which 20 or more objects never seen before ask for variables
    // (the dream at 0, the club at 2799 in the new-game route).
    let mut lot_ticks: BTreeSet<u32> = BTreeSet::new();
    {
        let mut seen_objects: BTreeSet<u32> = BTreeSet::new();
        let mut fresh: BTreeMap<u32, i32> = BTreeMap::new();
        for s in &log.steps {
            for v in &s.lookups {
                if seen_objects.insert(v.object) {
                    *fresh.entry(s.tick).or_default() += 1;
                }
            }
        }
        lot_ticks.extend(fresh.iter().filter(|&(_, &n)| n >= 20).map(|(&t, _)| t));
    }
    let mut lot_tick = u32::MAX;
    for (i, s) in log.steps.iter().enumerate() {
        if s.lookups.is_empty() {
            continue;
        }
        // (a load's objects may come over two ticks: one lot)
        if lot_ticks.contains(&s.tick) && (lot_tick == u32::MAX || s.tick > lot_tick.wrapping_add(1)) {
            if lot_tick != u32::MAX {
                notes.note("variables: a lot loaded (the native world cleared)");
            }
            world.clear();
            native.clear();
            cell_addr.clear();
            addr_cell.clear();
            seen.clear();
            last_writer.clear();
            addr_used.clear();
        }
        if lot_ticks.contains(&s.tick) {
            if lot_tick == u32::MAX || s.tick > lot_tick.wrapping_add(1) {
                loaded_at = s.tick;
            }
            lot_tick = s.tick;
        }
        let mut frame = Frame { params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, file: step_object[i], ..Default::default() };
        for v in &s.lookups {
            let me = object_at(&mut world, &mut native, v.object, step_object[i]);
            world.get_mut(me).person |= v.scope == 14 || v.scope == 18;
            let sc = format!("scope {}", v.scope);
            if v.flags & 1 == 0 {
                notes.note(&format!("variables: {sc}: the original's lookup failed"));
                continue;
            }
            if v.scope == 7 || v.scope == 26 {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue;
                }
                let g = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                if g.is_some_and(|g| g.value == v.value) {
                    notes.note(&format!("variables: {sc}: same"));
                } else {
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: DIFFERENT"), s, v);
                }
                continue;
            }
            // A stack object the native world does not have yet: made from the original's.
            if let Some(q) = owner_of(v) {
                // the object at this address, met before without its id (made from another
                // lookup): it is this id's now (an id reused: the marker 197 after the chips, at
                // another address)
                if let Some(&b) = native.get(&q)
                    && world.get(b).id() != s.stack_object
                    && world.get(b).id() == 0
                {
                    let o = world.get_mut(b);
                    if o.data.len() <= DATA_OBJECT_ID {
                        o.data.resize(DATA_OBJECT_ID + 1, 0);
                    }
                    o.data[DATA_OBJECT_ID] = s.stack_object;
                    world.reindex(b);
                }
                let so = match native.get(&q).copied().filter(|&b| world.get(b).id() == s.stack_object).or(world.find(s.stack_object)) {
                    Some(so) => so,
                    None => {
                        let n = object_at(&mut world, &mut native, q, None);
                        let o = world.get_mut(n);
                        if o.data.len() <= DATA_OBJECT_ID {
                            o.data.resize(DATA_OBJECT_ID + 1, 0);
                        }
                        o.data[DATA_OBJECT_ID] = s.stack_object;
                        world.reindex(n);
                        n
                    }
                };
                world.get_mut(so).person |= v.scope == 15 || v.scope == 19;
                let bound = native.get(&q).copied();
                if bound == Some(so) {
                    notes.note("variables: stack object the same");
                } else {
                    differ(notes, &mut why, loaded_at, "variables: stack object DIFFERENT", s, v);
                    if shown < 6 {
                        shown += 1;
                        let b = bound.map_or_else(|| "(nil)".to_string(), |b| format!("#{}", b.0));
                        println!(
                            "  stack object {}: by id native #{} (id {}), at the original's address {:x} native {} (id {}); scope {} caller {:x}",
                            s.stack_object,
                            so.0,
                            world.get(so).id(),
                            q,
                            b,
                            bound.map_or(-1, |b| world.get(b).id() as i32),
                            v.scope,
                            v.caller
                        );
                    }
                }
                if bound.is_none() {
                    native.insert(q, so);
                }
            }
            let Some(cell) = ScriptVariables::new(&mut world, me, &mut frame).cell(v.scope as u8, v.data) else {
                // a scope with no cell of its own (a value the engine works out): its value
                // natively, where the native variables give one (study first: counted apart)
                // an object whose steps name no file (a multi-tile part's): its id's file and
                // definition in the lot (a definition's fields, scope 21)
                for k in [Some(me), world.find(frame.stack_object)].into_iter().flatten() {
                    if let Some(o) = world.get_live(k)
                        && o.file.is_none()
                        && let Some(l) = lot.find_object(o.id())
                    {
                        let (file, def) = (l.file, l.definition);
                        let o = world.get_mut(k);
                        o.file = file;
                        o.definition = def;
                        notes.note("variables: an object's file and definition from the lot (its steps name none)");
                    }
                }
                let got = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                notes.note(&match got {
                    Some(g) if v.flags & 1 != 0 && g.value == v.value => format!("variables: {sc}: no cell, the value the same natively"),
                    Some(g) if v.flags & 1 != 0 => {
                        if std::env::var("LOCKSTEP_SHOW_UNMODELLED").ok().and_then(|x| x.parse::<i16>().ok()) == Some(v.scope) {
                            let at = |k: Option<ObjectKey>| k.and_then(|k| world.get_live(k)).map(|o| (o.id(), o.x16, o.y16, o.level, o.person));
                            eprintln!("other scope {} datum {}: native {}, log {} at tick {}: me {:?}, stack object {} {:?}", v.scope, v.data, g.value, v.value, s.tick, at(Some(me)), frame.stack_object, at(world.find(frame.stack_object)));
                        }
                        format!("variables: {sc}: no cell, the value other natively (study)")
                    }
                    _ => {
                        if std::env::var("LOCKSTEP_SHOW_UNMODELLED").ok().and_then(|v| v.parse::<i16>().ok()) == Some(v.scope)
                            && let Some(o) = world.get_live(me)
                        {
                            let so = world.find_object(frame.stack_object);
                            eprintln!(
                                "unmodelled scope {} datum {} = {} at tick {}: me {} (person {}, def {:?}, file {:?}), stack object {} (def {:?})",
                                v.scope, v.data, v.value, s.tick, o.id(), o.person, o.definition, o.file, frame.stack_object, so.map(|o| o.definition)
                            );
                        }
                        format!("variables: {sc}: not modelled yet")
                    }
                });
                continue;
            };
            // The cell as the C++ tool keyed it: its table and index, every table of an object
            // the world does not have one (its null pointer).
            let key = CellKey(if cell.table.owner() == Some(None) { None } else { Some(cell.table) }, cell.index);
            let addr = if v.ptr != 0 { v.ptr } else { v.fptr };
            // Frame variables are the step's own: rebuilt from the log each step.
            let in_frame = v.scope == 9 || v.scope == 25 || v.scope == 10;
            if addr != 0 && !in_frame {
                // A binding unused for a long time is stale: the memory was reused, or the object
                // at a pointer replaced (another lot loaded). Forget it.
                let stale = |addr_used: &BTreeMap<u32, usize>, a: u32| addr_used.get(&a).is_none_or(|&u| i - u > 20000);
                if let Some(&c) = addr_cell.get(&addr)
                    && c != key
                    && stale(&addr_used, addr)
                {
                    cell_addr.remove(&c);
                    seen.remove(&c);
                    addr_cell.remove(&addr);
                    notes.note("variables: memory reused");
                }
                let owner_remade = cell.table.owner().flatten().and_then(|k| world.get_live(k)).is_some_and(|o| {
                    let t0 = bound_at.get(&key).copied().unwrap_or(0);
                    remade.get(&o.id()).is_some_and(|ts| ts.iter().any(|&t| t > t0 && t <= s.tick + 1))
                });
                if let Some(&a) = cell_addr.get(&key)
                    && a != addr
                    && (stale(&addr_used, a) || owner_remade)
                {
                    addr_cell.remove(&a);
                    seen.remove(&key);
                    cell_addr.remove(&key);
                    notes.note("variables: object replaced");
                }
                // An address first read while its object was not in the world yet (bound to no
                // owner: an engine-made object listed from the next tick, the club's 181 at tick
                // 6513) has no identity: once it is an object's cell, it is bound anew.
                if let Some(&c) = addr_cell.get(&addr)
                    && c.0.is_none()
                    && key.0.is_some()
                {
                    cell_addr.remove(&c);
                    seen.remove(&c);
                    addr_cell.remove(&addr);
                    notes.note("variables: an address read before its object was in the world, bound to it now");
                }
                let ca = cell_addr.get(&key).copied();
                let ac = addr_cell.get(&addr).copied();
                addr_used.insert(addr, i);
                if ca.is_none() && ac.is_none() {
                    cell_addr.insert(key, addr);
                    addr_cell.insert(addr, key);
                    bound_at.insert(key, s.tick);
                } else if ca.is_some_and(|a| a != addr) || ac.is_some_and(|c| c != key) {
                    if std::env::var_os("LOCKSTEP_SHOW_WHERE").is_some() {
                        let other = ac.map(|c| (c.0.and_then(|t| t.owner().flatten()).and_then(|k| world.get_live(k)).map(|o| o.id()), c.1, bound_at.get(&c).copied()));
                        eprintln!(
                            "where tick {} scope {} data {} me {} stack {} addr {addr:08x}: the cell (owner {:?}, index {}) bound to {ca:08x?} since {:?}; the address bound to {other:?}",
                            s.tick, v.scope, v.data, world.get(me).id(), frame.stack_object,
                            cell.table.owner().flatten().and_then(|k| world.get_live(k)).map(|o| o.id()), cell.index, bound_at.get(&key)
                        );
                    }
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: where DIFFERENT"), s, v);
                } else {
                    notes.note(&format!("variables: {sc}: where the same"));
                }
            }
            if in_frame {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue; // the address only
                }
                let g = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                if g.is_some_and(|g| g.value == v.value) {
                    notes.note(&format!("variables: {sc}: value the same"));
                } else {
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: value DIFFERENT"), s, v);
                }
                continue;
            }
            // The value: seeded once, then the native one must hold. Only the expression asks for
            // the value; other primitives take the address alone (the log's value is then not
            // one).
            if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                if addr != 0 && v.flags & 6 != 0 {
                    last_writer.insert(addr, v.caller);
                }
                seen.insert(key, false); // it may write there: take the next value read as the new one
                continue;
            }
            let is_float = v.flags & 4 != 0 && v.flags & 2 == 0;
            let mut vars = ScriptVariables::new(&mut world, me, &mut frame);
            let got = vars.get(v.scope as u8, v.data);
            let set = |vars: &mut ScriptVariables| {
                if is_float {
                    vars.set_float(v.scope as u8, v.data, f32::from_bits(v.fvalue));
                } else {
                    vars.set(v.scope as u8, v.data, v.value);
                }
            };
            if !*seen.entry(key).or_insert(false) {
                set(&mut vars);
                seen.insert(key, true);
                notes.note(&format!("variables: {sc}: first seen"));
            } else {
                let same = if is_float {
                    got.and_then(|g| g.f).unwrap_or(0.0).to_bits() == v.fvalue
                } else {
                    got.is_some_and(|g| g.value == v.value)
                };
                if same {
                    notes.note(&format!("variables: {sc}: value the same"));
                } else {
                    let other = last_writer.get(&addr).is_some_and(|&w| w != EXPR_LHS && w != EXPR_RHS);
                    notes.note(&format!(
                        "variables: {sc}{}",
                        if other { ": changed by a primitive not modelled yet" } else { ": changed by the engine" }
                    ));
                    set(&mut vars);
                }
            }
            if addr != 0 && v.flags & 6 != 0 {
                last_writer.insert(addr, v.caller);
            }
        }
        // The expression's write: the native cell takes the value after (the expression itself is
        // checked above).
        if let (Some(e), Some(v)) = (&s.expr, s.lookups.last())
            && v.caller == EXPR_LHS
        {
            let me = object_at(&mut world, &mut native, v.object, step_object[i]);
            let mut vars = ScriptVariables::new(&mut world, me, &mut frame);
            if e.flags & 1 != 0 {
                vars.set(v.scope as u8, v.data, e.lhs_after);
            } else if e.flags & 2 != 0 {
                vars.set_float(v.scope as u8, v.data, f32::from_bits(e.lhs_float_after));
            }
        }
    }
    for (k, m) in &why {
        println!("{k}:");
        let mut top: Vec<(i64, &String)> = m.iter().map(|(w, &n)| (n, w)).collect();
        top.sort();
        top.reverse();
        for (n, w) in top.iter().take(4) {
            println!("    {n:>6}  {w}");
        }
    }
}

/// SetToNext on a world loaded from the lot.
///
/// For each lot the log shows (a new lot: 20 or more objects never seen before ask for
/// variables in one tick), the disc's lot whose objects best match the ids seen then, loaded
/// natively; the objects that appear at the load and are not in the lot's table (two in the
/// club) go in front, not Sims. Each SetToNext step the original took on the stack object is
/// run natively on that world, from the step's frame.
/// The lot the log ran on, among the save's (or the disc's) lots: the one whose table order the
/// original's longest walk over every object follows furthest; with the lots, their ids, the walk
/// and how far it follows.
type LotPick = (Vec<housefire::save::LotChunks>, Vec<BTreeSet<i16>>, Vec<i16>, usize, usize);
fn pick_lot(lib: &Library, ngh: &[u8], log: &housefire::tree_log::Log) -> LotPick {
    let lots = read_lots(ngh).expect("the lots");
    let orders: Vec<Vec<i16>> = lots.iter().map(|l| World::new(lib).read_objects(l).unwrap().objects.iter().map(|o| o.id).collect()).collect();
    let lot_ids: Vec<BTreeSet<i16>> = orders.iter().map(|o| o.iter().copied().collect()).collect();
    // Which lot: the one whose table order the original's longest walk over every object
    // (SetToNext kind 0 from 0 on the stack object) follows furthest. Object ids are small
    // numbers every lot reuses, so they do not tell lots apart; the order does.
    let (mut walk, mut cur): (Vec<i16>, Vec<i16>) = (Vec::new(), Vec::new());
    let mut walking = false;
    let raw_of = |s: &StepRec| -> Option<&[u8]> {
        let t = log.tree_of(s)?;
        if s.look != b'X' || t.tree != s.tree || s.node as usize * 12 >= t.raw.len() {
            return None;
        }
        Some(&t.raw[12 * s.node as usize..12 * s.node as usize + 12])
    };
    for s in &log.steps {
        let Some(raw) = raw_of(s) else { continue };
        if (u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF) != 31 || (raw[8] & 0x7F) != 0 || raw[9] != 0x0A || raw[2] == raw[3] {
            continue;
        }
        if s.stack_object == 0 {
            cur.clear();
            walking = true;
        }
        if !walking {
            continue;
        }
        if s.after == raw[2] as u16 {
            cur.push(s.stack_object_after);
            if cur.len() > walk.len() {
                walk = cur.clone();
            }
        } else {
            walking = false;
        }
    }
    let (mut best, mut best_n) = (0, 0);
    for k in 0..lots.len() {
        let mut n = 0;
        for id in &walk {
            if !lot_ids[k].contains(id) {
                continue;
            }
            if n < orders[k].len() && orders[k][n] == *id {
                n += 1;
            } else {
                break;
            }
        }
        if n > best_n {
            best = k;
            best_n = n;
        }
    }
    (lots, lot_ids, walk, best, best_n)
}

/// The room walk's lists and the original's room objects paired with them (a lockstep study).
type RoomLists = (Vec<Vec<(i32, i32)>>, std::collections::BTreeMap<u32, usize>);

#[allow(clippy::too_many_arguments)]
/// The disc's tables lockstep's checks use: the fence styles, the wall patterns, the floor
/// patterns (SimsTiles) and the objects' resources by GUID (SimsObjects).
type DiscTables = (Vec<u32>, Vec<u32>, Vec<housefire::quickdata::FloorPattern>, std::collections::BTreeMap<u32, housefire::quickdata::ObjResources>);

fn set_to_next_check(lib: &Library, ngh: &[u8], disc_ngh: &[u8], tiles: &DiscTables, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], notes: &mut Notes) {
    let (lots, lot_ids, walk, best, best_n) = pick_lot(lib, ngh, log);
    let raw_of = |s: &StepRec| -> Option<&[u8]> {
        let t = log.tree_of(s)?;
        if s.look != b'X' || t.tree != s.tree || s.node as usize * 12 >= t.raw.len() {
            return None;
        }
        Some(&t.raw[12 * s.node as usize..12 * s.node as usize + 12])
    };
    let mut lw = World::new(lib);
    lw.load_lot(&lots[best]).unwrap();
    // Objects made at load: in the walk but not in the lot, in front in the walk's order (when
    // the log has the original's list, it is replayed instead, below).
    let made: Vec<i16> = if log.lists.is_empty() { walk.iter().copied().filter(|id| !lot_ids[best].contains(id)).collect() } else { Vec::new() };
    for &id in made.iter().rev() {
        let mut o = Object::new();
        o.data.resize(DATA_OBJECT_ID + 1, 0);
        o.data[DATA_OBJECT_ID] = id;
        lw.add(o, true);
    }
    notes.note(&buf(format!("SetToNext: the disc's lot {best} (the walk of {} follows its order for {best_n})", walk.len()), 96));
    // The lot's saved state against the original: the first read of each of an object's own
    // variables (data, attributes, temps), before any expression wrote it, must be the lot's
    // value. Objects by their id (data 11, read as they ask).
    {
        let mut id_of: BTreeMap<u32, i16> = BTreeMap::new(); // the original's object pointer -> id
        let mut done: BTreeSet<(u32, i32, i32)> = BTreeSet::new();
        let diffs = std::env::var_os("LOCKSTEP_LOT_DIFFS").is_some();
        // the tick an id first leaves the original's list (after it, the id may be another
        // object's, not the lot's)
        let mut left: BTreeMap<i16, u32> = BTreeMap::new();
        for w in log.lists.windows(2) {
            for id in w[0].ids.iter().filter(|id| !w[1].ids.contains(id)) {
                left.entry(*id).or_insert(w[1].tick);
            }
        }
        for s in &log.steps {
            for v in &s.lookups {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue;
                }
                if v.scope == 3 && v.data as usize == DATA_OBJECT_ID {
                    id_of.insert(v.object, v.value);
                }
                if v.scope == 21 && left.get(&s.stack_object).is_some_and(|&t| s.tick > t) {
                    notes.note("scope 21: the stack object's id left the lot before (another object's now)");
                    continue;
                }
                if v.scope == 21 {
                    // The stack object's definition: field `datum` of its OBJD.
                    let Some(so) = lw.find(s.stack_object).filter(|&k| lw.get(k).definition.is_some()) else {
                        notes.note("scope 21: stack object or its definition not known");
                        continue;
                    };
                    let mut sf = Frame { stack_object: s.stack_object, ..Default::default() };
                    let got = ScriptVariables::new(&mut lw, so, &mut sf).get(21, v.data); // the engine's own lookup
                    let f = got.map_or(-32768, |g| g.value);
                    let e = buf(format!("datum {} of {}: native {f}, original {}", v.data, s.stack_object, v.value), 96);
                    notes.note_ex(if f == v.value { "scope 21: the definition's field the same" } else { "scope 21: DIFFERENT" }, &e);
                    if f != v.value {
                        let file = lw.get(so).file.map_or("?".to_string(), |f| {
                            let p = &lib.file(f).path;
                            p[p.rfind('/').map_or(0, |s| s + 1)..].to_string()
                        });
                        notes.note(&format!(
                            "  scope 21 differs: datum {} object {} ({file}) native {f} original {}",
                            v.data, s.stack_object, v.value
                        ));
                    }
                    continue;
                }
                if !matches!(v.scope, 3 | 0 | 8 | 14 | 18) {
                    continue;
                }
                if !done.insert((v.object, v.scope as i32, v.data as i32)) {
                    continue;
                }
                let Some(&id) = id_of.get(&v.object) else {
                    notes.note("lot state: object not known yet");
                    continue;
                };
                let Some(o) = lw.find_object(id) else {
                    notes.note("lot state: object not in the lot");
                    continue;
                };
                if v.scope == 14 {
                    // a Sim's motive: the float the lot saved
                    if o.motives.is_empty() {
                        notes.note("lot state: scope 14: not a Sim of the lot");
                        continue;
                    }
                    let Some(&m) = usize::try_from(v.data).ok().and_then(|k| o.motives.get(k)) else {
                        notes.note("lot state: scope 14: past the table");
                        continue;
                    };
                    let f = f32::from_bits(v.fvalue);
                    notes.note(&format!(
                        "lot state: scope 14{}",
                        if m == f { ": the first read the lot's value" } else { ": the first read not the lot's (changed before it)" }
                    ));
                    if m != f && diffs {
                        eprintln!("scope 14 tick {} object {id} datum {}: lot {}, read {}", s.tick, v.data, g(m as f64, 6), g(f as f64, 6));
                    }
                    continue;
                }
                let t: Option<&Vec<i16>> = match v.scope {
                    3 => Some(&o.data),
                    0 => Some(&o.attributes),
                    18 => Some(&o.person_data),
                    _ => None,
                };
                if v.scope == 18 && o.person_data.is_empty() {
                    notes.note("lot state: scope 18: not a Sim of the lot");
                    continue;
                }
                let native = match t {
                    Some(t) => match usize::try_from(v.data).ok().and_then(|k| t.get(k)) {
                        Some(&x) => x,
                        None => {
                            notes.note(&format!("lot state: scope {}: past the table", v.scope));
                            continue;
                        }
                    },
                    None => {
                        if !(0..8).contains(&v.data) {
                            continue;
                        }
                        o.temps[v.data as usize]
                    }
                };
                notes.note(&format!(
                    "lot state: scope {}{}",
                    v.scope,
                    if native == v.value { ": the first read the lot's value" } else { ": the first read not the lot's (changed before it)" }
                ));
                if native != v.value && v.scope == 18 && diffs {
                    eprintln!("scope 18 tick {} object {id} datum {}: lot {native}, read {}", s.tick, v.data, v.value);
                }
            }
            // A written variable's first read after it is not the lot's to check.
            if s.expr.is_some()
                && let Some(l) = s.lookups.last()
                && l.caller == EXPR_LHS
            {
                done.insert((l.object, l.scope as i32, l.data as i32));
            }
        }
    }
    // No object of the world's: SetToNext reads the frame alone (scope 10, the parameters).
    let nobody = ObjectKey(u32::MAX);
    // Data fields 2 (the container) and 59 of each object as the log reads them (scopes 3 and 4:
    // the address back to the object, the field at +0x26 + 2 x field), by step: what DropOnto and
    // SetToNext kind 6 read, set at run time by an object's own scripts, so a made object's are
    // taken from the log.
    const LOGGED_FIELDS: [i16; 2] = [DATA_CONTAINER as i16, 59];
    let mut fields: BTreeMap<(u32, i16), Vec<(usize, i16)>> = BTreeMap::new();
    for (i, s) in log.steps.iter().enumerate() {
        for v in s.lookups.iter().filter(|v| matches!(v.scope, 3 | 4) && LOGGED_FIELDS.contains(&v.data) && v.flags & 1 != 0) {
            let a = if v.ptr != 0 { v.ptr } else { v.fptr };
            let owner = if a != 0 { a.wrapping_sub(0x26 + 2 * v.data as u32) } else if v.scope == 3 { v.object } else { continue };
            fields.entry((owner, v.data)).or_default().push((i, v.value));
        }
        // an expression writing one: the value after the step, the owner from the left side's
        // lookup in the same step
        if let Some(e) = s.expr.as_ref().filter(|e| matches!(e.lhs_scope, 3 | 4) && LOGGED_FIELDS.contains(&e.lhs_data) && e.flags & 1 != 0)
            && let Some(v) = s.lookups.iter().find(|v| v.scope == e.lhs_scope && v.data == e.lhs_data && v.ptr != 0)
        {
            fields.entry((v.ptr.wrapping_sub(0x26 + 2 * e.lhs_data as u32), e.lhs_data)).or_default().push((i, e.lhs_after));
        }
    }
    // a made object's logged fields as they stood before step i
    let set_made_fields = |lw: &mut World, made_ptr: &BTreeMap<i16, u32>, i: usize| {
        for (&id, &ptr) in made_ptr {
            let Some(k) = lw.find(id) else { continue };
            for f in LOGGED_FIELDS {
                if let Some(&(_, v)) = fields.get(&(ptr, f)).and_then(|r| r.iter().take_while(|e| e.0 < i).last()) {
                    let d = &mut lw.get_mut(k).data;
                    if d.len() <= f as usize {
                        d.resize(f as usize + 1, 0);
                    }
                    d[f as usize] = v;
                }
            }
        }
    };
    let mut made_ptr: BTreeMap<i16, u32> = BTreeMap::new(); // a made object's id -> its pointer
    // Each stack's object (its sim -> the object whose own variables it reads most) and each
    // object pointer's id in the lists.
    let ptr_id: BTreeMap<u32, i16> = log.lists.iter().flat_map(|l| l.objects.iter().copied().zip(l.ids.iter().copied())).collect();
    let sim_object: BTreeMap<u32, u32> = {
        let mut own: BTreeMap<u32, BTreeMap<u32, usize>> = BTreeMap::new();
        for s in &log.steps {
            for v in s.lookups.iter().filter(|v| matches!(v.scope, 0 | 3 | 8)) {
                *own.entry(s.sim).or_default().entry(v.object).or_default() += 1;
            }
        }
        own.into_iter().filter_map(|(sim, c)| c.into_iter().max_by_key(|e| e.1).map(|e| (sim, e.0))).collect()
    };
    let mut li = 0;
    let mut shown = 0;
    let mut place_at = log.places.iter().position(|p| p.tick > 0).unwrap_or(log.places.len()); // the next 'P' after the lot's
    // study (LOCKSTEP_DUMP_PLACES=TICK, LOCKSTEP_DUMP_OUT=PATH): this world's objects as the
    // step loop reaches the tick (id, GUID, place in sixteenths, level, facing, container, file),
    // one a line, then its multi-tile leader (0: none), for the assets tool's check of the original's world matrices
    let dump_at = std::env::var("LOCKSTEP_DUMP_PLACES").ok().and_then(|v| v.parse::<u32>().ok());
    let mut dumped = false;
    // (LOCKSTEP_ROOM_AMBIENT=LOG,SNAPSHOT: each tick's inside room ambient factors from the
    // engine's world as the step loop reaches the tick: the walk's lists, the openings rule on
    // the objects then, `window_brightness`, `room_ambient` with the '5' record's clock and
    // day/night values; against every '6' brightness and '5' result of the LOG (the same
    // replay), its room objects paired with the engine's lists through the snapshot's)
    let ambient_study = std::env::var("LOCKSTEP_ROOM_AMBIENT").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((read_log(a)?, std::fs::read(b).ok()?))
    });
    let mut ambient_lists: Option<RoomLists> = None;
    let (mut amb_k, mut amb_tick, mut amb_counts) = (0usize, u32::MAX, [0usize; 4]);
    let mut amb_shown = 0;
    for (i, s) in log.steps.iter().enumerate() {
        if let Some((alog, snap)) = &ambient_study
            && s.tick != amb_tick
        {
            amb_tick = s.tick;
            let (lists, by_room) = ambient_lists.get_or_insert_with(|| {
                let open = |x: i32, y: i32, dx: i32, dy: i32| !lw.walls.wall_into(x, y, dx, dy) || matches!(lw.walls.style(x, y, dx, dy), 2 | 12 | 13 | 14);
                let lists = housefire::light::room_tile_lists((39, 39), |x, y| lw.walls.diagonal_bits(x, y), open);
                let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| snap.get(o as usize).copied());
                let mut by_room = std::collections::BTreeMap::new();
                for w in &alog.window_brightness {
                    if let (Some(y), Some(x)) = (byte(w[2]), byte(w[2] + 1))
                        && let Some(k) = lists.iter().position(|l| l.first() == Some(&(i32::from(x), i32::from(y))))
                    {
                        by_room.insert(w[1], k);
                    }
                }
                (lists, by_room)
            });
            while amb_k < alog.window_brightness.len() && alog.window_brightness[amb_k][0] <= s.tick {
                let w = alog.window_brightness[amb_k];
                if w[0] == s.tick
                    && let Some(&k) = by_room.get(&w[1])
                {
                    let m = &lists[k];
                    let halves: i32 = m.iter().map(|&(x, y)| if lw.walls.diagonal_bits(x, y) != 0 { 1 } else { 2 }).sum();
                    // (the persons where the log's places put them before this tick: lockstep's
                    // world does not walk them)
                    let person_at = |id: i16| alog.places.iter().rev().find(|p| p.id == id && p.tick < s.tick).map(|p| (p.x >> 4, p.y >> 4, p.level));
                    let openings = m
                        .iter()
                        .filter(|&&(x, y)| {
                            housefire::light::tile_counts_as_opening(
                                lw.objects()
                                    .filter(|o| o.on_lot() && if o.person { person_at(o.id()) == Some((x, y, 1)) } else { o.level == 1 && (o.x16 >> 4, o.y16 >> 4) == (x, y) })
                                    .map(|o| (o.door || o.window, o.person, o.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0)),
                            )
                        })
                        .count() as u32;
                    let b = housefire::light::window_brightness(openings, halves / 2);
                    amb_counts[0] += 1;
                    if b.to_bits() == w[4] {
                        amb_counts[1] += 1;
                    } else if amb_shown < 5 {
                        amb_shown += 1;
                        println!("room ambient: tick {} room {:08x}: brightness {b}, the log's {}", w[0], w[1], f32::from_bits(w[4]));
                    }
                    if let Some(a) = alog.room_ambients.get(amb_k) {
                        let f = f32::from_bits;
                        let mine = housefire::light::room_ambient(f(a[4]), f(a[5]), a[6] as i32, a[7] as i32, a[8] as i32, b);
                        amb_counts[2] += 1;
                        if mine.to_bits() == a[9] {
                            amb_counts[3] += 1;
                        }
                    }
                }
                amb_k += 1;
            }
        }
        if ambient_study.is_some() && i + 1 == log.steps.len() {
            println!("room ambient: brightness from the engine {} of {}; ambient factors {} of {}", amb_counts[1], amb_counts[0], amb_counts[3], amb_counts[2]);
        }
        if let Some(t) = dump_at
            && !dumped
            && s.tick >= t
        {
            dumped = true;
            let mut out = String::new();
            for o in lw.objects() {
                let path = o.file.map_or("", |f| lib.file(f).path.as_str());
                out += &format!(
                    "{} {:08x} {} {} {} {} {} {} {} {}\n",
                    o.id(),
                    o.guid,
                    o.x16,
                    o.y16,
                    o.level,
                    o.data.get(DATA_FACING).copied().unwrap_or(0),
                    o.data.get(DATA_CONTAINER).copied().unwrap_or(0),
                    u8::from(o.person),
                    path,
                    o.leader
                );
            }
            let at = std::env::var("LOCKSTEP_DUMP_OUT").unwrap_or_else(|_| "work/logs/places.txt".into());
            if std::fs::write(&at, out).is_ok() {
                println!("places at tick {} written to {at}", s.tick);
            }
            // and the walls' pieces as the engine draws them (LOCKSTEP_DUMP_WALLS: x y angle
            // fence pattern cap opening form), a lot whose save keeps no walls (the club) with its house's, the disc's
            // copy's (as replay builds them; lockstep's own world untouched)
            if let Ok(at) = std::env::var("LOCKSTEP_DUMP_WALLS") {
                let house = (lw.walls.wall_edges() == 0)
                    .then(|| read_lots(disc_ngh).ok().and_then(|l| l.get(best).and_then(|h| housefire::save::read_lot_arrays(h).ok())))
                    .flatten()
                    .map(|a| housefire::routing::Walls::new(&a, &lw));
                let walls = house.as_ref().unwrap_or(&lw.walls);
                let mut out = String::new();
                // (a wall part's pattern as its shader's name hash, a fence's style)
                for p in housefire::scene::wall_pieces(walls, 64, &tiles.0) {
                    let shader = if p.fence { u32::from(p.pattern) } else { tiles.1.get(usize::from(p.pattern)).copied().unwrap_or(0) };
                    let opening = match p.opening {
                        housefire::routing::Opening::None => "plain",
                        housefire::routing::Opening::Door => "door",
                        housefire::routing::Opening::Window => "window",
                    };
                    let form = match p.form {
                        housefire::scene::Form::Straight => "straight",
                        housefire::scene::Form::LeftExtends => "left_extends",
                        housefire::scene::Form::RightExtends => "right_extends",
                    };
                    out += &format!("{} {} {} {} {shader:08x} {} {opening} {form}\n", p.x, p.y, p.angle, u8::from(p.fence), u8::from(p.cap));
                }
                let _ = std::fs::write(&at, out);
                // (and each tile's floor, LOCKSTEP_DUMP_FLOORS: x y value shader, the non-zero
                // ones; the shader FloorSet's, 0 for a value past its list)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_FLOORS") {
                    let mut out = String::new();
                    for y in 0..64 {
                        for x in 0..64 {
                            let v = walls.floor(x, y);
                            if v != 0 {
                                let shader = tiles.2.get(usize::from(v)).map_or(0, |p| p.shader);
                                out += &format!("{x} {y} {v} {shader:08x} {:x}\n", walls.diagonal_bits(x, y));
                            }
                        }
                    }
                    let _ = std::fs::write(&at, out);
                }
                // (LOCKSTEP_ROOM_LISTS=LOG[,SNAPSHOT]: the engine's room walk, `light::room_tile_lists`,
                // against the original's lists: the log's '7' records, or the '6' records' ranges
                // read from the snapshot; matched by their first tile, entry for entry)
                if let Ok(v) = std::env::var("LOCKSTEP_ROOM_LISTS") {
                    let mut parts = v.split(',');
                    let log = parts.next().and_then(read_log).expect("LOCKSTEP_ROOM_LISTS: the log");
                    let snap = parts.next().and_then(|p| std::fs::read(p).ok());
                    let mut theirs: Vec<Vec<(i32, i32)>> = log.room_tiles.iter().map(|r| r.3.iter().map(|e| (((e >> 8) & 0xff) as i32, (e >> 16) as i32)).collect()).collect();
                    let mut ptrs: Vec<u32> = log.room_tiles.iter().map(|r| r.1).collect();
                    if let Some(m) = &snap {
                        let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| m.get(o as usize).copied());
                        let mut seen = std::collections::BTreeSet::new();
                        for w in &log.window_brightness {
                            if seen.insert(w[1]) {
                                ptrs.push(w[1]);
                                theirs.push((w[2]..w[3]).step_by(3).filter_map(|a| Some((i32::from(byte(a + 1)?), i32::from(byte(a)?)))).collect());
                            }
                        }
                    }
                    let open = |x: i32, y: i32, dx: i32, dy: i32| !lw.walls.wall_into(x, y, dx, dy) || matches!(lw.walls.style(x, y, dx, dy), 2 | 12 | 13 | 14);
                    let mine = housefire::light::room_tile_lists((39, 39), |x, y| lw.walls.diagonal_bits(x, y), open);
                    println!("room lists: the engine's {}, the original's {}", mine.len(), theirs.len());
                    // (and each room's window brightness from the engine alone, against the '6'
                    // records of LOCKSTEP_ROOM_BRIGHTNESS=LOG nearest this tick: the openings rule on
                    // each entry, over the half count (2 a whole tile's entry, 1 a split tile's) / 2)
                    if let Some(blog) = std::env::var("LOCKSTEP_ROOM_BRIGHTNESS").ok().and_then(|p| read_log(&p)) {
                        for (t, ptr) in theirs.iter().zip(&ptrs) {
                            let Some(m) = t.first().and_then(|f| mine.iter().find(|m| m.first() == Some(f))) else { continue };
                            let halves: i32 = m.iter().map(|&(x, y)| if lw.walls.diagonal_bits(x, y) != 0 { 1 } else { 2 }).sum();
                            let openings = m
                                .iter()
                                .filter(|&&(x, y)| housefire::light::tile_counts_as_opening(lw.objects().filter(|w| w.on_lot() && w.level == 1 && (w.x16 >> 4, w.y16 >> 4) == (x, y)).map(|w| (w.door || w.window, w.person, w.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0))))
                                .count() as u32;
                            let b = housefire::light::window_brightness(openings, halves / 2);
                            let logged = blog.window_brightness.iter().filter(|w| w[1] == *ptr).min_by_key(|w| (i64::from(w[0]) - i64::from(s.tick)).abs());
                            let half_logged = blog.window_entries.iter().zip(&blog.window_brightness).find(|(_, w)| w[1] == *ptr).map(|(e, _)| e.0);
                            println!("  room {ptr:08x}: halves {halves} (the log's +72 {half_logged:?}), openings {openings}, brightness {b} {}", match logged {
                                Some(w) if w[4] == b.to_bits() => format!("as the log's at tick {}", w[0]),
                                Some(w) => format!("DIFFERENT: the log's {} at tick {}", f32::from_bits(w[4]), w[0]),
                                None => "(no record)".into(),
                            });
                        }
                    }
                    for t in &theirs {
                        let Some(m) = t.first().and_then(|f| mine.iter().find(|m| m.first() == Some(f))) else {
                            println!("  from {:?} ({} entries): no engine room starts there", t.first(), t.len());
                            continue;
                        };
                        let first_diff = t.iter().zip(m).position(|(a, b)| a != b);
                        if let Some(k) = first_diff.filter(|_| std::env::var_os("LOCKSTEP_ROOM_LISTS_AROUND").is_some()) {
                            let lo = k.saturating_sub(12);
                            println!("    the original's {lo}..: {:?}", &t[lo..(k + 6).min(t.len())]);
                            println!("    the engine's   {lo}..: {:?}", &m[lo..(k + 6).min(m.len())]);
                            for &(x, y) in &t[lo..(k + 6).min(t.len())] {
                                print!(" ({x},{y}): d{:#x} w{:?}", lw.walls.diagonal_bits(x, y), [(-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dx, dy)| (lw.walls.wall_between(x, y, dx, dy), lw.walls.pattern(x, y, dx, dy))));
                            }
                            println!();
                        }
                        println!("  from {:?}: the original's {} entries, the engine's {}{}", t.first(), t.len(), m.len(), match first_diff {
                            None if t.len() == m.len() => " (the same)".to_string(),
                            None => " (the same as far as the shorter)".to_string(),
                            Some(k) => format!(", first different at {k}: {:?} against {:?}", &t[k..(k + 4).min(t.len())], &m[k..(k + 4).min(m.len())]),
                        });
                    }
                }
                // (study, LOCKSTEP_ROOM_WINDOWS=LOG,SNAPSHOT: each room's tile list from the
                // snapshot (the '6' records' ranges of (x, y, level) bytes), its tile count (+72 / 2),
                // the engine's window objects on those tiles, against the window share the log's
                // brightness gives)
                if let Some((lp, sp)) = std::env::var("LOCKSTEP_ROOM_WINDOWS").ok().and_then(|v| v.split_once(',').map(|(a, b)| (a.to_string(), b.to_string()))) {
                    let (Some(log), Ok(m)) = (read_log(&lp), std::fs::read(&sp)) else { panic!("LOCKSTEP_ROOM_WINDOWS: unreadable") };
                    let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| m.get(o as usize).copied());
                    let word = |a: u32| (0..4).try_fold(0u32, |w, k| Some(w << 8 | u32::from(byte(a + k)?)));
                    let mut rooms = std::collections::BTreeMap::new();
                    for w in &log.window_brightness {
                        rooms.insert(w[1], (w[2], w[3], w[4]));
                    }
                    for (o, (b, e, r)) in rooms {
                        let tiles: Vec<(i32, i32, i32)> = (b..e).step_by(3).filter_map(|a| Some((i32::from(byte(a)?), i32::from(byte(a + 1)?), i32::from(byte(a + 2)?)))).collect();
                        let count = word(o + 72).map_or(0, |v| (v as i32) / 2);
                        let implied = (f32::from_bits(r) - 1.0) / 0.25 * count as f32;
                        let windows: Vec<String> = lw.objects().filter(|w| w.window && w.on_lot()).filter(|w| tiles.contains(&(w.x16 >> 4, w.y16 >> 4, w.level))).map(|w| format!("{}@{},{},{} f{} lead{}", w.id(), w.x16 >> 4, w.y16 >> 4, w.level, w.facing(), w.leader == w.id())).collect();
                        let all: Vec<String> = lw.objects().filter(|w| w.window && w.on_lot()).map(|w| format!("{},{},{}", w.x16 >> 4, w.y16 >> 4, w.level)).collect();
                        println!("room {o:08x}: {} tile entries, count {count}, implied windows {implied}; the engine's window objects on them {}: {windows:?}", tiles.len(), windows.len());
                        let _ = all;
                        // every object on the room's tile entries, counted per entry (as the loop goes)
                        let mut per: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
                        for t in &tiles {
                            for w in lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == *t) {
                                *per.entry(w.file.map_or("?".into(), |f| lib.file(f).path.clone())).or_default() += 1;
                            }
                        }
                        println!("  objects per entry {per:?}");
                        // the entries in the engine's axes with the engine's room of each
                        println!("  entries (x, y: room) {:?}", tiles.iter().map(|t| (t.1, t.0, lw.walls.room(t.1, t.0))).collect::<Vec<_>>());
                        // the definition fields that are 8 for some object on the entry, counted per entry
                        let mut by_field: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
                        for t in &tiles {
                            let mut fields = std::collections::BTreeSet::new();
                            for w in lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == *t) {
                                if let Some(d) = lw.definition(w) {
                                    for (k, v) in d.fields.iter().enumerate() {
                                        if *v == 8 {
                                            fields.insert(k);
                                        }
                                    }
                                }
                            }
                            for k in fields {
                                *by_field.entry(k).or_default() += 1;
                            }
                        }
                        println!("  entries with a field at 8, by field {by_field:?}");
                        // (logs with the entries: each kind 8 entry's tile against its opening's
                        // own tile and facing, by the log's id)
                        if let Some(k) = log.window_brightness.iter().position(|w| w[1] == o) {
                            let list = log.lists.iter().rev().find(|l| l.tick <= log.window_brightness[k][0]);
                            let mut per: std::collections::BTreeMap<i16, Vec<(i32, i32)>> = std::collections::BTreeMap::new();
                            for (t, e) in tiles.iter().zip(&log.window_entries[k].1) {
                                if e[2] as i32 != 8 {
                                    continue;
                                }
                                if let Some(id) = list.and_then(|l| l.objects.iter().position(|p| *p == e[1].wrapping_add(112)).map(|j| l.ids[j])) {
                                    per.entry(id).or_default().push((t.0, t.1));
                                }
                            }
                            // the engine's: entries (a, b) with a door or window standing at (b, a)
                            // (the entries' bytes are (y, x, level) in the engine's axes)
                            let counted = |t: &(i32, i32, i32)| {
                                housefire::light::tile_counts_as_opening(
                                    lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == (t.1, t.0, t.2)).map(|w| (w.door || w.window, w.person, w.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0)),
                                )
                            };
                            let mine = tiles.iter().filter(|t| counted(t)).count();
                            let theirs = log.window_entries[k].1.iter().filter(|e| e[2] as i32 == 8).count();
                            println!("    openings: the engine's {mine}, the log's {theirs}");
                            for (t, e) in tiles.iter().zip(&log.window_entries[k].1) {
                                let here: Vec<String> = lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == (t.1, t.0, t.2)).map(|w| format!("{}{}:{}", w.id(), if w.door || w.window { "*" } else { "" }, w.file.map_or("", |f| lib.file(f).path.rsplit('/').next().unwrap_or("")))).collect();
                                let opening = counted(t);
                                if opening != (e[2] as i32 == 8) {
                                    let id = list.and_then(|l| l.objects.iter().position(|p| *p == e[1].wrapping_add(112)).map(|j| l.ids[j]));
                                    println!("      tile {t:?}: the log's kind {} id {id:?} grid {}; the engine's objects there {here:?}", e[2] as i32, e[0] as i32);
                                }
                            }
                            for (id, ts) in per {
                                let me = lw.find_object(id).map(|w| (w.x16 >> 4, w.y16 >> 4, w.facing(), w.leader, w.file.map_or(String::new(), |f| lib.file(f).path.rsplit('/').next().unwrap_or("").to_string())));
                                println!("    opening {id} {me:?}: its entries' tiles {ts:?}");
                            }
                        }
                    }
                }
                // (and the windows' lights, LOCKSTEP_WINDOW_LIGHTS=LOG: the engine's places for each
                // window's leading object against the other lights of the log's last 't' record,
                // x and y bit for bit, as sets)
                if let Some(log) = std::env::var("LOCKSTEP_WINDOW_LIGHTS").ok().and_then(|p| read_log(&p)) {
                    let mut mine: Vec<(u32, u32)> = lw
                        .objects()
                        .filter(|o| o.window && o.on_lot() && o.leader == o.id())
                        .filter_map(|o| housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8))
                        .flatten()
                        .map(|(x, y)| (x.to_bits(), y.to_bits()))
                        .collect();
                    let mut theirs: Vec<(u32, u32)> = log.light_rebuilds.iter().rev().find(|b| b.rooms > 0).map_or(Vec::new(), |b| b.lights[..b.other as usize].iter().map(|l| (l[0], l[1])).collect());
                    if std::env::var_os("LOCKSTEP_WINDOW_FIELDS").is_some() {
                        let mut seen = std::collections::BTreeSet::new();
                        for o in lw.objects().filter(|o| o.on_lot() && o.leader == o.id() && (o.window || o.file.is_some_and(|f| lib.file(f).path.contains("window")))) {
                            let path = o.file.map_or("", |f| lib.file(f).path.as_str());
                            if seen.insert(path.to_string()) {
                                println!("  {path} window {}: fields {:?}", o.window, lw.definition(o).map(|d| d.fields.clone()));
                                println!("    data {:?}", o.data);
                                println!("    attributes {:?}", o.attributes);
                            }
                        }
                    }
                    mine.sort_unstable();
                    theirs.sort_unstable();
                    let same = mine.iter().filter(|p| theirs.contains(p)).count();
                    println!("window lights: the engine's {}, the log's {}, places the same {same}{}", mine.len(), theirs.len(), if mine == theirs { " (all)" } else { "" });
                    // (each window's kind from its model, against the log's 'y' kind of the light
                    // at the engine's place)
                    if let Some(b) = log.light_rebuilds.iter().rev().find(|b| b.rooms > 0) {
                        let (mut n, mut same_kind) = (0, 0);
                        // (and each light's nearest lights: the room of the tile under the window's
                        // other light (the wall's other side) less one, ranked over that room's own range at
                        // the grid build before each fill, against the fill's list)
                        let (mut fills, mut near_same, mut first_fills) = (0, 0, 0);
                        for o in lw.objects().filter(|o| o.window && o.on_lot() && o.leader == o.id()) {
                            let Some(places) = housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8) else { continue };
                            // (the group's model: a multi-tile object's resources are on one of its
                            // tiles' definitions, not the leader's)
                            let model = lw.objects().filter(|p| p.leader == o.id()).find_map(|p| tiles.3.get(&p.guid)).map(|r| r.key);
                            let kind = model.map_or(0, housefire::light::window_kind);
                            if std::env::var_os("LOCKSTEP_WINDOW_FIELDS").is_some() {
                                println!("  window {} guid {:08x}: key {model:08x?}", o.id(), o.guid);
                            }
                            for (side, (x, y)) in places.into_iter().enumerate() {
                                let (ox, oy) = places[1 - side];
                                let room = walls.room(ox.floor() as i32, oy.floor() as i32).wrapping_sub(1) as usize;
                                let Some(index) = b.lights.iter().position(|l| l[0] == x.to_bits() && l[1] == y.to_bits()) else { continue };
                                let light = &b.lights[index];
                                for w in log.window_fills.iter().filter(|w| w.light == index as u32) {
                                    let Some(g) = log.grid_builds.iter().rev().find(|g| g.tick < w.tick) else {
                                        first_fills += 1;
                                        continue;
                                    };
                                    fills += 1;
                                    let Some(r) = g.ranges.get(room) else { continue };
                                    let mine: Vec<u32> = housefire::light::nearest_lights(w.at, (r[2], r[3]), &g.lights).map_or(Vec::new(), |m| m.iter().map_while(|s| s.map(u32::from)).collect());
                                    if mine == w.near.iter().map(|x| x.0).collect::<Vec<_>>() {
                                        near_same += 1;
                                    } else if std::env::var_os("LOCKSTEP_NEAREST_SHOW").is_some() && w.tick < 3 {
                                        let fits: Vec<usize> = (0..16).filter(|&k| housefire::light::nearest_lights(w.at, (g.ranges[k][2], g.ranges[k][3]), &g.lights).is_some_and(|m| m.iter().map_while(|s| s.map(u32::from)).collect::<Vec<_>>() == w.near.iter().map(|x| x.0).collect::<Vec<_>>())).collect();
                                        let (tx, ty) = (o.x16 >> 4, o.y16 >> 4);
                                        println!("  light {index} window {} at ({tx},{ty}) facing {}: engine room {room}, fits {fits:?}; rooms around {:?}", o.id(), o.facing(), [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dy)| walls.room(tx + dx, ty + dy).wrapping_sub(1) as i32));
                                    }
                                }
                                let Some((h, _)) = log.window_lights.iter().find(|(h, _)| h[0] == light[7]) else { continue };
                                n += 1;
                                if h[6] == kind {
                                    same_kind += 1;
                                }
                            }
                        }
                        println!("window lights' kinds: {n} found, the engine's (by its resources' key) {same_kind}");
                        // (and the other lights' array whole: each window in list order, its two
                        // lights (front, behind), each in the room under its own place and facing
                        // the other's; sorted room by room (`sort_lights`); its x, y, height and
                        // reach against the rebuild's lights in order)
                        let mut objs = Vec::new();
                        let mut info = Vec::new();
                        for o in lw.objects().filter(|o| o.window && o.on_lot() && o.leader == o.id()) {
                            let Some(places) = housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8) else { continue };
                            let model = lw.objects().filter(|p| p.leader == o.id()).find_map(|p| tiles.3.get(&p.guid)).map(|r| r.key);
                            let kind = model.map_or(0, housefire::light::window_kind);
                            let room_at = |(x, y): (f32, f32)| i32::from(walls.room(x.floor() as i32, y.floor() as i32).wrapping_sub(1));
                            for side in 0..2 {
                                let (me, other) = (room_at(places[side]), room_at(places[1 - side]));
                                objs.push((housefire::light::LightKind::Window, me, other));
                                info.push((places[side], kind, me));
                            }
                        }
                        let rooms = b.rooms as usize;
                        let (_, order, _) = housefire::light::sort_lights(rooms, &objs);
                        let theirs = &b.lights[..b.other as usize];
                        let (mut placed, mut heights, mut reaches) = (0, 0, 0);
                        for (k, &i) in order.iter().enumerate() {
                            let ((x, y), kind, me) = info[i];
                            let Some(t) = theirs.get(k) else { break };
                            if (t[0], t[1]) == (x.to_bits(), y.to_bits()) {
                                placed += 1;
                            }
                            if let Some((reach, _, h)) = housefire::light::window_light_params(kind, me != 0) {
                                if h.map(f32::to_bits) == Some(t[2]) {
                                    heights += 1;
                                }
                                if reach.to_bits() == t[3] {
                                    reaches += 1;
                                } else if std::env::var_os("LOCKSTEP_NEAREST_SHOW").is_some() {
                                    println!("  other light {k}: reach {reach}, the log's {} (kind {kind}, room {me})", f32::from_bits(t[3]));
                                }
                            }
                        }
                        println!("other lights' array: the engine's {}, the log's {}; in order the same place {placed}, height {heights}, reach {reaches}", order.len(), theirs.len());
                        // (and the grid build after this rebuild from the engine: its cells' rooms
                        // (`cell_room_adds` through `CellRooms`), the other ranges and lights (the
                        // array above), the own ranges and lights still the log's (the lamps'
                        // places wait for the poses); each cell room's four lights by `cell_lights`)
                        let (ranges, _, _) = housefire::light::sort_lights(rooms, &objs);
                        if let Some(g) = b.builds_before.checked_sub(1).and_then(|i| log.grid_builds.get(i)) {
                            let mut cells = housefire::light::CellRooms::default();
                            for (x, y, r) in housefire::light::cell_room_adds((walls.size().0.min(39), walls.size().1.min(39)), |x, y| walls.room(x, y).wrapping_sub(1) as u8, |x, y| walls.diagonal_bits(x, y)) {
                                cells.add(x, y, r);
                            }
                            let mut lights = g.lights.clone();
                            for (k, &i) in order.iter().enumerate() {
                                let ((x, y), kind, me) = info[i];
                                if let (Some(l), Some((reach, _, Some(h)))) = (lights.get_mut(k), housefire::light::window_light_params(kind, me != 0)) {
                                    *l = ([x, y, h], reach);
                                }
                            }
                            let (mut cell_rooms, mut rooms_same, mut lit_same) = (0, 0, 0);
                            for (ci, cell) in g.cells.iter().enumerate() {
                                let (x, y) = (2 + ci / 36, 2 + ci % 36);
                                let mine = cells.at(x as i32, y as i32).unwrap_or([0xFF; 4]);
                                let ids = cell[0].to_be_bytes();
                                if mine == ids {
                                    rooms_same += 1;
                                }
                                for (k, &id) in mine.iter().enumerate() {
                                    if id == 0xFF {
                                        break;
                                    }
                                    cell_rooms += 1;
                                    let logged: [u16; 4] = {
                                        let (a, c) = (cell[1 + 2 * k], cell[2 + 2 * k]);
                                        [(a >> 16) as u16, a as u16, (c >> 16) as u16, c as u16]
                                    };
                                    let (Some(other), Some(own)) = (ranges.get(usize::from(id)), g.ranges.get(usize::from(id))) else { continue };
                                    let point = [x as f32, y as f32, housefire::light::GRID_Z];
                                    if housefire::light::cell_lights(point, (own[2], own[3]), (other[0], other[1]), &lights) == Some(logged) {
                                        lit_same += 1;
                                    }
                                }
                            }
                            println!("grid build at tick {} from the engine: cells {}, rooms the same {rooms_same}; cell rooms {cell_rooms}, the four lights the same {lit_same}", g.tick, g.cells.len());
                        }
                        println!("window lights' nearest lights: {fills} fills after a build ({first_fills} before any), from the other side's room {near_same}");
                    }
                }
                // (and the rooms the light grid is given at the load, LOCKSTEP_CELL_ROOMS=LOG: the
                // engine's adds against the log's 'f' records, add for add)
                if let Some(log) = std::env::var("LOCKSTEP_CELL_ROOMS").ok().and_then(|p| read_log(&p)) {
                    let mine = housefire::light::cell_room_adds((walls.size().0.min(39), walls.size().1.min(39)), |x, y| walls.room(x, y).wrapping_sub(1) as u8, |x, y| walls.diagonal_bits(x, y));
                    let theirs: Vec<(i32, i32, u8)> = log.cell_adds.iter().map(|a| (a[1] as i32, a[2] as i32, a[3] as u8)).collect();
                    let same = mine.iter().zip(&theirs).filter(|(a, b)| a == b).count();
                    let first = mine.iter().zip(&theirs).position(|(a, b)| a != b);
                    println!("light grid rooms: the engine's adds {}, the log's {}, the same in order {same} (first differing {first:?})", mine.len(), theirs.len());
                }
                // (and each tile's room, LOCKSTEP_DUMP_ROOMS: x y room diagonal bits, the whole
                // grid; the engine's rooms, 0 on a diagonal's tile and off the grid)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_ROOMS") {
                    let mut out = String::new();
                    for y in 0..64 {
                        for x in 0..64 {
                            out += &format!("{x} {y} {} {:x}\n", walls.room(x, y), walls.diagonal_bits(x, y));
                        }
                    }
                    let _ = std::fs::write(&at, out);
                }
                // (and the floor runs the engine draws, LOCKSTEP_DUMP_RUNS: x y n shader lead trail
                // value: the whole tiles, the half tiles' diagonal bits)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_RUNS") {
                    let mut out = String::new();
                    for r in housefire::scene::floor_runs(walls, 64, &tiles.2) {
                        out += &format!("{} {} {} {:08x} {:x} {:x} {}\n", r.x, r.y, r.n, tiles.2[usize::from(r.value)].shader, r.lead, r.trail, r.value);
                    }
                    let _ = std::fs::write(&at, out);
                }
            }
        }
        // The places the log has for the ticks before this step's (each the end of its tick),
        // against this world's for objects that are not persons: made, grabbed, dropped and
        // snapped natively (persons walk: routing's, not this world's)
        while place_at < log.places.len() && log.places[place_at].tick < s.tick {
            let p = log.places[place_at];
            place_at += 1;
            let Some(o) = lw.find_object(p.id).filter(|o| !o.person && !lw.find_object(p.id).is_some_and(|o| o.file.is_some_and(|f| lib.file(f).path.contains("template_person")))) else { continue };
            // a held object (container field an object) is at its holder's place: the holder's
            // as the log has it that tick (a person's place is routing's)
            let holder = o.data.get(DATA_CONTAINER).copied().filter(|&h| {
                h > 0 && h != p.id && lw.find_object(h).is_some_and(|c| c.person || c.file.is_some_and(|f| lib.file(f).path.contains("template_person")))
            });
            if let Some(h) = holder {
                let at = log.places.iter().rfind(|q| q.id == h && q.tick <= p.tick).map(|q| (q.x, q.y, q.level));
                notes.note(if at == Some((p.x, p.y, p.level)) {
                    "objects: a held object's place at a tick's end its holder's (the original's)"
                } else {
                    "objects: a held object's place at a tick's end NOT its holder's"
                });
                if at != Some((p.x, p.y, p.level)) && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                    println!("held {} by {h} at tick {}: holder {at:?}, original {:?}", p.id, p.tick, (p.x, p.y, p.level));
                }
                continue;
            }
            let same = (o.x16, o.y16, o.level) == (p.x, p.y, p.level);
            notes.note(if same { "objects: an object's place at a tick's end natively the original's (not a person)" } else { "objects: an object's place at a tick's end NOT the original's (not a person)" });
            if !same && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                println!("place of {} at tick {}: native {:?}, original {:?}", p.id, p.tick, (o.x16, o.y16, o.level), (p.x, p.y, p.level));
            }
        }
        // The disc's node (its operand in the file's order); SetToNext as the log's tree has it,
        // CreateObjectInstance by the disc's (its maker's tree has no record in the log).
        let disc_node = || step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
        let op = match raw_of(s) {
            Some(raw) if u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF == 31 && raw[2] != raw[3] => 31,
            _ if s.look == b'X' && disc_node().is_some_and(|n| matches!(n.opcode, 42 | 18)) => disc_node().unwrap().opcode,
            _ if disc_node().is_some_and(|n| matches!(n.opcode, 43 | 4 | 46)) => disc_node().unwrap().opcode,
            _ => continue,
        };
        let Some(node) = disc_node() else {
            continue;
        };
        if op == 31 && node.operand[4] & 0x80 != 0 && node.operand[5] != scope::STACK_OBJECT {
            continue;
        }
        // The objects made and removed, as the original's list shows them, replayed up to this
        // step's tick (each new id checked against the world's rule). An 'L' record is written
        // at the start of the next tick under the tick it ends, so a step sees the lists of the
        // ticks before its own.
        while li < log.lists.len() && log.lists[li].tick < s.tick {
            let l = &log.lists[li];
            li += 1;
            let now: BTreeSet<i16> = l.ids.iter().copied().collect();
            let gone: Vec<ObjectKey> = lw.order().iter().copied().filter(|&k| !now.contains(&lw.get(k).id())).collect();
            for k in gone {
                lw.remove(k);
                notes.note("objects: removed");
            }
            for &id in l.ids.iter().rev() {
                if lw.find(id).is_some() {
                    continue;
                }
                let want = lw.next_id();
                // its file: that of the first step after it appears that reads its own variables
                // (scopes 0, 3, 8 on its pointer, which the list has), and the GUID of the file's
                // definition
                let mut o = Object::new();
                let own = |s: &StepRec, ptr: u32| s.lookups.iter().any(|v| v.object == ptr && matches!(v.scope, 0 | 3 | 8));
                if let Some(ptr) = l.ids.iter().position(|&x| x == id).map(|p| l.objects[p]) {
                    made_ptr.insert(id, ptr);
                    o.file = log.steps.iter().enumerate().find(|(i, s)| s.tick >= l.tick && own(s, ptr) && step_object[*i].is_some()).and_then(|(i, _)| step_object[i]);
                    if let Some(d) = o.file.and_then(|f| lib.file(f).definitions.first()) {
                        o.guid = d.guid();
                        o.definition = lib.definition(o.guid);
                    }
                }
                if std::env::var_os("LOCKSTEP_SHOW_LISTS").is_some() {
                    println!("made {id} at tick {}: file {:?}, guid {:08x}", l.tick, o.file.map(|f| &lib.file(f).path), o.guid);
                }
                lw.make_object(o);
                notes.note(if want == id { "objects: made, the id the same" } else { "objects: made, the id DIFFERENT" });
                if want != id {
                    let k = lw.find(want).unwrap();
                    lw.get_mut(k).data[DATA_OBJECT_ID] = id;
                    lw.reindex(k);
                }
            }
            let order: Vec<i16> = lw.objects().map(Object::id).collect();
            notes.note(if order == l.ids { "objects: the list's order the same" } else { "objects: the list's order DIFFERENT" });
            if order != l.ids && std::env::var_os("LOCKSTEP_SHOW_LISTS").is_some() {
                let first = order.iter().zip(&l.ids).position(|(a, b)| a != b).unwrap_or(0);
                println!("list at tick {}: native {:?} log {:?} (from {first}, of {} and {})", l.tick, &order[first..(first + 6).min(order.len())], &l.ids[first..(first + 6).min(l.ids.len())], order.len(), l.ids.len());
            }
        }
        if op == 4 || op == 46 {
            // Grab and Snap natively on this world: the actor's place as the log has it last
            // before the step (persons' places are routing's, not this world's), then the moved
            // objects against the places the log has next
            set_made_fields(&mut lw, &made_ptr, i);
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            if let Some(p) = log.places.iter().rfind(|p| p.id == me && p.tick < s.tick)
                && let Some(k) = lw.find(me)
            {
                let o = lw.get_mut(k);
                (o.x16, o.y16, o.level) = (p.x, p.y, p.level);
            }
            let next = |id: i16| log.places.iter().find(|p| p.id == id && p.tick >= s.tick).map(|p| (p.x, p.y, p.level));
            let name = if op == 4 { "Grab" } else { "Snap" };
            if op == 46 && node.operand[0] != 0 {
                notes.note(&format!("objects: Snap kind {}: not modelled", node.operand[0]));
                continue;
            }
            if next(me).is_none() && op == 46 {
                notes.note("objects: Snap: no place logged after it");
                continue;
            }
            let same = if op == 4 {
                grab(&mut lw, me, s.stack_object).map(|(id, x, y, l)| next(id) == Some((x, y, l)))
            } else {
                let held: Vec<i16> = lw.objects().filter(|o| o.id() != me && o.data.get(DATA_CONTAINER) == Some(&me)).map(Object::id).collect();
                snap(&node, &mut lw, me, s.stack_object).map(|(x, y, l)| next(me) == Some((x, y, l)) && held.iter().all(|&h| next(h) == Some((x, y, l))))
            };
            if same != Some(true) && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                let so = lw.find_object(s.stack_object).map(|o| (o.x16, o.y16, o.level, o.file.map(|f| lib.file(f).path.clone())));
                println!("{name} at tick {} by {me} operand {:02x?}: stack object {} {so:?}; me next {:?}; me now {:?}", s.tick, node.operand, s.stack_object, next(me), lw.find_object(me).map(|o| (o.x16, o.y16)));
            }
            notes.note(&match same {
                Some(true) => format!("objects: {name} natively, the moved objects' places the original's"),
                Some(false) => format!("objects: {name} natively, NOT the original's"),
                None => format!("objects: {name}: its objects not found natively"),
            });
            continue;
        }
        if op == 43 {
            // DropOnto natively on this world: the object the actor holds to the stack object's
            // tile centre, against the place the log has for it next
            set_made_fields(&mut lw, &made_ptr, i);
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some((id, x, y, level)) = drop_onto(&mut lw, me, s.stack_object) else {
                notes.note("objects: DropOnto: nothing held or no stack object natively");
                continue;
            };
            let logged = log.places.iter().find(|p| p.id == id && p.tick >= s.tick).map(|p| (p.x, p.y, p.level));
            notes.note(if logged == Some((x, y, level)) { "objects: DropOnto natively, the held object and its place the original's" } else { "objects: DropOnto natively, NOT the original's" });
            continue;
        }
        if op == 18 {
            // RemoveObjectInstance natively on this world: operand byte 0 1 the stack object, 0
            // the step's own object (its sim's: the object whose own variables it reads most);
            // the next list must then lack it
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some(id) = remove_object(&node, &mut lw, me, s.stack_object) else {
                notes.note("objects: RemoveObjectInstance: its object not found natively");
                continue;
            };
            let next = log.lists.iter().find(|l| l.tick >= s.tick);
            notes.note(if next.is_some_and(|l| !l.ids.contains(&id)) {
                "objects: RemoveObjectInstance natively, the object the original removed"
            } else {
                "objects: RemoveObjectInstance natively, NOT an object the original removed"
            });
            continue;
        }
        if op == 42 {
            // CreateObjectInstance natively on this world: the new object's id against the
            // maker's stack object after, its GUID against the file its own steps run (the next
            // list then finds it made)
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some(k) = create_object(&node, &mut lw, me, s.stack_object) else {
                notes.note("objects: CreateObjectInstance: no definition of the GUID");
                continue;
            };
            let (id, guid) = (lw.get(k).id(), lw.get(k).guid);
            if std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
                let at = |id: i16| lw.find_object(id).map(|o| (o.id(), o.x16, o.y16, o.level, o.data.get(1).copied()));
                let first = log.places.iter().find(|p| p.id == id && p.tick + 1 >= s.tick).map(|p| (p.tick, p.x, p.y, p.level));
                let slot0 = lw.find_object(s.stack_object).and_then(|o| {
                    let d = o.definition.map(|d| lib.def(d))?;
                    lib.file(o.file?).slots.get(&d.slot_table()).map(|t| t.iter().filter(|s| s.kind != 3).map(|s| (s.kind, s.x, s.y, s.z, s.facing)).take(2).collect::<Vec<_>>())
                });
                println!("made {id} at tick {} operand {:02x?}: maker {:?} (me {:?}), stack object before {:?} (first container slots {slot0:?}); its first place {first:?}", s.tick, node.operand, at(me), me, at(s.stack_object));
            }
            notes.note(if id == s.stack_object_after { "objects: CreateObjectInstance natively, the id the original's" } else { "objects: CreateObjectInstance natively, the id DIFFERENT" });
            // the object's pointer: the one the next list has at that id
            let ptr = log.lists.iter().find(|l| l.tick >= s.tick).and_then(|l| l.ids.iter().position(|&x| x == id).map(|p| l.objects[p]));
            let own = |t: &StepRec, ptr: u32| t.lookups.iter().any(|v| v.object == ptr && matches!(v.scope, 0 | 3 | 8));
            let file = ptr.and_then(|p| log.steps.iter().enumerate().skip(i.saturating_sub(400)).find(|(j, t)| own(t, p) && step_object[*j].is_some()).and_then(|(j, _)| step_object[j]));
            let file_guid = file.and_then(|f| lib.file(f).definitions.first()).map(|d| d.guid());
            notes.note(if file_guid == Some(guid) { "objects: CreateObjectInstance natively, the GUID its own steps' file's" } else { "objects: CreateObjectInstance natively, the GUID NOT its steps' file's" });
            if let Some(p) = ptr {
                made_ptr.insert(id, p);
            }
            continue;
        }
        let mut f = Frame { params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, ..Default::default() };
        if node.operand[4] & 0x7F == 6 {
            set_made_fields(&mut lw, &made_ptr, i);
        }
        // me, the step's own object (kind 9 searches beside it)
        let me_key = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).and_then(|&id| lw.find(id)).unwrap_or(nobody);
        let r = set_to_next(&node, &mut ScriptVariables::new(&mut lw, me_key, &mut f));
        let Some(raw) = raw_of(s) else { continue };
        let orig_true = s.after == raw[2] as u16;
        let kind = format!("SetToNext kind {}", node.operand[4] & 0x7F);
        if r == PrimResult::Stop {
            notes.note(&format!("{kind}: not modelled"));
            continue;
        }
        let same = (r == PrimResult::True) == orig_true && (!orig_true || f.stack_object == s.stack_object_after);
        let ex = buf(
            format!(
                "tick {} from {}: original {} {}, native {} {}",
                s.tick,
                s.stack_object,
                if orig_true { "true" } else { "false" },
                s.stack_object_after,
                if r == PrimResult::True { "true" } else { "false" },
                f.stack_object
            ),
            128,
        );
        // While the lot loads (its first ticks) the original's list is still being built, in
        // another order (a global init tree walks the objects of a GUID in ascending id order
        // then); the native world has the finished list: counted apart.
        let verdict = if same {
            ": same"
        } else if s.tick < 10 {
            ": different while the lot loads (ticks 0-9)"
        } else {
            ": DIFFERENT"
        };
        notes.note_ex(&format!("{kind}{verdict}"), &ex);
        if !same && s.tick >= 10 && shown < std::env::var("LOCKSTEP_SHOW_N").ok().and_then(|v| v.parse().ok()).unwrap_or(12) {
            shown += 1;
            let o = &node.operand;
            let guid = u32::from_le_bytes([o[0], o[1], o[2], o[3]]);
            let pos = |id: i16| lw.objects().position(|x| x.id() == id).map_or(-1, |p| p as i64);
            println!(
                "  {kind} tick {} tree {} node {} GUID {guid:08x}: from {} (at {}), original {} (at {}, GUID {:08x})",
                s.tick,
                alt_hex(s.tree as u64),
                s.node,
                s.stack_object,
                pos(s.stack_object),
                s.stack_object_after,
                pos(s.stack_object_after),
                lw.find_object(s.stack_object_after).map_or(0, |x| x.guid)
            );
        }
    }
}
