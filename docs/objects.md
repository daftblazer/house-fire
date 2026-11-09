# The object model: what the scripts' variables are

A script names a variable by a scope and a datum (the expression primitive's two sides, and
other primitives' operands). What each scope names was measured on the recompiled Bustin'
Out. Its log (`BUSTIN_TREE_LOG`) records every variable lookup the original makes (its
`InterpValue`, `fn_80214110`):

- the object asked for;
- the scope and datum;
- the value;
- the address the variable was found at.

`tools/variables.py` reads that. The engine's model is `engine/include/housefire/world.h`:
`World`, `Object` and `ScriptVariables`. The layout is the engine's own; what each scope
reaches is the original's.

## The scopes

The original's layout is relative to the object a lookup is for. Names are the PC game's
where the measurements agree.

| Scope | | In the original | Native |
|---|---|---|---|
| 0 | my attribute | a table of the object's own, allocated apart | `Object::attributes` |
| 1 | stack object's attribute | the stack object's table | the same, of the stack object |
| 3 | my object data | object + 0x26, 2 bytes a field; field 11 is the object's id | `Object::data` |
| 4 | stack object's data | the stack object's | the same |
| 6 | global | one table for everyone | `World::globals` |
| 7 | literal | the datum itself | |
| 8 | temp | object + 0x16, eight of them | `Object::temps` |
| 9 | parameter | the frame's (+16 on) | `Frame::params` |
| 10 | stack object | the frame's header, +4; a call's is its caller's | `Frame::stack_object` |
| 14 | my motive | a float, 4 bytes, before the object part of a Sim (object - 0x598) | `Object::motives` |
| 15 | stack object's motive | the stack object's | the same |
| 18 | my person data | 2 bytes a field, before the object part (object - 0x638) | `Object::person_data` |
| 19 | stack object's person data | the stack object's | the same |
| 22 | stack object's attribute by parameter | the stack object's attribute named by a parameter | |
| 25 | local | the frame's, after its parameters | `Frame::locals` |
| 26 | constant | the frame's file's BCON tables (`interpreter.md`, "Constants") | `Library::constant` |
| 30 | my person data by temp | the person data field a temp names | |
| 34 | neighbourhood data | one table for everyone | `World::neighbourhood` |

Not modelled yet:
- 12: one table for everyone, PC "check tree ad range".
- 16, 20: slots.
- 21: the stack object's definition.
- 23, 24, 32, 40.

The stack object is an object id (scope 10). The object it names is the one whose data
field 11 holds that id: 17,364 of 17,364 lookups agree. A Sim's motives and person data sit
in front of its object part, so a Sim is an object with more.

Constants come from the frame's file. That's the tree's object, not necessarily the one the
script runs on: a Sim running a fridge's interaction reads the fridge's constants.

## Checked against the original (`tools/lockstep`, "variables")

The native world gets an object for each one the original asks for. Each variable takes its
value from the log the first time it is seen. After that, every value the original's
expressions read must be the native world's, given the native writes. And every variable
must always be the same address in the original, and every address the same variable:
that checks what each scope names.

Primitives other than the expression ask only for an address, so their reads are not
compared, and what they write is taken as new.

A session of the new-game route (the dream lot, then the club from tick 2799; 2026-11-09):

| | Same | Different |
|---|---|---|
| Literals, constants, parameters, locals, the stack object id | 191,994 | 0 |
| Values read from objects' and shared tables (scopes 0-4, 6, 8, 14-22, 34) | 122,184 | 0 |
| Where a variable is (each cell one address and back) | 88,684 | 9 |
| Stack object resolved to the same object | 28,084 | 7 |

Changed between the scripts' own accesses ("changed by the engine"):
- object data, 1,380 times (scope 3: position, flags and so on);
- motives and person data, about 300 times;
- temps, 209 times;
- globals, 82 times.

Those are the engine's own state, to be modelled with the systems that change them: time,
routing, animation, the motives' decay.

Left over:
- About 380 differences while a lot loads (within 25 ticks of it), counted apart: the
  engine sets the new lot's objects up there, their ids among them.
- 16 differences later in the club. Objects are also created during play (11 in the
  session), and a new object can take a deleted one's pointer or id. That is the object
  primitives' work (create, remove), not yet modelled.

## Sizes

How many attributes, object data and person data fields an object has is not taken from the
game yet: the native tables grow to the highest index used. Object data fields seen go up to
61, person data up to 74 (80 fit before the motives); motives up to 15.

## SetToNext (primitive 31) and the object list

The world's objects are in a list. A lot's objects come in its table's order
(`lots.md`), and objects made later go in front of them all. SetToNext gives the next
object after the target's in that order, or the first if the target is 0, of a kind:

| Kind | | Checked (a session of the new-game route) |
|---|---|---|
| 0 | any object | 23,016 same |
| 1 | a Sim (here: an object whose definition is not on the disc, a character) | 2,144 same |
| 2 | not a Sim | not run in the session |
| 3 | the other part of a multi-tile object | not modelled (89 steps) |
| 4 | an object of the GUID in operand bytes 0-3 | 10,903 same |
| 5 | neighbours, ascending | not modelled (468 steps) |
| 6 | (open: in the club it finds the DJ booth; the operand's 0x0BE8F446 is no definition's GUID, and no definition or data field of the objects found equals temp 0) | not modelled (2,188 steps) |

The target is the stack object, or with flag 0x80 in operand byte 4 the variable of scope
byte 5, datum byte 7. The step is true with the target set, or false if there is no next.

Checked on a world loaded natively from the club lot (`World::load_lot`). The lot is picked
by the order of the original's longest walk; object ids are reused between lots and don't
tell them apart. In this session the club lot is loaded at tick 0 and again at 2799, each
time with two objects the lot does not list (179, 180, not Sims) made in front.

52 steps differ, all at ticks 0-6 while the lot loads: objects the original has made by
then (the two in front, and others such as 141) are not yet made natively. That is chunk 2's
(the objects made and removed).
