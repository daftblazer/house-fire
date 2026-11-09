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

## Tree ids (The Sims 2's `Behavior::GetTreeClass`, `GetResFileID`)

0-255 a primitive; 256-4095 global (the global file); 4096-8191 the object's own; 8192-9000
the semi-global file; above 9000 none. A call's id picks the file this way, and the tree is
then found in it by id. No fallback from own to semi-global here: the + 0x1000 rule above is
found elsewhere, if in the engine at all.

## Frames (The Sims 2's `StackElem`, the same code in Bustin' Out)

A frame on a tree stack (`TreeStack`, frames end to end):

| Offset | |
|---|---|
| +0 | tree id, 16 bits (the top bit is a flag, cleared when the id is read; -1 none) |
| +2, +4 | 16 bits each, copied on a push (the node and the caller's details; to be named) |
| +6 | locals count (8 bits) |
| +7 | parameter count (8 bits) |
| +8, +16 | 32 bits each, copied on a push |
| +20 | parameters (16 bits each), then locals |

Its size is 20 + ((parameters + locals + 3) * 2 rounded down to a multiple of 8). A push
copies the frame's header and the parameters given (`StackElem::Setup`, `TreeStack::Push`).

## Bustin' Out's interpreter

Matched to The Sims 2's named functions by their code ("same": the same code with addresses
masked; a number: the share of instruction shapes in common), or placed by the source
file's order where a function changed.

| Bustin' Out | The Sims 2 | |
|---|---|---|
| `8023E594` | `TreeStack::TreeStack(TreeSimImpl &)` | same |
| `8023E874` | `TreeStack::MakeNewFrame(unsigned int)` | same |
| `8023EB30` | `TreeSimImpl::TreeSimImpl(int)` | 0.67 |
| `8023ED0C` | `TreeSimImpl::Initialize(int, short *)` | same |
| `8023EDB8` | `TreeSimImpl::Reset(Behavior *, short)` | 0.73 |
| `8023EF78` | `TreeSimImpl::RunCheckTree(Behavior *, short, short, short *)` | 0.63 |
| `8023F194` | `TreeSimImpl::RunOneTickTree` | by order |
| `8023F1B4` | `TreeSimImpl::HandleBreakpoint(StackElem *, BehaviorNode *)` | same |
| `8023F1FC` | `TreeSimImpl::DoNodeAction(StackElem *)` | by order (0x444 bytes; 0x564 in The Sims 2) |
| `8023F640` | `TreeSimImpl::GetLastTransition(void)` | by order |
| `8023F654` | `TreeSimImpl::Simulate(int)` | by order |
| `8023F6D8` | `TreeSimImpl::GetHighLevelAction(void)` | same |
| `8023FC9C` | `StackElem::GetSize(void) const` | same |
| `8023FCB8` | `StackElem::NextFrame(void) const` | same |
| `8023FCE8` | `StackElem::Setup(StackElem *, short *)` | same |
| `8023FD88` | `TreeStack::GetNewFrame(void)` | same |
| `8023FE3C` | `TreeStack::AssignFrames(int)` | same |
| `8023FFCC` | `TreeStack::Push(StackElem *, short *)` | same |
| `8024014C` | `TreeStack::Pop(void)` | 0.64 |
| `802401CC` | `TreeStack::GetNthFrame(int)` | same |
| `80240214` | `StackElem::GetTreeID(void) const` | same |
| `80237C88` | `cSimulatorImpl::SimulateOneTick(void)` | 0.76 |
| `80238150` | `cSimulatorImpl::TickAllObjects(void)` | 0.61 |
| `8021242C` | `cXObjectImpl::TryElement` (the object primitives) | by its switch |
| `802246B0` | `cXPersonImpl::TryElement` (the person primitives) | by its switch |

## Node step (to come)

`TreeSimImpl::DoNodeAction` in The Sims 2 (`80147FD8`) takes the frame, gets the node
(`Behavior::GetNodeRef`), handles calls (`TreeSimImpl::Gosub`) and returns (true 254,
false 253, error 255) and hands primitives to the object's `TryElement` through a virtual
call. Bustin' Out's is `8023F1FC`; what it does, node by node, is next (chunk 5 of `loop.md`).
