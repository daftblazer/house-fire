# Behaviour script primitives

A BHAV node's opcode below 256 is a primitive; 256 and up call another tree (256-4095
global, 4096-8191 the object's own, 8192 and up its semi-global file). The tables are in
`tools/primitives.py`: the names (the console engine's own, `cXObjectImpl::Try<Name>` and
`cXPersonImpl::Try<Name>` in The Sims 2's map) and, per game, the opcodes its interpreter
accepts, read from its executable by `tools/dispatch.py`.

## One numbering

The four games with scripts (The Sims keeps its objects another way, `survey.md`) number
the primitives alike: wherever a game's handler matches a named one by its code, it is at
the same opcode. Each game adds its own at the top:

| | Object switch | Person switch | Accepted | Highest |
|---|---|---|---|---|
| Bustin' Out | `8021242C` | `802246B0` | 55 | 57 (person) |
| The Urbz | `80165964` | `80180E1C` | 63 | 61, and person 100 |
| The Sims 2 | `80109770` | `80124ADC` | 69 | 67, and person 100 |
| Pets | `800EC050` | `80105D48` | 68 | 70, and person 71 |

Opcode 57 is the one known exception to one-name-per-opcode: a person primitive in Bustin'
Out, `HUDCreate` in The Urbz. The Sims 2 adds goals (56), the HUD (58-60), particles (61),
social mode (62), memories (63), idle animation (64), passive influence (65), wants and
fears (66, 67); Pets 68-71, not named yet.

## What the scripts use (`tools/bhav_text.py`)

Every script on the four discs checked: node targets are nodes of their tree or a return
(253 error, 254 true, 255 false), every primitive used is one its game accepts (one dead
script in Pets names the Game Boy link, 54, which Pets dropped), and every call to an
object's own tree finds it (four from semi-global trees, which resolve in the calling
object).

| | Trees | Primitive nodes | Expression | Animate | Calls (global / own / semi-global) |
|---|---|---|---|---|---|
| Bustin' Out | 9,529 | 96,505 | 72% | 5% | 6,060 / 7,797 / 4,554 |
| The Urbz | 4,907 | 51,227 | 68% | 9% | 5,602 / 2,129 / 4,108 |
| The Sims 2 | 5,148 | 59,289 | 69% | 10% | 6,440 / 1,551 / 4,349 |
| Pets | 5,272 | 61,606 | 70% | 10% | 7,321 / 1,898 / 3,878 |

So the expression primitive (2) is most of what the interpreter does; then animation (44),
change suit (6), sounds (23), set-to-next (31), dialogs (36), random (8). A few opcodes are
accepted and never used by any script (Bustin' Out: 53, 57).

## Idle (opcode 0)

The PC's Sleep. The operand's first 16 bits are a parameter's index (the rest 0xFFFF). The
stack sleeps for as many of the tick's passes as that parameter holds (`simulation.md`): the
step yields, the next N − 1 passes that reach the object pass over it, and the N-th steps it.
Then the parameter is 0 and the node goes to its true target. A parameter of 0 or less: true
at once.

Measured on the recompiled game:
- In a session of the new-game route, 11,950 of 12,257 wake-ups come exactly N ticks after
  the yield; 307 later, by a few lengths (227, 101, 168, 17, 269 ticks).
- With the pass's cursor logged, all 2,810 regular sleeps of a session are passed over by
  exactly N − 1 passes, the late ones included: the passes in between stopped at a dialog and
  did not reach the object. 23 others ended early, woken by something else.

Who may sleep, read from the original's fn_80210024 (Idle) and fn_80204594: an object that is
not a person, whose lockout (object data 25) is 0, and that has no animation playing on it. Any
other stack takes the stepped form: each step yields with the parameter one less (1800, 1799,
...), true when it reaches 0. This is the second form seen on the logs (the global tree 0x118's
1800, while a person's or an animating object's stack waits); natively both forms are the
original's (`primitives::idle`, `may_sleep`).

The lockout (object data 25, `world::DATA_LOCKOUT`) counts down by one at the end of each turn
the object ran (`Simulator::end_turn`). A sleep ends early when:
- an expression reads or writes data 25 of me or of the stack object (Castaway's
  `cXObject::InterpValue`): the object is woken, the sleep's parameter set to the passes that
  were left, and the Idle goes on from there (`Wake::Lockout`);
- NotifyStackObject (opcode 49, fn_80211F34) names it: at an Idle the parameter is 0 and the
  sleep ends (true when next stepped); at an IdleForInput (17) its count is 0 (`Wake::Ended`).
  No stack object: the script error 21.
The wakes are applied after the waking turn (`Simulator::apply_wakes`).

A sleep belongs to its Idle's frame. A tree the engine runs on a sleeping object (DropOnto's
placement tree on the counter 54 at 1455 and the table 47 at 5083; 251's at ticks 2, 260, 497,
4922) is pushed over that frame and stepped; the sleep goes on under it (`Interpreter::asleep`,
the stack at the Idle's depth). When an earlier turn of the pass ran it (the table under Sim
12's drop), the pass reaching it that tick does not count it: the table's wake at 5084 has
426 left, one more than a counted pass. (Seen once; the reverse order, the counter before Sim
237, counts as usual.) With these every Idle parameter on both full logs is the original's.

## LookTowards (opcode 22): a turn on the walker, not native yet

The person switch's case 22 is `fn_8021F15C`. All 240 uses in the full sessions (51 home, 189
club: the dance floor's 4126 and 4119, 4106, the person template's 8302) have operand
`02 00 ...`, and 229 of them are false. Operand bytes 0-1 are the mode: 0 hands the stack object
to the head's look (the router at person +0x470, vtable +0xA8; true); 1 turns by the Sim's own
direction; 2 turns towards the stack object (none: error 21); 3 turns away from it; others are
error 39. Modes 1 to 3, on the first call (frame +8 clear), check a person field at +8
(vtable +0x110, `fn_8022A324`, the field is 0 in every probed tick: Castaway's place here is
`IsIdling`) and, when it is set, return false. Otherwise they write a two-point route into the
walker's waypoint vector (+0x474: my place and the target place, both from object +0xC8), start
it (+0x60) and set frame +8. Every call, the first included, then takes the walker's result
(+0x68): nothing means true; a yield, or another result while +0x70 says keep waiting, means
yield; otherwise that result. No person or object data field separates the true results from
the false ones, so the answer is the walker's, and the case belongs with the native walks
(`simulation.md`, "Routing"). The probe reached it through the observer's person struct
(object −0x640; its +4 is object −0x44, a base whose vtable is 802CBF48).

## Relationship2 (opcode 26): one value of a relationship

Native for every use (`primitives::relationship`; 107 on home-full, 146 on club-full, 0
different), read from the original's `fn_802089D8`, with Castaway's `Relationship2Param`
getters naming the flags. The operand:

