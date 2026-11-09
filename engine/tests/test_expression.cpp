// The expression primitive's rules (docs/interpreter.md, "The expression primitive"), on
// variables made here. The common cases are checked against the recompiled game by
// tools/lockstep; these are the edges a session may not reach.
#include <cstdio>
#include <map>

#include "housefire/expression.h"

using namespace housefire;

static int failures = 0;
#define CHECK(c)                                                              \
    do {                                                                      \
        if (!(c)) { std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); failures++; } \
    } while (0)

// Scope 7 is a literal (read-only), 25 a local, 15 a motive (float).
struct Vars : Variables {
    std::map<int16_t, int16_t> locals;
    std::map<int16_t, float> motives;
    std::optional<Variable> get(uint8_t scope, int16_t data) override {
        if (scope == 7) return Variable{data, {}};
        if (scope == 25) return Variable{locals[data], {}};
        if (scope == 15) return Variable{(int16_t)motives[data], motives[data]};
        return std::nullopt;
    }
    bool set(uint8_t scope, int16_t data, int16_t v) override {
        if (scope != 25) return false;
        locals[data] = v;
        return true;
    }
    bool set_float(uint8_t scope, int16_t data, float v) override {
        if (scope != 15) return false;
        motives[data] = v;
        return true;
    }
};

static ExprResult run(Vars& v, ExprOp op, uint8_t ls, int16_t ld, uint8_t rs, int16_t rd) {
    ExpressionOperand e;
    e.op = (uint8_t)op; e.lhs_scope = ls; e.lhs_data = ld; e.rhs_scope = rs; e.rhs_data = rd;
    return expression(e, v);
}

int main() {
    Vars v;
    v.locals[0] = 7;
    CHECK(run(v, ExprOp::Greater, 25, 0, 7, 3).result == PrimResult::True);
    CHECK(run(v, ExprOp::Equal, 25, 0, 7, 3).result == PrimResult::False);
    CHECK(run(v, ExprOp::Add, 25, 0, 7, 32767).result == PrimResult::True && v.locals[0] == (int16_t)(7 + 32767));

    // modulo: of the absolute values
    v.locals[1] = -7;
    CHECK(run(v, ExprOp::Modulo, 25, 1, 7, 3).result == PrimResult::True && v.locals[1] == 1);
    v.locals[1] = 7;
    CHECK(run(v, ExprOp::Modulo, 25, 1, 7, -3).result == PrimResult::True && v.locals[1] == 1);

    // division by zero is error 38, for both kinds of variable
    CHECK(run(v, ExprOp::Divide, 25, 1, 7, 0).error == ExprError::DivideByZero);
    CHECK(run(v, ExprOp::Modulo, 25, 1, 7, 0).error == ExprError::DivideByZero);
    CHECK(run(v, ExprOp::Divide, 15, 0, 7, 0).error == ExprError::DivideByZero);

    // flags: flag n is bit n - 1; above 16 is error 13
    v.locals[2] = 0;
    CHECK(run(v, ExprOp::SetFlag, 25, 2, 7, 1).result == PrimResult::True && v.locals[2] == 1);
    CHECK(run(v, ExprOp::SetFlag, 25, 2, 7, 16).result == PrimResult::True && v.locals[2] == (int16_t)0x8001);
    CHECK(run(v, ExprOp::FlagSet, 25, 2, 7, 16).result == PrimResult::True);
    CHECK(run(v, ExprOp::FlagSet, 25, 2, 7, 2).result == PrimResult::False);
    CHECK(run(v, ExprOp::FlagSet, 25, 2, 7, 0).result == PrimResult::True);   // flag 0: the sign
    CHECK(run(v, ExprOp::ClearFlag, 25, 2, 7, 1).result == PrimResult::True && v.locals[2] == (int16_t)0x8000);
    CHECK(run(v, ExprOp::SetFlag, 25, 2, 7, 17).error == ExprError::FlagTooHigh);

    // step and compare
    v.locals[3] = 4;
    CHECK(run(v, ExprOp::IncrementLess, 25, 3, 7, 5).result == PrimResult::False && v.locals[3] == 5);
    CHECK(run(v, ExprOp::DecrementGreater, 25, 3, 7, 3).result == PrimResult::True && v.locals[3] == 4);

    // a literal cannot be written: error 19
    CHECK(run(v, ExprOp::Assign, 7, 1, 7, 2).error == ExprError::NotWritable);
    // an unknown operator: error 1
    CHECK(run(v, (ExprOp)40, 25, 0, 7, 1).error == ExprError::UnknownOperator);
    // a missing variable
    CHECK(run(v, ExprOp::Assign, 99, 0, 7, 1).error == ExprError::NoVariable);

    // motives: float arithmetic, kept between -100 and 100; comparisons on the 16-bit value
    v.motives[0] = 99.5f;
    CHECK(run(v, ExprOp::Add, 15, 0, 7, 2).result == PrimResult::True && v.motives[0] == 100.0f);
    CHECK(run(v, ExprOp::Subtract, 15, 0, 7, 300).result == PrimResult::True && v.motives[0] == -100.0f);
    v.motives[0] = 50.75f;
    CHECK(run(v, ExprOp::Equal, 15, 0, 7, 50).result == PrimResult::True);
    CHECK(run(v, ExprOp::Divide, 15, 0, 7, 2).result == PrimResult::True && v.motives[0] == 25.375f);
    CHECK(run(v, ExprOp::And, 15, 0, 7, 1).error == ExprError::NotWritable);  // 16-bit only

    std::printf("test_expression: %s\n", failures ? "FAILED" : "ok");
    return failures ? 1 : 0;
}
