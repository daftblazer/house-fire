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

**Iteration 3** (chunk 1b, part two: the lots' chunks):
- **`tools/lot.py`.** A lot block's chunk table: n FourCCs, n offsets (from the block's
  start in the file, header included), n sizes. Arry x3, objt, ObjM.
- **objt, the object types:** a GUID, six 16-bit fields and an even-padded name each, ended
  by a zero GUID. All 16 lots read to exactly their end: 2,000 types, 1,947 of them
  definitions on the disc (the rest are characters).
- **ObjM (version 80) is not the PC's field encoding.** It's the engine's own object
  stream. The Sims 2's map names it:
  - `ObjectModuleImpl::DoStream(ReconBuffer *)`, through `DoReconObject` / `DoReconPerson`
    to each object's `cXObjectImpl::ReconStream` (`ReconHeader`, `ReconSlots`,
    `EdithVariableSet::ReconStream`);
  - `TreeStack::ReconStream`, so running scripts are saved too.
  `ReconBuffer` writes `Recon16` / `Recon32` / `ReconFloat` / `ReconCmprInt` (with a
  compression `Scheme`) / `ReconBits` / marks. The same framework saves families, the house,
  the neighbourhood, relationships and the simulator: the memory card's saves too.
- **Next:** iteration 4, `ReconBuffer`'s encoding, from The Sims 2's code and Bustin'
  Out's (matched by their code). Checked by decoding the 16 lots' ObjM streams to their ends
  with a first object layout.

**Iteration 4** (chunk 1b, part three: the save streams' encoding):
- **`ReconBuffer`, from The Sims 2's code** (Recon8/16/32/Float/Bool/CmprInt/Bits/Mark and
  the static initialiser that builds the schemes), written as `tools/recon.py`:
  - Bits are read from the top of each byte.
  - A compressed value is 0 (bit 0), or bit 1, a two-bit code and that many bits,
    sign-extended. Widths: 8-bit 2/4/6/8, 16-bit 5/8/13/16, 32-bit 6/11/21/32. The 16- and
    32-bit widths are the PC's field encoding.
  - Floats go as their 32 bits; a bool as an 8-bit value.
  - A mark pads to a byte and is a plain 32-bit length.
  - Every call takes a count (an array).
- **Bustin' Out's** (found by the schemes' constants in its initialiser at `80231730`):
  - the schemes at `8035AFEC` / `8035B00C` / `8035B02C`;
  - `Recon8` `fn_8023037C`, `Recon16` `fn_80230530`, `Recon32` `fn_802306D0`,
    `ReconFloat` `fn_8023085C`, with the bool, int, mark and string calls beside them.
  Its ObjM loader is `fn_801FDE80`, found by its calls, which begin like The Sims 2's
  `ObjectModuleImpl::DoStream`: a bool (compression on), then 16/32/32/8/32/16/32/32/8/32/16,
  a mark, an int, ...
- **Measured:** on all 16 lots the first values decode sensibly (122, 4, 5; 107, 431,
  432; 269, 408, 409; ...). But the loader branches on the stream's version (46, 64, 66,
  67; the lots are 80) and loops over a list of objects, so a flat header is wrong after
  the fifth value.
- **Next:** iteration 5, `fn_801FDE80` followed exactly (the version-80 path, the object
  loop and what it calls per object), until all 16 lots' ObjM streams decode to their ends.

