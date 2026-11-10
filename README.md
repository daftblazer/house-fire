# House Fire

A native engine for the GameCube Sims games and The Sims 2: Castaway on the Wii (Edge of
Reality and Maxis's EOR engine): one
engine whose game logic is the games' own behaviour scripts, read from the player's disc,
with only the assets and scripts coming from the disc.

| Game | Disc ID | Engine |
|---|---|---|
| The Sims (2003) | `GCIE69` | EOR Engine v2.0 |
| The Sims Bustin' Out (2003) | `G4ME69` | EOR Engine v2.0 |
| The Urbz: Sims in the City (2004) | `GUBE69` | EOR/Maxis Engine v3.0 |
| The Sims 2 (2005) | `G4ZE69` | EOR/Maxis Engine v3.0 |
| The Sims 2: Pets (2006) | `G4OE69` | EOR/Maxis Engine v3.0 |
| The Sims 2: Castaway (2007, Wii) | `R42E69` | EOR/Maxis Engine v3.0 (built Aug 25 2007) |

Status: started. `engine/` is the C++ engine (readers and the interpreter's core so far):

```sh
CXX=clang++ cmake -S engine -B work/build -G Ninja && ninja -C work/build
work/build/test_interp                                      # the node step's rules
work/build/test_expression                                  # the expression primitive's rules
HOUSEFIRE_BUSTIN_ISO=/path/to/G4ME69.iso work/build/test_lots  # every lot of your disc, as a world
work/build/test_primitives                                  # SetToNext's and Idle's rules
HOUSEFIRE_BUSTIN_ISO=/path/to/G4ME69.iso work/build/test_disc  # every script on your disc
work/build/lockstep LOG G4ME69.iso   # the native interpreter against a log of the recompiled game's
```

A log comes from the recompiled Bustin' Out run with `BUSTIN_TREE_LOG=<file>`
(`docs/interpreter.md`, "Checked against the original").

 `docs/survey.md` is what the discs hold, `docs/objects.md` what the scripts' variables are, `docs/lots.md` where the world comes from, `docs/plan.md` how the engine
is to be built, `docs/primitives.md` the script primitives found so far, `docs/loop.md` the work under way.

The reference for what the engine must do is the recompiled game
(`../bustin-out-recomp`), which runs the original code exactly and replays exactly: each
part of this engine is checked against it before it takes over.

## Rules

- **No game code or data in the repository.** Discs are the player's own and are read in
  place; anything extracted goes to `work/` (ignored). Function names and addresses, and
  descriptions of formats, may be written down; code, scripts, strings and assets from the
  games may not.
- Commit messages are one line.

## Tools

Python 3. `.rvz` images are read through decomp-toolkit (`DTK=/path/to/dtk`); `.iso` directly.

```sh
tools/survey.py IMAGE...        # IFF chunk types on each disc, and where they are
tools/check_chunks.py IMAGE...  # read every chunk the engine uses; non-zero exit on a failure
tools/bhav_text.py [--dump] IMAGE...  # check every script; --dump writes them as text to work/scripts/
tools/quickdata.py IMAGE [ENTRY] # the tables in each quick data entry (quickdat.arc)
tools/objects.py check IMAGE    # object definitions in quick data (check: against Bustin' Out's IFFs)
tools/resfile.py check IMAGE    # object resource files in quick data (check: against Bustin' Out's IFFs)
tools/interactions.py check|trees IMAGE  # interaction tables in quick data
tools/dispatch.py IMAGE...      # the opcodes each game's interpreter accepts, from its executable
tools/tree_log.py LOG IMAGE     # a log of the recompiled game's interpreter: its trees on the disc, its steps
tools/ngh.py IMAGE [--dump]     # the neighbourhood file the lots come from: its blocks unpacked
tools/lot.py IMAGE              # the lots in it: chunk tables and object types
tools/objm.py IMAGE             # each lot's objects (ObjM): the table in list order, the sections
tools/recon_trace.py LOG SYMBOLS  # a trace of the recompiled game's save stream (BUSTIN_RECON_LOG)
tools/variables.py LOG          # the variable scopes, from a log of the original's lookups
tools/locals_check.py IMAGE [LOG]  # scripts that read a local before writing it (and what the original read)
tools/capped.sh CMD...          # run CMD with at most 2 GB and no swap (use it for everything)
```

`tools/disc.py` (disc images), `tools/arc.py` (the `.arc` archives), `tools/iff.py` (Maxis
IFF 2.5), `tools/chunks.py` (the chunk types), `tools/primitives.py` (the primitives' names and each
game's opcodes), `tools/ppc.py` (reading switches in the executables).
