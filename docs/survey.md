# What the discs hold (2026-11-09)

Read from the USA discs without running anything.

## One engine line

All five executables carry "EOR Engine v2.0" (The Sims, February 2003; Bustin' Out,
November 2003) or "EOR/Maxis Engine v3.0" (The Urbz 2004, The Sims 2 2005, Pets 2006),
on Dolphin SDKs from late 2002 to 2004.

Compared function by function with Bustin' Out (functions of 16 instructions or more; "same"
is the same code with addresses and offsets masked, "close" a similar shape of
instructions):

| | Same | Close | ERC renderer same | Port's replaced functions same |
|---|---|---|---|---|
| The Sims | 26% | 9% | 0% | 43% |
| The Urbz | 50% | 7% | 71% | 93% |
| The Sims 2 | 39% | 7% | 67% | 78% |
| Pets | 32% | 6% | 57% | 76% |

The three v3.0 games share about 55% with each other in turn. The Sims has an earlier
version of the engine, its renderer compiled with assertions.

## Symbols: The Sims 2 ships its map

The Sims 2's disc holds `u2_ngc_release_dvd.elf` and three linker maps
(`u2_ngc_release_dvd.map` and others). The ELF's code is byte for byte the disc's
`main.dol`, and the map names all 18,556 functions with their C++ signatures and their
object files (583, the engine's source layout). The Urbz and Pets ship ELFs too, without
symbols.

Matched against the map, 2,350 of Bustin' Out's functions get their real names from code
that is the same; more by shape. The map shows the engine is the PC Sims' code carried over:
`cSimulatorImpl` (`SimulateOneTick`, `TickAllObjects`), `TreeSimImpl` (the behaviour
interpreter: `Gosub`, `RunCheckTree`, `DoNodeAction`), `TreeStack`/`StackElem`, `cXObjectImpl`,
`cXPersonImpl`, `cXPortalImpl`, `Behavior`, `treetab`, `objdefinition`, `iffresfile2`,
`routing`, `motive`, `neighborhood`, `slots`, and the console's own (`ERC` renderer, `Apt`
for the menus, `EResource` and its managers for the archives).

## The behaviour is data: SimAntics in IFF

| | IFF files | BHAV scripts | Other chunks | Where |
|---|---|---|---|---|
| The Sims | 0 | 0 (see below) | | `quickdat.arc`: `SimsObjects` |
| Bustin' Out | 742 | 9,529 (PC byte order) | OBJD 1,867, TTAB 960, TTAs, OBJf, SLOT, BCON, TPRP, TREE, TRCN, STR#, CTSS, GLOB, HOUS, FAMI, NGBH, ... | `Runtime/` |
| The Urbz | 1,092 | 10,192 (big-endian) | none | `edithtre.arc`, `datasets.arc` |
| The Sims 2 | 1,008 | 10,318 (big-endian) | none | `edithtre.arc`, `datasets.arc` |
| Pets | 1,127 | 10,744 (big-endian) | none | `edithtre.arc`, `datasets.arc` |

The IFF files are the PC's format, header and all ("IFF FILE 2.5:TYPE FOLLOWED BY SIZE ...
JAMIE DOORNBOS & MAXIS 1"). BHAV signatures are 0x8000-0x8002 as on PC. In the v3.0 games
only the scripts stay IFF; object definitions, interaction tables and the rest are the
engine's own records (to be decoded).

The Sims has no IFF on its disc. Its objects are in `quickdat.arc` as five packed entries
(`SimsObjects` 5.9 MB, `SimsTiles`, `CreateASim`, `SimsCareers`, `SimsUI`), uncompressed.
Bustin' Out has the same entries (its `SimsObjects` is 13 MB) beside its IFF files, so its
IFFs are the key to that format. (The Sims 2's map has `treetablequickdata.obj`: "quick
data" is a baked form of the IFF data.)

## Archives

Every `.arc` but the sound ones reads with one index format (`tools/arc.py`): a
little-endian word at 0 giving the index's offset; there a count, a word, then per entry the
offset, size, name and 12 bytes (a name hash, a time stamp, a word). Bustin' Out's
`audiostr.arc` has the index first instead. Bustin' Out: `animatio` 4,013 entries, `models`
1,843, `textures` 2,032, `shaders` 2,132, `rletextu` 817, `datasets` 486 (lots and objects),
`characte` 238, `flashes` 61 (menus), `particle` 138, `levels` 18.

## The interpreter's primitives

`docs/primitives.md`. The dispatch is a virtual `TryElement(StackElem *, short opcode,
XPrimParam *)` per kind of object (a binary-search switch), the cases calling `Try*`
methods. Bustin' Out's and The Sims 2's opcode numbers agree, and agree with the PC game's.
