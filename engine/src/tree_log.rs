//! A log of the recompiled Bustin' Out's interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG),
//! read for the engine's checks (lockstep, replay).

use std::collections::BTreeMap;

use crate::bhav::Tree;

/// A tree as the original had it loaded: its nodes' address, id, file, and its nodes' raw bytes.
#[derive(Clone, Debug, Default)]
pub struct TreeRec {
    pub nodes: u32,
    pub tree: u16,
    pub file: u32,
    pub raw: Vec<u8>,
}

/// The original's expression during a step: what its variable lookup found for each side.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExprRec {
    pub lhs_scope: i16,
    pub lhs_data: i16,
    pub rhs_scope: i16,
    pub rhs_data: i16,
    pub op: u8,
    /// 1 a 16-bit pointer on the left, 2 a float one, 4/8 a lookup failed.
    pub flags: u8,
    pub rhs: i16,
    pub lhs: i16,
    pub lhs_after: i16,
    pub lhs_float: u32,
    pub lhs_float_after: u32,
    pub result: u32,
}

/// A variable lookup of the original's (any primitive's).
#[derive(Clone, Copy, Debug, Default)]
pub struct LookupRec {
    pub object: u32,
    pub caller: u32,
    pub scope: i16,
    pub data: i16,
    pub value: i16,
    /// 1 found, 2 a 16-bit pointer, 4 a float one.
    pub flags: u8,
    pub ptr: u32,
    pub fptr: u32,
    pub fvalue: u32,
}

#[derive(Clone, Debug, Default)]
pub struct StepRec {
    pub sim: u32,
    pub frame: u32,
    pub file: u32,
    pub nodes: u32,
    pub tree: u16,
    pub node: u16,
    pub after: u16,
    pub error: u16,
    pub look: u8,
    pub result: u8,
    pub params: Vec<i16>,
    pub locals: Vec<i16>,
    pub params_after: Vec<i16>,
    pub locals_after: Vec<i16>,
    /// The interpreter's own parameters (its +44), for calls giving none.
    pub defaults: [i16; 8],
    /// The frame's (+4), before and after.
    pub stack_object: i16,
    pub stack_object_after: i16,
    /// The tree GetNodeRef found, as loaded at the time: its place in `Log::trees`.
    pub t: Option<usize>,
    pub expr: Option<ExprRec>,
    /// Made during the step, in order.
    pub lookups: Vec<LookupRec>,
    pub tick: u32,
    /// A turn in the tick's pass (logs with 'Q' records).
    pub in_pass: bool,
}

/// The original's object list, when it changed: each object's address and id.
#[derive(Clone, Debug, Default)]
pub struct ListRec {
    pub tick: u32,
    pub objects: Vec<u32>,
    pub ids: Vec<i16>,
}

/// The tick's pass over the objects: the id its cursor goes on from (0: a new pass), and the id
/// it stopped at (0: it went through).
#[derive(Clone, Copy, Debug, Default)]
pub struct PassRec {
    pub from: u32,
    pub stopped_at: u32,
}

/// An object's place when it changed, at the start of a tick (the 'P' record): sixteenths of a
/// tile, and its level; -16, -16 off the lot.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaceRec {
    pub tick: u32,
    pub id: i16,
    pub x: i32,
    pub y: i32,
    pub level: i32,
}

/// An autonomy score's call (the 'A' record): the motives it read, the adverts (16 of
/// personality, minimum, delta; big-endian in the record), its score.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScoreRec {
    pub tick: u32,
    pub score: f32,
    pub motives: [f32; 16],
    pub adverts: [[i16; 3]; 16],
}

/// A base score ('B').
#[derive(Clone, Copy, Debug, Default)]
pub struct BaseRec {
    pub tick: u32,
    pub score: f32,
}

/// An entry of autonomy's sorted list.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChoiceEntry {
    pub id: i16,
    pub interaction: u16,
    pub value: f32,
    pub takes_best: u32,
}

