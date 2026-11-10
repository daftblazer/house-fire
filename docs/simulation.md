# The simulation tick

Which objects' tree stacks run in a tick, and in what order. Read from Bustin' Out's tick
(`Simulator_SimulateOneTick`, 80237C88, and its pass over the objects, `fn_80238150`) and
measured on the recompiled game's interpreter log (`tools/tick_order.py`, a log of 5,129
ticks from a new game into the club). The engine's is `engine/src/simulator.cpp`.

## The pass

Each tick the simulator walks object ids from 1 up to the highest id in use, taken once when
the pass starts, looking each one up. For each object there:
- if it is asleep (Idle: its sleep count, which the pass counts down), it is skipped;
- otherwise its tree stack is stepped until it yields (or stops).

So stacks run in id order, not in the world's list order (which is the reverse of it for a
lot's objects, and has newer objects in front). An object made during the pass has an id
above the pass's highest and waits for the next tick, except that it starts running its
first tree inside the turn of the object that made it.

Measured: of the 5,129 ticks, 5,128 have their objects' turns in id order. The one other is
tick 84, where object 2 makes 179, 179's first steps run inside 2's turn, then 2 goes on.

The pass's position is kept between calls (the simulator's +1424), and the code can end a
pass partway and go on from there on the next call. No logged tick does: every tick's pass
starts from the lowest id. The engine does not model it.

## Runs the engine starts

Apart from the pass, the engine runs trees on a Sim's stack itself: a new frame above the
stack's top, started at node 0 with no call, each tree run to its return before the next.
In the log these are the pie menu's checks, "get social eligibility" (PersonGlobals 8230)
and the interactions' "... TEST" trees, ending with "hide menu" (Global 265), and "Dream
Sequence - Prefix Check Tree" (Global 463): 200 such runs, after the pass's turns. They are
for the pie menu's chunk, not the tick's.

`tools/tick_order.py` tells them apart by their shape: the stack had yielded (its last step
in its own turns stayed on its node), and the run starts a tree at node 0 on a frame above.

## The clock

The engine's clock is the number of ticks run. Idle's sleep is counted in it
(`docs/primitives.md`, "Idle"), and it does not move while the game is paused, as no tick
runs then.

## Still open

- What decides that a tick runs at all (pauses, the game's speed settings): the tick's
  caller.
- A stack that stops (an error): what the original does with the object.
- A native run of the club for ticks on end, checked against the log step by step, with the
  primitives that are not native yet answered from it.