**Iteration 5** (chunk 1b, part four: the lots' objects):
- **The save stream, traced.** The observer wraps Bustin' Out's ReconBuffer functions
  (`BUSTIN_RECON_LOG`: each outermost call's kind, caller, position and values;
  `tools/recon_trace.py` names the callers). One run of the new-game route shows exactly
  what the loader reads, in place of reading every version branch of its code.
- **ObjM decoded** (`tools/objm.py`):
  - A bool (compression), then the object table in the world's list order. Each entry is
    the object id (16), two 32-bit values, a count (8) with that many 32-bit values, and
    the object's type (16), ended by id 0.
  - Then a chain of marks (each the position of the next) around each object's section
    (`fn_801F1E30`'s header first), and one more section for the lot itself.
  - All 16 lots decode to their ends: 4,572 objects, one section each plus the lot's.
- **The type is objt's field 4:** all 4,572 objects resolve to an object type, so to a GUID
  and a definition.
- **Checked against the running game.** The club (the last lot in the file) has 178
  objects. The object list the game walked (SetToNext, in the tree log) is the lot's table
  in exactly its order (164 of 164 compared), with the two objects made at load (179, 180,
  the household's Sims) at its front: new objects go first.
- **Next:** iteration 6, the native lot loader in the engine (C++: the container, LZSS,
  the stream reader, objt, the ObjM table), making the World's objects with their ids,
  order and definitions. Then each object section's data fields (`fn_801F1F20`), checked
  against the lockstep's first-seen values.

**Iteration 6** (chunk 1b, part five: the native lot loader):
- **The engine reads the lots** (`engine/src/save.cpp`):
  - the neighbourhood file's blocks and LZSS;
  - the save stream (`SaveStream`);
  - each lot's chunk table, object types (objt) and object table and sections (ObjM).
- **`World::load_lot`** makes a lot's objects in its table's order, each with its id, GUID,
  definition and file. `World::add(..., front)` puts objects made later in front, as the
  original's list has them.
- **`tests/test_lots.cpp` on the player's disc:** 35 blocks, 16 lots, 2,000 object types,
  4,572 objects (53 without a definition on the disc: the characters), and the club's first
  objects 178, 177, 176, 174, 172, 171. The same as the Python tools and the running game.
  The other tests pass.
- **`docs/lots.md`** writes the format up.
- **Next:** iteration 7, SetToNext natively (chunk 1) on a world loaded from the club lot:
  its kinds against the logged steps, the order and definitions now the lot's.

**Iteration 7** (chunk 1, SetToNext, on a lot-loaded world):
- **Native `set_to_next`** (`engine/src/primitives.cpp`): kinds 0 (any), 1 (Sims), 2 (not
  Sims) and 4 (a GUID) on the world's list. The target is the stack object or the variable
  the operand names. A lot's objects with no definition on the disc are Sims.
- **The lockstep runs it** on a world loaded from the lot, picked by the order of the
  original's longest walk. Ids can't pick it: every lot reuses small ids.
  - Kinds 0, 1 and 4: 36,063 steps the same.
  - 52 differ, all in ticks 0-6 while the lot loads, before objects the original makes
    then exist natively.
  - Kinds 3, 5 and 6 are not modelled (2,745 steps).
- **Found:** this session's first lot is the club too (its walk is the club's table), loaded
  at tick 0 and again at 2799, each time with two objects not in the lot (179, 180, not
  Sims, not people for kind 1) made in front.
- **Kind 6 is open.** It finds the DJ booth for operand 0x0BE8F446, which is no definition's
  GUID, and no definition or data field equals temp 0 or the parameter.
- **Next:** iteration 8, chunk 2: the objects made and removed (CreateObject 42, KillObject
  18, and those the engine makes at load), so that the native world has them when the
  original does.

**Iteration 8** (chunk 2, the objects made and removed):
- **No script makes an object in the session:** no CreateObject, one KillObject. The objects
  made are effects (`efx_smokepuff`), during Animate steps.
- **The original's object list:** each object's next at +188, its manager at +184, whose
  first object is its +44. Found by following SetToNext's virtual calls. The observer walks
  it every tick and logs each change ('L') and each head change after a script step ('C').
- **The rule:** a new object takes one more than the highest id in use (a removed id comes
  back) and goes in front (`World::make_object`, `next_id`). In the lockstep, all three new
  ids and the list's order at all five changes agree.
- **SetToNext with the list replayed:** kind 0 23,082 same; kind 1 2,144; kind 4 10,903,
  with 28 different while the lot loads (its list is still being built, in another order)
  and 3 later (the dance floor's multi-tile search, open).
- **Next:** iteration 9, chunk 3: each object's table sizes from its definition. Or, if
  that is not in the definition, from the lot's object sections, which hold the arrays:
  `fn_801F1F20`.

**Iteration 9** (chunk 3, the table sizes, and a lot's saved state):
- **From the stream's code and trace:** object data is 72 fields, temps 8, and attributes
  as many as the save gives. The attribute count is not the definition's field 56 (667 of
  4,519 match).
- **`World::load_lot` now gives each object its saved state:** attributes, temps and data.
  The loader checks data 11 is the id: all 4,572 objects of the 16 lots.
- **Checked:** the first read of each of an object's own variables, before any expression
  wrote it, is the lot's value 437 times. 13 others were changed first by something else.
- **Next:** iteration 10, chunk 4: the scopes not modelled (21, the stack object's
  definition, with the definitions now on the objects; 12, 16, 20, 23, 24, 32, 40).

**Iteration 10** (chunk 4, the scopes not modelled):
- **Scope 21, the stack object's definition:** the datum counts 16-bit words of the
  in-memory row from its 32-bit version, so it is the OBJD's field datum - 2. Found on the
  pool table's six parts, whose sub-indices scripts read. In the engine
  (`ScriptVariables`): all 2,221 reads of the session agree on the lot-loaded world.
- **Scope 12** is one shared value (8035ADC8, datum 15), set by scripts and by the engine
  (`World::ad_range`): 4,221 reads the same, 60 changes by the engine.
- **Left:** 16, 20, 23, 24, 32, 40: 583 lookups in all, mostly 32 (neighbour, 459) and 20
  (my slot, 78), whose systems (the neighbourhood, slots) come later.
- **Next:** iteration 11, chunk 5: the next primitives by use. Idle (0), then the script
  side of Animate (44) and others, checked step by step.

**Iteration 11** (chunk 5, part one: Idle):
- **Idle (0)** sleeps the stack for parameter N's ticks: one yield, then no steps at all
  until the wake. Then the parameter is 0 and the result true.
  - 11,950 of 12,257 wake-ups are exactly N ticks on.
  - 307 come later, by pause lengths: the sleep's clock stops while the game is paused.
- **In the engine:** `idle()`, the interpreter's wake time (`sleep_until`, `asleep`), and
  `tests/test_primitives.cpp` (Idle and SetToNext's rules, the next-id rule).
- **Next:** iteration 12, the last: the script side of Animate (44, 24,000 steps: its
  results, yields and variable writes such as the event local), and the loop's wrap-up.

**Iteration 12** (chunk 5, part two: Animate's script side; the last):
- **Animate (44):**
  - It yields every tick while its animation plays, then ends true (526 runs) or false
    (102).
  - With flag 0x20, an event ends it false with the event in local 0 (74).
  - 44 of 102 animations always last the same number of ticks.
  - Written up as the contract for the animation system (`primitives.md`). Not implemented:
    it needs the animation data.

## Where the second loop ended

Twelve iterations, twelve commits here (and the observer's in Bustin' Out). Chunks:

- **1 (SetToNext), done for kinds 0, 1, 2 and 4.** Kinds 0, 1 and 4 agree on 36,129 steps on
  a lot-loaded world, with the original's list replayed. Kinds 3, 5 and 6 are open: kind 6
  is unidentified, and 3 and 5 wait for multi-tile objects and neighbours.
- **1b (lots), added and done for the objects.** The neighbourhood file, the save stream
  and each lot's objects are in the engine:
  - their order, definitions and saved attributes, temps and data;
  - 437 first reads match the lot.

  Still open: each object's tree stack, the Sims' parts, and the lot's own section.
- **2 (objects made and removed), done for what the session does:** effects made during
  Animate, KillObject, the next-id and in-front rules.
- **3 (table sizes), done for objects:** 72 data fields, 8 temps, attributes from the save.
  Person data sizes are open.
- **4 (scopes):** 21 and 12 done. 16, 20, 23, 24, 32 and 40 are left: 583 lookups.
- **5 (primitives):** Idle done; Animate's contract written up.
- **6 (frames the engine starts):** not reached.

The world now comes from the disc, not the log: a native world made from the club lot gives
the original's object order, ids, definitions and saved state. The scripts' control flow,
expressions, variables, SetToNext and Idle agree with the original on that world.

Next, in order:
- the rest of each object's saved state (tree stacks, so the scripts running at load can be
  started natively, and the Sims' motives and person data);
- the simulation tick: which stacks run when, the sleep clock and pauses;
- then the animation data, so that Animate, and the effects it makes, can be native.

# The third loop

Planned 2026-11-09, after the second loop (above) and Castaway's survey (`survey.md`). Same
rules as the second loop: one chunk an iteration, checked against the recompiled Bustin'
Out, game runs only in the agreed light setup, one-line commits never pushed. The CPU budget
in `plan.md` applies to every system that comes in.

## Chunks, in order

1. **The rest of an object's saved state.** Bustin' Out's object sections after the data
   (`lots.md`, "Not decoded yet"):
   - the tree stack (`fn_8023F928`, frames `fn_8023E9A8`), so the scripts running when the
     lot was saved restart natively;
   - the Sims' parts (motives, person data, the further table);
   - the lot's own section.
   Traced like ObjM's start (`BUSTIN_RECON_LOG`), decoded in the engine, all 16 lots
   checked.
2. **The simulation tick, natively.** Which stacks run when, and in what order. The sleep
   clock (Idle) and pauses. Frames the engine starts (+8). Then a native run of the club lot
   for ticks on end, checked against the log step by step for the parts that are native,
   with the rest (routing, animation results) taken from the log.
3. **The benchmark** (`plan.md`, "The CPU budget"): headless, milliseconds per tick per
   system and the worst tick, on the PC. Then on the user's RK3566: ask the user for SSH
   access first, and build for aarch64.
4. **Castaway's object stream.** ObjM versions 120-130, read from its named code
   (`ObjectModule`'s stream and the `ReconBuffer` functions in its ELF). Done when all 28
   lots of its `default.ngh` decode to their ends, as Bustin' Out's 16 do.
5. **The next primitives by use** that need no new system: from the log's counts, each
   checked step by step.
6. **SetToNext's open kinds** (3 multi-tile, 5 neighbours, 6 unidentified) and the dance
   floor's multi-tile search, if the decoded object state explains them.

Stop after iteration 12, when the chunks are done, or when two iterations in a row end
without a commit.

## Progress

Iteration 12 of 12 (the loop ended).

- **Iteration 1, chunk 1 (the tree stack and the base object).** Read the loader's code for
  everything after an object's data: the tree stack and its frames, the +192 member, the
  pairs, the trailing values (`lots.md`, "An object's section"). The engine reads them
  (`SavedFrame`; each lot object's `Object::stack` holds its frames, resolved to trees).
  Measured: all 176 traced sections of the club end the base part at exactly the traced
  position; over the 16 lots, all 4,522 frames of objects with a definition resolve to a tree
  with their node in range and exactly its parameter and local counts. The frame's file is
  an index into a load-order list; the engine uses the object's file. Next: the class's own
  part (the Sims' motives and personality, the other classes'), then the lot's section.

- **Iteration 2, chunk 1 (the classes' parts).** Read the four classes' stream functions and
  found which class each object is from its definition (master id, type), as `lots.md`
  "The classes" gives. The engine reads every section to exactly its mark and gives the
  lot's Sims their saved motives and person data. Measured: all 4,572 sections of the 16
  lots end at their marks; the rule gives the traced class for all 177 in the club. In play,
  the club Sims' first reads: 22 of 24 motives and 81 of 102 person data fields are the
  lot's; the rest the engine sets at load (person data 27 and 32 to 1, motive 13), which the
  native load must model (chunk 2). Next: the lot's own section.

- **Iteration 3, chunk 1 (the lot's own section).** Read the loader's code after the
  objects: an object reference, a counted list of them, a wide string (`lots.md`, "The
  lot's own section"); the engine reads it and requires it to end the stream. A new trace
  (the recon log now flushes each record, as runs end by a signal) has all 178 club
  sections: every one starts and ends where the engine's reading does, and so do the lot
  section's three reads. All 16 lots end exactly at the stream's end; their lot parts are
  empty. Chunk 1 is done (the Arry chunks, floors and walls, wait for the systems that use
  them). Next: chunk 2, the native simulation tick.

- **Iteration 4, chunk 2 (the pass).** Read the tick's pass over the objects and measured it
  (`simulation.md`, `tools/tick_order.py`): each tick, ids 1 up to the highest at the
  pass's start, asleep objects skipped, each stack stepped until it yields. Measured: 5,128
  of 5,129 ticks run their objects in id order (the other: an object made in another's turn
  runs inside it); no pass stops partway; 200 runs are the engine's own (the pie menu's
  checks on a Sim's stack). The engine has it (`Simulator`, `test_simulator`). Next: a
  native run of the club over many ticks, checked against the log step by step, the
  primitives that are not native answered from the log.

- **Iteration 5, chunk 2 (the pass, natively over many ticks).** `engine/tools/replay` runs
  the club natively over a log (`simulation.md`, "Checked by replay"). The recomp's observer
  now logs the pass over the objects with its cursor (`Q`/`q`), which showed what the tick
  really does: the pass stops after a dialog's turn and the next goes on from there, and Idle
  sleeps a number of passes, not ticks. The engine has both. Measured on a 719-tick log: every
  tick's turns the native pass's, every stop where the log's, Idle the same natively every
  time (5,809), 67,001 steps with the native stack as the log's, 30,845 expressions the same
  natively (555 not). Also: the lot's Sims run template_person's trees; a return step's tree
  is the frame's (its logged nodes are the caller's). Next: the frames the engine pushes on a
  Sim's stack (its interactions), the rest of the replay's differences, then the benchmark.

- **Iteration 6, chunk 3 (the benchmark on the PC).** `replay ... --bench`: the native work
  per tick, five runs, medians (`plan.md`, "The CPU budget"). The PC: 12.3 µs a tick on
  average over the 5,130-tick session, 190 µs at worst past the load; the scripts 7.1 µs, the
  pass 5.2 µs. Then the RK3566 (the user gave SSH access): 125 µs a tick on average on one
  A55 core, 1.8 ms at worst past the load, 7.7 ms for the tick after the load; the pass costs
  as much as the scripts (59 against 67 µs). Next: make the pass cheap, then chunk 4.

- **Iteration 7, chunk 3 (the pass made cheap).** The simulator's objects in an id-sorted
  vector with their stacks, rebuilt when the world changes. The PC: the pass 5.2 → 1.6 µs a
  tick, a tick 12.3 → 8.7 µs; the replay's checks the same (every turn, every stop, Idle,
  67,001 steps). The RK3566 again when the user next has it on. Next: chunk 4, Castaway's
  object stream.

- **Iteration 8, chunk 4 (Castaway's object stream).** Read `ObjectModule::DoStream`,
  `cXObject::ReconStream` and their parts from the ELF's names (`lots.md`, "Castaway"); the
  decoder is `tools/castaway_objm.py`. Measured: all 28 lots decode to their ends, every
  section at its mark, 2,611 objects (2,339 plain, 271 multi-tile parts). No saved lot has a
  person or a door, so those parts are not read; Castaway's object files hold no OBJD, so
  the class is found from the data. Chunk 4 is done. Next: chunk 5, the next primitives by
  use.

- **Iteration 9, chunk 5 (TestObjectType).** The primitives the replay still answers from the
  log, by use: Animate 23,811, Dialog 2,475, CompleteGoal 2,377, IdleForInput 677, Random
  269, GotoRoutingSlot 206, LookTowards 189, TestObjectType 181, Update 168, Relationship2
  128, CallNamedTree 116, PlaySound 100 and fewer. TestObjectType needs no new system: it is
  native (`primitives.md`), all 181 uses the same (all false in this session; the true case
  in the unit test). Next: CallNamedTree and Random, which need no new system either.

- **Iteration 10, chunk 5 (Random).** Native (`primitives.md`, "Random"), with the engine's
  own generator as a listed difference (`plan.md`). Measured: all 339 uses true natively,
  every one of the 294 numbers the log shows within its range. CallNamedTree was looked at
  first: all 116 uses in the session go false without a call (the tree not found or not
  run), too little to read its other path from; left for a session that calls one.
  Next: chunk 6, SetToNext's open kinds.

- **Iteration 11, chunk 6 (SetToNext kind 3).** The replay's SetToNext differences by kind:
  3 (141), 5 (468), 6 (2,188), and 8 of kind 4 (the dance floor). Kind 3 read from the log:
  the next part of the target's multi-tile object in list order, round; the engine keeps
  each part's leader from the save. Native: all 141 the same. Castaway's
  `cXObject::TrySetToNext` names the rest: kind 5 is the next neighbour by id; kind 6 shares
  its case with 0 and 2 (its operand is the boombox's GUID, read little-endian; it finds
  other stereos). Next (the last iteration): kinds 5 and 6.

- **Iteration 12, chunk 6 (kinds 5 and 6, open).** Kind 5 walks the neighbourhood's 51
  neighbours by id (3-63, with gaps), false after the last. They are NBRS's 51 records
  (`objects.md`, "The neighbours"), the PC's layout; the id field of a record is not found
  yet, so kind 5 is not native. Kind 6 (2,188 steps) is still open: its operand is a GUID
  (the boombox) and it finds other stereos, through a case Castaway shares with kinds 0
  and 2.

## Where the third loop ended

All 12 iterations ran, each with a commit. Done:
- **Chunk 1, the saved state:** every object's whole section and the lot's own part, for
  all 16 lots, classes from the definitions (`lots.md`); the tree stacks restored onto the
  world's objects; the Sims' motives and person data loaded.
- **Chunk 2, the native tick:** the pass by id with its cursor, stopped by a waiting dialog;
  Idle counting passes (`simulation.md`); `replay` runs the club natively over a log and
  agrees on every turn and stop.
- **Chunk 3, the benchmark:** 8.7 µs a tick on the PC, 125 µs on one RK3566 core before the
  pass was made cheap (`plan.md`, "The CPU budget"); the RK3566 to be measured again.
- **Chunk 4, Castaway's object stream:** all 28 lots decode to their ends.
- **Chunk 5, primitives:** TestObjectType and Random native (Random's numbers the engine's
  own, a listed difference).
- **Chunk 6, SetToNext:** kind 3 native.

Open, for the next loop:
- SetToNext kinds 5 (NBRS's id field) and 6, and the dance floor's 8 kind-4 steps;
- CallNamedTree (only its false path in the logs), and the systems behind the most-used
  primitives still answered from the log: Animate, Dialog, CompleteGoal, IdleForInput,
  routing (GotoRoutingSlot, GotoRelative), LookTowards;
- the frames the engine pushes on a Sim's stack (its interactions), the replay's remaining
  step differences, and what the engine sets as a lot loads (person data 27 and 32, motive
  13; main trees restarted, e.g. the dance floor's 8216 for its saved 8220);
- Castaway's object definitions (its files have no OBJD), its persons' and portals' parts;
- the RK3566 benchmark again, with the cheap pass.

# The fourth loop

Planned 2026-11-09 for an unattended night. Same rules as the third: one chunk (or one
well-defined part) an iteration, checked against the recompiled Bustin' Out (lockstep,
replay), game runs only in the agreed light setup and one at a time, one-line commits never
pushed. No RK3566 runs (the user is away; ask first, as always).

## Chunks, in order

1. **The neighbours.** NBRS's records decoded (their id field, the person data's length),
   read by the engine; SetToNext kind 5 native, checked by replay.
2. **SetToNext kind 6.** Its handler in Bustin' Out's code (the primitive dispatch, then the
   case kind 6 takes), native, checked by replay; and the dance floor's 8 kind-4 steps.
3. **What the engine sets as a lot loads:** person data 27 and 32, motive 13, the main trees
   it restarts (the dance floor's 8216 for its saved 8220): from the loader's code and the
   log's first ticks, done natively by `World::load_lot` or a lot-start step.
4. **Relationship2** (opcode 26, 128 uses) on the relationships now decoded from the save,
   and **CallNamedTree** (28) from its code.
5. **The Sims' interactions:** the frames the engine pushes on a Sim's stack, from its saved
   queue and the log; the replay's "another tree" and depth differences.
6. **The next primitives by use** that need no new system, one an iteration.
7. **Castaway's object definitions:** where its OBJD data lives (datasets, quickdat or the
   ELF's tables).

## Limits

Stop after iteration 16, when the chunks are done, or when two iterations in a row end
without a commit (record why here). An iteration that finds a chunk needs a whole new system
records what it found and moves to the next chunk rather than starting the system.

## Progress

Iteration 9 of 16 (the loop ended).

- **Iteration 1, chunk 1 (the neighbours).** NBRS decoded (`objects.md`, "The
  neighbours"): 63 slots, 51 neighbours, read to the chunk's end, ids the slot's number plus
  one; the engine reads them (`read_neighbours`, `World::neighbours`). SetToNext kind 5
  native: all 468 steps the same. Chunk 1 done. Next: chunk 2, SetToNext kind 6.

- **Iteration 2, chunk 2 (SetToNext kind 6).** Read from Castaway's `TrySetToNext`: the
  next object whose data field 59 is the frame's first parameter; Bustin' Out's log agrees
  (field 59 equals it for 1,984 of 2,000 objects found). Native: all 2,188 and 549 steps
  the same; the stack object at step starts now differs at 2,498 steps, down from 4,652.
  The dance floor's 8 kind-4 steps are left, not looked at here. Next: chunk 3, what the engine sets as a lot loads.

- **Iteration 3, chunk 3 (the lot's load).** Measured (`lots.md`, "What the load does"): no
  tree is restarted (176 of 178 first steps at the saved top frame, the other two the
  deeper stacks); the lot's Sims are neighbours, by GUID. Person data 27 and 32 (set to 1)
  and motive 13 (computed) are the person system's (routing, motives in Castaway's code):
  recorded and skipped, as a system of its own. Next: chunk 4, Relationship2.

- **Iteration 4, chunk 4 (Relationship2).** The relationships saved with each object are now
  kept (`Object::relationships`; the club's 16, in pairs). Relationship2 itself: the
  commonest get is true exactly when the object has a saved relationship with the stack
  object (27 of 27), but the other operand forms need its modes, neighbours, create flag
  (Castaway's `TryRelationship2`): the relationship system, recorded and skipped
  (`primitives.md`). Next: CallNamedTree from its code, then chunk 5.

- **Iteration 5, chunk 4 (CallNamedTree).** From Castaway's `TryCallNamedTree`: a tree named
  by a string (table, file, index), called on the stack object. Native as far as the logs
  go: all 116 uses false and the same (the strings asked for do not exist); the call itself,
  never made in the logs, stops as not modelled. The library now reads string tables
  (`test_disc`). Chunk 4 done. Next: chunk 5, the Sims' interactions.

- **Iteration 6, chunk 5 (the Sims' interactions).** The replay's own errors fixed (saved
  frames below a stack's first step, returns into them, a rebuilt frame's file): on the
  719-tick log 68,273 steps the same and 314 not (were 67,001 and 1,572). What is left is
  where a Sim gets its interactions: in "Wait For Notify" at IdleForInput, the engine runs
  check trees and pushes the chosen interaction (`simulation.md`). That is the interaction
  queue and autonomy, a system of its own: recorded and skipped. Next: chunk 6, the next
  primitives by use.

- **Iteration 7, chunk 6 (PlaySound, Update, KillSounds).** Requests to the presentation,
  always true in the logs and never changing their frame: native as events queued on the
  world for the front ends (`World::presentation`, `primitives.md`). All 270 and 64 uses
  the same. Of the rest answered from the log, ChangeSuit (appearance), PushAction and
  IdleForInput (the interaction queue), GenericSimCall, Animate, Dialog, CompleteGoal,
  routing and SetMotiveDelta are systems. Next: chunk 7, Castaway's definitions.

- **Iteration 8, chunk 7 (Castaway's definitions).** Found: the quick data's ObjDefinition
  table, 1,335 rows of Bustin' Out's OBJD fields, big-endian (`lots.md`, "Castaway"); read by
  `castaway_objm.py --image`. 2,608 of the 2,611 lot objects read as their definition's
  class; 1 has no definition; 2 (type 9, swapped at load for a plain object) carry 7 bits
  more than a plain object, open. Chunk 7 done: all the fourth loop's chunks have been
  worked; the ones left open are systems of their own (`loop.md`, below).

## Where the fourth loop ended

All seven chunks worked in 8 iterations, each with a commit: SetToNext complete (kinds 5
and 6 native), the lot's load measured, relationships kept, CallNamedTree, PlaySound/Update/
KillSounds and Random native, the replay's own errors fixed (314 step differences left on the
719-tick log), Castaway's definitions found. What is left is systems of their own, the fifth
loop's.

# The fifth loop

Planned 2026-11-09, the same night, unattended. Same rules and limits as the fourth (one
well-defined part an iteration, checked by replay and lockstep, one-line commits, no RK3566),
but its chunks are systems, each built a checked piece at a time: a piece that cannot be
checked against the log is not built.

## Chunks, in order

1. **The interaction queue.** How a Sim's chosen interaction reaches its stack: the saved
   queue (`lots.md`, the Sim's part), IdleForInput (17) in "Wait For Notify", PushAction
   (13); the engine's push of the interaction's frame, checked against replay's pushes.
2. **Animate (44) as the scripts see it.** Each animation's length in ticks from the disc's
   animation data, the yields, the events (flag 0x20); checked step by step.
3. **The motives.** SetMotiveDelta (29), the motives' change each tick, motive 13; checked
   against the motive reads in the log.
4. **Dialog (36)** with a player-input interface (the replay answering for the player), and
   **CompleteGoal (56)**.
5. **Routing**, only if the floor and walls (the lots' Arry chunks) decode first: the Arry
   chunks, then GotoRoutingSlot / GotoRelative's results.

## Limits

Stop after iteration 16, when the chunks are done, or when two iterations in a row end
without a commit (record why here).

## Progress

Iteration 9 of 16 (the loop ended).

- **Iteration 1, chunk 1 (the queue's entries and their delivery).** The engine keeps each
  Sim's saved queue (`Object::current`, `queue`); an entry's fields named from the log: the
  Sim, the target, the tree (`SavedQueueEntry::tree`). The delivery read from the log
  (`simulation.md`): at IdleForInput, the prefix check and "hide menu" on the target, then the
  entry's tree pushed with the target as stack object. Checked: all 5 queued Sims' first
  interaction is their entry's, in both logs. Next: the delivery natively (IdleForInput
  pushing the entry's tree, the checks run by the engine), checked by replay.


- **Iteration 2, chunk 1 (the delivery natively).** IdleForInput delivers a queued entry
  natively: the target's TTAB names the checks (the table's first tree, then the
  interaction's guard: the "hide menu" seen before was one interaction's guard), run by the
  engine on their own stack (`Host::run_check`), then the entry's tree pushed. The library
  reads the TTABs (all versions; 9 and up packed as the save stream). Checked: all 5
  deliveries native, all 81 check steps the same, the pushed interactions the log's; steps
  the same on the 719-tick log 68,217. Next: what PushAction (13) queues, and the queue's
  later deliveries.

- **Iteration 3, chunk 1 (PushAction).** Native: an interaction from the target's TTAB (by
  place) queued for the stack object Sim (`simulation.md`). All 21 and 5 uses the same; one
  more delivery native (6), 101 check steps the same. Open: operand bytes 2-3, repeated
  pushes. Chunk 1 is as far as the logs show it; the Sims with nothing queued are autonomy's
  (a later system). Next: chunk 2, Animate as the scripts see it.

- **Iteration 4, chunk 2 (Animate: where the animations are).** Animate's index is the quick
  data's AnimRef row; the animations are animatio.arc's 4,013 named entries; the AnimRef id
  to name mapping is not found (`primitives.md`, "Animate": nine hashes ruled out). Without
  it no animation's length can be checked, so Animate is set aside, recorded. Next: chunk 3,
  the motives.

- **Iteration 5, chunk 3 (the motives: measured, not checkable).** SetMotiveDelta's operand
  read (`primitives.md`); but the logs show almost no motive change (1 of 47 variables), so
  nothing of the motive system can be checked against them. Recorded; a home-lot session is
  the log it needs. Next: chunk 4, Dialog and CompleteGoal.

- **Iteration 6, chunk 4 (Dialog).** Native, with the player's answer an input the front end
  gives (`World::dialog_answer`): open, wait and stop the pass, then true. The replay plays
  the player: all 2,471 waits and 4 answers as the log's, all 719 passes stopped as the
  log's. Next: CompleteGoal (56).

- **Iteration 7, chunk 4 (CompleteGoal).** Its operand named from Castaway's
  `CompleteGoalParam` (`primitives.md`); all 2,377 uses false, asking about goals whose
  state is the player's progress: the goal system's, recorded and skipped. Chunk 4 done
  (Dialog native). Next: chunk 5, the lots' Arry chunks (floors and walls) before routing.

- **Iteration 8, chunk 5 (the Arry chunks).** Decoded (`lots.md`, "The arrays"): three
  64 by 64 grids a lot (elements of 1, 8 and 1 bytes), run-length coded in bytes; all 48 of
  the 16 lots read to exactly their size and their chunk's end (`read_lot_arrays`,
  `test_lots`). Their meaning (floors, walls, ground?) and routing over them next.

- **Iteration 9, chunk 5 (what the arrays hold).** The objects' headers hold their places in
  sixteenths of a tile; the first array, indexed x × 64 + y, has a non-zero value under every
  one of the club's 155 objects on the lot (the floors, it seems). Rotation and the header's
  exact fields are open (`lots.md`, "The arrays"). Routing itself is not started: the logs
  do not show where anything is, so a route could not be checked against them.

## Where the fifth loop ended

9 iterations, each with a commit. Done natively and checked: the interaction queue's delivery
(IdleForInput with the target's TTAB checks, 6 deliveries, 101 check steps), PushAction,
Dialog (2,475 steps, every pass stop); decoded: the lots' arrays (48 of 48), the Sims'
queues. Recorded and set aside, each for want of what to check it against: Animate (the
AnimRef id to animation mapping), the motives (they hardly change in the logs), CompleteGoal
(the goals' state), routing (no positions in the logs).

What the next loops need first is new logs rather than new code: a session at a home lot
(motives running, the household's own interactions, autonomy), and the observer logging
positions (an object's place as the original has it), so that routing and animation can be
checked. Both are runs of the recompiled game with small read-only observer additions.

## A home-lot log (2026-11-10)

`work/logs/home.log`: the household at Mom's house, from the user's save. Made by replaying
a recorded play session (`--replay=work/inputs/mom-live`: the launcher's input recording with
its card snapshot, cut before its last Start press, which paused the game) and letting the
game run on past its end with no input, the usual headless run of 330 s. 6,869 passes, about
3.8 minutes of play. Unlike the club's logs, the Sims' needs move here: of 37 motive values
read, 29 change (270 changes), Sims 12 and 237 losing theirs a point at a time and refilling
some, so the motive system can now be checked. The observer does not log positions yet.

# The sixth loop

Planned 2026-11-10 with the user, now that a home-lot log exists: `work/logs/home.log`, the
household at Mom's house (above), with the observer's `P` records (each object's place when
it changes, in sixteenths of a tile). Same rules and limits as the fifth: one well-defined
part an iteration, checked by replay and lockstep on a log (the home log and the club's), a
piece that cannot be checked is not built, one-line commits, never pushed, no RK3566.

## Chunks, in order

1. **Mom's house from the save.** The memory card's save holds the neighbourhood in the
   disc's chunk formats (16 `ObjM`, the `Arry` chunks, `NBRS`, `FAMI`, `HOUS`), so the
   readers should load it: find the save's lot blocks on the card
   (`work/inputs/mom-live/memcard_a.raw`), read Mom's house, and run `replay` over the home
   log from it (the replay taking a lot from the card, not only from the disc).
2. **The motives.** Their change each tick, SetMotiveDelta (29), and what refills them;
   checked against the home log's motive reads (29 of 37 values change, 270 changes).
3. **Autonomy.** What a Sim with nothing queued chooses (the interactions' advertisements
   and the scores), checked against the interactions the home log's Sims push.
4. **Routing.** A Sim's walk over the lot's floors and walls, checked against the `P`
   records (positions a sixteenth of a tile a tick) and GotoRoutingSlot / GotoRelative's
   results.

## Limits

Stop after iteration 16, when the chunks are done, or when two iterations in a row end
without a commit (record why here).

## Progress

Iteration 16 of 16 (the loop ended).

- **Iteration 1, chunk 1 (Mom's house read from the save).** The card's save is the
  neighbourhood file with big-endian block headers and chunk tables (`lots.md`, "Saves");
  the readers take either. New in the save: a type-2 definition (an NPC) makes a Sim, and a
  Sim's +1160, +1180 and +1120 lists (the last the motive deltas). Checked: all 4,582 objects
  of its 16 lots read to their ends (Python and engine alike), and Mom's house's 206 are the
  home log's first world list, in order. Next: replay over the home log from the card.

- **Iteration 2, chunk 1 (the replay at Mom's house).** `replay LOG ISO --card CARD` runs
  the save's lot (Mom's house) natively over the home log: 566,100 of its 581,110 steps with
  the native stack as the log's, all 6,869 passes starting where the log's do and 6,858
  stopping where they did. Scope 24 (the person with a neighbour id, by person data 31) made
  native: all 30,684 reads the same, and the expressions that differ fell from 31,967 to
  1,283. A frame with no tree now fails as a missing tree instead of crashing the
  interpreter (it did on a return into one). Next: the motives (chunk 2), from the saved
  deltas (the +1120 list) and SetMotiveDelta.

- **Iteration 3, chunk 2 (the motives measured).** The observer logs each person's motives
  when they change (`N` records; a person told by the pointer back at -0x640); a new home
  log, the same 581,110 steps. The motives' change is the engine's, not the scripts' (18
  script writes of 270 changes): every 60 ticks, by a rate for each motive, some the same
  for both Sims, some each Sim's own (personality), one proportional to the value
  (`simulation.md`, "The motives"). Next: the update's code (Castaway's `Motives::Sim`,
  `InitDecayRates`, the decay tables) and its rates, then the native update.

- **Iteration 4, chunk 2 (the motive update's code).** Found by a watch on Sim 12's motive
  5 in a replay (a fresh `GCN_WATCH` build of the recomp, `build-watch-cachy`; the old
  `build-watch` had the Bazzite paths): `fn_801D0504`, Bustin' Out's `Motives::Sim`, called
  by `fn_8022501C` each 60 ticks; what it does to motives 5-9, 14, 15 is in `simulation.md`.
  Its rates are set at run time (zero-filled data the DOL does not hold). Next: where they
  come from (a watch on one), then the native update, checked against the `N` records.

- **Iteration 5, chunk 2 (the motive rates' source).** A watch on a rate names its writer,
  `fn_801CFFB4`, which takes each by name from the global file's tuning tables (FCNS) with a
  DOL default. The FCNS chunks read (`Library::tuning`; all four on the disc, 57 values,
  each to its end); their values are the measured rates (hygiene 0.17, hunger 0.0021,
  bladder 0.3, fun 0.25, energy 180 / 16 / 30). Next: the rates derived in `fn_801CFFB4`,
  then the native update against the `N` records.

- **Iteration 6, chunk 2 (the motive update, native).** The rates `fn_801CFFB4` derives
  decoded; `update_motives` and `MotiveRates::from` (the tuning tables by name). Checked
  (`replay --card`, the `N` records): 75 update steps bit for bit, 14 with a script's change
  besides, none different. Next: when it runs (the caller and its gate, Sim 237's stop at
  tick 5042), so that the simulator runs it itself; then motive 3.

- **Iteration 7, chunk 2 (when the motive update runs).** The observer logs the update's gate
  (`U` records); a new home log, the same 581,110 steps. Every 60th of a person's turns, at
  the end of the turn, not for an NPC with its own file; native in the simulator. Checked:
  99 after-tick motive states as the log's, the differing scope-15 reads 338 → 23. Next: the
  running motive changes (SetMotiveDelta, the +1120 list), Sim 237's fun.

- **Iteration 8, chunk 2 (running motive changes).** SetMotiveDelta native (its operand by
  Castaway's `SetMotiveDeltaParam`; the change an hour's / 1800), the changes applied at the
  end of each turn. Checked: 703 of 704 after-tick motive states as the log's. At the club
  motives do not move by themselves (measured); the gate's test that decides it is not found,
  so `World::motives_run` stands for it, and the club logs' figures are kept. Chunk 2 done as
  far as the home log goes (motive 3 and the asleep path are not in it). Next: chunk 3,
  autonomy.

- **Iteration 9, chunk 3 (autonomy measured).** Find Best Action is opcode 30, run by the
  Sims' own script (PersonGlobals 8233); the home log has three choices (Sim 12 at ticks 260
  and 1503, Sim 237 the easel at 1901); the other four interactions are the player's
  (`simulation.md`, "Autonomy"). Next: the advertisements in the TTAB entries (their motive
  fields), then the scores, checked as the log's choice among the native best four.

- **Iteration 10, chunk 3 (the advertisements).** The interaction tables kept whole
  (`Interaction`: flags, attenuation, threshold, an advert a motive), every file's by chunk
  id; an object's table is its definition's field 5. The three choices found with what they
  advertise (hunger +20, bladder +70, fun +10), each the Sim's need then. Next: the score
  (motives against adverts, attenuation by distance, the tuning table), checked as each
  logged choice among the native best four.

- **Iteration 11, chunk 3 (where autonomy runs).** Find Best Action is in PersonGlobals 8233
  "try autonomy" and takes a choice made by an evaluation 60-75 ticks earlier: ~145 guard
  checks over ~41 objects by id, each object's interactions in table order (ticks 186, 1443
  for Sim 12, 1847 for Sim 237; the same sequence both times for Sim 12). Next: the
  evaluation's candidates and guard checks natively, checked step by step against these.

- **Iteration 12, chunk 3 (the evaluation's candidates).** The 145 checks measured against
  the tables: objects by id, each one's advertising interactions in reverse table order, a
  version-11 table's first tree before them: 23 of 41 objects exactly. Open: the parts'
  tables (field 5 0xFFFF: the master's) and which interactions take part (flags, minimum,
  threshold). No engine code yet: the rule is not complete enough to check.

- **Iteration 13, chunk 3 (the candidates, native).** `autonomy_candidates` (objects by id,
  not type 7 nor a lesser part by field 6; the table by field 5 or the master's; threshold
  below 100, no flag 0x80, reverse order, a guard once); `Library::interactions_of`;
  objects keep their place. Checked: the three evaluations' 216 object checks the same in
  order. Next: the persons' candidates, then the guards' results and the score.

- **Iteration 14, chunk 3 (persons as candidates).** A person's table is its semi-global
  file's (person_sg for Sims, NPC_SG for NPCs); flag 0x40 keeps an interaction out too.
  Checked: the home log's three evaluations, objects and persons, all 421 checks the same in
  order. The club's bursts (about 4 checks each) do not match and are left open.

- **Iteration 15, chunk 3 (which candidates pass).** The checks' results read from the log
  (the bottom frame's return): 40, 41 and 42 pass in the three evaluations, each choice's
  guard among them (`simulation.md`, "Which pass"). The native guard runs need the
  evaluation inside the replay's turn; not built this iteration, as there is no checked
  piece of it smaller than that.

- **Iteration 16, chunk 3 (PushAction by the object's own table, tried).** PushAction taking
  the target's table by `interactions_of` makes one more of the home log's uses native, but
  the delivery it leads to then runs object 198's own guard (4202) where the original runs
  8248 and 265 first: a table's first tree is kept per file, not per table, and 198's table
  is not the one the lookup gives. It made the replay 3 steps worse, so it was reverted;
  recorded for the next loop (`Interaction` should carry its table's first tree, and 198's
  table is to be found).

## Where the sixth loop ended

16 iterations, each with a commit. Done and checked against the home log (Mom's house, from
the user's save):
- **The save read** (the memory card's neighbourhood: big-endian blocks, NPCs as Sims, a
  Sim's three lists) and the replay run from it (566,100 of 581,110 steps; scope 24).
- **The motives, native:** the update (Motives::Sim, its rates the global file's tuning
  tables FCNS), when it runs (every 60th turn, at the turn's end, not for NPCs), and the
  running changes (SetMotiveDelta): 703 of 704 after-tick motive states as the log's.
- **Autonomy's candidates, native:** all three evaluations' 421 guard checks the same, in
  order (objects and persons; tables by OBJD field 5 or the master's; the filter by
  threshold and flags).
- The observer: objects' places (`P`), persons' motives (`N`), the motive gate (`U`).

Open, for the next loop: autonomy's guards run in the turn and the score (the choice among
the native best four); routing (chunk 4, not started: the `P` records and the floors are
ready for it); at the club, what freezes the motives and what its bursts of checks are;
motive 3; a table's first tree per table (iteration 16).

# The seventh loop

Planned 2026-11-10 at the user's request, to go on from the sixth. The rules and limits are
the same: one well-defined part an iteration, checked by replay on the home log (and the
club's logs, kept as they are), a piece that cannot be checked is not built, one-line
commits, never pushed, no RK3566.

## Chunks, in order

1. **A table's first tree per table.** `Interaction` (or its table) keeps the version-11
   field, instead of the file-wide `prefix_of`; then PushAction by the target's own table
   (iteration 16's attempt), with object 198's table found. Checked: the home log's PushAction
   uses and the deliveries they lead to, with no step lost.
2. **The evaluation run natively.** In the turn the log's evaluation happens, the native
   candidates' checks run (`Host::run_check` against the log's steps of them), each result
   compared with the log's (40-42 pass in each evaluation).
3. **The score.** The passing candidates scored from their adverts, the Sim's motives, the
   distance (the objects' places) and the autonomy tuning table (FCNS 2); checked as each of
   the three choices among the native best four, and the ranking's stability across the
   three evaluations.
4. **Routing, first piece.** A Sim's walk to its chosen object (the `P` records: a sixteenth
   of a tile a tick) against the floors (the first Arry grid): the path's tiles, checked
   against the logged places.

## Limits

Stop after iteration 16, when the chunks are done, or when two iterations in a row end
without a commit (record why here).

## Progress

Iteration 16 of 16.

- **Iteration 1, chunk 1 (a table's first tree per table; PushAction by the object's
  table).** Object 198 is the group meal, whose table has action 4201 twice (guards 4202 and
  8248): a queued entry now carries its table index, tables keep their first trees, and
  PushAction and the delivery use the target's own table. Checked: one more PushAction and
  delivery native at Mom's house, 5 check steps as the log's, 3 differing steps fewer; the
  club unchanged. Next: chunk 2, the evaluation natively.

- **Iteration 2, chunk 2 (the evaluation natively, first piece).** The evaluation is opcode 3
  at "try autonomy" node 0 (its nested steps are the checks; it sets the stack object to the
  choice). `run_evaluation` native; the replay runs it against the log's steps (with
  `REPLAY_EVALUATION`): 153 results the same, 45 different, 235 not finished, all from the
  checks' opcode-28 calls (into another file's tree) not being made inside a check. Kept off
  by default so the replay's figures stay. Next: opcode 28 natively (the call), so the
  checks run through.

- **Iteration 3, chunk 2 (opcode 28's call).** CallNamedTree makes its call natively (the
  string's index from 1, the tree looked up in the stack object's files, a Gosub): 850 calls
  native at Mom's house, the club's 116 uses unchanged. In the evaluation (`REPLAY_EVALUATION`)
  the checks' results the same went 153 → 195, different 45 → 20 (218 not finished). Next:
  the next thing the checks stop at.

- **Iteration 4, chunk 2 (the evaluation's checks run through).** A call step's record is
  after its callee's: taken from there inside checks; trees resolved by the object's own file
  (the paper girl's guard was missed). Checked (`REPLAY_EVALUATION`): 410 of 433 check results
  as the log's (3 different, 20 unfinished), one evaluation exactly; the defaults and the club
  unchanged. Next: the last differences, so it can be on by default.

- **Iteration 5, chunk 2 done (the evaluation, native).** The last differences were an object
  made during the session (201), which the native world lacks; its checks are set aside.
  Checked: all 433 check results the same, all three evaluations exactly the log's steps; on
  by default for the save's lot (matching steps 566,983 vs 566,104 without, differing 2,334 vs
  2,858), off for the club (worse there; open). Next: chunk 3, the score.

- **Iteration 6, chunk 3 (the score function, logged).** `fn_80267E9C` (Autonomy_Score) found
  by Castaway's shape, its arithmetic read; the observer logs each call (`A`: the motives, the
  adverts, the score; a new home log, the same steps). The scores alone do not rank the three
  choices first (5th, 5th, 3rd): the distance and the selection are next.

- **Iteration 7, chunk 3 (the choice's value).** The score's caller (`fn_802266BC`): (score −
  base) / (1 + attenuation × distance), kept above a minimum, into the list the choice is made
  from. On the logged scores with the logged places: each of the three choices among the best
  two (2nd, 1st, 2nd). Next: the base and the distance exactly, and the curves' data so the
  score is native.

- **Iteration 8, chunk 3 (the value's terms read).** The distance read exactly (`fn_801F1C18`:
  tiles from the two places, 20 a level), which is what iteration 7 tested; f27 = 1, f29 the
  family's or a visitor's minimum, f28 a function of its own (`fn_80267D48`). Next: the
  curves' data and f28 logged, so the score and the value are native.

- **Iteration 9, chunk 3 (the curves and the base, logged).** The observer dumps the motive
  curves (`W`) and the base score (`B`); a new home log, the same steps. The score recomputed
  from the curves: 105 of 111 bit for bit, 6 by the last bit. Next: the score native in the
  engine (and where the curves come from in the game's data), then the choice.

- **Iteration 10, chunk 3 (the score, native).** The curves are the global file's STR# 501
  ("InteractionScoreCurves", text points); `score_curves`, `interaction_score`. Checked: all
  111 logged scores bit for bit, the 3 base scores too. Next: the value and the choice
  natively (the candidates' adverts from the tables, the distance from the places).

- **Iteration 11, chunk 3 (the value, native).** `candidate_value` ((score − base) / (1 +
  attenuation × distance)) in the replay's evaluation, the places synced from the `P`
  records. Checked: 102 candidate scores bit for bit (7 shifted past the set-aside object),
  the three choices at native ranks 2, 1 and 2: all among the best four. Next: the pick among
  the best four natively, and the evaluation's result made by the engine.

- **Iteration 12, chunk 3 (the choice, native).** The original's pick read (the end of
  `fn_802266BC`): the list sorted, entry `random % min(4, n)`; the observer logs the sorted list
  (`Y`) and the random number (`R`), a new home log with the same 581,110 steps. The distance
  needed the game's square root (two Heron steps, up to 4% over): with it every object's value
  bit for bit. Checked: the three choices made natively with the original's random numbers
  are the log's, the evaluation's stack object set by the engine; one list the same entire;
  the home figures (561,398) and the club's (338,305, 68,202) unchanged, lockstep's 7,902 too.
  Open: Sim 237's own interactions' values. Next: chunk 4, routing's first piece.

- **Iteration 13, chunk 4 (routing: the walls).** The second array's first byte is the walls,
  a bit an edge of the tile (1 −y, 2 −x, 4 +y, 8 +x; 0x10, 0x20 diagonal), a door's two parts
  opening the wall between them; the floors do not block (0 the grass). `Walls`, built as the
  lot loads. Checked on the home log's places: all 79 tile-to-tile steps of the Sims open
  natively, 2 through doors, none through a wall; the figures (561,398, 338,305, 68,202,
  lockstep 7,902) unchanged. Next: the path search, its tiles against Sim 12's logged walk to
  the fridge (tick 307 on).

- **Iteration 14, chunk 4 (routing: the route).** `Walls::route`, a breadth-first search over
  the walls (four ways, diagonal walls' tiles closed); windows (type 8 with field 3 set) no
  longer open their walls. Checked on the home log's 15 walks: 13 as long as the native route,
  9 on its very tiles; the other 2 go round a dining table and a counter (object blocking: not
  native yet, no OBJD field found for it). Figures unchanged (561,398, 338,305, 68,202,
  lockstep 7,902). Next: which objects block (their footprints), then the tie-break.

- **Iteration 15, chunk 4 (which objects block: not found in the data; the routing code
  found).** No OBJD field and no object data field separates the objects the walks avoid (the
  dining table, the counters) from those the Sims' tiles cross (the chairs, the newspaper,
  the wall objects), and the logged tiles are the smoothed line's (it clips corners) with a
  speed that varies, so blocking cannot be settled from the places. Nothing native built. The
  watch build found the routing code instead (named in the recomp: the place setter, the
  walker, GotoRoutingSlot and its worker, and 80222FF4, 18 calls, the planner it seems).
  Next: the observer logs the planner's result, checked against `Walls::route`.

- **Iteration 16, chunk 4 (the original's routes, logged and checked against the walls).**
  The observer logs `Route_Plan`'s vector (`G`): waypoints in sixteenths, a route broken at
  doors (planning by rooms, the third array, and their doors). A new home log, the same
  581,110 steps. Checked: all 17 plans open natively along their lines; figures unchanged
  (561,398, 338,305, 68,202, lockstep 7,902). Next: the waypoints natively.

## The seventh loop's end

Stopped after iteration 16, as its limits say. Done: chunk 1 (a table's first tree, PushAction
by the target's table), chunk 2 (the evaluation native, all 433 checks), chunk 3 (the score,
the value and the choice native: every logged score bit for bit, every object's value bit for
bit with the game's own square root, all three choices the log's with the original's random
numbers). Chunk 4 begun: the walls and doors native (every logged step and all 17 of the
original's planned routes open), a shortest tile route (13 of 15 walks as long), the original's
planner found and logged. Open: Sim 237's own interactions' values; which objects block; the
planner's waypoints natively (rooms, doors, lines); the walk's speed.


# The eighth loop

Planned 2026-11-10 at the user's request, to go on from the seventh, with a quarter of an hour between
iterations. The rules and limits are the same: one well-defined part an iteration, checked by
replay on the home log (and the club's logs, kept as they are), a piece that cannot be checked
is not built, one-line commits, never pushed, no RK3566.

## Chunks, in order

1. **The rooms and their doors.** The third array's rooms as regions, the doors joining them;
   checked: each of the original's 17 planned routes (`G`) breaks at a door between the rooms
   its pieces lie in.
2. **The planner's waypoints natively.** From the planner's code (`Route_Plan`, 80222FF4): the
   rooms' path through the doors, then the waypoints within each room; checked against the 17
   logged plans, waypoint by waypoint.
3. **What blocks.** How the planner closes tiles to objects (read in its code, as the data
   alone did not show it); checked as the plans that go round the dining table and the counter.
4. **The walk.** A Sim following its plan (`Route_WalkStep`, 801F0410): its speed and turns;
   checked against the logged places (`P`) tick by tick.
5. **Sim 237's own interactions' values**, which score lower natively than in the original;
   checked against the logged choice list (`Y`).

## Limits

Stop after iteration 16, when the chunks are done, or when two iterations in a row end
without a commit (record why here).

## Progress

Iteration 16 of 16 (the loop ended).

- **Iteration 1, chunk 1 (the rooms and their doors).** The third array is not the rooms (1
  inside, 7 outside); the rooms are the regions the walls close (`Walls::room`, a door left
  closed, a diagonal wall's tile in none): 4 at Mom's house, the 5 doors joining them.
  Checked on the home log's 17 plans: 13 within one room, 4 changing rooms only at a door
  with a waypoint either side; figures unchanged (561,398, 338,305, 68,202, lockstep 7,902).
  Chunk 1 done. Next: chunk 2, the planner's code read for the order of rooms and the
  waypoints within a room.

- **Iteration 2, chunk 2 (the routing slots).** SLOT read natively (version 10, the PC's 70
  bytes a slot; all 764 on the disc to their end, `test_disc`), OBJD field 18 naming an
  object's; GotoRoutingSlot's operand decoded (the number among the routing slots, from the
  object's own SLOT or the global one). Checked: the home log's 448 routing steps with such a
  slot all find it natively (81 more from a stack variable, 116 on objects not on the lot
  natively); the plans' starts all the Sim's place; figures unchanged (561,398, 338,305,
  68,202, lockstep 7,902). Next: the plan's goal from the slot (the object's place, its
  directions and proximity, the side nearest the Sim) against the 17 plans' ends.

- **Iteration 3, chunk 2 (an object's facing; the plan's last leg).** Resumed after the Rust
  port. A plan to an object ends with two waypoints: the tile the Sim stands on, then the
  object's own place (it turns to face it). An object's facing is its data field 1, in eighths
  (`Object::facing`); the tile in front (`routing::front`): 0 towards -x (the fridge, the
  counters, the stove), 6 towards -y (the easel), 2 and 4 by the turn between (not in the log).
  Checked on the home log: of the 7 plans that end at their routing step's object, 5 come from
  the tile in front of it; the other 2 are the dining chair's (approached from its side, to sit:
  its own slots' offsets); 10 plans end elsewhere (a door, a slot's side, the lot's edge).
  Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902); clippy clean. Next: the
  sitting and the any-side slots (the chair, the global slot 1's nearest side).

- **Iteration 4, chunk 2 (a routing slot's goal).** `routing::slot_places`: the places a slot
  offers, one for each direction its flags name (bit k k eighths from the object's front),
  at its optimal proximity; the goal the nearest to the Sim of those the walls let it reach and
  no other object stands on. The plan is the routing step's that walks (it yields, result 1;
  a step that ends only shares the tick). Checked on the home log's five such steps: three
  goals the plan's (the counter's front, the meal's open side by the fridge and stove, the
  chair's side nearest the Sim); the other two are slots of the paper girl, a person, whose
  facing the save gives (5) and her walk then turns. Figures unchanged (561,398, 338,305,
  68,202, lockstep 7,902); clippy clean. Next: a person's facing as it walks (the place
  records' direction), or the planner's waypoints within a room.

- **Iteration 5, chunk 2 (the planner's request, logged; the goals it is given).** The planner
  (`Route_Plan`) read as far as its request: a vector of 16-byte goals (place, cost, the
  direction's bit) passed to the search `fn_8024C5DC` (6 KB, its own search `fn_80249BB8` 10 KB:
  the waypoints are for later iterations), the one taken at the request's +88. The observer logs
  them (`H`; recomp tree_log.cpp; the new home log has the same 581,110 steps). Natively: a
  diagonal's parts are proximity × √½ cut to a sixteenth (11 for 16), and a slot offers only
  places in its object's room with no other object (persons do not count). Checked: three of
  the four single-proximity requests on native objects the native open places exactly, in order
  with their bits (the miss the paper girl's, her facing stale); the off-lot one fits a slot's
  shape; the planner's choice the native nearest in both requests natively taken. Figures
  unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: a slot of a
  range of proximities (the paper girl's area of 48 goals at tick 931), or the search's
  waypoints (`fn_8024C5DC`'s stages logged).

- **Iteration 6, chunk 3 (the planner's obstacles, logged).** The search's object loop read:
  each object's footprint (sixteenths) and cost (0x7FFFFFFF, or 10 by a flag; kind 8 adds side
  strips at 10000), a blocking one grown by half a global width (6: the waypoints' 3/16 from
  corners). The observer logs the gathered rectangles (`X`, at the empty `Route_ObstacleAdded`
  the loop calls after each object; recomp names.txt and hle.txt; the new home log has the same
  581,110 steps). Checked: all 709 rectangles of the 18 plans a footprint within a tile, grown as
  their cost says. Measured against the native world (not yet built): 226 rectangles with a
  native object at their place, 483 without; 278 native objects in their extent covered, 1,194
  not (the dining table too). Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4,
  24); clippy clean. Next: which objects the search leaves out (`fn_802499DC`) and an object's
  footprint (its +696 virtual), to build the rectangles natively.

- **Iteration 7, chunk 3 (the obstacles' objects).** The observer logs the objects the search's
  loop asks about and whether each is left out (`J`, at `Route_SkipObstacle` 802499DC; three
  more home runs to find how its part names its owner; the new home log has the same 581,110
  steps). The kept objects pair one for one with the rectangles (one dropped where it overlaps
  the Sim), and the rectangles are stored turned ({y0, x0, y1, x1}; the reader now turns them
  back). Checked: all 701 rectangles of native objects lie on their object's native tile
  (persons: 6 on a side round their logged place), 8 of objects not native; the tick-1905 walk
  keeps 3 off them. Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy
  clean. Next: each object's footprint and cost from its definition, and which are left out,
  to build the obstacles natively (rectangle by rectangle against `X`).

- **Iteration 8, chunk 3 (the footprints).** An object's footprint natively
  (`routing::footprint`): its tile less OBJD field 75, the footprint mask, nibble k the inset of
  the side `front(facing + 2k)`. Checked against the logged obstacles: 647 of the 694 of native
  objects (persons apart) exactly, the mask turning with the facing (the trees' quarters, the
  armchair, the jukebox); the 47 others the outdoor chairs (0x3333 but 4 in) and two of the
  game's own objects (mask 0, 8 at +4). Figures unchanged (561,398, 338,305, 68,202, lockstep
  7,902, 4, 24); clippy clean. Next: the cost (10 or blocking: the doors, the newspaper, the
  trees at 10) and which objects are left out, from the definitions, to build the whole list.

- **Iteration 9, chunk 3 (which objects block).** Found in the objects' own data, not their
  definitions: left out when the flags (data 8) have 0x4 or the object is in another (data 2);
  at cost 10, not grown, when the flags have 0x10; else blocking, grown by 3
  (`routing::obstacle`, `DATA_FLAGS`, `DATA_CONTAINER`). Checked: all 858 of the loop's
  questions about objects not persons, and all 98 costs; the native lists against the 12
  logged ones of as many as kept: 441 rectangles exact, 7 of objects not native, 34 the known
  footprint misses. Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy
  clean. Next: the persons' rule and the one dropped as overlapping the Sim (then the whole list
  native), or the chairs' footprint; then the search itself.

- **Iteration 10, chunk 3 (the obstacle by the Sim).** The loop's drop made native: an object
  whose footprint, before it is grown, overlaps the planning Sim's (6 on a side round its place)
  is not an obstacle (`routing::overlaps`, `ungrown`). Checked on all 17 logged lists: lengths
  the planner's in 16 (the 17th drops the paper girl, not native); 623 rectangles exact, 9 of
  objects not native, 78 not (44 the known footprints, 34 the 17th's shifted). Figures unchanged
  (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: the persons' rule (the
  loop's kind-2 branch), or the chairs' footprint; then the search over the obstacles.

- **Iteration 11, chunk 3 (a definition's width).** OBJD field 65 (TileWidth) narrows a
  footprint by (16 - width) / 2 on every side (the outdoor chairs' 14, the tron's and trunk's 8;
  `routing::footprint` takes it). Checked: all 694 footprints of native objects the planner's;
  16 of the 17 logged obstacle lists natively exact (but for objects not native), the 17th the
  paper girl's drop (not native). Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902,
  4, 24); clippy clean. Chunk 3 done but for the persons' rule (taken from the log for now).
  Next: chunk 2's search over the obstacles (the waypoints), or the persons' rule.

- **Iteration 12, chunk 2 (the planner's lines against the native obstacles).** Checked: all
  62 walked legs of the 17 plans clear of the native blocking obstacles' insides (the 9 others
  the last legs into the faced object); of the 54 inner waypoints 17 grown-obstacle corners, 16
  the goal, 16 on the line between their neighbours, 5 other. The search is corner to corner
  (a visibility graph). Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24);
  clippy clean. Next: the search's choice (read `fn_80249BB8`, or log its graph), or the persons'
  rule.

- **Iteration 13, chunk 2 (a route within a room).** `routing::corner_path`: Dijkstra over the
  corners of the grown blocking obstacles and of the walls (each closed edge grown by 3,
  `Walls::wall_rects`; lines checked by `Walls::line_open` and `segment_enters`). Checked: 11 of
  the home log's 12 single-room plans pass exactly the planner's corners in order (tick 1905's
  way round the table among them, decided by the walls' 3); the 12th (521) turns at a wall
  corner the planner passes differently. Figures unchanged (561,398, 338,305, 68,202, lockstep
  7,902, 4, 24); clippy clean. Next: the planner's in-between points (a sixteenth off the start,
  near corners), the walls' corners at tick 521, or the plans through doors.

- **Iteration 14, chunk 3 (persons among the obstacles).** The kind-2 branch read: a person is
  left out when it is the request's target or more than 3.0 tiles (the constant at 802CFAAC,
  read from the DOL) from the planning Sim. Checked: 17 of the loop's 20 answers about persons
  natively (target from the tick's walking routing step, places as logged); the 3 others the
  paper girl (a state test, `fn_801FD05C`, not read). Figures unchanged (561,398, 338,305,
  68,202, lockstep 7,902, 4, 24); clippy clean. Next: the planner's in-between points or the
  plans through doors (chunk 2), then the loop's end write-up.

- **Iteration 15, chunk 2 (plans through a door).** The corner search run on the 4 plans that
  change rooms: 3 by the planner's corners (the two crossings, tick 1901 to the back door); tick
  260 (yard to the front door) natively by the house's wall corners on a slightly shorter
  diagonal, the planner west then along the wall. A cost for crossing walk-through obstacles
  tried at several weights and not kept (it does not give the planner's line). Figures
  unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: the loop's
  end (iteration 16): the write-up of what is native and what is open.

- **Iteration 16 (the loop's end).** This write-up; no new piece (chunks 4 and 5 did not fit).
  Figures as at iteration 15 (561,398, 338,305, 68,202, lockstep 7,902, 4, 24).

## The eighth loop's end

Sixteen iterations (house-fire ecb1b73..4dda804; recomp ..b0a76ee), the Rust port between the
second and the third. Chunks 1 and 3 are done, chunk 2 in large part, chunks 4 and 5 not begun.

- **Rooms and doors (chunk 1, done).** The rooms are the regions the walls close (4 at Mom's
  house, 5 doors); a plan breaks at each door.
- **What blocks (chunk 3, done but a person's state test).** Logged at the planner's object loop
  (`X` the rectangles, `J` the objects asked about). Natively (`routing::obstacle`,
  `footprint`): an object is left out when its flags (data 8) have 0x4 or it is in another
  (data 2); it may be walked through at cost 10, not grown, when they have 0x10; else it blocks,
  its footprint (the tile less OBJD field 75's per-side insets, turned with the facing, and
  narrowed by field 65's width) grown by half the width, 3. The one overlapping the Sim is
  dropped. A person is left out when it is the target or over 3.0 tiles off. Checked: all 858
  answers about objects, all 98 costs, all 694 footprints, 16 of 17 whole lists (the 17th the
  paper girl, not native); 17 of 20 answers about persons (the paper girl's state test unread).
- **The planner's goals and route (chunk 2, in part).** Logged: the request's goals (`H`).
  Natively: a slot's places (diagonals at proximity × √½), only those in the object's room with
  no other object; 3 of 4 single-proximity requests exact (the paper girl's facing stale).
  `routing::corner_path`, the shortest line over the corners of the grown obstacles and walls:
  14 of the 16 plans by the planner's corners in order (11 of 12 within a room, 3 of 4 through
  a door); every walked leg clear of the native obstacles.
- **Open.** The planner's points between corners (a sixteenth off the start, along its lines,
  near corners), ticks 521 and 260 (wall corners), a slot of a range of proximities (an area of
  goals), the paper girl's facing and state test; the walk (chunk 4: speed and turns against the
  `P` places); Sim 237's own interactions' values (chunk 5). The routing is checked, not yet
  used by the simulator (the plans still come from the log), so the lockstep's figures do not
  move.
- **Figures** at the end: replay 561,398 / 338,305 / 68,202 steps the same; lockstep 7,902 / 4 /
  24 differences (all unchanged through the loop, as nothing native replaced a logged value);
  the home log now 581,110 steps with `H`, `X`, `J`.


## The eighth loop paused

Paused after iteration 2 at the user's request, to port the engine to Rust first (the ninth
loop): memory safety against the crashes and corruption the rules say to fix outright. Its
chunks 2 (from the goal on) to 5 continue on the Rust engine afterwards.

# The ninth loop: the Rust port

Planned 2026-11-10 at the user's request, its iterations back to back (the other loops keep
a quarter of an hour between). The engine and its tools move from C++ to Rust, behaviour unchanged: the C++ engine is moved to
`engine-cpp/` and stays the reference until the port is done, the Rust one is built in its
final place (`engine/`, a Cargo crate: the library, the `replay` and `lockstep` binaries, the
tests). One module an iteration, ported in the dependency order and checked before the next:
until the tools are ported, by the ported tests printing the C++ tests' very figures; after,
by the Rust `replay` and `lockstep` printing the C++ ones' very lines (timings aside) on the
home log (`--card work/inputs/mom-live/memcard_a.raw`), `tree.log` and `tree3.log`. Builds
through `tools/capped.sh` (`cargo build --release -j4`, `CARGO_TARGET_DIR=work/target`), no
crates from outside unless one is needed and named here. Rust's way, not a transcription:
owned data and indices instead of the C++ pointers, `Result` for the readers' errors,
wrapping arithmetic only where the original wraps, `mul_add` where the C++ has `fmaf`. Commits
one-line, never pushed; no game runs are needed (the logs exist); no RK3566.

## Chunks, in order

1. **The crate and the readers.** `engine-cpp/` moved (its build and the docs' commands with
   it), the Cargo crate set up; `iff`, `disc`, `bhav` ported. Checked: `test_disc`'s script
   counts (9,529 trees, every one read to its end) from the Rust test.
2. **The interpreter's core.** `expression`, `interp`, `primitives`; `test_interp`,
   `test_expression`, `test_primitives` ported, their figures the C++ ones'.
3. **The library and the world.** `library` (with the SLOT reader), `save`, `world`;
   `test_disc` and `test_lots` entire, their figures the C++ ones'.
4. **The simulation.** `simulator`, `motives`, `autonomy`, `routing`; `test_simulator`.
5. **The log and lockstep.** `tree_log` and `lockstep`: 7,902 differences on the home log,
   its every line the C++ one's.
6. **The replay.** `replay`: the home log's and the club's output the C++ one's, line for line
   (561,398, 338,305, 68,202).
7. **The C++ engine removed.** `engine-cpp/` deleted, the README, `plan.md` and the docs'
   commands and file names (`*.cpp`, `*.h`) pointed at the Rust ones.

## Limits

Stop when the chunks are done, after iteration 16, or when two iterations in a row end
without a commit (record why here). Then the eighth loop resumes.

## Progress

Iteration 12 of 16.

- **Iteration 1, chunk 1 (the crate and the readers).** The C++ engine moved to
  `engine-cpp/` (built with clang from there, its home-log replay output the very same as
  before the move); the Cargo crate in `engine/` (no outside crates), `iff` (the file owning
  its bytes, chunks as ranges into them), `disc` and `bhav` ported, a `Result` for each
  reader's error. Checked: the Rust disc test reads all 9,529 trees, none failing, as the C++
  `test_disc` (`BHAV read: 9529, failed 0`); clippy clean. Chunk 1 done. Next: chunk 2,
  `expression`, `interp`, `primitives` and their tests.

- **Iteration 2, chunk 2 (the interpreter).** `interp` ported: trees and files as indices
  (`TreeId`, `FileId`, into the host's trees and the library's files) instead of pointers, the
  host passed to each step instead of held, a primitive given the interpreter (its top frame
  the C++ `Frame&`) and frames' locals read by index. Checked: `test_interp`'s ten cases as
  Rust tests, every assertion the C++ one's, all passing (the C++ `test_interp: ok`); clippy
  clean. Next: `expression` and `test_expression`.

- **Iteration 3, chunk 2 (the expression primitive).** `expression` ported: the 16-bit
  arithmetic wrapping as the original's (`wrapping_add`, `_sub`, `_mul`, `_div`), the
  modulo's -32768 kept as its 16-bit store keeps it, a float's step converted as the C++ does
  (to 32 bits, then the low 16), an unknown operator an `Option` from the byte. Checked:
  `test_expression`'s 26 checks as a Rust test, all passing (the C++ `test_expression: ok`);
  clippy clean. Next: `primitives` and `test_primitives`.

- **Iteration 4, chunk 3 (the save's readers).** `primitives` works on the world and the
  library, so it moves after chunk 3 (with `test_primitives`). `save` ported: the save stream,
  the LZSS blocks, the chunk tables, the memory card's file, the arrays, the neighbours, the
  object types and ObjM's objects, each reader a `Result`. Checked against `test_lots`'s
  figures that need no library: 35 blocks, 16 lots, 2,000 object types, 48 arrays of 64 by 64,
  51 neighbours with ids 3 to 63, and the card's save of 1,318,912 bytes with 16 lots, all the
  C++ ones; clippy clean. Next: `library` (with `test_disc` entire), then `world`
  (`test_lots`' objects and classes), then `primitives`.

- **Iteration 5, chunk 3 (the library).** `library` ported: the files and their trees kept in
  vectors (`FileId`, `TreeId`), a semi-global file an `Option<FileId>`, an object's
  interaction table named by its chunk id (not a pointer into the map), a definition by
  `DefId`; the readers' strings Latin-1, byte for character, so names compare as the C++
  bytes do; a TTAB that stops reading keeps what it read, as the C++'s. Checked: `test_disc`
  entire as a Rust test, its output line for line the C++ one's (9,529 trees, 1,689 BCON,
  every tree run through the Rust interpreter: 1,251,290 steps, the outcomes 21, 1,740, 443,
  758, 1,921 and 4,646 alike; STR#, FCNS, TTAB and SLOT alike); clippy clean. Next: `world`
  and the rest of `test_lots`.

- **Iteration 6, chunk 3 (the world).** `world` ported: the objects in a slot arena
  (`ObjectKey`, stable while an object lives) with the list's order and the ids' map beside
  it, so that the stack object and `me` can be one object without two mutable borrows;
  `ScriptVariables` borrows the world and the frame and finds `me` and the stack object by
  key each time; a cell's table an enum by owner (the C++ compared pointers). The lot's walls
  come with `routing` (chunk 4). Checked: `test_lots` entire as Rust tests, every figure the
  C++ one's (4,572 objects, 53 without a definition; 4,575 frames, 53 of characters, 4,522
  found and fitting; classes 1,754, 1,821, 944, 53; 16 lots' own parts, 16 club relationships;
  the club's first objects 178 177 176 174 172 171; the save's 4,582 objects, the first lot's
  206, 54 Sims, 15 motive deltas); clippy clean. Chunk 3 done. Next: `primitives` and
  `test_primitives` (which exercise `ScriptVariables`).

- **Iteration 7, chunk 2 (the primitives `test_primitives` checks).** The interpreter finds
  trees through a `Trees` trait (the library's, or a test's), so that a primitive can push a
  call without holding its host; a stack's sleep is its own `Sleep`, Idle given it and the top
  frame together. Ported: SetToNext, Random, TestObjectType, PlaySound/Update/KillSounds,
  Dialog, Idle and the motive deltas' step; each takes what it uses (the variables, the
  world, the object's id). CallNamedTree, IdleForInput, PushAction and SetMotiveDelta are
  checked by the replay alone, so they come with it (chunk 6). Checked: `test_primitives`'s
  checks as a Rust test, each the C++ one's (its `me`, outside the world there, is the world's
  object 5, the id it is given), all passing (the C++ `test_primitives: ok`); clippy clean.
  Next: chunk 4, `motives` and `simulator` with `test_simulator`.

- **Iteration 8, chunk 4 (the simulator).** `simulator` and `motives` ported. The C++ makes a
  host for each object, each holding the world by reference; here the simulator keeps the
  stacks (by `ObjectKey`) and a tick is given one host that holds the world (`SimHost`: the
  world, whose turn it is, a turn's end), so nothing is borrowed twice. A turn asks the pass
  to stop through `World::stop_pass` (as Dialog does; the C++ tick reads it too), the
  simulator's own flag dropped. The motives' update uses `mul_add` where the C++ has `fmaf`.
  Checked: `test_simulator`'s nine checks as a Rust test, all passing (the C++
  `test_simulator: ok`); the motives' update is checked by the replay (its `N` records), in
  chunk 6; clippy clean. Next: `routing` (the walls, the rooms, the route) and `autonomy`.

- **Iteration 9, chunk 5 (the log and lockstep).** `routing` and `autonomy` have no tests of
  their own (the replay checks them), so they come with it in chunk 6. `tree_log` ported (a
  step's tree an index into the log's trees), `lockstep` as `src/bin/lockstep.rs`, and `cfmt`
  for the C formats its lines use (`%#x`, `%g`, a `snprintf` buffer's length). The C++ keyed
  a variable's cell by table pointer and index, every table of an object the world lacks the
  one null pointer; the Rust keys merge them the same way. Its one diagnostic of native
  addresses shows object keys. Checked: on the home log, `tree.log` and `tree3.log` its whole
  output is the C++ one's, line for line, those addresses aside (7,902, 4 and 24
  differences; 177, 139 and 157 lines); 1.2 s against the C++'s 1.4 s on the home log;
  clippy clean. Chunk 5 done. Next: chunk 6, the replay, with `routing`, `autonomy` and the
  four primitives it alone checks.

- **Iteration 10, chunk 6 (the replay).** `routing`, `autonomy`, CallNamedTree,
  IdleForInput, PushAction and SetMotiveDelta ported, and the replay as `src/bin/replay.rs`.
  The C++ made a host for each object, each sharing the replay's state by reference; here one
  host holds the world and that state and is told whose stack it steps. An interpreter keeps
  its own parameters (the original's +44; the simulator gives it the object's temps when it
  makes the stack, as the C++ host did), so a check run takes them from its caller. A
  candidate carries its object's key and its interaction's place in the table. The list's sort
  is libstdc++'s `stable_sort` as it runs (two halves in chunks of 7, then merged), since its
  comparator's near-ties make the result depend on the algorithm. One C++ behaviour found by
  the diff and kept: a frame reset to a semi-global tree no file names keeps the tree's own
  file (the club's 643 steps where the native stack then stops). The simulator's stacks are a
  vector by object key, and keys are never reused (`World::clear`). Checked: the replay's
  whole output on the home log, `tree.log` and `tree3.log` the C++ one's, line for line (236,
  133 and 119 lines: 561,398, 338,305, 68,202), and six of its studies (routing, autonomy with
  the distances, slots, diffs, turns, pushes) the same; lockstep still the same; the tests
  pass; clippy clean. Measured: 14.0 µs a tick against the C++'s 10.6 (`--bench`,
  interleaved), so the next part is its speed. Chunk 6 done. Next: the replay's speed, then
  chunk 7.

- **Iteration 11, chunk 6 (the replay's speed).** Profiled (`perf`): the timed steps spent
  most of the gap formatting the counts' texts, which `--bench` then drops, and in
  `World::find`'s tree map. The replay formats a count only when counting (a `note!` macro);
  the world keeps a direct index from id to object beside its ordered map (`find` one
  look-up). Checked: the replay's and lockstep's outputs on the three logs still the C++ ones'
  line for line; the tests pass; clippy clean. Measured (`--bench` on the home log, the two
  interleaved twice): 9.9 and 10.0 µs a tick against the C++'s 10.6 (the C++ figure includes
  its own counts' texts); of the engine's parts, the expression 1.84 against 2.20, SetToNext
  0.54 against 0.41, the pass 0.87 against 0.80. Next: chunk 7, the C++ engine removed.

- **Iteration 12, chunk 7 (the C++ engine removed).** `engine-cpp/` deleted; the README's
  commands are cargo's (the build, the tests, `lockstep`, `replay`), the docs name the Rust
  files (`engine/src/*.rs`, `engine/tests/*.rs`, `engine/src/bin/*.rs`), `plan.md` has the
  Rust measurement, `engine/target/` is ignored. Checked: from the README's own commands (the
  default target directory, built afresh) the tests pass (18), the home replay's output is
  the C++ one's saved before the removal, and lockstep's 7,902. Chunk 7 done.

## The ninth loop's end

Done in 12 iterations: the engine and its tools are Rust, no outside crates, and every check
the C++ engine had gives its very figures: the tests' (`test_disc`'s output line for line,
its 9,529 trees run through the interpreter in 1,251,290 steps), lockstep's and the replay's
whole outputs on the home log and the club's two (561,398, 338,305 and 68,202 steps, 7,902
lockstep differences), and six of the replay's studies. On the way: one C++ quirk kept on
purpose for the outputs to match (a frame reset to a semi-global tree no file names keeps the
tree's own file, so the club's native stacks stop 643 times there), recorded for the eighth
loop to look at; the list's sort is libstdc++'s `stable_sort` as it runs. Speed: the home
replay 9.9 µs a tick against the C++'s 10.6.

The eighth loop resumes from its chunk 2 (the plan's goal from the slot), its checks now with
`engine/target/release/replay` and `lockstep`, built with `cargo build --release` through
`tools/capped.sh`.

# The tenth loop

Planned 2026-11-10 at the user's request, to go on from the eighth through the night: a quarter
of an hour between iterations, as before. The rules are the same: one well-defined part an
iteration, checked by replay on the home log (the club's logs kept as they are), a piece that
cannot be checked is not built, one-line commits, never pushed, no RK3566.

## Chunks, in order

1. **The walk.** A Sim following its plan (`Route_WalkStep`, 801F0410): the distance a tick, its
   start and stop, the turns at waypoints; checked against the logged places (`P`) tick by tick
   along the logged plans.
2. **The planner's points between corners.** The sixteenth off the start, the points along its
   lines and near corners (`fn_8024C5DC`'s stages, `fn_80249BB8`); checked waypoint by waypoint
   against the 18 logged plans (`G`).
3. **The two route misses.** Tick 521 (by the counter) and tick 260 (yard to the front door): the
   walls' corners as the planner has them; checked as all 16 plans by the planner's corners.
4. **The rest of the request.** A slot of a range of proximities (the paper girl's area of 48
   goals at tick 931), the paper girl's state test (`fn_801FD05C`) and a person's facing as it
   walks; checked against `H` and `J`.
5. **The native routes in the simulator.** The routing primitives (GotoRoutingSlot,
   GotoRelativePosition) planning and walking natively instead of taking the log's places;
   checked by the lockstep's and replay's figures, which must not get worse.
6. **Sim 237's own interactions' values**, which score lower natively than in the original;
   checked against the logged choice list (`Y`).

## Limits

Stop after iteration 24, at 05:00 on 2026-11-11 (moved from 08:00 at the user's request), when the chunks are done, or when two
iterations in a row end without a commit (record why here).

## Progress

Iteration 4 of 24.

- **Iteration 1, chunk 1 (the walk keeps to its line).** The planning Sim's logged places from its
  plan to its next, against the plan's line (study `REPLAY_SHOW_WALKS`): 852 of 854 within 1.5
  sixteenths (the 2 others the paper girl moved off the lot after her walk); about 0.7 sixteenths
  a tick along straight legs, 3 to 6 every 4 ticks through doors. Figures unchanged (561,398,
  338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: the walk's speed and its start
  (`Route_WalkStep`, 801F0410: the distance a tick), against the places tick by tick.

- **Iteration 2, chunk 1 (the walk's speed, measured).** Cruising 0.82 to 0.85 sixteenths a tick
  over the 7 long walks (short walks slower: the start and stop), a first move 1 to 89 ticks
  after the plan. `Route_WalkStep` (801F0410) found to be an object's move to a given place (from
  `fn_801D3524`, a group's move), not the walk's step; the distance a tick comes from above it,
  likely the walk animation. No native walk built (the log does not give its speed exactly).
  Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: the
  caller that picks the walk's next place (log its distance a tick), or chunk 2 meanwhile.

- **Iteration 3, chunk 1 (the walker found and its steps measured).** Two observer records: `F`
  (each person's move through 801F0410, with its return addresses: 9,827 of 9,863 from
  800AE0A4) and `Z` (that follower's state, its float place stored turned, its speed factor and
  the frame's 1/30, before and after each step); recomp names.txt `Route_Follow`. The new home
  log has the same 581,110 steps. Measured: a step grows by 0.0432 sixteenths a frame (0.0376 for
  the paper girl) to 0.6547 (0.5702), times a factor (1 or 5); 1,416 of the 1,724 moving steps the
  top exactly, 46 the ramp exactly, 262 shorter (stops, turns, waypoints). The walk is per frame
  (about 1.25 frames a tick). Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24);
  clippy clean. Next: the step natively (the ramp, the top, the factor, the waypoint reached
  within a step) against every `Z` step; where the top and the growth come from (the walker's
  animation or the person's data).

- **Iteration 4, chunk 1 (the walker's step, native).** `routing::walk_step` and `along_line`:
  walking, the speed grows by its growth a frame to its top and near the end is held to
  sqrt(2 growth left); the fast mode (factor 5) at its top times 5, no ramp or brake; along the
  line round corners, to the end less a faced object's leg. Checked: 1,511 of the 1,700 logged
  steps on a plan's line natively the original's to a thousandth of a sixteenth (24 set aside).
  Figures unchanged (561,398, 338,305, 68,202, lockstep 7,902, 4, 24); clippy clean. Next: the
  tops (0.6547 adults, 0.5702 the child) from the person's data, Sim 237's 0.02, the doors' states.
