//! A movie's actions: the bytecode of ActionScript as the game keeps it, decoded into
//! instructions with their operands (`docs/assets.md`, "Menus and HUD (Apt)").
//!
//! A stream is the opcode bytes one after another; an opcode of 0x80 or more takes operand
//! words, and those start on a word boundary (the bytes between are zero). A `Push`'s values
//! and a `ConstantPool`'s entries are indices into the movie's constant table (the `.const`
//! file), which gives each one's type and value.

use super::img::{Img, Res};

/// A constant of the `.const` file, or a value a `Push` pushes.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Float(f32),
    Undefined,
    /// A register's number.
    Register(u32),
    Bool(bool),
    Int(i32),
    /// An entry of the constant pool in force when the action runs.
    Pool(u32),
    /// A type or value this reader has no meaning for (counted as unknown).
    Unknown(u32, u32),
}

macro_rules! ops {
    ($($code:literal $name:ident,)*) => {
        /// The opcodes without operands (the ActionScript 1 and 2 set, as the Flash file format
        /// numbers them).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Op { $($name,)* }
        impl Op {
            pub fn from_code(c: u8) -> Option<Op> {
                match c { $($code => Some(Op::$name),)* _ => None }
            }
            pub fn code(self) -> u8 {
                match self { $(Op::$name => $code,)* }
            }
        }
    };
}

ops! {
    0x04 NextFrame, 0x05 PreviousFrame, 0x06 Play, 0x07 Stop, 0x08 ToggleQuality, 0x09 StopSounds,
    0x0a Add, 0x0b Subtract, 0x0c Multiply, 0x0d Divide, 0x0e Equals, 0x0f Less, 0x10 And, 0x11 Or,
    0x12 Not, 0x13 StringEquals, 0x14 StringLength, 0x15 StringExtract, 0x17 Pop, 0x18 ToInteger,
    0x1c GetVariable, 0x1d SetVariable, 0x20 SetTarget2, 0x21 StringAdd, 0x22 GetProperty,
    0x23 SetProperty, 0x24 CloneSprite, 0x25 RemoveSprite, 0x26 Trace, 0x27 StartDrag, 0x28 EndDrag,
    0x29 StringLess, 0x2a Throw, 0x2b CastOp, 0x2c ImplementsOp, 0x30 RandomNumber,
    0x31 MbStringLength, 0x32 CharToAscii, 0x33 AsciiToChar, 0x34 GetTime, 0x35 MbStringExtract,
    0x36 MbCharToAscii, 0x37 MbAsciiToChar, 0x3a Delete, 0x3b Delete2, 0x3c DefineLocal,
    0x3d CallFunction, 0x3e Return, 0x3f Modulo, 0x40 NewObject, 0x41 DefineLocal2, 0x42 InitArray,
    0x43 InitObject, 0x44 TypeOf, 0x45 TargetPath, 0x46 Enumerate, 0x47 Add2, 0x48 Less2,
    0x49 Equals2, 0x4a ToNumber, 0x4b ToString, 0x4c PushDuplicate, 0x4d StackSwap, 0x4e GetMember,
    0x4f SetMember, 0x50 Increment, 0x51 Decrement, 0x52 CallMethod, 0x53 NewMethod,
    0x54 InstanceOf, 0x55 Enumerate2, 0x60 BitAnd, 0x61 BitOr, 0x62 BitXor, 0x63 ShiftLeft,
    0x64 ShiftRight, 0x65 ShiftRightUnsigned, 0x66 StrictEquals, 0x67 Greater, 0x68 StringGreater,
    0x69 Extends,
    // GetURL2 (the Flash numbering's 0x9a) has no operand word here: the next opcode follows it
    0x9a GetUrl2,
}

/// One instruction.
#[derive(Clone, Debug, PartialEq)]
pub enum Instr {
    Op(Op),
    /// 0x81: a frame number.
    GotoFrame(u32),
    /// 0x9f: the flags word (play, and a frame-offset bit as in the Flash format).
    GotoFrame2(u32),
    /// 0x87: stores the top of the stack in a register.
    SetRegister(u32),
    /// 0x88: the strings of the pool later `Push`es of `Value::Pool` take from.
    ConstantPool(Vec<Value>),
    /// 0x96.
    Push(Vec<Value>),
    /// 0x99: jumps to an instruction of this list (its length for the end).
    BranchAlways(usize),
    /// 0x9d: jumps if the popped value is true.
    BranchIfTrue(usize),
    /// 0x94: the next `size` bytes of instructions run with the popped object as the scope;
    /// `until` is the index of the first instruction after them.
    With { size: u32, until: usize },
    /// 0x9b: a function, its body kept as its own list.
    DefineFunction { name: Option<String>, params: Vec<String>, body: Vec<Instr> },
    /// 0x00 inside a function's body (a stream's own end is not kept).
    End,
    /// An opcode this reader has no meaning for; the stream ends there.
    Unknown(u8),
}

