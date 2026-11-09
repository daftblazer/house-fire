# Plan

One native engine for the five games, its game logic being each game's behaviour scripts
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

## What does not come over from Bustin' Out's port

The port's own redrawn interface (its "HD UI": `src/hud_read.cpp`, `hud_pieces.h`,
`hud_draw.cpp`, `BUSTIN_HDUI`) is not part of House Fire. The menus and HUD here are the
games' own (the `Apt` movies in `flashes.arc`, their fonts and textures), played natively
and drawn at the window's resolution. Nor is the port's launcher (`launcher/`, its settings
window and `settings.ini`): House Fire gets its own front end, designed for it. What may come over is what is about the engine and
not about one port: the renderer's work for the ERC command stream and materials, the
picture options, the audio mixing.

## Next

Bustin' Out's interpreter in its executable: the node step, the stack layout and the
primitives' parameters, named through The Sims 2's map, and the lockstep hook in the
recompiled game (an observer that changes nothing, so its replays stay identical).
