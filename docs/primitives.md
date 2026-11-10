# Behaviour script primitives

A BHAV node's opcode below 256 is a primitive; 256 and up call another tree (256-4095
global, 4096-8191 the object's own, 8192 and up its semi-global file). The tables are in
`tools/primitives.py`: the names (the console engine's own, `cXObjectImpl::Try<Name>` and
`cXPersonImpl::Try<Name>` in The Sims 2's map) and, per game, the opcodes its interpreter
accepts, read from its executable by `tools/dispatch.py`.

## One numbering

The four games with scripts (The Sims keeps its objects another way, `survey.md`) number
the primitives alike: wherever a game's handler matches a named one by its code, it is at
the same opcode. Each game adds its own at the top:

| | Object switch | Person switch | Accepted | Highest |
|---|---|---|---|---|
| Bustin' Out | `8021242C` | `802246B0` | 55 | 57 (person) |
| The Urbz | `80165964` | `80180E1C` | 63 | 61, and person 100 |
| The Sims 2 | `80109770` | `80124ADC` | 69 | 67, and person 100 |
| Pets | `800EC050` | `80105D48` | 68 | 70, and person 71 |

Opcode 57 is the one known exception to one-name-per-opcode: a person primitive in Bustin'
Out, `HUDCreate` in The Urbz. The Sims 2 adds goals (56), the HUD (58-60), particles (61),
social mode (62), memories (63), idle animation (64), passive influence (65), wants and
fears (66, 67); Pets 68-71, not named yet.

## What the scripts use (`tools/bhav_text.py`)

Every script on the four discs checked: node targets are nodes of their tree or a return
(253 error, 254 true, 255 false), every primitive used is one its game accepts (one dead
script in Pets names the Game Boy link, 54, which Pets dropped), and every call to an
object's own tree finds it (four from semi-global trees, which resolve in the calling
object).

| | Trees | Primitive nodes | Expression | Animate | Calls (global / own / semi-global) |
|---|---|---|---|---|---|
| Bustin' Out | 9,529 | 96,505 | 72% | 5% | 6,060 / 7,797 / 4,554 |
| The Urbz | 4,907 | 51,227 | 68% | 9% | 5,602 / 2,129 / 4,108 |
| The Sims 2 | 5,148 | 59,289 | 69% | 10% | 6,440 / 1,551 / 4,349 |
| Pets | 5,272 | 61,606 | 70% | 10% | 7,321 / 1,898 / 3,878 |

