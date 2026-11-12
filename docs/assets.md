# Assets

The data the game around the simulation uses (stage 4): what each archive holds and how it is
read, as found on Bustin' Out's disc. `engine/src/bin/assets.rs` reads them natively.

## Archives

`engine/src/arc.rs` (the layouts: `survey.md`, "Archives"). On Bustin' Out's disc 16 of the 18
`.arc` files are archives of this kind, 18,523 entries in all, each the same name, offset and
size as `tools/arc.py` gives; `scripts.arc` and `sounds.arc` are something else (the sound
banks).

## Textures

`textures.arc`, 2,032 entries, every one an `LFXT` record (`engine/src/texture.rs`):

| Offset | |
|---|---|
| 0 | `LFXT` |
| 4 | 7, big-endian (a version) |
| 8 | 12 zero bytes |
| 20 | the name, NUL-ended |
| h + 0 | the kind: 0x81 4-bit blocks (8 bytes for 4 by 4 texels: two 16-bit colours, two bits a texel), 0x82 16 bits a texel, 0x85 32 bits, 0x89 4-bit palette indices, 0x8a 8-bit ones |
| h + 1 | the bits a texel (4, 16, 32, 4, 8: always the kind's) |
| h + 2 | the width, 16 bits |
| h + 4 | the height |
| h + 6 | 0x20 for 16 and 32 bits, 0 for blocks, 0x220 for palettes (not read) |
| h + 8 | the palette's colours (16, 256) |
| h + 12 | flags: 0x0490, 0x0493, 0x04b3, 0x04d3, 0x0480 (wrap and filter, to be read against the original) |
| h + 14 | the mip levels, the full-size one counted (0 for one) |
| h + 16 | a float (1.0 with mips: a level-of-detail value, to be read) |
| h + 20 | the texels, each level after the last, halved each time; then the palette area, 4 bytes a colour: two palettes of 16-bit entries, the first the one the game loads (as IA8) |

(h is just past the name's NUL.) Every entry's parts fill it exactly: 1,043 of 16 bits, 854 of
blocks, 74 and 19 of palettes of 16 and 256, 3 of 32 bits, 38 with mip levels.

Checked against the original: the observer's 'x' record (each GXInitTexObj: the image, its size
and GX format, and an FNV-1a hash of its first level) on the home replay (`work/logs/home-x.log`,
581,110 steps as `home.log`): of the 215 images the original sets up, 148 are a `textures.arc`
entry's first level byte for byte, at its size, and the kinds are GX formats: 16 bits RGB5A3 (5,
55 images), blocks CMPR (14, 89), 4-bit palettes C4 (8, 2), 8-bit C8 (9, 2). The 67 others are
not in `textures.arc`: 18 frame-sized I8 (320x224 to 640x480: copies of the picture), 47 C8 of
other sizes (32x256, 32x64, 128x128, 8x256, 8x64, 16x32: from another archive, to be found),
one RGB5A3 256x256 and one IA8 4x4. `assets ISO texcheck LOG`.

## Run-length textures

`rletextu.arc`, 817 entries: the Sims' hair, faces and clothing, read by the original's
`ERRleTexture` (the decompilation's `src/sims/ERRleTexture.cpp`; `engine/src/rle_texture.rs`):

- a depth byte: 0x10 for a palette of 16 colours, anything else 256 (all 817 here: 256);
- the palette, 4 bytes a colour, each word's bytes reversed as it is read;
- the coded bytes' length (32 bits, big-endian), then the coded bytes.

The coding is a series of runs read at a nibble position: a count byte (it may straddle two
bytes), then for a count with its top bit set that many (the count negated, as a byte) indices
each its own, otherwise one index for `count` texels (a count of 0 still gives one: the count is
compared after each texel). An index is a nibble with 16 colours, a byte with 256. With 16
colours a literal run is always followed by a repeat, the next count's top bit not looked at.
The entry keeps no size: the original takes the texels one at a time. Decoded natively, every
entry gives exactly 65,536 texels, 256 by 256 (`assets ISO rle`). Where they go (the Sim's
256 by 256 textures, one RGB5A3 image among those the home replay sets up) is to be checked.

`datasets.arc` (the lots' and objects' data) keeps 5,636 more `LFXT` records inside its
entries; each reads at its tag (`texture::read_prefix`), all 5,636. None is one of the home
replay's 47 unmatched C8 images: those are set up at tick 0 and again in sets at ticks 25, 2,050
and 5,632 (the same seven contents each time: 8x64, 32x64 twice, 8x256, 32x256 twice, 128x128),
as a menu opens: the menus' own (chunk 8, `flashes.arc`), not the lot's. The lot's textures are
`textures.arc`'s.

Decoded natively (`texture::decode_level`, the GX formats: RGB5A3, CMPR, RGBA8; RGBA, red in
the low byte, row after row) and checked against the recompiled game's own GX decoder:
`assets ISO texbatch IN OUT` writes the first level of every texture without a palette (7,452:
textures.arc's and datasets.arc's) and its native decoding; `tools/texdecode_check.sh` builds the
recomp's decoder (gcn-recomp's `gx/texture.cpp`, as it is at check time) and decodes the same:
the 189,997,056 bytes the same.

