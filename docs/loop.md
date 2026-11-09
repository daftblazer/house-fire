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

Iteration: 10 (the last; the loop stopped here)

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
- **Iteration 8: the interpreter's map and frames.** From The Sims 2's code: tree ids by
  range (`Behavior::GetTreeClass`, `GetResFileID`: global 256-4095, own 4096-8191,
  semi-global 8192-9000), with no fallback from own to semi-global there; the frame
  (`StackElem`: tree id, counts of locals and parameters, header words, parameters then
  locals, its size rule) and the stack (`TreeStack::Push`). Bustin' Out's interpreter
  functions placed by matching code and by the source file's order: 25 functions, its node
  step `8023F1FC`, in `docs/interpreter.md`.

  Next: the node step itself (`DoNodeAction`, The Sims 2 `80147FD8`, Bustin' Out
  `8023F1FC`): what it reads from a node, how it calls and returns, what it passes a
  primitive; then chunk 6, the C++ library's start.
- **Iteration 9: the node step.** Read from The Sims 2's `DoNodeAction` and written up in
  `docs/interpreter.md`: returns (254 true, 255 false), calls (the operand as parameters
  unless all -1), primitives (results 0 false, 1 true, 2 yield, 3 go on, else stop), the
  iteration limit and the five error codes (1000, 1001, 1100, 1101, 1102). Bustin' Out's
  `8023F1FC` compares with the same values and uses the same codes and results. It
  corrected an earlier reading: node target 253 is error and 255 return false, not the
  other way round (`tools/bhav_text.py`, `primitives.py`, `primitives.md` fixed; the checks'
  counts do not change, since 253-255 were all accepted as targets).

  Next: chunk 6, the C++ library: IFF and BHAV readers and the interpreter's core (frames,
  the node step as written up, the expression primitive) with tests on real scripts.
- **Iteration 10: the engine's start.** `engine/`, a C++20 library (CMake): the IFF and
  BHAV readers (both byte orders), tree kinds by id, a disc reader for .iso, and the
  interpreter's core: frames, calls (the operand as parameters), returns, the node step's
  transitions, results and five error codes as `docs/interpreter.md` has them, with
  primitives and tree lookup left to a host (each object's). Tests: `test_interp` (the
  step's rules on trees written in the test: targets, returns, calls and parameters, yield,
  each error, the iteration limit) passes; `test_disc` reads all 9,529 BHAVs of the
  player's Bustin' Out disc and runs every tree with a fixed-rule host, all 9,529 ending
  (6,150 returning, the rest the step's errors, as a host that answers at random leads to
  calls with no tree and 253 exits). The expression primitive is not in: its operand
  layout and variable scopes (The Sims 2's `cXObjectImpl::InterpWriteValue`, `TryExpression`)
  are the next thing to read.

## Where it ended

Ten iterations, ten commits, every chunk started and chunks 1-3 and 5 done as planned; 4
done for the object data (The Sims' interactions and strings read with the same code, its
scripts not found yet); 6 started (the core, not the expression primitive).

What the next steps need that this loop could not do: confirming the interpreter against
the original needs the recompiled Bustin' Out running (an observer hook at its node step,
`8023F1FC`, that leaves replays identical, then the native interpreter in lockstep); that
waits for the user, since nothing here ran a game. Open questions it would settle: the
own-to-semi-global + 0x1000 rule for interactions, what a callee's parameters are when a
call passes none, and how locals start.

# The second loop

Started 2026-11-09, after the interpreter, the expression and the object model agreed with
the recompiled Bustin' Out (`interpreter.md`, `objects.md`). The user allows game runs now,
in this setup only: headless gamescope at the game's native 640 x 480 and 30 fps, muted,
the new-game route, in a 4 GB scope at low priority, at most 330 s, one run at a time.

## Rules for each iteration

- Read this section's Progress first; bump the iteration; do ONE chunk, or one
  well-defined part of one.
- Check it against the original: `tools/lockstep` on a log of the recompiled game
  (`BUSTIN_TREE_LOG`). Extend the observer (`../bustin-out-recomp/src/tree_log.cpp`,
  read-only hooks) when a chunk needs more of the original's state, and re-run the game
  only then. Unit tests for the rules a session does not reach.
- The rule in `~/.claude/CLAUDE.md`: the same game, a new engine. Match what the player
  sees, write the internals anew, list every deliberate difference in `plan.md`.
- Commit each repository's work with a one-line message, never pushed. Bustin' Out's
  observer changes are committed there (on main; the submodule untouched).
