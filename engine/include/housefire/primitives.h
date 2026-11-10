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

// Random (opcode 8): a number from 0 up to the range (operand bytes 4-5 its datum, byte 6 its
// scope) into the destination (bytes 0-1 its datum, byte 2 its scope); true. The numbers are
// the engine's own (World::rng). Measured: every logged result within its range
// (docs/primitives.md, "Random").
PrimResult random(const Node& n, World& world, ScriptVariables& vars);

// CallNamedTree (opcode 28): the tree whose name is a string (operand bytes 0-1 its STR#,
// byte 4 its index) of a file (bytes 2-3: 1 the global file, 2 the semi-global, 3 the
// object's own; otherwise the calling tree's own kind), called on the stack object
// (Castaway's TryCallNamedTree and CallNamedTreeParam). No such string, or no tree of that
// name: false. A tree found: Stop, as its call is not modelled (no logged use finds one).
PrimResult call_named_tree(const Node& n, const Frame& f, const Library& lib);

// TestObjectType (opcode 32): whether the object whose id a variable holds (operand bytes
// 4-5 the datum, byte 6 the scope) is of the GUID in bytes 0-3. An id no object has: false.
// Measured on the recompiled game (docs/primitives.md, "TestObjectType").
PrimResult test_object_type(const Node& n, World& world, ScriptVariables& vars);

// Idle (opcode 0, the PC's Sleep): the stack sleeps for as many of the tick's passes as the
// frame's parameter (operand: its index) holds, yielding: passed over by the next N - 1 that
// reach it, stepped on the N-th. Then the parameter is 0 and the node goes to its true target.
// A missing parameter, or one of 0 or less: no sleep. Measured: all 2,810 regular sleeps of a
// log with the pass's cursor (docs/primitives.md, "Idle").
PrimResult idle(Interpreter& in, Frame& f, const Node& n);

}  // namespace housefire
