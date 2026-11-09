# The first loop

Started 2026-11-09 at the user's request: start the project and work through `plan.md` in a
bounded loop. The user is running another heavy project, so nothing in this loop runs a
game: every step works from the discs and the executables at rest.

## Rules for each iteration

- One chunk below (or one well-defined part of one), then a one-line commit, never pushed.
- Checked on real data: a reader must read every instance on every disc that has it, with
  the failures counted and explained; code with unit tests that pass.
- No game code or data in the repository: extracted files and dumps go to `work/`.
- Nothing runs the game or its recompilation. Every script and build runs through
  `tools/capped.sh` (2 GB, no swap, nice 19), one at a time; `/tmp` stays small. (A reader
  bug once ran away with the user's memory: a misread count, and a bit reader that read
  zeros past the end. Readers now bound their counts and stop at the end of their data.)
- Stop after iteration 10, when the chunks are done, or when two iterations in a row end
  without a commit (the reason written here).

## Chunks, in order

1. **IFF chunks.** Readers for BHAV (every version, both byte orders), OBJD, TTAB, TTAs,
   BCON, STR#/CTSS, OBJf, SLOT, GLOB, TPRP, TREE; every chunk on Bustin' Out's disc and
   every BHAV on the v3.0 discs read without a failure.
2. **Scripts as text.** A disassembler for BHAV (nodes, primitives by name from
   `primitives.md`, operands, true/false targets, calls to other trees), and the opcode
   counts per game, checked against each game's dispatch tables: every opcode a script uses
   has a case, or is a call to a tree.
3. **The v3.0 games' object records.** Where the object definitions and interaction tables
   went in The Urbz, The Sims 2 and Pets (the `datasets.arc` and `quickdat.arc` entries).
4. **The Sims' quick data.** `SimsObjects` and the rest in `quickdat.arc`, worked out
   against Bustin' Out's, which has both it and the IFF files.
5. **The interpreter in Bustin' Out.** `TreeSimImpl`, `TreeStack` and `StackElem`: the
   node step, the frame layout, params and locals, transitions, Gosub, named through The
   Sims 2's map, written up in `docs/interpreter.md`.
6. **The engine's start.** A C++20 library (CMake): the IFF and BHAV readers, the
   interpreter's core (frames, the tree walk, the expression and other script-only
   primitives), with tests on real scripts read from the player's disc.

## Progress

Iteration: 4

- **Iteration 1: IFF chunks, done.** `tools/chunks.py` reads strings (STR#, CTSS, TTAs),
  BHAV, BCON, GLOB, OBJf, OBJD, TTAB, TPRP, SLOT, and the editor's TREE and TRCN as far as
  their headers; each reader must account for every byte (padding 0x00, 0xA3 or 0xCD).
  `tools/check_chunks.py` over each disc: Bustin' Out 54,000 chunks of 13 types, none
  failing, every interaction's string present in its TTAs; The Urbz 10,192 BHAV, The Sims
  2 10,318, Pets 10,744, none failing. What was learnt:
  - BHAV: 12-byte header and 12-byte nodes, signatures 0x8000-0x8002 only, in both byte
    orders; the v3.0 games swap each operand field by its own width, so operands stay raw
    until a primitive's layout reads them.
  - TTAB has six layouts on one disc: versions 2-3 (16-bit count and flags, a delta per
    motive, the string index implied), 4-6 (no attenuation code), 7-8 (three fields per
    motive), and 9-11 bit-packed in the order of 7-8; version 10 is 9's, and 11 opens with
    one 16-bit field for the whole table (meaning not known yet). An empty table may still
    carry a version and a packing code.
  - The first run of the check ate the user's memory: a misread count and a bit reader
    that read zeros past the end. Readers now bound their counts and stop at the end, and
    every run goes through `tools/capped.sh` (2 GB, no swap).

  Next: chunk 2, the scripts as text and the opcodes each game uses.
- **Iteration 2: scripts as text, done.** Each game's interpreter switches found and read
  (`tools/ppc.py`, `tools/dispatch.py`): Bustin' Out, The Urbz, The Sims 2 (by its map) and
  Pets (by the switch's shape and by matching The Sims 2's person switch). The switch reader
  first missed cases (a value left alone in its range after an equals test was not
  recognised), which showed as The Sims 2's scripts using primitives "not accepted"; fixed
  by carrying the values ruled out, and the default's own cases dropped. The numbering is
  one across the games (`docs/primitives.md`), names in `tools/primitives.py`.
  `tools/bhav_text.py` checks every script (targets, primitives accepted, own-tree calls)
  and writes them as text to `work/scripts/`: 24,856 trees, 268,627 primitive nodes, no
  failure; one dead script in Pets (opcode 54, dropped), four own-tree calls from
  semi-global trees. Expression is 68-72% of all primitive nodes.

  Next: chunk 3, where the v3.0 games keep their object definitions and interactions.
- **Iteration 3: quick data, the container.** The v3.0 games keep object data out of IFF,
  in `quickdat.arc`, and so do The Sims and Bustin' Out (beside its IFFs): one format in all
  five, read as The Sims 2's `ERQuickdata` reads it (`LoadXImage`, `ReloadXImage`,
  `applyFixups`, `getTable`, `getStartAddr`; disassembled in `work/`, described in
  `tools/quickdata.py`). An entry is a relocatable image of the engine's tables: 24-byte
  records (the main image, then one image a language, 11 to 21), a fix-up stream of four
  kinds giving the pointers, and a directory of named tables (name, rows, row names, count,
  row size) in two header layouts (v2.0 games and v3.0). All 35 entries on the five discs
  read, every pointer inside its image. `SimsObjects` has the same tables in every game:
  `ObjDefinition` (196-byte rows: The Sims 954, Bustin' Out 1,778, The Urbz 1,368, The Sims 2
  1,069, Pets 1,066), `ObjDefinition::ResData`, `ResFile`, `AnimRef`, `PropRef`, the
  sound tables.

  Next: the rows. `ObjDefinition` and `ResFile` against Bustin' Out's IFF files (its OBJD,
  TTAB, OBJf), which say what each field holds; then chunk 4 is mostly done, since The
  Sims' objects are the same tables.
- **Iteration 4: object definitions in quick data.** An `ObjDefinition` row is an OBJD:
  version, then its first 96 fields big-endian, the three 32-bit pairs (12-13 the GUID,
  68-69, 70-71) stored as 32-bit words; the tool's own fields are 92 (an id), 94-95 (a
  pointer to an `ObjDefinition::ResData` row) and 0xFFFF in field 6 of some parts of
  multi-tile objects (`tools/objects.py`). Checked on Bustin' Out: all 1,747 objects that
  are in both its quick data and its IFF files identical, field for field. Every game's rows
  are version 138: The Sims 954 objects, Bustin' Out 1,778 rows (1,748 GUIDs), The Urbz
  1,368, The Sims 2 1,069, Pets 1,066. So chunk 4's objects are read too; The Sims' object
  definitions are the same table.

  Next: `ResFile` (72-byte rows, one per object file: slots by resource type, the
  interactions, strings, function table and so on), from the engine's `QuickResFile`
  (The Sims 2's map: `GetByID`, `GetIndType`, `Count`), checked against Bustin' Out's IFFs.
