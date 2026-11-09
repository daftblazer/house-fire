// Primitives other than the expression (expression.h), each on the world (world.h).
#pragma once
#include "housefire/bhav.h"
#include "housefire/interp.h"
#include "housefire/world.h"

namespace housefire {

// SetToNext (opcode 31): the next object after the target's in the world's list order (or
// the first, if the target is 0) of a kind: 0 any object, 1 a Sim, 2 not a Sim, 4 an object
// of the GUID in operand bytes 0-3. The target is the stack object, or with flag 0x80 in
// byte 4 the variable of scope byte 5, datum byte 7. True and the target set if there is
// one; false if not. Other kinds are not modelled yet: Stop. Measured on the recompiled
// game (docs/objects.md, "SetToNext").
PrimResult set_to_next(const Node& n, World& world, ScriptVariables& vars);

}  // namespace housefire