/// The words that start a function's body, after its size.
const BODY_MARK: [u32; 2] = [0x9876_5432, 0x1234_5678];

/// Reads the zero bytes up to the next word boundary.
fn align(m: &mut Img, p: usize) -> usize {
    let mut q = p;
    while !q.is_multiple_of(4) {
        if m.peek(q) == Some(0) {
            let _ = m.u8(q);
        }
        q += 1;
    }
    q
}

fn value(consts: &[Value], idx: u32) -> Res<Value> {
    consts.get(idx as usize).cloned().ok_or_else(|| format!("constant {idx} is not in a table of {}", consts.len()))
}

/// Decodes the stream at `at`: up to its `End` opcode, or for a function's body over its
/// `limit` bytes.
pub fn decode(m: &mut Img, consts: &[Value], at: usize, limit: Option<usize>) -> Res<Vec<Instr>> {
    let end = limit.map(|n| at + n);
    let mut out = Vec::new();
    let mut starts = Vec::new();
    let mut targets: Vec<(usize, i64)> = Vec::new();
    let mut withs: Vec<(usize, i64)> = Vec::new();
    let mut p = at;
    // where the stream's own End byte is (a branch may land on it, meaning the end)
    let mut end_at = None;
    loop {
        if end.is_some_and(|e| p >= e) {
            break;
        }
        let op = m.u8(p)?;
        p += 1;
        if op == 0 && end.is_none() {
            // the stream's end: zero bytes pad it to a word
            end_at = Some(p - 1);
            align(m, p);
            break;
        }
        starts.push(p - 1);
        if op == 0 {
            // a function's body may hold an End
            out.push(Instr::End);
            continue;
        }
        if let Some(o) = Op::from_code(op) {
            out.push(Instr::Op(o));
            continue;
        }
        let known = matches!(op, 0x81 | 0x87 | 0x88 | 0x94 | 0x96 | 0x99 | 0x9b | 0x9d | 0x9f);
        if !known {
            out.push(Instr::Unknown(op));
            break;
        }
        p = align(m, p);
        match op {
            0x81 => {
                out.push(Instr::GotoFrame(m.u32(p)?));
                p += 4;
            }
            0x9f => {
                out.push(Instr::GotoFrame2(m.u32(p)?));
                p += 4;
            }
            0x87 => {
                out.push(Instr::SetRegister(m.u32(p)?));
                p += 4;
            }
            0x94 => {
                let size = m.u32(p)?;
                p += 4;
                withs.push((out.len(), p as i64 + size as i64));
                out.push(Instr::With { size, until: 0 });
            }
            0x88 | 0x96 => {
                let (n, ptr) = (m.u32(p)? as usize, m.ptr(p + 4)?);
                p += 8;
                let mut vals = Vec::with_capacity(n);
                for i in 0..n {
                    let idx = m.u32(ptr + 4 * i)?;
                    vals.push(value(consts, idx)?);
                }
                out.push(if op == 0x96 { Instr::Push(vals) } else { Instr::ConstantPool(vals) });
            }
            0x99 | 0x9d => {
                let off = m.i32(p)?;
                p += 4;
                targets.push((out.len(), p as i64 + off as i64));
                out.push(if op == 0x99 { Instr::BranchAlways(0) } else { Instr::BranchIfTrue(0) });
            }
            0x9b => {
                let name = m.opt_str(p)?;
                let (n, pptr, size) = (m.u32(p + 4)? as usize, m.ptr(p + 8)?, m.u32(p + 12)? as usize);
                p += 16;
                let mut params = Vec::with_capacity(n);
                for i in 0..n {
                    let s = m.ptr(pptr + 4 * i)?;
                    params.push(m.cstr(s)?);
                }
                for w in BODY_MARK {
                    if m.u32(p)? != w {
                        return Err(format!("function body at {p:#x} does not start with its mark"));
                    }
                    p += 4;
                }
                let body = decode(m, consts, p, Some(size))?;
                p += size;
                out.push(Instr::DefineFunction { name, params, body });
            }
            _ => unreachable!(),
        }
    }
    let stream_end = end.or(end_at).unwrap_or(p);
    for (i, target) in targets {
        let t = starts.iter().position(|&s| s as i64 == target).or((target == stream_end as i64).then_some(starts.len())).ok_or_else(|| format!("a branch at instruction {i} of the stream at {at:#x} lands off an instruction ({target:#x})"))?;
        match &mut out[i] {
            Instr::BranchAlways(x) | Instr::BranchIfTrue(x) => *x = t,
            _ => unreachable!(),
        }
    }
    for (i, target) in withs {
        let t = starts.iter().position(|&s| s as i64 >= target).unwrap_or(starts.len());
        if let Instr::With { until, .. } = &mut out[i] {
            *until = t;
        }
    }
    Ok(out)
}