/// Autonomy's sorted list ('Y': the sort is recursive, so a tick's last is the whole list).
#[derive(Clone, Debug, Default)]
pub struct ChoiceListRec {
    pub tick: u32,
    pub entries: Vec<ChoiceEntry>,
}

/// A route the original planned (the 'G' record, Route_Plan): its waypoints, sixteenths of a
/// tile.
#[derive(Clone, Debug, Default)]
pub struct PlanRec {
    pub tick: u32,
    pub state: u32,
    pub points: Vec<(i32, i32)>,
}

/// The random number autonomy's choice draws ('R').
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomRec {
    pub tick: u32,
    pub site: u8,
    pub value: u32,
}

/// A person's 16 motives when they changed, at the start of a tick (the 'N' record).
#[derive(Clone, Copy, Debug, Default)]
pub struct MotiveRec {
    pub tick: u32,
    pub id: i16,
    pub motives: [f32; 16],
}

#[derive(Clone, Debug, Default)]
pub struct Log {
    pub trees: Vec<TreeRec>,
    pub lists: Vec<ListRec>,
    /// Tick -> its pass over the objects (logs with 'Q' records).
    pub passes: BTreeMap<u32, PassRec>,
    pub steps: Vec<StepRec>,
    pub places: Vec<PlaceRec>,
    pub motives: Vec<MotiveRec>,
    pub scores: Vec<ScoreRec>,
    pub bases: Vec<BaseRec>,
    pub choice_lists: Vec<ChoiceListRec>,
    pub randoms: Vec<RandomRec>,
    pub plans: Vec<PlanRec>,
    pub ticks: u32,
}

impl Log {
    /// The tree record a step names.
    pub fn tree_of(&self, s: &StepRec) -> Option<&TreeRec> {
        s.t.map(|t| &self.trees[t])
    }
}

struct Bytes<'a> {
    b: &'a [u8],
    p: usize,
}

