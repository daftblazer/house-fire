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

A second form, seen only with a parameter of 1800 (the global tree 0x118's, five stacks on the
home log: the made chips at 692 and meal at 1765, and three lot objects at 843): the node is
stepped on every tick for a stretch, each step yielding with the parameter one less (1800, 1799,
...), for 49, 26, 200, 25 and 49 ticks; then the stack is passed over like any sleep (the chips
from 742 until something else wakes them at 1381, the parameter left at 1750). What ends the
stepped stretch is not in the log. Natively the sleep is the passed-over one from the first step,
so replay counts those stepped ticks as "the log ran a stack the native has asleep". The pass
(`Simulator_RunObjects`, 80238150) reaches each object's sleep through virtual calls (its vtable's
+0x90, +0xA8, +0x228, +0x238; a decrement at 80238368 before the +0x218 call, the pass-over
count): which of those checks lets a sleeping stack be stepped is the place to observe next (a
read-only record of the +0x238 call's result per object per tick).

The engine has it as `idle()` (`engine/src/primitives.rs`), the interpreter's `Sleep` and the
simulator's pass. `engine/tests/primitives.rs` and `engine/tests/simulator.rs` test it.

## Relationship2 (opcode 26): not native yet

Reads and writes the relationships between objects (each object's saved RelMatrix, `lots.md`)
and between neighbours (NBRS's). 128 uses in the session, with six operand forms. For the
commonest get (`00 02 01 00 08 00 00 00`, 27 uses) the step is true exactly when the object
has a saved relationship with the stack object (5 of 5), false otherwise (22 of 22). The
other forms do not follow from the operand's obvious reading (one of them is both true and
false between objects with no relationship). Castaway's `cXObject::TryRelationship2` (0xAA8
bytes) shows why: modes in operand byte 1 (0, 2, 4 ...), objects or neighbours
(`Relationship2Param::GetUseNeighbors`), a create flag (`GetCreate`), pet ownership. It is
the relationship system, to come as one; the engine keeps the saved relationships
(`Object::relationships`; the club's 16) for it.

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

Asks about a goal's state. Castaway's `CompleteGoalParam` names the operand: a 16-bit
operation, a 16-bit goal datum and its owner (the scope), and a flags byte (status into
temp 0; true if the goal is locked, open, satisfied or completed; the low two bits an old
form), then a type. In the session all 2,377 uses (the party controller and the dance floor's
tiles, every few ticks) are false, all of the old form, the goal named by constants. Native,
it needs the goals' state (the player's progress, in the save): the goal system's, to come
with it. Until then the replay takes the log's answer.

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
