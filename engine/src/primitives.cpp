#include "housefire/primitives.h"

namespace housefire {

PrimResult set_to_next(const Node& n, World& world, ScriptVariables& vars) {
    const auto& o = n.operand;
    const uint8_t kind = o[4] & 0x7F;
    const uint8_t scope = (o[4] & 0x80) ? o[5] : (uint8_t)Scope::StackObject;
    const int16_t data = (o[4] & 0x80) ? (int16_t)o[7] : 0;
    // The GUID in the file's byte order (Bustin' Out's files are little-endian).
    const uint32_t guid = n.order == Order::Little
                              ? (uint32_t)o[0] | o[1] << 8 | o[2] << 16 | (uint32_t)o[3] << 24
                              : (uint32_t)o[0] << 24 | o[1] << 16 | o[2] << 8 | o[3];
    auto fits = [&](const Object& x) {
        switch (kind) {
            case 0: return true;
            case 1: return x.person;
            case 2: return !x.person;
            case 4: return x.guid == guid;
            default: return false;
        }
    };
    if (kind != 0 && kind != 1 && kind != 2 && kind != 4) return PrimResult::Stop;
    const std::optional<Variable> cur = vars.get(scope, data);
    if (!cur) return PrimResult::Stop;
    const auto& list = world.objects();
    size_t i = 0;
    if (cur->value != 0)
        for (size_t k = 0; k < list.size(); k++)
            if (list[k]->id() == cur->value) { i = k + 1; break; }
    for (; i < list.size(); i++)
        if (fits(*list[i])) return vars.set(scope, data, list[i]->id()) ? PrimResult::True : PrimResult::Stop;
    return PrimResult::False;
}

PrimResult idle(Interpreter& in, Frame& f, const Node& n, uint32_t now) {
    const int16_t index = (int16_t)n.operand16(0);
    if (index < 0 || (size_t)index >= f.params.size()) return PrimResult::True;
    if (in.woke()) {
        in.wake();
        f.params[(size_t)index] = 0;
        return PrimResult::True;
    }
    const int16_t ticks = f.params[(size_t)index];
    if (ticks <= 0) return PrimResult::True;
    in.sleep_until(now + (uint32_t)ticks);
    return PrimResult::Yield;
}

}  // namespace housefire