impl Bytes<'_> {
    fn has(&self, n: usize) -> bool {
        self.p + n <= self.b.len()
    }
    fn u8(&mut self) -> u8 {
        self.p += 1;
        self.b[self.p - 1]
    }
    fn u16(&mut self) -> u16 {
        self.p += 2;
        u16::from_le_bytes([self.b[self.p - 2], self.b[self.p - 1]])
    }
    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    fn u32(&mut self) -> u32 {
        self.p += 4;
        u32::from_le_bytes(self.b[self.p - 4..self.p].try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
}

/// The log at `path`; `None` if it cannot be read or has a record of a kind not known (which
/// is reported, as the C++ reader did).
pub fn read_log(path: &str) -> Option<Log> {
    let b = std::fs::read(path).ok()?;
    let mut r = Bytes { b: &b, p: 0 };
    let mut log = Log::default();
    let mut at: BTreeMap<u32, usize> = BTreeMap::new();
    let mut pending: Option<ExprRec> = None;
    let mut pending_lookups: Vec<LookupRec> = Vec::new();
    let mut in_pass = false;
    while r.p < b.len() {
        let k = r.u8();
        match k {
            b'K' => {
                if !r.has(4) {
                    break;
                }
                r.u32();
                log.ticks += 1;
            }
            b'L' => {
                // the object list, when it changed
                if !r.has(6) {
                    break;
                }
                let tick = r.u32();
                let cnt = r.u16() as usize;
                if !r.has(6 * cnt) {
                    break;
                }
                let mut l = ListRec { tick, ..Default::default() };
                for _ in 0..cnt {
                    l.objects.push(r.u32());
                    l.ids.push(r.i16());
                }
                log.lists.push(l);
            }
            b'Q' => {
                // the pass over the objects starts: its cursor
                if !r.has(4) {
                    break;
                }
                log.passes.entry(log.ticks).or_default().from = r.u32();
                in_pass = true;
            }
            b'q' => {
                // the pass returns: 0, or the id it stopped at
                if !r.has(4) {
                    break;
                }
                log.passes.entry(log.ticks).or_default().stopped_at = r.u32();
                in_pass = false;
            }
            b'B' => {
                if !r.has(8) {
                    break;
                }
                r.u32();
                log.bases.push(BaseRec { tick: log.ticks, score: r.f32() });
            }
            b'Y' => {
                // autonomy's sorted list
                if !r.has(2) {
                    break;
                }
                let cnt = r.u16() as usize;
                if !r.has(16 * cnt) {
                    break;
                }
                let mut l = ChoiceListRec { tick: log.ticks, entries: Vec::with_capacity(cnt) };
                for _ in 0..cnt {
                    let id = r.i16();
                    r.u16();
                    let interaction = (r.u32() & 0xFFFF) as u16;
                    let value = r.f32();
                    l.entries.push(ChoiceEntry { id, interaction, value, takes_best: r.u32() });
                }
                log.choice_lists.push(l);
            }
            b'G' => {
                // a route planned
                if !r.has(10) {
                    break;
                }
                let state = r.u32();
                r.u32();
                let cnt = r.u16() as usize;
                if !r.has(8 * cnt) {
                    break;
                }
                let points = (0..cnt).map(|_| (r.u32() as i32, r.u32() as i32)).collect();
                log.plans.push(PlanRec { tick: log.ticks, state, points });
            }
            b'R' => {
                // the random number autonomy's choice draws
                if !r.has(5) {
                    break;
                }
                let site = r.u8();
                log.randoms.push(RandomRec { tick: log.ticks, site, value: r.u32() });
            }
            b'W' => {
                // an evaluator's curves (tools/tree_log.py)
                if !r.has(6) {
                    break;
                }
                r.p += 4;
                let cnt = r.u16() as usize;
                if !r.has(cnt * (8 + 256 + 128)) {
                    break;
                }
                r.p += cnt * (8 + 256 + 128);
            }
            b'A' => {
                // an autonomy score
                if !r.has(12 + 64 + 132) {
                    break;
                }
                r.u32();
                r.u32();
                let mut s = ScoreRec { tick: log.ticks, score: r.f32(), ..Default::default() };
                for m in &mut s.motives {
                    *m = r.f32();
                }
                let a = &b[r.p..];
                for m in 0..16 {
                    for k2 in 0..3 {
                        s.adverts[m][k2] = i16::from_be_bytes([a[4 + 8 * m + 2 * k2], a[4 + 8 * m + 2 * k2 + 1]]);
                    }
                }
                r.p += 132;
                log.scores.push(s);
            }
            b'U' => {
                // a call of the motive update's gate (tools/tree_log.py)
                if !r.has(7) {
                    break;
                }
                r.p += 7;
            }
            b'N' => {
                // a person's motives changed (before the tick's 'K')
                if !r.has(66) {
                    break;
                }
                let mut m = MotiveRec { tick: log.ticks, id: r.i16(), ..Default::default() };
                for v in &mut m.motives {
                    *v = r.f32();
                }
                log.motives.push(m);
            }
            b'P' => {
                // an object's place changed (before the tick's 'K')
                if !r.has(14) {
                    break;
                }
                let id = r.i16();
                let (x, y, level) = (r.u32() as i32, r.u32() as i32, r.u32() as i32);
                log.places.push(PlaceRec { tick: log.ticks, id, x, y, level });
            }
            b'C' => {
                if !r.has(6) {
                    break;
                }
                r.p += 6;
            }
            b'M' => {
                // an object's memory (tools/tree_log.py)
                if !r.has(4 + 0x1100) {
                    break;
                }
                r.p += 4 + 0x1100;
            }
            b'O' => {
                // where an object keeps its definition (tools/definitions.py)
                if !r.has(4) {
                    break;
                }
                r.p += 4;
                let mut ended = false;
                while r.has(2) {
                    if r.i16() == 0x7FFF {
                        ended = true;
                        break;
                    }
                    if !r.has(202) {
                        break;
                    }
                    r.p += 202;
                }
                if !ended {
                    break;
                }
            }
            b'V' => {
                if !r.has(27) {
                    break;
                }
                pending_lookups.push(LookupRec {
                    object: r.u32(),
                    caller: r.u32(),
                    scope: r.i16(),
                    data: r.i16(),
                    value: r.i16(),
                    flags: r.u8(),
                    ptr: r.u32(),
                    fptr: r.u32(),
                    fvalue: r.u32(),
                });
            }
            b'E' => {
                if !r.has(28) {
                    break;
                }
                pending = Some(ExprRec {
                    lhs_scope: r.i16(),
                    lhs_data: r.i16(),
                    rhs_scope: r.i16(),
                    rhs_data: r.i16(),
                    op: r.u8(),
                    flags: r.u8(),
                    rhs: r.i16(),
                    lhs: r.i16(),
                    lhs_after: r.i16(),
                    lhs_float: r.u32(),
                    lhs_float_after: r.u32(),
                    result: r.u32(),
                });
            }
            b'T' => {
                if !r.has(12) {
                    break;
                }
                let nodes = r.u32();
                let tree = r.u16();
                let n = r.u16() as usize;
                let file = r.u32();
                if !r.has(n * 12) {
                    break;
                }
                let raw = b[r.p..r.p + n * 12].to_vec();
                r.p += n * 12;
                at.insert(nodes, log.trees.len());
                log.trees.push(TreeRec { nodes, tree, file, raw });
            }
            b'S' => {
                if !r.has(28) {
                    break;
                }
                let mut s = StepRec { sim: r.u32(), frame: r.u32(), file: r.u32(), nodes: r.u32(), ..Default::default() };
                s.tree = r.u16() & 0x7FFF;
                s.node = r.u16();
                s.after = r.u16();
                s.look = r.u8();
                s.result = r.u8();
                s.error = r.u16();
                let (np, nl) = (r.u8() as usize, r.u8() as usize);
                if !r.has(4 * (np + nl)) {
                    break;
                }
                s.params = (0..np).map(|_| r.i16()).collect();
                s.locals = (0..nl).map(|_| r.i16()).collect();
                s.params_after = (0..np).map(|_| r.i16()).collect();
                s.locals_after = (0..nl).map(|_| r.i16()).collect();
                if !r.has(20) {
                    break;
                }
                for d in &mut s.defaults {
                    *d = r.i16();
                }
                s.stack_object = r.i16();
                s.stack_object_after = r.i16();
                s.t = at.get(&s.nodes).copied();
                s.expr = pending.take();
                s.tick = log.ticks;
                s.in_pass = in_pass;
                s.lookups = std::mem::take(&mut pending_lookups);
                log.steps.push(s);
            }
            _ => {
                println!("bad record at {}", r.p - 1);
                return None;
            }
        }
    }
    Some(log)
}

/// The nodes as the original has them in memory: opcode, targets, and a call's operand (four
/// 16-bit fields; a primitive's operand fields are reordered on loading by their widths, which
/// are not all known, so those are not compared). Variants of an object can differ only in a
/// call's operand.
pub fn same_shape(t: &Tree, raw: &[u8]) -> bool {
    if raw.len() != t.nodes.len() * 12 {
        return false;
    }
    t.nodes.iter().zip(raw.as_chunks::<12>().0).all(|(n, r)| {
        u16::from_be_bytes([r[0], r[1]]) == n.opcode
            && r[2] == n.true_target
            && r[3] == n.false_target
            && ((n.opcode & 0x7FFF) <= 255 || (0..4).all(|k| u16::from_be_bytes([r[4 + 2 * k], r[5 + 2 * k]]) == n.operand16(k)))
    })
}
