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

// IdleForInput (opcode 17), where a Sim waits in "Wait For Notify" (Global 281) for its next
// interaction. With an entry queued: the engine's checks on the entry's target, as its
// interaction table (TTAB) names them (the tree the table runs first, then the interaction's
// guard; run by Host::run_check), then the entry's tree
// (the target's) pushed on the stack with the target as stack object and zero parameters;
// the entry leaves the queue, and the step goes on into it (Continue; this node stays, and
// the interaction's return comes back to it). docs/simulation.md, "Where a Sim's
// interactions come in". Not modelled yet: nothing queued (autonomy's, or a wait), a check
// that fails, a host that cannot run checks: Stop.
PrimResult idle_for_input(Interpreter& in, const Node& n, Object& me, World& world, Host& host);

// PushAction (opcode 13): queues an interaction for the Sim that is the stack object: the
// interaction in place byte 0 of the target's interaction table (its file's TTAB), the
// target the object whose id local byte 1 holds; true. Measured: the interactions the
// controller pushes onto the club's Sims are the ones IdleForInput then delivers
// (docs/simulation.md). Not modelled yet: operand bytes 2 and 3 (1 or 2; 0x0a), a target
// whose file has several TTABs: Stop.
PrimResult push_action(Interpreter& in, Frame& f, const Node& n, World& world);

// Dialog (opcode 36): opens a dialog for the player (a presentation event, the world's
// dialog_object) and waits for the answer (World::dialog_answer): each step until then
// yields and asks the pass to stop (World::stop_pass); the answer given, the node goes to its
// true target. Measured (replay): all 2,471 waits and 4 answers of the session as the log's,
// every answer true (the dialogs' one button); docs/simulation.md, "Dialogs".
PrimResult dialog(const Node& n, World& world, const Object& me);

// SetMotiveDelta (opcode 29): a running change of one of the Sim's (`me`) motives. Its
// operand (Castaway's SetMotiveDeltaParam): byte 0 the change's scope and field 4 its datum
// (a change an hour), byte 1 the stop value's scope and field 6 its datum, byte 2 the motive,
// byte 3 flags (1 clear all first; 2 and 4 an influence on the Sim or the stack object, not
// this, nullopt). The change a tick is the hour's / 1800 (30 ticks a second, a game hour a
// real minute). Measured: the home log's one use, Sim 237 at the easel, fun +48 an hour to
// 50: its 593 steps of +0.026667 the same (docs/simulation.md, "The motives").
std::optional<PrimResult> set_motive_delta(const Node& n, Object& me, ScriptVariables& vars);

// A person's running motive changes, as one of its turns begins: each moves its motive by
// its change, not past its stop value.
void apply_motive_deltas(Object& person);

// PlaySound (23), Update (7) and KillSounds (48): requests to the presentation, queued on the
// world for the front ends (World::presentation); the step is true. In the logs they are true
// every time (100, 168 and 2 uses) and never change their frame.
PrimResult present(const Node& n, World& world, const Object& me);

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
