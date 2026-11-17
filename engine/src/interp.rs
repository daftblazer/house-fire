//! The behaviour interpreter's core: frames, calls, returns and the node step, as
//! `docs/interpreter.md` describes the engine's (TreeSimImpl::DoNodeAction). The primitives are a
//! host's: each object (and person) answers them, and finds the trees it may call.

use crate::bhav::{Node, RETURN_FALSE, RETURN_TRUE, TARGET_ERROR, Tree};
use crate::{FileId, TreeId};

/// What a primitive answers (the engine's values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PrimResult {
    False = 0,
    True = 1,
    Yield = 2,
    Continue = 3,
    Stop = 4,
}

/// The node step's error codes (stored by the engine at +52 of its interpreter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Error {
    None = 0,
    /// A call to a tree that exists could not be made.
    CallFailed = 1000,
    /// The bottom frame returned (the main tree must not).
    ReturnFromBottom = 1001,
    /// A target of 253, or a return value other than 254 or 255.
    BadTransition = 1100,
    TooManyIterations = 1101,
    /// No such node, or no such tree.
    Missing = 1102,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// `None` for a saved frame naming a tree the files lack.
    pub tree: Option<TreeId>,
    pub node: u16,
    pub params: Vec<i16>,
    pub locals: Vec<i16>,
    /// Locals start at zero (the original leaves whatever its stack's memory held: see
    /// `docs/interpreter.md`, "The call"). Bit k is set while local k has not been written in
    /// this frame, so that a read of one is reported ([`Host::unset_local`]) and a script that
    /// depends on the original's left-over values can be found. Locals past 63 are not tracked.
    pub unset: u64,
    /// The stack object (scope 10): an object id, the frame's own, a call's taken from its
    /// caller (the original keeps it in the frame's header, +4).
    pub stack_object: i16,
    /// The object file the frame's trees are found in and its constants read from (the
    /// original's +12): a call's is its caller's.
    pub file: Option<FileId>,
    /// A frame the engine started rather than a script (the original's +8).
    pub engine: bool,
}

/// Where the trees a [`TreeId`] names are kept: the library's, or a test's own.
pub trait Trees {
    fn tree_at(&self, t: TreeId) -> &Tree;
}

impl Trees for crate::library::Library {
    fn tree_at(&self, t: TreeId) -> &Tree {
        crate::library::Library::tree_at(self, t)
    }
}

pub trait Host: Trees {
    /// The tree a call names (`tree_kind` says which file); `None` if there is none.
    fn tree(&mut self, id: u16) -> Option<TreeId>;
    /// The primitive at `node`, for the interpreter's top frame.
    fn primitive(&mut self, interp: &mut Interpreter, node: &Node) -> PrimResult;
    /// Runs a check tree to its return on its own stack ([`Mode::Check`]), with this stack
    /// object and file, parameter 0 set to 1, and the interpreter's own parameters (`defaults`)
    /// for its calls: its result, or `None`
    /// if the host cannot (the engine's own checks: the interaction queue's prefix check, "hide
    /// menu").
    fn run_check(&mut self, _t: TreeId, _file: Option<FileId>, _stack_object: i16, _defaults: &[i16]) -> Option<bool> {
        None
    }
    /// A primitive read local k of frame `frame` before anything wrote it
    /// (`tools/locals_check.py` finds the scripts that can do this; on the disc they read zero
    /// in the original too).
    fn unset_local(&mut self, _interp: &Interpreter, _frame: usize, _k: usize) {}
    /// The check tree ad range (scope 12), if the host keeps one: a candidate's guard starts
    /// with it as its interaction's advertisement and may change it (autonomy's evaluation).
    fn ad_range_mut(&mut self) -> Option<&mut Vec<i16>> {
        None
    }
    /// The parameters a call that gives none passes, as they are now: the running object's temps
    /// (the original's interpreter keeps a pointer to them, so a guard that writes a temp and then
    /// calls passes the new value). `None`: the interpreter's own copy (`set_defaults`).
    fn default_params(&mut self) -> Option<[i16; 8]> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Go,
    Yield,
    Stop,
}

/// A main tree's bottom frame may not return (error 1001); a check tree's return is its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Main,
    Check,
}

