# The simulation tick

Which objects' tree stacks run in a tick, and in what order. Read from Bustin' Out's tick
(`Simulator_SimulateOneTick`, 80237C88, and its pass over the objects, `fn_80238150`, named
`Simulator_RunObjects` in the recomp) and measured on the recompiled game's interpreter log
(`tools/tick_order.py`; `engine/src/bin/replay.rs`). The engine's is `engine/src/simulator.rs`.

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
their ends (`Library::tuning`, the `disc` test). The rest of `fn_801CFFB4` derives the rates the
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

**Sim 237's own interactions, narrowed.** Their values differ by their score, not the distance:
each implies a different distance from one place, and two (13 and 24) score the same natively and
not in the log. Of the home log's 7 scores that differ (`REPLAY_SHOW_SCORES`), all are Sim 12's of
Sim 237's interactions, and natively only motive 14's advertisement differs from the log's (`A`);
the logged ones change between ticks (interaction 24: 0/0/54 at tick 186, 0/0/0 at 1443), so the
original makes that advertisement as it scores (a person's, by their relationship it may be). But
the logged motive-14 entries are not what it scores with either: only one of the 7 scores comes
out the log's from them natively. The score's real input for motive 14 is to be logged from the
score function itself. (The chooser's and a person candidate's places are now taken as logged
at the tick for the distance, as the walk is not native; no value changed with it.)

**Motive 14's advertisement, as the original scored it.** The logged entry for motive 14 is the
original's, in another order: read as (personality, min, delta) it gives all 7 differing scores
exactly (the count `autonomy: a differing score the log's with motive 14's advertisement as the
original made it`). Against the table's: interaction 24's delta 47 is 54 at tick 186 and 0 at
1443; 16's 7 is 9 (its personality 9, then 0); 13's 10 is 15 (then 9, with personality 9); 14's
0 is 3. So all three of motive 14's numbers are written at run time, per Sim and per tick, before
the score reads them (a script's write, it may be: a social interaction's appeal by relationship).
Who writes them is next (a watch on the advertisement's memory).

**The check tree ad range** (scope 12; `Host::ad_range_mut`, `EvaluationCheck::ad_range`). The
writer is the candidate's own guard: before it runs, `fn_80219530` makes the interaction's
advertisement the check tree ad range (scope 12, one delta a motive, data 0 to 15), and a social
interaction's guard adds to motive 14's (`scope 12 data 14 += local 0`: 47 + 7 = 54 for Sim 237's
interaction 24 at tick 186, 10 + 5 = 15 for its 13); the score then reads the ad range's deltas.
Natively: the evaluation sets the ad range to each candidate's deltas before its guard, keeps what
the guard leaves for its score and value, and puts the earlier ad range back (its datum 15 is the
shared value scripts use). Checked on the home log: all 109 scores the original's bit for bit (3
of them a place out of the log's order at tick 1443, which has one more call of the score than
candidates: compared as one of the tick's), and every value of the three choice lists; two lists
the same entire, the third (tick 1443) but for object 124's two equal entries, in the other order.
(The motive-14 entry the `A` record carries is the ad range's, which explains its other order.)

**The list's order.** The original's list holds the passing candidates the other way round from
the order they are checked and scored in (the last first), and its sort (the stable sort with the
1e-7 comparator, `autonomy_list`) starts from that: equal values end in that order. Measured on the
home log's one tie (object 124's two interactions at tick 1443, the same value): with the list
reversed before the sort all three choice lists are the original's entire, objects, interactions
and values bit for bit; the three choices, made with the original's random numbers, the log's.

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
low (−5 to −33); Sim 237's fun the lowest of its motives (about 21). Checked: the `disc` test
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

**Which objects block: not in the definitions or the objects' data.** Among the objects the
walks show blocking (the dining table's parts, the counters) and those the Sims' tiles cross
(the chairs, the newspaper, the burglar alarm, a painting, the wall phone, the wall lamps,
the windows), no OBJD field and no object data field (the 72 the save gives) separates the
two but the GUID and the id. Two things weaken the tile test besides: the logged tiles are
those of the smoothed line, which can clip a corner of a tile the route avoids (the chairs by
the table), and the speed is not constant (0.62 to 0.90 sixteenths a tick over a walk: it
starts and stops). So the next check is the original's own route, logged.

**The original's routing code**, found by the watch build (`GCN_WATCH_ADDR` on Sim 12's
place, `GCN_COUNT`) and named in the recomp's `names.txt`: the place's every change is a
store in `Object_SetPlace` (801F16D4), called from the walker `Route_WalkStep` (801F0410,
9,875 calls in the home replay); opcode 45, GotoRoutingSlot, is the person switch's case
`Prim_GotoRoutingSlot` (8021FE94, 647 calls), its worker `Route_Run` (802200C4, 2,002); a
large function of it, 80222FF4, runs 18 times, about once a walk (15 walks logged): the
planner, it seems (`Route_Plan`, to be observed).

**The original's planned routes, logged.** `Route_Plan` fills a vector at its state's +1140
with 8-byte entries, which are waypoints: x and y in sixteenths of a tile (the observer's
`G` records, on return; a new home log with the same 581,110 steps, 18 plans). They are lines
at any angle, not tiles (Sim 12's first, tick 260: (344, 120), (331, 123), (304, 125), (301,
125), (297, 185), ... to the front door), and a route breaks at a door: the yard to the door,
through it (tick 497: (296, 232) to (296, 263)), the house beyond (tick 521). So the original
plans by rooms and their doors, and within a room a line of waypoints. Checked: all 17 plans of two waypoints or more open
natively along their lines (`Walls`: no wall crossed where there is no door). Their lines
cross tiles of counters, the fridge and the stove as well as chairs (near their ends, where a
Sim reaches an object), so which objects block is still open. Next: the plan's waypoints
natively (the rooms, the doors between, the line within a room).

**The rooms** (`Walls::room`). The lot's third array is not the rooms: it marks the inside (1)
and the outside (7), the house's rooms all 1. The rooms are the regions the walls close, a
door's opening left closed and a diagonal wall's tile in none: Mom's house has 4 (the yard
with the porches, the main room, and two at the back), joined by its 5 doors. Checked on the
home log's plans: 13 of the 17 within one room; the other 4 change rooms only at a door, with
a waypoint on each side of it (a single step through the opening), so a plan is a piece a
room, joined at the doors.

