//! The script machine: the ActionScript opcodes the movies use, the variables a script sees,
//! members of clips, objects and arrays, and the few built-in functions the movies call.

use std::rc::Rc;

use super::actions::{Instr, Op, Value as Const};
use super::player::{ClipKind, Player};
use super::value::{ClipId, Func, Kind, Native, ObjRef, Object, Props, Value, loose_eq, parse_num, strict_eq};

/// A script's running state: its stack, the registers, the function's local variables (none
/// for a frame's own script, whose variables are the clip's), `this`, the clip the code's
/// variables belong to, the constant pool, and the objects of enclosing `with`s.
pub(super) struct Cx {
    stack: Vec<Value>,
    regs: [Value; 4],
    locals: Option<Props>,
    this: Value,
    target: ClipId,
    pool: Rc<Vec<Const>>,
    with: Vec<(Value, usize)>,
}

/// The most instructions one call or action may run, and the deepest call (a runaway script is a
/// bug in the player, not a hang).
const MAX_STEPS: u64 = 50_000_000;
const MAX_DEPTH: u32 = 200;

impl Cx {
    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Undefined)
    }

    fn pop_num(&mut self) -> f64 {
        self.pop().to_num()
    }
}

fn obj(kind: Kind) -> Value {
    Value::Obj(Object::new(kind))
}

impl Player {
    pub(super) fn init_globals(&mut self) {
        let g = &mut self.globals;
        g.set("extern", Value::Obj(self.extern_obj.clone()));
        g.set("Math", obj(Kind::Native(Native::Math)));
        g.set("Key", obj(Kind::Native(Native::Key)));
        for c in ["Array", "Object", "String", "Number"] {
            g.set(c, obj(Kind::Native(Native::Class(c))));
        }
    }

    /// Runs a frame's script for a clip.
    pub(super) fn run_top(&mut self, clip: ClipId, code: &[Instr]) {
        let mut cx = Cx {
            stack: Vec::new(),
            regs: Default::default(),
            locals: None,
            this: Value::Clip(clip),
            target: clip,
            pool: Rc::new(Vec::new()),
            with: Vec::new(),
        };
        self.steps = 0;
        self.run(code, &mut cx);
    }

    /// Calls a script function.
    pub(super) fn call_func(&mut self, f: &Rc<Func>, this: Value, args: Vec<Value>) -> Value {
        if self.depth >= MAX_DEPTH {
            self.note_stub("calls nested too deeply".into());
            return Value::Undefined;
        }
        let mut locals = Props::default();
        // A parameter the call does not pass is not declared: its name then reads and writes
        // the variable of the clip (the wrapper's buttons rely on it: `createButton` sets
        // `orientation` for the buttons made after it). Checked against the game's commands.
        for (p, v) in f.params.iter().zip(args) {
            locals.set(p, v);
        }
        let mut cx = Cx {
            stack: Vec::new(),
            regs: Default::default(),
            locals: Some(locals),
            this,
            target: f.scope,
            pool: f.pool.clone(),
            with: Vec::new(),
        };
        self.depth += 1;
        let r = self.run(&f.body, &mut cx);
        self.depth -= 1;
        r.unwrap_or_default()
    }