/// A stack's sleep (Idle): passed over by the next `left` passes of the tick that reach it,
/// then stepped again. A pass that stops early does not reach the objects after it, so their
/// sleep does not count down (`docs/simulation.md`). Woken early (`wake_early`: an expression
/// touched the object's lockout), it is stepped at once with what was left.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sleep {
    left: u32,
    sleeping: bool,
    early: bool,
    /// The Idle's parameter (its index in the frame), which an early wake sets to what is left.
    param: usize,
    /// The sleeping frame's depth (the stack's length at the Idle): frames run on top of it (a
    /// tree an engine call runs on the object, as DropOnto's placement tree) leave it asleep.
    depth: usize,
}

impl Sleep {
    pub fn sleep(&mut self, visits: u32, param: usize) {
        self.left = visits;
        self.sleeping = true;
        self.early = false;
        self.param = param;
    }
    pub fn asleep(&self) -> bool {
        self.sleeping && self.left > 0 && !self.early
    }
    /// A pass reached the sleeping stack.
    pub fn pass_over(&mut self) {
        self.left = self.left.saturating_sub(1);
    }
    /// A pass over it taken back: the tick's pass reached it after a tree ran over the sleep.
    pub fn unpass(&mut self) {
        self.left += 1;
    }
    /// Slept, and is being stepped again.
    pub fn woke(&self) -> bool {
        self.sleeping
    }
    /// Woken before its time.
    pub fn early(&self) -> bool {
        self.early
    }
    /// Woken before its time (an expression touched the object's lockout): the frame's
    /// parameter is set to the passes left, and the next pass steps it.
    pub fn wake_early(&mut self, f: &mut Frame) {
        if self.asleep() {
            self.early = true;
            if let Some(p) = f.params.get_mut(self.param) {
                *p = self.left.min(i16::MAX as u32) as i16;
            }
        }
    }
    /// Its sleep ended now (NotifyStackObject): the next step finds it run out.
    pub fn end(&mut self) {
        self.sleeping = true;
        self.early = false;
        self.left = 0;
    }
    pub fn wake(&mut self) {
        self.sleeping = false;
        self.early = false;
        self.left = 0;
    }
}

#[derive(Clone, Debug)]
pub struct Interpreter {
    mode: Mode,
    max_iterations: u32,
    iterations: u32,
    stack: Vec<Frame>,
    last_result: bool,
    finished: bool,
    error: Error,
    sleep: Sleep,
    defaults: Vec<i16>,
}

impl Interpreter {
    pub fn new(mode: Mode, max_iterations: u32) -> Self {
        Interpreter {
            mode,
            max_iterations,
            iterations: 0,
            stack: Vec::new(),
            last_result: false,
            finished: false,
            error: Error::None,
            sleep: Sleep::default(),
            defaults: Vec::new(),
        }
    }

    /// A main tree's interpreter, with the engine's iteration limit.
    pub fn main() -> Self {
        Self::new(Mode::Main, 10000)
    }

    /// Start `t` as the bottom frame with these parameters. A tree that cannot start (none, no
    /// nodes, a type-4 tree's negative node) ends at once, as a call to it fails.
    pub fn start<T: Trees + ?Sized>(&mut self, trees: &T, t: Option<TreeId>, params: &[i16], file: Option<FileId>) {
        self.stack.clear();
        self.finished = false;
        self.error = Error::None;
        self.iterations = 0;
        if t.is_some_and(|t| self.push(trees, t, params)) {
            self.top().file = file;
        } else {
            self.fail(Error::CallFailed);
        }
    }

    /// Push a call (what a call node does, and what a primitive may do before Continue), with
    /// these parameters (the tree's count of them is taken, the rest zero). A tree of type 4
    /// starts at the node its first parameter names (the last if past the end); false if that
    /// is negative or the tree has no nodes.
    ///
    /// The engine's call (Bustin' Out's fn_8023EE1C, TreeSimImpl::Gosub): the frame's header is
    /// made from the tree (its parameter and local counts), the parameters given are copied in,
    /// and the first node is 0, or for a tree of type 4 the first parameter.
    pub fn push<T: Trees + ?Sized>(&mut self, trees: &T, t: TreeId, params: &[i16]) -> bool {
        let tree = trees.tree_at(t);
        let count = tree.nodes.len() as i32;
        let mut node = 0i32;
        if tree.kind == 4 {
            node = params.first().map_or(0, |&p| p as i32).min(count - 1);
        }
        if node < 0 || node >= count {
            return false;
        }
        let mut p = vec![0i16; tree.args as usize];
        for (d, s) in p.iter_mut().zip(params) {
            *d = *s;
        }
        // The original does not clear a new frame's locals: they are what its stack's memory
        // held. Here they are zero, and marked unset until written (Frame::unset).
        let locals = tree.locals as usize;
        let caller = self.stack.last();
        let frame = Frame {
            tree: Some(t),
            node: node as u16,
            params: p,
            locals: vec![0; locals],
            unset: if locals >= 64 { !0 } else { (1u64 << locals) - 1 },
            stack_object: caller.map_or(0, |c| c.stack_object),
            file: caller.and_then(|c| c.file),
            engine: false,
        };
        self.stack.push(frame);
        true
    }

