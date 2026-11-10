//! The expression primitive (opcode 2): most of what the scripts do (`docs/primitives.md`). One
//! operator between two variables, each named by a scope and a datum; the variables themselves
//! are the host's ([`Variables`]), so that this is the same for every game.
//!
//! The rules are Bustin' Out's TryExpression (fn_80213890), read from its code and checked
//! against the recompiled game (`docs/interpreter.md`, "The expression primitive").

use crate::bhav::Node;
use crate::interp::PrimResult;

/// The operand: the left and right data (16 bits each), a byte the primitive does not read,
/// the operator, the left and right scopes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExpressionOperand {
    pub lhs_data: i16,
    pub rhs_data: i16,
    /// An [`ExprOp`], or another value (error 1).
    pub op: u8,
    pub lhs_scope: u8,
    pub rhs_scope: u8,
}

impl ExpressionOperand {
    pub fn of(n: &Node) -> Self {
        ExpressionOperand {
            lhs_data: n.operand16(0) as i16,
            rhs_data: n.operand16(1) as i16,
            op: n.operand[5],
            lhs_scope: n.operand[6],
            rhs_scope: n.operand[7],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExprOp {
    Greater = 0,
    Less = 1,
    Equal = 2,
    Add = 3,
    Subtract = 4,
    Assign = 5,
    Multiply = 6,
    Divide = 7,
    FlagSet = 8,
    SetFlag = 9,
    ClearFlag = 10,
    IncrementLess = 11,
    Modulo = 12,
    And = 13,
    GreaterEqual = 14,
    LessEqual = 15,
    NotEqual = 16,
    DecrementGreater = 17,
}

impl ExprOp {
    pub fn from_u8(op: u8) -> Option<ExprOp> {
        use ExprOp::*;
        const ALL: [ExprOp; 18] = [
            Greater, Less, Equal, Add, Subtract, Assign, Multiply, Divide, FlagSet, SetFlag, ClearFlag,
            IncrementLess, Modulo, And, GreaterEqual, LessEqual, NotEqual, DecrementGreater,
        ];
        ALL.get(op as usize).copied()
    }
}

/// A variable as a primitive sees it: its 16-bit value, and for a float variable (a motive) the
/// float, which arithmetic then works on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Variable {
    pub value: i16,
    pub f: Option<f32>,
}

/// The variables a primitive reads and writes, by scope and datum.
pub trait Variables {
    /// `None` if there is no such variable.
    fn get(&mut self, scope: u8, data: i16) -> Option<Variable>;
    /// False if the variable cannot be written.
    fn set(&mut self, scope: u8, data: i16, value: i16) -> bool;
    fn set_float(&mut self, scope: u8, data: i16, value: f32) -> bool;
}

/// The primitive's error codes (the original stores them in its interpreter, +48).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i16)]
pub enum ExprError {
    None = 0,
    UnknownOperator = 1,
    FlagTooHigh = 13,
    NotWritable = 19,
    DivideByZero = 38,
    /// A lookup failed (the original's lookup reports its own error).
    NoVariable = -1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExprResult {
    pub result: PrimResult,
    pub error: ExprError,
}

/// The motive scopes, whose float results are kept between -100 and 100.
pub fn is_motive_scope(scope: u8) -> bool {
    matches!(scope, 14 | 15 | 17)
}

fn ok(b: bool) -> ExprResult {
    ExprResult { result: if b { PrimResult::True } else { PrimResult::False }, error: ExprError::None }
}

fn fail(e: ExprError) -> ExprResult {
    ExprResult { result: PrimResult::Stop, error: e }
}

/// The flag operators' bit: flag n is bit n - 1 (flag 0 is out of range, as in the original:
/// its shift then takes the sign).
fn flag_test(value: i16, flag: i16) -> bool {
    let n = (flag as i32 - 1) as u32 & 63;
    let x = value as i32;
    if n >= 32 { x < 0 } else { (x >> n) & 1 != 0 }
}

fn flag_mask(flag: i16) -> u16 {
    let n = (flag as i32 - 1) as u32;
    if n & 0x20 != 0 { 0 } else { (1u32 << (n & 31)) as u16 }
}

pub fn expression<V: Variables + ?Sized>(e: &ExpressionOperand, v: &mut V) -> ExprResult {
    let Some(r) = v.get(e.rhs_scope, e.rhs_data) else { return fail(ExprError::NoVariable) };
    let Some(l) = v.get(e.lhs_scope, e.lhs_data) else { return fail(ExprError::NoVariable) };
    let (a, b) = (l.value, r.value);
    let lf = l.f;
    let fb = b as f32;

    // A result written to the left side: 16-bit, or for a float variable its float (motives
    // kept between -100 and 100).
    let put = |v: &mut V, x: i16| if v.set(e.lhs_scope, e.lhs_data, x) { ok(true) } else { fail(ExprError::NotWritable) };
    let put_float = |v: &mut V, mut x: f32| {
        if is_motive_scope(e.lhs_scope) {
            x = x.clamp(-100.0, 100.0);
        }
        if v.set_float(e.lhs_scope, e.lhs_data, x) { ok(true) } else { fail(ExprError::NotWritable) }
    };
    // The arithmetic operators: on the float if there is one, else wrapping to 16 bits as the
    // original's.
    let arith = |v: &mut V, f: fn(f32, f32) -> f32, i: fn(i16, i16) -> i16| match lf {
        Some(x) => put_float(v, f(x, fb)),
        None => put(v, i(a, b)),
    };

    let Some(op) = ExprOp::from_u8(e.op) else { return fail(ExprError::UnknownOperator) };
    match op {
        // Comparisons take the 16-bit values, float variables included.
        ExprOp::Greater => ok(a > b),
        ExprOp::Less => ok(a < b),
        ExprOp::Equal => ok(a == b),
        ExprOp::GreaterEqual => ok(a >= b),
        ExprOp::LessEqual => ok(a <= b),
        ExprOp::NotEqual => ok(a != b),
        ExprOp::FlagSet if b > 16 => fail(ExprError::FlagTooHigh),
        ExprOp::FlagSet => ok(flag_test(a, b)),

        ExprOp::Assign => arith(v, |_, y| y, |_, y| y),
        ExprOp::Add => arith(v, |x, y| x + y, i16::wrapping_add),
        ExprOp::Subtract => arith(v, |x, y| x - y, i16::wrapping_sub),
        ExprOp::Multiply => arith(v, |x, y| x * y, i16::wrapping_mul),
        ExprOp::Divide if b == 0 => fail(ExprError::DivideByZero),
        ExprOp::Divide => arith(v, |x, y| x / y, i16::wrapping_div),

        // These need a 16-bit variable.
        ExprOp::Modulo if lf.is_some() => fail(ExprError::NotWritable),
        ExprOp::Modulo if b == 0 => fail(ExprError::DivideByZero),
        ExprOp::Modulo => {
            // Of the absolute values (the original makes both positive first; -32768 stays
            // itself, as its 16-bit store keeps it).
            let x = (a as i32).abs() as i16 as i32;
            let y = (b as i32).abs() as i16 as i32;
            put(v, (x - (x / y) * y) as i16)
        }
        ExprOp::And if lf.is_some() => fail(ExprError::NotWritable),
        ExprOp::And => put(v, a & b),
        ExprOp::SetFlag | ExprOp::ClearFlag if b > 16 => fail(ExprError::FlagTooHigh),
        ExprOp::SetFlag | ExprOp::ClearFlag if lf.is_some() => fail(ExprError::NotWritable),
        ExprOp::SetFlag => put(v, (a as u16 | flag_mask(b)) as i16),
        ExprOp::ClearFlag => put(v, (a as u16 & !flag_mask(b)) as i16),

        // Step by one, then compare. On a float variable the original compares a word read
        // through a null pointer; here the new value is compared (plan.md, deliberate
        // differences).
        ExprOp::IncrementLess | ExprOp::DecrementGreater => {
            let up = op == ExprOp::IncrementLess;
            let (n, w) = match lf {
                Some(x) => {
                    let f = x + if up { 1.0 } else { -1.0 };
                    // The C++ conversion: to 32 bits, then the low 16.
                    (f as i32 as i16, put_float(v, f))
                }
                None => {
                    let n = if up { a.wrapping_add(1) } else { a.wrapping_sub(1) };
                    (n, put(v, n))
                }
            };
            if w.result != PrimResult::True {
                return w;
            }
            ok(if up { n < b } else { n > b })
        }
    }
}
