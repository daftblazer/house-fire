//! The expression primitive's rules (`docs/interpreter.md`, "The expression primitive"), on
//! variables made here: the C++ engine's `test_expression`, check for check. The common cases
//! are checked against the recompiled game by lockstep; these are the edges a session may not
//! reach.

use std::collections::BTreeMap;

use housefire::expression::{ExprError, ExprOp, ExprResult, ExpressionOperand, Variable, Variables, expression};
use housefire::interp::PrimResult;

/// Scope 7 is a literal (read-only), 25 a local, 15 a motive (float).
#[derive(Default)]
struct Vars {
    locals: BTreeMap<i16, i16>,
    motives: BTreeMap<i16, f32>,
}

impl Variables for Vars {
    fn get(&mut self, scope: u8, data: i16) -> Option<Variable> {
        match scope {
            7 => Some(Variable { value: data, f: None }),
            25 => Some(Variable { value: *self.locals.entry(data).or_default(), f: None }),
            15 => {
                let m = *self.motives.entry(data).or_default();
                Some(Variable { value: m as i32 as i16, f: Some(m) })
            }
            _ => None,
        }
    }
    fn set(&mut self, scope: u8, data: i16, v: i16) -> bool {
        scope == 25 && {
            self.locals.insert(data, v);
            true
        }
    }
    fn set_float(&mut self, scope: u8, data: i16, v: f32) -> bool {
        scope == 15 && {
            self.motives.insert(data, v);
            true
        }
    }
}

fn run(v: &mut Vars, op: u8, ls: u8, ld: i16, rs: u8, rd: i16) -> ExprResult {
    let e = ExpressionOperand { op, lhs_scope: ls, lhs_data: ld, rhs_scope: rs, rhs_data: rd };
    expression(&e, v)
}

use ExprOp::*;
const T: PrimResult = PrimResult::True;
const F: PrimResult = PrimResult::False;

#[test]
fn the_expression_primitives_rules() {
    let mut v = Vars::default();
    let o = |op: ExprOp| op as u8;
    v.locals.insert(0, 7);
    assert_eq!(run(&mut v, o(Greater), 25, 0, 7, 3).result, T);
    assert_eq!(run(&mut v, o(Equal), 25, 0, 7, 3).result, F);
    assert!(run(&mut v, o(Add), 25, 0, 7, 32767).result == T && v.locals[&0] == 7i16.wrapping_add(32767));

    // modulo: of the absolute values
    v.locals.insert(1, -7);
    assert!(run(&mut v, o(Modulo), 25, 1, 7, 3).result == T && v.locals[&1] == 1);
    v.locals.insert(1, 7);
    assert!(run(&mut v, o(Modulo), 25, 1, 7, -3).result == T && v.locals[&1] == 1);

    // division by zero is error 38, for both kinds of variable
    assert_eq!(run(&mut v, o(Divide), 25, 1, 7, 0).error, ExprError::DivideByZero);
    assert_eq!(run(&mut v, o(Modulo), 25, 1, 7, 0).error, ExprError::DivideByZero);
    assert_eq!(run(&mut v, o(Divide), 15, 0, 7, 0).error, ExprError::DivideByZero);

    // flags: flag n is bit n - 1; above 16 is error 13
    v.locals.insert(2, 0);
    assert!(run(&mut v, o(SetFlag), 25, 2, 7, 1).result == T && v.locals[&2] == 1);
    assert!(run(&mut v, o(SetFlag), 25, 2, 7, 16).result == T && v.locals[&2] == 0x8001u16 as i16);
    assert_eq!(run(&mut v, o(FlagSet), 25, 2, 7, 16).result, T);
    assert_eq!(run(&mut v, o(FlagSet), 25, 2, 7, 2).result, F);
    assert_eq!(run(&mut v, o(FlagSet), 25, 2, 7, 0).result, T); // flag 0: the sign
    assert!(run(&mut v, o(ClearFlag), 25, 2, 7, 1).result == T && v.locals[&2] == 0x8000u16 as i16);
    assert_eq!(run(&mut v, o(SetFlag), 25, 2, 7, 17).error, ExprError::FlagTooHigh);

    // step and compare
    v.locals.insert(3, 4);
    assert!(run(&mut v, o(IncrementLess), 25, 3, 7, 5).result == F && v.locals[&3] == 5);
    assert!(run(&mut v, o(DecrementGreater), 25, 3, 7, 3).result == T && v.locals[&3] == 4);

    // a literal cannot be written: error 19
    assert_eq!(run(&mut v, o(Assign), 7, 1, 7, 2).error, ExprError::NotWritable);
    // an unknown operator: error 1
    assert_eq!(run(&mut v, 40, 25, 0, 7, 1).error, ExprError::UnknownOperator);
    // a missing variable
    assert_eq!(run(&mut v, o(Assign), 99, 0, 7, 1).error, ExprError::NoVariable);

    // motives: float arithmetic, kept between -100 and 100; comparisons on the 16-bit value
    v.motives.insert(0, 99.5);
    assert!(run(&mut v, o(Add), 15, 0, 7, 2).result == T && v.motives[&0] == 100.0);
    assert!(run(&mut v, o(Subtract), 15, 0, 7, 300).result == T && v.motives[&0] == -100.0);
    v.motives.insert(0, 50.75);
    assert_eq!(run(&mut v, o(Equal), 15, 0, 7, 50).result, T);
    assert!(run(&mut v, o(Divide), 15, 0, 7, 2).result == T && v.motives[&0] == 25.375);
    assert_eq!(run(&mut v, o(And), 15, 0, 7, 1).error, ExprError::NotWritable); // 16-bit only
}
