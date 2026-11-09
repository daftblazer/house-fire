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

Iteration: 7

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
- **Iteration 5: object resource files in quick data.** `QuickResFile` turned out to be
  stubs; the readers are per type (`TreeTableQuickData`, `ObjFnTableQuickData`, ...), each
  finding its array in a `ResFile` row through `FindRes<T>`. Their callers give the row's
  twelve slots and element sizes (`tools/resfile.py`). Checked on Bustin' Out's 559 object
  files: every quick string set and interaction table is the IFF chunk of the same id, and
  the counts of constants (1,569), slots (641), function tables (1,671) and sounds (6,022)
  equal the files' BCON, SLOT, OBJf and FWAV; what the tool left out is string sets 129,
  130, 304 and a few more (labels the game does not read) and 18 unused interaction tables.
  The same layout reads in all five games (The Sims 199 files, The Urbz 547, The Sims 2
  508, Pets 568).

  Next: the elements: an interaction table's 36-byte entries against TTAB (action, guard,
  motives, the name from TTAs), constants against BCON, function tables against OBJf.
- **Iteration 6: interactions in quick data.** An interaction table's entries are 28 bytes
  in the v2.0 games, 32 in The Urbz, 36 in The Sims 2 and Pets (`tools/interactions.py`):
  action and guard, motives (only those changed: personality, minimum, delta, motive index),
  attenuation, flags, order and autonomy threshold, joining and string index, and the name
  through the table's list of names in the language image (UTF-16). Quick data needed the
  language images loaded after the main image (`tools/quickdata.py`, as the engine does).
  Checks: The Sims 2 and Pets, 6 of 2,823 and 3,923 actions and guards name an own tree the
  object's script file lacks (two shared objects); The Urbz 192 of 2,669. Bustin' Out: 597
  of 921 tables identical to its IFF field by field (the tool sets flag 0x200 and renumbers
  the names), the rest an older revision in the IFF (names absent from its TTAs).
  - **Found:** Bustin' Out's `Runtime/` IFF files are not its full scripts. Its quick
    interactions call 2,570 own trees those files lack (`abduction` has 0x1000-0x1001 and
    calls 0x1005). Bustin' Out's trees must be in quick data too, probably the ResFile slot
    still unnamed (+0). Everything about scripts so far (iteration 2) holds for what is in
    the IFF files; whether those files are what the game runs is open.

  Next: Bustin' Out's trees in quick data (ResFile slot +0 and `GlobalResFile`), against its
  IFF BHAVs where both exist; then the rest of chunk 3 (constants against BCON, function
  tables against OBJf).
- **Iteration 7: where Bustin' Out's trees are.** Not in quick data: ResFile slot +0 is
  the file's object definitions (pointers to `ObjDefinition` rows). Not anywhere else on
  the disc either: `scripts.arc` is 4 bytes, `datasets.arc` holds graphics and houses, and
  no other archive (`samples.arc` included) has IFF data; the executable loads
  `GameData/Objects/` IFF files. The missing trees are in the semi-global files: of the
  2,624 own-tree actions and guards Bustin' Out's interaction tables name but their files
  lack, 2,312 are the semi-global file's tree at id + 0x1000 (variants keep their shared
  trees there); 145 belong to objects without a semi-global file (leftovers), 167 are not
  explained. Written up as the first part of `docs/interpreter.md`; to be confirmed in the
  engine's code and by the recompiled game. So the scripts on the discs are complete as far
  as can be told, and iteration 2's findings stand.

  Next: chunk 5, the interpreter in Bustin' Out: the node step, frames, the tree lookup
  (this rule), named through The Sims 2's map.
