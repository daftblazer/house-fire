# Lots: where the world comes from

Bustin' Out's lots, families and neighbourhood come from `Runtime/default.ngh`. The game
never reads the PC-style house files in `Runtime/*/Houses/` (checked by mapping a run's disc
reads to files). The engine reads it in `engine/src/save.cpp`; the Python tools are
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
- **Person data 27 and 32 are 1 for every lot Sim at tick 1,** before any script writes
  them: set by the engine. In Castaway, `SetPersonData` is called with 32 from the Sim's
  routing states (`Routing_Initial`, `Routing_Succeeded`, ...) and with 27 from its
  `Cleanup`; and motive 13 is computed (56.2 for a Sim whose saved one is 0). Both are the
  person system's (routing, motives), which comes as a system of its own.

Over the 16 lots: 4,575 frames, one per object but for one stack of 2 and one of 3. All
4,522 frames of objects with a definition on the disc name a tree that is there, with the
node in it and exactly its parameter and local counts (`engine/tests/test_lots.cpp`). The
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

## Not decoded yet

- The meaning of a multi-tile part's values, a door's floats, and a Sim's second table of
  floats, +1136 member and queue fields.
- **The Arry chunks** (floors, walls and the like).
- **The two 32-bit values and the counted ones** of each table entry.
