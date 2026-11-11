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

## Frames (Bustin' Out's `StackElem`; The Sims 2's header is 4 bytes longer)

A frame on a tree stack (`TreeStack`, frames end to end, the bottom one first):

| Offset | |
|---|---|
| +0 | tree id, 16 bits (the top bit is a flag, cleared when the id is read; -1 none) |
| +2 | the current node, 16 bits |
| +4 | 16 bits, the caller's +4 on a call (to be named) |
| +6 | locals count (8 bits) |
| +7 | parameter count (8 bits) |
| +8 | 32 bits, 0 on a call; frames the engine pushes itself have it set, and a return into such a frame takes no transition (the node step returns 0 after the pop) |
| +12 | the object's resource file, 32 bits: the file a call's tree id is found in |
| +16 | parameters (16 bits each), then locals |

Its size is 16 + ((parameters + locals + 3) * 2 rounded down to a multiple of 8)
(`fn_8023FC9C`; the values' address is `fn_8023FC80`). In The Sims 2 the header is 20 bytes
and the values start at +20. Measured on the recompiled game: callee frames sit exactly
that far above their callers.

## The call (Bustin' Out's `fn_8023EE1C`, The Sims 2's `TreeSimImpl::Gosub`)

Arguments: the interpreter, a file (0: the caller's, +12), the parameters (0: none given),
the tree id. The tree is found in the file (none: the call fails). A header is made: the
tree id, node 0, +4 from the caller, +8 zero, +12 the file, the counts from the tree. Then:

- **No parameters given** (a call node whose operand is all -1): the parameters are copied
  from the array the interpreter was set up with (`TreeSimImpl::Initialize(int, short *)`
  keeps its pointer at +44). Not the caller's. In the PC game these are the object's
  temporaries. Measured: all 48,248 calls of a session agree once this is modelled.
- **A tree of type 4** starts at the node its first parameter names, the last node if that
  is past the end; a negative one fails the call.
- **Locals are not cleared.** A new frame's locals are whatever the stack's memory held
  there: the values of frames that were there before, or their headers. Measured: of 11,644
  calls whose locals were not zero, 11,337 hold exactly the values earlier frames left
  there, and most of the rest look like header words (a tree id, 254). The native
  interpreter starts them at zero, by design (`plan.md`, "Same game, new engine"), and
  reports a read of one not yet written (`Host::unset_local`). `tools/locals_check.py`
  finds the scripts that can do that: on Bustin' Out's disc 60 reads in about 25 of the
  3,506 trees with locals (for example "Start Jealousy" tests local 1 at its first node),
  and 324 more past primitives it does not decode yet. In the logged session the original
  ran 442 such reads and every one read zero, so zero is also what the game does in
  practice.

## Semi-global files

A file's GLOB chunk names its semi-global file, as a length-prefixed string (the
characters' files: `\rPersonGlobals`) or a zero-ended one (the objects': `person_sg`). The
characters' files name PersonGlobals, but the game runs trees for them that only
`person_sg.iff` has (`0x2032`, `0x204F`, ...): their semi-global file is `person_sg`, as
`template_person.iff` names it. `PersonGlobals.iff` is on the disc but not used for them.

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

## The node step (The Sims 2's `TreeSimImpl::DoNodeAction`, `80147FD8`)

Read from its code; Bustin' Out's (`8023F1FC`) compares with the same values and uses the
same error codes and results, so it is taken to be the same step. One call runs one node of
the current frame and says what next: 0 go on, 1 the tree yields for this tick, 2 stop.

- The current node (frame +2) above 250 is a return: 254 true, 255 false, anything else
  error 1100. Returning from the bottom frame is error 1001. Otherwise the frame is popped,
  the result kept (+44 in The Sims 2, +40 in Bustin' Out), and the caller's node moves to that node's true or false target;
  a target of 253 is error 1100.
- Else the node is read (`Behavior::GetNodeRef`; none: error 1102) and an iteration
  counted (+4 of the interpreter; past the limit, error 1101).
- Opcode (15 bits) above 255: a call. The node's 8 operand bytes are the parameters, or
  none if both their words are -1, and then the interpreter's own (`TreeSimImpl::Gosub`; "The call" above). A failed call is error 1000 if
  the tree exists, 1102 if not.
- Else a primitive: the object's `TryElement` (frame, opcode, operand) gives a result:
  1 true, 0 false (the node's true or false target becomes the current node; 253 there is
  error 1100), 2 the tree yields this tick, 3 go on without a transition (the primitive
  changed the stack itself, as a call does), anything else stop.
- An error stores its code (+52 of the interpreter), sets a global flag and calls the
  interpreter's error handler; the step returns 2.

Node targets 253, 254 and 255 are therefore error, return true and return false, as in the
PC game.

## The expression primitive (opcode 2; Bustin' Out's `fn_80213890`)

The operand: left datum (16 bits, bytes 0-1), right datum (bytes 2-3), a byte the primitive
never reads (4), the operator (5), the left scope (6), the right scope (7); the 16-bit
fields in the file's order. Each side is looked up by scope and datum (`fn_80214110`, The
Sims 2's `cXObjectImpl::InterpValue`), the right first: its 16-bit value, and for the left
a pointer to the variable, 16-bit or float (motives are floats).

| Operator | | Operator | |
|---|---|---|---|
| 0 | `>` | 9 | set flag |
| 1 | `<` | 10 | clear flag |
| 2 | `==` | 11 | `++`, then `<` |
| 3 | `+=` | 12 | `%=` |
| 4 | `-=` | 13 | `&=` |
| 5 | `:=` | 14 | `>=` |
| 6 | `*=` | 15 | `<=` |
| 7 | `/=` | 16 | `!=` |
| 8 | flag set? | 17 | `--`, then `>` |

- Comparisons take the 16-bit values, float variables included.
- Arithmetic on a float variable is done in single precision, with the right side
  converted. A float result in a motive scope (14, 15, 17) is kept between -100 and 100
  (the constants at `802CB630`).
- `%=` works on absolute values. `%=`, `&=` and the flag writes need a 16-bit variable.
- Flag n is bit n - 1. A flag above 16 is error 13. Flag 0 shifts out of range and tests
  the sign.
- Errors (stored at +48 of the interpreter, the primitive then stops): 1 an unknown
  operator, 13 a flag above 16, 19 a left side that cannot be written, 38 division by zero.
- `++`/`--` on a float variable: the original compares a 16-bit word read through a null
  pointer. The native one compares the new value instead (`plan.md`, deliberate
  differences).

The recompiled game's log records each expression: what the lookups found and the left
side after. `tools/lockstep` runs the native expression on those values. In a session of
the new-game route, all 169,010 expressions agree: results, 16-bit values written and float
values written, bit for bit, over every operator but none of the errors.
`engine/tests/expression.rs` covers the rules the session does not reach.

What the lookups themselves are, scope by scope, belongs to the object model
(`engine/src/library.rs` so far).

## Constants (scope 26)

A constant's datum is a table and an entry: its top 9 bits the table, its low 7 bits the
entry. Tables 0-63 are the object's own BCON chunks from id 4096, 64-127 its semi-global
file's from 8192, and 128 on the global file's from 256 (the PC game's rule). A BCON chunk
is a count, a flags byte, then the 16-bit values. Checked against the recompiled game: all
19,245 constant reads of a session agree (`Library::constant`).

Variants of an object (the sinks, the beds) share their trees but not always their
constants, so which variant an object is shows only in the constants it reads.

## Checked against the original (`tools/lockstep`)

The recompiled Bustin' Out logs its interpreter when `BUSTIN_TREE_LOG=<file>` is set
(`src/tree_log.cpp` there; it only reads memory). `engine/src/bin/lockstep.rs` reads such a
log with the disc: each step the original took is given to the native interpreter, with the
stack as the log has it and the primitive answering what the original's answered, and the
native stack after it is compared with the original's next step.

A session of the new-game route (blank card, the dream and the club, 5,130 ticks, 366,558
steps, 286 trees, 85 objects; 2026-11-09):

| | same | different |
|---|---|---|
| primitive: the true or false target | 224,909 | 0 |
| primitive: a yield (the node stays) | 36,273 | 0 |
| primitive: a stop | 2,471 | 0 |
| return: the caller's next node | 48,066 | 0 |
| call: the callee's tree, found by the native lookup, same nodes | 48,248 | 0 |
| call: the callee's parameters | 48,248 | 0 |
| call: the callee's locals | 36,604 | 11,644 (not cleared, above) |

Not compared: 67 calls whose callee was replaced from outside the node step before it ran
(a new action), 6,336 returns from the bottom of a check stack, 73 returns into frames the
engine pushed (+8 set), 128 steps of objects run before any tree of theirs could identify
them. Every tree the game ran was found on the disc with the same nodes. Which object a
frame belongs to is found from the trees run under its file pointer, since memory is
reused: the file pointer can name different objects over a session.


A primitive that runs a tree (RunFunctionalTree 20, CallNamedTree 28, GosubFoundAction 30) and
stays on its node has pushed that tree and goes on into it: lockstep now answers it Continue
(the node unchanged) where it answered false before. On the home log that was 426 of its 446
differences (CallNamedTree's), a gap in the tool rather than the engine: lockstep 446 → 18. The
5 primitive transitions left are routing primitives (27, 45) staying on their node.
