# Lots: where the world comes from

Bustin' Out's lots, families and neighbourhood come from `Runtime/default.ngh`. The game
never reads the PC-style house files in `Runtime/*/Houses/` (checked by mapping a run's disc
reads to files). The engine reads it in `engine/src/save.rs`; the Python tools are
`tools/ngh.py`, `lot.py`, `recon.py` and `objm.py`. Measured on the player's disc and the
recompiled game, 2026-11-09.

## The neighbourhood file

The file is made of 4 KB blocks, with 0xCD where a block is unused. A block with data starts
with seven 32-bit little-endian words:

- the section;
- 28, the header's size;
- the unpacked size;
- the packed size;
- a checksum;
- 4;
- a count.

The data that follows is packed with LZSS:
- flag bytes, read low bit first, 1 meaning a literal byte;
- references of two bytes: a 12-bit position (the second byte's high nibble its top) and a
  length of its low nibble + 3;
- a 4096-byte ring buffer starting at 0xFEE.

All 35 blocks unpack to exactly their stated size from exactly their packed size.

| Section | Blocks | |
|---|---|---|
| 0 | 1 | the directory: "TTAT", then 16 SIMI/HOUS pairs (FourCCs stored backwards) |
| 1 | 1 | the neighbourhood: NGBH, NBRS, the FAMI records |
| 2 | 17 | thumbnails (THMB) |
| 3 | 16 | the lots |

## A lot

A lot block starts with a chunk table: n FourCCs, n offsets and n sizes. The offsets count
from the block's start in the file, header included. The chunks are Arry (three of them),
objt and ObjM.

**objt, the object types.** 0, the version (2), "tjbo". Then for each type: a GUID, six
16-bit fields (field 4 is the type number the objects name) and a name (zero-ended, padded
to an even length). The list ends with a zero GUID. Over the 16 lots there are 2,000 types;
1,947 are definitions (OBJD) on the disc, and the rest are characters, whose definitions are
made at run time.

**ObjM, the objects.** 0, the version (80), "MjbO", then the engine's save stream:
- a bool: compression on;
- the object table, in the world's object list order. Each entry is:
  - the object id (16);
  - two 32-bit values;
  - a count (8) and that many 32-bit values;
  - the object's type (16, objt field 4).

  The table ends with id 0.
- a chain of marks, each holding the stream position of the next mark. Between them are the
  objects' sections, one per object in table order, then one more for the lot itself.

All 16 lots decode to their ends: 4,572 objects, all with a type, one section each.

The club (the last lot in the file) has 178 objects. The running game's object list (the
order SetToNext walks) is exactly the club's table, with the objects made at load (the
household's Sims, 179 and 180) in front. Objects made later go first.

## The save stream

The Sims 2's `ReconBuffer`, read from its code; Bustin' Out's functions were found by the
schemes' constants and are named `Recon_*` in the recomp's `recomp/names.txt`.

- Bits are read from the top of each byte.
- With compression on, a value is 0 (bit 0), or bit 1, a two-bit code and that many bits,
  sign-extended:

  | Values | Widths by code |
  |---|---|
  | 8-bit | 2, 4, 6, 8 |
  | 16-bit | 5, 8, 13, 16 |
  | 32-bit | 6, 11, 21, 32 |

- Floats go as their 32 bits; a bool as an 8-bit value.
- A mark pads to the next byte and is a plain 32-bit word.

