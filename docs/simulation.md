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
- **An entry the engine queues knows its place in the table** (`SavedQueueEntry::interaction`):
  an action can be in a table more than once with different guards (the group meal's 4201,
  guards 4202 and 8248), and the original checks the one PushAction chose. PushAction takes
  the target's own table (`Library::table_of`: its definition's, a person's semi-global
  file's), and the delivery that table's first tree (version 11, kept per table) and that
  entry's guard. Checked (`replay --card`): one more PushAction native at Mom's house, its
  delivery made natively with all 5 check steps as the log's; the club logs unchanged.

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

**The candidates, native** (`autonomy_candidates`). The rule that fits every check: the
lot's objects by id; not one whose definition is of type 7 (field 7: the phone's and the
stereo's controllers; Mom's house's scenario controller, type 4, is in though off the lot),
nor a part whose definition's field 6 is 1 to 0xFFFD (a bed's, a mailbox's, a compactor's
lesser parts; 0, 0xFFFE and 0xFFFF are in); its table (field 5, a part's master's, or the
file's only one); the interactions whose autonomy threshold is below 100 and whose flags lack
0x80, in reverse table order, a guard shared by several once. Checked (`replay --card`): the
three evaluations' 216 checks of objects (persons aside) the same natively, in order. 

**Persons as candidates.** A person's interactions are its semi-global file's one table:
`person_sg`'s (table 129, version 11, its first tree 8230 run before its guards) for a Sim,
whose file is `template_person`; `NPC_SG`'s for an NPC (the paper girl). One more flag keeps
an interaction out: 0x40 (person_sg's 8486, 0x245, which the log never checks; no object's
interaction with it is a candidate either). Checked (`replay --card`): all three
evaluations the same natively, objects and persons, 421 checks in order (the objects present
taken from the log's list at that tick: the paper girl leaves at 1382 and 1767, and the
replay does not remove objects natively yet).

At the club the replay finds 67 bursts of checks over many objects with about 4 top-level
checks each, against a native list of 599: either the club's evaluations are spread over many
ticks a few candidates at a time, or they are something else. Open; the club is not claimed.

**Which pass.** A check's result is how its bottom frame returns (254 true, 255 false).
Of the 145 checks, 40 pass at tick 186, 41 at 1443, 42 at 1847 (of 147): the sofas', the
beds', the fridge's snack (8216), the toilet's (8225), the easel's (4099), the scenario
controller's (265), the other Sim's social ones (8230, 265, 8297, 8272, …) among them; each
choice's guard is among those that pass. Natively the guards are to run in the evaluation's
turn (the check trees through `Host::run_check`, against the log's steps of them, as the
interaction queue's are), which the replay's flow does not do yet.

**The evaluation is opcode 3**, node 0 of "try autonomy" (8233): one step whose nested work is
all the checks, which ends with the stack object set to the choice (then Find Best Action at
node 1 pushes the chosen interaction on it). Its step record carries its last check's lookup
and is in a frame below its checks'. Native: `run_evaluation` (the candidates' checks
through `Host::run_check`, each object's table's first tree once before its guards). In the
replay (only with `REPLAY_EVALUATION`): its checks step through the log's steps of them and
each result is compared with the log's (its bottom frame's return): 153 the same, 45
different and 235 not finished over the three evaluations, because a check that calls
another file's tree through opcode 28 (430:3 → ScenarioGlobals 8251) goes on natively
without the call (the main replay syncs such frames; a check does not). The choice is taken
from the log until the score is native.

With CallNamedTree native (its calls made inside checks too), a call step's record taken from
after its callee's (the log writes a step when it ends), and the trees resolved as the
object's own file names them (a person's table is its semi-global file's, whose 8192+ ids
the person's file resolves: the paper girl's 8218 was found nowhere before): 410 of the
three evaluations' 433 checks give the log's result, 3 do not, 20 do not finish; one
evaluation uses exactly the log's steps; 4,531 check steps as the log's, 296 not. Still off
by default (with it the replay's matching steps total 160 fewer, the differing 228 fewer).

The last differences were an object made during the session (201, after the save) that the
native world lacks: the replay sets its checks aside (74 steps). Then all **433 checks give
the log's result, and all three evaluations use exactly the log's steps** (5,585 check steps,
none different). It is on by default for the save's lot (`replay --card`): the replay's
matching steps total 566,983 against 566,104 without it, the differing 2,334 against 2,858.
At the club it is off: there it makes the figures worse (the bursts noted above are not
evaluations as at Mom's house), so `REPLAY_EVALUATION=1` turns it on there only by hand.

**The score, first measured.** Bustin' Out's scoring function is `fn_80267E9C` (named
`Autonomy_Score` in the recomp; found by the shape of Castaway's
`MotiveEffects::GetInteractionScore`: a personality value, or 1000 less it, scaling an
advert). Over a set of per-motive curves (the evaluator's +0, 20 bytes each: points, slopes, a
count, the motive) it reads each motive's advert (personality, minimum, delta: the minimum
plus the delta × the personality value / 1000, or the minimum plus the delta without one),
adds it × 0.001 to the person's motive (the evaluator's +192 points at its 16 motives), reads
the curve there, and averages. The observer's `A` records log each call: 111 in the home log
(36, 37 and 38 in the three evaluations, about the checks that pass), the new log the same
581,110 steps. The scores differ only in the fourth decimal (26.9131 to 26.9142 at tick 186),
and alone they do not rank the choices first: at 186 the toilet (bladder +70) is first and
the chosen fridge snack (hunger +20) fifth; at 1443 the chosen toilet fifth; at 1847 the
chosen easel third. So the choice weighs more: the distance by each interaction's
attenuation (the places are logged) and what the evaluation does with these scores.