- Every build and script through `tools/capped.sh` (or the 12 GB scope for the recomp's
  build), one at a time; logs in `work/`, `/tmp` kept small.
- Stop after iteration 12, when the chunks are done, or when two iterations in a row end
  without a commit (the reason written here).

## Chunks, in order

1. **SetToNext (31).** Its operand, its search kinds, and its writes (the stack object
   and others), natively, checked step by step.
1b. **Lots.** The house files: objt, ObjM, HOUS and the arrays; the native world made from
   a house file (each object's definition, id, data, attributes, the list's order), checked
   against the logged club's objects.
2. **Objects made and removed.** CreateObject (42), KillObject (18) and how ids are
   given out; the world's object lifecycle, so the 16 differences of `objects.md` go.
3. **Table sizes from the definitions.** Attribute, object data and person data counts
   taken from each object's OBJD (and the Sims'), not grown on use.
4. **The scopes not modelled.** 12, 16, 20, 21 (the stack object's definition: OBJD
   fields), 23, 24, 32, 40: what each reaches, checked like the rest.
5. **The next primitives by use.** Idle (0), Animate (44: what scripts see of it),
   Dialog (36), CompleteGoal (56), IdleForInput (17) and the others the session runs:
   their operands, results and variable writes, checked step by step; each one's work
   outside the scripts noted for its system.
6. **Frames the engine starts.** The frames with +8 set, the stack resets outside the
   node step, and what starts an action: where the interaction tables' trees are looked
   up (the + 0x1000 question).

## Progress

**Iteration 1** (chunk 1, SetToNext; re-planned):
- **The operand.** Byte 4: the search kind in the low 7 bits; 0x80: the target is the
  variable of scope byte 5, datum byte 7 (else the stack object). Bytes 0-3: a GUID for
  kind 4. From its code (`fn_8020C024`, reached through the object switch's case 31) and
  45,000 logged steps.
- **The kinds seen:**
  - 0: every object, in the world's object list order (not by id: a full walk of the club
    is 179 objects, 178, 177, ... 127, 2, 57, 168, ...);
  - 1: Sims;
  - 3: the other part of a multi-tile object;
  - 4: objects of a GUID;
  - 5: neighbours, ascending;
  - 6: a category.
  The step ends false when there is no next.
- **The dependency found.** The kinds that filter need each object's definition, and the
  order is the object list's: both come from how a lot loads its objects. Objects do not
  keep a plain pointer to their definition row: a scan of each object's memory found one
  only for the 9 Sims, whose rows are made at run time and are on no file
  (`tools/definitions.py`, the observer's 'O' records).
- **The disc's lots** are PC-format house files (`Runtime/StoryData/Houses/House00-11.iff`,
  `ChalData`, `Neighborhood.iff`): HOUS, objt (object types by GUID), ObjM (each object's
  state), Arry, SIMI, FAMI. A native lot loader gives every object its definition, id and
  place in the list.
- **Engine:** the library now reads the OBJD chunks (`Definition`, by GUID).
- **Re-planned:** lot loading becomes chunk 1b, before SetToNext's kinds, the object
  lifecycle and the table sizes.
- **Next:** iteration 2, the house files' chunks (objt, ObjM, HOUS), read on every house
  file of the disc, and the club's objects matched to the logged ones (ids, order).

**Iteration 2** (chunk 1b, part one: where the lots are):
- **Not the house files.** The game never reads `Runtime/*/Houses/*.iff`. The run's disc
  reads, mapped to files, show the lots come from `Runtime/default.ngh` (and the small
  `levels.arc`).
- **`default.ngh` is the engine's container:** 4 KB blocks, each data block a 28-byte
  header (section, 28, unpacked size, packed size, checksum, 4, count) then LZSS (flag bytes
  low bit first, 12-bit positions, lengths 3-18, a 4096-byte ring from 0xFEE). FourCCs are
  stored backwards.
- **`tools/ngh.py` reads it:** 35 blocks, all 35 unpack to exactly their stated size from
  exactly their packed size.
  - Section 0 is the directory: 16 SIMI/HOUS pairs.
  - Section 1 is the neighbourhood: NGBH, NBRS, the FAMI records.
  - Section 2 is 17 thumbnails (THMB).
  - Section 3 is the 16 lots. Each starts with a chunk table (Arry x3, objt, ObjM, with
    offsets and sizes), the PC house format inside.
- **Next:** iteration 3, a lot's chunk table and its objt and ObjM (the PC field encoding),
  read on all 16 lots, and the club's objects matched to the logged ones.