So the expression primitive (2) is most of what the interpreter does; then animation (44),
change suit (6), sounds (23), set-to-next (31), dialogs (36), random (8). A few opcodes are
accepted and never used by any script (Bustin' Out: 53, 57).

## Idle (opcode 0)

The PC's Sleep. The operand's first 16 bits are a parameter's index (the rest 0xFFFF). The
stack sleeps for as many of the tick's passes as that parameter holds (`simulation.md`): the
step yields, the next N − 1 passes that reach the object pass over it, and the N-th steps it.
Then the parameter is 0 and the node goes to its true target. A parameter of 0 or less: true
at once.

Measured on the recompiled game:
- In a session of the new-game route, 11,950 of 12,257 wake-ups come exactly N ticks after
  the yield; 307 later, by a few lengths (227, 101, 168, 17, 269 ticks).
- With the pass's cursor logged, all 2,810 regular sleeps of a session are passed over by
  exactly N − 1 passes, the late ones included: the passes in between stopped at a dialog and
  did not reach the object. 23 others ended early, woken by something else.

The engine has it as `idle()` (`engine/src/primitives.rs`), the interpreter's `Sleep` and the
simulator's pass. `engine/tests/primitives.rs` and `engine/tests/simulator.rs` test it.

## Relationship2 (opcode 26): not native yet

Reads and writes the relationships between objects (each object's saved RelMatrix, `lots.md`)
and between neighbours (NBRS's). 128 uses in the session, with six operand forms. For the
commonest get (`00 02 01 00 08 00 00 00`, 27 uses) the step is true exactly when the object
has a saved relationship with the stack object (5 of 5), false otherwise (22 of 22). The
other forms do not follow from the operand's obvious reading (one of them is both true and
false between objects with no relationship). Castaway's `cXObject::TryRelationship2` (0xAA8
bytes) shows why: modes in operand byte 1 (0, 2, 4 ...), objects or neighbours
(`Relationship2Param::GetUseNeighbors`), a create flag (`GetCreate`), pet ownership. It is
the relationship system, to come as one; the engine keeps the saved relationships
(`Object::relationships`; the club's 16) for it.

## CallNamedTree (opcode 28)

Calls a tree by name. The operand (Castaway's `CallNamedTreeParam`): bytes 0-1 a string
table (STR#), bytes 2-3 which file's names (1 the global file, 2 the semi-global, 3 the
object's own; otherwise the calling tree's own kind), byte 4 the string's index. The tree with
that name, in that file, is called on the stack object (`cXObject::TryCallNamedTree`). No
such string, or no tree of that name: false.

The index counts from 1, and the name is looked up in the **stack object's** files (its own,
its semi-global, the global file), not the one the strings are from: Global 430 ("PS2 -
Story - Am I Dudley?") asks for Global's STR# 303 string 57, "CT - Story - Am I Dudley?",
which the log runs as ScenarioGlobals 8251, the semi-global of its stack object (Mom's
house's scenario controller). The call is a Gosub: the tree pushed in that file, its return
taking the node's true or false way.

Measured with `replay`: at Mom's house 850 calls made natively and 10 uses with no tree the
same (2 different, the log's taken); the club's 116 uses, all without a tree, the same as
before (a name the stack object's files lack).

The engine has it as `call_named_tree()`; the library keeps each file's string tables and its
trees by name.

## PlaySound (23), Update (7), KillSounds (48)

Requests to the presentation: a sound to play, the object's picture to refresh, sounds to stop.
For the scripts they are true and nothing else: in the session all 270 uses (100, 168 and
2) are true, never yield and never change their frame. The engine queues each on the world
(`World::presentation`: the primitive, the object, its operand) for the front ends to take,
and goes on; the simulation never waits on them, so its run is the same with or without a
front end. What each operand asks (which sound, which refresh) is the front ends' to read.

The engine has them as `present()`.

## SetMotiveDelta (opcode 29) and the motives: not checkable in the logs yet

The operand: byte 0 the scope of the change, byte 1 the scope of its limit, byte 2 the
motive, bytes 4-5 and 6-7 the two data. In the session its 12 uses are the dance floor's
dance (tree 4107) on four Sims, three each: motive 15 by 45 towards 75, motive 7 by 15
towards -20, motive 14 by 25 towards 10 (all constants, scope 26).

The motives themselves hardly move in the logs: of 47 motive variables read (2,829 reads in
the 5,130-tick session), one changes (a Sim's motive 15: 6, 12, 20 at ticks 696, 1,790 and
3,117), and none in the 719-tick one. That is too little to check a motive system against;
a session with the household at home, motives running, is the one to log for it.

## CompleteGoal (opcode 56): the goal system's

Asks about a goal's state. Castaway's `CompleteGoalParam` names the operand: a 16-bit
operation, a 16-bit goal datum and its owner (the scope), and a flags byte (status into
temp 0; true if the goal is locked, open, satisfied or completed; the low two bits an old
form), then a type. In the session all 2,377 uses (the party controller and the dance floor's
tiles, every few ticks) are false, all of the old form, the goal named by constants. Native,
it needs the goals' state (the player's progress, in the save): the goal system's, to come
with it. Until then the replay takes the log's answer.

## Random (opcode 8)

A random number into a variable. The operand, as 16-bit little-endian fields: bytes 0-1 the
destination's datum, byte 2 its scope; bytes 4-5 the range's datum, byte 6 its scope (a
literal, 7, in every logged use). The destination gets a number from 0 up to the range, and
the node goes to its true target.

Measured with `replay`: all 339 uses (269 and 70 in two sessions) true natively; of those
whose destination the log shows (a parameter or a local), all 294 numbers within their
range. The numbers themselves are the engine's own, a deliberate difference (`plan.md`); the
replay takes the original's. A range of 0 or less gives 0 (not seen in the logs).

The engine has it as `random()` (`engine/src/primitives.rs`).

## TestObjectType (opcode 32)

Whether an object is of a type. The operand: a GUID (bytes 0-3, in the file's byte order),
then a variable that holds the object's id (bytes 4-5 the datum, byte 6 the scope). True if
the object with that id is of that GUID; false otherwise, and for an id no object has.

Measured with `replay` on the 5,130-tick session: all 181 uses the same natively. All of them
are false there (tests of the Sims against one GUID, of one object against another), so the
true case is checked only by `engine/tests/primitives.rs` so far.

The engine has it as `test_object_type()` (`engine/src/primitives.rs`).

## Animate (opcode 44): what the scripts see

Measured on the recompiled game, a session of the new-game route: 23,811 steps, 628 runs of
102 animations.

- **While the animation plays, the step yields once every tick** (23,183 yields).
- **A run ends true** (526) **or false** (102).
- **Flag 0x20 in operand byte 5:** an animation event ends the step false with the event in
  local 0 (74 runs). The script handles the event and calls Animate again.
- **The flags byte** is 0 (12,909 steps), 0x20 (10,447) or 0x22 (455).
- **Operand bytes 0-1** are the animation.
- **A run lasts as long as its animation:** 44 of the 102 animations run the same number of
  ticks every time (12, 13, 16, 20, 24, 28 ... 92). The others vary, as their events and
  continuations would explain.

This is the contract the animation system must keep (each animation's length in ticks and
its events, from the animation data, which is not decoded yet).

Where the animations are (the fifth loop): the object files' "animations" string tables
(STR# 128) are empty on the consoles. An Animate operand's first 16 bits index the quick
data's `AnimRef` table (`SimsObjects`: 2,891 rows of 12 bytes, a 32-bit id, a 32-bit value,
an empty name), which `AnimRefName` (2,840 pointers into it) also reaches. The animations
themselves are `animatio.arc`'s 4,013 entries, by name ("a2a-tickler-stop-laugh" ...). Which
entry an AnimRef id names is not found yet: the id is not the archive's own name hash, nor
CRC-32, FNV-1/1a, djb, sdbm, ELF or a ×31/37/131/65599 hash of the name (as is, lower or
upper case, with or without an extension), and the row's value does not follow the run's
length (0xA6A for 16 ticks, 0x514 for 17). Castaway's ELF has `EAnimManager` (resources by
32-bit id) to read it from. Until then each animation's length cannot be checked, and
Animate stays the log's. The primitive is a thin
wrapper over that system, so it waits for it. Animate steps also make the effect objects
(`objects.md`, "Objects made and removed").