**The choice's value.** The score's caller (`fn_802266BC`) takes for each candidate whose
guard passed (the `A` calls are exactly those, in order: 36, 37, 38 against 40, 41, 42 passing
checks less the tables' first trees) its interaction's attenuation value (the entry's +48), the
factor 1 / (1 + attenuation × f30) (f30 the distance, it seems), and keeps (score − f28) ×
that factor if it reaches a minimum (f29), as a 16-byte entry in a list the choice is made
from. Tested on the logged scores with the distance in tiles from the `P` places, the
attenuation by code (1 none, 2 0.1, 3 0.3, 4 0.6 from the tuning table, 0 the entry's own) and
the evaluation's lowest score for f28: each choice is among the best two, so within the
best four the tuning's "random selection count" names (tick 186: the toilet first, the chosen
fridge snack second; 1443: the chosen toilet first; 1847: the chosen easel second).
Read exactly since: f30 is the Sim's distance to the candidate, a virtual call on the Sim
(its function `fn_801F1C18`): √((dx² + dy²) / 256 + (20 × the levels' difference)²) over
the two places (the FTilePt at +0xC8, sixteenths; the level at +0xE0), so tiles, as tested;
f27 is 1.0; f29 is one of two minimums (r13 − 28316 or − 28320, by the evaluator's +72: the
family's or a visitor's, the tuning's "min autonomy score" for each); f28 is
`fn_80267D48` on the same evaluator before the candidates (the score of no advert, it seems).
Still approximate: f28 (the lowest score here) and the code to attenuation mapping.

**The curves, logged.** The observer's `W` records dump the evaluator's curves where the base
score is taken (`Autonomy_BaseScore`, 80267D48, its result a `B` record): 9 curves for each of
the two evaluators (Sim 12's and Sim 237's), each a motive, points (x, y) and per-segment
slopes (1 / the segment's width). The score, recomputed from them: for each curve, the advert
for its motive (the minimum, plus the delta × the personality value / 1000 where it names
one; a personality p is the table at 0x802D17F8, 12 bytes an entry, its person data field
and whether it counts 1000 less it), × 0.001, plus the motive; the curve there (the segment
found from the end: y + (v − x) × slope × (y' − y); beyond the ends, the end's y); averaged.
Checked: 105 of the 111 logged scores bit for bit, the other 6 off by the last bit (26.283998
against 26.284000). The bases are logged too (26.91314 at tick 186: the lowest score, as
approximated before).

**The curves' source** is the global file's STR# 501, "InteractionScoreCurves": ten strings of
points, "(-100;-100) (-72;90) (100;90)", not always in order (the comfort curve lists "(10;10)
(-45;0)"), sorted by x when read; the first nine are motives 5, 6, 7, 8, 9, 3, 13, 14 and
15, in that order. The personality table is regular: entry p is person data field
(p + 1) / 2 + 1, the even entries counting 1000 less it.

**Native** (`score_curves`, `advert_amount`, `interaction_score`; the curve's step and the
advert's × 0.001 as multiply-adds in one rounding, which the last bit needed). Checked
(`replay --card`, the `A` and `B` records): all 111 scores bit for bit from the adverts and
motives they read, and all 3 base scores (the score of no advert).

**The value, native** (`candidate_value`: (score − base) / (1 + attenuation × distance),
`person_distance`, `attenuation` from the tuning table by code). In the replay's evaluation,
each passing candidate's score from its own table adverts and the native motives, its value
with the places set to the log's each tick (`P`: routing is not native). Checked: 102 of the
candidates' scores the same as the logged ones bit for bit (7 not: where the session's new
object 201 is set aside, the pairing with the log's scores shifts); the three choices at
native ranks 2, 1 and 2 (tick 186: the toilet 0.000357, the chosen fridge snack 0.000125;
1443: the chosen toilet 0.000548; 1847: 40's 4098 0.0000388, the chosen easel 0.0000328),
each among the best four the tuning's "random selection count" names. The choice among them
is the original's random one (its numbers are not the engine's, `plan.md`), so the native
choice is to pick among the best four with the engine's own numbers.

**The choice, native** (`autonomy_list`, `autonomy_pick`). After the values, the original
(the end of `fn_802266BC`) sorts its list best first (`Util_Sort`, 80110C38, the comparator
8022667C: a difference within 1e-7 is a tie) and, for a family member's evaluation whose best
entry lacks a flag of its interaction's, takes entry `random % min(count, n)`, `count` the
tuning's "random selection count" (4), `random` the game's generator (`Sim_Random`, 8023C254:
an LCG); otherwise the best. A sitting Sim's choice below "min autonomy score for sitting" is
dropped. Its entry's object is the stack object, its interaction's string index kept. The
distance in the values uses the game's own square root (`fn_801F1BB8`: 1 + s / 20, then two of
Heron's steps; 4% over at s = 25), which `game_sqrt` copies: with it every object's value is
the original's bit for bit. The observer logs the sorted list (`Y`) and the random number the
choice draws (`R`). Checked (a new home log, the same 581,110 steps): one of the three lists the
same entire (tick 1847), the others' objects' values bit for bit, all three choices made
natively with the original's random numbers the log's (the stack object set by the engine).
Open: Sim 237's own interactions value lower natively (7.24e-5 against 8.33e-5 at tick 186;
the distance to a moving Sim, or a person's adverts), one of them (its interaction 14) missing
from the native list, and one tie (object 124's two entries) in the other order. The visitor,
flag and sitting branches are not built: the log has none.

Earlier: the pick and its result (the stack object, Find Best Action's push) native of those that pass (the
adverts, the motives, the distance, the tuning table), checked as each choice among the native
best four.

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

## Routing

**The walls** are the lot's second array, its first byte a tile (`docs/lots.md`, "The
arrays"): a bit an edge, 1 towards y − 1, 2 towards x − 1, 4 towards y + 1, 8 towards x + 1,
each wall seen from both of its tiles (the house's west wall: 8 on x = 12, 2 on x = 13); 0x10
and 0x20 the diagonal walls at the house's corners. The array is row after row of y. A door
(OBJD type 8, two parts) stands on the two tiles either side of its wall and opens it
(`Walls`, built as the lot loads). The floors (the first array) do not block: 0 is the grass
the Sims cross. Checked (`replay --card`, the Sims' places, `P`): 79 steps from a tile to a
neighbour, every one open natively, 2 of them through a door (Sim 12 through the front door,
object 67 at (18, 14) and its part at (18, 15); Sim 237 through the lattice door, 65 and
66); 162 wall edges, 19 doors' openings. Two moves of 197 (the paper girl) are not steps: she
is placed (37, 5 to 25, 20). The ground floor only: the lot has one walls array.

The doors are type 8 with field 3 zero: windows are type 8 too (saved as doors), with field 3
set (all 30 doors and all 30 windows on the disc), and do not open their walls. Mom's lot:
5 doors' openings.

**The route** (`Walls::route`). A Sim's walk goes a tile at a time to a neighbouring tile (x
or y by 1; its smoothed line crosses diagonals as a staircase), and its tiles are a shortest
such route: a breadth-first search through the open edges, the tiles of diagonal walls (the
bay window's corners, 0x10, 0x20) closed. Checked (`replay --card`; a walk is a Sim's tile
changes until a pause of 40 ticks): 13 of the 15 walks as long as the native route, 9 of them
on its very tiles (the others an equally short route, the original's choice among them still
open). The other two (Sim 12 from (22, 15) to (25, 16), 6 steps against 4; Sim 237 from
(23, 21) to (21, 24), 9 against 5) go round the dining table and a counter: some objects block
(the tables, the counters) and others do not (the chairs, the newspaper, the wall objects:
the Sims cross their tiles). No OBJD field tells them apart (only the GUIDs, among the fields
of these); the objects' footprints are next. The walk's line through its tiles (the `P`
places, about 0.7 sixteenths a tick) is to come.

## Still open

- What wakes a sleeping stack early (23 of 2,833 sleeps).
- A stack that stops (an error): what the original does with the object.
- The frames the engine pushes on a Sim's stack: its interaction queue.