Every family, house, object, Sim, tree stack and relationship is saved through it (The Sims
2's map lists their `DoStream` / `ReconStream` functions), and so are the memory card's
saves.

## An object's section

Read from the loader's code (`fn_801F1F20` and what it calls) and checked against the
recompiled game's trace of the club: all 176 traced sections end the base object's part at
exactly the traced position. Every condition below is on the stream's version (80 here); the
engine and `tools/objm.py` read version 80 only.

- The header (`fn_801F1E30`): six 32-bit values, an int, a 16-bit value.
- The attributes (a count, then that many), 8 temps and 72 data fields.
- A 16-bit value, always 0.
- **The tree stack** (`fn_8023F928`): a frame count and a capacity (512), both 32-bit, then
  the frames, bottom first. A frame (`fn_8023E9A8`):
  - the stack object (16; every saved frame has 0);
  - the tree id (16), as the object's scripts name it;
  - the node (16);
  - the counts of locals and of parameters (8 each);
  - the parameters, then the locals (16 each);
  - a 32-bit value, set for a frame the engine started (the frame's +8; none on the disc);
  - the frame's file (`fn_801F961C`), a 16-bit index into the game's list of loaded files.
    Within each lot every object of a type has the same index; across lots they differ, as
    the list is built in load order. The engine does not keep it: a frame's file is its
    object's.
- The relationships (the +192 member, `fn_8023243C`; Castaway names it `RelMatrixImpl`): a
  32-bit value `a`. If it is negative: a 32-bit value, then a list (`fn_80232E2C`: a count,
  then for each an int, and if that is set an entry). If not: `a` entries. An entry
  (`fn_802326CC`, RelArray) is the other object's id and a counted list of 32-bit values
  (`fn_80233084`). On the disc `a` is always -1; the club has 16 relationships, in pairs
  (119 and 125, 124 and 126 ...). The engine keeps them (`Object::relationships`).
- A list of pairs (`fn_801F22D0`, the class's virtual): a 16-bit count, then two 16-bit
  values each. Not named yet.
- A 32-bit value, then a 16-bit count and that many 16-bit values. Not named yet.

Then comes the class's own part (below), an int written by the loader, and the next mark.

## The classes

The loader makes each object of a class, and the class's stream function adds its own part
after the base object's. Which class follows from the definition (OBJD) of the object's type:

| Class | When | Its part | Over the 16 lots |
|---|---|---|---|
| object | otherwise | none (`fn_801F1F20` alone) | 1,754 |
| part of a multi-tile object | field 8, the master id, is set | `fn_801D3F50`: two ints, an int (v>57), three ints (v>39), a 16-bit value | 1,821 |
| part of a multi-tile door | that, and field 7, the type, is 8 | the part's, then (v>14) a count and that many floats (v>37) | 944 |
| Sim | no definition on the disc (a character) | `fn_80224C88`, below | 53 |

A Sim's part, for the versions after 63:
- its +1136 member (`fn_800AD6E8`): a 16-bit count of (bool, 32-bit) pairs, then a bool, a
  float and three ints;
- 16 floats at +168: its motives (scope 14);
- 16 more floats at +232 (to be named);
- 80 16-bit values at +8: its person data (scope 18; 64 before version 41). The person data
  and the motives are 160 bytes apart in the object, as scopes 18 and 14 are;
- three counted lists the disc never fills (`fn_8022933C`, `fn_8022989C`, `fn_80229D5C`;
  the engine requires them empty);
- the entry it is on and its queue: a count (up to 8) and that many entries. An entry
  (`fn_801CFB68`, 88 bytes): an int, a 32-bit id, three object ids (`fn_801FDD0C`), an int,
  four 16-bit values, an int, a 16-bit value, a float, and (v>61) an int of flags.

Every section of the 16 lots, read by its class, ends with the loader's int exactly at its
mark; in the club the rule gives the traced class for all 177 objects.

Checked in play (`lockstep`, the lot state): of the first reads of the club Sims' variables
before any script writes them, 22 of 24 motives and 81 of 102 person data fields are the
lot's. The rest are set by the engine as the lot loads (by tick 2): person data 27 and 32
become 1 for every Sim, and motive 13 is computed (56.2 for two); person data 33 becomes 100
later in play. The native load will have to do the same.

What the load does, measured further (`replay`, the 5,130-tick session):
- **No tree is restarted:** 176 of the 178 lot objects make their first step at the top
  frame their saved stack has; the other two are the stacks of two and three frames, whose
  first step is at the same tree and node (the log does not show the frames below it).
- **Each lot Sim is a neighbour** (`objects.md`, "The neighbours"): its type's GUID is one
  of NBRS's, the club's nine being neighbours 17, 34, 44, 46, 50, 51, 53, 58 and 59. The
  neighbour's saved person data is not what the load sets: it has 0 at 27 and 32 too.
- **Person data 27 counts the person's turns** (2026-11-17): one more at the end of each turn
  (a pass that reaches it), from the saved value; the scripts read and divide it (global 282,
  "divide tick counts"). Native (`Simulator::end_turn`, `world::PERSON_TICKS`): the reads of it
  that differ fell from 112 to 2 on home-full and from 232 to 6 on club-full (those six one
  dancer's, 178, whose count runs behind: not found yet). So the "1 at tick 1" below is the
  saved 0 after one turn, not a value the load sets.
- **Person data 27 and 32 are 1 for every lot Sim at tick 1,** before any script writes
  them: set by the engine. In Castaway, `SetPersonData` is called with 32 from the Sim's
  routing states (`Routing_Initial`, `Routing_Succeeded`, ...) and with 27 from its
  `Cleanup`; and motive 13 is computed (56.2 for a Sim whose saved one is 0). Both are the
  person system's (routing, motives), which comes as a system of its own.

Over the 16 lots: 4,575 frames, one per object but for one stack of 2 and one of 3. All
4,522 frames of objects with a definition on the disc name a tree that is there, with the
node in it and exactly its parameter and local counts (`engine/tests/lots.rs`). The
other 53 are the characters'.

## The lot's own section

After the last object's section and its mark, the loader (`fn_801FDE80`) reads:
- an object (a 16-bit reference, through the object manager's virtual, `fn_801FDD0C`);
- (v>51) an int count and that many object references;
- (v>63) a wide string (`Recon_String2`): on to an even byte position, an optional
  byte-order mark (bytes FE FF: little-endian units follow; FF FE: big-endian, the
  console's own order), then 16-bit units to a zero unit. (Before version 64, a string in
  bytes.)

On every lot both object fields are empty and the string is empty, written as FE FF 00 00;
where it starts at an odd position the byte before it is CD, padding the writer never set.
Every lot's part ends exactly at the stream's end, and in the club the traced reads
(`fn_801FDD0C`, the int, `Recon_String2`) start and end where the engine's do. None of it is
named yet, and the engine uses none of it.

## Castaway

The Sims 2: Castaway's lots (`R42E69`, `default.ngh`: 28 lots, ObjM versions 120, 129 and
130) are read by `tools/castaway_objm.py`, from its ELF's named code (`ObjectModule::DoStream`,
`cXObject::ReconStream` and what they call). It is Bustin' Out's stream two years on:

- **The table** is shorter: an object id, then for each object its type (16 bits: from
  version 47 `ObjectFolder::ReconSelector` streams an index into the objt, before it a GUID)
  and the next id, ended by 0. Then the same mark chain.
- **An object's section** (`cXObject::ReconStream`):
  - the header (`ReconHeader`): six 32-bit values, an int (from 25), a 16-bit value;
  - the attributes (a count and that many), then two more counted arrays (from 99 and 118);
  - 8 temps;
  - the object data as `EdithVariableSet`: a 72-bit set of the fields that are saved (two
    32-bit words and a byte), then one 16-bit value for each, in field order (Bustin' Out
    saves all 72);
  - (until 105) a 16-bit value;
  - the tree stack (`TreeStack`, `StackElem`): Bustin' Out's;
  - the relationships (`RelMatrixImpl` at +184: the same as Bustin' Out's "+192 member",
    which is so its relationships: a value, then a counted list of present-flagged arrays,
    each an id and a counted list of 32-bit values);
  - the slots (`ReconSlots`: Bustin' Out's pairs), a counted array of 16-bit values;
  - a multi-tile part's own (`cXMTObject`): Bustin' Out's part's, and a portal's
    (`cXPortal`) adds a counted list of floats; a person's (`cXPerson`) is the motives and
    a good deal more, not read (no lot has one);
  - the loader's int (from 54), the mark.
- **The lot's part:** Bustin' Out's.

The class comes from the definition, as in Bustin' Out (`ConstructObject`: a master id makes a
multi-tile part, of type 8 a portal; single-tile of type 2 a person; single-tile of type 8 or
9 none at all, and the table's reading first swaps a type 9's selector for GUID 0xB54AF253, a
plain object). Castaway's object files have no OBJD chunks: its definitions are the quick
data's `ObjDefinition` table (`quickdat.arc`, `SimsObjects`; `ObjectDataObjDefinition::
LoadFromQDataByID`), 1,335 rows of 196 bytes, big-endian, version 138 and the 96 fields of
Bustin' Out's OBJD (the GUID in fields 12-13, high half first). The tool also finds each
type's classes from the data alone: those whose reading ends every one of its objects'
sections at its mark, across the 28 lots.

Measured: every section of the 28 lots ends at its mark under its type's class, and every
lot's part ends the stream. 2,611 objects: 2,339 plain, 271 parts of multi-tile objects, 1 of
a type that reads either way (its values all zero). With the definitions
(`castaway_objm.py --image=DISC`): 2,608 read as their definition's class; 1 has no
definition (GUID 0x334945B8); 2 are the type-9 objects (GUID 0x437) swapped for 0xB54AF253, a
plain object, whose sections carry 7 bits more than a plain object's (they read as a
multi-tile part's zeros): still open.

## Saves

Bustin' Out's save on a memory card (file `G4ME69`, "Sims BO Save", 161 blocks) is the
neighbourhood file as the game holds it, the same size as the disc's `default.ngh`
(1,318,912 bytes): the same 4 KB blocks, LZSS-packed, sections 0-3 as the disc's (and one
more block, tagged 0xDEADBEEF), but with its block headers and the lots' chunk tables
big-endian and the FourCCs not reversed. The chunks' own contents are the disc's formats:
of the user's save of Mom's house, 14 of the 16 lots are byte for byte the disc's, and the
two the player had been to (the first, Mom's house, and the club) read with the same readers
(`memory_card_file`, `read_lots`; `work/momlot.py`). Its first lot's 206 objects are the
running game's world list when it loads, in the same order (`work/logs/home.log`).

What the save has that the disc's lots never do:
- **A person with a definition** (OBJD type 2, an NPC with a file of its own: the paper
  girl, `npc_papergirl.iff`) is saved as a Sim, not an object.
- **A Sim's three lists**, empty on the disc: +1160 (fn_8022933C) saves only its count,
  as each 168-byte element's read (fn_80249B80) reads nothing; +1180 (fn_8022989C, each
  fn_8021A92C) an object, a 32-bit value and a bool; +1120 (fn_80229D5C, each fn_8021A598) a
  32-bit value and two floats, which read as a motive, its change a tick and the value it
  goes to: in the club's five dancers (15, +0.025, 75), (7, +0.0083, −20), (14, +0.0139, 10),
  SetMotiveDelta's state.

All 4,582 objects of the save's 16 lots read to exactly their ends (the `lots` test with
`HOUSEFIRE_CARD`).

### Writing a save back (`save_write.rs`, `tests/save_write.rs`)

The file on the card (directory entry `G4ME69`, 161 card blocks of 8 KB in the block map's
chain, 1,318,912 bytes) is, in order: the banner, icons and comment (the first 16 KB), at
4 KB block 4 a block tagged 0xDEADBEEF (a thumbnail: its checksum is not the rule below, and in
the cards the game wrote during the recordings its header is garbage, so nothing reads it:
`file_blocks` keeps only sections 0-3 and the rest of the file stays as the template), then
section 0 (block 6), section 1 (block 8), section 2 (the 17 thumbnails, blocks 18-34) and
section 3 (the 16 lots, one in each 64 KB slot from block 66), each at a 4 KB-aligned place.
The bytes after a packed block, to the end of its 4 KB unit or its slot, are not filled: they
are what the game's buffer held (mostly zeros, with stale bytes among them), so a file is
written into a template (the original file, or zeros for one made from nothing) and only the
blocks' own bytes are replaced.

- **A block**: a header of seven words (section, 28, unpacked size, packed size, checksum, 4,
  count; big-endian on a card, little-endian on the disc) and the packed bytes. The checksum is
  the CRC-32 (zlib's) of the header with the checksum word zeroed, then the packed bytes: it holds
  for all 35 blocks of every card and the disc.
- **LZSS**: the packer is the classic encoder (Okumura's) with its binary tree of the ring's
  strings, a ring that starts as zeros, matches of 3-18 bytes (under 3 a literal), the first
  longest match met going down the tree (a newer equal string replaces the older in the tree).
  Measured: all 35 blocks of each of the five cards and the disc repack to the game's own
  packed bytes. A simpler packer (longest match, nearest or farthest on a tie) does not: 0 of 35.
- **A chunk table** (`encode_chunk_table`): the n FourCCs, n offsets from the block's start in
  the file (header included), n sizes, then (not in the earlier description) **n 16-bit values**
  (1 in section 0; in section 1 the families' and made Sims' numbers), the table padded to 4
  and each chunk padded to 4. The padding bytes are stale (`LotChunks::padding` keeps them); the
  FourCCs are stored backwards on the disc and not on a card.
- **The save stream** (`SaveWriter`): a compressed value is written at the smallest of the four
  widths (8-bit 2/4/6/8, 16-bit 5/8/13/16, 32-bit 6/11/21/32) that holds it as a signed number,
  and 0 as a single 0 bit. Measured over every value of every lot of the cards and the disc: no
  value is wider than that.
- **ObjM** (`encode_lot_objects`): the version header (0, 80, "MjbO"), the packing byte, the
  table, the chain of marks (each the position of the next, written by patching when the
  section ends; the last one is kept as read), each object's section by class, the lot's part
  and its string. What the readers had dropped, now kept: a frame's file index and its +8 word
  (it holds more than 0 and 1), the 16-bit value before the tree stack and the stack's capacity,
  the relationships' form (a count, or the negative head, a second value and a flag word for
  each slot), the 32-bit value after the pairs, each object's loader int, a part's six values
  and a door's floats, a Sim's +1136 member (and the bools of it and of +1180 as the bytes they
  are), the string's mark and the pad byte before it.
- **The arrays** (`encode_array`): a run of 8 or more equal bytes is a run entry (split at
  0x7FFF), anything else is a literal, runs and literals alternate, the pad bytes are 0. No
  run shorter than 8 is coded as a run in any of the 48 arrays of any file.
- **The other chunks**: objt (pad byte after an odd-ended entry kept), SIMI (the globals), NGBH
  (the words, the marks, and the bytes after them, not decoded, kept), NBRS (each slot's record,
  the empty slots by their index, the unnamed words of the record and of each relationship),
  FAMI (its unnamed words, and the bytes after the members), User (the names, the looks and the
  bytes after them). HOUS, TATT and DUMP (some saved lots have a DUMP chunk the disc's lack) are
  not decoded and are written back as the bytes they are; so is a thumbnail section.
- **The card** (`replace_card_file`): the file's bytes go into the card's block chain; the
  directory and the block map are not touched.

The figures (`tests/save_write.rs`, with the five cards and the disc): all 35 sections of each
(0, 1, the 17 thumbnails, the 16 lots) read into the models and written back the same, all 16
lots' ObjM on their own, all 35 blocks repacked to the same bytes, all 35 CRCs verifying, every
file assembled byte for byte and each card put back unchanged.

### Saving the world (`save_world.rs`, packet S2)

`save_card(world, card, lot, stacks)` writes the engine's World into a copy of a card: the
card's file as the template, the world's lot (block `lot` of section 3: ObjM and objt), its
SIMI (section 0) and the NGBH words and goal marks and the neighbours (section 1) rewritten from
the engine's state; the other lots, thumbnails and chunks stay as read. `CardLot::read` /
`CardLot::load` read a card's lot into a world as `replay` does without the log's load pass;
`reload_differences` counts the fields a world differs in from the one its card loads as
(`replay ... --save-at TICK OUT.raw` prints it; `--resume` with `--alone` runs on from such a
card with no load pass).

- **Kept as loaded** (`Object::loaded`, a `LotObject` with the lists the engine models emptied):
  the table entry's two serials and counted values, the section header's other words, the pairs,
  shorts, the words before the stack, after the pairs and the loader's int, a stack's capacity, a
  part's six values and a door's floats, a Sim's +1136 member and +1180 list, the relationships'
  form. Each frame's `file` index and +8 word are kept for a frame the run left as it was.
- **The header** (six place words, then 1, 1): the object's place and the box round it; the box
  moves with the object (a translation of the loaded words).
- **The +8 words of the frames**: word k says frame k + 1 was started by the engine (the load
  moves each up a frame); the top frame's word means nothing and is kept.
- **The file index of a frame** (`SavedFrame::file`) is an objt type number (not an index of
  files): 474 of the home card's 475 frames name the object's own type, the 475th the type of
  the object whose file the tree is in. The load now resolves a frame left without a tree by it
  (`World::load_lot`: the club's Sim 176 above a Sim-to-Sim interaction in the dream sequence's
  file); the replay and alone figures do not change. A frame the run pushed takes the object's
  type if the tree is in its file, else the first object of the list with that file: the club's
  frames in the dance floors' and the bar's files name another type of the file, by a rule not
  found (the index is only the file: the load reads it back right).
- **Neighbours**: the load copies a house-less family's neighbour motives into its Sims
  (`take_neighbour_motives`), so the save copies the Sims' back (all but motive 11, which the
  load zeroes). The figures compare motive 11, person data 32 (household 0, a visitor's 0 to 1)
  after the load's rule.
- **A new object** (made during play; `new_object`): its type is its GUID's in objt (a new one
  numbered after the last if the lot has none, its other fields the last type's; a Sim with no
  definition takes its neighbour's GUID); the unnamed values are those of a loaded object of the
  same type (else of the lot's first Sim, else zeros, capacity 512, loader's int 65536); the
  serials follow the lot's highest (`a`, `b = a + 1`, the word after the pairs `a`, as the home
  card's objects have them). Not known: what the original gives; nothing records a lot saved
  after play.

Figures (`tests/save_world.rs` with the five cards; `tests/alone.rs`; `replay --save-at`): a loaded
lot written back has the input's ObjM and objt byte for byte (all 32 lots of the home and club
cards; five cards in the checks), but the Sims whose motives the load took from their neighbours;
the world loaded from the written card is the world written: 0 differences at the load of every
lot. After play, run alone: 0 differences at ticks 0, 1000 and 5000 of the home and of the club
(club-live card, `--lot 15`). Log-driven: home 0 at ticks 0, 100, 2000; club 1 (person data 65 of
Sim 57, the log's 1 where the description says 0) at 100, 2000 and 4000.

The run alone's lot written at tick 0 is not the input's: the load pass has run. Home (206
objects): 184 headers (the tick-0 placement to the tile's centre), attributes of 12 objects,
temps of 2, person data of 3 Sims, motives of 1, data of 3 (what the load's calls set). Club
(card, lot 15; 178): 155 headers, 177 stacks (a community lot's stacks restart at their bottom
frame), 8 queues, motives of 1 Sim.

What a card does not hold, so that a run resumed from it differs from the uninterrupted one: the
generator's state (`REPLAY_ALONE_RNG` hands it over), the sleeps of the stacks asleep at an Idle
(at tick 1000 of the home 45 of 207 objects differ after one tick: the saved stack's first Idle
step wakes), the walks going on, the turn counts and the animation clocks. With the generator
handed over the home run resumed at 1000 still differs after one tick (the sleeps), so the
54,000 hash is not the uninterrupted run's.

## The arrays

Each lot has three Arry chunks: 0, a width and a height (64 and 64 on the disc), an element
size (1, 8 and 1), a 1 (32-bit words), then the elements' bytes, row after row,
run-length coded. A 16-bit header: with its top bit set, that many copies of the one byte
after it (padded to 2 bytes); otherwise that many bytes as they are (padded to an even
length). The counts are in bytes, whatever the element size. All 48 arrays of the 16 lots
read to exactly their size and their chunk's end (`read_lot_arrays`, the `lots` test).

Measured towards naming them: an object's header (`fn_801F1E30`'s six values) holds its
place in sixteenths of a tile: the dance floor's parts, 16 apart, are at their leader's
place plus 16 times their saved offset. Taking an object's tile as (header 5, header 4) / 16,
every one of the club's 155 objects on the lot stands on a non-zero value of the first
array indexed x × 64 + y (indexed the other way, 48 stand on 0): the floors, it seems, with
0 for none. Over all 16 lots, 2,342 objects stand on a floor value and 1,933 on 0 (the
yards), 297 outside the 64 by 64. Of the 1,838 parts of multi-tile objects, 686 are at their
leader plus 16 × offset (y, x), 441 at (x, y) and 711 at neither: the objects' rotation turns
the offsets, and what the six header values are exactly is still open.

The second array's first byte is the walls (`simulation.md`, "Routing"), indexed y × 64 + x.
What the rest hold is to be named. In the club: the first is mostly 0 (2,763 of 4,096) with
tile kinds such as 7, 33 and 18 (the floors?); the second, of 8 bytes a tile, is mostly
`00 0f 01 01 00 00 00 00` (the walls?); the third mostly 32, then 7 (the ground?). At Mom's house the third is 1 inside the house and
7 outside: inside and out, not the rooms (`simulation.md`, "The rooms").

## Not decoded yet

- The meaning of a multi-tile part's values, a door's floats, and a Sim's second table of
  floats, +1136 member and queue fields.
- **The arrays' meaning** (decoded, not named).
- **The two 32-bit values and the counted ones** of each table entry.

## A community lot in a save (2026-11-12)

The club-live save's copy of the club (lot 15) keeps its objects but not its house: its second
array's first byte (the walls) is clear on every tile and its first array (the floors) has 233
non-zero bytes to the disc's 1,333; the third (inside) is the same. The disc's copy has 209 tiles
with walls (250 wall edges, 6 rooms). The original plays the club on its walls: the club log's 13
planned routes all open through the disc's walls (doors included) and each within one room. The
world takes a house from the disc's copy of the lot when the loaded lot keeps no walls
(`World::take_house_from`); replay passes the disc's lot of the same number. The objects are the
save's: the disc's copy, tried whole, puts 16 stack objects off (its object order, SetToNext) though
it gives the dream controller's frame the original's (node 32, an Idle of 1, where the save has 9,
an Idle of 10000): which of the save's other state the original takes from the disc is open.

The controllers off the lot (its objects at -24, -24 that are not persons: the dream controller,
the party and NPC controllers, the social interactions, 22 on the club) are the disc's copy's too.
The observer's 'k' record (an interpreter's first 32 words at its first step, +8 its stack's
first frame, and the 512 bytes below that step's frame) shows the original's dream controller at
its first tick with 4104 at node 32 (the frame reads 0x9008, node 0x20), the disc's state, where
the save has node 9 in an Idle of 10000. With the controllers' stacks from the disc
(`World::take_house_from`), the club log runs with no differing step, a new club log of the same
428,688 steps (`club-k.log`, with 'k' records) likewise.

### What a load does to a saved stack (packet S3a, `eng-2000`)

The card the engine wrote at tick 2000 of the home run, loaded by the recompiled game (`eng-2000.log`),
showed what the game does with a saved stack: it takes each frame as the card says, and nothing else.

- **A frame's file index is the file** (`SavedFrame::file`, the frame's +12). The game finds a
  frame's tree in the file of the objt type that index names (`fn_8023E9A8` reads it back as it
  stands; `TreeSim_DoNodeAction`, 8023F1FC, looks the node up with it at every step). The writer
  had given the frames of a made object's file (Sim 12's 4196, 8200 and 8234 in the meal 202's
  file, a type new in the lot's objt) the Sim's own type, because the new type was added to the
  objt only when its object was written, after the Sim's frames (`save_world::lot_objects`: the
  types of the new objects are now taken first). In the game tree 8234 of file 89 is the person's
  "Kiss" (person_sg), whose node 3 has error for false: Sim 12's return from 8199 took the error
  and the game cancelled the interaction (the log's 8234:255, res 2, then the stack back at 8210
  and 4096), and the objects of that interaction (47, 202) ran on differently. Natively the same
  card read the frames in the object's file by a rule of ours (`load_lot`'s carried files); the
  load now reads each frame's file from its index first, and the old rule only for a frame whose
  index names no file with its tree (the figure of the old card, the log of the old card, is
  therefore not the figure to drive to 0: the log has to be recorded again with a card written
  by the fixed writer, `replay ... --save-at 2000`).
- **A return into a routing or idle node** (a frame at IdleForInput 17, GotoRelativePosition
  27, GotoRoutingSlot 45 with a frame above) takes no transition only when the frame's +8 word is
  set (`TreeSim_DoNodeAction` 8023F57C: the caller's +8 nonzero ends the step; a primitive's
  transition clears it, 8023F4D0). The routing primitives set it when they push the portal
  function, and the saved word carries it; the engine's writer writes 0 there (its flag is on
  the returning frame), so the game's load has +8 = 0 and the return goes the node's true or false
  way: Sim 12's 8207 returning false into 8218 node 74 (op 45) went to 255, where the native
  held the node. A loaded frame whose saved +8 is 0 now takes the transition
  (`Frame::plain_return`, cleared when the frame steps).
- **Not found**: the 49 steps of the made objects 47 and 202 (a sleep that the game counts down
  stepwise where the native wakes it, 202 restarted at 4193 node 0) and Sim 237's tick-4 steps
  (313:18 against 313:29). They follow Sim 12's cancelled interaction (the objects it used) in the
  old log; whether they remain with a log of the corrected card is not known.

### A card the game can load: slots, holders, new types (S3, `eng-2000`)

The corrected card made the game fault at load, then hang in the same place; what the save got
wrong, each checked on the game's saves and on its memory in the home run (`BUSTIN_TREE_PROBE`):
- **Slots** (`pairs`, `saved_slots`): entry s + 1 names the object in slot s; all 176 held
  objects of the home and club cards are there, nothing else past the first entry. They were
  written as loaded, so the card had 198 still in Sim 237's hand (it was on counter 54) and the
  meal 202 in no one's (Sim 12 held it). A type's count is its slot table's (+240, 36-byte
  entries, filled from the saved pairs by fn_801F22D0; 206 of 207 objects of the home run): one
  more than the kind-0 entries of the SLOT that OBJD field 18 names (616 of 617 types; 197 of
  197 with none have 1). The game's meal and trash pile have 2.
- **The holder** (`DATA_HOLDER`, data field 26): the game keeps the container there too (88 of 88
  held objects; the meal 202 in the home run at tick 1766: container 12, field 26 12). The
  engine left it 0; the load's room walk (fn_80243C24, through fn_801FCF04, the slot tree's
  next) then found no holder and faulted. Kept with the container now (`world::set_container`).
  This changes the home run alone (hash 0f9a65d4409162e0): its scripts read the field.
- **The header's last two words** are y then x (the box's middle): moved as x, y before.
- **A new objt type**: field 5 is the OBJD's field 7 (1,855 of 1,855); fields 0 to 3 differ
  between lots for one type (run-time figures), written 0 (a value the game's saves have).
- **A made object's relationships**: the form every saved object has (-1, -2, none).

Left open: with all of these the game's load no longer faults but spends minutes in the slot
walk (fn_801FCF04 and fn_801EF09C, from the room walk) and never reaches the lot. Without the
meal in Sim 12's slots it loads (and faults at tick 1, in Sim 12's slot test reaching the
meal). The group meal 198 held by Sim 237 on the home card loads; the single meal (a type new
to the lot) held by Sim 12 does not. Tried without effect: the new types' fields 0 to 3 as the
group meal's, 8 slots for the made objects.