    /// Take over a stack as it stands (a saved game's, or the original's in a lockstep check).
    pub fn restore(&mut self, frames: Vec<Frame>) {
        self.stack = frames;
        self.finished = false;
        self.error = Error::None;
    }

    fn fail(&mut self, e: Error) -> Step {
        self.error = e;
        self.finished = true;
        Step::Stop
    }

    /// A node's true or false target becomes the top frame's node; 253 is the error exit.
    fn transition<T: Trees + ?Sized>(&mut self, trees: &T, value: bool) -> Step {
        let f = self.stack.last().expect("a frame");
        // A frame whose tree was not found (a saved frame naming a tree the files lack): an
        // error, as the original's missing tree, never a read through nothing.
        let Some(n) = f.tree.and_then(|t| trees.tree_at(t).nodes.get(f.node as usize)) else {
            return self.fail(Error::Missing);
        };
        let t = if value { n.true_target } else { n.false_target };
        if t == TARGET_ERROR {
            return self.fail(Error::BadTransition);
        }
        self.top().node = t as u16;
        Step::Go
    }

    pub fn step<H: Host + ?Sized>(&mut self, host: &mut H) -> Step {
        if self.finished || self.stack.is_empty() {
            return Step::Stop;
        }
        let f = self.stack.last().unwrap();

        // A return.
        if f.node > 250 {
            if f.node != RETURN_TRUE as u16 && f.node != RETURN_FALSE as u16 {
                return self.fail(Error::BadTransition);
            }
            let value = f.node == RETURN_TRUE as u16;
            self.last_result = value;
            if self.stack.len() <= 1 {
                if self.mode == Mode::Main {
                    return self.fail(Error::ReturnFromBottom);
                }
                self.stack.pop();
                self.finished = true;
                return Step::Stop;
            }
            let engine = self.stack.pop().is_some_and(|f| f.engine);
            // A frame the engine pushed itself (an object's update of its function scores over
            // its Idle, the counter's 8215) returns into the frame below without moving it on:
            // its node runs again on a later turn.
            if engine {
                return Step::Yield;
            }
            // An interaction IdleForInput delivered comes back to that node, which runs again as
            // a step of its own (the log: 4106's return, then 281 node 4 to 2), not on by the
            // result; so does a door's portal function a routing step pushed (GotoRelativePosition
            // 27, GotoRoutingSlot 45: the home log's 8215 returning to 8210 node 0 at tick 521),
            // which plans again from the door's far side.
            let caller = self.stack.last().unwrap();
            if caller.tree.and_then(|t| host.tree_at(t).nodes.get(caller.node as usize)).is_some_and(|n| matches!(n.opcode, 17 | 27 | 45)) {
                return Step::Go;
            }
            return self.transition(host, value);
        }

        let Some(n) = f.tree.and_then(|t| host.tree_at(t).nodes.get(f.node as usize)).copied() else {
            return self.fail(Error::Missing);
        };
        self.iterations += 1;
        if self.iterations > self.max_iterations {
            return self.fail(Error::TooManyIterations);
        }
        let op = n.opcode & 0x7FFF;

        // A call: the operand's four fields are the parameters, unless both its words are -1.
        if op > 255 {
            let Some(t) = host.tree(op) else { return self.fail(Error::Missing) };
            let pushed = if n.operand.iter().all(|&b| b == 0xFF) {
                if let Some(d) = host.default_params() {
                    self.push(host, t, &d)
                } else {
                    let d = std::mem::take(&mut self.defaults);
                    let pushed = self.push(host, t, &d);
                    self.defaults = d;
                    pushed
                }
            } else {
                let p: [i16; 4] = std::array::from_fn(|i| n.operand16(i) as i16);
                self.push(host, t, &p)
            };
            return if pushed { Step::Go } else { self.fail(Error::CallFailed) };
        }

        // A primitive.
        match host.primitive(self, &n) {
            PrimResult::True => self.transition(host, true),
            PrimResult::False => self.transition(host, false),
            PrimResult::Yield => Step::Yield,
            PrimResult::Continue => Step::Go,
            PrimResult::Stop => {
                self.finished = true;
                Step::Stop
            }
        }
    }

