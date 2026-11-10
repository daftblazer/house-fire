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
that result (2,471 steps). The engine's Dialog (`dialog()`) opens the dialog (a presentation
event, `World::dialog_object`), yields and asks the pass to stop (`World::stop_pass`) until
the front end gives the player's answer (`World::dialog_answer`), then goes to its true
target. In the session every dialog answered goes true (four dialogs of the party
controller's, one of them never answered); the replay plays the player with the original's
answers: all 2,471 waits and 4 answers as the log's, every pass stopped where its was.
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

What the log shows of the queue so far (the fifth loop):
- **A Sim's saved queue** (`lots.md`, the Sim's part): each entry has the Sim, the
  interaction's target and a third object, and the tree the interaction runs (in the
  target's file); in the club five Sims have one entry each (the dance floor, the booth).
- **Its delivery:** when a waiting Sim's IdleForInput step comes with an entry queued, the
  engine runs, on a stack of their own, the checks the target's interaction table (TTAB)
  names: the tree the table runs first (version 11's field for the whole table: in the club
  Global 463, "Dream Sequence - Prefix Check Tree", which calls 462), then the interaction's
  guard (for the booth's 4106 it is Global 265, "hide menu"; for 4128 on object 154, 4129),
  with the target as stack object. Then the IdleForInput step ends on its node, and the
  entry's tree is pushed above "Wait For Notify" at node 0, its stack object the target and
  its parameters 0; it runs in the same turn, and its return comes back to the wait.
- **Native** (`idle_for_input`, `Host::run_check`; the library reads each file's TTABs for
  the guards and the table's first tree). Checked (`replay`, both logs): all 5 queued Sims'
  deliveries made natively, their checks stepped natively through all 81 of the log's steps
  of them (the same at each), the interactions pushed as the log's. Not modelled yet: a check
  that fails, and a Sim with nothing queued (autonomy's: 3 and 2 Sims).
- **PushAction (13)** queues an interaction for the Sim that is the stack object: the one in
  place operand byte 0 of the target's interaction table (its file's TTAB, in order), the
  target the object whose id local byte 1 holds. In the club the party controller (object 2)
  pushes the dance floor's first interaction (4104) onto Sims 171, 172 and 174, which is what
  they were saved with, and its own fourth onto 178, again every few ticks. Native
  (`push_action`): all 21 (and 5) uses true as the log's; one more delivery is then made
  natively (6 in all, 101 check steps the same). Bytes 2 and 3 (1 or 2; 0x0a: a priority and
  flags?) and what the original does with an interaction pushed again while one is queued
  are not known yet: no logged delivery shows them.

Replay figures after its own fixes (2026-11-09: the frames saved below a stack's first step,
returns into them, the file a rebuilt frame names its trees from): on the 719-tick log 68,273
steps with the native stack as the log's, 314 not; on the 5,130-tick one (no pass records,
the pie menu's runs counted as turns) 338,376 and 7,033.

## The motives

Measured on the home log with the observer's `N` records (each person's 16 motives at the
start of a tick, when they changed): at Mom's house the scripts change a motive only 18
times (expressions) and SetMotiveDelta runs once, against 270 changes the scripts read. The
rest is the engine's own update, which comes every 60 ticks (122, 182, 242, …) and moves
each motive by its own amount:

| Motive | Sim 12 | Sim 237 | |
|---|---|---|---|
| 5 | −0.375 | −0.375 | the same for both |
| 8 | −0.17 | −0.17 | |
| 9 | −0.3 | −0.3 | |
| 15 | −0.25 | −0.25 | |
| 6 | −0.48 | −0.44 | a rate of each Sim's own |
| 3 | −0.2295 | −0.209 | |
| 14 | −0.1553 | −0.1428 | |
| 7 | −0.2027 at −3.51 | −0.4032 at 92.01 | 0.0021 × (value + 100), both |

Motive 13 moves only when something sets it.

**The update's code** (found by watching Sim 12's motive 5 in a replay of a `GCN_WATCH`
build: 45 of its changes stored by one function). `fn_8022501C(person)`, when the person's
+66 and a flag of its +0's +106 are 0 and a virtual check says so, calls `fn_801D0504` on the
person's motive block (person + 168; motive m at 4m, a copy of all 16 at +64, the person at
+128): Bustin' Out's `Motives::Sim`. In it, with the threshold 0.0 and motive 11 (its state:
at or above 0, or below):
- motive 5: plus one rate (motive 11 ≥ 0), or plus another, less a third while a value
  from a global object's virtual call lies in a range (the hour?);
