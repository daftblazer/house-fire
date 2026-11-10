# Plan

One native engine for the six games (the five GameCube Sims games and The Sims 2: Castaway on the Wii), its game logic being each game's behaviour scripts
(SimAntics BHAV, `survey.md`) run by a native interpreter, with the C++ engine around the
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
| A new frame's locals | what the stack's memory held | zero | the scripts that read one first read zero in practice (`interpreter.md`, "The call"); reads are reported |
| `++`/`--` then compare, on a float variable | compares a word read through a null pointer | compares the new value | a crash-class bug; no script in the logged session does it |

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
The RK3566 is to be measured again the next time it is on.

## What does not come over from Bustin' Out's port

The port's own redrawn interface (its "HD UI": `src/hud_read.cpp`, `hud_pieces.h`,
`hud_draw.cpp`, `BUSTIN_HDUI`) is not part of House Fire. The menus and HUD here are the
games' own (the `Apt` movies in `flashes.arc`, their fonts and textures), played natively
and drawn at the window's resolution. Nor is the port's launcher (`launcher/`, its settings
window and `settings.ini`): House Fire gets its own front end, designed for it. What may come over is what is about the engine and
not about one port: the renderer's work for the ERC command stream and materials, the
picture options, the audio mixing.

## Next

The node step, calls and returns agree with the recompiled Bustin' Out in lockstep
(`interpreter.md`, "Checked against the original"). Next:

- The object model's next parts (`objects.md`): the scopes not modelled yet, the object
  primitives (create, remove, SetToNext), and the sizes of each object's tables from its
  definition.
- The frames the engine pushes itself (+8 set) and the stack resets outside the node step:
  what starts an action, which is where the + 0x1000 interaction rule would show.
