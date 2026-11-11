# The object model: what the scripts' variables are

A script names a variable by a scope and a datum (the expression primitive's two sides, and
other primitives' operands). What each scope names was measured on the recompiled Bustin'
Out. Its log (`BUSTIN_TREE_LOG`) records every variable lookup the original makes (its
`InterpValue`, `fn_80214110`):

- the object asked for;
- the scope and datum;
- the value;
- the address the variable was found at.

`tools/variables.py` reads that. The engine's model is `engine/src/world.rs`:
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
| 12 | (PC: check tree ad range) | one shared 16-bit value (8035ADC8; datum 15 is the only one used), set by scripts and by the engine | `World::ad_range` |
| 21 | stack object's definition | its definition row: a 32-bit version, then the OBJD's fields, so the datum is field + 2 | `Definition` |
| 22 | stack object's attribute by parameter | the stack object's attribute named by a parameter | |
| 25 | local | the frame's, after its parameters | `Frame::locals` |
| 26 | constant | the frame's file's BCON tables (`interpreter.md`, "Constants") | `Library::constant` |
| 30 | my person data by temp | the person data field a temp names | |
| 34 | neighbourhood data | one table for everyone | `World::neighbourhood` |

Not modelled yet:
- 16, 20: slots.
- 23, 24, 32, 40.

These are 583 of the session's 382,874 lookups.

Checked: scope 21 on the lot-loaded world gives the original's value in all 2,221 reads
(the datum offset found on the pool table's six parts, whose sub-indices it reads). Scope 12
holds in 4,221 reads, with 60 changes made by the engine itself.

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

## Sizes, and a lot's saved state

What fixes the table sizes, read from Bustin' Out's object stream (`fn_801F1F20`; traced):

- **Object data:** 72 fields (the code's own count).
- **Temps:** 8.
- **Attributes:** as many as the save gives: a count, then the attributes. That is not the
  definition's field 56 (the PC's attribute count), which matches only 667 of 4,519
  objects. Objects made during play presumably take their definition's (to be checked on
  one).
- **Sims:** a further table (20 values in the session's six) follows. Person data and
  motives are still grown to the highest index used: up to 74 (80 fit before the motives)
  and 15.

`World::load_lot` gives each object its state as the lot saved it: attributes, temps and
data (`LotObject`; the loader checks that data 11 is the object's id, true for all 4,572
objects of the disc's 16 lots).

Checked against the running game: the first read of each of an object's own variables,
before any expression wrote it, is the lot's value 437 times (data 199 of 199, attributes
174 of 181, temps 64 of 70). The other 13 were changed before that first read by something
other than an expression.

## SetToNext (primitive 31) and the object list

The world's objects are in a list. A lot's objects come in its table's order
(`lots.md`), and objects made later go in front of them all. SetToNext gives the next
object after the target's in that order, or the first if the target is 0, of a kind:

| Kind | | Checked (a session of the new-game route) |
|---|---|---|
| 0 | any object | 23,016 same |
| 1 | a Sim (here: an object whose definition is not on the disc, a character) | 2,144 same |
| 2 | not a Sim | not run in the session |
| 3 | the next part of the target's multi-tile object, in list order, round from the last part to the first (false if the target is not a part); a part's object is its saved leader (`lots.md`, "The classes") | 141 same (`replay`) |
| 4 | the next object of the GUID in operand bytes 0-3 by id (the definition's list, in id order), not the list's order | 10,934, 1,941 and 9,753 of 9,755 same (`lockstep`, the club's two sessions and the home log) |
| 5 | the next neighbour by id (Castaway's `Neighborhood::GetNextNeighborID`); after the last, the target 0 and false. The neighbours are NBRS's (below) | 468 same (`replay`) |
| 6 | an object whose data field 59 is the frame's first parameter (Castaway's `TrySetToNext`, which shares the case with 0 and 2; in Bustin' Out's log field 59 is the parameter for 1,984 of 2,000 objects found). The operand's bytes 0-3 (the boombox's GUID in the club's use) are not read | 2,188 and 549 same (`replay`, two sessions) |

The target is the stack object, or with flag 0x80 in operand byte 4 the variable of scope
byte 5, datum byte 7. The step is true with the target set, or false if there is no next.

Checked on a world loaded natively from the club lot (`World::load_lot`). The lot is picked
by the order of the original's longest walk; object ids are reused between lots and don't
tell them apart. In this session the club lot is loaded at tick 0 and again at 2799, each
time with two objects the lot does not list (179, 180, not Sims) made in front.

52 steps differ, all at ticks 0-6 while the lot loads: objects the original has made by
then (the two in front, and others such as 141) are not yet made natively. That is chunk 2's
(the objects made and removed).

### The neighbours (SetToNext kind 5)

Kind 5 walks the neighbours by id: from 0 the first, then the next above the target; after
the last, the target 0 and false. In the session: 51 ids, 3-63 with gaps, and the native
walk (`read_neighbours`, `World::neighbours`) the same on all 468 steps (`replay`).

The neighbours are the neighbourhood file's (`lots.md`) section 1, a chunk table like a
lot's (NGBH, NBRS, then FAMI and "User" chunks). NBRS is the PC's layout, not the save
stream: 0, the version (80), "SRBN", a count of 63 slots, then each slot:
- an empty one: a word 0;
- a neighbour: a word 1, the record's version (4), a name (zero-ended, to an even
  position: the character file, "template_person", "grimreaper", "robot", "maid" ...), an
  int, an int "mode" (set: 80 16-bit values of person data follow), 128 bytes (to be named;
  floats among them), the id (16 bits: the slot's number plus one), the GUID, -1, -2, and a
  count of relationships, each a word 1, the other neighbour's id, a count and that many
  32-bit values.

All 51 records read to the chunk's exact end, their ids the walk's (the `lots` test).

**Scope 24, "neighbour in stack object".** The stack object holds a neighbour id rather than
an object's; field 0 is the id of the person on the lot whose neighbour id (person data 31)
it is, 0 if that neighbour is not on the lot. At Mom's house the household's scripts ask it
of neighbours 34 and 50 (Sims 12 and 237) thousands of times: all 30,684 reads of the home
log the same natively, and the club logs' 38.

## Objects made and removed

The original's object list is linked through its objects. Each object keeps its manager at
+184 and its next object at +188 (`801F6580`); the manager's first object is at its +44
(`80203E1C`, reached from SetToNext through virtual calls). The observer walks this list
every tick ('L' records) and checks its head after every script step ('C').

In a session of the new-game route the club's 178 objects are the list at tick 0:
- object 179 is made at tick 85;
- 179 is removed at tick 276 (the session's one KillObject);
- 179 is made again at tick 741;
- 180 is made at tick 1684.

They are effects (`efx_smokepuff.iff`), made during Animate steps (animation events). No
script runs CreateObject in the session.

A new object takes the lowest id not in use, so a removed one's id comes back, and goes in
front of the list (`World::make_object`). (One more than the highest, the earlier rule, gave the
club's three the same ids, but not the home log's: there the paper girl leaves as 197 and comes
back as 197 below higher ids, and all 14 made objects take the lowest free id.) Replayed in the lockstep, all three
ids and the list's order at all five changes are the original's. (The home log's lockstep read the disc's lots until it
took `--card`: with the save's Mom's house, all 8 of the home log's lists are the original's, in
order, and SetToNext's kinds 0, 1, 4 and 6 are the same but for 3, 3, 39 and 4 steps, from
6,495 differing kind-4 steps before.) With them, SetToNext's
kind 0 agrees on all 23,082 steps.

Kind 4 walks a GUID's objects by id, not the list's order. That was first seen while a lot
loads (a global init tree, `0x190`, walking a GUID in ascending id order: 28 steps) and in the
dance floor's "Choose dance orientation" (from tile 135 to 145, an earlier one in the list: 3
steps), and taken for the list still being built and for multi-tile; with the home log's global
trees 0x191-0x193 (37 steps) it is the rule everywhere: all of the club's kind-4 steps and all but
2 of the home log's agree.
