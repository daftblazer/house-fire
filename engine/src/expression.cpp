#include "housefire/expression.h"

#include <algorithm>
#include <cstdlib>

namespace housefire {

ExpressionOperand ExpressionOperand::of(const Node& n) {
    ExpressionOperand e;
    e.lhs_data = (int16_t)n.operand16(0);
    e.rhs_data = (int16_t)n.operand16(1);
    e.op = n.operand[5];
    e.lhs_scope = n.operand[6];
    e.rhs_scope = n.operand[7];
    return e;
}

bool is_motive_scope(uint8_t scope) { return scope == 14 || scope == 15 || scope == 17; }

namespace {

ExprResult ok(bool b) { return {b ? PrimResult::True : PrimResult::False, ExprError::None}; }
ExprResult fail(ExprError e) { return {PrimResult::Stop, e}; }

// The flag operators' bit: flag n is bit n - 1 (flag 0 is out of range, as in the original:
// its shift then takes the sign).
bool flag_test(int16_t value, int16_t flag) {
    const uint32_t n = (uint32_t)(flag - 1) & 63;
    const int32_t x = value;
    return n >= 32 ? x < 0 : ((x >> n) & 1) != 0;
}
uint16_t flag_mask(int16_t flag) {
    const uint32_t n = (uint32_t)(flag - 1);
    return (n & 0x20) ? 0 : (uint16_t)(1u << (n & 31));
}

}  // namespace

ExprResult expression(const ExpressionOperand& e, Variables& v) {
    const std::optional<Variable> r = v.get(e.rhs_scope, e.rhs_data);
    if (!r) return fail(ExprError::NoVariable);
    const std::optional<Variable> l = v.get(e.lhs_scope, e.lhs_data);
    if (!l) return fail(ExprError::NoVariable);
    const int16_t a = l->value, b = r->value;
    const bool is_float = l->f.has_value();

    // A result written to the left side: 16-bit, or for a float variable its float (motives
    // kept between -100 and 100).
    auto put = [&](int16_t x) { return v.set(e.lhs_scope, e.lhs_data, x) ? ok(true) : fail(ExprError::NotWritable); };
    auto put_float = [&](float x) {
        if (is_motive_scope(e.lhs_scope)) x = std::clamp(x, -100.0f, 100.0f);
        return v.set_float(e.lhs_scope, e.lhs_data, x) ? ok(true) : fail(ExprError::NotWritable);
    };
    const float fb = (float)b;

    switch ((ExprOp)e.op) {
        // Comparisons take the 16-bit values, float variables included.
        case ExprOp::Greater: return ok(a > b);
        case ExprOp::Less: return ok(a < b);
        case ExprOp::Equal: return ok(a == b);
        case ExprOp::GreaterEqual: return ok(a >= b);
        case ExprOp::LessEqual: return ok(a <= b);
        case ExprOp::NotEqual: return ok(a != b);
        case ExprOp::FlagSet:
            if (b > 16) return fail(ExprError::FlagTooHigh);
            return ok(flag_test(a, b));

        case ExprOp::Assign: return is_float ? put_float(fb) : put(b);
        case ExprOp::Add: return is_float ? put_float(*l->f + fb) : put((int16_t)(a + b));
        case ExprOp::Subtract: return is_float ? put_float(*l->f - fb) : put((int16_t)(a - b));
        case ExprOp::Multiply: return is_float ? put_float(*l->f * fb) : put((int16_t)(a * b));
        case ExprOp::Divide:
            if (b == 0) return fail(ExprError::DivideByZero);
            return is_float ? put_float(*l->f / fb) : put((int16_t)(a / b));

        // These need a 16-bit variable.
        case ExprOp::Modulo: {
            if (is_float) return fail(ExprError::NotWritable);
            if (b == 0) return fail(ExprError::DivideByZero);
            // Of the absolute values (the original makes both positive first).
            const int16_t x = (int16_t)std::abs((int)a), y = (int16_t)std::abs((int)b);
            return put((int16_t)(x - (x / y) * y));
        }
        case ExprOp::And:
            if (is_float) return fail(ExprError::NotWritable);
            return put((int16_t)(a & b));
        case ExprOp::SetFlag:
            if (b > 16) return fail(ExprError::FlagTooHigh);
            if (is_float) return fail(ExprError::NotWritable);
            return put((int16_t)(a | flag_mask(b)));
        case ExprOp::ClearFlag:
            if (b > 16) return fail(ExprError::FlagTooHigh);
            if (is_float) return fail(ExprError::NotWritable);
            return put((int16_t)(a & ~flag_mask(b)));

        // Step by one, then compare. On a float variable the original compares a word read
        // through a null pointer; here the new value is compared (plan.md, deliberate
        // differences).
        case ExprOp::IncrementLess:
        case ExprOp::DecrementGreater: {
            const int step = e.op == (uint8_t)ExprOp::IncrementLess ? 1 : -1;
            int16_t n;
            ExprResult w;
            if (is_float) {
                const float f = *l->f + (float)step;
                w = put_float(f);
                n = (int16_t)f;
            } else {
                n = (int16_t)(a + step);
                w = put(n);
            }
            if (w.result != PrimResult::True) return w;
            return ok(step > 0 ? n < b : n > b);
        }
    }
    return fail(ExprError::UnknownOperator);
}

}  // namespace housefire
