# The simulation tick

Which objects' tree stacks run in a tick, and in what order. Read from Bustin' Out's tick
(`Simulator_SimulateOneTick`, 80237C88, and its pass over the objects, `fn_80238150`, named
`Simulator_RunObjects` in the recomp) and measured on the recompiled game's interpreter log
(`tools/tick_order.py`; `engine/tools/replay`). The engine's is `engine/src/simulator.cpp`.

## The pass

Each tick the simulator makes a pass over the objects by id, with a cursor (its +1424) that
is kept from tick to tick: from where the last pass stopped (or id 1) up to the highest id in
use, taken once when the pass starts. Each id is looked up when the cursor comes to it. For
each object there:
- if its sleep count (the object manager's word for the id, its low half) is above 0, it is
  counted down and the object is passed over;
- otherwise its tree stack is stepped until it yields;
- if the turn left a dialog waiting for the player (below), the pass stops there.

A pass that goes through sets the cursor back to 0, so the next one starts from id 1. One that
stops leaves it on the object that stopped it, and the next tick's pass goes on from that
object: the objects before it are not reached, and their sleep does not count down.

So stacks run in id order, not in the world's list order (which is the reverse of it for a
lot's objects, and has newer objects in front). An object made during the pass has an id above
the pass's highest and waits for the next tick, except that it starts running its first tree
inside the turn of the object that made it.

Measured (a log with the pass's cursor, the `Q`/`q` records: 719 ticks of the club):
- every tick's turns are the objects the native pass reaches (`replay`, all 719);
- 101 passes stop early, every one at Sim 178's dialog ("Dialog", opcode 36, waiting), and
  each next pass goes on from 178; the native pass stops in exactly those ticks;
- in a longer log (5,129 ticks), 5,128 ticks have their turns in id order; the other is an
  object made in another's turn (above).

The first pass after a lot loads starts from a cursor the original never set (0xFFFF9754 in
the log); as that is below 1, the pass covers the objects as one from id 1 would. The engine
starts from 1.

## Dialogs

A dialog waiting for the player's answer (Dialog, opcode 36) is the original's step result 2:
the node stays, and the pass stops after this turn. In the logs it is the only primitive with
that result (2,471 steps). The engine's Dialog will yield and call `Simulator::stop_pass`.
While a dialog waits, then, only its object runs, once a tick, and everything else, sleep
included, waits with it: what the game shows as the simulation pausing for the dialog.

## Sleep

Idle (`docs/primitives.md`) puts a stack to sleep for a number of passes, not of ticks: it is
passed over by the next N − 1 passes that reach it and stepped on the N-th. Measured: all
2,810 regular sleeps in the log with the cursor, including the 49 that came 101 ticks late
because the passes in between stopped at a dialog. (23 others ended early: woken by something
else, not modelled yet.)

## Runs the engine starts

Steps outside the pass (between the `Q` and `q` records there are none of these) are the
engine's own: a new frame above a Sim's stack, a tree started at node 0 with no call, each
run to its return. In the logs they are the pie menu's checks, "get social eligibility"
(PersonGlobals 8230) and the interactions' "... TEST" trees, ending with "hide menu" (Global
265), and "Dream Sequence - Prefix Check Tree" (Global 463). They belong to the pie menu's
chunk, not the tick's.

## The clock

`Simulator::now` counts the passes run. Nothing in the tick is counted in ticks: Idle counts
passes that reach the object.

## Checked by replay

`engine/tools/replay` runs the club natively over a log's ticks: the passes by the simulator,
each stack stepped natively, the expression, Idle and SetToNext native, the other primitives
answered from the log; where the native state differs from the log's (a frame the engine
pushed, a variable a primitive that is not native wrote) it is counted and set to the log's.
On the 719-tick log: 67,001 steps with the native stack as the log's, 1,329 not (mostly frames
the engine pushed or popped: interactions, not native yet); 30,845 expressions the same
natively, 555 not; Idle the same every time (5,809).

## Where a Sim's interactions come in

A Sim waits for its next interaction in "Wait For Notify" (Global 281), at node 4,
IdleForInput (opcode 17). While it waits, in its turn, the engine runs check trees on its
stack (a frame above, "Dream Sequence - Prefix Check Tree" and the like, then "hide menu")
and then pushes the interaction it chose (a tree of the Sim's or of an object, at node 0, a
frame above the wait). Of the replay's remaining step differences on the 719-tick log, these
are nearly all (the rest are the pie menu's checks and the first ticks). They are the
interaction queue's and autonomy's, which come as a system of their own: the next large one.

Replay figures after its own fixes (2026-11-09: the frames saved below a stack's first step,
returns into them, the file a rebuilt frame names its trees from): on the 719-tick log 68,273
steps with the native stack as the log's, 314 not; on the 5,130-tick one (no pass records,
the pie menu's runs counted as turns) 338,376 and 7,033.

## Still open

- What wakes a sleeping stack early (23 of 2,833 sleeps).
- A stack that stops (an error): what the original does with the object.
- The frames the engine pushes on a Sim's stack: its interaction queue.
