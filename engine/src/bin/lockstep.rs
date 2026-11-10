//! The native interpreter against the original's, step by step: a log of the recompiled Bustin'
//! Out's interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: lockstep LOG ISO
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
use housefire::primitives::set_to_next;
use housefire::save::read_lots;
use housefire::tree_log::{ExprRec, LookupRec, StepRec, read_log, same_shape};
use housefire::world::{DATA_OBJECT_ID, Object, ObjectKey, ScriptVariables, Table, World, scope};
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
    defaults: Vec<i16>,
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
    fn default_params(&self) -> &[i16] {
        &self.defaults
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
    let lib = Library::bustin_out(&d);
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
            let mut h = LogHost { lib: &lib, object: Some(object), answer: PrimResult::Stop, defaults: s.defaults.to_vec() };
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
                    _ if s.after == n.true_target as u16 => PrimResult::True,
                    _ => PrimResult::False,
                };
            }
            let mut interp = Interpreter::new(if native.len() == 1 { Mode::Check } else { Mode::Main }, 10000);
            interp.restore(native.clone());
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

    variables(&lib, &log, &step_object, &mut notes);
    set_to_next_check(&lib, &d, &log, &step_object, &mut notes);

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
fn variables(lib: &Library, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], notes: &mut Notes) {
    let mut world = World::new(lib);
    let mut native: BTreeMap<u32, ObjectKey> = BTreeMap::new(); // the original's object pointer -> native object
    let mut cell_addr: HashMap<CellKey, u32> = HashMap::new();
    let mut addr_cell: BTreeMap<u32, CellKey> = BTreeMap::new();
    let mut seen: HashMap<CellKey, bool> = HashMap::new();
    let mut last_writer: BTreeMap<u32, u32> = BTreeMap::new(); // address -> the caller that last had its pointer
    let mut addr_used: BTreeMap<u32, usize> = BTreeMap::new(); // address -> the last step that reached it
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
                let so = match world.find(s.stack_object) {
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
                notes.note(&format!("variables: {sc}: not modelled yet"));
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
                if let Some(&a) = cell_addr.get(&key)
                    && a != addr
                    && stale(&addr_used, a)
                {
                    addr_cell.remove(&a);
                    seen.remove(&key);
                    cell_addr.remove(&key);
                    notes.note("variables: object replaced");
                }
                let ca = cell_addr.get(&key).copied();
                let ac = addr_cell.get(&addr).copied();
                addr_used.insert(addr, i);
                if ca.is_none() && ac.is_none() {
                    cell_addr.insert(key, addr);
                    addr_cell.insert(addr, key);
                } else if ca.is_some_and(|a| a != addr) || ac.is_some_and(|c| c != key) {
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
fn set_to_next_check(lib: &Library, d: &Disc, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], notes: &mut Notes) {
    let lots = read_lots(&d.read("Runtime/default.ngh").unwrap_or_default()).expect("the disc's lots");
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
        for s in &log.steps {
            for v in &s.lookups {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue;
                }
                if v.scope == 3 && v.data as usize == DATA_OBJECT_ID {
                    id_of.insert(v.object, v.value);
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
    let mut li = 0;
    let mut shown = 0;
    for (i, s) in log.steps.iter().enumerate() {
        let Some(raw) = raw_of(s) else { continue };
        if (u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF) != 31 || raw[2] == raw[3] {
            continue;
        }
        // The disc's node (its operand in the file's order).
        let Some(node) = step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied() else {
            continue;
        };
        if node.operand[4] & 0x80 != 0 && node.operand[5] != scope::STACK_OBJECT {
            continue;
        }
        let mut f = Frame { params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, ..Default::default() };
        // The objects made and removed, as the original's list shows them, replayed up to this
        // step's tick (each new id checked against the world's rule).
        while li < log.lists.len() && log.lists[li].tick <= s.tick {
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
                lw.make_object(Object::new());
                notes.note(if want == id { "objects: made, the id the same" } else { "objects: made, the id DIFFERENT" });
                if want != id {
                    let k = lw.find(want).unwrap();
                    lw.get_mut(k).data[DATA_OBJECT_ID] = id;
                    lw.reindex(k);
                }
            }
            let order: Vec<i16> = lw.objects().map(Object::id).collect();
            notes.note(if order == l.ids { "objects: the list's order the same" } else { "objects: the list's order DIFFERENT" });
        }
        let r = set_to_next(&node, &mut ScriptVariables::new(&mut lw, nobody, &mut f));
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
        if !same && s.tick >= 10 && shown < 12 {
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