    fn run(&mut self, code: &[Instr], cx: &mut Cx) -> Option<Value> {
        let mut pc = 0;
        while pc < code.len() {
            while cx.with.last().is_some_and(|w| pc >= w.1) {
                cx.with.pop();
            }
            self.steps += 1;
            if self.steps > MAX_STEPS {
                self.note_stub("a script ran away".into());
                return None;
            }
            let ins = &code[pc];
            pc += 1;
            match ins {
                Instr::Push(vs) => {
                    for v in vs {
                        match v {
                            Const::Register(r) => cx.stack.push(cx.regs.get(*r as usize).cloned().unwrap_or_default()),
                            v => cx.stack.push(Self::pool_value(&cx.pool, v)),
                        }
                    }
                }
                Instr::ConstantPool(vs) => cx.pool = Rc::new(vs.clone()),
                Instr::SetRegister(r) => {
                    let v = cx.stack.last().cloned().unwrap_or_default();
                    if let Some(slot) = cx.regs.get_mut(*r as usize) {
                        *slot = v;
                    }
                }
                Instr::BranchAlways(t) => pc = *t,
                Instr::BranchIfTrue(t) => {
                    if cx.pop().truthy() {
                        pc = *t;
                    }
                }
                Instr::With { until, .. } => {
                    let o = cx.pop();
                    cx.with.push((o, *until));
                }
                Instr::DefineFunction { name, params, body } => {
                    let f = Func {
                        name: name.clone(),
                        params: params.iter().map(|p| p.as_str().into()).collect(),
                        body: Rc::new(body.clone()),
                        pool: cx.pool.clone(),
                        scope: cx.target,
                    };
                    let v = obj(Kind::Func(Rc::new(f)));
                    match name.as_deref().filter(|n| !n.is_empty()) {
                        Some(n) => self.set_variable(cx, n, v, true),
                        None => cx.stack.push(v),
                    }
                }
                Instr::GotoFrame(n) => {
                    // (the Apt action is `gotoAndStop`: a script that plays on has a `Play` after it;
                    // `p_buy`'s frame 1 sends the movie to frame 7 and it stays, as buy-a.log has it)
                    self.goto_target(cx.target, *n as usize);
                    self.clips[cx.target].playing = false;
                }
                Instr::GotoFrame2(flags) => {
                    let v = cx.pop();
                    self.goto_value(cx.target, &v);
                    // (bit 0 is the play flag: 0 stops at the frame, as `gotoAndStop` does)
                    self.clips[cx.target].playing = flags & 1 != 0;
                }
                Instr::End => {}
                Instr::Unknown(op) => {
                    self.note_stub(format!("opcode {op:#x}"));
                    return None;
                }
                Instr::Op(op) => {
                    if let Some(r) = self.op(*op, cx) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    /// The instructions without operands; `Some` for a `Return`.
    fn op(&mut self, op: Op, cx: &mut Cx) -> Option<Value> {
        use Op::*;
        match op {
            Pop => {
                cx.pop();
            }
            PushDuplicate => {
                let v = cx.stack.last().cloned().unwrap_or_default();
                cx.stack.push(v);
            }
            StackSwap => {
                let n = cx.stack.len();
                if n >= 2 {
                    cx.stack.swap(n - 1, n - 2);
                }
            }
            Return => return Some(cx.pop()),
            Add | Subtract | Multiply | Divide | Modulo => {
                let (b, a) = (cx.pop_num(), cx.pop_num());
                cx.stack.push(Value::Num(match op {
                    Add => a + b,
                    Subtract => a - b,
                    Multiply => a * b,
                    Divide => a / b,
                    _ => a % b,
                }));
            }
            Add2 => {
                let (b, a) = (cx.pop(), cx.pop());
                cx.stack.push(if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
                    let mut s = a.to_text().to_string();
                    s.push_str(&b.to_text());
                    Value::Str(s.into())
                } else {
                    Value::Num(a.to_num() + b.to_num())
                });
            }
            StringAdd => {
                let (b, a) = (cx.pop(), cx.pop());
                cx.stack.push(Value::Str(format!("{}{}", a.to_text(), b.to_text()).into()));
            }
            Increment => {
                let v = cx.pop_num();
                cx.stack.push(Value::Num(v + 1.0));
            }
            Decrement => {
                let v = cx.pop_num();
                cx.stack.push(Value::Num(v - 1.0));
            }
            ToNumber => {
                let v = cx.pop_num();
                cx.stack.push(Value::Num(v));
            }
            ToInteger => {
                let v = cx.pop_num();
                cx.stack.push(Value::Num(v.trunc()));
            }
            ToString => {
                let v = cx.pop();
                cx.stack.push(Value::Str(v.to_text()));
            }
            Not => {
                let v = cx.pop();
                cx.stack.push(Value::Bool(!v.truthy()));
            }
            And | Or => {
                let (b, a) = (cx.pop().truthy(), cx.pop().truthy());
                cx.stack.push(Value::Bool(if op == And { a && b } else { a || b }));
            }
            Equals | Equals2 => {
                let (b, a) = (cx.pop(), cx.pop());
                cx.stack.push(Value::Bool(loose_eq(&a, &b)));
            }
            StrictEquals => {
                let (b, a) = (cx.pop(), cx.pop());
                cx.stack.push(Value::Bool(strict_eq(&a, &b)));
            }
            StringEquals => {
                let (b, a) = (cx.pop(), cx.pop());
                cx.stack.push(Value::Bool(a.to_text() == b.to_text()));
            }
            Less | Less2 | Greater => {
                let (b, a) = (cx.pop(), cx.pop());
                let ord = match (&a, &b) {
                    (Value::Str(x), Value::Str(y)) if parse_num(x).is_none() || parse_num(y).is_none() => Some(x.cmp(y)),
                    _ => a.to_num().partial_cmp(&b.to_num()),
                };
                let r = match (op, ord) {
                    (Greater, Some(o)) => o.is_gt(),
                    (_, Some(o)) => o.is_lt(),
                    _ => false,
                };
                cx.stack.push(Value::Bool(r));
            }
            StringLength | MbStringLength => {
                let v = cx.pop();
                cx.stack.push(Value::Num(v.to_text().chars().count() as f64));
            }
            TypeOf => {
                let v = cx.pop();
                cx.stack.push(Value::str(match &v {
                    Value::Undefined => "undefined",
                    Value::Null => "null",
                    Value::Bool(_) => "boolean",
                    Value::Num(_) => "number",
                    Value::Str(_) => "string",
                    Value::Clip(_) => "movieclip",
                    Value::Obj(o) => match o.borrow().kind {
                        Kind::Func(_) => "function",
                        _ => "object",
                    },
                }));
            }
            GetVariable => {
                let name = cx.pop().to_text();
                let v = self.get_variable(cx, &name);
                cx.stack.push(v);
            }
            SetVariable => {
                let (v, name) = (cx.pop(), cx.pop().to_text());
                self.set_variable(cx, &name, v, false);
            }
            DefineLocal => {
                let (v, name) = (cx.pop(), cx.pop().to_text());
                if self.trace_sets {
                    eprintln!("apt:   local {name} = {}", v.to_text());
                }
                self.set_variable(cx, &name, v, true);
            }
            DefineLocal2 => {
                let name = cx.pop().to_text();
                let has = match &cx.locals {
                    Some(l) => l.contains(&name),
                    None => self.clips[cx.target].props.contains(&name),
                };
                if !has {
                    self.set_variable(cx, &name, Value::Undefined, true);
                }
            }
            GetMember => {
                let (name, base) = (cx.pop(), cx.pop());
                let v = self.get_member(&base, &name.to_text());
                cx.stack.push(v);
            }
            SetMember => {
                let (v, name, base) = (cx.pop(), cx.pop(), cx.pop());
                if self.trace_sets {
                    eprintln!("apt:   set {}.{} = {}", base.to_text(), name.to_text(), v.to_text());
                }
                self.set_member(&base, &name.to_text(), v);
            }
            Delete => {
                let (name, base) = (cx.pop(), cx.pop());
                let r = self.delete_member(&base, &name.to_text());
                cx.stack.push(Value::Bool(r));
            }
            Delete2 => {
                let name = cx.pop().to_text();
                let r = match &mut cx.locals {
                    Some(l) if l.contains(&name) => l.remove(&name),
                    _ => self.clips[cx.target].props.remove(&name),
                };
                cx.stack.push(Value::Bool(r));
            }
            CallFunction => {
                let name = cx.pop().to_text();
                let n = cx.pop_num() as usize;
                let args: Vec<Value> = (0..n).map(|_| cx.pop()).collect();
                let f = self.get_variable(cx, &name);
                let r = self.call_value(&f, Value::Clip(cx.target), &name, args);
                cx.stack.push(r);
            }
            CallMethod => {
                let name = cx.pop();
                let base = cx.pop();
                let n = cx.pop_num() as usize;
                let args: Vec<Value> = (0..n).map(|_| cx.pop()).collect();
                let r = if name.is_undefined() || name.to_text().is_empty() {
                    self.call_value(&base, Value::Undefined, "", args)
                } else {
                    self.call_method(&base, &name.to_text(), args)
                };
                cx.stack.push(r);
            }
            NewObject => {
                let name = cx.pop().to_text();
                let n = cx.pop_num() as usize;
                let args: Vec<Value> = (0..n).map(|_| cx.pop()).collect();
                let r = self.new_object(cx, &name, args);
                cx.stack.push(r);
            }
            InitArray => {
                let n = cx.pop_num() as usize;
                let items: Vec<Value> = (0..n).map(|_| cx.pop()).collect();
                cx.stack.push(obj(Kind::Array(items)));
            }
            InitObject => {
                let n = cx.pop_num() as usize;
                let o = Object::new(Kind::Plain);
                for _ in 0..n {
                    let (v, k) = (cx.pop(), cx.pop().to_text());
                    o.borrow_mut().props.set(&k, v);
                }
                cx.stack.push(Value::Obj(o));
            }
            Enumerate2 => {
                let o = cx.pop();
                cx.stack.push(Value::Null);
                for k in self.keys_of(&o) {
                    cx.stack.push(Value::Str(k));
                }
            }
            Play | Stop => {
                self.clips[cx.target].playing = op == Play;
            }
            NextFrame | PreviousFrame => {
                let c = cx.target;
                let n = self.frame_count(c);
                let f = self.clips[c].frame;
                let t = if op == NextFrame { (f + 1).min(n - 1) } else { f.saturating_sub(1) };
                self.goto(c, t, true);
                self.clips[c].playing = false;
            }
            CloneSprite => {
                let depth = cx.pop_num() as i32;
                let name = cx.pop().to_text();
                let src = cx.pop();
                if let Some(id) = self.clip_of(cx, &src) {
                    self.duplicate(id, &name, depth);
                }
            }
            RemoveSprite => {
                let t = cx.pop();
                if let Some(id) = self.clip_of(cx, &t) {
                    self.remove_clip(id);
                }
            }
            GetUrl2 => {
                let (target, url) = (cx.pop(), cx.pop().to_text());
                match self.clip_of(cx, &target) {
                    Some(t) => self.request_load(&url, t),
                    None => self.note_stub(format!("GetUrl2 {url} to {}", target.to_text())),
                }
            }
            Trace => {
                cx.pop();
            }
            RandomNumber => {
                let n = cx.pop_num();
                let r = self.random(n);
                cx.stack.push(Value::Num(r));
            }
            GetTime => cx.stack.push(Value::Num(0.0)),
            other => self.note_stub(format!("opcode {other:?}")),
        }
        None
    }

    /// A deterministic pseudo-random number below `n` (the simulation stays reproducible; the
    /// game's own sequence is not needed).
    fn random(&mut self, n: f64) -> f64 {
        let s = self.rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.rng = s;
        if n <= 0.0 { 0.0 } else { (((s >> 8) & 0xffff) as f64 / 65536.0 * n).floor() }
    }

    // ----- variables -----

    fn get_variable(&mut self, cx: &Cx, name: &str) -> Value {
        match name {
            "this" => return cx.this.clone(),
            "_root" | "_level0" => return self.root.map_or(Value::Undefined, Value::Clip),
            "_parent" => return self.clips[cx.target].parent.map_or(Value::Undefined, Value::Clip),
            _ => {}
        }
        if name.contains(['.', '/', ':']) {
            return self.get_path(cx, name);
        }
        if let Some(v) = cx.locals.as_ref().and_then(|l| l.get(name)) {
            return v.clone();
        }
        for (w, _) in cx.with.iter().rev() {
            if self.has_member(w, name) {
                return self.get_member(w, name);
            }
        }
        if self.has_member(&Value::Clip(cx.target), name) {
            return self.get_member(&Value::Clip(cx.target), name);
        }
        self.globals.get(name).cloned().unwrap_or_default()
    }

    /// A variable named by a path (`a.b.c`).
    fn get_path(&mut self, cx: &Cx, path: &str) -> Value {
        let mut parts = path.split(['.', ':']);
        let first = parts.next().unwrap_or("");
        let mut v = self.get_variable(cx, first);
        for p in parts {
            v = self.get_member(&v, p);
        }
        v
    }

    fn set_variable(&mut self, cx: &mut Cx, name: &str, v: Value, local: bool) {
        if local {
            match &mut cx.locals {
                Some(l) => l.set(name, v),
                None => self.set_member(&Value::Clip(cx.target), name, v),
            }
            return;
        }
        if let Some(l) = cx.locals.as_mut()
            && l.contains(name)
        {
            l.set(name, v);
            return;
        }
        for (w, _) in cx.with.iter().rev() {
            if self.has_member(w, name) {
                let w = w.clone();
                self.set_member(&w, name, v);
                return;
            }
        }
        self.set_member(&Value::Clip(cx.target), name, v);
    }

    // ----- members -----

    fn has_member(&self, base: &Value, name: &str) -> bool {
        match base {
            Value::Clip(id) => {
                let c = &self.clips[*id];
                c.props.contains(name) || self.clip_prop(*id, name).is_some() || self.child_named(*id, name).is_some()
            }
            Value::Obj(o) => {
                let o = o.borrow();
                o.props.contains(name) || matches!(&o.kind, Kind::Array(_) if name == "length")
            }
            _ => false,
        }
    }

    pub(super) fn get_member(&mut self, base: &Value, name: &str) -> Value {
        match base {
            Value::Clip(id) => {
                let id = *id;
                if let Some(v) = self.clips[id].props.get(name) {
                    return v.clone();
                }
                if let Some(v) = self.clip_prop(id, name) {
                    return v;
                }
                if let Some(c) = self.child_named(id, name) {
                    return Value::Clip(c);
                }
                Value::Undefined
            }
            Value::Obj(o) => {
                let o = o.borrow();
                if matches!(o.kind, Kind::Native(Native::Extern))
                    && let Some(v) = self.read_value(name)
                {
                    return v;
                }
                if let Some(v) = o.props.get(name) {
                    return v.clone();
                }
                match &o.kind {
                    Kind::Array(a) => {
                        if name == "length" {
                            return Value::Num(a.len() as f64);
                        }
                        if let Ok(i) = name.parse::<usize>() {
                            return a.get(i).cloned().unwrap_or_default();
                        }
                        Value::Undefined
                    }
                    Kind::Native(Native::Math) => match name {
                        "PI" => Value::Num(std::f64::consts::PI),
                        _ => Value::Undefined,
                    },
                    Kind::Native(Native::Key) => key_const(name),
                    _ => Value::Undefined,
                }
            }
            Value::Str(s) if name == "length" => Value::Num(s.chars().count() as f64),
            _ => Value::Undefined,
        }
    }

    pub(super) fn set_member(&mut self, base: &Value, name: &str, v: Value) {
        match base {
            Value::Clip(id) => {
                if !self.set_clip_prop(*id, name, &v) {
                    self.clips[*id].props.set(name, v);
                }
            }
            Value::Obj(o) => {
                let native = matches!(o.borrow().kind, Kind::Native(Native::Extern));
                if native {
                    // a command to the game; what the movie wrote is not stored: the members
                    // the movies read are the game's own values
                    self.send(name, &v);
                    return;
                }
                let mut ob = o.borrow_mut();
                if let Kind::Array(a) = &mut ob.kind {
                    if name == "length" {
                        a.resize(v.to_num().max(0.0) as usize, Value::Undefined);
                        return;
                    }
                    if let Ok(i) = name.parse::<usize>() {
                        if i >= a.len() {
                            a.resize(i + 1, Value::Undefined);
                        }
                        a[i] = v;
                        return;
                    }
                }
                ob.props.set(name, v);
            }
            _ => {}
        }
    }

    fn delete_member(&mut self, base: &Value, name: &str) -> bool {
        match base {
            Value::Clip(id) => self.clips[*id].props.remove(name),
            Value::Obj(o) => {
                let mut ob = o.borrow_mut();
                if let Kind::Array(a) = &mut ob.kind
                    && let Ok(i) = name.parse::<usize>()
                {
                    if i < a.len() {
                        a[i] = Value::Undefined;
                    }
                    return true;
                }
                ob.props.remove(name)
            }
            _ => false,
        }
    }

    fn keys_of(&self, base: &Value) -> Vec<Rc<str>> {
        match base {
            Value::Clip(id) => self.clips[*id].props.keys().cloned().collect(),
            Value::Obj(o) => {
                let ob = o.borrow();
                let mut k: Vec<Rc<str>> = Vec::new();
                if let Kind::Array(a) = &ob.kind {
                    k.extend((0..a.len()).map(|i| Rc::from(i.to_string())));
                }
                k.extend(ob.props.keys().cloned());
                k
            }
            _ => Vec::new(),
        }
    }

    /// The pseudo-members of a clip (`_x` and the like).
    fn clip_prop(&self, id: ClipId, name: &str) -> Option<Value> {
        let c = &self.clips[id];
        let num = |v: f64| Some(Value::Num(v));
        match name {
            "_x" => num(c.matrix[4]),
            "_y" => num(c.matrix[5]),
            "_xscale" => num(c.matrix[0].hypot(c.matrix[1]) * 100.0),
            "_yscale" => num(c.matrix[2].hypot(c.matrix[3]) * 100.0),
            "_rotation" => num(c.matrix[1].atan2(c.matrix[0]).to_degrees()),
            "_alpha" => num(c.color.mul[3] * 100.0),
            "_visible" => Some(Value::Bool(c.visible)),
            "_name" => Some(Value::str(&c.name)),
            "_parent" => c.parent.map(Value::Clip),
            "_root" => self.root.map(Value::Clip),
            "_currentframe" => num(c.frame as f64 + 1.0),
            "_totalframes" => num(self.frame_count(id) as f64),
            "_width" => num(self.extent(id).map_or(0.0, |b| b[2] - b[0])),
            "_height" => num(self.extent(id).map_or(0.0, |b| b[3] - b[1])),
            "text" | "htmlText" if c.kind == ClipKind::Text => c.text.clone().map(Value::Str),
            _ => None,
        }
    }

    fn set_clip_prop(&mut self, id: ClipId, name: &str, v: &Value) -> bool {
        let n = v.to_num();
        if name == "_width" || name == "_height" {
            // scale the clip so what it draws is this wide (or high)
            let Some(b) = self.extent(id) else { return true };
            let (cur, wide) = if name == "_width" { (b[2] - b[0], true) } else { (b[3] - b[1], false) };
            if cur > 0.0 {
                let k = n / cur;
                let m = &mut self.clips[id].matrix;
                if wide {
                    m[0] *= k;
                    m[1] *= k;
                } else {
                    m[2] *= k;
                    m[3] *= k;
                }
            }
            return true;
        }
        let c = &mut self.clips[id];
        match name {
            "_x" => c.matrix[4] = n,
            "_y" => c.matrix[5] = n,
            "_xscale" => {
                let l = c.matrix[0].hypot(c.matrix[1]);
                let k = if l > 0.0 { n / 100.0 / l } else { 0.0 };
                if l > 0.0 {
                    c.matrix[0] *= k;
                    c.matrix[1] *= k;
                } else {
                    c.matrix[0] = n / 100.0;
                }
            }
            "_yscale" => {
                let l = c.matrix[2].hypot(c.matrix[3]);
                if l > 0.0 {
                    let k = n / 100.0 / l;
                    c.matrix[2] *= k;
                    c.matrix[3] *= k;
                } else {
                    c.matrix[3] = n / 100.0;
                }
            }
            "_rotation" => {
                let (sx, sy) = (c.matrix[0].hypot(c.matrix[1]), c.matrix[2].hypot(c.matrix[3]));
                let r = n.to_radians();
                c.matrix[0] = sx * r.cos();
                c.matrix[1] = sx * r.sin();
                c.matrix[2] = -sy * r.sin();
                c.matrix[3] = sy * r.cos();
            }
            "_alpha" => c.color.mul[3] = n / 100.0,
            "_visible" => c.visible = v.truthy(),
            "_name" => c.name = v.to_text().to_string(),
            "text" | "htmlText" if c.kind == ClipKind::Text => c.text = Some(v.to_text()),
            _ => return false,
        }
        true
    }

    /// The box of what a clip draws, in its parent's space: left, top, right, bottom.
    pub fn extent(&self, id: ClipId) -> Option<[f64; 4]> {
        let c = &self.clips[id];
        let local: Option<[f64; 4]> = match c.kind {
            ClipKind::Shape => match c.flash.movie.characters.get(c.ch as usize) {
                Some(super::movie::Character::Shape { bounds, .. }) => Some(bounds.map(f64::from)),
                _ => None,
            },
            ClipKind::Text => match c.flash.movie.characters.get(c.ch as usize) {
                Some(super::movie::Character::Text(t)) => Some(t.bounds.map(f64::from)),
                _ => None,
            },
            ClipKind::Button | ClipKind::Image => None,
            ClipKind::Sprite => {
                let mut acc: Option<[f64; 4]> = None;
                for &k in c.children.values() {
                    if let Some(b) = self.extent(k) {
                        acc = Some(match acc {
                            None => b,
                            Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
                        });
                    }
                }
                acc
            }
        };
        let b = local?;
        let m = &c.matrix;
        let pts = [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])].map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
        let xs = pts.map(|p| p.0);
        let ys = pts.map(|p| p.1);
        Some([
            xs.iter().copied().fold(f64::MAX, f64::min),
            ys.iter().copied().fold(f64::MAX, f64::min),
            xs.iter().copied().fold(f64::MIN, f64::max),
            ys.iter().copied().fold(f64::MIN, f64::max),
        ])
    }

    // ----- clips -----

    /// The clip a script value names: the clip itself, or a path in a string.
    fn clip_of(&self, cx: &Cx, v: &Value) -> Option<ClipId> {
        match v {
            Value::Clip(id) => Some(*id),
            Value::Str(s) => self.resolve_path(cx.target, s),
            _ => None,
        }
    }

    fn goto_target(&mut self, id: ClipId, frame: usize) {
        self.goto(id, frame, true);
    }

    /// Goes to a frame a script names: a number (counted from 1) or a label.
    pub(super) fn goto_value(&mut self, id: ClipId, v: &Value) {
        let frame = match v {
            Value::Num(n) => Some((*n as i64 - 1).max(0) as usize),
            Value::Str(s) => self.label_frame(id, s).or_else(|| parse_num(s).map(|n| (n as i64 - 1).max(0) as usize)),
            _ => None,
        };
        match frame {
            Some(f) => self.goto(id, f, true),
            None => self.note_stub(format!("goto frame {}", v.to_text())),
        }
    }

    /// `duplicateMovieClip`: a new clip of the same character beside the first.
    fn duplicate(&mut self, src: ClipId, name: &str, depth: i32) -> Option<ClipId> {
        let parent = self.clips[src].parent?;
        let (flash, ch, kind, m, col) = {
            let s = &self.clips[src];
            (s.flash.clone(), s.ch, s.kind, s.matrix, s.color)
        };
        let id = self.spawn(parent, flash, ch, kind, depth, name);
        self.clips[id].matrix = m;
        self.clips[id].color = col;
        Some(id)
    }

    // ----- calls -----

    pub(super) fn call_value(&mut self, f: &Value, this: Value, name: &str, args: Vec<Value>) -> Value {
        if let Value::Obj(o) = f {
            let func = match &o.borrow().kind {
                Kind::Func(f) => Some(f.clone()),
                _ => None,
            };
            if let Some(func) = func {
                return self.call_func(&func, this, args);
            }
        }
        self.call_builtin_function(name, args)
    }

    fn call_builtin_function(&mut self, name: &str, args: Vec<Value>) -> Value {
        let a0 = args.first().cloned().unwrap_or_default();
        match name {
            "parseInt" => {
                let s = a0.to_text();
                let radix = args.get(1).map_or(10, |r| r.to_num() as u32);
                let t = s.trim();
                let digits: String = t.chars().take_while(|c| c.is_digit(radix.clamp(2, 36))).collect();
                i64::from_str_radix(&digits, radix.clamp(2, 36)).map_or(Value::Num(f64::NAN), |v| Value::Num(v as f64))
            }
            "parseFloat" | "Number" => Value::Num(a0.to_num()),
            "String" => Value::Str(a0.to_text()),
            "Boolean" => Value::Bool(a0.truthy()),
            "isNaN" => Value::Bool(a0.to_num().is_nan()),
            "int" => Value::Num(a0.to_num().trunc()),
            "random" => Value::Num(self.random(a0.to_num())),
            "getTimer" => Value::Num(0.0),
            "escape" | "unescape" => a0,
            _ => {
                self.note_stub(format!("function {name}"));
                Value::Undefined
            }
        }
    }

    pub(super) fn call_method(&mut self, base: &Value, name: &str, args: Vec<Value>) -> Value {
        // a script's own function first
        let member = self.get_member(base, name);
        if let Value::Obj(o) = &member {
            let func = match &o.borrow().kind {
                Kind::Func(f) => Some(f.clone()),
                _ => None,
            };
            if let Some(f) = func {
                return self.call_func(&f, base.clone(), args);
            }
        }
        let a = |i: usize| args.get(i).cloned().unwrap_or_default();
        match base {
            Value::Clip(id) => self.clip_method(*id, name, &args),
            Value::Str(s) => string_method(s, name, &args).unwrap_or_else(|| {
                self.note_stub(format!("string method {name}"));
                Value::Undefined
            }),
            Value::Num(n) => match name {
                "toString" => Value::Str(Value::Num(*n).to_text()),
                _ => {
                    self.note_stub(format!("number method {name}"));
                    Value::Undefined
                }
            },
            Value::Obj(o) => {
                let native = match &o.borrow().kind {
                    Kind::Native(n) => Some(*n),
                    _ => None,
                };
                match native {
                    Some(Native::Math) => self.math(name, &args),
                    Some(Native::Key) => match name {
                        "getCode" => Value::Num(self.key_code),
                        "isDown" => Value::Bool(self.key_code == a(0).to_num()),
                        _ => {
                            self.note_stub(format!("Key.{name}"));
                            Value::Undefined
                        }
                    },
                    Some(Native::Class(c)) => {
                        self.note_stub(format!("{c}.{name}"));
                        Value::Undefined
                    }
                    Some(Native::Extern) => {
                        self.note_stub(format!("extern.{name}()"));
                        Value::Undefined
                    }
                    None => self.array_method(o, name, &args),
                }
            }
            _ => {
                self.note_stub(format!("method {name} of {}", base.to_text()));
                Value::Undefined
            }
        }
    }

    fn math(&mut self, name: &str, args: &[Value]) -> Value {
        let x = args.first().map_or(f64::NAN, Value::to_num);
        let y = args.get(1).map_or(f64::NAN, Value::to_num);
        Value::Num(match name {
            "round" => (x + 0.5).floor(),
            "floor" => x.floor(),
            "ceil" => x.ceil(),
            "abs" => x.abs(),
            "sqrt" => x.sqrt(),
            "min" => x.min(y),
            "max" => x.max(y),
            "pow" => x.powf(y),
            "sin" => x.sin(),
            "cos" => x.cos(),
            "atan2" => x.atan2(y),
            "random" => self.random(65536.0) / 65536.0,
            _ => {
                self.note_stub(format!("Math.{name}"));
                return Value::Undefined;
            }
        })
    }

    fn array_method(&mut self, o: &ObjRef, name: &str, args: &[Value]) -> Value {
        let mut ob = o.borrow_mut();
        let Kind::Array(a) = &mut ob.kind else {
            drop(ob);
            self.note_stub(format!("method {name} of an object"));
            return Value::Undefined;
        };
        match name {
            "push" => {
                a.extend(args.iter().cloned());
                Value::Num(a.len() as f64)
            }
            "pop" => a.pop().unwrap_or_default(),
            "shift" => {
                if a.is_empty() {
                    Value::Undefined
                } else {
                    a.remove(0)
                }
            }
            "unshift" => {
                for (i, v) in args.iter().enumerate() {
                    a.insert(i, v.clone());
                }
                Value::Num(a.len() as f64)
            }
            "join" => {
                let sep = args.first().map_or(Rc::from(","), Value::to_text);
                Value::Str(a.iter().map(|v| v.to_text().to_string()).collect::<Vec<_>>().join(&sep).into())
            }
            "reverse" => {
                a.reverse();
                Value::Undefined
            }
            "concat" => {
                let mut r = a.clone();
                for v in args {
                    match v {
                        Value::Obj(x) => match &x.borrow().kind {
                            Kind::Array(b) => r.extend(b.iter().cloned()),
                            _ => r.push(v.clone()),
                        },
                        _ => r.push(v.clone()),
                    }
                }
                obj(Kind::Array(r))
            }
            "slice" => {
                let n = a.len() as i64;
                let norm = |v: i64| if v < 0 { (n + v).max(0) } else { v.min(n) } as usize;
                let s = norm(args.first().map_or(0, |v| v.to_num() as i64));
                let e = norm(args.get(1).map_or(n, |v| v.to_num() as i64));
                obj(Kind::Array(if s < e { a[s..e].to_vec() } else { Vec::new() }))
            }
            "toString" => Value::Str(Value::Obj(o.clone()).to_text()),
            _ => {
                drop(ob);
                self.note_stub(format!("Array.{name}"));
                Value::Undefined
            }
        }
    }

    fn clip_method(&mut self, id: ClipId, name: &str, args: &[Value]) -> Value {
        let a = |i: usize| args.get(i).cloned().unwrap_or_default();
        match name {
            "gotoAndPlay" | "gotoAndStop" => {
                self.goto_value(id, &a(0));
                self.clips[id].playing = name == "gotoAndPlay";
            }
            "play" => self.clips[id].playing = true,
            "stop" => self.clips[id].playing = false,
            "nextFrame" | "prevFrame" => {
                let n = self.frame_count(id);
                let f = self.clips[id].frame;
                self.goto(id, if name == "nextFrame" { (f + 1).min(n - 1) } else { f.saturating_sub(1) }, true);
                self.clips[id].playing = false;
            }
            "removeMovieClip" => self.remove_clip(id),
            "duplicateMovieClip" => {
                return self.duplicate(id, &a(0).to_text(), a(1).to_num() as i32).map_or(Value::Undefined, Value::Clip);
            }
            "attachMovie" => {
                // attachMovie(export name, new name, depth)
                let export = a(0).to_text();
                let flash = self.clips[id].flash.clone();
                let ch = flash.movie.exports.iter().find(|e| *e.name == *export).map(|e| e.id);
                return match ch {
                    Some(ch) => Value::Clip(self.spawn(id, flash, ch, ClipKind::Sprite, a(2).to_num() as i32, &a(1).to_text())),
                    None => {
                        self.note_stub(format!("attachMovie of {export}"));
                        Value::Undefined
                    }
                };
            }
            "getDepth" => return Value::Num(self.clips[id].depth as f64),
            "loadMovie" => {
                let t = a(0).to_text();
                self.request_load(&t, id);
            }
            "unloadMovie" => self.request_load("", id),
            "hitTest" => return Value::Bool(false),
            _ => self.note_stub(format!("clip method {name}")),
        }
        Value::Undefined
    }

    fn new_object(&mut self, cx: &mut Cx, class: &str, args: Vec<Value>) -> Value {
        match class {
            "Array" => {
                if args.len() == 1
                    && let Value::Num(n) = args[0]
                {
                    return obj(Kind::Array(vec![Value::Undefined; n.max(0.0) as usize]));
                }
                obj(Kind::Array(args))
            }
            "Object" => obj(Kind::Plain),
            "String" => Value::Str(args.first().map_or(Rc::from(""), Value::to_text)),
            "Number" => Value::Num(args.first().map_or(0.0, Value::to_num)),
            _ => {
                // a constructor of the movie's own
                let f = self.get_variable(cx, class);
                let o = Object::new(Kind::Plain);
                self.call_value(&f, Value::Obj(o.clone()), class, args);
                Value::Obj(o)
            }
        }
    }
}

fn key_const(name: &str) -> Value {
    Value::Num(match name {
        "LEFT" => 37.0,
        "UP" => 38.0,
        "RIGHT" => 39.0,
        "DOWN" => 40.0,
        "ENTER" => 13.0,
        "SPACE" => 32.0,
        _ => return Value::Undefined,
    })
}

fn string_method(s: &Rc<str>, name: &str, args: &[Value]) -> Option<Value> {
    let a = |i: usize| args.get(i).cloned().unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let sub = |from: i64, len: i64| -> Value {
        let from = if from < 0 { (n + from).max(0) } else { from.min(n) };
        let end = (from + len.max(0)).min(n);
        Value::Str(chars[from as usize..end as usize].iter().collect::<String>().into())
    };
    Some(match name {
        "substr" => sub(a(0).to_num() as i64, if args.len() > 1 { a(1).to_num() as i64 } else { n }),
        "substring" => {
            let (x, y) = (a(0).to_num() as i64, if args.len() > 1 { a(1).to_num() as i64 } else { n });
            let (x, y) = (x.clamp(0, n), y.clamp(0, n));
            sub(x.min(y), (x - y).abs())
        }
        "charAt" => sub(a(0).to_num() as i64, 1),
        "indexOf" => {
            let needle = a(0).to_text();
            Value::Num(s.find(&*needle).map_or(-1.0, |i| s[..i].chars().count() as f64))
        }
        "lastIndexOf" => {
            let needle = a(0).to_text();
            Value::Num(s.rfind(&*needle).map_or(-1.0, |i| s[..i].chars().count() as f64))
        }
        "toUpperCase" => Value::Str(s.to_uppercase().into()),
        "toLowerCase" => Value::Str(s.to_lowercase().into()),
        "split" => {
            let sep = a(0).to_text();
            let parts: Vec<Value> = if sep.is_empty() { s.chars().map(|c| Value::Str(c.to_string().into())).collect() } else { s.split(&*sep).map(Value::str).collect() };
            obj(Kind::Array(parts))
        }
        "toString" => Value::Str(s.clone()),
        _ => return None,
    })
}
