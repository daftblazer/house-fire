# Plan

One native engine for the six games (the five GameCube Sims games and The Sims 2: Castaway on the Wii), its game logic being each game's behaviour scripts
(SimAntics BHAV, `survey.md`) run by a native interpreter, with the engine (Rust) around the
scripts written anew to do what the original does. The discs supply scripts, object data and
assets; nothing of the games' executables is used.

## The reference

The recompiled Bustin' Out (`../bustin-out-recomp`) runs the original code exactly and its
replays are exact. Every part of this engine is checked against it on the same inputs before
it takes over: the same state after the same steps. The Sims 2's linker map names the engine's
classes and functions, and through matching code, most of Bustin' Out's (`survey.md`). Each
other game gets the same kind of reference from its own recompilation later.

## Order

1. **Formats.** Readers for everything the engine loads: IFF and its chunks (BHAV, OBJD,
   TTAB, TTAs, OBJf, SLOT, BCON, TPRP, TREE, STR#, CTSS, GLOB, HOUS, FAMI, NGBH, ...), the
   `.arc` archives, the v3.0 games' own records for object data, The Sims' `quickdat`
   (worked out from Bustin' Out, which has both), models, textures, animations, menus.
   Checked by reading every file on every disc.
2. **The interpreter.** `TreeSim` (stack frames, params, locals, the tree walk, true/false
   transitions, Gosub) and the primitives in `primitives.md`, first those that only touch the
   script's own state (expression, random, gosub, test), then the ones that act on the world.
   Checked in lockstep: a hook in the recompiled game at the interpreter's node step reports
   each node, its frame and the variables it changed; the native interpreter, given the same
   world state, must report the same.
3. **The simulation around it.** Objects and people (`cXObjectImpl`, `cXPersonImpl`), the
   tick (`cSimulatorImpl`), motives, autonomy (`TryFindBestAction`), routing, slots, the lot,
   time. Each checked by comparing state with the recompiled game's in replays.
4. **The game around that.** Rendering (the existing Vulkan renderer's work), animation,
   sound, menus (`Apt`), Create-A-Sim, saves, the per-game modes (Bustin' Out's story, The
   Urbz's reputation and districts, The Sims 2's wants and fears, Pets' pets).
5. **The other games,** each as its data and its own module of primitives and modes.
6. **Multiplayer beyond the originals** (after the games are fully playable; the user's
   roadmap item, 2026-11-15): a dedicated server holding a lot or neighbourhood, with players
   joining it to play together. A deliberate difference, so off for every check against the
   original.

### Built for multiplayer from now on

Nothing of it is built yet, but the engine is shaped so it can be added without a rewrite:
- **The simulation is a deterministic state machine apart from the presentation.** It steps on
  its own tick from the world state and a list of inputs, with no reads of the renderer, audio,
  the wall clock or the host's randomness (its random stream is part of the state). This is
  what the lockstep checks already hold it to, and a server needs the same.
- **Player actions are commands.** What a player does (an interaction picked, a build or buy
  change, a speed change, a Sim chosen) reaches the simulation as a command for a tick, never
  as a direct write from the UI. Replays, the server and later save-game journals use the same
  commands; a command carries which player sent it.
- **The world state serializes whole** (chunk 4.7's saves): a joining player gets a snapshot,
  then the commands after it. Ids are the engine's own and stable across machines.
- **No single-player assumptions inside the simulation**: whose household or Sim a command acts
  on is an argument, not a global "the player"; the camera, selection and UI state live in the
  client.
- **The client renders from the state, never feeds back into it**: animation, sound and the
  picture are derived from the simulation's state and events, so a client can run without
  authority (the server's tick decides).

## Same game, new engine

The games must play and look the same; the engine under them is new, written to be clean
and light, not a copy of the original's code. What a player sees (what Sims and objects do
and when, the picture, the sound) is matched against the recompiled game; how the engine
does it (memory layout, data structures, loading, the engine's own bookkeeping) is ours.

Where the original's behaviour is a bug, it is kept unless it is changed on purpose, and
each change is listed here so that the lockstep check skips exactly it and keeps proving
the rest. Crashes and corruption are fixed without a listing.

Deliberate differences:

| | The original | Here | Why it does not change play |
|---|---|---|---|
| A new frame's locals | what the stack's memory held | zero | the scripts that read one first read zero in practice (`interpreter.md`, "The call"); reads are reported. Checked on the three logs: 372, 62 and 21 reads of a local its frame had not written (the original's left-over values, -32569 among them), and with them left at zero no expression, branch or step changes (replay `REPLAY_ZERO_LOCALS`); replay counts them apart |
| `++`/`--` then compare, on a float variable | compares a word read through a null pointer | compares the new value | a crash-class bug; no script in the logged session does it |
| Random's numbers | the console's generator, shared with animation, sound and the rest | the engine's own (xorshift64*, `World::rng`), unbiased over the range | the numbers are as random and in the same ranges; a sequence the same as the original's would mean copying every other user of its generator. Lockstep and replay take the original's numbers (autonomy's choice included: its `R` records) |
| Autonomy's list, values equal within 1e-7 | the order its sort (`Util_Sort`, not stable) leaves them | the candidates' order (a stable sort) | such a tie is between interactions of the same worth; which of two equal ones a Sim prefers is chance in the original too (the home log has one, of the 41 entries, below the pick) |

## The CPU budget

The recompiled Bustin' Out was too heavy for an RK3566 (four Cortex-A55 at 1.8 GHz), for
reasons this engine does not have:
- every load and store through the emulated memory, byte-swapped;
- exact float rounding;
- a clock tick and an interrupt check at every backward branch;
- one emulated thread.

The original's logic ran at 30 fps on one 485 MHz Gekko with much else beside it, so native
code doing the same work should fit in a fraction of one A55 core. That is made a target from
the start, not an afterthought:

- **Budget:** the game logic under 5 ms of each 33 ms tick (30 ticks a second) on one A55
  core of an RK3566, with the club lot's busiest scene. The game logic is the scripts,
  routing, autonomy and the simulation's own updates. Rendering, animation and sound are
  measured apart, with their own budgets once they exist.
- **Benchmark:** a headless tool runs a lot's simulation natively, with no window, and reports
  milliseconds per tick for each system (scripts, routing, autonomy, the rest) and the worst
  tick. It runs on the PC, and on the player's RK3566 device: the user gives SSH access when
  asked.
- **Checked whenever a system comes in,** like the lockstep: a system that passes the
  lockstep but breaks the budget is not done.
- **Designs that suit a small CPU:** data laid out for the cache, routing incremental and
  cached, autonomy scores cached and updated as they change rather than recomputed for every
  Sim each tick, animation evaluated only for what is on screen. Work independent between
  objects can go to the other cores, which the original never used, as long as the result is
  the same.

Where the cost goes is measured, not guessed. The scripts are cheap: the logged session ran
about 71 steps a tick. The heavy parts are expected in routing, autonomy, animation and
skinning, the systems still to come.

**Measured (the PC, 2026-11-09).** `replay LOG ISO --bench` runs the club natively over a
logged session five times and gives the medians of the native work alone: the passes, the
steps, and in them the expression, SetToNext and Idle (the primitives not native yet are
answered from the log and cost next to nothing; the replay's own checks are not counted).
On the 5,130-tick session (67 steps a tick): 12.3 µs a tick on average, 8.1 µs median,
105 µs at the 99th percentile, 190 µs at worst once the lot has loaded (689 µs for the
tick right after it, which sets up every object's stack). The scripts take 7.1 µs of a tick,
the pass 5.2 µs. On the 719-tick one: 14.7 µs on average, 136 µs at worst. The budget is
5,000 µs on an A55.

**Measured again in Rust (the PC, 2026-11-10).** The engine moved from C++ to Rust with its
behaviour unchanged: the replay's and lockstep's outputs on the three logs line for line the
C++ tools'. On the home log (6,869 ticks, 82 steps a tick),
interleaved with the C++ build: 9.9 µs a tick on average against 10.6, the expression 1.8
against 2.2 µs a tick, SetToNext 0.54 against 0.41, the pass 0.87 against 0.80.

**Measured (the RK3566, an RG353VS on ROCKNIX, 2026-11-09).** The same benchmark, a static
aarch64 build (`-mcpu=cortex-a55`, cross-compiled in a Debian container), pinned to one core
at 1.8 GHz, the frontend running: 125 µs a tick on average, 80 µs median, 918 µs at the 99th
percentile, 1.8 ms at worst past the load, 7.7 ms for the tick after it (two runs, within
2%). About ten times the PC. The scripts take 67 µs of a tick, the pass 59 µs: the pass (a
lookup for each id, each tick) is as costly as all the scripts, and is the first thing to
make cheaper. The budget leaves room, but not much to waste, for the systems to come.

**The pass made cheap (2026-11-09).** The simulator keeps its objects by id in a vector, each
with its stack, rebuilt only when the world's objects change (`World::version`), so a pass
walks a vector instead of looking up every id in two maps. On the PC the pass went from
5.2 µs to 1.6 µs a tick (most of what is left is the replay's own list of the pass for its
checks) and a tick from 12.3 µs to 8.7 µs on average; every check of the replay unchanged.
Measured again on the RK3566 (2026-11-10, the same device, log and settings, two runs within
2%), with the fifth loop's primitives native as well (the queue's deliveries and checks,
PushAction, Dialog): 118 µs a tick on average (was 125), 58 µs median (was 80), 1.15 ms at
the 99th percentile, 2.2 ms at worst past the load (was 1.8), and 8.9 ms for tick 2 (was
7.7). The pass went from 59 µs to 32 µs a tick; the scripts rose from 67 µs to 85 µs, as more
of their work is now native rather than answered from the log. The ticks past the load fit
the budget with room; tick 2, which builds every object's stack, does not (8.9 ms of 5): to
be spread or made cheaper, or done during the load, before the systems to come add to it.

**The planner (2026-11-11, on the PC).** `replay --bench` now also times the native planner on
each of the home log's 18 plans (the whole request: the goal choice, the search, the
refinement, the point after the goal), 21 times with the rounds interleaved, each plan's median,
in each of the bench's 5 runs, three times over (15 samples): a plan's median 30-32 µs, the
worst 136-143 µs (tick 260, the yard to the front door, and tick 1901, both one goal), all 18
together 0.83-0.85 ms. A tick plans at most once a Sim, so the worst is about 3% of the 5 ms
budget on the PC; the RK3566's figure is the user's to measure (the same command there).

**On the RK3566 (2026-11-11).** The same `replay --bench` on the home log (today's, with made
objects, the clock and the planner native), a static aarch64 build (musl, `-C
target-cpu=cortex-a55`, linked with rust-lld; musl's allocator may cost a little against the
earlier glibc builds), pinned to one A55 (`taskset -c 3`), twice: the planner 216-222 µs a plan
median, 1.04-1.05 ms at worst (tick 260; about 6.7x the PC), all 18 6.3 ms; a tick 112 µs on
average (was 118), 1.75 µs median, 530-538 µs at the 99th percentile, 1.27-1.28 ms at worst past
the load (was 2.2 ms); scripts 88 µs a tick, the pass 24 µs. The worst plan and the worst tick
together, about 2.3 ms, fit the 5 ms budget; the planner's runs are not yet in the tick's
figures (they are measured apart), nor animation and the rest of routing.

## What does not come over from Bustin' Out's port

The port's own redrawn interface (its "HD UI": `src/hud_read.cpp`, `hud_pieces.h`,
`hud_draw.cpp`, `BUSTIN_HDUI`) is not part of House Fire. The menus and HUD here are the
games' own (the `Apt` movies in `flashes.arc`, their fonts and textures), played natively
and drawn at the window's resolution. Nor is the port's launcher (`launcher/`, its settings
window and `settings.ini`): House Fire gets its own front end, designed for it. What may come over is what is about the engine and
not about one port: the renderer's work for the ERC command stream and materials, the
picture options, the audio mixing.

## Next

Stage 3 (the simulation) agrees with the recompiled Bustin' Out on every log there is (the
eleventh loop's end, `loop.md`): replay runs the home lot (Mom's house), the club (a new log and
the two old ones) with no step of any Sim's or object's stack different from the original's, and
lockstep finds no difference. What the logs hold is a few minutes of play on two lots; each new
log will show cases they did not.

Left to stage 4, with the presentation they come from:

- **A walking Sim's place at a tick's edge and at a walk's start** (20 of the home log's 1,186
  walking places; `loop.md`, iterations 155-163). The walker itself moves exactly as the
  original's; 14 places are taken a frame early or late where two of the walker's frames fall in
  one tick (the game loop's order of frames and ticks), 6 are the Sim's body off its walker by up
  to 1.5 sixteenths as a walk starts from a run on, closing over a dozen ticks (its animation).
- **The family's funds**: AlterBudget's spending is seen (the money balloon), the funds are the
  log's.
- **The player's commands** come from the front end (`World::queue_player_command`); replay
  takes them from the log, as the dialogs' answers.

Then stage 4 proper: rendering, animation, sound, the menus, Create-A-Sim, saves, the story mode.
