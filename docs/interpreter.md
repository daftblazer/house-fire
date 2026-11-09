# The interpreter

What the native interpreter has to do, as far as it is known; each point says how it was
found and how sure it is. The reference is the recompiled game, which runs the original
interpreter exactly (`plan.md`).

## Where the trees are

A node's opcode 256-4095 calls a global tree, 4096-8191 one of the object's own, 8192 and
up one of its semi-global file's (the file named by its GLOB chunk). In the scripts on the
four discs that have them, every call from a tree to one of the object's own trees finds it
in the object's file (`tools/bhav_text.py`, `primitives.md`).

Interaction tables are different (measured on Bustin' Out, 2026-11-09). Of the 4,373 own-
tree actions and guards in its IFF files' interaction tables, 1,749 are in the object's
file, and of the 2,624 that are not, 2,312 are in its semi-global file at the id plus
0x1000 (the fridges' `0x1001` is `appliances_fridge_sg`'s `0x2001`): the variants of an
object keep their interactions' trees in the file they share. Of the rest, 145 belong to
objects with no semi-global file (leftovers such as `abduction`, which no house may place)
and 167 are not explained. So an interaction's tree is looked up in the object's own trees,
then in its semi-global file at id + 0x1000. Not yet confirmed in the engine's code or by
the recompiled game; to be.

The quick data's interaction tables (`tools/interactions.py`) name the same trees; in the
v3.0 games 6 of 2,823 (The Sims 2) and 6 of 3,923 (Pets) are not in the object's script
file, The Urbz 192 of 2,669, not yet checked against the semi-global rule.

## Node step (to come)

`TreeSimImpl::DoNodeAction` in The Sims 2 (`80147FD8`) takes the frame, gets the node
(`Behavior::GetNodeRef`), handles calls (`TreeSimImpl::Gosub`) and returns (true 254,
false 253, error 255) and hands primitives to the object's `TryElement` through a virtual
call. Bustin' Out's equivalent and the frame layout are chunk 5 of `loop.md`.