- byte 0: the value's field (the relationship is a list of 32-bit values);
- byte 1: who to whom: 0 me to the stack object, 1 the stack object to me, 2 the stack object
  to the object in the local byte 3 names, 3 that object to the stack object (a local index
  past the frame's: error 51);
- byte 2: flags. Bit 0 set: do not create. Bit 1: the ids are neighbours' (NBRS ids; I am
  then my own neighbour id, person data 31). Bit 2: write. Bit 3: pet ownership (not used by
  the discs' forms seen);
- byte 4: the variable's scope, bytes 6-7 its datum.

Without bit 1 both are objects by id (none: error 21). If both are Sims (the object type 2 test,
vtable +0x368) the relationship is their neighbours' (fn_801FD05C's person data, the
neighbourhood manager's +0x118 lookup), else the first object's own (`Object::relationships`,
the saved RelMatrix, `lots.md`) with the second's id. If the list is too short for the field:
not creating, false; creating, the list is made (zeros) up to the field. Then reading puts the
value into the variable, writing puts the variable into the value, and the step is true. A
write does not create: the group talk's exit (global 403, `01 01 05 ...`) writes field 1 of a
relationship that is not there, and the next read of it (global 401) is false. That step's
two ends are the same node, so the log cannot tell its result; the replay counts such a step
as the same either way.

## CallNamedTree (opcode 28)

Calls a tree by name. The operand (Castaway's `CallNamedTreeParam`): bytes 0-1 a string
table (STR#), bytes 2-3 which file's names (1 the global file, 2 the semi-global, 3 the
object's own; otherwise the calling tree's own kind), byte 4 the string's index. The tree with
that name, in that file, is called on the stack object (`cXObject::TryCallNamedTree`). No
such string, or no tree of that name: false.

The index counts from 1, and the name is looked up in the **stack object's** files (its own,
its semi-global, the global file), not the one the strings are from: Global 430 ("PS2 -
Story - Am I Dudley?") asks for Global's STR# 303 string 57, "CT - Story - Am I Dudley?",
which the log runs as ScenarioGlobals 8251, the semi-global of its stack object (Mom's
house's scenario controller). The call is a Gosub: the tree pushed in that file, its return
taking the node's true or false way.

Measured with `replay`: at Mom's house 850 calls made natively and 10 uses with no tree the
same (2 different, the log's taken); the club's 116 uses, all without a tree, the same as
before (a name the stack object's files lack).

The engine has it as `call_named_tree()`; the library keeps each file's string tables and its
trees by name.

## PlaySound (23), Update (7), KillSounds (48)

Requests to the presentation: a sound to play, the object's picture to refresh, sounds to stop.
For the scripts they are true and nothing else: in the session all 270 uses (100, 168 and
2) are true, never yield and never change their frame. The engine queues each on the world
(`World::presentation`: the primitive, the object, its operand) for the front ends to take,
and goes on; the simulation never waits on them, so its run is the same with or without a
front end. What each operand asks (which sound, which refresh) is the front ends' to read.

The engine has them as `present()`.

## SetMotiveDelta (opcode 29) and the motives: not checkable in the logs yet

The operand: byte 0 the scope of the change, byte 1 the scope of its limit, byte 2 the
motive, bytes 4-5 and 6-7 the two data. In the session its 12 uses are the dance floor's
dance (tree 4107) on four Sims, three each: motive 15 by 45 towards 75, motive 7 by 15
towards -20, motive 14 by 25 towards 10 (all constants, scope 26).

The motives themselves hardly move in the logs: of 47 motive variables read (2,829 reads in
the 5,130-tick session), one changes (a Sim's motive 15: 6, 12, 20 at ticks 696, 1,790 and
3,117), and none in the 719-tick one. That is too little to check a motive system against;
a session with the household at home, motives running, is the one to log for it.

## CompleteGoal (opcode 56): the goal system's

Asks whether the player has met a story goal. Read from the original's handler, fn_80211338
(the case of opcode 56 in the primitive switch fn_8021242C, 80213424). Castaway's
`CompleteGoalParam` (its ELF's symbols) names the fields, but the layout is Bustin' Out's own:
- bytes 0-1: the house's datum, byte 4 its scope;
- bytes 2-3: the goal's datum, byte 5 its scope;
- byte 6: flags. Bit 0 is the old form.

Every use in the logs is the old form, the house and goal constants. The old form:
1. In the original's game modes 1 and 2 (+0x468 of the object at 8037BFA8) it answers true at
   once. The mode is 0 in every session logged (probed).
2. It reads the house and the goal (fn_80214110, InterpValue).
3. It takes `SimsUI`'s `HouseData` table from quick data (resource 0xA173A1EE).
   - The house's row: word 8 points at its goals, 12-byte records with their count in the word
     before.
   - A record's third word points at the goal's definition. The 16-bit value at +6 of the
     definition is its mark: bit (mark & 15) of progress word (mark >> 4).
   - No such house is script error 70; no such goal, 71.
4. The answer is that bit of the goal manager's progress words (the singleton at 8037D948,
   fn_801D9F54; vtable 802C7CB0 +0xB8, fn_801E0FB8; the words at its +0x2E8; a second set at
   +0x336 is the new form's).

The new form (bit 0 clear) asks the goal manager more, can open a dialog and wait. It has never
been seen in a log, and the replay takes the log's answer for it.

Where the progress comes from: the goal manager's stream, fn_801DB408. It is the
neighbourhood's NGBH chunk, little-endian as the disc's formats, after the chunk's header (0, a
version, "HBGN"). It holds:
- first a 16-bit array at +0x168 (16, 64, 128 or 192 words by the version);
- then the marks at +0x2E8 (none before version 0x4B, 34 words at it, 39 after; the save's
  version is 0x50).

The user's save of Mom's house (house 1) has one mark met, page 2 bit 1: house 1's goal 0. A
probe of the running game (`BUSTIN_TREE_PROBE='8037D948>2E8:64;...'`) shows the same words at
tick 0 on both sessions; the disc's save has none.

The engine has it as `goals::Goals` (`quickdata::house_goals`, `save::read_goal_marks`) and
`complete_goal()` (`engine/src/primitives.rs`). Measured with `replay`, every use the log's:
- home: 20,153 (5,756 true);
- club: 2,772;
- tree: 2,377; tree3: 541.

## IdleForInput (opcode 17): a Sim's wait

Where a Sim waits in "Wait For Notify" (Global 281). Read from the original's handler,
fn_80226378 (the person switch fn_802246B0's case, 80224830). The operand: bytes 0-1 a
parameter's number (the wait's count), bytes 2-3 whether the wait may deliver.

1. A parameter the frame does not have is script error 8.
2. It asks the Sim (a vtable +0x240 call). Castaway's same place is `cXPerson::ShouldInterrupt`:
   the current action cancelled, a pending one of higher priority, a low motive, or something
   queued.
3. If that is no: a count in the parameter. Above zero, it goes down by one and the node waits
   (the log's result 1). At zero, true.
4. If yes: a wait whose bytes 2-3 are 0 is true at once. Otherwise, with nothing queued, true;
   with an entry queued, the target's checks and then the entry's tree (`idle_for_input`,
   simulation.md "Where a Sim's interactions come in").

Every use logged fits the count for the waits of bytes 2-3 = 0, also where the engine has
entries queued (the dance floor's 4104 on the club's dancers). So the +0x240 test answered no
there; it is not read yet. The engine has the count as `idle_for_input_waiting()`
(`engine/src/primitives.rs`); bit 0x40 of +0x36, which the original sets and clears with it, is
not modelled (nothing logged reads it). Measured with `replay`, every use the log's:
- club: 286 waits a tick more, 514 true, 13 deliveries;
- home: 45 true, 4 deliveries;
- tree: 230, 442, 11; tree3: 50, 86, 8.

## Random (opcode 8)

A random number into a variable. The operand, as 16-bit little-endian fields: bytes 0-1 the
destination's datum, byte 2 its scope; bytes 4-5 the range's datum, byte 6 its scope (a
literal, 7, in every logged use). The destination gets a number from 0 up to the range, and
the node goes to its true target.

Measured with `replay`: all 339 uses (269 and 70 in two sessions) true natively; of those
whose destination the log shows (a parameter or a local), all 294 numbers within their
range. The numbers themselves are the engine's own, a deliberate difference (`plan.md`); the
replay takes the original's. For a temp or attribute destination, which the step records do not carry, replay takes
the number from that variable's first read by the same stack after the step: 146, 28 and 17
of the three logs, all within their range (the aquarium's "tweak dirt inc", Random into temp 0
over a constant range, most of the home log's); temps differing home 291 → 158, tree 238 → 230,
tree3 24 → 17, attributes 14 → 12, 13 → 3, 8 → 2. A range of 0 or less gives 0 (not seen in the logs).

The engine has it as `random()` (`engine/src/primitives.rs`).

## TestObjectType (opcode 32)

Whether an object is of a type. The operand: a GUID (bytes 0-3, in the file's byte order),
then a variable that holds the object's id (bytes 4-5 the datum, byte 6 the scope). True if
the object with that id is of that GUID; false otherwise, and for an id no object has.

Measured with `replay` on the 5,130-tick session: all 181 uses the same natively. All of them
are false there (tests of the Sims against one GUID, of one object against another), so the
true case is checked only by `engine/tests/primitives.rs` so far.

The engine has it as `test_object_type()` (`engine/src/primitives.rs`).

## Animate (opcode 44): what the scripts see

Measured on the recompiled game, a session of the new-game route: 23,811 steps, 628 runs of
102 animations.

- **While the animation plays, the step yields once every tick** (23,183 yields).
- **A run ends true** (526) **or false** (102).
- **Flag 0x20 in operand byte 5:** an animation event ends the step false with the event in
  local 0 (74 runs). The script handles the event and calls Animate again.
- **The flags byte** is 0 (12,909 steps), 0x20 (10,447) or 0x22 (455).
- **Operand bytes 0-1** are the animation.
- **A run lasts as long as its animation:** 44 of the 102 animations run the same number of
  ticks every time (12, 13, 16, 20, 24, 28 ... 92). The others vary, as their events and
  continuations would explain.

This is the contract the animation system must keep (each animation's length in ticks and
its events, from the animation data, which is not decoded yet).

Where the animations are (the fifth loop): the object files' "animations" string tables
(STR# 128) are empty on the consoles. An Animate operand's first 16 bits index the quick
data's `AnimRef` table (`SimsObjects`: 2,891 rows of 12 bytes, a 32-bit id, a 32-bit value,
an empty name), which `AnimRefName` (2,840 pointers into it) also reaches. The animations
themselves are `animatio.arc`'s 4,013 entries, by name ("a2a-tickler-stop-laugh" ...). Which
entry an AnimRef id names (found in iteration 113 of the twelfth loop, once the archive index was
read right in iteration 38): the id is the `animatio.arc` entry's own id, for all 2,891 rows
(The Sims 2's `SAnimator2::loadSkillAnim` passes it to `EResourceManager::IsLoaded`). The
earlier search, on the index read one entry off, found it not the archive's own name hash, nor
CRC-32, FNV-1/1a, djb, sdbm, ELF or a ×31/37/131/65599 hash of the name (as is, lower or
upper case, with or without an extension), and the row's value does not follow the run's
length (0xA6A for 16 ticks, 0x514 for 17). Castaway's ELF has `EAnimManager` (resources by
32-bit id) to read it from. How an operand names its animation (iteration 114, The Sims 2's
`SAnimator2::resolveSkillForPrimitive(StackElem *, AnimateNewParam *, AnimRef *&)` and the
`Get*AnimRef` it calls, disassembled from its ELF):
- bytes 0-1 (s16) are an index, taken from a parameter instead when byte 5 has 4 set;
- byte 4 is the source: 0 the object's anim table (`GetObjectAnimRef`: the ObjSelector's adult,
  child, adult-to-child or child-to-adult table, by the Sim's age and the target's), 1 a global
  table (`GetGlobalAnimRef`), 2 the person's stock table (`GetPersonStockAnimRef`), 3 a misc one
  (`GetMiscAnimRef`).
An object's anim tables are its STR# 129 (`a2o`) and 130 (`c2o`): strings of the original
`.max` paths (`..\..\Characters\ANIMATIONS\ConvertedFromMaxis\a2o-fridge-open.max`; string
0 a note). An entry's base name is an `animatio.arc` entry's name. Through the tree's file's
STR# 129, home's Animate steps from source 0 name 2,363 animations, all of them entries
(fridge, stove, eating, easel). 751 are not named that way: index by parameter, a child table,
or the table of another file than the tree's. Source 3's 913 and source 2's 1 are not read yet
(lockstep `LOCKSTEP_ANIM_RUNS`). On the club, tree and tree3 logs every source-0 step is named (club 18,528).
A run's ticks follow the animation's length, not the frames: the original's ticks a frame vary
(home about 0.5 a camera record, the club 1.3; `assets frameticks`), while the runs that always
last the same take about 24 ticks a second on every log (iteration 115):

| Animation | Seconds | Ticks |
|---|---|---|
| a2o-stopeat-stand | 0.450 | 12 |
| a2o-starteat-stand | 0.483 | 13 |
| a2o-easel-startpaint | 0.500 | 13 |
| a2o_bar_enter_bar | 0.683 | 17 |
| a2o-fridge-look | 1.933 | 46 |
| a2o_dj_start_spinning | 2.883 | 69 |
| a2o_dance_floor_dance_outgoing01 | 6.350 | 151 |

The long ones fit 23.8 ticks a second; the short ones run a tick or two over. The rule (iteration 116): the animation runs by the frame, not the tick. EA's
`EAnimController::UpdateTrack` (The Sims 2's map) is Bustin' Out's `fn_8015708C` (94% of its
instructions' shapes alike, `work/match/fnmatch.py`, not in git). Once a frame (skipped when the
track's +52 is the frame counter at r13 - 29732), it moves the track's place (+60, in frames) by
the frame's time (r13 - 29720, 0.033 s, the walkers' step) times the controller's +80, the
track's +72 and its +0 (the rate), and the track is done past its last frame. So an animation
takes the frames for (frames - 1) / (0.033 x rate), and the frames map to ticks by the frame clock
(`routing::FrameClock`: 0.042 s a tick). Lockstep's `LOCKSTEP_ANIM_RUNS` sets every run of one
length against the ticks those frames span at any phase of the clock. Of the nine that take
ticks, seven fall in the span, the long ones exactly: a2o_dj_start_spinning 87 frames, 68 or
69 ticks, logged 69; a2o_dance_floor_dance_outgoing01 192 frames, 151 or 152, logged 151. Two
15-frame starts (a2o-easel-startpaint, a2o-starteat-stand) take 13, a tick over. The runs of 0
ticks end at once.
The original's own clock (iteration 117): the observer's '8' record, at each call of
`fn_8015708C` (named `Anim_UpdateTrack` in the recomp), gives the controller, the track and its
animation (at 0x7E... addresses the hook cannot read), the track's place before and after, its
speeds, the frame counter and the frame its play started (+52), and +160 (set once the place
passes the last frame). On a new home log (home-a8.log, 562,616 steps like the kept one; 532,941
track steps of 58 controllers): the place grows 0.990 or 1.980 a frame (the speeds 1 on nearly
all), and the ticks hold 1 or 2 frames (4,929 and 1,835, the frame clock's 1.27). Each Sim's
controller has two tracks, an object's one. Matching each Animate run to the play (a track's steps
from one start frame) of its animation's rate starting within two ticks (lockstep
`LOCKSTEP_ANIM_RUNS` on that log):
- a play left to run takes exactly the predicted frames (60, 23, 58, 122, 15, 14, 31, 182: 21
  plays whole);
- its first frame is in the run's first tick or the next (where that tick's frames fall);
- a run that lasts to its animation's end returns on the tick after the track's last frame (5
  of 5). The rest end earlier, on an event (flag 0x20), with the track still going.
So the two 15-frame starts' extra tick is that last tick plus the start: from the first tick, the
frames by the clock, and one more.
Where a play starts (iteration 118; each '8' record keeps its place among the steps). 15 of the
21 whole plays start on the first frame logged after the run's first Animate step. The other 6
skip that frame and start on the first frame of the next tick (1483 against 1484, 2613 against
2615). The operand does not tell them apart: eatloop's same operand is late once, on time three
times. All 6 late starts are an animation's first use on the log, and no repeated use is late
(8 of 8). But 7 first uses start on time (the fridge's, the reaches, the approach), which looks
like an animation loaded ahead of its step (`SAnimator2::loadSkillAnim`, `PreloadEvents`)
against one loaded at it.
That is so (iteration 119). Bustin' Out's `fn_80177B24` is EA's `EResourceManager::IsLoaded(
unsigned int)` (identical in shape), and The Sims 2's `TryAnimate` (Bustin' Out's `fn_800AC854`,
0.65; its `resolveSkillForPrimitive` `fn_800AC5B4`, 0.85) asks it from 800AC7E4. The observer's
'9' record logs each ask (the id, the answer, the caller). On a new home log, home-a9.log (562,616
steps), every Animate step's first ask names its animation's `animatio.arc` id. Of the 21 whole
plays, the 6 late starts are exactly those whose animation was not loaded at the run's first step
and was at the next tick's. The 15 on time were loaded at their first step, the 7 first uses
included. So a track starts on the first frame after the Animate step that finds its animation
loaded. What loads them (iteration 120): the observer's '0' record logs EA's
`EResourceManager::AddRefAsync(unsigned int)` (Bustin' Out `fn_801776C0`, by shape 0.957) and
`GetRef(unsigned int)` (`fn_801770C0`, 0.979) with their callers. On a new home log, home-a10.log
(562,616 steps): each late start's first step asks for its animation in the background (from
800AC820, in TryAnimate) and finds it loaded at the next tick's step (6 of 6). Later uses take the
loaded copy (GetRef from 800AC89C at the tick after). The 7 on-time first uses are never asked for
on the log: they were in memory when it began (the log starts in play, after the lot loaded). So
natively: an animation not yet loaded asks for its load at its first step and starts a tick later;
which are loaded when a lot comes up (and whether a load ever takes longer than a tick, as a big
one might from the disc) is the rest.
Which are loaded when the lot comes up (iteration 121): the disc's `datasets.arc` (486 entries)
holds sets of resources by name: `globals`, `createasim`, `House1Preload`, `housepre01` to
`housepre15` and more (`assets ISO idwhere NAME...` lists the sets that hold an animation's id,
unaligned). Home's lot has `House1Preload` and `housepre01` in memory. Every animation's first
load ask on home-a10.log (43, `assets ISO resasks LOG` with
`ASSETS_RES_DATASET=House1Preload,housepre01`) is answered loaded exactly when the animation is
in one of the two: 28 in and loaded, 14 out and not, 42 of 43. The one left is
`am_cas_idle_loop` at tick 0, a create-a-Sim animation still in memory from before the lot. How a lot names its sets (iteration 122): the requests at tick 0 ('0', `assets ISO resasks LOG`
with `ASSETS_RES_TICK0`, named from every archive on the disc) are `globals` at boot, the
create-a-Sim data, then the lot's two sets, `House1Preload` and `housepre01`, each through
`fn_80082BB4` (an AddRefAsync of the set) and GetRef. Their ids are immediates in the code (lis/ori,
so not found as bytes). In `fn_80081...`, the lot load: at 80081F54 `House1Preload` (ECF23484)
when the global at 0x802F76D8 is 0, else `House2Preload` (627D3367; `House3Preload` AED733F9
elsewhere). At 800822F8 `housepre[n - 1]`, from a table of the 16 sets' ids in the DOL at
0x802D3380 (housepre01 to housepre16 in order), n the global at 0x802E68F8. Home's snapshots
(ticks 1, 100, 1,000) hold n = 1 and the flag 0: House1Preload and housepre01, the two the log
asks for. The club's (tick 100) holds n = 16, the flag 0. What the flag and the other branches
of the switch are (House2/3Preload) is still to read.
The engine's own (iteration 123, `anim::Loads`): the animations in memory start as those the
lot's preload sets hold (the archive's entry ids found in the sets' bytes); an ask for one not in
memory asks for its load and is answered loaded from the next tick. Lockstep
`LOCKSTEP_ANIM_LOADS` (house n from `LOCKSTEP_HOUSE`, default 1) answers every Animate load ask on
home-a10.log (TryAnimate's, 800AC7E4) in turn: 104 of 104 as the original's. With iteration
119's rule (a track starts on the first frame after the step that finds its animation loaded),
that gives each whole play's start.
A run's return natively (iteration 124, lockstep `LOCKSTEP_ANIM_NATIVE` on home-a10.log):
1. the run's first step whose ask finds the animation loaded (`anim::Loads`);
2. the first frame after it;
3. the frames to the last, (frames - 1) / (0.033 × rate) + 1, one more when operand byte 5 has
   0x02 (played backward: the controller's speed -1, from the last frame, the first frame
   staying put);
4. the tick after that frame's.
Of home's 166 runs, the 7 that last to their animation's end return on that tick, all 7 (two
backward approaches among them). The other 159 end before it, on an event (flag 0x20) or an
interruption: next, from the animations' events. The frames' ticks are still the log's (the
frame clock natively is the walkers' fitted phase).
The early ends (iteration 125, `LOCKSTEP_ANIM_EARLY`: each early run's frames played, its
branch and local 0 after). They repeat: `a2o-easel-paint` (flag 0x20) ends false with local 0
101 after 7, 20, 14, 37, 31, 17 and 14 frames, over and over (the script calls it again to go
on), and with 102 after 15, 12, 7. `a2o-eatloop-stand` ends after 16 or 17 frames (101), the
fridge's with 62, the stove's with 198, the reaches with 16 or 0. So events at fixed frames of
the play. They are not the animation's: EA's `EAnimController::ProcessEvents` (Bustin' Out
`fn_80156E48`, 0.912; `AddEventRange` `fn_8015701C`, 0.982) reads the animation's event array
(+144, +148 the count, 80 bytes each, the frame at +68), and that array is empty both in the
archive (0 on all 4,013) and in memory (home's tick-1,000 snapshot of the 0x7E... region:
a2o-fridge-getfood's struct, 121 frames, 0 events). The Sims 2's `SAnimator2::processEvents(
AnimRef &, ...)` takes the AnimRef, so the events are the AnimRef row's.
They are (iteration 126). The AnimRef row (12 bytes) is the animation's id, its length in ms
(0x176F, 5,999, for a2o-easel-paint's 361 frames at 60; shared by paired animations of one
length) and a pointer to its notes, with their count in the word before it (`assets ISO
animnotes NAME...`). A note is three words: its time in ms, a type (6 on all seen) and its
value, the event the script sees. a2o-easel-paint's 10: 101 at 200, 833 and 1,333 ms, 102 at
1,866, 2,233 and 2,500, 101 at 3,733, 4,766, 5,333 and 5,833: the order of its early ends
(three 101s, three 102s, four 101s, at 7, 20, 14, 15, 12, 7, 37, 31, 17 and 14 frames, about the
gaps' 200, 633, 500, 533, 367, 267, 1,233, 1,033, 567 and 500 ms at 33 ms a frame).
a2o-eatloop-stand's one: 101 at 500 ms, its ends after 16 or 17 frames. a2o-stove-cook has 8
(101 to 108), a2o-reach-tableht 2 (100 at 866, 0 at 900), a2o-fridge-getfood 1 (0 at 1,500). The
Sims 2 fires a type-5 note 150 ms early. The note file's layout follows The Sims 2's
`EAnimNote`, but Bustin' Out keeps it in the quick data, not in the animation (whose note array
`ERAnim::Load` would copy is absent from `OldLoad`'s layout, which these files use). Next, each
early end's frame exactly against its notes, from the tracks' places ('8').
An event end natively (iteration 127, lockstep `LOCKSTEP_ANIM_NOTES` on home-a10.log). A run
with operand byte 5's 0x20 returns on the tick after the frame whose place passes its play's next
note (the note's ms × the rate / 1000, in frames). Forward the place runs 0 to the last frame,
backward (0x02) the other way after a still first frame. A later Animate of the animation the Sim
is still playing (left at a note, not at its end) goes on with that play and waits for the note
after, whatever came between. Otherwise it starts a new play (the load, the first frame after
the step that finds it loaded). Of the 60 note ends, 59 return on that tick. The one left is
a2o-stove-cook's last note (6,100 ms, 366 frames, near its 418-frame end), two ticks later. Of the
runs left to their end, 21 return on the native tick, 1 later, and 70 end earlier with no note
due (an interruption, or other flags: the fridge's 0x11 ends with 62 in local 0). 14 end on
their first tick.
Iteration 128: the flags do not choose the notes; the note type does. Every type-6 note ends a
run, whatever byte 5 holds. a2o-counter-foodprep's 12 notes (value 100) end its 0x00 runs, and
a2o-fridge-open/close's notes end their 0x00 and 0x11 runs. Types 1 (a2o-approach: −1, 1, −2, 2,
its steps) and 5 (a2o-flamingo-kick: pointers, sounds) end nothing. With that, of 165 runs, 92 end
at a note on the native tick (7 later, 22 earlier), 27 run to the end on it (1 later, 3 earlier),
and 2 of 14 that end at once. The misses gather in a2o-counter-foodprep's second half (notes
3 to 11, predicted about 110 ticks late: its play there seems to start at another frame) and in
a2o-stove-cook's runs that end at once (tick 1096).
Iteration 129: those food-prep misses are fast-forward. From tick 426 to 563 of home-a10.log the
game runs at speed 5: a frame comes every ~4 ticks (36 frames in 138 ticks), and each steps every
track's place by 5 × the usual step (the track's +72 is 5, or −5 played backward; −1 and 1 are
the normal speeds). The speed lags one frame: a frame steps at the speed its frame before set.
In The Sims 2, `SAnimator2::Update` stores the game's speed into the animator's +252 (a virtual
call on the global at r13 − 21496, when three checks pass, else 1). `TryAnimate` hands +252 to
`EAnimController::SetTrackSpeed`, negated for 0x02, and `adjustAnimationPlayRates` →
`SetAllTrackSpeed` carries changes to the tracks. With the speed, 144 of 165 runs return on the
native tick (113 at a note, 29 at the end, 2 at once). All misses are now the Sim's
a2o-stove-cook (16), from tick 1096, where 8 runs return in one tick.
Iteration 130: those 8 are a burst. The food processor's tree 0x2003 node 7 (Animate, operand
02 00 06 00 00 20 08 00) returns false 9 times in tick 1096, the tick after its play's end
(true at 1094 and 1095). Then tree 0x2006 node 2 starts the play over. The burst does not use up
the new play's notes: its 8 later runs return on the native tick, one per note from the restart.
With the 15 runs that return on their first tick left out (`LOCKSTEP_ANIM_SKIP_ONCE`), 146 of 150
return natively. The misses left are a2o-stove-cook's last note twice (366 frames, 2 ticks late)
and a2o-fridge-open/close's note 6 once each. Tried and dropped: a burst of every note whenever a
play left at its end is started again within a tick. Food prep's and the easel's restarts are
like that and have no burst (57 native instead of 113). What makes a call return at once is
still open.
Iteration 131: counted events, from The Sims 2's `SAnimator2::TryAnimate` (0x8005E520 on).
Byte 6 of the operand is how many counted events the call expects: notes of value 0 to 99, in
order (the animator's +240 counts them, from byte 6 − 1 down when played backward). A note over 99
always ends the run with its value. A counted note ends it only when it is the next one expected;
otherwise it is passed over and the run goes on. When the animation is done and fewer than byte 6
came, the call hands out the next missing one (false, its index as the value). It does that at
once on each call until all have come, then returns true. The value goes to local byte 2 with
0x20, else to parameter 0. The stove's 0x2003 node 7 asks for 8 (byte 6 = 08) but its notes are
101 to 108, so all 8 come at the end: the burst of tick 1096. With this, 158 of 166 runs return on
the native tick (119 at a note, 29 at the end, 8 owed at once, 2 at once). Left: a2o-stove-cook's
last note and its end, 2 to 3 ticks late on both plays, and a2o-flamingo-kick's 4 calls that
return at once with nothing owed.
Iteration 132: the stove's lateness is a pause. On some frames all of one Sim's tracks have
speed 0 (+72), so that Sim's animations stand still while the other Sims' move. This happens for
1 frame at tick 994, 2 at 1007 and 2 at 1208, on the ticks the other Sim starts an Animate. Each
paused frame pushes the play's later notes and end back by a frame. Stepping each play by its own
track's logged speed, one frame behind like the fast-forward (`LOCKSTEP_ANIM_TRACK_SPEED`; the
play's track is the one whose start frame +52 is the frame before the play's first), 162 of 166
runs return on the native tick. 121 return at a note, 31 at the end, 8 owed at once and 2 at once;
food prep's fast-forward comes out of the same speeds. Only a2o-flamingo-kick's 4 at-once calls
are left. The speeds are still read from the log: what pauses a Sim, and the fast-forward, are
next to make native.
Iteration 133 (`LOCKSTEP_ANIM_PAUSES`): the pauses come with Animate load misses. Of 11 ticks where
a controller's tracks all stop, 8 are ticks when an Animate asked for an animation not yet in
memory (`IsLoaded` false from 800AC7E4, then `AddRefAsync`). It is the other Sim that pauses, not
the one asking: at 994 the chips Sim asks for a2o-starteat-stand and the cook's tracks stop, at
208 the cook asks for a2o-counter-foodprep and the other Sim stops. 5 misses (ticks 18, 1418,
1618, 1632 and 1700, all logged) pause nobody, and 3 pauses (23, 2048, 5630) come with no miss
(2048 with two other loads, 80256088). New `assets animid ID...` names animation ids. Still
open: which loads pause whom (the loader's stall, a shared track or the camera's Sim).
Iteration 134, in Bustin' Out's code:
- `fn_800AB820` is its `SAnimator2::Update`. Unless global r13 − 28672 is 0, it asks the game
  object at r13 − 23196 three questions (virtual +144, +104, +152). If any answers nonzero, the
  animator's speed (+124) is 0.001. Otherwise the speed comes from that object's +72 call: 0
  gives 1.0, −1 gives 0.5, −2 gives 5.0 (the fast-forward of iteration 129), −3 gives 10.0, and
  anything else gives 0.0, a pause (constants at 0x802A8BB4).
- `fn_800ABFD8` is its `adjustAnimationPlayRates`. When +124 differs from the last speed (+128),
  a 0 saves each of the Sim's six track speeds (+1600 to +1620) and stops them. Any other value
  sets them to it, or puts the saved ones back.
- `fn_8015AB80` (controller, track id, speed) stores one track's speed (+72), found through the
  controller's +44.
So the speed is the game's, read by each Sim's Update in turn, and a pause is that game state
being "other" on a frame. A Sim updated while it holds pauses for that frame, and a Sim updated
earlier does not: the other Sim on a load miss. Next: the game state's +72 answer around a load
miss, and the Sims' update order.
Iteration 135: the observer's new '#' record logs each `Anim_SpeedUpdate` (the animator, its +8,
the speed before and after), on home-a11.log. That log is 30 ticks shorter than home-a10.log
(6,840 ticks, cut at the end), the same before that, and lockstep 0. There are three animators
(owners 80f56cf0, 80f59990 for the chips Sim 80f594bc, 80ffe850 for the cook 80ffe378); the
first stops updating after tick 465.
- All three are at 0 for ticks 23 to 84 (the lot's start) and both Sims for 2048 to 4911 and from
  5630 (other screens). All are at 5 from 426 (the first only to 465).
- Every Animate load miss pauses the Sim asking, on that tick's frames: the chips Sim at 18, 1418
  and 1618, the cook at 1700. At 208, 994, 1007 and 5013 both pause. On each of those, the other
  Sim also runs an Animate step that tick.
- The pause is not in the track records when the paused Sim's tracks were not moving (iteration
  133's "5 misses pause nobody").
Next: the exact rule (the tick's Animate steps against the miss, and the speed source's three
checks) and the speed natively.
Iteration 136, the pause rule (`LOCKSTEP_ANIM_PAUSE_RULE` on home-a11.log). On a tick with an
Animate load miss, the asking Sim and every Sim with an Animate step that tick are at speed 0
for that tick's frames. Their steps may come before or after the miss, since the speed is read
at the frame's update after the tick. Every other Sim keeps its speed. This holds on 12 of the
13 ticks with a miss or a short pause. The one left is tick 1, the lot's first, where an
object, not a Sim, asks and all are paused. No short pause comes without a miss.
Iteration 137: the notes model with that rule in place of the logged tracks' pauses
(`LOCKSTEP_ANIM_RULE_SPEED`). The misses come from the engine's `anim::Loads` on the asks in
order, and the paused Sims are the asker plus that tick's Animate Sims. It gives 162 of 166 runs
on the native tick, on home-a10.log and home-a11.log alike, the same as the tracks' own speeds.
The fast-forward is still the logged one (its frames' ticks). Only a2o-flamingo-kick's 4 at-once
calls are left.
Iteration 138: those are a name the original does not find. Global.iff's STR# 129 entry 0 is
"A2O-flamingo-kick" (capitals), and animatio.arc only has "a2o-flamingo-kick". The four Animate
steps from the global table (source 1, index 0: the fridge, the chips and the food processor
"break" calls) return at once, with no load ask ('9', '0'). The lookup is case-sensitive,
so the call fails as TryAnimate does when it cannot resolve an animation. This is a bug of the
original's data that the engine keeps: an Animate whose table name is not an entry as written
returns at once (true: iteration 143). With it, all 167 runs return on the native tick on home-a10.log and
home-a11.log: 121 at a note, 31 at the end, 8 owed at once, 5 not found, 2 at once. Still taken
from the log: the frames (the frame clock), the fast-forward frames and the tick's Animate steps
(which the engine's own run gives).
Iteration 139, the fast-forward's input. The replay (work/inputs/mom-live/inputs.txt, by pad poll
and video frame) holds R (button 0x0020, analog FF) from video frame 2104 to 2142, its only
press after 1601. The game runs at 5 from track frame 1761 to 1796 (ticks 426 to 563): 36 game
frames for R's 39 video frames, starting with the press (video frame − 343). The game loses 3
frames during the fast-forward, so it ends at video − 347. So the speed answer −2 (5×) is R held.
The engine reads its own pad on its own frames, so it needs only "R held: 5×", not this
offset. The other zero spans are not input: the lot's start (frames 1248 to 1326, ticks 23 to 84,
with the A press of video 1601 inside it) and ticks 2048 to 4911 and from 5630 (no press after
2104). They are the game's own states, still to find (a dialog, the lot's intro).
Iteration 140: those spans are the game paused, not a pause of the animations alone. The
log's ticks go on, but almost nothing runs: from 2048 to 4911, 3,159 steps (1.1 a tick, by 4
objects) against about 200 a tick before and after; from 5630, 1,438 (2 objects); from 23 to 84,
254 (3). The event messenger's checks every 12 ticks (STR# 303 18, "CT - Event - NPC Reminder -
Resident scripted event", then 10, "Goal - Reminder", false each time) stop with them. The
party controller calls "CT - Party - Override" at 2047. What pauses the game is not found from
the steps; the engine runs the log's ticks as they are. No Animate runs inside a pause, so
the animation clock needs only the speed of the ticks that run: 1, 5 with R held, and 0 for a
load miss's Sims.
Iteration 141: the model is in the engine, `anim::Play` (with `anim::notes` reading the type-6
notes from the quick data's AnimRef rows). A play is stepped frame by frame from the first
frame after the step that starts it, each frame by 0.033 s × the rate × that frame's own speed.
`poll()` gives Pending, Event(value) or Done: notes over 99, the next counted event, the owed
counted events past the end. `LOCKSTEP_ANIM_ENGINE` drives each Sim's play through it on the
log's frames: 167 of 167 runs native on home-a10.log and home-a11.log.
Iteration 142: the Sims' plays are kept by `anim::Animator` (one play a Sim, the loads, the
notes), driven in the game's order: `animate()` for each Animate step (once a tick for a run;
the runs that start on one tick, like the owed events at once, take its steps in turn) and
`frame()` for each frame where it falls among the steps, each Sim at its speed. A name not
found returns at once (true, iteration 143). Through it, 167 of 167 runs native on home-a10.log and home-a11.log.
The replay's home.log is older and has no frame records ('k'), so the replay cannot run
Animate natively on it; that needs the replay on home-a11.log's frames.
Before that, the first 16 bits were taken for the AnimRef table's row, which they are not: read
so, home's 272 Animate runs would be mostly `a2a-apologizee` and the attack loops, with lengths
that follow no animation's (lockstep `LOCKSTEP_ANIM_RUNS`). They index something per object
(the object's or the Sim's own list into AnimRef, by the operand's source byte), still to read.
Until then each animation's length cannot be checked, and Animate stays the log's. The primitive is a thin
wrapper over that system, so it waits for it. Animate steps also make the effect objects
(`objects.md`, "Objects made and removed").

## CreateObjectInstance (opcode 42): what the home log shows

Three of the home log's four made objects come from opcode 42 in a Sim's interaction (the step
whose stack object becomes the new id, `lockstep` with `LOCKSTEP_SHOW_MAKERS`):

| Tick | Made | By | Operand |
|---|---|---|---|
| 692 | 197, a bag of chips | the fridge's 0x2007 node 1 | `75 e3 01 f3 04 01 00 00` |
| 1219 | 201, a trash pile | the chips' 0x2022 node 1 | `75 70 90 7f 06 00 00 00` |
| 1765 | 202, a meal | the meal group's 0x200f node 6 | `53 f6 62 bf 04 00 00 00` |

Bytes 0-3 are the new object's GUID (little-endian: f301e375, 7f907075, bf62f653), byte 4 where
it goes (4, 6), byte 5 a flag. The new object is made inside the step: its own init runs at once
(its first steps, 0x1004 / 0x200b / 0x1003 node 0, come before the maker's), the list's head is
it ('C' during the step), and the maker's stack object is its id after. The fourth made object,
the "go here and show" marker at 1503, has no opcode-42 step: the engine's own.

Natively (`create_object`): an object of the GUID's definition and file, 72 data fields, its
definition's field 56 attributes, the lowest id free, in front of the list. Checked in lockstep
on its world of the original's lists: the home log's three and one in each club session, all
with the original's id and a GUID the file of its own later steps has; the lists after them in
the original's order. Not yet: where it goes (byte 4), its init run inside the step, and replay
(whose maker steps do not reach the primitive yet).

## RemoveObjectInstance (opcode 18)

Operand byte 0: 1 the stack object, 0 the object itself. The home log's three removals are all
it: the paper girl removing herself as she leaves (`portal_pedestrian.iff`'s 0x1004 node 3,
operand `00 00 02 00`, tick 466), the bag of chips removed by its own tree 0x2022 node 23 (the
stack object, 1382), and the marker (1767); the club's three (one and two a session) too.
Natively `remove_object`, checked in lockstep's world: all six the objects the original's next
lists lack, and with them no object is left for the lists to remove.

## DropOnto (opcode 43): the snaps

The places the tenth loop found set to a tile's centre outside any walk (the meal at 165, 877,
1455; 202 at 5083) are opcode-43 steps: a counter's 0x200f node 11, the food processor's 0x2003
node 13, the counter's 0x2018 node 15, the dining table's 0x2007 node 62, operand 0 (byte 4 1 at
the table). The dropped object is the one the actor holds (its container field, data 2, is the
actor: the meal 198 for Sim 237, from the save; 202 for Sim 12, written at run time), and it goes
to the centre of the stack object's tile on its level (the counter at (400, 336) → (408, 344),
the stove at (400, 352) → (408, 360), the table at (384, 256) → (392, 264)). Natively
`drop_onto`, checked in lockstep's world (a made object's fields 2 and 59 from the log): all 4
the held object and the place the log has next. The club's sessions have none.

The other snaps of the tenth loop's 13. Four are made objects' first places, set by
CreateObjectInstance's byte 4 (with Sim 12 the maker each time):
- 693, the chips, byte 4 = 4 and byte 5 = 1: (408, 328), the fridge's tile, the one in front of
  Sim 12 (at (392, 328), facing it);
- 1766, the meal, byte 4 = 4 and byte 5 = 0: (392, 344), Sim 12's own place (in its hand?);
- 1220, the trash pile, byte 4 = 6: (392, 296), the tile north of Sim 12's (392, 304);
- 1503, the marker: made by the engine.
Three cases do not fix byte 4's meaning (in front of the maker by its facing, in its hand,
next to it): open. The meal's move at 752 happens in the counter's Animate steps (opcode 44,
the meal its stack object): an animation event, with the animation (stage 4). 823, 1330 and
4996 are Grab and Snap:

- **Grab (opcode 4):** the stack object goes to the actor's place and is held by it (its
  container field the actor): the chips taken by Sim 12 at 823 (to (392, 328)), the meal by Sim
  237 at 1330 (to (392, 360)). Natively `grab`, checked in lockstep (the actor's place the log's
  last before the step): all 4 of the home log's grabs, the object's next logged place.
- **Snap (opcode 46), kind 0 (operand byte 0):** the actor goes to the centre of the stack
  object's tile and what it holds with it, and is then in the stack object's slot 0 (its
  container the stack object: a chair's slot 0 holds the Sim seated in it, scope 20's reads): Sim 12 onto the dining chair at 4996, with the meal
  202, both to (408, 264). Natively `snap`, the home log's one of kind 0. Kinds 5 (931, Sim 12
  onto itself), 6 (5066) and 1 (one in each club session) are not modelled.

CreateObjectInstance's byte 4 takes the values 0-9 on the disc (6 the most, 336 nodes): The
Sims' "where" (0 in front of me, 1 on top of me, 2 in my hand, 3 in front of the stack object, 4
the stack object's slot 0, 5 underneath me, 6 out of the world, 8 next to me in a local's
direction). Natively 2, 4 (the centre of the stack object's tile) and 6 ((-16, -16)). The chips
(4) start in the fridge's slot; the meal (4) in the meal group's, then Sim 12 grabs it in the same
tick; the trash pile (6) out of the world, then FindLocationFor (opcode 16) puts it at (392, 296)
in the same step: not native yet.

A made object's own init sets its flags (data 8) at once, in the tick it is made: 0, then 16
(walk-through) for the chips and the meal, read three times by its init's steps. Its container
(data 2) is first read after a Grab (12, Sim 12): nothing in the log shows what byte 4 = 4 leaves
there, so `create_object` does not set it. For replay this means a made object is an obstacle
of the right kind only once its init has run natively (flags 16, a walk-through one), not with
the flags it is made with (0, a blocking one): what iteration 18's attempt lacked.

Checked in lockstep at each tick's end, every logged place of an object that is not a person:
a held object (container field an object) is at its holder's place (272 of 272, the holder's
logged place that tick: the chips Sim 12 carries at 957-989 and 1270-1282, the tenth loop's "two
Sims moving as one", are this), a dropped one no longer held (DropOnto clears the field); the
others 5 of 6 the native place, the one left the trash pile's FindLocationFor.

So of the tenth loop's 13 snaps, 4 are DropOnto, 2 Grab, 1 Snap (and 202's with it), 3 made
objects' first places (CreateObjectInstance's byte 4, open), 1 an animation event and 1 the
engine's marker.

## The recomp's primitive dispatch

The primitives are run by fn_8021242C, a compare tree on the opcode (in r5) that calls each
handler directly: opcode 2 (the expression) fn_80213890, opcode 14 (FindBestObjectForFunction)
fn_8020CC8C (from its case at 80212B30). The other cases are read the same way, from the
compares at the function's start.

## FindBestObjectForFunction (opcode 14), read in the recomp

fn_8020CC8C. Operand byte 0 the function; each maps to a pair (a, b): 0 (0x1F, 0x12), 1 (0x20,
0x13), 2 (0x21, 0x14), 3 (0x24, 0x15), 6 (0x25, 0x18), 7 (0x26, 0x19), 8 (-1, 0x1A), 10 (0x3D,
0x0E), 11 (0x27, 0x1C), 12 (0x40, 0x10), 13 (0x41, 0x11), 14 (0x0F, 0x1D); others none. It walks
the lot's objects: one in use (its int16 at +88 above 0) or failing its method at 1176 is
skipped; its definition must support function b (a method at slot 24 with b) and gives a check
tree (slot 32 with b, 0 for none); its rating is the int16 at +38 + 2a (a = -1: 50), 0 skipping
it; the check tree, if any, runs with the object as stack object (fn_8023EF78) and a false one
skips it; the score is the rating less the distance times a factor, and the best is the result.
Measured (the home log's six): function 1 checks the stove 61 with 8199, function 7 the four
chairs with 8200 (two true, the nearer found), functions 10 and 6 no check. Named: b is the
entry of the object's function table (OBJf) — its action tree the support, its test tree the
check (the stove's entry 19: 8199 "cook test", 8197 "cook function"); the rating is the
object's data a (the stove's data 32 = 16, the tables' 38, the counters' 61, the sink's 37),
which its "update function scores" tree keeps; the distance factor 3.0 (0x8037C550), the floor
1.0; an object in use (+88 is its data 25, above 0) is left out. Replay runs it natively: all 6
of the home log's find the original's object (the second search at 1356 skips counter 55, in
use since the first).

## RunFunctionalTree (opcode 20)

Runs a function of the stack object: the operand's first byte names the function as
FindBestObjectForFunction's does (`primitives::function_entry`: the same function → function-table
entry pairs), and the object's function table (OBJf) entry's action tree is pushed with the Sim's
temps as its parameters, the stack object unchanged. The node stays while it runs; its return takes
the node's true or false way (the home log: food_sg's 8234 node 3 returns true to node 5 at 5084).
The entry's test tree is not run first (the log goes straight to the action's node 0). Measured on
the home log, 4 of 4 as the original's: 8234 node 3 (function 7, after node 2's
FindBestObjectForFunction(7) chose the dining table 47: its entry 0x19, 8199, with 25, 198, 0) and
food_sg's 8206 node 11 (function 1 on the stove 61: entry 0x13, 8197, with 26). The operand's third
byte (1 in both) is not read yet. `primitives::run_functional_tree`.
Iteration 143: the replay runs Animate through the engine (`anim::Catalog`: the archive by id,
the case-sensitive STR# 129 lookup, the notes and the house's preload sets; `anim::Animator`),
where a log has the frames ('k'). On home-a11.log, every Animate step with an archive animation
answers natively as the log's: 2,200 yields, 36 true and 131 false with an event, none
different (the same on home-a10.log). 1,665 steps are not modelled yet: no name, or a name not
in the archive. The not-found name returns **true** at once, not false: the five
"A2O-flamingo-kick" steps go their node's true way (the tick checks before could not tell).
The event's value is not checked yet: it is not always local 0 (16 there at tick 164 where the
note's value is 100). The log's result is still the one taken.
Iteration 144: the tables. Operand byte 4 picks the name's table: 0 the tree's file's STR# 129,
or, for a semi-global's tree (whose file has no names), the stack object's own file's; 1
Global's STR# 129; 2 the Sim's file's; 3 Global's STR# 156, the adult list of 307 (157 is the
same list for children, "c2o-..."). With them, 4,024 of the 4,032 Animate steps of home-a11.log
are modelled. 4,024 answer natively as the log's (3,758 yields, 92 true, 174 false with an
event) and 7 do not. The game's pause stops the Sims' plays: from tick 23 to 84 the Sims' speed
updates ('#') are all 0 while the objects' tracks go on, and a play over it keeps its place
(the replay takes those ticks from the log, like the fast-forward). The 7 left: one Sim's own
pause (a 30-frame backward play lasting 45 ticks from 186; a per-Sim speed, the '#' record's
owner to its Sim, still to map); a2o-sit-left after the long pause at 5065, with flag 0x40;
one flag 0x10 call that returns false at once (tick 1791). On home-a10.log (no '#' records)
9 differ.
Iteration 145: the Sims' own speeds. Each Sim's '#' records (its animator's owner, the nearest
owner above the Sim's address) give the same answers as the rules (1, 5 in fast-forward, 0 on
a load miss or the game's pause): the rules hold. The one Sim's long run at 186 is not a
pause. Its backward play (a2o-idle-neutral-trans-mstand-mhips, flags 0x22) is stopped for one
frame by a load miss at 208 (place 2.27 of 29), and when its speed comes back it is +1: the
track turns and plays forward to the end, which the run waits for (231). The speed's restore
loses the direction. That is the original's, and kept: in `anim::Play`, a backward play
paused for a frame goes on forward from where it is. On home-a11.log, 4,026 Animate steps are
native as the log's and 5 differ (a2o-sit-left with flag 0x40 at 5065, 4 steps; flag 0x10's
false at once at 1791).
Iteration 146: the 5 left, by flags (`REPLAY_ANIMATE_BY_FLAGS`). Every flag value but 0x40
and 0x10 answers natively as the log's in every step (0x00 1,239, 0x02 40, 0x08 197, 0x11 154,
0x20 1,975, 0x22 303, 0x31 49). Flag 0x40 is one run on home-a11 (a2o-sit-left at the outdoor
chair, rate 60, 171 frames, from 4996). Its track reaches its last frame and is done at frame
7521 (tick 5064). The original still yields at 5065; at 5066 the node answers false (to its
node 15), then at once true, and the next node's a2o-sit-loop is true at once too. Flag 0x10
is one call (a2o-reach-tableht-aroundchairr of the group meal at 1791), true at once. All three
"true at once" steps look like an interaction being ended (the Sim's animations skipped), but
there is no player input then (none after video frame 2104) and the engine's queue does not
show it. One sample of each: not a rule that can be checked yet, so these 5 steps keep the
log's answer. A log with the Sims' queue changes, or more runs with 0x40 and 0x10, is needed.