The palettes: the observer's 'u' record (each GXInitTlutObj: the palette's format, count and
entries) on the home replay (`home-u.log`, 581,110 steps): 102 setups, 28 palettes, all of
format 0 (IA8, intensity and alpha: the colour comes from the draw). A palette texture's area is
two palettes of 16-bit entries, n each: the four palette textures the replay uses (2 C4, 2 C8)
are a logged palette's entries byte for byte in their area's first half. With the first half as
an IA8 palette (`texture::decode_indexed`) all 7,668 textures decode as the recomp's decoder does
(270,002,944 bytes). What the second half is for is not known yet.

## Models (surveyed)

`models.arc`, 1,843 entries, each beginning `00 01 00 00`, two zero bytes and the model's name
(NUL-ended), then a word 0x25 and the model's data; `shaders.arc`'s 2,132 begin with the word 13,
12 zero bytes and a name (a material each, by the textures' names). The engine draws through its
command list (`../bustin-out-recomp/docs/dev/native.md`, "The engine's command list"), but on
the home replay none of its strips of short arrays is recorded by `ERC_TriStripShort` or
`ERC_TriStripRefShort` (an observer hook on each: no call): the models are drawn otherwise,
likely as GX display lists (the list's opcode 6) over the arrays a GeometrySetup (opcode 40)
names, to be observed next.

The strips the device plays (the command list's opcode 56): the observer's 'w' record, from
`ERC_Play`'s loop (`../bustin-out-recomp/src/erc_play.cpp`; the handlers are called through it,
past the HLE table, which is why hooks on the recording and playing functions saw nothing),
writes each command's first 16 words and 32 bytes at each word that points into memory. The
command keeps its arguments inline: the opcode byte (0x38) with three more, the vertex count,
then the arrays' addresses. On the home replay (`home-w.log`, 581,110 steps; the first 4,000
strips): words 2 and 3 always arrays, their first 32 bytes found verbatim in `models.arc` for
1,424 and 1,842 of the 4,000; word 4 in 323 of 323; word 5 (always an array) never (made at run
time: lit colours?); words 6, 11, 14, 15 in part. With every archive scanned: word 2's bytes
are in `datasets.arc` or `models.arc` (or both) for 3,152 of the 4,000 strips, nowhere for 848
(made at run time: the Sims' skinned vertices?); word 3's for 3,224; word 4's 323 of 323;
words 6, 14 and 15 alike; word 11's in `rletextu.arc` for 456; words 5, 9, 10, 12 and 13 never.
The objects' models are in `datasets.arc` as well as `models.arc`. `assets ISO stripcheck LOG`
(study).

A model record (`models.arc`), as far as read: after the name (byte-packed, no alignment) the
word 0x25, 16 zero bytes, the float 1/4096 (0x39800000: the positions' scale, 16-bit positions
in 4096ths), then counts and floats (a bound) and the sub-models' data. A strip's positions
(16-bit x, y, z, 8 bytes a vertex) follow a 16-bit vertex count, which is the strip's own count
for 520 of the first 600 strips whose positions are found in `models.arc` (the other 80: their
first 32 bytes found at another place, or another layout); before the count, bytes 01 00 and an
attribute byte (01, 03) and a float. Next: the record walked from its start, sub-model by
sub-model.

The arrays after the positions (the same strips): the strip's word 3 array is the 4n bytes
right after the positions in 489 of them (4 bytes a vertex: three 8-bit normal components and
one more?), word 4 the 4n after that in 27 (texture coordinates?), word 14 at +16n in 19. The
byte 7 before the positions, first taken for the attributes, takes many values (0x01 to 0x34):
an index, not a set of attributes. The smallest record, `prop_card_spade` (218 bytes), read by
hand: one sub-model `01 00 2a 00 00 00 00`, a count of 4, four positions, 16 bytes of 4-byte
pairs, 12 bytes of 3-byte triples, then floats (its bound) and `01 01 01` with zeros.

A record of one sub-model, the simplest kind: the head (`00 01 00 00 00 00`, the name, then 0x36
bytes: the word 0x25, zeros, the positions' scale as a float at +0x15 past the name, and fields
not yet read), the sub-model's header (`01 00`, an index byte, four zeros, a 16-bit count n),
then n vertices' arrays end to end: positions (8 bytes: three s16 and two more), a 4-byte array
(the strips' word 3: texture coordinates?) and normals (3 bytes, three s8: the strips' word 4),
15 bytes a vertex, and a 73-byte tail (floats: a bound; `01 01 01`; zeros). 287 of the 1,843
records read this way to their last byte (the coffee mug: 115 vertices; the playing cards: 4).
The head varies in the others: a list before the scale (the effects' `efx_lightning`,
`efx_fire`: a count byte at +0x13 and floats), another scale (the shadows' 1/32768 at +0x15, a
null model's 1.0), and more sub-models: to be read next.

A sub-model whose first byte is 0 (the burn mark, `efx_burn_mark`; the shadows) has a 5-byte
header (`00 00 00` and the count) and more arrays: positions (8n), texture coordinates (4n: two
16-bit values in 4096ths, 0x0002-0x0ffe on the burn mark's quad), colours (4n, 0x80 each) and
normals (3n: 0, 0, 127), 19 bytes a vertex; then what reads as strip indices (`00 04 04 …`, small
16-bit values) and floats: a bounding box (min and max: -16384 to 16384 and 396 on the burn
mark's) and a radius (23170.5, its corner's distance). The card's tail of 73 bytes holds such a
box too (-963 … 6064). So which arrays a sub-model has varies, and its first byte is not just
"lit": the arrays' presence is to be read from the header next (against the strips' words: word
3 the 4-byte array, word 4 the normals, word 5 the run-time colours).

Read natively (`engine/src/model.rs`: a record's head and first sub-model, of the two kinds):
1,265 of the 1,843 records (434 of kind 1, 831 of kind 0). Checked against the strips the
original plays (`assets ISO modelcheck LOG`, the home replay's 'w' records): the 133 strips
whose positions are a read sub-model's have that sub-model's count, and their word 3 is its
texture coordinates byte for byte, all 133. (Word 3 is the texture coordinates, the 4-byte array.)

After the arrays: a kind-1 sub-model's record ends with its 73-byte tail (the byte 6, then floats:
the bound); a kind-0 one goes on with a 16-bit value a vertex (`00 85 00 85 …`, `00 f4 …`, runs of
equal values: a per-vertex index, a bone's or a group's?), then more. Against the strips: kind
1's word 4 is its normals as the record packs them (3 bytes a vertex; 57 strips). What a strip's
words are, from their addresses (`ASSETS_SHOW_DELTAS`): word 0 the opcode and flags, 1 the count,
2 the positions, 3 the texture coordinates (about 4n bytes below the positions), 4 an array or
none, 5 another below it (each copied to a scratch buffer, aligned, in front of the positions:
the record is not drawn from in place), 6 and 7 zero, 8 0x08000000, 9 and on further values and
addresses far off (the material's?). (An earlier count of kind-0 strips whose words 6 to 11
"point at the bytes after the arrays" was a false match: those words are not addresses, their
recorded bytes zeros, and those bytes often begin with zeros.) Some kind-0 reads are misreads (a
count of 0: a head of another shape, the Sims' parts and the maid).

The arrays settled against the strips (`assets ISO modelcheck`): a strip's word 2 is the
positions, 3 the texture coordinates, 4 the colours (kind 0; RGBA, the record's 4-byte array:
19 of 19 strips), 5 the normals widened to 4 bytes (kind 0: the record's three s8 and a zero,
19 of 19), and for kind 1, with no colours, word 4 is the normals as the record packs them
(57). Every array of both kinds of sub-model is so the original's, byte for byte, in the strips
the home replay plays from a read sub-model (133).

The vertex format: the word at +0x25 past the name is a mask of the arrays a sub-model has, in
this order after the positions: 0x08 texture coordinates (4n), 0x04 colours (4n), 0x10 normals
(3n), 0x20 a 16-bit value a vertex (2n); 0x02 is always set. Kind-1 records are 0x1a (no
colours) or 0x1e (colours); kind 0 mostly 0x1a, 0x3a, 0x3e (with the 16-bit values). Read by it
(`model.rs`), 354 kind-1 records read exactly to their end (the 73-byte tail: 287 of 0x1a, 67 of
0x1e; 80 go on, further sub-models), and against the strips: word 4 is the colours wherever the
format has them (57 of 57 kind-0 strips, and the kind-1 strips of 0x1e, whose word 4 an earlier
read took for normals), word 5 the normals widened to 4 bytes. Kind-0 reads with no plausible
format (0x1f9fa000 and the like, about 30) are other heads misread.

A whole record (`model::read_record`): the head to +0x19 past the name, then a node count (a
word), then each node: its parent (a word, -1 for the root) and its sub-model count, then its
sub-models, each a block of the format word, a 4-byte hash, the word 1, a zero byte, a float,
the header (kind 1: `01 00`, an index, four zeros, the count; kind 0: three bytes, the count), the
arrays by the format, and the byte 6; after the last node 72 bytes (the bounds). 442 records
read so to their last byte (1,182 nodes, 1,192 sub-models). Against the strips: 247 of the home
replay's come from one of their sub-models, all 247 with its count and texture coordinates
(sub-models can share their positions: the rubber tree plant's two, with other texture
coordinates; the check takes each that has them). The kind-0 records mostly do not have the 6 after their arrays: their 16-bit values
and more come first; other first bytes than 0 and 1 (2 to 254) are further kinds of sub-model.

Kind 0 is an indexed mesh: format bit 0x20 is not a per-vertex value but a GX display list
after the arrays: the vertices it draws (a word), the indices a vertex (a byte: one for each
attribute, 3 or 4), the list's size (a word, padded to 32 bytes), the word 3, then the list
itself: 0x9e (a triangle strip in vertex format 6), a 16-bit count, the indices (16 bits each,
position, texture coordinate, colour, normal in turn). E.g. the burn mark: 4 drawn, 4 indices,
0x40 bytes; the Persian rug 21 drawn, 3 indices, 0xa0 (21 × 3 × 2 + 3 = 129, padded). These are
the command list's opcode 66 ("a model drawn from a list made when it was loaded"). Read so
(`model::read_record`), 843 records whole (1,585 nodes, 2,267 sub-models). The display lists are
to be checked against the original's opcode-66 draws.

Against the original's opcode-66 draws (the observer's 'w' records widened to them;
`home-w66.log`, 581,110 steps): of 4,000, 858 carry as their word 6 a read sub-model's display
list byte for byte (its first 32 bytes), and 286 of those its positions as word 2; the other
3,142 are of records not read whole yet or of `datasets.arc`'s models (not read yet).

`datasets.arc` keeps models too, inside its entries (the objects' own): 1,926 heads, 1,289 read
whole there (`model::read_record_prefix`). With `models.arc`'s 843: 2,132 records whole (4,224
nodes, 6,093 sub-models), and 533 of the home replay's strips come from one of their
sub-models, all 533 with its count and texture coordinates. The opcode-66 figure is unchanged
(858 of 4,000): the other display-list draws are of models not read whole yet (or the Sims').

The head's entries: the byte at +0x13 past the name counts entries of 28 bytes kept before the
scale (0 for every object; 1 to 7 in 63 records, the scenes'): two words, 12 zero bytes, two
floats (the Create-a-Sim scene's: 0x3dc4, 0x3e, 1.0, 80.0; a camera's or a light's, not read
yet). The scale, node count and nodes follow them. Read so, 2,290 records whole (7,412
sub-models), the Create-a-Sim scene among them (one node, 13 sub-models, to its last byte). On
the home replay: every one of 4,000 opcode-66 draws carries a read sub-model's display list as
word 6 (3,428 its positions as word 2), and 4,247 strips come from a read sub-model, all with its
count and texture coordinates. Study ASSETS_SHOW_UNMATCHED_66 (the unmatched draws' lists,
searched for in the archives).

Batches. After a sub-model's hash comes not the word 1 but a count of bytes, then those bytes:
the matrix slots its vertices use (1 for an object, up to 11 for a Sim's part). Then, after the
float, the sub-model is a run of batches until the byte 6. A batch is its matrices (`01`, a
16-bit slot, its index in the batch; none for an object's mesh), three zero bytes, a 16-bit
count and the arrays (positions, texture coordinates, colours, normals as the format says).
A byte from 2 to 5 between batches makes the batches after it blended (the Sims' skins: 4
bytes a vertex, in an array after the others, whether the byte is 2 or 4; what its value says
is not known yet); a 3 with no batch after it appears too. The "kinds" 0 and 1 of the first reading are a batch with none and with one
matrix. Read so: 4,140 records whole (1,820 of models.arc's 1,843, 2,320 in datasets.arc),
26,996 batches. On the home replay, 7,962 strips come from a read batch, every one with its
count and texture coordinates; the 4,000 opcode-66 draws stay matched. Left of models.arc: 7
cursors whose display list runs past the end, 4 other cursors, 3 BUYMODE_CURSOR, 1 with weights
4 (sports_dive_high).

Float vertices. Format bit 0x10 is not the normals (every batch keeps them, 3 bytes a vertex):
with it, positions and texture coordinates are 16-bit (8 and 4 bytes a vertex); without it
(the cursors' formats 0x2a and 0x2e) floats: x, y, z and a zero (16 bytes), and two (8). The
matrix slots run to 82 (the whole-character records: NPCs, the promotion and career scenes).
Read so, every one of models.arc's 1,843 records whole, and 4,232 records in all (27,785
batches); the home replay's checks unchanged (7,962 strips, 4,000 opcode-66 draws, all
matching). Study ASSETS_SHOW_UNREAD.
