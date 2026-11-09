// The expression primitive (opcode 2): most of what the scripts do (docs/primitives.md).
// One operator between two variables, each named by a scope and a datum; the variables
// themselves are the host's (Variables), so that this is the same for every game.
//
// The rules are Bustin' Out's TryExpression (fn_80213890), read from its code and checked
// against the recompiled game (docs/interpreter.md, "The expression primitive").
#pragma once
#include <cstdint>
#include <optional>

#include "housefire/bhav.h"
#include "housefire/interp.h"

namespace housefire {

// The operand: the left and right data (16 bits each), a byte the primitive does not read,
// the operator, the left and right scopes.
struct ExpressionOperand {
    int16_t lhs_data = 0, rhs_data = 0;
    uint8_t op = 0, lhs_scope = 0, rhs_scope = 0;
    static ExpressionOperand of(const Node& n);
};

enum class ExprOp : uint8_t {
    Greater = 0, Less = 1, Equal = 2, Add = 3, Subtract = 4, Assign = 5, Multiply = 6,
    Divide = 7, FlagSet = 8, SetFlag = 9, ClearFlag = 10, IncrementLess = 11, Modulo = 12,
    And = 13, GreaterEqual = 14, LessEqual = 15, NotEqual = 16, DecrementGreater = 17,
};

// A variable as a primitive sees it: its 16-bit value, and for a float variable (a motive)
// the float, which arithmetic then works on.
struct Variable {
    int16_t value = 0;
    std::optional<float> f;
};

// The variables a primitive reads and writes, by scope and datum.
class Variables {
public:
    virtual ~Variables() = default;
    // Nothing if there is no such variable.
    virtual std::optional<Variable> get(uint8_t scope, int16_t data) = 0;
    // False if the variable cannot be written.
    virtual bool set(uint8_t scope, int16_t data, int16_t value) = 0;
    virtual bool set_float(uint8_t scope, int16_t data, float value) = 0;
};

// The primitive's error codes (the original stores them in its interpreter, +48).
enum class ExprError : int16_t {
    None = 0, UnknownOperator = 1, FlagTooHigh = 13, NotWritable = 19, DivideByZero = 38,
    NoVariable = -1,  // a lookup failed (the original's lookup reports its own error)
};

struct ExprResult {
    PrimResult result = PrimResult::Stop;
    ExprError error = ExprError::None;
};

ExprResult expression(const ExpressionOperand& e, Variables& v);

// The motive scopes, whose float results are kept between -100 and 100.
bool is_motive_scope(uint8_t scope);

}  // namespace housefire
