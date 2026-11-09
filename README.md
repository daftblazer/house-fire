# House Fire

A native engine for the GameCube Sims games (Edge of Reality and Maxis's EOR engine): one
engine whose game logic is the games' own behaviour scripts, read from the player's disc,
with only the assets and scripts coming from the disc.

| Game | Disc ID | Engine |
|---|---|---|
| The Sims (2003) | `GCIE69` | EOR Engine v2.0 |
| The Sims Bustin' Out (2003) | `G4ME69` | EOR Engine v2.0 |
| The Urbz: Sims in the City (2004) | `GUBE69` | EOR/Maxis Engine v3.0 |
| The Sims 2 (2005) | `G4ZE69` | EOR/Maxis Engine v3.0 |
| The Sims 2: Pets (2006) | `G4OE69` | EOR/Maxis Engine v3.0 |

Status: started. `docs/survey.md` is what the discs hold, `docs/plan.md` how the engine
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
tools/survey.py IMAGE...      # IFF chunk types on each disc, and where they are
```

`tools/disc.py` (disc images), `tools/arc.py` (the `.arc` archives), `tools/iff.py` (Maxis
IFF 2.5).