- motive 9: less a rate × how much motive 7 rose since the copy (if it rose);
- motive 7: less (motive 7 + 100) × a rate (measured: 0.0021);
- motive 6: less (a rate × person data 3 + another) (each Sim's own: personality);
- motive 14: less (a rate × person data 6 + another), motive 11 ≥ 0 only;
- motives 15, 8 and 9: less a rate each (motive 11 ≥ 0), or motive 15 times a rate and 8
  and 9 less others (below 0);
- then all 16 clamped to −100 … 100 and copied to +64, and a virtual call on the person.
Motive 3 is not in it: another update moves it. The 0.0, 100 and −100 are in the DOL; the
rates (from r13 − 24772 on) are not: they are set at run time, from tuning data
(Castaway's `MotiveConstantsClient::UpdateConstants`), and are to be found at their source.

**The rates' source.** A watch on the first rate's word names its writer, `fn_801CFFB4`, run
once at the start: for each rate it asks a tuning object for a value by name, with a default
from the DOL (the defaults are not the game's values: "hyg decrement, awake" defaults to
0.15, the game has 0.17). The values are the global file's (`Global.iff`) tuning tables, its
four FCNS chunks: 0, 1, "SNCF", a count, then each entry a name, a little-endian float and a
description (strings zero-ended, padded to even with 0xCD). Table 1 is the motives':

| Name | Value | |
|---|---|---|
| energy span | 180 | energy 5: 180 / 16 waking hours / 30 = 0.375 a step, as measured |
| wake hours, wake hour | 16, 7 | |
| energy drift | 0.01 | |
| hunger to bladder multiplier | 0.3 | |
| hunger decrement ratio | 0.0021 | hunger 7: (value + 100) × 0.0021, as measured |
| social decrement base, multiplier | 0.055, 0.0001254 | social 14: 0.055 + 0.0001254 × person data 6 |
| ent decrement, awake / multiplier, asleep | 0.25 / 1 | fun 15: 0.25, as measured |
| hyg decrement, awake / asleep | 0.17 / 0.08 | hygiene 8: 0.17, as measured |
| bladder decrement, awake / asleep | 0.3 / 0.15 | bladder 9: 0.3, as measured |
| comfort decrement (active, lazy, base) | 0.4, 0.6, 0.5 | comfort 6, by person data 3 |

Table 2 is autonomy's (the minimum scores, attenuations, "random selection count" 4,
"friendship threshold" 50), 3 rooms' and light's, 4 the family's score. All four read to
their ends (`Library::tuning`, `test_disc`). The rest of `fn_801CFFB4` derives the rates the
update uses from these, as follows (W the wake hours, 16; H the wake hour, 7; S the span,
180): energy −S / W / 30 a step awake (−0.375) and +S / (24 − W) / 30 asleep (+0.75), less
the drift while asleep after hour H and before H + W; comfort less lazy + (active − lazy) /
1000 × person data 3 (0.6 − 0.0002 × p3); asleep is motive 11 below 0.

**Native** (`update_motives`, `MotiveRates::from`; its two personality terms a multiply-add
in one rounding, as the original's `fmadds`). Checked by `replay --card` on each person's
`N` records: 75 update steps of the home log the same bit for bit (every motive but 3 and
13, which it does not touch), 14 more where a script changed a motive in the same interval,
none that differs. Sims 12 and 237 have person data 3 = 600 and 800, 6 = 800 and 700.

**When it runs** (the observer's `U` records, a hook on `fn_8022501C`, named
`Person_SimMotives` in the recomp): in a person's own turn, every 60th turn, a turn being a
pass that reaches it, asleep or not (counted from the lot's load: 60, 120, …; both Sims' 45
updates, and none in the 2,924 ticks the passes stopped at a dialog at object 196 before
reaching them). At the end of the turn: that turn's scripts read the motives as they were.
The gate's +66 is 1 for the paper girl (an NPC with a file of its own), who is never updated;
0 for the family. Native (`Simulator::begin_turn`, `end_turn`). Checked (`replay --card`):
the persons' motives after each tick against the `N` records, 99 the same; the reads of
another's motives (scope 15) that differ fell from 338 to 23. 

**Running changes** (SetMotiveDelta, opcode 29; the save's +1120 list). Its operand, by
Castaway's `SetMotiveDeltaParam`: byte 0 the change's scope and field 4 its datum, byte 1 the
stop value's scope and field 6 its datum, byte 2 the motive, byte 3 flags (1 clear all; 2
and 4 an influence, not modelled). The change is an hour's: a tick's is it / 1800. In the
home log Sim 237 paints at the easel: `art_easel`'s constants 2 and 3, fun +48 an hour up to
50, +0.026667 a tick. Each change applies at the end of each of the person's turns, before
the update; the first in the turn that set it. Native (`set_motive_delta`,
`apply_motive_deltas`, `Simulator::end_turn`). Checked (`replay --card`): 703 of the 704
after-tick motive states as the log's (the one left a change to the paper girl's hunger),
the SetMotiveDelta step the same; the differing scope-15 reads 23 of 41,427.

**At the club** the motives do not move by themselves (its logs: 1 of 47 values changes,
though five dancers have saved running changes): neither the update nor the changes run.
What decides it is the gate's third test (a virtual call through the object's +4, not
identified; its +106 test is data field 34, which is 0 for every Sim at Mom's house and 1 for
some at the club). Until it is found, the lot's loader says (`World::motives_run`; the replay
sets it off for the disc's club, which keeps the club logs' figures as they were).

