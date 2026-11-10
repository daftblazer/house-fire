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

Iteration 7 of 12.

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