**The routing slots** (`Slot`, `routing_slot`). A plan starts at the walking Sim's place (all
17). Its goal comes from a routing slot: GotoRoutingSlot's operand is the slot's number and
where it is from (big-endian 16-bit fields: 1 the stack object's own SLOT, named by OBJD field
18; 2 the global file's 15; 0 a stack variable), the number counting the routing slots (type
3) alone, as on the PC. SLOT is the PC's version 10, 70 bytes a slot: its type, an offset
(three floats), standing, sitting, ground, the flags (the directions it may be taken from: 1
the front, then round by eighths to 128; 0x55 the four sides), the snap slot, the minimum,
maximum and optimal proximity (sixteenths), a size, a gradient (3/16 nearly always: the
waypoints' 3/16 clearance), a height, the facing and a resolution. All 764 on the disc read
to their end. Checked on the home log's routing steps: 448 slots found natively (193 the
object's own, 255 the global ones), none missing; 81 name a stack variable (not modelled)
and 116 an object not on the lot natively. Measured towards the goal: plan ends are mostly a
target object's place (the counter, the meal, the chair being sat on) or a tile beside it
(the chair's global slot 1, the side nearest the Sim), and a door's crossing ends a sixteenth
short of the far tile's middle.

**The last leg: an object's front.** A plan to an object ends with the tile the Sim stands on,
then the object's own place (the Sim turns to it). The tile is in front of the object, one tile
away, by the object's facing: its data field 1, in eighths of a turn (`Object::facing`,
`routing::front`), 0 towards -x (the fridge, the counters, the stove), 6 towards -y (the easel),
2 and 4 by the turn between them (no logged walk ends at one). Checked: 5 of the 7 plans that
end at their routing step's object; the other 2 are to the dining chair, approached from its
side to sit down.

**A routing slot's goal** (`routing::slot_places`). A slot offers a place for each direction
its flags name (bit k: k eighths of a turn from the object's front), at its optimal proximity;
the Sim goes to the nearest of them that the walls let it reach and no other object stands
on. Checked on the home log's five walking GotoRoutingSlot steps: three goals the original's
(the counter's front; the group meal's four sides, two taken by the fridge and the stove, the
open west one; the chair's four sides, the south one nearest); the two others are slots of the
paper girl, a person whose facing changes as she walks (the save's 5 is stale by then).

**The planner's request, logged.** `Route_Plan` (80222FF4) is given a request (its r4) whose
vector of 16-byte goals (at its +0..+4) it passes to the search (`fn_8024C5DC`, which fills the
waypoints at the state's +1140); the goal it takes is the request's +88 on return (-1 none),
that goal's +14 kept at the state's +82. The observer logs the goals before the call and the
index after (`H`, before each `G`; a new home log with the same 581,110 steps). A goal is its
place in sixteenths, a cost (0x87 for a slot's places, 0x64 for a relative position's) and its
direction's bit (the slot's flag it came from) in the low byte of the last word. The request
holds only the open places: a slot of one proximity offers, in the order of its bits, those in
its object's room (one across a wall is left out though a door leads there: the group meal's
far side) that no other object stands on (a person does not count: the paper girl at the
counter's front); a diagonal's two parts are the proximity times the square root of a half, cut
to a whole sixteenth (11 for 16, the paper girl's seven goals at tick 1220). Checked on the home
log: of the four single-proximity requests on objects on the lot natively, three the native
open places exactly (the counter, the group meal, the chair), the fourth the paper girl's (her
facing stale); the one on an object not on the lot (her at tick 1220) fits a slot's shape. A
slot of a range of proximities (her slot 8 of the global SLOT, 16 to 64 at resolution 8)
offers an area of places, 48 at tick 931 (`routing::slot_area`): for each direction its flags
name, the points of that direction's own grid (steps of the resolution forward and sideways)
within 45 degrees of it and between the least and greatest proximity, a diagonal's grid turned
with it and its places rounded. All 48 goals lie in their bits' areas (408 places before the
closed are taken out), by facing 0: the paper girl's facing then, not the save's 5. The open
places are those in the object's room, strictly inside no blocking obstacle of the plan (an
obstacle's edge is open) and on or in no wall's rectangle: 135 of the places the builder reaches
at tick 931. Which 48 is the game's random generator's:
about 390 draws of `Sim_Random` at that tick before the plan (from 8024F030, on two generators in
turn, and from 8024F504), none logged yet; the goals' order (shuffled) comes from the same.
The goal's third word is its score (fn_8024E480, the builder): 100 less the distance times the
slot's gradient, with bonuses (25, 20) by tests not yet read, and with the slot's flag 0x2000 a
random part, `random % 20 - 10` (the draws from 8024F030). For the home log's slots (gradient 0)
it is 135 (0x87), and 125 to 144 on the paper girl's flagged slot 8 (`routing::slot_goal_score`):
50 of the 52 goals that are their slots' own places. The 2 others are tick 1790's second
request, made just after the first on the same table with the same two places: its goals carry
the table's id (49) in their last word's high half and score 225, 90 more. With that bonus for a
request that names its object (measured, not yet read: the base is the builder's `[r1+144]` or
`r14` by its tests at 8024EC3C), all 52. The 48 kept at tick 931 are not the best: they are
drawn (8024F504). The observer logs these draws too (`R`, sites 2 and 3: a new home log, its
steps and figures the same): at tick 931, 339 for the scores (8024F030) and 48 for the pick
(8024F504), one a goal; at tick 1579, 165 for scores (its 43 goals round the paper girl, not her
slot's places, are scored with a random part too) and none for a pick. All 48 goals' scores at
tick 931 are among the drawn ones. The builder's order: direction by direction (bit order), then
forward step, then sideways step, the sideways step from minus the forward one up to but not
including it, so the eight wedges tile the area (47 places a direction at 16 to 64 by 8). Each
place takes the next draw for its score: with that, all 48 goals' scores at tick 931 are their
places' draws. One direction stops short, the paper girl's northeast after 10 places (the builder
stops when its classifier, fn_80248F4C, puts a place in category 3, setting its +176 flag; not
yet read: the check takes the short direction's count from the draws' total). The pick (8024F504, `routing::slot_pick`): up to the request's cap (its +164,
48), a draw `random % n` into the n open places in the builder's order; a place already taken
passes to the next not taken (none past the last), and the place is copied out and its score
zeroed to mark it. All 48 goals at tick 931 are the places the 48 draws take, in order. Tick
1579's request (GotoRoutingSlot on the paper girl, whose one routing slot is 16 to 16 with flags
0x44) gives 43 goals that are not that slot's places: all with bit 1, in the builder's order of a
single westward wedge round the girl (392, 344), forward 16 to 72 and sideways ascending, its +
edge included (no neighbouring direction flagged), all kept (43, under the cap: no pick draws),
scored with 165 draws. Its range and score rule (scores 116 to 139) and why the request widens
are not yet read (the primitive's other operands, or the builder's fallback). The planner's choice among the goals is the
nearest native one in the two requests that took one natively.

**The planner's obstacles, logged.** (Read turned at first: see "The obstacles' objects" below.) The search (`fn_8024C5DC`) goes over the objects and, for
each on the Sim's level that it does not leave out (`fn_802499DC`), takes its footprint (a
virtual call, a rectangle {x0, y0, x1, y1} in sixteenths) and a cost: 0x7FFFFFFF, or 10 when a
virtual flag (its +704 with 10) says so; an object of kind 8 (its +872) adds a strip of a tile
at cost 10000 on each side its +1248 bits name; a blocking rectangle (or one of kind 2) is grown
on each side by half a global width (r13 - 28468, 6 in the home log: the 3/16 of a tile the
waypoints keep from corners); each goes into a vector of 20-byte entries, after which it calls
an empty function (`Route_ObstacleAdded`, 8024F82C, from 8024CD78). The observer logs the
vector as the loop last left it (`X`, before each `H`; a new home log with the same 581,110
steps; the record letter `O` was taken). In the home log's 18 plans, 709 rectangles: 384 a tile
grown by 3 (22×22), 47 half a tile grown (14×14), and smaller blocking parts (12, 6, 4, with
16×14 and 16×10); not grown, 64 whole tiles and 164 parts of 4 or 6 at cost 10 (the yard's
flower beds). Checked: all 709 a footprint within a tile, grown as their cost says. Against the
native world they do not yet meet: 226 rectangles have a native object at their place, 483 none
(another tile of a larger object, or objects not on the lot natively), and of the native
objects within their extent 278 are covered, 1,194 not (the dining table among them, though the
walk round it at tick 1905 keeps clear of it): which objects the search leaves out, and where
an object's footprint lies, are next.

**The obstacles' objects.** The loop asks `Route_SkipObstacle` (802499DC) of each object on the
Sim's level but the Sim; the observer logs each asked about, in order, with whether it was left
out (`J`, before each `X`; the part asked about is not the interpreter object but one of its
parts, 356 bytes into it or 104 or 112 before it by class, its owner named by a word holding the
owner's +0x16). The objects kept are the rectangles, one for one in order (one fewer in 7 of the
17 plans: the one overlapping the Sim, dropped). And the rectangles are turned: the vector holds
{y0, x0, y1, x1} (the footprint's four words taken in reverse), so the earlier reading put each
on the mirrored tile. Read the right way round: of the home log's 709 rectangles, 701 are the
footprint of a native object on its own tile (8 of objects not in the native world): a whole
tile for most (counters, the fridge, the table's four tiles, the couch's three), 8 at +4 for the
outdoor chairs, 12 at +2 for the end table, 16 by 14 for the armchair, 16 by 10 for the
jukebox; a person 6 on a side centred on its place (its place as logged, the walk not yet
native). The walk round the table at tick 1905 keeps exactly 3 off them (x 22 + 1/16: the chair's
8 at +4, grown by 3). Which objects are left out (932 native objects in the rectangles' extent
are not among them: rugs, windows, wall lamps, paintings, ...) and each footprint's size are next.

**The footprints** (`routing::footprint`). An object's footprint is its tile less its
definition's footprint mask, OBJD field 75 (the PC's FootprintMask): nibble k, the low first, is
the inset in sixteenths of the side k quarter turns from the object's front (`front(facing +
2k)`), so the mask turns with the object. Checked on the home log's 694 obstacles of native
objects (persons apart): 647 the native footprint exactly: whole tiles (mask 0), the trees' four
quarter pieces (0x0cc0 and its turns, each 4 by 4 in a corner, by facing), the armchair's 1 at
two sides (0x1010 at facing 2: 14 by 16), the jukebox's 6 at its back (0x0006 at facing 4), the
end table's 2 (0x2222), the newspaper's 5 and the garden lamp's 6. The other 47: the three
outdoor chairs, 0x3333 but 4 in on each side (8 by 8 at +4, one more than the mask), and two
of the game's own objects with mask 0 but 8 by 8 at +4 (`zz_move_a__tron`, the two-player money
trunk): something besides the mask shrinks them (found: the width, below).

**A definition's width.** OBJD field 65 (the PC's TileWidth; 0 for most objects) narrows the
footprint by (16 - width) / 2 more on every side: the outdoor chair's 14 (its mask's 3 and 1
more, 8 at +4), the tron's and the trunk's 8 (8 at +4); the other definitions with a width are
left out anyway (the table lamp's 8, the wall lamp's 4). With it all 694 footprints of native
objects are the planner's, and 16 of the 17 logged obstacle lists natively exactly the planner's
(but for 9 rectangles of objects not native); the 17th (tick 1220) drops the paper girl, not
native, as the one by the Sim.

**The planner's lines and the obstacles.** Against the native obstacle lists: every leg of the
home log's 17 plans keeps out of the blocking obstacles' insides (62 legs; it may run along an
edge or touch a corner), but for the 9 last legs into the object a plan ends at (the Sim turns
to face it there; the object is among the obstacles). Of the 54 waypoints between a plan's ends,
17 are corners of grown obstacles (round the dining table at tick 1905: the chairs' and the
table's), 16 the request's goal (the place before the object), 16 lie on the line between their
neighbours (a sixteenth off the start, a door's sides, a step along a straight line), and 5
none of these (tick 521's (313, 279), 1791's (403, 304) twice, 1905's (363, 247), 4922's (352,
403)). So the search goes from corner to corner of the grown obstacles, as a visibility graph
would; how it picks among them, and the points along its lines, are the search's own (its
`fn_80249BB8`, to read).

**A route within a room, natively** (`routing::corner_path`). The shortest line from the start
to the goal (the place before the object a plan ends at) that keeps out of the blocking
obstacles' insides and as far from the walls (each closed wall edge a rectangle grown by the
same 3: `Walls::wall_rects`), going from corner to corner of them, by Dijkstra over the
visibility graph. Checked on the home log's 12 plans whose start and goal share a room: 11 pass
exactly the planner's obstacle corners, in order; among them tick 1905's long way round the
dining table (the six corners of the chairs and the table), which the walls decide: without
their 3 the short way east of the chair, along the wall, is open. The 12th (tick 521, from the
front door to the fridge) turns at a wall's corner, (317, 275), where the planner passes (313,
279) and goes by the counter's corner (371, 317): the walls' corners are not yet the planner's.
The planner's other points (a sixteenth off the start, along its lines, near corners) are not
built.

**Persons among the obstacles.** The loop's kind-2 branch (`Route_SkipObstacle` for a person):
a person is left out when it is what the request goes to (the request's +132) or when its
distance from the planning Sim (a virtual, +128) is more than a float constant, 3.0 tiles (at
802CFAAC in the DOL); first a state test (`fn_801FD05C` on its +32 with 2) and the request's
+152 decide whether to ask at all. Natively (the places as logged, the walk not native; the
target the stack object of the tick's walking routing step): 17 of the loop's 20 answers about
persons; at exactly 3 tiles the log has them left out (the distance measured mid-tick, the
logged places being the tick's start). The loop's order (802499DC): three virtual tests of the
object first (any may leave it out); then, for a person, the request's +152 set leaves it out;
the state test (`fn_801FD05C` on its +32 with 2) returns the word at its slot 2's +4, for a
person its own object's (never 0), so it always goes on to the target and the distance. The 3
answers once put down to the paper girl were not hers: an object of another class 356 bytes past
her own, which the observer files under her by nearness, left out by the other branch (all 6 of
its entries). Counting only a person's own part (104 bytes before its listed object), all 14
answers about persons are the loop's.

**Plans through a door.** A plan breaks at a door (`The rooms`), so the pieces that change
rooms are short: up to a door, through it, from it. The same search (`corner_path`; the door's
opening is open to `Walls::line_open` and has no wall rectangle) takes the planner's corners in
3 of the home log's 4: the two crossings (ticks 497 and 2038, straight through), and tick 1901
(by the counter's two corners to the back door). The 4th, tick 260 from the yard to the front
door, goes natively by the house's wall corners on a diagonal (146 sixteenths), where the
planner goes west to an obstacle's corner (301, 125) and down along the wall (150): something it
counts lies on the diagonal. A cost for crossing the walk-through obstacles (the flower beds,
the trees, the door's own tile) at any weight does not give its line (tried, not kept).

**The floor in the planner's choice** (tick 260). The lot's first array is each tile's floor
(`REPLAY_SHOW_WALL_BYTES`): 0x00 grass, 0x0a the sidewalk (y 5), 0x1d the front path (x 18, y 6
to 11, to the porch), 0x33 the porch, 0x19 the house's floor, 0x2d the kitchen's. The planner's
line from the yard to the front door goes west onto the path and down it: 40.4 sixteenths on
grass, 110.1 on the path and the porch (150.6 in all), where the native shortest line runs 83.5
on grass and 46.6 on them (130.1). So the planner weighs the floor: grass at least 1.47 times the
path. Tick 521's miss (in the house, from the front door to the fridge) is not the floor: its
line has a little more of the kitchen's floor than the native one, the rest the same.

**The search's own regions, logged** (`D`). The search (`fn_80249BB8`, when the planner runs it)
fills a second vector of the same 20-byte rectangles, calling `Route_ObstacleAdded` after each
(from 8024A068 to 8024C358); the observer logs it as last left (`D`, before each `X`; a new home
log, the same 581,110 steps). On the yard plan at tick 2: 36 blocking ones, the lot's border and
the house (its rooms, closed to a plan outside, and its walls as zero-width lines), and 7 at cost
0: the street, the sidewalk, the front path, the porch and its steps, the back path. Checked
against the floor array: the tiles in the cost-0 rectangles are the outdoor floors other than
grass (the street 0x27 to 0x29, the sidewalk 0x0a, the paths 0x1d and 0x1b, the porch 0x33; a few
at their edges), and outside them the grass (0x00) and the house. So the paved floors are regions
of cost 0 to the search and the grass is not: the reason tick 260's line keeps to the path. (The
other plans' vectors are empty or the house's 27: a later plan in the same place reuses it.) How
the search weighs a line by them is next.

**The line weighed by the paved regions** (`routing::weighed_path`, study `HF_PAVED_WEIGHT`).
The corner search with a line's length outside the paved regions counting w times its length in
them, and the regions' corners (and those corners 3 in, the half-width) among its nodes; the
regions as logged (`D`, the latest with paved ones). At w from 2.5 to 3.5, 15 of the 16 plans pass
the planner's corners, tick 260 among them, now by (301, 125), the path's edge 3 in, and down the
path; below (2.3) a plan in the house turns differently, above (4) one does. So grass weighs
about three times a paved floor; the planner's own figure (a constant of the search, it may be)
and the regions natively from the floor are next.

**The paved regions natively** (`Walls::paved_regions`, `routing::OFF_PAVED_WEIGHT`). An outdoor
tile (the third array not 1) whose floor is not grass (0x00) nor none (0xff) is paved; each row's
runs of them are joined with the same runs below into rectangles. Against the search's logged ones
(tick 2): the street, the sidewalk and the porch exactly; the front path to the porch (the planner's
runs on through it), the porch steps a tile narrower, the back path one rectangle where the
planner's are two, and some beyond the lot's border (outside it, harmless). With them and a
weight of 3 (any from 2.5 to 3.5: fitted, the planner's own figure not yet found in its code) the
replay's corner search is now weighed by the floor by default: 15 of the 16 checked plans by the
planner's corners (4 of 4 through a door, 11 of 12 within a room; the one left, tick 521 in the
house, not the floor's).

**Tick 521, the miss left.** From the front door to the fridge, in one room but for the opening
in the wall at x 320 (y 272 to 320, which both lines pass): the planner's line (131.0 sixteenths)
is longer than the native shortest (126.6), bending in small steps, (301, 259), (313, 279), (349,
313), along the counter's top edge to its corner (371, 317), and on to the goal. None of its
points is a corner of an obstacle or of the search's own rectangles (`D`: the house's outer walls,
the walls as lines, the room's openings), grown or not. So the planner's line is not the visibility
graph's optimum here: its search finds a route its own way (by a grid, and then pulls it straight,
it may be: the in-between points of the other plans look the same) and the corner search agrees
with it where the two coincide, 15 plans of 16. Reproducing it is the search's code (`fn_8026A210`
and its callees: chunk 2).

**The walk keeps to its plan's line.** The planning Sim's logged places (`P`, at each tick's start,
whole sixteenths, logged when they change) from its plan's tick to its next plan, against the
plan's line: 852 of the home log's 854 within a sixteenth and a half of it (the 2 others the paper
girl's, moved off the lot after her walk). Along the line the place moves a sixteenth every one or
two ticks (about 0.7 a tick: so a fractional position underneath, the logged place its rounding),
3 to 6 every four ticks through a door (the door's own animation), and its starts and stops are
the speed's.

**The walk's speed, measured.** Along the line (the study `REPLAY_SHOW_WALKS`), away from a walk's
first and last 10 ticks: 0.82 to 0.85 sixteenths a tick on the home log's 7 long walks (0.818 to
0.852, about a tile in 19 ticks), less over a short walk (0.58 to 0.74: a start and a stop). A
walk's first move comes 1 to 89 ticks after its plan (26 for several: a Sim standing up, turning,
or a script's animation first). So the speed is not one constant, and the logged places alone do
not give it exactly. `Route_WalkStep` (801F0410, as the recomp's names.txt has it) is not the
walk's step: it moves an object (each of its parts, from `fn_801D3524`, the group's move) to a
place it is given; the distance a tick is decided above it, likely from the walk's animation
(its root's motion), which the engine does not read yet.

**The walker found** (`Route_Follow`, 800AE0A4). The observer logs each call of 801F0410 for a
person with its return addresses (`F`): 9,827 of the home log's 9,863 come from 800AE0A4, a
follower's step (and 3 levels up, 800AB820, 8005BD68). Its follower (r3): +4 the object's
handle, +12 the waypoints (8-byte entries), +28 a state (1 starting, 2 walking), +40 and +44 the
place as floats, stored turned (y, x) like the planner's obstacles, +124 a speed factor (1, or 5);
the step is per frame, the global at r13 - 29720 being 1/30. Logged before and after each call
(`Z`; a new home log, the same 581,110 steps): only three followers move, Sim 12's, Sim 237's
and the paper girl's (their first places their plans' starts, turned). The step grows by a
fixed amount a frame (0.0432 sixteenths for the Sims, 0.0376 for the paper girl, a child) to a
top (0.6547 and 0.5702: each its top over 15.15 frames), times the speed factor (5: 3.27 and
2.85). Of the 1,724 steps that move, 1,416 are the top times the factor exactly, 46 the ramp
exactly (k times the step's growth), 262 shorter (a stop, a turn, a waypoint reached within the
step). At 0.6547 a frame the Sims' 0.82 a simulation tick is about 1.25 frames a tick: the walk
runs by the frame, not the tick.

**The walker's step** (`routing::walk_step`, `along_line`). Walking (factor 1) the speed grows by
its growth a frame (the top over 15.15 frames) to its top, and near the end is held to what lets
it stop there, sqrt(2 × growth × the distance left) (the stops' steps fit it to the thousandth:
4.269 left gives 0.608); the distance runs along the line, round its corners within a step (the
chord between two places is then shorter), to the walk's end: the plan's last point but for a
plan of three or more, whose last leg into the faced object is not walked. The fast mode (factor
5) has neither ramp nor brake: its top times 5 from the first frame, the last step the distance
left. Checked on every logged step that moves (`Z`), each from its logged place: 1,511 of the
1,700 on a logged plan's line natively the original's to a thousandth of a sixteenth (24 more not
on one, set aside). The rest: one walk of Sim 237 whose steps each run 0.02 beyond the ramp, and
the door's states (4 and 5).

**The walker's speeds, from the DOL** (`routing::walk_rates`). The follower's step works in time:
its budget a frame is the frame (the global at r13 - 29720, 0.033 s, not 1/30) times its factor,
spent at its speed in sixteenths a second (its +144). The top is a constant of the DOL: 19.84 at
802A8DB8 (an adult), 17.28 at 802A8DC0 (a child: the paper girl), 64.96 at 802A8DBC when the
follower's +132 is 1; the speed grows by the top over 0.5 s (802A8DC8), the brake its stopping
distance at that rate. So a frame's top is top × 0.033 (0.65472, 0.57024) and its growth top ×
0.033² / 0.5 (0.04321, 0.03764): the logged figures. With the constants in place of the logged
tops the same 1,511 steps are exact. Which top a walker has is the original's test through the
follower's +8 (`fn_80059194`): its person's body (a virtual on its +1004, then 8022A45C and
8022A46C) has a skeleton entry at +108 whose +4 is 1 for an adult (else one at +112, tested the
other way). That is the body's age, the first of the body strings (STR# 200: "adult" in
`template_person`, "child" in `npc_papergirl`), not the scripts' age (person data 58, 20 for all
three walkers of the home log). Natively `routing::walk_top` takes it from the person's file:
all 3 walkers' tops the original's. A template person's body comes from the save's character, so
a child Sim of a save still needs its body strings from there.

**A walk's start and end.** A walk starts at the speed the follower's last walk stopped at: its
speed (+144) is not reset (the "0.02 beyond the ramp" of the earlier figures was that). And it
ends at the request's chosen goal (`H`): a plan's last leg past the goal, into what the Sim then
faces in the same room (an object, the paper girl), is not walked; one through a door is (the
door's crossings walk to their far side). With both: 1,653 of the 1,701 logged steps on a plan's
line natively the original's to a thousandth (23 set aside, on no logged line). The rest: a walk
begun just after one cut short at speed (tick 1909: it starts near its top), the brake of one walk
(ticks 1639 to 1645), the walk up to the door at tick 4922, and the door's own states (4, 5).

**The walk run free.** The same steps without the log between them: a walker's place set at its
walk's start (state 1, its speed the one its last walk left natively) and then moved only by the
native step along its line, frame after frame, against the logged place after each. On the home
log: 1,270 of the 1,465 frames that move at the original's place to a hundredth of a sixteenth;
the walks part from it at six frames only (the door's fast mode at ticks 488 and 512, a ramp at
1275, the brakes at 1639 and 4915, and the walk begun at speed at 1909), and once parted stay off.
The walk at 1909 starts from the speed of the follower's last frame of state 5 (the move on to
its +116/+120 after a walk, at 1645: 0.557, so its first step 0.557 + 0.043): the speed carried
into a walk is its last frame's, state 5's too. State 5 is not native yet (it does not brake as
a walk toward its point: at 2036 it keeps its top 2.3 short of it), so its speed is taken from
its logged step; with that, 1,677 of the 1,697 steps on a line natively the original's and
1,431 of the 1,461 frames run free. A walk followed by state 5 does not brake: it reaches its end
at speed and runs on (the home log's 5 walks into state 5, at 487, 511, 1643, 2034 and 4918, end
at their top or the fast mode's; the 11 into state 4 brake to a stop). Which walks run on is
the walker's object data 9 (see "State 5, the run on" below). With it, 1,691 of 1,697 steps and
1,444 of 1,461 frames run free; the rest the fast mode's two walks (488, 512), a ramp at 1275,
and two runs on that end mid-line (2035) or past the line's end (4919: a step that runs on is
not stopped at the end).

**State 5, the run on** (`routing::run_on_point`). Route_Follow, entering state 4 at a walk's end,
asks the walker's object a test (a virtual at +708 of its class, with 9: fn_801F5F4C, which
returns the object's data 9, the 16-bit word at its +142 + 2 × 9, data 11 there being its id); if
it holds (a stderr study of the home run: data 9 is 1 at all 5 walk ends that go on to state 5
and 0 at the 17 others), the follower goes to state 5 with its point (+116/+120) set from the plan's last waypoint
but one (where the walk ends) towards the last, at most 3 on each axis (the DOL's 3.0 at
802A8D40; 9.0 at 802A8D3C when the follower's +132 is 1). Checked: the 3 entries into state 5
with a logged plan ending there (ticks 496, 1645, 2037) natively the original's point; the 2
others (the fast mode's at 519, the overshoot at 4920) have no plan ending where they start. A
person's data 9 is set by its own scripts: an expression "my object data 9 := 1" (scope 3, data 9,
op 5) in the routing trees run before a walk (trees 8215 and 4107 of the home log), and cleared
when the routing primitive ends (Route_Run, 80220918, through the object's setter). Natively the
replay keeps that per person from the steps it runs (the expression's literal; a GotoRoutingSlot
or GotoRelativePosition returning clears it): all 16 walk ends' run on as the original's. Its
move: the speed the walker carries, kept (no ramp, no brake), times the game's speed, up to its
point (the first frame with the point just set). 5 of the home log's 6 moving state-5 frames
natively the original's; the sixth (tick 1646, a diagonal move: 0.557 where the carried 0.655 is
expected, 3.59 from its point) is not yet understood, and it is the speed the walk at 1909 starts
from. (Read since: see "The mover" below; all 6 natively now.) Route_Follow's
frame: the budget (the frame times the game's speed, +124), then fn_800AE6FC for every state (the
walker's facing, +56, turned towards where it goes: in state 5 its point, +116/+120, less its
place, +40/+44; the angle constants at 802A8D64 to 802A8D90), then in state 5 fn_800AEBFC, which
moves the walker towards its point through the general mover fn_800AEC38 with that budget. Tick
1646's slower frame is the one where the walk's heading (from (380, 338) to (376, 336)) turns to
the diagonal: the turn may cost speed there (not yet read in the mover); tick 2036's state 5,
straight on, keeps its speed.

**The mover** (fn_800AEC38, `routing::walk_step`). The walker's speed (+144, sixteenths a
second) and step, in state 2 and state 5 alike: its stopping distance D is half its top times its
ramp's time (0.5 s over the game's speed, +124; 1e12 when paused); when the distance left (to the
waypoints' end, or state 5's point) is under D, the walker is on its last leg and its object data
9 is clear, the speed is set to 1.001 × sqrt(2 × top/ramp × left) (1.001 the DOL's at 802A8DD0);
otherwise it grows by top/ramp × the frame to its top. The frame's step is the speed times the
frame (0.033 s), the factor folded into the ramp. With the 1.001 and data 9's test: 1,693 of the
1,697 steps on a line natively the original's, 1,457 of 1,461 frames run free, all 6 state-5
frames (1646's is the brake, data 9 cleared that tick; 2036's keeps its speed, data 9 still
set), and, with a whole walk run on when its walker's data 9 is set and started from the speed
the walker carries natively (not its last logged step), all 10 whole walks and all 10 routing
primitives' returns. A walk that runs on ends at the plan's last point but one (state 5 goes on
from there): with it 1,695 of 1,697 steps and 1,459 of 1,461 frames run free. The two left (512,
in the fast mode, and 4919) end past their walk's end by exactly the frame's budget left over
(3.274 − 3.179 and 0.655 − 0.287): the rest of the frame spent going on towards the follower's
point; tick 2035's does not (its point then a stale one, elsewhere): not yet read. The plans'
lines end where the walks do (512 at 248, 4919 at 392), so the overshoot goes beyond the plan,
towards what the follower's +116/+120 holds before its state 5 sets it ((296, 263) and (344,
407)). Route_Follow sets it (800AE4CC): in state 2, when the waypoint index (+24) reaches the
last leg (count − 1), the walker's data 9 is set and the follower's +888 is clear, its point
becomes the last waypoint (from the follower's list, +12), and the frame's walk goes on towards
it. So a run-on walk heads for its list's last waypoint: (344, 407) at 4919 and (296, 263) at
512 lie past the logged plans those walks are matched to, so the follower's list there is not the
plan the replay finds (an older or longer one); 2035's stays at its corner. Which list a follower
holds is not yet modelled (the replay matches a walk to the latest plan whose line it is on).

**A routing primitive, natively** (`routing::walk_frames`, `routing_return_tick`). From its
walker's start: the whole walk frame after frame, the turn, one frame more, and the return on the
tick after that frame's (the frame clock as logged); each step before it a yield. On the home log
all 10 runs whose walk ends in a turn return on the original's tick, and all 1,430 of their steps
(yields and returns) are the original's; the replay's host decides those steps so too (1,427
of them as it reaches them, none different). A walk that runs on ends the same way with a turn of
no angle (one frame arrived, state 3 to 4, then the frame into state 5): with it 12 runs modelled
(5 run-on ends right), all 12 return on the original's tick, and the host decides 1,586 routing
steps natively, none different (409 left in runs not modelled). A walk begun where a run on left
its walker (past its plan's start) goes from there through the plan's points after its start,
less the sixteenth the planner nudged off it; and a walk end is a door's crossing only when its
routing run goes on more than 5 ticks past the plan that follows. With both, all 16 whole walks
and 15 routing runs native and right, 10 of 12 with the frame clock native, and the host decides
1,898 routing steps natively, none different (97 left, in one run). That run's walk starts the
tick before its first yield (the primitive plans in the tick the one before returns): with a
run allowed to begin a tick after its walk, all 16 of the home log's routing runs are native,
11 of 13 with the frame clock native too, and the host decides 1,989 routing steps natively,
none different (6 left, outside any run's span). With the frame clock native as well (`FrameClock` at
its fitted phase giving each frame's tick; only the walk's start taken from the log), 7 of the 8
runs it covers return on the original's tick (2 start before the ticks it is fitted over). The
clock runs at the game's speed (`FrameClock::frames_to_tick_at`): a tick takes 0.042 s over the
speed, so in the fast mode (5) a walker gets a frame every 3 or 4 ticks; with that 3,822 of the
3,835 ticks a walker moves in hold the original's frames (3,736 before), and the one run left
(from tick 304, into the fast mode) returns 4 ticks late: the speed switches a few ticks before
the first fast frame shows it. Ruled out as its cause: the clock kept in a normal tick's time
(a frame adding its time times the speed, so a leftover is not rescaled at the switch: 3,730
ticks) and each tick taking the speed of the next frame rather than the last (3,800). The clock
started afresh on entering the fast mode puts the first fast frame where the log has it, and
with it all 14 covered runs return on the original's tick; but no rule found for leaving the
fast mode keeps the phase after it (afresh again: 3,178 ticks; the leftover kept: 3,168; scaled
as the same part of a tick: 1,853), so it is not kept. The DOL has no 0.042, but 1/24 at
8037BC58 and 8037DB70: with it as the tick and the frame at 0.033 only 2,413 ticks hold the
original's frames, so the clock's ratio (14 frames in 11 ticks, 0.033 / 0.042) is not those two
constants; its real source is still to find.

The club's logs (`tree.log`, `tree3.log`) are older than the places, walkers and plans records
(`P`, `Z`, `G`, `H`): their routing runs cannot be checked natively, and the replay's host takes
their results from the log (new club logs would need a club replay, which the loop's game runs
do not have).

**The planner's search, its shape** (for chunk 2). fn_8024C5DC gathers the obstacles, then
(8024D344) builds a graph with fn_8026C9AC and runs the search fn_8026A210 on it (at 8024D384,
and again at 8024D5F0). The search's state (its r3): a node vector from +4 (and another from
+20), the node it is at (+100) and the goal's (+96); it returns when they meet, pushing nodes as
it goes. Its helpers: fn_802685DC (six calls), fn_8026861C, fn_8026875C, fn_80268950,
fn_80268ED4, fn_8026CA50, fn_8026CB00. The point test fn_8024C408 sits on a branch before
(8024D26C) the home run never takes. To make the planner's own points checkable the observer
will log the graph fn_8026C9AC builds and the nodes the search visits, one record at a time.

fn_8026C9AC only starts a search: the state's +0 its map (a fixed node graph: two in the home
run, at 80C26600 and 80C26638), its vectors cleared (+4 and +20 of node indices, +36 of 8-byte
points), +104 −1, and the first step run (fn_8026A038). The search then runs a step at a call
(fn_8026A210): +56 counts the steps, +100 is the node it expands (of the map's, numbered from 1),
and when it reaches the goal node (+96, 1) it returns 1 with the way's points at +36, stored
turned (y, x). A stderr study of the home run: at tick 2 the way (328, 104), (328, 96), (333, 86),
(355, 86), (600, 88) after six steps, where the logged plan has (328, 104), (332, 96), (333, 93),
... So the planner's points are a coarse way through the map's nodes, refined after (the
in-between points of chunk 2 are that refinement). The observer now logs the coarse way (`I`,
a new home log with the same steps and figures): it equals the native corner path in only 4 of
the 16 plans. Its points are not the obstacles' corners (tick 2's (328, 96) and (461, 86) where
the plan has (332, 96) and (461, 88)); the plan's nudged sixteenths are already in it (497's
(296, 233), 2038's (344, 377)). So the search's nodes, and the refinement that moves its points
onto the plan's, are what chunk 2 needs; both now checkable against `I`. A first look
(`REPLAY_SHOW_COARSE`): moving each coarse point to the nearest logged obstacle corner within 12
lands it on one of the plan's points for many interior points (tick 1901's (338, 317) to the
counter's (349, 317), (331, 368) to the door's (336, 368); 521's, 260's, 1905's), not for the
ends or for points in the open (tick 2's (461, 86) where the plan has (461, 88), on the paved
region's edge). So the refinement looks like a string pulled taut through the openings between
the map's cells, its points their ends (obstacles' corners, cells' edges): the map's cells are
the next thing to read. The search's +0 (80C26600, 80C26638) is not the node graph but its
request: the start at +12/+16 and the goal's rectangle at +72 to +84 (turned: tick 2's {88, 600,
89, 601}, the goal (600, 88)), a pointer at +8 (8102E764 at tick 2, 8102E814 at 521) to what
holds the nodes, a 4-byte vector at +56. So the nodes expanded (1 to 17) are likely cells of the
free space between the obstacles and the search's rectangles, the coarse points the crossings
between them: +8's structure is next. It holds, at its +0, the search's own rectangles
(20-byte {y0, x0, y1, x1, cost}): 43 in the yard's map, 27 in the house's, the `D` records'
counts. The coarse points are not their corners (the nearest at least 8 away; `REPLAY_SHOW_COARSE`
prints them), so the nodes are cells cut from these rectangles and the obstacles, the coarse
points likely the midpoints of the openings between them: the cutting (in fn_8024C5DC before
fn_8026C9AC, or fn_8026B39C) is next. The search's nodes: fn_802685DC gives node i of an object
as its 44-byte entry in a vector at that object's +76 (to +80), none past the end; every step
of the search and its helpers ask for nodes so. The way at +36 is written by fn_80268950 (425
lines; from fn_8026A038, the search's first step, and fn_80268ED4), from the nodes the search
reached: the 44-byte nodes' layout (their cell, their link) is next, by a study dump. (The
search state's own +76 is not that node table but a vector of node indices, the order it took
them in: 0, 3, 9, 11, 2, ... at tick 2, the nodes it expanded; the 44-byte table is the object the
lookup is called on, still to find.) That table is a global (8035D2F4, its vector at +76), 15
nodes at the home run's first search (a study dump, reverted). A node is an A* node: +4 its id,
+8/+12 two links (-1 for none), +16 a kind (0, 3 or 10), +20/+24 its point (as the coarse way's,
turned y then x), +28 its parent, then f, g and h as floats (f = g + h). The step's cost is
(distance + 1) times the kind's weight, 1 for kind 0, 2 for 3, 6 for 10; it holds for every node
with a parent (9 of 9; e.g. node 13, (96, 423) from node 2 at (86, 355), g 44.18: 44.18 +
2·(68.73 + 1) = 183.64, logged 183.64). The h is the plain distance to the request's chosen goal (the
'H' record's, (600, 88) here; 13 of 13 nodes to three places), weight 1, so the search is a
plain A*; nodes 0 and 1 (state -1) are its own start and end markers. The coarse way ('I') is the
best node's parent chain, start to goal: here 3, 9, 11, 2 and then the goal itself, and every
coarse way in the home log ends at its request's chosen goal (16 of 16). Where the nodes come from, at
that first search (its D and X records, raw order as the nodes'): the paved strip D (80..96,
16..608, cost 0) and the cost-10 obstacle X (117..123, 325..331) lie round the start (104, 328).
Five nodes sit on those rects' edges: 9 at 96 and 12 at 80 (the strip's two edges), 6 at 117 (the
obstacle's), 13 and 14 at 96 again. Each one's kind is the cost of the area past that edge in
the step's direction: 9 enters the strip (0), 12 and 13/14 leave it into the yard (3, taken as
the ground's own cost, not yet found in any record), 6 enters the obstacle (10). So a step goes
from a node to where its ray crosses a cost edge, and is weighted by the cost it crosses into,
not the one it walked over (3 to 9 crosses 8 of yard at weight 1). Not yet explained: the
off-edge nodes 4, 5, 7, 8 (the fan round the obstacle, kind 3, at 13 to 13.6 from the start),
10, 11 and 2 (inside the strip, kind 0), and how many rays a node casts. The edge's cost is read in the
code (fn_8026875C(graph, a, b, out)): w = (kind(a) + kind(b) + 2) >> 1, the two kinds' mean
plus one rounded down, and cost = (|a's point - p| + 1.0) · w (1.0 at 802D1938), where p is
b's own point when a and b share their +4 id, else the crossing point fn_80268344 finds between
them, stored to out. A missing node costs 0.0 (802D1928), and from node index 0 (the start
marker) the cost is the plain cost of the rect that b's +0 indexes. Every parent in the dump is
kind 0, so the weights 1, 2 and 6 for kinds 0, 3 and 10 are (0+k+2)>>1. A step between two
kind-3 nodes weighs 4, not 2: still to see in a dump. The crossing point (fn_80268344 via fn_80268284): each node's +4 is
the index of its cell, a 20-byte rect {y0, x0, y1, x1, cost} in a vector at the graph's +12;
both cells are grown by 1 on every side (fn_80248D20(r, 1, 1): y0-1, x0-1, y1+1, x1+1),
intersected (fn_80248DD8: the overlap, all zero unless they overlap strictly on both axes), and
the point is the overlap's centre, each axis (lo + hi) / 2 rounded toward zero. So the graph's
nodes are its free cells, the search runs cell to cell, and the coarse way's points are the
midpoints of the shared edges: the "string through cell openings" before refining. Checked on a one-off dump of
the graph's cells and nodes at the home run's first search (reverted): every node's point is
that centre for its parent's cell and its own (12 of 12; the start node keeps the start), and
every node's kind is its cell's cost (13 of 13). The cells are not a whole-lot grid: 13 rects,
all round the start and along the way to the goal (y 67..185, x 23..601), e.g. cell 0 (96, 323,
117, 333), the start's, cost 0; cell 4 the cost-10 obstacle itself (117, 325, 123, 331); cells 3,
5 its sides to y 185; cell 1 the paved strip from 355 to the goal (80, 355, 96, 601). So the
cells are cut as the search needs them, from the D and X rects. Their edges, checked on that dump
against the same plan's D and X: every cell edge (52 of 52) lies on an X obstacle's edge as
logged, a cost-0 D region's edge as logged, or a blocked D rect's (the lot border's, the house's)
edge pushed out by 7 (the border's 16 becomes 23, the house's 192 becomes 185). 7 is the only such
value that fits; with 0, 3, 6 or 8 only 45 fit. The cutter is fn_8026B39C (5.6 KB, from the
search's init fn_8026C9AC), with fn_8026901C (1.3 KB) its likely rect splitter; which rects it
cuts, and in what order, is next. When (a study printing the
graph's counts round each search step, the run's first three searches, reverted): the init
cuts 2 cells, the start's and the goal's (cell 0, round the start, and cell 1, the paved strip
holding the goal (88, 600)); then each step cuts the neighbours of the cell it expands, so the
cells grow in A* order: expanding node 3 (cell 0) adds 6 (cells 2-7: three under it split at
the cost-10 obstacle's x edges, two thin side strips, and one above), node 9 (cell 7) adds 3,
node 11 (cell 9) adds none (its neighbour, cell 1, is already cut), node 2 (cell 1) adds 2. The
second search goes 2, 8, 11, 14, 15, 17 and the third 1, 5, 5 (start and goal in one cell).
So the native planner needs: a cell round a point, and a cell's neighbours across each side. A rule fitted
by hand to that dump (raw order: y0, x0, y1, x1): a cell's sides go in the order x0, y1, x1, y0;
on each, take every stretch of the line just past the side that is free (no obstacle, no cell yet)
and of one cost; grow it sideways along that line first, as far as the same cost and nothing in
the way allow (an X obstacle, a blocked D grown by 7, a cost edge, an existing cell); then grow
it outward the same way. It gives cells 2-10 exactly (9 of 9): e.g. cell 10, from cell 7's y0
side at x 323..333, first runs sideways to 601 (the border) and only then up to 67, where X (45,
349, 67, 371) stops the whole width; cell 2 runs sideways to 125 and then out to 304, the paved
region's edge. Not yet: cells 11 and 12 from cell 1's y1 side (355..601 at y 96). Grown whole,
it would stop at 156, where the cost-10 rects (156..164, 492..500) begin. Instead it splits at
x 492, the near edge of what blocks it: 355..492 runs on to 185, the house grown by 7, and
492..601 stops at 156. The split rule (and whether a middle obstacle makes three) is for the
cutter's code, fn_8026B39C / fn_8026901C. The grower, read (fn_8026901C(graph, rect)): it grows a seed rect, not
a stretch at a time. Four steps, one a side in the order y0, x0, y1, x1, start at -s, -s, +s, +s,
with s = 32, or a quarter of the map's cell cap (map +44, at least 1) when the cap is on (map +40).
It goes round the sides (a failed move halves that side's step and tries the same side again;
only a kept move or a zero step goes on to the next side): move the side by its step, and keep
the move only if the rect is still
in the same region as the seed (fn_802684C4, not blocked: cost != 0x7FFFFFFF), overlaps no
existing cell (fn_80268440 = -1), and is of the seed's kind (fn_80268FC0), and, with the cap on,
neither side exceeds the cap. A failed move is undone and the step halves, rounding toward zero.
A side whose step is 0 is skipped; it stops when all four are 0 (or after 200 tries). So a cell's
shape depends on the order its sides reach their stops, which explains cells 11 and 12: they are
whatever the race of steps 32, 16, 8, ... gives, not a split. Then, if map +32 is on, a cell
longer than map +36 times its width is cut back round its seed (fn_8022F890). The three tests, read:
fn_80268440 is the first existing cell that strictly overlaps the rect (its index), else -1;
fn_80268FC0(a, b) is "a lies inside b"; fn_802684C4 walks the map's rect list and gives the
first strictly overlapping blocked one (cost 0x7FFFFFFF), failing that the last overlapping one,
failing that none (and a second list, behind a flag, the same way). So a move is kept while the
rect still finds the seed's own region, is not blocked, overlaps no cell, and lies inside that
region: a yard seed, in no region, must stay clear of every rect. Simulated (a script on the
first plan's D and X, the blocked D grown by 7, no cap): from a seed at the start (104, 328) it
gives (96, 323, 117, 333) and from the goal (88, 600) it gives (80, 355, 96, 601), the dump's
cells 0 and 1 exactly, a point seed or a 1x1 one alike. The start cell's cost is logged as 0
where the yard's is 3: the start's is set apart. The neighbours, simulated the same way: for an
expanded cell, take its sides in the order x0, y1, x1, y0; walk each along the line just past it
(x0 - 1, y1, x1, y0 - 1), in rising coordinate, and from every point there that is neither
blocked nor in a cell yet, grow a cell from that point. Expanding the dumped cells in the
search's order (0, 7, 9, 1) gives all 13 dumped cells exactly, in order (13 of 13); of the 24
side orders only that one does (the next best, 9). The whole search, simulated (a Python model: start cell
cut round the start at cost 0, goal cell round the goal, then A* over cells, each expansion
cutting its cell's neighbours, an edge to every existing cell its grown rect overlaps, crossing
at the overlap's centre, cost (d + 1)·((ka + kb + 2) >> 1), h the distance to the goal, and from
the goal's cell an edge to the goal itself): of the home log's 23 searches (a plan often runs
two, 'I' before its J, D, X, H and G) it gives 12 coarse ways exactly. Every first search in the
yard is among them. Not yet: the plans' second searches where they differ from the first (plan
0's second runs to 461 and 473 along the strip, not 355: another setting, perhaps the map's cell
cap), the searches through the house (plans 2, 4, 14, 16: their walls and doors), plan 9 (no
start cell in the model), and plan 10's second point (313 for 317). Why two searches (fn_8024C5DC, 8024D20C..8024D5F0): the request's
+136 is the cell cap's switch and +140 the cap (128, or the caller's +440); the first search
runs with the cap off (+136 cleared at 8024D340), only when the switch was on and a count
(fn_80235EDC of +444) is under 256. If it finds no way the plan fails (+144 := 3). If it finds
one, the switch goes back on and the search runs again from a fresh init, and that second way is
the plan's. With the cap on, a cell may not be longer than 128 on either side (the grower's step
is then 128 / 4 = 32 as before). With the cap in the model for every second (or only) search,
plan 0's second way comes out exactly too: 13 of 23. The misses left are the house's (plans 2,
3, 4, 14, 16), plan 9 and plan 10. Plans 2, 3, 12, 14 and 16 log an empty D (the observer's "fn_80249BB8
did not run"), so the model had no regions there and the planner presumably kept the last ones
it built; every empty-D plan but 12 is a miss. A model run that let an empty D keep the previous
plan's regions did not finish (stopped after 55 minutes: a runaway search or cut, so the model
needs a step cap like the original's, min(1000, 50 + n·k)). With a cap of 1000 expansions it
finishes: still 13 of 23 (plan 14 comes closer, its way's second half exact; plan 2 hits the
cap, plan 16 finds none). The union of the last yard D (43) and house D (27) for an empty D is
worse (12; plan 12 lost). So which regions an empty-D search uses is not to be guessed: the
region lookup reads two lists through the graph (*(8035D2F4 + 4), and *(8035D2F4 + 8) when not
null); they are the thing to log. Logged now ('r', after each 'I'): the first list is the X obstacles
as the search had them, the second the D regions with the blocked ones already grown by 7 (the
lot's border (0, 0, 16, 624) is (-7, -7, 23, 631) there). The empty-D plans use the yard's 43
regions (plans 2, 3, 16) or the house's 27 (12, 14), not simply the previous plan's. On the
logged lists the model gives 15 of 23 coarse ways exactly (plan 16 now too). The misses left:
plans 9 and 10 start one way off (313 for 317 in y at the first crossing), plan 3 lacks a point
next to its start (233, 296), and 2, 4 and 14 differ inside the house. The first three look like
the start's cell. They were the grower: it retries a side after a failed move (the failure path
goes back to 80269238, which does not step the side on), where the model had gone on to the
next side. Fixed, and checked on a one-off dump of every search's cells (reverted): 21 of 23
coarse ways exact, and the cells of 9 searches equal the original's in full. Left: searches 7
and 19 (of 23, in log order), start cells off by a little (search 20's y0 247 for 249, search
7's y1 368 for 359), and the cost of a cell in no region: 3 in the yard plans' searches but 0 in
the house plans' (cells of equal shape, cost 3 in the model, 0 in the original). That cost is the
request's +48 (fn_80269514 at 80269684: a cell in a region takes the region's cost, else
*(*graph + 48)), now logged with the request's other grower settings at the end of 'r': +48 is
3 in the yard plans and 0 in the house's; the cap's switch +40 is on only in second searches
(the house plans' single searches run uncapped; +44 is then stale); the aspect trim's +32 is
off in every search of the home run. On the logged settings: still 21 of 23 ways, but 8 searches'
cells equal in full, and the first differences now come right after the start cell: the order
in which neighbours are cut in the house (searches 9-20).

**A sixteenth off the start.** Of the home log's 17 plans, 3 have a second point one sixteenth
from the start towards the third (ticks 496, 2037 and 4921): exactly the plans made just after a
walk ran on into state 5, from its end. What makes the planner add it is not yet read (the
walker's data 9 alone does not: the plan at 520 has it set and no such point). Ruled out too: the
walker off the plan's start (520 and 1900 are, without the point), the walker in state 5 (520 is,
without it), the walker still moving in state 5 (1900 is, without it; 496 is not, with it), and
the walker's place or state-5 point rounded (4921's x is the third point's way, not the walker's).
The point is start + the sign of (third − start) on each axis in all 3. The search's point test,
fn_8024C408 (called four times by fn_8024C5DC), asks of a rectangle vector whether the 1 by 1
rectangle at a point overlaps a blocking entry (cost 0x7FFFFFFF, through fn_80268D78 at 8024C448);
but it is never called in the home run (a study hook on it printed nothing), so the nudge
comes from elsewhere in fn_8024C5DC; neither the logged obstacles (`X`) nor the search's own
rectangles (`D`) block the three nudged starts anyway.

**Where a walk ends, at a door.** The walk ends at the request's chosen goal, the plan's last leg
past it (into what the Sim faces) not walked; but at a door the home log has both: the yard's walk
to the front door (tick 260, its end in the house) stops at the door's near side, arrived (state
3), where the walk to the back door (tick 1901, its end through the door) walks on into the door
at full speed and on through it (tick 2038's crossing starts at its top). A rule by the rooms of
start, goal and end (a crossing walked, the rest stopped) gets those two and tick 2's paper girl
right but misses the starts of the crossings that follow (1,257 of 1,465 frames run free, against
1,270 with the rule kept, which walks a last leg whose end is in the goal's room); what tells the
two door walks apart is the door's own state machine (the follower's states 4 and 5, its fast
mode at the yard's door), still to read.

**The turn at a walk's end** (`routing::turn_frames`). Arrived (state 3), a walker turns to face
what it walked to, 3 degrees a frame, and is done (state 4) a frame after: ⌊angle / 3°⌋ + 1 frames
(90 degrees 31 frames, 180 61, 27.6 10, 62.3 21; straight on, 1). The angle from its last step's
way to the plan's last point past its end. Checked on the home log: all 13 walk ends that turn
(the ends a new plan follows within 5 ticks set aside: a door's crossing walks on, as at tick
2035) take the original's frames exactly. Then (`Z`) states 4 and 5 are the door's: 5 moves the
walker straight to a point it keeps at +116/+120 (3 into the door's opening at tick 496).

**A whole walk's length.** What a routing primitive waits on: from the walk's start (state 1, its
speed the one carried) natively along its line to its end, then the turn, against the original's
frames from state 1 to state 4. Of the home log's 10 walks that end in a turn: 5 exactly (the house's
short walks to the counter, the stove, the paper girl); the two long walks in the yard (ticks 7
and 304) go partly in the fast mode, which is not native (when it comes on: the door, and off the
screen it may be); one (tick 1931) is cut short (its state 4 far later); two are off by a frame and
by six (the speed they start with).

**The fast mode is the game's speed.** The follower's factor (+124) changes for every walker at
the same frame: 0 at tick 23 and back to 1 at 85, 5 at 426 and back to 1 at 567, 0 at 2048 and 1
at 4912 (`Z`): the player's speed (paused, normal, the fastest), from the replay's input, not the
walker's. With it as an input, each frame's: 8 of the 10 whole walks take the original's frames
exactly (the yard's long walks too, and tick 1931's, paused midway rather than cut short); the
two left start at a speed a little off (ticks 1380 and 1619).

**Frames and ticks** (`routing::FrameClock`, `TICK_TIME`). A simulation tick holds one walker
frame or two, in a steady pattern (three ticks of two in every eleven: 14 frames in 11 ticks,
0.042 / 0.033), paused or not: the frames run on a clock of 0.033 s a frame and a tick each time
it has passed another 0.042 s. Natively from the first tick a walker moves in, its start phase
fitted once (the log begins in play), over the 3,835 ticks the most moving walker moves in:
3,736 hold its frames exactly (97%); the rest drift by a frame (the 0.042 measured, not yet found
in the code).

**A routing primitive's return.** GotoRoutingSlot and GotoRelativePosition yield (result 1) every
tick from their plan until they return. Their walker done (state 4), the follower runs one frame
more (4 again, or 5 at a door) and is let go; the primitive returns on the tick after that frame's
tick: two ticks after the walker is done when the frame falls in the next tick, one when it falls
in the same (its second frame). Checked on the home log: all 16 runs of the routing primitives
return on that tick. With the walk's frames (the step, the turn) and the frame clock, a routing
primitive's yields are native up to its walker's start.

**A routing primitive natively.** From its walker's start: the walk natively (the step, at the
game's speed as logged), the turn, the frame after done, the frames through the walker's own
ticks (the frame clock as it ran), and the return the tick after: of the home log's 10 runs whose
walk ends in a turn, 8 return on the original's tick, every yield before it with them; the 2 left
are the two walks whose start speed is a little off.

**Which objects block** (`routing::obstacle`). Read from the objects the loop asks about (`J`)
against the object data the save gives: an object is left out when its flags (object data 8)
have 0x4 (no extent: rugs, windows, wall lamps, paintings, the alarms, the phone) or it sits in
or on another (object data 2, its container: the table lamps and plants, the sink in its
counter, the meal on the counter); one whose flags have 0x10 may be walked through, its
footprint at cost 10, not grown (the doors, the trees' quarters, the newspaper); any other
blocks, its footprint grown by 3. Checked on the home log: the rule decides all 858 of its
questions about objects not persons (390 left out, 468 kept), and the cost all 98 objects kept
(47 at 10). No definition field does (the earlier search, among OBJD fields, could not find it:
the flags are the object's own). A person is decided otherwise (the loop's kind-2 branch: the
request's +152 and a distance; 8 of 16 left out), as the log has it for now. The obstacles
natively, in the loop's order, against the 12 logged lists of as many as it kept: 441
rectangles exactly the planner's, 7 of objects not native, 34 the footprints not yet found (the
chairs, the tron, the trunk).

**The one by the Sim** (`routing::overlaps`). Before it grows a footprint, the loop intersects
it with the planning Sim's own (`fn_80248DD8` with the rectangle at its +8, the Sim's +696
footprint: 6 on a side round its place) and drops the object if they share any area. Natively:
the Sim the person nearest the plan's start (its logged place, within a tile). Checked on all 17
lists of the home log: the lengths now the planner's in 16 (the 5 that dropped one, as it did);
the 17th (tick 1220) drops the paper girl, who is not in the native world. 623 rectangles
exactly the planner's, 9 of objects not native, 78 not (44 the known footprints, 34 tick 1220's
shifted by one).

## Still open

- What wakes a sleeping stack early (23 of 2,833 sleeps).
- A stack that stops (an error): what the original does with the object.
- The frames the engine pushes on a Sim's stack: its interaction queue.