Not yet: motive 3's own update, the clock's hour (read only while asleep), the gate's +66
(1 for the paper girl) and its third test, an influence's change. Castaway names the update's parts: `Motives`
(`Sim`, `InitDecayRates`), a base decay table (`sMotiveDecayBase`) and changes to it by
personality (`sMotiveDecayActiveMod`, `NeatMod`, `OutgoingMod`, `PlayfulMod`), and
`MotiveConstantsClient::UpdateConstants`, which suggests the rates are game data: the
engine's update is to be built from those, checked against every `N` record.

## Autonomy

A Sim with nothing to do chooses for itself through **Find Best Action** (opcode 30,
Castaway's `cXPerson::TryFindBestAction`), which its own scripts run: in the home log all
three uses are PersonGlobals 8233 at node 1, with an empty operand. Each pushes the chosen
interaction on the Sim's stack (the next step is its tree at node 0, a new frame, its stack
object the target) and the node is stepped again when it returns:

| Tick | Sim | Chosen | Target |
|---|---|---|---|
| 260 | 12 | 8215 (a PersonGlobals interaction) | object 62 |
| 1503 | 12 | 8223 | object 114 |
| 1901 | 237 | 4097, the easel's | object 152 (the easel) |

The interactions the household starts otherwise in the log (four, at ticks 931 to 1791)
come through "Wait For Notify" while the recorded input still plays: the player's. The
scoring is to be built from the interactions' advertisements (their TTAB entries), the
motives (native now), the distance (the `P` records) and the autonomy tuning table (FCNS 2:
attenuations, the minimum scores, "random selection count" 4). As the choice is random among
the best four with the original's own random numbers (which the engine does not copy,
`plan.md`), the check is that the log's choice is among the native best four, and the
scores' order where the log shows it.

**Where it runs.** PersonGlobals 8233 is "try autonomy": node 0 opcode 3, node 1 Find Best
Action, then "reset idle" (8292) or 8282. Find Best Action takes a choice already made:
its stack object is the chosen target before it runs. The choice comes from an
**evaluation** about 60-75 ticks earlier, one for each (Sim 12 at ticks 186 and 1443, Sim
237 at 1847): in one turn, the Sim's stack runs about 145 check trees (each at node 0 in a
frame of their own, with nested calls above it) over about 41 objects, by id ascending
(7, 10, 13, 25, 26, 29, 33, 34, …), each object's interactions' guards in table order, the
stack object the candidate. The same objects and guards each time for Sim 12 (186 and 1443
run the same 145). Next to build natively: that sequence (the candidates and their guards,
`Host::run_check`), then the score of those that pass.

**The evaluation's candidates, measured** (Sim 12 at tick 186, its 145 top-level checks, the
starts at node 0 in the bottom check frame). By object, id ascending; for an object, its
table's interactions that advertise, in **reverse** table order (the fridge's table is 8216,
8218, 8213, 8233 and four that advertise nothing; the checks run 8233, 8213, 8218, 8216);
a version-11 table's first tree (463) before its guards. That predicts 23 of the 41 objects
exactly. The others:
- a multi-tile object's parts (beds 29 and 34, sofas 7, 10, 13, 147) have field 5 = 0xFFFF:
  their table is the multi-tile object's (its master definition's, by the master id in
  field 8), and the single-tile objects with 0xFFFF there too (the mailbox 33, the trash
  compactor 139) use their file's one table;
- which interactions take part is more than "advertises": some that advertise nothing are
  checked (40: 8219, 8235, 8237, 8243) and two that do are not (25: 8208, 8239), so the
  autonomy flags, the minimums or the threshold decide it.

**The advertisements** (the interaction tables, TTAB; `Interaction`, `Library` keeps every
file's by chunk id). A file can have several tables (a fridge's for cheap, moderate and
expensive models); an object uses the one its definition's field 5 names. Each interaction
advertises, motive by motive, a minimum, a delta and the personality field scaling it; with
an attenuation (code and value), an autonomy threshold and flags. The three choices:

| Object | Table | Interaction | Advertises |
|---|---|---|---|
| 62, a fridge (`appliance_fridge_c_white`) | 128 "Fridge - Cheap" | 8215 (a snack) | hunger +20, attenuation 2 / 0.02, threshold 50 |
| 114, a toilet (`plumbing_toilet_x`) | 130 "Toilet - Expensive" | 8223 | bladder +70 |
| 152, the easel (`art_easel`) | 129 | 4097 | fun +10 (minimum 1), attenuation 4 / 0.2 |

Each is what its Sim lacked then: Sim 12 hungry (hunger −3.5 at the load), then its bladder
low (−5 to −33); Sim 237's fun the lowest of its motives (about 21). Checked: `test_disc`
reads all 856 tables on the disc and finds the three.

## Still open

- What wakes a sleeping stack early (23 of 2,833 sleeps).
- A stack that stops (an error): what the original does with the object.
- The frames the engine pushes on a Sim's stack: its interaction queue.