    /// Steps until the tree yields or stops; then the number of steps run.
    pub fn run<H: Host + ?Sized>(&mut self, host: &mut H, max_steps: u32) -> u32 {
        for i in 0..max_steps {
            if self.step(host) != Step::Go {
                return i + 1;
            }
        }
        max_steps
    }

    /// Frame `frame`'s local k, for the primitives: a read of an unset one is reported to the
    /// host.
    pub fn local<H: Host + ?Sized>(&self, host: &mut H, frame: usize, k: usize) -> i16 {
        let f = &self.stack[frame];
        if k < 64 && (f.unset >> k & 1) != 0 {
            host.unset_local(self, frame, k);
        }
        f.locals.get(k).copied().unwrap_or(0)
    }

    pub fn set_local(&mut self, frame: usize, k: usize, v: i16) {
        let f = &mut self.stack[frame];
        if let Some(l) = f.locals.get_mut(k) {
            *l = v;
            if k < 64 {
                f.unset &= !(1u64 << k);
            }
        }
    }

    /// The parameters a call that gives none passes: the interpreter's own, given when it is set
    /// up (TreeSimImpl::Initialize keeps a pointer at +44; Bustin' Out's call, fn_8023EE1C,
    /// copies from it). In the PC game they are the object's temporaries.
    pub fn defaults(&self) -> &[i16] {
        &self.defaults
    }
    pub fn set_defaults(&mut self, d: Vec<i16>) {
        self.defaults = d;
    }
    /// The same, copied into the buffer already held (a turn's, every tick).
    pub fn copy_defaults(&mut self, d: &[i16]) {
        self.defaults.clear();
        self.defaults.extend_from_slice(d);
    }

    /// The stack's sleep (Idle).
    pub fn sleep(&mut self) -> &mut Sleep {
        &mut self.sleep
    }
    /// Asleep, its sleeping frame on top (a frame run over it is stepped).
    pub fn asleep(&self) -> bool {
        self.sleep.asleep() && self.stack.len() == self.sleep.depth
    }
    pub fn pass_over(&mut self) {
        self.sleep.pass_over()
    }
    /// Asleep under a frame run over it (an engine call's tree on the object).
    pub fn asleep_under(&self) -> bool {
        self.sleep.asleep() && self.stack.len() > self.sleep.depth
    }
    /// The sleep and the top frame together, for Idle (the top frame the sleeping one).
    pub fn sleep_and_top(&mut self) -> (&mut Sleep, &mut Frame) {
        self.sleep.depth = self.stack.len();
        (&mut self.sleep, self.stack.last_mut().expect("a frame"))
    }
    /// The sleep and its frame (the sleeping one, under any run over it; else the top), for a
    /// wake.
    pub fn sleep_and_sleeper(&mut self) -> (&mut Sleep, &mut Frame) {
        let k = if self.sleep.sleeping && (1..=self.stack.len()).contains(&self.sleep.depth) { self.sleep.depth - 1 } else { self.stack.len() - 1 };
        (&mut self.sleep, &mut self.stack[k])
    }

    pub fn done(&self) -> bool {
        self.finished
    }
    /// The last return (or a check tree's answer).
    pub fn result(&self) -> bool {
        self.last_result
    }
    pub fn error(&self) -> Error {
        self.error
    }
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    pub fn stack(&self) -> &[Frame] {
        &self.stack
    }
    /// The top frame's index.
    pub fn top_index(&self) -> usize {
        self.stack.len() - 1
    }
    pub fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("a frame")
    }
    pub fn reset_iterations(&mut self) {
        self.iterations = 0;
    }
}
