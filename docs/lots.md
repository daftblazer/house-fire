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

## Not decoded yet

- **An object's section:**
  - the header (`fn_801F1E30`: six 32-bit values, an int, a 16-bit value);
  - the data arrays (`fn_801F1F20`: 10, 8 (the temps) and the object data);
  - the tree stack (`fn_8023F928`, frames `fn_8023E9A8`);
  - the Sims' motives and the rest.
- **The lot's own section.**
- **The Arry chunks** (floors, walls and the like).
- **The two 32-bit values and the counted ones** of each table entry.
