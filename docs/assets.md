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
entry gives exactly 65,536 texels, 256 by 256 (`assets ISO rle`).

### The Sim's run-time texture (T1)

Where they go: a Sim's texture, the 128 x 128 C8 image and its 256-entry palette (RGB5A3) that the
log has for each Sim ('`' and '{'), is made by the composer `Sim_SetModels` (800591CC) calls at
8005D018 (`engine/src/sim_texture.rs`, `engine/src/quantizer.rs`):

1. The layer stack, by the description (`layer_stack`; the original's 8005CA04 and the loads of
   8005D018): up to 15 resource ids in the order they are laid. The skin by gender, age and the
   description's byte 16 (the tone 0 to 2, any other the fourth: twelve ids); then the words 2
   and 3 of the part table's entries (`PartTables::entry`: the table entries keep the part's name
   hash, then these) of the upper body, shoes (word 2 only), lower body, eyes (3), nose (2), mouth
   (2), the ninth table (the hair of an adult male) and the head, with two fixed ids (0xC9E921E7
   and 0xD57882B9) among them. A layer with id 0 is left out. The layers' palettes are tinted
   by the description's colour bytes, in place (the original restores them after): the skin tone
   for the eyes, nose and mouth (4 offsets of hue, saturation and lightness, a grey kept);
   byte 18 / 19 for the upper and lower body (only when the Sim object's word at 0x5A0 is set: true
   for both logged Sims) and 20 for the shoes, a hue turn of a tenth for each three and darker, as
   is or lighter by the remainder by 0.15 (1 is untouched, 30 to 32 grey and 0.2); byte 17 for the
   hair (11 offsets, 1 untouched), byte 22 for the second fixed layer. The conversions are the
   character creator's (`CASSkin.cpp` in the decompilation: 80022940 and 80022778 are 8005E784 and
   8005E5BC), floats as the original's (fused multiply-add in the hue turn).
2. The 256 x 256 picture: each texel the first layer's colour with no alpha, then every other layer
   that has an alpha over it (8005FAC0): alpha 0x80 and over opaque, below it a weight of twice the
   alpha in 255ths, integer division by 255.
3. The blur to 128 x 128: the kernel 1 2 1 / 2 4 2 / 1 2 1 (sixteenths, +8) about texel (2x, 2y), the
   edges repeating. The original blurs twice (once to collect the colours, once to index) and its
   three row buffers are swapped by pointer: after the first pass's 127 swaps the second starts
   with two exchanged, so in the second pass only output row 1 has the picture's row 0 (not 1)
   before it. This one row is the whole difference between the two.
4. The quantizer (the object at 801B7464, `Quantizer`; 801B7538 to 801B8664, 0x1CF8 bytes): colours
   in an octree over a YIQ space (the NTSC matrix, the second and third axes scaled by a third,
   offsets 0, 127.5, 127.5; the cell centred (256, 256, 256), side 512, up to 10 levels), fed through
   a direct-mapped cache of 511 buckets (colour mod 511, counts; a colliding colour is flushed into
   the octree with its count). The tree has 579 nodes (0x7F00 gives (0x7F00 >> 3) / 7 - 1) and
   when 569 are used the deepest level is merged into its parents. At the end the cache is flushed
   and the tree pruned by node weight (counts shifted by 8 minus the level) until at most 256 nodes
   hold colours (the count of one pass includes what it merges); each colour-holding node's mean
   is a palette entry (children first), turned back by the inverse matrix, truncated to bytes,
   RGB5A3 (0x8000, five bits of each). A texel's entry is the nearest in the transform's space
   (squared distance, the first two squares fused with the third): by the cell the colour falls in
   and the entries below that cell's parent, or by a linear search for 16 entries or fewer. The
   indices go in the console's C8 tiles (8 by 4).

Checked against the log of home-a39.log (`LOCKSTEP_SIM_TEXTURES=1`, `lockstep`, the study
`engine/src/bin/lockstep/sim_textures.rs`): the cook (7 layers) and the male (the made Sim,
8 layers) are each the logged image and palette byte for byte: 0 of 16,384 texels and
0 of 256 palette entries differ (141 and 192 colours; the log's entries past them are 0). The only
other log with the records, home-a40.log, has the same two Sims (the same result); the club logs
and the other home logs have no '`' or '{' records. Not checked, for no Sim shows it: the other skin tones, children,
a Sim in a costume (the original's override record, `ESim` +0x598 and +0x59C), a Sim whose word at
0x5A0 is clear (its upper and lower body's colours not applied), and the tints of colours with a
green greatest and red least (the original's hue is not the inverse there: `rgb_to_hsl`).

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

## The command list around a strip

The observer's 16 words from a strip run past it: a strip (opcode 0x38) is 8 words (opcode,
count, positions, texture coordinates, colours, normals, two zeros), and what follows is the
next commands. For the Sims' parts: `0x22` and a word of four bytes, the matrix slots the next
batch uses (its `01 00 slot index` entries' slots, in order: 3,374 of 3,374 on the home replay);
then `0x28`, GeometrySetup (opcode 40: no arguments; its second word is padding, stale
bytes, the commands being whole 8-byte units); then the next strip. Study
ASSETS_SHOW_FAR_WORDS.

Where the materials come from. The home replay's command lists play, a frame: GeometrySetup
1,704, short strips 912, matrix slots 905, Terminate 822, display-list models 596, float strips
196 (BUSTIN_ERCSTATS): every mesh a list of its own, and no Texture (17) or Material (35)
command at all. A mesh's material reaches the device past the list, by its SetMaterial
(fn_801A63C0: up to two layers, each with a texture bound by the device), and GeometrySetup
makes it TEV stages (ERC_SetMaterialStages, the recomp's `src/erc_material.cpp`). Neither
the 0x28 word nor an opcode-66 draw's words carry the sub-model's hash (kept now, `hash`, for
the materials' check).

## Materials as the device has them

The observer's 'm' records (ERC_SetMaterialStages as it is called, before the draw it is for:
`home-m.log`, the replay's output on it the same as on `home-w66.log`): the device's layer
count, its two layers' 0x18 bytes, its two texture slots. A layer: a pointer, a word (8, 0x40,
0x48: flags), 0x18 or 0x1c, a float (0.5, 0.01, 0.03), then the bytes ERC_SetMaterialStages
reads (+0x10 mode, blend, blend-by, enabled, factor, unlit, uv source). A texture slot points
at the loaded texture, its LFXT header's flags (`04 93`) in its first bytes. With one layer the
second's bytes and slot are left from before (stale). Against the sub-model hashes of the
7,962 draws of read batches: 20 hashes (a hash is shared by many sub-models: a material's,
then, not a mesh's); one two-layer material is seen with every hash (a pass over everything:
not a mesh's own); 18 hashes have one material of their own besides it, 2 have two (their
texture differs). Study ASSETS_SHOW_MATERIALS.

The textures a draw uses. A material's texture slot is an in-memory texture object (`90 0d be
ef`, two code addresses, a data pointer; +0x10 width, +0x12 height, +0x18 the LFXT kind byte),
whose bytes reach the texels for 6 of 41 slots only. The observer's 'l' records instead take
each texture as GX loads it (GXLoadTexObjPreLoaded, before the draw it is for: the map and the
GXTexObj's 32 bytes, its image at +0x0C as the physical address >> 5): all 16,000 on the home
replay are an 'x' record's image, 9,658 named by their hash (an LFXT of textures.arc or
datasets.arc). Each draw of a read sub-model with the map-0 texture loaded last before it, by
the sub-model's hash: each of the 20 hashes one LFXT (cas_door, cas_sky_blue, rubber_tree_plant,
af_eyelashes, ...) besides two textures made at run time: one with most hashes (the common
pass's) and the Sims' skin (every `92964a4c` part). The hash is not the LFXT's name hash in the
archive index, nor in the LFXT: what it names is to be read (shaders.arc). Study
ASSETS_SHOW_DRAW_TEXTURES.

## Shaders

`shaders.arc`'s 2,132 entries (`shader::read`, all read to their size): the word 13, 12 zero
bytes, the name, then 78 bytes (the layer count, a byte, first; colours: nine floats of 1, then
10 and 1; not read yet), the layers of 23 bytes each (the texture by name hash, a flags word,
a word, the seven bytes the device's SetMaterialStages reads at +0x10, a float), then 28 bytes.
A sub-model names its shader and a layer its texture by the CRC-32 of the name in capitals
(`shader::name_hash`: `CAS_SKY_BLUE` 0x51a6fa33). The device's layer is the shader's, its texture
hash made a pointer and the float moved before the bytes.

Against the home replay (`home-m.log`): every one of the 7,962 draws of read sub-models names a
shader. Of the 574 drawn with one layer: 334 have the shader's first layer exactly (the factor
byte, +0x14, aside: the game sets it at run time, 0x80 for the shader's 0xff, a fade) and its
texture; 216 POLYSHADOW draws keep the draw before's material (none of their own is set);
Privacy_Window and Plate_Glass_Window (12 each) have their float halved (0.02 to 0.01) and,
the plate glass, flag 0x08 cleared: set at run time, not read yet. Two-layer draws are of the
common pass (its own material). Study ASSETS_SHOW_LAYER_DIFFS.

## The scene's matrices

The observer's 'p' records: GX's position matrices (GXLoadPosMtxImm, by id) and, from
ERC_ApplyState as each draw is set up, the render context's view (id 0xF0, when it changes) and
world matrix (0xF1, each time a model-view is made from it), 3x4 rows; 'j' the projections
(GXSetProjection: the type, its six floats). Their caps (20,000, 2,000) and the other draw
records' are spent from a tick on (`BUSTIN_TREE_DRAW_TICK`, 0 by default: `home-m.log` was such
a run, made again by running so): from tick 1,000 of the home replay (`home-d.log`, the
replay's output on it the same as on `home-w66.log`) they cover ticks 1,000 to 1,142: 1,191
views, 18,809 world matrices, 2,000 projections (910 perspective, 1,090 orthographic). The
world matrices' translations sit on the lot's tiles (14.5, 12; 15.5, 12; ...: tile centres,
in tiles). Against our world's objects: next.

Against our world. `lockstep` writes this world's objects as it reaches a tick
(`LOCKSTEP_DUMP_PLACES=TICK`, `LOCKSTEP_DUMP_OUT`: id, GUID, place in sixteenths, level, facing,
container, person, file), and `assets ISO scenecheck LOG PLACES TICK` sets them beside the
original's world matrices then. Home replay, tick 1,000 (lockstep the same, 0 differences): 1,035
world matrices, 172 objects on the lot (not held, not persons). An object's place is not where
it is drawn: 82 have a world matrix at the centre of the tile their place is in (the newspaper's
place 20.3125, 7.3125 drawn at 20.5, 7.5); the rest are drawn as their model's nodes, each its
own matrix (heights, small offsets: the trees' four parts, a quarter tile apart, swaying), and
objects on a tile's corner (the jukebox at 13, 16 drawn at 12.5, 15.5: its footprint). To read
next: a model's node transforms, and the footprint and facing that place a root.

A world matrix's 3x3 carries the model's own scale (its 16-bit positions' scale folded in:
0.001 to 0.004), so it is a rotation times that scale. At an object's tile centre its rotation
about the vertical is mostly 90° - 45° x facing (facing 0 at 90°, 2 at 0°, 4 at -90°, 6 at
180°: 164 of 213 such matrices at tick 1,000; the rest other objects' on the tile or nodes').

Which model an object draws. Each world matrix is followed by its draw, whose batch or display
list names its record (1,033 of 1,035 at tick 1,000 read). The records are named by the
catalogue's names (`Snoozemore_Alarm_Clock`, `Mulberry_Tree`), not the objects' files
(`electronics_alarmclock.iff`, `plant_tree_mulberry.iff`): none matches by file name. Datasets
`RD_<kind> - <name>` (`RD_Clock - Alarm`) keep an object's models, shaders and textures, its
model by name hash; `globals` keeps the objects' definitions (their GUIDs); `characte.arc` (238
entries, named after objects: `snoozemore_alarm_clock`) keeps their skeletons: the name, a bone
count, then bones (`Object01`, ...) each with a 4x4 (the node transforms a sub-model's matrix
slots index). What ties an object's file to its dataset and skeleton is to be found.

An object's dataset, a first lead. For some objects the interaction table's label names it:
"Clock - Alarm tree table" in `electronics_alarmclock.iff`, the dataset `RD_Clock - Alarm`;
"Stereo - Jukebox tree table", `RD_Stereo - Jukebox`. For 38 of the 172 objects at tick 1,000
that name is on the disc, and their draws agree with the placement: of 112 world matrices of a
model one of those datasets keeps, 31 are at the tile centre of an object of the dataset, all
31 turned 90° - 45° x facing. It is not the link: many tables are shared or misnamed ("debugging
tables", "Play Structure", "doors", "New Tree Table"), and lot datasets (`housepre01`) keep their
objects' models too. The objects' own link to their dataset (by GUID: `globals`' definitions, or
the program's table) is to be found. Study ASSETS_SHOW_NO_RD.

What a dataset holds, and how an object finds it. A dataset entry is its name, then sections,
each a name and its items by name CRC: `RD_Clock - Alarm` has Textures (4), Shaders,
ParticleTypes, Animations (6), Models (1: `36b9fdd5`, `SNOOZEMORE_ALARM_CLOCK`) and Characters
(1, the same CRC). `globals` keeps the characters' animation lists (rows of the character's CRC,
an animation's, an index). The object files hold no model name, CRC or dataset name (OBJD, CTSS
and the rest searched in every byte order). The decompilation's ESimsDataManager
(`src/sims/ESimsDataManager.cpp`: collectResInfoForSel) loads an object's resources by id: its
ObjSelector's material word +0x44 and its definition's extra word +0x10, ids the archive manager
finds by (fn_80176F6C: an id to an offset and size). 


An archive entry's id. An index entry is its id, offset, size, name (NUL-ended) and two words:
the id is the word before the offset (the first entry's right after the count), not one of the
words after the name, which an earlier reading took it as (one entry off). It is the CRC-32 of
the name in capitals (`shader::name_hash`) for 15,404 of the 18,484 entries of every archive
but audiostr.arc (`HOUSEPRE06`: `00189200`); the rest are named otherwise but found the same way,
by id. `arc::Entry::id`.

An object's dataset, as the original finds it. The observer's 's' records (a patch at
collectResInfoForSel's entry, 0x8003E898, writing the ObjSelector and its definition: 177 on
the home replay, all as the lot loads) show the chain: the selector's definition (+0x18, the
object's definition data, its GUID at +0x1c) points at an extra record (+0xC0) whose word +0x10
is a dataset's id; the selector's material (+0x34) has an id at +0x44, 0 for all but 2. The
pointers lie in the game's 12 MB heap at 0x7E000000 (`vm.cpp`), so the observer reads them
there too. Of 139 GUIDs, each has one extra id; 51 name a datasets.arc entry and they are the
right ones (the jukebox `RD_Stereo - Jukebox`, the alarm clock `RD_Clock - Alarm`, the cactus
`RD_Plant - Floor - Cactus`, the garden lamp `RD_Lamp - Garden`); the rest are 0 (controllers,
portals, the newspaper: drawn from elsewhere or not at all). 73 of the home lot's 113 GUIDs are
among them. `assets ISO selcheck LOG PLACES` (study ASSETS_SHOW_LINK, _SELECTORS,
_SELECTOR_BYTES, _SELECTOR_WORDS). The definition and its extra are
quick data: `quickdat.arc`'s `SimsObjects` image, tables `ObjDefinition` (1,778 rows of 196
bytes, the GUID at +0x1c, a pointer at +0xc0) and `ObjDefinition::ResData` (433 rows of 48 bytes:
the dataset's id at +0x10, the model's name hash at +0x18: `36b9fdd5`, `SNOOZEMORE_ALARM_CLOCK`,
for the alarm clock). `quickdata::object_resources` reads them natively (the image's fix-ups
make the pointer an offset): 460 definitions with resources, 372 naming an archive entry; for
all 139 GUIDs the original collected on the home replay, the same dataset id as its own (0
different).

The scene's draws by that link (scenecheck, home replay, tick 1,000). Of the lot's 172 objects
(not held, not persons) 42 have a model by quick data (`model_of`: the GUID's ResData model
hash, against a drawn record's `name_hash`); the rest have none (controllers, walls' and
floors' objects, the ones drawn from the lot's dataset). 587 world matrices draw one of those
42 models, and an object's model is drawn within 1.5 tiles of its place for 36 of them (6, the
brass wall lamps, the bathtub, the scooter, not drawn that tick). Only 25 matrices sit at their
object's tile centre (all turned 90 - 45 x facing): the rest are a node's matrix, the object's
times the node's transform (a mulberry tree: 395 matrices, its branches and leaves about
(9.0, 9.0) for a tree at (8.75, 8.75), each turned and scaled its own way), and outdoor objects
stand at quarter tiles, not tile centres. Study ASSETS_SHOW_UNDRAWN, _MISPLACED=NAME, _FAR.

## Skeletons

`characte.arc`'s 238 entries (`skeleton.rs`, all read to their ends; 236 with the id their
name's hash, so a model's record finds its skeleton by its name): the name, a bone count, then
the bones, the first named by the entry's name and the others each by its own: the parent (-1
for the root), a child count and the children, a translation (3 floats), a rotation (a
quaternion, 4), a flag byte and two 4x4 matrices; then a plane's name (`Plane05`) and five
floats. The objects' bones are at rest (no translation, no turn, identity matrices: the
fridge's eight, the alarm clock's fourteen).

A model record's node word (read before as a parent) is the bone its meshes hang from: the
cradle's 14 nodes on bones 1 to 14, -1 for a skinned mesh (whose batches name their matrix
slots). For all 211 records with a skeleton of their name every node's bone is one of its
bones and every matrix slot is within them (`assets ISO skelcheck`).

So the matrices a node is drawn with are the object's, times its bone's pose: at rest the
object's own, but the original's draws show the poses its animations give (the fridge's body
and doors drawn at their hinges, (25.39, 20.11), (25.64, 20.49), (25.39, 20.88) for a fridge on
(25, 20), each turned and scaled its own way): the pose is stage 4.4's (animation).

An object's own matrix, as the original sets it. The Sims 2's map names
`EIStaticModel::SetOrient(EMat4 &)` (its ELF, from the disc, matched against Bustin' Out's code:
`fn_80167AD8`, 78 of its 91 instructions alike, the rest offsets): it copies the 4x4 it is given
(rows, the translation last) into the model at +0x78 and +0xF8. The observer's 'o' records
(a patch at its entry): the tick, the model's address, the 16 floats; 20,000 on the home
replay, 4,472 by tick 1,000, 347 models. Against the lot's 172 objects at tick 1,000 (the last
orientation of each model; `assets ISO orientcheck LOG PLACES TICK`): 110 have one at their
tile's centre (floor + 0.5), and for 86 of them one turned 90 - 45 x facing (the rotation's
first row, atan2); 11 others (quarter-tile places) have one at their place itself, all turned
so. The 24 left are parts of objects of several tiles: a door's or window's two objects (one
each side of the wall) are drawn one by the rule and the other half a turn more (the second
object, the higher id), and the second tiles of beds and the bathtub carry their lead's
orientation. Study ASSETS_SHOW_ORIENTS, ASSETS_SHOW_UNTURNED=RULE.

Where an object is drawn (`scene::placements`). An object of several tiles is drawn once, for
its leader (the world's `leader`; lockstep's places dump now gives it): at the centre of its
parts' cells' bounding box, a cell being a tile (the table's 2x2 at (24.0, 16.0), the rug's
3x3 at (15.5, 19.5), the bed's six tiles at (14.5, 27.0), a door or window on the wall between
its two parts) or, for a group placed off the tile grid, a quarter tile (a tree's four parts at
8.75 and 9.0: (9.0, 9.0)); an object of one tile at its tile's centre, wherever on the tile it
stands (the end tables at 13.125). It is turned 90 - 45 x the leader's facing. On the home
replay at tick 1,000: 97 objects drawn from the lot's 172 parts, 96 with an orientation of the
original's at that place turned so; the one other, a painting of two panels, is drawn 0.45 of a
tile towards its wall ((19.95, 23.0) for (19.5, 23.0)).

Objects hung on a wall. The ResData row's second word has bit 0x40000000 for them (the wall
lamps, paintings, the wall mirror: 0x42000000 and 0x42800000; floor objects 0x02000000,
0x02800000); no bit of the definition row tells them apart. They are drawn 0.45 tile towards the
wall, (0, -0.45) in their own frame (the brass lamps on (16.375, 28.375) facing 0 at (16.95,
28.5); on (22.375, 23.375) facing 6 at (22.5, 23.95)). The group takes it from any part (the
painting's lead has no ResData; its second part has). `quickdata::ObjResources::on_wall`; with
it `scene::placements` places all 97 objects of the home lot at tick 1,000 where the original
oriented them, turned as it turned them.

The club (club-d.log: the club replay with the draw records, its steps the club log's, 324,072;
places at tick 1,000 from lockstep). A group's parts may stand off the tile grid by a quarter
(trees) or a sixteenth (the pool table at 14.0625, the dance floor at 20.25): it is drawn at the
centre of the box of the tiles its parts stand on (`floor`), the pool table's at (15.5, 22.0),
the dance floor's 16 parts at (22.0, 18.0); the trees' quarter-tile rule is the same rule. Only
objects with a dataset of their own are drawn (a marker, `destination_gohereandshow`, has a
ResData row with no dataset and no model). Every such object at the original's place and turn:
home 90 of 90, club 79 of 79 (and the home lot's 7 drawn from the lot's data, also at their
place).

## Walls

The original draws a lot's walls as static models too (SetOrient's 'o' records). For each edge
with a wall (`Walls::wall_between`, doors' openings counted as wall), by its style: a wall has a
part on each side, at that tile's centre turned with its back to the wall (90 degrees for the
wall on the tile's +x side, -90 on -x, 180 on +y, 0 on -y: the frame of an object hung on that
wall); a fence has one piece on the edge itself, along it ((14.5, 12.0) turned 0 for the home
porch's railing between rows 11 and 12). An edge's style is kept by the tile on its +x or +y
side, in the lot's second array: byte 2 for its -y edge, byte 3 for its -x edge
(`Walls::style`); the fences are the styles `SimsTiles`' `FenceData` lists (2, 12 and 13;
`quickdata::fence_styles`), the walls `WallTile`'s (home: 1, 3 and 22). `scene::wall_pieces`
gives the pieces; every one is the original's (home, tick 1,000: 148 pieces, 14 of them fences;
the club, its house's walls: 207, 43 fences; `assets ISO wallcheck LOG PIECES TICK` with
lockstep's LOCKSTEP_DUMP_WALLS).

## Wall patterns

Each side of a wall has its own pattern, kept by the tile it faces: the lot's second array's
bytes 4 to 7 are a tile's -y, -x, +x and +y sides (`Walls::pattern`). A pattern indexes
`SimsTiles`' `WallSet` list (its one row points at a list of `WallTile` rows, 20 bytes each: a
value, a name, the pattern's shader's archive id, two more; `quickdata::wall_patterns`); 0 is
the plain `sims_wall_paper_000`. At home, 17 is `sims_wall_paper_024`, 21 `_031`, 5 `_205`,
24 `_014` and 8 the outside `exterior-tile--202`. A shader's layer names its texture by the
texture's archive id, which is not always its name's hash (`exterior-tile--202`: 7a397517).
Checked (`assets wallcheck`, tick 1,000; a part's draw is the strip after its world matrix, with
the map-0 texture loaded last before it, named by the 'x' record last made for that address by
the load's tick, as addresses are reused: 18 of 215 at home): every wall part the original draws
with a wallpaper draws its own pattern's (home 67, the club 39). The rest are not drawn that
frame (home 38, club 57) or drawn only with a plain image whose pixels `walls_down`,
`wall_black`, `polyshadow` and `1st_final` share (home 28, club 68: the walls-down view's lowered
parts, it seems; which ones is next). One home part shares its place and turn with a counter.

A wall part draws in two pieces, each after its own world matrix at the part's place, both in
the main pass (the same projection and view): its face, a display list of 18 vertices (23 at a
door or window) in a one-layer material (flags 0x08, word 0x18) with its pattern; and its cap, a
display list of 6 vertices in the material flags 0x148, word 4, with the plain image. Faces are
one-sided: at tick 1,000 a part's face is drawn exactly when it is turned towards the camera
(its normal, (-sin a, cos a) for the part's turn a, pointing at the eye: `WallPiece::faces`) and
in view: home 67 drawn of 67 facing, 0 of 67 facing away; the club 39 of 82 facing (the other
43 out of the logged view and projection, 41 clearly, 2 at the frustum's edge), 0 of 82 facing
away. (Iteration 50's "outside faces lowered" was a misreading: objects standing at a part's place
add plain draws, and the listing had named those parts plain first.) The renderer culls back faces
to the same effect. No walls are lowered in either view; the walls-down view is still to be seen.

A part's cap (a 6-vertex display list, 5 at a wall's end, at the floor, z 0, with the plain image)
is drawn when the part's own tile is outdoors (the lot's third array not 1: `Walls::indoors`),
whichever way the part faces and in view or not: `WallPiece::cap`. At tick 1,000 the engine's
caps and the original's agree on every part (home 48 capped, 86 not; club 62 and 102). So an
outside wall has a cap on its outside only, an inside wall none. What the cap looks like (its
model and size) comes with the wall models.

The wall models are records of the lot's datasets (`House1Preload`, `housepreNN`) and of
`models.arc`: `wall_straight`, `wall_straight_door`, `wall_straight_standard_window` and
`_privacy_window`, each also `_left_extends`, `_right_extends` and `_cap` forms; a record's face
is a display list of 18 vertices (23 with a door or window's hole) and its cap one of 6 (5). Many
records share their lists' first bytes, so a draw names its family, not its record. The family
follows the edge (`Walls::opening`, `WallPiece::opening`): a door's two parts on its two tiles
make a door edge (as routing's openings), a window's (an OBJD type 8 with field 3 set,
`Object::window`) a window edge. At tick 1,000 every face drawn is of the engine's family: home
47 plain, 12 window, 5 door; the club 36 plain, 3 door.

A part's form (`scene::Form`, `WallPiece::form`, `scene::diagonal_form`): by what its model does at
each end of its wall (`scene::End`): reach past it (`_left_extends` at its end towards +t,
`_right_extends` towards -t, `_both_extends`, t = (cos a, sin a) for its turn a), close it
(`_left_cap`, `_right_cap`, `_bothcap`) or nothing. The ends are the wall's two corners; the walls
that leave a corner are its arms (`scene::arms`: the four edges and the four diagonals through it,
fences marked). An end reaches past its corner at an outer corner: an edge (not a fence, not a
diagonal) leaves the corner behind the part's face, none in front of it and none goes on along the
wall's line (for a straight part, behind is the side of its wall). An end is closed (a free end)
when no arm but the part's own leaves the corner, fences and diagonals counted: a wall that stops
in the open (the home lot's gap in the wall at x = 20, rows 17 to 19: `_right_cap` at (20.5, 16.5)
and `_left_cap` at (20.5, 20.5)); the 14 ends against the porch's fences and the corners at a
diagonal are plain. The same rule gives the diagonals' forms (a face's normal n is a diagonal step;
t = (n.y, -n.x); behind is an arm pointing away from n): at tick 1,039 all 5 faces seen agree
(`wall_diagonal` x2, `_left_extends`, `_right_extends`, `_both_extends`). A draw shows its form by
its positions (the strip's `at[2]`), a cap's two pieces of 17 and 4 vertices come with the 6 and
18 (the 5-vertex piece of the straight record is the undrawn one). At tick 1,039 every draw agrees
(home 216 wall draws, none missing; the club's frame is not checked: no draw-window log has the
study's clock). The mixed forms (`_left_extends_cap`, `_right_extends_cap`: one end each) are not
seen on either lot; which of the two names is which end is a guess. A record missing for a family
(a door's cap) draws the plain one. A record is several sub-models drawn one after another at the
part's place (each after its own world matrix): `wall_straight` [5, 6, 5, 18 vertices], the
`_extends` forms the same counts with other positions, and the cap forms add an end piece of 17 and
4 vertices (`assets ISO recdiff DATASET/NAME ...` compares records).

## Floors

A tile's floor is the lot's first array's byte (`Walls::floor`): an index into `SimsTiles`'
`FloorSet` list (its one row points at 54 `FloorTile` rows of 24 bytes: a value, a name, two
values, the floor's shader's archive id at +12, a value, a last word, 0x01000000 for most;
`quickdata::floor_patterns`). Home: 10 is `final-floor-tile-46` (the sidewalk), 7 `sim_road`,
39-41 the street's edge and middles, 25 `final-floor-tile-19` (97 tiles), 45 linoleum, 51 rough
wood; the club: 33 black marble, 18 `final-floor-tile-07`. Values past the list (255: home 5,
club 4) name none. Checked at tick 1,000 (`assets ISO floorcheck LOG FLOORS TICK`, the floors
from lockstep's LOCKSTEP_DUMP_FLOORS): the floor textures the original loads are exactly those
the lot's floors name (home 12, club 9) but for the road's stripes (`sim_road_stripe_north`,
`_west`: home and club 2 and 3 tiles), never loaded: they are out of view on both lots (not their
rows' last word of 0, which the street's rows share and are drawn). The floors are drawn batched, one display list a texture (the sidewalk's 216 vertices for
43 tiles: 5 a tile and 1, a strip), not a matrix a tile.

The floors' own draws are the command list's `TriStrip(float)` (opcode 42), which the observer
now keeps as 'g' records (the list's draws other than 56 and 66, apart from the 'w' strips so
their count still places the other records; `Log::other_draws`). A strip's vertices are floats in
the lot's frame (x, y, z, w, then texture coordinates), one row's run of n tiles in 2n + 2
vertices from its first tile's corner (x, y): the tiles (x, y) ... (x + n - 1, y), whichever way
its first edge goes. Home, tick 1,000 (`home-g.log`; `assets floorcheck`): 364 floor tiles drawn
so, 363 with the engine's tile's texture; the one other, (16, 14), holds 255 (no pattern) and is
drawn as part of the rough wood run beside it. The engine's floored tiles not so drawn: the road
(`sim_road`, 37, drawn as one of the lot's display lists, 'w'), its stripes (5, never loaded),
6 sidewalk tiles at the lot's corners and 4 porch tiles (14-15 and 23-24, 14).

The runs (`scene::floor_runs`, `FloorRun::vertices`): along each row, each stretch of tiles of
one pattern is one TriStrip of floats (op 42), z 0, taking in a half tile (255, cut by a diagonal
wall) at either end: a half before it adds the diagonal's far corner first (0x10, "/" from (x, y +
1) to (x + 1, y): (x, y + 1); 0x20, "\\" from (x, y) to (x + 1, y + 1): (x, y)), then each tile
edge's corners (y, then y + 1) along x, then a half after it its far corner (0x10 (x + 1, y),
0x20 (x + 1, y + 1)); a shared half tile goes to both its neighbours. UVs: u the corner's x less
the middle of the strip's x range (rounded down), v its y less the row's (the texture repeats a
tile). Each vertex's colour (the lighting) is logged too, not built yet. Tick 1,000 (`assets
floorverts LOG TICK RUNS`, the observer's 'v' records: every vertex of each op 42 strip;
lockstep's LOCKSTEP_DUMP_RUNS): every floor strip the original draws is the engine's, vertex for
vertex (position, UV, order): home 42 of 42, the club 132 of 132; the engine's others (18 each)
are wholly out of the logged view (the road's column at x 39-41, the sidewalk's ends, the
stripes), so the original culls runs out of view, as the renderer's clipping will. (Corrected: a
half tile after a stretch is drawn, its last triangle; the club's 4 runs once taken for "one tile
shorter" are these halves.)

## The camera

The lot's camera is `ESimsCam`'s parameters (the decomp's `CameraParameters`: a target, a
distance, a rotation and a tilt in degrees; the live ones ease towards the wanted ones each frame,
the target's z kept at 0.25). `CalcEyePosition`: from the base vectors (0, 0, 0) and (10, 10, 0),
the eye is the target plus the distance along (-1, -1, 0) / sqrt 2 tilted up by the tilt and turned
by the rotation about the vertical. The view (the 0xF0 matrix) is a look from the eye at the target
with z up: row 0 level across, row 2 back towards the eye, the translation -R eye
(`scene::Camera`, `eye`, `view`). Checked (`assets ISO camtrack LOG`: each tick's view, the
parameters it is, and the engine's view from them): home and the club at tick 1,000, the engine's
view is the log's to 4e-6. The club's parameters come out round (distance 8, tilt 25; rotation
151.97), home's not (17.667, 36.10, 81.23: a zoom level's or easing). The camera held still over the
logs' drawn ticks. Where the wanted parameters come from (the followed Sim, the zoom levels, the
player's turns) is next.

The camera's records (the observer's 'c', on `ESimsCam_Ease` = the decomp's
`fn_8000698C__8ESimsCam`, after each frame's step, when it changed: the wanted parameters at
+0x398, the live ones at +0x3B0, the mode +0x3C8, the eye +0x378, the cursor +0x324; the decomp's
header has "current" and "wanted" the other way round, but its code eases +0x3B0 towards +0x398 and
takes the eye from +0x3B0). Home (`home-c.log`, ticks 0-424, 407 records: rotation 0-125, tilt
25-36, distance 8-17.7, the target over the lot): `scene::Camera::eye` from the live parameters is
the logged eye to 4e-6 on every record (`assets ISO camlog LOG`). The wanted distance and tilt step
together (8 to 17.667 and 25 to 36.1 in 11 even steps: a zoom), the rotation by 2.145 a frame
(a held turn); the live z eases from 0.04 to the target's 0.25 at the start.

The easing (`scene::Camera::ease`, the decomp's fn_80007714, fn_800078BC and fn_800079C0 as the
usual mode 0 calls them): the target moves straight at 5 times its distance a second (2.5 within
1.5), the distance at 2 times its difference, the rotation (the short way round) at 3 and the tilt
at 2.5, by the time step at 0x8037BFC8 (0.033 s); each lands on the wanted value when the step
would pass it or the difference is 0.0001 or less (then an angle is not wrapped: a wanted 360
stays 360). Not once a frame: the frame's step (`ESimsCam_Ease`) eases once and calls `SetWinPos`,
which eases again, and a second `SetWinPos` a frame eases a third time (6 callers; the lot view
alternates 2 and 1 steps between records). The observer logs each easing ('c' records on both,
with the steps entered since the last); `assets camlog`: home (`home-c.log`) 809 records and the
club (`club-c.log`) 4,311, each the last eased by its step count towards its wanted values, to
4.4e-5 (home exactly); a still stretch (easing leaves the camera as it is, on its wanted values or
a float's step short of them) whose wanted values changed somewhere in it is matched by its last
k steps (home 3, club 16); the lot's start places the target's x and y on the Sim at once (1 each,
not built yet).

The wanted parameters from the stick (the decomp's fn_80005EE4, fn_800060A8):
`scene::Camera::turn` adds the stick squared (keeping its sign) times the time step times 65
degrees a second to the rotation, which past 360 goes to 0 and below 0 to 360 (so the club's
wanted 360); `zoom` takes the input off the distance, kept within 8 and 35 (0.65 of 35 in the
original's other case), and sets the tilt in proportion, 25 at 8 to 56 at the far end (home's
17.667 gives 36.099). `assets camlog` over the wanted values' changes: zooms home 11 and club 26,
each the engine's zoom's distance and tilt; turns home 83 and club 303, each the engine's turn by a
stick within -1..1 (a full stick 2.145 a frame). Still to come: the tilt's own stick (fn_80005FC8,
20 to 88, a control mode the logs never use) and the target from the followed Sim.

The target (the decomp's fn_80006E6C, centring on the followed Sim): the cursor is put on the
Sim's place (z 0), and the wanted target 2 beyond it along the look's level direction (from the
eye to the wanted target at that moment; so after a turn the target stays where it was); a target
more than 10 from the live one places the live one there at once (the lot's start); a place off
the lot (outside 1 to size - 1) leaves the camera. `scene::Camera::centre_on`. The observer's
'c' records now carry the cursor's place (+0xA0 of the cursor, `Unk80026864::fn_8002D2C8`).
`assets camlog`: the club, every one of the wanted target's 1,062 moves is the engine's centring
on the logged cursor by the look just before, and the target is 2 from the cursor on all 4,312
records; home, its start's move, the rest (163) the player's cursor moving the camera at the
screen's edge (fn_80007470's box, 0.15-0.85 across, 0.3-0.75 down), next.

The player's cursor and the target (the decomp's CursorMoved, mode 0, and fn_80006C58): while the
cursor is off the screen's middle (fn_80007470's box, 0.15-0.85 across and 0.3-0.75 down), its
moves are added to the wanted target's x and y, and once a frame it is pulled back towards the
target by the cursor's speed (`scene::Camera::cursor_speed`, from the wanted distance: 5 tiles a
second at 8 to 15 at 35; `pull_cursor`). `assets camlog`, home's 164 target moves: 1 the start's
centring, 144 the cursor's move carried over, 18 that and the pull; 1 at the lot's edge (the
cursor held at x 1) not explained yet. The box test itself (the cursor projected through the
window's view) is not checked yet.

The projection (`scene::Camera::projection`, from the decomp's GetFov / GetNearPlane /
GetFarPlane and the window's 640 x 448 frame): the vertical field of view 55 degrees at the near
zoom to 62 at the far, times 448 / 640; aspect 4:3; near plane 2 to 10, far 200; from the wanted
distance. Against every lot projection the logs loaded (home 546, club 592): exact. The screen's
middle (`on_screen`, `off_middle`: fn_80007470's box, 0.15-0.85 across, 0.3-0.75 down): home's 331
player cursor moves carry the camera exactly when the moved cursor is off it, through the live
view and the projection (the club's cursor only follows the centring). The 2D overlay's
projections (8.144, an orthographic one) are not the lot's.

The floor's vertex colours (the 'v' records' colour, RGBA with A 0x80): lighting, per corner and
per run. A corner shared by two runs can carry two colours, one each (home 210 of 1,444 corners,
the club 186): each run lit as its own room (home: the street's corners 80 80 70, the sidewalk's
beside them a0 a0 8d; the club: a dark room's 18 18 1e beside the outside's 4d 43 45), and within a
run the colour changes smoothly corner by corner (lamps' and windows' falloff: home's indoor
runs 67 67 60 to 6b 6b 69, outdoor a0 a0 8c to a3 a3 91). `assets floorverts` (ASSETS_COLOURS,
ASSETS_STRIP_COLOURS, ASSETS_CORNER_KEY) lists them. The strips are not recorded through
ERC_TriStripFloat (a hook on it never ran); the functions that store opcode 42 themselves are
fn_800C76C0 (9.3 KB, twice: the likeliest floor builder), fn_8002FE20 (a CTile function),
fn_8019754C, fn_8019788C, fn_80197A34, fn_80197D3C and some CAS ones, none decompiled yet.

Where the floor's colours come from (found with temporary call probes, removed): the floor's
strips are recorded once, at the lot's load (tick 0), by fn_800751DC (called from fn_80075B60),
which writes its ERC entries itself through fn_8019788C (an opcode 42 writer of its own; the
entries are then played every frame, so ERC_TriStripFloat never runs for them). Next to it,
LightGrid_Sum (0x80074B1C, the recomp's `src/light_grid.cpp`, called by fn_80074D88) lights a
39 x 39 grid of points: each grid cell lists up to four lights for each of a few ids (rooms, so a
corner between two rooms has a value for each: the two colours a shared corner carries), a light
32 bytes (x, y, height, reach, colour RGB), and a point's light the sum over its cell's list of
colour x max(height / distance, 0) x min((reach / distance)^2, 1). How the sums become the
vertices' bytes, and where the lights and the lists come from (the lot's lights, its rooms,
outside light), is next.

The light grid's sum (`light::point_light`, `Light`): checked through the observer's 'i'
records (each LightGrid_Sum call after it: the room id, each lit point with its cell's lights' 32
bytes and the output; the recomp's `hle_LightGrid_Sum` calls `tree_log_light_grid`, read only):
home (`home-l.log`, 400 calls, ticks 0-77, room ids 0-3) 152,200 points and the club
(`club-l.log`, 400 calls, ticks 0-51, ids 0-5) 103,302, every one the engine's sum bit for bit.
The grid is lit again and again, not only at the load (400 calls by tick 77): which room ids,
spans and lights each call takes, and how the sums become the floor's vertex bytes, are next.

The floor's vertex colours (`light::vertex_colour`, `light::entry_points`): when a room's floor is
relit (the original's Light_RoomFloor, 0x80074D88: LightGrid_Sum over the room's spans, then each
floor group's colour base and scale from Light_GroupScale, 0x80074A70), each vertex of each floor
entry takes the light-grid sum at its corner. Each channel is the sum times the group's scale plus
its base (fused), truncated, then only the top clamped (255; the low byte is stored). Alpha is
0x80. The corners follow the strip's order: a leading half tile's one corner (flag 0x40 on the
row, 0x80 the row after), then two a column, then a trailing half's. Checked through the
observer's new 'b' (each group's base and scale) and 'n' (each relit room: every group's
entries, each one's word and its vertex colours) records: home 400 relits (1,200 groups, 9,800
entries) gave 293,400 vertices, and the club 400 (869 groups, 10,249 entries) gave 203,936.
Every one is the engine's colour.
The room objects sit in the game's heap at 0x7E7Axxxx, so the observer accepts addresses from
0x7E000000.

The floor groups' colour bases and scales (`light::group_colour`, `GROUP_K` = 128, the
original's Light_GroupScale): base = (diffuse x the room's light + the material's ambient
triple x the room's ambient) x 128, fused; scale = diffuse x 128. The blue scale takes the
diffuse's green (the original reads +0x48 twice), kept as is. The material holds the diffuse at
+0x44 and the ambient triple at +0x54; the room's triples are at +0x28 (ambient) and +0x3C
(light). The 'b' records now carry these inputs and the constant: home (4,000 groups, 3
materials) and club (4,000, 2) give the engine's base and scale bit for bit on all. No logged
material has a diffuse green that differs from its blue, so the blue-from-green read is from the
code only, not yet seen in a log.

The rooms' ambient and light triples (`light::room_triples`, `NO_LIGHT` = 0, the original's
Light_RoomTriples, 0x80074958). Each room id has an 80-byte light record (at the light state +
id x 80 + 0x5018).
- The ambient is its colour at +0x10 when the word at +0x1C is set (a pointer), else 0.
- The light is its colour at +0x30 times the light direction's downward part, max(-z, 0), with z
  at +0x28 (the direction at +0x20). That applies when the word at +0x3C is set, else 0.
Checked through the observer's 'a' records (the record, the triples, the constant): home 4,000
calls (734 distinct records, 3,000 rooms without the two) and club 4,000 (1,020, 3,333), every
one bit for bit. Only room id 0 (outside) has the light, and its direction and colours change
from tick to tick (the sun through the day).

The light ranking (`light::rank`, `distance2`, `RANK_AT` = 0 and `RANK_INSTEAD` = 0.0001; the
original's Light_Rank, 0x801691BC). Each light in a range of the 32-byte light array scores
reach^2 / its squared distance from a point. The squared distance is the paired dot product,
rounded as `point_light`'s; a distance of exactly 0 is taken as 0.0001. The light goes into a
list kept strongest first, before the first slot it beats: the later slots move down one and
the last drops off. A slot is 8 bytes: the light's index (u16, the rest of the word kept) and its
score. Its callers (0x8016906C, 0x80169418) build the lists the grid cells take their lights
from. Checked through the observer's 'd' records (the point, the lights, the list before and
after, the constants): home 5,512 rankings (40,328 lights) and club 20,000 (69,174), every list
word for word.

The light grid's cells (`light::cell_lights`, `GRID_Z` = 0, `SLOT_EMPTY` = -1, `SLOT_TAKEN` = -2;
the original's Light_BuildGrid, 0x80169418). The grid's points are x, y from 2 to 37 at height 0.
Each 36-byte cell lists up to 4 room ids (int8, -1 ends them), each with 4 light indices (u16).
For each room:
1. Four slots start empty (-1, score -1).
2. The room's own lights (its record's words 18-19, a range of the light array) are ranked in
   (`light::rank`).
3. From the slot past the filled ones (at most the third, so two are kept for them), the room's
   other lights (words 16-17) are ranked against what is left.
4. The slots are taken strongest first: the first of equals, each taken one's score set to -2,
   and with none above -2 the first slot again.
An empty slot's index is 0xFFFF. Checked through the observer's 'e' records (the constants, room
ids 0-15's ranges, the lights, every cell after the build): home 2 builds (2,728 cell rooms) and
club 40 (55,440), every cell room's four lights the engine's. The other caller (0x8016906C)
gives each object its 4 nearest lights the same way (one ranking over its room's own range,
pointers at the manager + 0x4014 + 16 a slot); that one is not ported yet.

The light grid's room lists (`light::CellRooms`, the original's Light_AddCellRoom, 0x80168D70).
Each grid point (0..=38 each way) lists up to four room ids. Adding a room puts it in the first
empty slot, or does nothing when it is listed already. A full point, or one off the grid, is
left alone. The add also clears that slot's lights and marks the grid to build. The adds all
come at the load (tick 0) from one function, 0x800741D8, the lot's tiles walked. Each tile adds
its room to its corners through a corner table (0x802A1220), and diagonal tiles split their
corners between their two half rooms; that rule is still to port. Checked through the
observer's 'f' records (each add: x, y, the room, the caller): home 5,486 adds and club 5,484.
Replayed in order through `CellRooms::add`, they give every build's room lists: home 2 builds
(2,592 cells) and club 40 (51,840), all the original's. `light::cell_lights`' cell rooms are
these lists.

The light grid's rooms at the load (`light::cell_room_adds`; the original's tile walk,
0x800741D8). Tiles x and y run from 1 to the lot's size less 2 (x inner). The size is 39 on
both logged lots, taken from the lot by a virtual call not yet read; the engine's wall arrays
are 64 square. Each tile adds its room to its corners (x, y), (x, y+1), (x+1, y+1), (x+1, y).
A diagonal's tile adds six:
- its west half (the room across x - 1) to (x, y), (x, y+1) and its side's far corner (0x10:
  (x+1, y); 0x20: (x+1, y+1));
- its east half (across x + 1) to the other line end's corner ((x, y+1) for 0x10, (x, y) for
  0x20), then (x+1, y+1) and (x+1, y).
The original's room ids are the engine's rooms less one: outside 0, and the same order. That
holds on every whole tile of both lots. Checked add for add in order (`LOCKSTEP_CELL_ROOMS=LOG`
with `LOCKSTEP_DUMP_PLACES`/`LOCKSTEP_DUMP_WALLS`, against the log's 'f' records): home
5,486 and club 5,484 (the club with its house's walls), 9 diagonal tiles among them, all the
same. The halves' rooms come from the original's half-tile lookup (0x80234774). The neighbours
across x give the same on these 9 tiles, but a diagonal next to another diagonal, or a wall on
that edge, is not seen in the logs. `LOCKSTEP_DUMP_ROOMS` writes the engine's rooms per tile.

The lights' sorting (`light::sort_lights`, `LightKind`; the original's Light_Rebuild,
0x80168E08). When the grid is marked, the light objects are walked room by room, in list
order:
- a lamp in the room takes the next own light (from 256, at most 511);
- a window in it whose other side is another room takes the next other light (from 0, at most
  255);
- a room's ambient object goes to its record's +0x1C, the sun to +0x3C.
The lights' data are then filled from the objects (0x801692F8 for the own lights, clamping the
colour between the constants at 0x802BA7B0 and 0x802BA7B4; 0x80169780 for the others), the
record's ambient and sun from theirs (0x801696F0), each object's 4 nearest (0x8016906C), and
the grid built.
The four classes, by vtable: 802b9318 a room's ambient (one a room), 802b94b0 the sun (one),
802b9d70 a lamp (home 11, club 15) and 802b9f60 a window (home 28, club none). Windows come in
pairs at a window: an inner one (reach 3.5, colour (0.6, 0.6, 0.545)) and a dimmer outer one
(reach 1.75 or 1.5). The lamps' positions in the light array read near 0 (object-relative) at
the load.
Checked through the observer's 't' records (each call: the objects, their vtable, room and
changed flag, and the lights with their objects) against the ranges of the grid build before:
home and club 38 records each, every room's ranges and both ranges' objects in order the
engine's. The records include calls that did not rebuild (home has 2 builds), and a window's
other room is not logged: it is taken as another room than its own, true of every window here.

The window lights (`light::window_light_places`). Objects the original counts as windows (the
engine's `Object::window`: a door-class object whose definition's field 3 is set; windowed
doors are not) get two window lights from their creator (0x8007F8CC: one each side, 0xE0
bytes, at the object's +996 and +1000, set up by 0x801660BC). Only a window's leading object
has them. Each light's update (0x801666D4, the class's vtable +0x54) does the following:
- **Place:** the window's transform applied to (0, +-0.8, 0) (the 0.8 at r13 - 29604, r13 =
  0x803833E0). The transform sits on the wall: the leading tile's centre moved half a tile the
  way it faces (0: +x, 2: -y, 4: -x, 6: +y).
- **Height:** set by the window's kind (the object's +12): 2.0 for kind 1, 2.5 for kind 2.
- **Reach and colour:** from per-kind tables indexed by whether the light's room is inside.
  Reach tables: kind 1 (7, 14), kind 2 (6, 8), kind 3 (12, 24); the logged reaches are a
  quarter of these. Colour tables: kind 1 (0.24, 0.6), kind 2 (0.22, 0.4), kind 3 (0.28, 0.7).
- **The colour then filled** (0x801697F0): the outside's sun colour when its other side is
  outside, plus up to three of the window's nearest lights by reach^2/distance^2, normalised
  by the largest channel against a cap.
On home the storm windows are kind 1 and the small rectangle ones kind 2. Checked: home's 28
window lights' x and y, every one bit for bit at the engine's places (lockstep
`LOCKSTEP_WINDOW_LIGHTS=LOG`, against the log's last 't' record); the club has none, and the
engine gives none. The kind (to read from the object's definition), the reach's quarter and the
colours are not ported yet.

The window lights' reach, base colour and height (`light::window_light_params`). These depend
on the window's kind and on whether the light's room is inside (index 1):
- reach is the kind's table times 0.25 (the get-light, 0x8016645C, scales by 0x802B9F38): kind 1
  (7, 14), kind 2 (6, 8), kind 3 (12, 24);
- the base colour (+0x94) is kind 1 (0.24, 0.6), kind 2 (0.22, 0.4), kind 3 (0.28, 0.7);
- the height is 2.0 for kind 1 and 2.5 for kind 2 (kind 3's not read).
The kind is the window object's +0xC (the object of class 0x802A1E08, made by 0x8007C05C): 1
for home's storm windows and 2 for the small rectangle ones. It is not in the definition's
fields, nor the object's data or attributes; its writer is still to find. Checked through the
observer's 'y' records (each window light: side, raw reach, base colour, its window object, the
kind): home's 28, every reach, base colour and height the engine's from the logged kind and the
light's room.

The window kind (`light::window_kind`). The original sets it in 0x801679CC, which 0x80078A68
calls with the window object's +0x138, from a fixed table of nine name hashes:
- kind 1: 08FD5188, 27B37C8F, 437C7240, CDDD1EDE, D02B5C12 (home's storm windows), D491D417;
- kind 2: 92751A1A (home's small rectangle windows);
- kind 3: 61B5D7D6, 040E93A9, 63BE954C;
- any other key: 0.
The key is the engine's `quickdata::ObjResources::key`: the first word of the record that the
first word of the definition's `ObjDefinition::ResData` row points at. A window's ResData row
belongs to its second tile's definition, not to the leader's. Found with a `-DGCN_WATCH` build
watching a window's +0xC; the observer's 'y' record now also carries +0x138. Checked by lockstep
(`LOCKSTEP_WINDOW_LIGHTS`): home's 28 window lights, each at the engine's place, with the
engine's kind taken from the window group's key.

The window lights' colours (`light::window_mix`, the original's Light_FillWindow at
0x801697F0, run for each window light by 0x80169780). A light's mix starts at 0. An inside
window light adds the sun (record +0x5048); outside ones don't. The filler adds it when the
light's vtable +48 call gives room 0, and the log shows that this is so for every inside light
(the other side's room, it seems; not read further). Then up to 4 of its
nearest lights (the list at record +0x4014 + 16·index, read up to its first gap) each add
colour × min(reach² / d², 1). Here d² is the paired square length (0x80122228: x² and y²
rounded, z² fused onto x², then y² added), and a d² of 0 is taken as 0.0001. Last, the mix is
scaled down by 1 / max(1, its largest part). The object keeps it at +152 (flagging a change at
+136 when the move is over 0.25), and the light's colour is the mix × the base (+148, by kind).
At the first fill the window lights' positions are still 0, so lamps near the origin count in
full: that is the original's own state, and the port keeps it.
Checked through the observer's 'z' record (per fill: the light, its object's room, its
position, the sun, the nearest list with each light as read, then the mix, the base, the reach
and the colour) with `assets windowfill`: home's 4,000 fills (the record's cap), mix and colour bit for bit;
club's lot makes none.

The lamps' lights (`light::lamp_light`). Obj_PlaceLights (0x8007A8CC) puts an object's lights at
its nodes. The object's definition (resources +344, then +140) lists 8-byte entries (a kind,
then a node). The object keeps its light objects at +1604 (their count at +1608) and its nodes at
+848, 64 bytes each:
- word 0: flags;
- words 1-4: a rotation (quaternion);
- words 5-7: the position;
- words 8-10: a scale;
- words 11-13: a colour;
- word 14: a factor;
- word 15: a radius.
A kind 0 light takes the node's position (Light_SetPosition, 0x80165C28, the light object's +180,
which the lamp get-light 0x80165A90 copies) and, with flag 0x80, the radius (+204, twice it at
+208). The light's colour is the node's colour × its factor, and its reach is +208 × 0.25. The
node records are on the disc in the objects' `animatio.arc` entries (home's lamps:
`Bottle_Lamp_A_00`, `Top_Brass_Sconce_A_00`, ...). Their scale, colour, factor and radius are as
the disc has them; the position is the posed node's, in the object's own space. The world place
follows the object's pose, which is stage 4.4 (animation): until a lamp is posed, its light sits
near the origin, as in the first rebuilds. Checked through the observer's '1' record with
`assets lamplights`, against the node as logged last before each rebuild (a node's colour and
factor change: club's lamps go from (0.502, 0.502, 1) × 0.5 to white × 1 after the first
ticks). Home's 418 lamp lights across the rebuilds and club's 494 kind 0 ones match on colour
and reach bit for bit. Club's other 76 belong to an object whose list entry is not of kind 0
(not read yet).

A room record's ambient and sun (`light::room_sun`, the original's Light_RoomSun at 0x801696F0,
run by Light_Rebuild for each record). The record keeps its ambient object at word 7 (+28) and
its sun object at word 15 (+60).
- The ambient's get-light (0x801631C8) gives words 0-3 as 0 (a position and reach) and words
  4-6 as its colour (+152) × its factor (+148).
- The sun's get-light (0x801636B4) gives words 8-10 as its direction (+180), word 11 as 0 and
  words 12-14 as its colour × its factor.
A record without the object keeps its words. Home's outside record has an ambient of
(0.8, 0.8, 1) × 0.4 and a sun of (1, 1, 0.7) × 1.3, with the direction moving a little every tick
(the time of day). The window fill's sun (record +0x5048) is the outside record's words 12-14.
Checked through the observer's '2' record (the two objects' factor, colour and direction
before, the record's 20 words after) with `assets roomsun`: home's and club's 4,000 records each, word for word.
Where the objects' colours, factors and direction come from (the time of day) is next.

The time of day's light (`light::time_of_day`, the original's Light_TimeOfDay at 0x8004A1F4,
called with the sun object, the ambient object, the hour, minute and second, and a pointer
that receives the last value). The keys (`light::TIME_KEYS`) are ten entries: an hour (0, 4, 5,
6, 7, 12, 17, 18, 19, 20), the sun's colour and factor, the ambient's colour and factor, and a
last value. The original builds them in code (0x8004B784) into 0x802E60C4, so they are not on
the disc. The key for an hour (0x8004A194) is the one at the hour, else the key before the first
later one (wrapping round), else the last. Then:
- t = (time since the key) / (span to the next key), each wrapped by 86,400 when below 0;
- a colour is next × t + key × (1 − t);
- a factor and the last value are next fused onto key × (1 − t).
The objects take a new colour only when it moved (squared) more than 0, and are flagged changed
(+136) when it moved more than 0.25 (a factor: 0.1). The sun's direction comes from a rotation
by the time (0x801B3024) and is not ported yet.
Checked through the observer's '3' record (the call's registers, the 100-word table, the sun
object's factor, colour and direction after and the last value) with `assets timeofday`: the
engine's keys equal the logged table. Home's 2,000 calls (from 7:04) and club's 2,000 (from
18:15) give the sun's colour and factor and the last value bit for bit. The ambient's part follows
the same code; the observer doesn't log its object yet.

The sun's direction (`light::sun_angle`, `light::sun_direction`, with `mathf`):
- The angle runs over the day from 0 at midnight, at even speed within each span: 0:00 to 4:00
  covers 0 to π/2, 4:00 to 20:00 covers π/2 to 3π/2, and 20:00 to 24:00 the rest. The code uses
  its own π (0x40490FD0) and π/2 (0x3FC90FD0).
- (0, 0, 1) is turned by the angle about the axis (0.5, 0.5, 0.2), normalised (0x801B3024's
  rotation: the matrix's third row).
- The result is normalised, its z made negative, and z kept between −0.8 and −0.2.
The sine and cosine are the game's own: fdlibm's float sinf and cosf (0x8010E450, 0x8010E288,
the kernels 0x8010E698 and 0x8010E5AC, the reduction 0x8010E958), ported step for step in
`mathf` (each step rounded to single, the code's fused steps fused). The normalise is the paired
one (0x801221E4). The recomp gives its reciprocal-square-root estimate as an exact 1/√x, and the
engine matches the recomp there. Checked with `assets timeofday` on the '3' records: home's
2,000 calls (7:04: angle 2.17, the reduction's short path) and club's 2,000 (18:15: angle 4.37,
its middle path) give the direction bit for bit.

The time of day from the engine's own clock. The original's caller (0x8004A91C) reads the hour,
minute and second from globals 0, 5 and 6. The engine's clock (`World::clock_pass`) gives the
second as 2 per pass counted. `replay`, on a light log, checks each tick's '3' calls against the
engine's clock after the tick and against the light it gives (`time_of_day`, `sun_direction`).
At home all 2,000 calls match: the clock, and the sun's colour, factor and direction, the last
value and the ambient's colour and factor, bit for bit. At club the hour and minute match from
tick 1 (set from the log), but the second does not: the original starts at 18:15:24, and no lot
in the save has that time. The club lot (15) says 19:27:40, and home's lot 0 says 7:04:00, which
home starts at. The original's minute turns at tick 18 there, the engine's at 30; replay setting
the globals to the log's hides it (club's "scope 0 different" notes).
Found (iteration 88): the observer's '4' record (a one-time scan of main memory for the clock at
the first time of day call) put the globals at 0x80ECE114. A `-DGCN_WATCH` build watching
global 6 then named the clock's tick (0x80237D18: +2 a pass, 0x80237D60: back to 0 at 60) and
the loader that set 24: the SIMI reader 0x80237810. It reads 32, 42 or 48 globals by the chunk's
version (up to 63, up to 68, after) into the lot object's +1056, through 0x80230530 and
0x80230370. The 18:15:24 is the disc's `default.ngh` lot 15 SIMI (`assets cardfind disc 18 15
24`); the save's lot 15 says 19:27:40, and home's 7:04:00 is the save's lot 0 (the disc's says
6:30:00). The engine's clock now keeps the second as global 6 (`World::clock_pass`), as the
original does.
The club session's lot is the disc's, not the card's (iteration 89): the session did not load
the card's save, and played the disc's default neighbourhood (as a free play start would). The
card already held the club at 19:27:40 before the session, and the session still began at the
disc's 18:15:24. Replayed from the disc (`replay club-l.log ISO --lot 15`, no `--card`), the club
log agrees far better than from the card:
- the clock and the time of day's 2,000 calls match end to end;
- global differences (scope 0) fall from 78 to 4, scopes 14, 15 and 1 from 111, 559 and 41 to
  0, scope 4 from 41 to 2 and scope 8 from 70 to 20;
- motives set to the log's fall from 94 to 1 (the card run lets the motives move, `card_lot`).
The figures keep their `--card` (both give 0 differing steps).

The outside room record from the clock alone (`assets roomclock`). The outside record's two
objects are the time of day's (its ambient is the call's r4, its sun r3). For each '2' record,
the tick's last time of day call gives the clock, then `time_of_day`, `sun_direction` and
`room_sun` give its words 0-14 word for word: home's 1,000 and club's 667. The inside rooms'
records hold an ambient object of their own (no sun). Its colour is the time of day's ambient
colour (0.8, 0.8, 1), but its factor is the room's own: home's three rooms about 0.303, 0.315 and
0.309 (the outside's 0.4), the club's about 0.265 to 0.272, all drifting with the time. Its
writer is next.

The inside rooms' ambient factor (iteration 91). A watch on an inside ambient object's factor
(+148) named its writers: 0x8004B10C once at the lot's load, then 0x8004AE38 each tick. The
tick's pass writes the new value only when its square difference from the old passes a
threshold (0.1), and marks the object (+136) past a larger one (0.25). Both take the value from
`Light_RoomAmbient` 0x8004B300 (`light::room_ambient`):
- the lot's day and night values, from a run-time table at 0x80340D5C by the light manager's
  kind (+60): day 0.3 and night 0.2 at home (kind 1) and the club (kind 16);
- day from 7:00 to 17:59, night otherwise; across 6:00 night to day, and across 18:00 day to
  night, by (minute × 60 + second) / 3,600;
- times the room's window brightness, `Room_WindowBrightness` 0x80236270
  (`light::window_brightness`): 1 + 0.25 × the share of the room's tiles whose object reports
  kind 8 (a window), clamped to [0, 1].
The observer's '5' and '6' records hold each call's inputs and result. From the logged inputs,
`assets roomambient` gives all 4,000 calls bit for bit at home and at the club. The window
counts behind the brightness come next, from the engine's own room tiles.

Memory snapshots (iteration 92). The observer writes the whole 24 MiB of main memory before the
simulation of chosen ticks: `BUSTIN_TREE_SNAP=1,100,500` (the ticks) and
`BUSTIN_TREE_SNAP_DIR=DIR`, giving `DIR/mem-<tick>.bin` in the console's byte order. The engine
reads them offline: `assets snapword SNAPSHOT ADDR [COUNT]` for words, and
`assets snapcheck LOG DIR` to check each snapshot against the log. Home's ten (in
work/snap/home, ticks 1 to 6,000) hold the clock's globals as the log's '5' records wherever the
log has them (ticks 1, 100, 500 and 1,000), and the day/night table (0.3, 0.2) as logged. A run
with ten snapshots came out 30 steps short of the full log, so snapshot runs give data only. The
steps are counted from the log kept beside them.

The window brightness's inputs, from the snapshots (iteration 93). A room object (the '6'
record's r3) holds its tile entries between the pointers at +4 and +8. Each entry is 3 bytes (x,
y, level), and some tiles appear more than once. The tile count the share divides by is +72 / 2
(rounded towards 0, 0x80235EDC): home's rooms 219, 34 and 34, which with the logged brightness
give whole window counts (12, 7, 4). For each entry, the loop:
- looks the tile up in the manager's (r13 − 23124) 64 × 64 grid of s16 ids at +52 (row y × 128
  + x × 2, 0x802031B0; the level byte is not used);
- takes the id's entry in the table at +8332 (0x80203168, its +4);
- counts the entry when that object's kind is 8: its virtual +872 (0x801F60D8), the s16 at +18
  of the record its +232 points at.
Those records live below 0x80000000 (0x7E81FAEC for one), outside the snapshots. Checked so far
against the engine: the big room's 12 windows are the engine's 12 window objects on its tiles.
The two small rooms are not (7 and 4 implied, against 4 and 0 windows, and no definition field
at 8 fits). So the grid maps tiles to objects beyond those standing on them. Next: log each
entry's grid id and kind in the observer (memory reads only).

The window share's kinds (iteration 94, one home recording with the batch). The observer's '6'
now also holds the room's +72 and, for each tile entry, its grid id, the table's object and the
kind (memory reads only). The snapshots now also hold the game's virtual heap (0x7E000000,
12.25 MiB, `mem7e-<tick>.bin`). From the logged kinds, `light::window_brightness(the entries of
kind 8, +72 / 2)` gives all 4,000 brightness values bit for bit. The table's object is the
listed object less 112, the same this-adjust. Named through the log's lists, the kind 8 objects
are the doors and windows: storm windows, small windows, wooden archways and the lattice door.
The grid gives a tile the door or window on its walls, so a room counts the openings around it.
The big room counts 12 entries, the small rooms 7 (two archways, two storm windows) and 4 (two
archways, a small window). Kind 4 (147,171 entries seen) and 2 are other objects, not counted.

The grid's openings from the engine's lot (iteration 95, no game run). Paired by the log's ids,
each kind 8 entry is the door or window standing on that very tile. The entries' bytes are (y,
x, level) in the engine's axes, which hid this before. The grid keeps one object per tile: the
tile's floor object when there is one. That is a desk, counter, chair or bathtub, not a person,
not an object on another (the plant on the desk, the sink on the counter) and not the opening
itself. Only an opening with nothing on its floor counts (`light::tile_counts_as_opening`).
With the engine's objects at tick 100, home's three rooms count 12, 7 and 4 openings, entry for
entry as the log's kinds (lockstep `LOCKSTEP_ROOM_WINDOWS=LOG,SNAPSHOT` with
`LOCKSTEP_DUMP_PLACES=100` and `LOCKSTEP_DUMP_WALLS`). The rooms' tile lists themselves still
come from the snapshot; the engine's rooms are next.

Where the rooms' tile lists come from (iteration 96). In the engine's axes, a room's entries are
a serpentine walk over its tiles, column pairs up and down. There are repeats, and some entries
at the turns lie outside the room (the engine's room 0). It is the floor's strip order, not the
room's tile set, so the window share counts some tiles twice and outside tiles too. A watch on
a room's +4 at load named the setter `Room_SetTiles` 0x80235EF0 (a vector copy, 0x80236FA8). Its
caller 0x80243C24 is the lot's room walk, about 3,000 instructions. The engine's rooms per tile
(`routing::Walls::room`) agree with the entries inside the rooms: rooms 2, 3 and 4 are home's
three. Porting the walk is next, checked against the snapshot's lists entry for entry.

The rooms' tile lists as a check target (iteration 97). The observer's '7' record
(`Room_SetTiles`, after it) logs each room's list as set: room, its +72 (still 0 then; set
later) and the entries (y, x, level). `assets roomtiles LOG` lists them in the engine's axes.
The club's (work/logs/club-rooms.log, a data-only log 510 steps short; the rooms are set at tick
0) has six rooms: the outside (over 2,048 entries, the record's cap), three one-tile rooms of 3
entries ((10, 13), (11, 13), (10, 13): the tile, its east neighbour, the tile again), one of 36
and one of 400. Home's three are in the snapshots. Each list starts with a tile, its east
neighbour, the tile's north, that one's east, then the first tile again, and goes on in pairs up
the column, adding a third tile where the room widens. The walk that builds them (inside
0x80243C24, before `Room_SetTiles`) is next to port, checked against these lists.

The room walk's shape (iteration 98, read from 0x80243C24; the port follows). Per level, it
visits every tile in rows, x inside y. A tile not yet assigned starts a new room. The room's id
is (level − 1) << 10 | a counter. The tile's list and a stack are cleared and the tile is pushed.
The walk is depth first. The stack is a vector of 3-byte tiles, and it pops from the back. A
grid of u16 per tile holds each tile's state:
- 0xFFFF: not reached;
- 0xFFFC: reached whole;
- 0xFFFE or 0xFFFD: the first half reached of a diagonal tile (the wall byte's 0x10 or 0x20);
- 0xFFFB: shared;
- the room's id: done.
A popped tile not yet reached is marked, pushed back and appended to the room's list. That is
why the list repeats tiles: a tile is appended each time the walk comes back to it through
another half or edge. Each of the four directions is then tried in order. A direction is open
when the tile's wall byte has no wall on that edge, or the wall's pattern is 2, 12, 13 or 14
(these let the walk through). Across an open edge, the neighbour is pushed by its state.
Diagonal tiles go half by half through the half-tile lookup 0x80234910 and the half setter
0x80234588 (r6 1 to 4: which half).
After the stack empties, the room is made (0x8023443C) and given its list (`Room_SetTiles`). A
room left with no list is dropped and its counter taken back.

The room walk for whole tiles (iteration 99, `light::room_tile_lists`). A pop of a tile not yet
reached (only ever the seed) marks it, pushes it back and appends it. Any other pop marks the
tile done. Either way the four directions follow, in the order −x, +x, −y, +y. A neighbour on
the lot (39 × 39 on both logged lots), not done and not diagonal is appended and pushed if its
edge is open. It is pushed even when it is already on the stack. An edge is open when the
neighbour has no wall on its side of it (`Walls::wall_into`: the original tests only that
byte, masks 8, 2, 4, 1), or the wall's style is 2, 12, 13 or 14 (`Walls::style`).
Against the original (lockstep `LOCKSTEP_ROOM_LISTS=LOG[,SNAPSHOT]` with `LOCKSTEP_DUMP_PLACES`
and `LOCKSTEP_DUMP_WALLS`), five of the nine logged rooms come out entry for entry: home's
34-entry room, and the club's 36-entry room and three one-tile rooms. The other four (home's big
room and kitchen, the club's outside and 400-entry room) first differ at a diagonal tile, and
the walk's halves are next.

The walk's diagonal halves (iteration 100). The half rooms are a map by tile: room and side
for each half, a side 1 to 4 naming the two directions it opens (1: +x, −y; 2: +x, +y; 3: −x,
+y; 4: −x, −y), its other half's side opposite (1 and 3, 2 and 4). The setter 0x80234588
inserts the half with the other one free. It removes the entry when the second half is the
same room, and the tile is then done; otherwise it records the second room. Every pop opens all
four directions, except:
- a seed's first pop on a split tile (0x10: side 4; 0x20: side 3);
- a reached half's pop (0x10: 4 or 2, 0x20: 1 or 3);
- a shared tile's pop (its other half's side; appended when that half is free).
A step into a split tile enters the half facing it (0x10: 2 from −x or −y, 4 from +x or +y;
0x20: 1 from −x or +y, 3 from +x or −y). The tile becomes whole when its other half is reached
in this walk. A shared tile is entered only into its free half. A shared tile with a free half
seeds a room as well. The edge test was mended too, to the original's own: the neighbour's
wall bit and the wall's style (was: both sides' walls and patterns).
With both, all nine logged rooms come out entry for entry (lockstep `LOCKSTEP_ROOM_LISTS`):
- home's three: 224, 34 and 35 entries;
- the club's six, its outside the same up to the record's cap of 2,048.

The rooms' window brightness from the engine alone (iteration 101). The share's tile count,
+72 / 2, is the list's half count: 2 for each whole tile's entry, 1 for a split tile's (home's
+72 438, 68 and 68, all the engine's). With the walk's lists, the openings rule on each entry
and `window_brightness`, home's three rooms give the log's brightness bit for bit at tick 100
(1.0136986, 1.0514706, 1.0294118; lockstep `LOCKSTEP_ROOM_LISTS` with
`LOCKSTEP_ROOM_BRIGHTNESS=LOG`). The club's room objects differ between its two recordings, so
they pair up by value instead. The engine's five inside rooms give 1.0277778, 1.0056962 and 1.0
(three one-tile rooms), the log's three values exactly. The outside is not lit this way.

The inside rooms' ambient factors over the whole home replay (iteration 102). The grid holds
persons too, as kind 2. The log's per-entry kinds show Sim 12 taking the archway at (18, 15)
(both its entries) from tick 508 to 535, so the brightness fell to 1.0114155. The openings rule
now counts a person as standing on the floor (`light::tile_counts_as_opening`). Each tick, with
the engine's room lists, objects and the openings rule, the persons on the tiles of their places
before the tick, `window_brightness` and `room_ambient` (the '5' records' clock and day/night
values), lockstep `LOCKSTEP_ROOM_AMBIENT=LOG,SNAPSHOT` gives:
- all 4,000 logged brightness values bit for bit;
- all 4,000 ambient factors.
(The persons' places come from the log, since lockstep's world does not walk them. The grid
follows a Sim's place a tick late.)

The inside rooms' records (iteration 103, `assets roomclock`). An inside room's '2' record
takes words 0-6 from its own ambient object through `room_sun`: zeros, then its colour times its
factor. The rest stays as it was (no sun object). Bit for bit:
- home: 3,000 inside records (and the outside's 1,000 from the clock);
- the club: 3,333 inside records (and the outside's 667), from work/logs/club-rooms.log.
Every inside record's factor is one the logged room ambient ('5') gave by then. So the whole
inside chain is the engine's: clock, day/night blend, the room walk's lists, the openings,
brightness, and the record. (club-l.log, recorded in iteration 91, holds a single '3' call
and no '2' records; club-rooms.log holds them all.)

The floor groups' colours from the room records (iteration 104, `assets floorchain`). A room
id's light record, which Light_RoomTriples reads at the light state + id x 80 + 0x5018, is the
'2' record that Light_RoomSun writes (home's light state at 0x80F33838, the club's at
0x80F35248, found by pairing records by value). Taking each room id's last '2' record before the
call, through `room_triples`:
- the 'a' records: home's 4,000 and the club's 4,000 (club-rooms.log), the same record and
  triples bit for bit;
- the relit groups' 'b' bases and scales, through `group_colour` with the logged material: all
  1,200 of home's and 869 of the club's.
Since the '2' records are the engine's from the clock and the room walk (iterations 90-103),
the floor's colour bases and scales are the engine's, except the materials (logged) and the
light grid sums (the lights' positions: the lamps wait for stage 4.4's poses).

The floor groups' materials from the disc (iteration 105, `assets floormaterials`,
`Shader::material`). The material Light_GroupScale reads is the floor's shader's: its diffuse
is the head's words 7-9 and its ambient words 4-6 (after the layer count's two bytes). The
floor patterns hold 1.0 and 1.0, or 0.7 and 1.0; the terrain shaders hold 1.25 and 1.25, except
terrain_grass_dead and terrain_hollywood_house_under_filler. A group's tiles are its entries'
(x, row) as the floor grid's (x, y), first to last x inclusive. A diagonal tile (value 255)
keeps its halves' floors elsewhere, so it is skipped. A group on no floor takes the lot's
terrain. With the floors from lockstep's `LOCKSTEP_DUMP_FLOORS`:
- home: all 1,200 relit groups are their tiles' material (100 the terrain, terrain_mom_house);
- the club: all 869 (67 the terrain, terrain_club_rubb; club-rooms.log).
No group spans two materials.

A lot's terrain (iteration 106, `quickdata::houses`). SimsUI's `HouseData` table has 17 rows of
44 bytes, row k being house number k (the lot's global 10). Word 9 is the outer lot model (the
surroundings, `models.arc`) and word 10 the terrain shader (the lot's bare ground). Home is house
1: outer_lot_mom_house, terrain_mom_house. The club (the disc's lot 15) is house 16:
outer_lot_freeplay_03, terrain_freeplay_3. That shader's colour layer is
terrain_mimi_house_color, and the outer lot model's own ground is terrain_club_rubb. The club
log's set-up textures hold both colours. Home's hold terrain_mom_house_color only. With the
house's terrain, `assets floormaterials LOG FLOORS HOUSE` gives every relit group's material
as before: home's 1,200 (100 terrain) and the club's 869 (67). The terrain shaders all carry
the 1.25 material, so the textures are what tells the row apart. `replay`'s
`REPLAY_SHOW_LOT_CLOCKS` now also prints each lot's house number.

The objects' nearest lights (`light::nearest_lights`, the original's 0x8016906C, iteration 107).
At a rebuild, for each light of a range:
1. it takes the light's object (the light entry's +28) and gets its position (the object's
   vtable +304);
2. it takes the object's room (+48 on +56, or -1);
3. it ranks that room's own lights (its record's words 18-19) into 4 empty slots (`light::rank`);
4. it stores each slot's light pointer, 0 when empty, at the manager + 0x4014 + 16 x the light's
   index.
The window fill reads that list up to its first gap, so each fill uses the last rebuild's
lists. A window light's room is the wall's other side (the room under its window's other light),
which is also why an inside light's fill adds the outside's sun. Checked by lockstep
(`LOCKSTEP_WINDOW_LIGHTS=LOG`): home's 28 window lights, each at the engine's place, ranked
over the engine's other-side room at the grid build before each fill. All 3,916 fills after a
build get the fill's list, light for light. The 84 fills before any build (each light's first,
at tick 0) read lists not made yet.

The other lights' array from the engine (iteration 108). Each window (leading object, the
list's order) gives two lights, front then behind (`window_light_places`). Each light is in the
room under its own place, with the room under the other as its other side. `sort_lights` takes
them room by room, giving the array's order. The height and reach are `window_light_params` by
the window's kind (`window_kind`) and whether the light's own room is inside. Checked by lockstep
(`LOCKSTEP_WINDOW_LIGHTS=LOG`) against the log's last rebuild: home's 28 lights in order, each
place, height and reach bit for bit. The club has none, as the engine gives. The lights'
colours are the window fill's (iteration 82). They need the nearest lamps, whose places follow
the objects' poses (stage 4.4).

The light grid built from the engine (iteration 109). For the grid build after the last rebuild,
lockstep builds three things:
- the cells' rooms: `cell_room_adds` over the engine's tiles, through `CellRooms::add`;
- the other ranges: `sort_lights` over the engine's window lights;
- the other lights: the engine's array (place, height, reach).
The own ranges and lights (the lamps) stay as logged: their places follow the objects' poses
(stage 4.4). Then every cell room's four lights come from `cell_lights`. Against the log's 'e'
build:
- home: 1,296 cells' rooms all the same, and all 1,364 cell rooms' lights;
- the club (club-rooms.log, no windows): 1,296 and 1,386, all the same.
(`LOCKSTEP_WINDOW_LIGHTS=LOG`; club-l.log has no rebuild with rooms to check against.)

The cursor at the lot's edge (`scene::Camera::move_cursor`, the original's fn_80029BF8, iteration
110). The cursor's step (the stick squared, times the zoom's cursor speed and dt, turned by the
camera's angle) is added to the cursor. An axis that leaves 1 ..= size - 1 has its part of the
move dropped, and the cursor is clamped there. The camera's CursorMoved then carries only what
is left. The size is the lot's (a virtual call, 39 on both lots). Home's last unexplained target
move (tick 225: the cursor's x from 1.17 to the edge at 1, the target moving in y only) is that
rule. `assets camlog` now explains all 164 of home's target moves (1 centring, 145 carried, 1 of
them at the edge, 18 carried and pulled) and all 1,062 of the club's.

The Sims' draws (iteration 111, `assets near`, `costumes`, `cardfile`). At tick 1,000 of
home-d.log, every world matrix drawn within a tile of a Sim is a posed bone's: the outfit's parts
(am_ub_1c_tight_01, af_ub_1c_robe_01, am_ub_jacket_leather, af_jw_mom, eyes, mouth, nose) and
its shadow (shadow_adult), turned and lifted by the pose (the hips about 1.1 up). None of them is
a root at the Sim's place, so the Sims' matrices need the skeleton posed (stage 4.4). Which parts
a Sim wears: the parts are models by name hash (crc32 of the upper-case name, as the shaders').
The hashes are not in the card's save. The base bodies are rows of CreateASim's `Sim::Costume`
(136 rows of 44 bytes: the upper body at word 0, others at 2, 4, 6, 8, 10; am_ub_1c_tight_01 in
rows 118, 126, 130). The robe, jacket and jewellery are in other tables: a Sim's outfit is
layered, and how the save names it is still to read.

## Animations

`animatio.arc`'s 4,013 entries (`anim::read`, iteration 112, `assets anims`), all read to their
ends. The layout is The Sims 2's `ERAnim::OldLoad` (its map names it; its ELF read with
llvm-objdump). The same fields sit there behind an "ANIM" chunk header. Here they follow the
animation's name and its NUL, all big-endian and packed (nothing aligned):
1. the frame count (u32);
2. two float triples (1, 1, 1 on all seen);
3. the nodes: a count, then 28 bytes each (seven words; The Sims 2's are 32). On the pool
   table's, the third word is 3k + 1 and the fourth -1 - 66k (66 its frame count);
4. a word (a float on the pool table, 1.59);
5. the constants: a count, then floats (the tracks' values);
6. a bit count, then its bits as words (count / 32 rounded up);
7. the frame rate (30.0 on 2,475 entries, 60.0 on 1,538), two floats, a byte, four words, two
   bytes;
8. the events: a count, then 80 bytes each (a 64-byte name, four words). It is 0 on every entry,
   so the scripts' animation events come from elsewhere.
The engine's ERAnim::Load takes the frame count over the rate as the length (+20, in ticks by a
constant). In all: 190,424 nodes and 265,862 constants. 3,050 animations have 58 nodes (the
adult Sim's), the largest 248.
The quick data's `AnimRef` rows (SimsObjects, 2,891) name animations by the entry's id: every
row's id is an `animatio.arc` entry's (`assets anims`; iteration 113). The row's second word
is not the length: paired rows share it (an apology's accept and reject, 0xB32 for 86 frames),
and 0xEF9 stands beside a 116-frame entry.

### Animation channels (stage 4.4, iteration 147)

Bustin' Out's evaluator is fn_801581C8, EA's `EAnimController::Animate(EACTrack *,
EACNodeState *)`. It was found by the order of its neighbours against The Sims 2's map: after
`FixAccumulationForRootNode` (0x44 in both, 80158184), before `Blend` (0x190, 801587D4) and
`Layer` (0x1FC, 80158964). For each node it reads four channel words. 0 is no channel (an
identity, the constants at 802B7F38/7F3C). Positive is an index into the animation's
constants. Negative is a stream, read at the track's place (+60) by a decoder:
- rotation: fn_801B5F48;
- scale and translation: fn_801BC4E0;
- the fourth channel: fn_801BD4D4.
The file's node words (`assets animnodes`, all 4,013 entries) give each channel. Word 1 is the
scale: 4,200 constant (uniform triples like 1.61, 1.15, 0.10), the rest none. Word 2 is the
translation: 16,003 constant, 20,690 a stream. Word 3 is the rotation, a stream on every one of
the 190,424 nodes (the pool table's -1 - 66k). Word 4 is a fourth channel: 186 constant (triples
in 0 to 1), 50 streams. Every positive word's three constants are inside the animation's. The
streams' encoding (the bit array, the "-1 - 66k" indices) is the decoders' and still to read.
The scale and translation decoder is fn_801BC4E0 (named Anim_Vec3Stream in the recomp,
iteration 148). It takes the stream (r4), the place (f1) and three floats to fill (r3). The
stream keeps a window of two key frames, its first (+12) and last (+16). While the place is past
the last it steps forward (fn_801BC958); before the first, it steps back (fn_801BCA8C) or reads
again from the start (fn_801BCC28, fn_801BCBBC). Inside the window t = (place - first) / (last
- first). With the mode byte (+33) nonzero it blends the keys P0 (+44) and P1 (+80) straight.
Otherwise it takes a cubic Hermite curve with the tangents T0 (+56) and T1 (+68):
P0 (2t³ - 3t² + 1) + T0 (t³ - 2t² + t) + P1 (-2t³ + 3t², one fused multiply-add) + T1 (t³ - t²),
summed in that order. The engine's `anim::channel3` does the same f32 steps. Checked by the
observer's new '%' records (the window as the call leaves it, and the result) on home-a12.log (now home-a13.log):
157,750 reads, every one the same to the bit (51,150 curves, 106,600 straight). How the keys
come out of the stream's bits (the stepping functions) is next.
The keys (iteration 149). A stream steps its window forward with fn_801BC958. The old last frame
becomes the first, and the new last is the old last + 1 + the frame gap of the record two keys
on. The old P1 becomes P0, and the new P1 is key (+8) + 1. The mode byte is set when the place
moved since the last read (+40) by at least the window's frames × the constant at 802C3848.
So a jump (fast-forward) blends straight, and otherwise the two tangents are worked out from
the neighbouring keys (fn_801BCEF4 kinds 0 and 1: differences scaled by 802C3878 and 387C,
with a case for streams of 2 keys at +24). The tangents are not stored. A stream's bits are an
array of big-endian words: the +0 object's first word points at it, in the game's 0x7E000000
virtual memory. Fields are read low bits first and run on into the next word (fn_801ADA74,
`anim::bits`). A key's record starts at bit +4 + key × the record's bits (s16 +28): the frame
gap (+30 bits), a signed scalar (+32 bits, times +36), then the three values (+31 bits each,
unsigned). Each value is the field times its scale (+92, +96, +100) plus its offset (+104, +108,
+112), one fused multiply-add (fn_801BCE50, `anim::key3`). Checked through the observer's '&'
records (the stream's header, and four words of its bits at P1) on home-a13.log: all 157,750
P1s the same to the bit. Where the header comes from (EAnimController::CreateStreams, from the
node's words, its constants and the bit array) is next.
The stream header (iteration 150) is in the animation's bits, at the stream's first bit. It is
read by fn_801BC318, the stream's init, which then resets the stream to its start (fn_801BCBBC):
- the key count (20 bits);
- the widths of a record's frame gap, its scalar and each of its three values (5 bits each; a
  value width of 0 means 32);
- for each value, its range and its minimum as two raw 32-bit floats (fn_801ADB1C), giving the
  scale range / (2^width - 1) (fn_801ADC18) and the offset minimum;
- the scalar's factor 1 / (2^(width - 1) - 1) (fn_801ADBCC; the constant at 802C3820 with no
  scalar).
The records follow, so they start 227 bits after the header. A record's bits are the gap +
the scalar + 3 x the value, and the frames are the sum of the gaps + 1. `anim::stream_header`
reads it. Each of the 290 streams on home-a13.log is found in one animation's bits at its first
record - 227, and every field is the same to the bit (`LOCKSTEP_ANIM_HEADERS`). Still to read:
the stream's first bit from the node's word (CreateStreams), and the rotation stream.
A stream's first bit is its node word negated (iteration 151, `anim::Channel`): all 290 logged
translation streams start at -(word 2) (parrot_cas_loop1's node 2: -7954, bit 7954). Word 0 is
set on most nodes, both signs (79,228 negative, 50,067 positive, 61,129 zero), so it is
something else, maybe the bone's id. Words 5 and 6 are as rare as word 4 (199 and 215 positive,
37 and 21 negative). The streams are not packed from bit 0: a walk of headers from 0 does not
reach them, as the rotation streams (word 3, its own format) sit between.
A window from the animation alone (iteration 152, `StreamHeader::window`). Key k's frame is
the gaps of keys 1..=k, each + 1 (801BCD2C). Its scalar is a bias: a signed field times the
header's factor, 0 with no scalar (802C3820 is 0.0, not 1). The window from key k holds frames
time(k) and time(k + 1), P0 and P1 the two keys' points, T0 key k's outgoing tangent and T1
key k + 1's incoming tangent (801BCEF4):
- Inside: ((B - A)(1 + bias) + (C - B)(1 - bias)) x this side's frames / both sides' frames.
- At an end: the one difference with two keys, otherwise 1.5 x it - 0.5 x the next key's tangent.
Moving forward, a window is blended straight (mode 1, tangents not refreshed) when the place
moved at least a third of the window's frames since the last read (802C3848 = 1/3). The start
read's last place is FLT_MAX (802C385C), so the first window is curved.
Check: given only each record's key (+8), all 157,750 logged windows on home-a13.log have the
same frames and points, and all 51,150 curved ones the same tangents, to the bit. A stream's
address is reused by later animations, so a record is matched to its animation by its whole
header.
A stream run from the place alone (iteration 153, `anim::Stream`). The evaluator (801BC4E0)
moves the window to hold the place, then stores the place at +40 and evaluates:
- Past the last frame: step forward (801BC958) while the place is past the window, when the
  place is nearer the window than the stream's end (frames - 1). Otherwise jump to the last
  window (801BCC28: it ends at frames - 1, the last place is -FLT_MAX) and step back (801BCA8C).
- Before the first frame: step back while it is nearer than frame 0. Otherwise start again
  (801BCBBC: key 0, the last place FLT_MAX) and step forward.
A step back blends when the last place - the place is a third of the window or more (802C3858
= 1/3), and keeps P1 = the old P0. A start or jump keeps the blend flag.
A stream starts (801BC318) whenever a play starts its animation, and later streams reuse its
memory. The '&' snapshot's word at +32 holds the blend flag (+33), so a stream is told apart by
its header without that byte. Iteration 153's "rebind in place" came from keying on that
whole word and was wrong. All 153 animation changes at an address on home-a14.log start a new
stream. The one read left (tick 141 after the last) is a play restarting the same animation:
the same header, a new stream.
Of the 157,750 reads, 157,749 have the same key,
window, flag and result to the bit, given only the places (iteration 154). home-a14.log is home-a13 again
with the observer logging every read whatever the result's address. It has the same 157,750
reads, so none had been skipped, and the same 562,616 steps.
A stream's place (iteration 155). The controller's Animate (801581C8, now hooked as
Anim_Animate: '(' with the track and its place, ')' after) poses one track. The track's +148
holds a 28-byte record a node, with +0 the rotation stream and +4 and +8 the scale and
translation streams. +152 holds the weights. Every stream is read at the track's place +60.
On home-a15.log, all 157,750 reads are inside a track's pose at exactly its place. 419 of 430
streams belong to one track; the others are addresses reused by a later play. A stream's
bit array object (+0) is the animation + 88, and that animation is shared by Sims playing it.
Of the reads, 146,334 come after their track's step that tick, 10,562 before it (posed first),
and 853 at another place. The order of poses and steps within a tick is still to read.
When a track is posed (iteration 156). With the log's records ordered by their offset
(`TrackRec::at`, `PoseRec::at`), a track is posed after each of its steps, at its place then.
Outside loading, all 477,899 poses on home-a15.log fit: 273,991 in ticks of one frame (step,
pose) and 203,908 in ticks of two (step, pose, step, pose). 18 are poses of a track not
stepped that tick, at its standing place. Iteration 155's "posed before the step" were the
first pose of a two-frame tick. During loading (tick 0) the order differs (12 of 963 poses),
which leaves the picture unchanged.
A track's step (iteration 157, Anim_UpdateTrack 80157318). Once a frame, the place +60 becomes
(float) fma((frame time x the controller's +80) x the track's +72, the track's +0, place).
The fma is double precision with one rounding to single (fmadd then frsp). The frame time is
r13-29720 = 0.033; the controller's speed is 5 in fast-forward; +0 is the rate. Past the last
frame (the animation's +32 frames - 1) a play stops there or, looping, comes back by the last
frame as many times as it takes. Before 0 it stops at 0. `anim::track_advance` and
`anim::wrap_back` give all 186,272 moved steps on home-a16.log to the bit: 145,219 on, 82
stopped, 40,960 wrapped, 11 stopped at 0. Two separate single roundings miss 616 of them. The
observer now reads the animation's head from the 0x7E virtual memory too (home-a16.log: the
same 562,616 steps).
A track's ends (iteration 158). The track's byte +13 is its end mode, now logged as '*' after
each step. Past the last frame or before 0, the step sets done (+160 = 1):
- Mode 0 (a loop) comes back by the last frame, as many times as it takes. Before 0 it goes on
  by the last frame until it is not below 0.
- Modes 1 and 2 stop at the end they passed. Mode 2 also sets a flag, r22, not yet followed.
A track already stepped this frame (+52 = the frame) is kept as it is. `anim::track_step`
gives every step on home-a17.log, place and done, to the bit: 482,150 in mode 0 and 49,841 in
mode 1. 12 had already run that frame. No mode 2 occurs on this replay.
A play's start (iteration 159). A track's animation (+120) carries its archive id at +4, its
frames at +32 and its rate. A track takes its rate (+0) and end mode (+13) from the animation.
The end mode is the animation's second flag byte (`Anim::end_mode`): 1 stops, 0 loops. On
home-a17.log, all 690 play starts have the animation's frames, rate and end mode. The track's
own speed (+72) at its first step is 1 for 670 of them, -1 for 10 (backward), 0 for 7
(paused) and 5 for 3. Tying the speed to the Animate call that started the play is still to do.
An Animate call's play (iteration 160). A Sim's plays run on one controller, and the play is
the track of that controller with the run's animation (archive id at head +4). Runs that end
with an event chain into one play: the next run of the same animation starts the tick the last
ended. Only the chain's first run starts a play; a play started within two ticks of it was
found for 22 of 54 chains. Track slots change across a chain, so a play is known by
(controller, animation), not by track. On home-a17.log, both Sims keep one controller. For 128
of 152 runs, that controller plays the run's animation on every tick of the run. The other 24
differ at the run's first or last tick (load waits and the end tick), which is still to read.
The 24 runs with ticks off their play (iteration 161). Every tick of a run without the run's
animation on its Sim's controller is explained:
- 126 ticks: the controller was not stepped at all that tick. This covers a first tick waiting
  on the animation's load (the whole controller waits) and spans where it steps every fourth
  tick (414 to 573).
- 4 ticks: the run's last tick, the play done the tick before.
So for all 152 runs, the run's play is the controller's track with its animation whenever the
controller is stepped. When a controller is stepped is the next question.
When a controller steps (iteration 162). Animation steps go by frames, not ticks: a tick holds
0, 1 or 2 frames (home-a17.log: 105, 4,929 and 1,835 ticks). Of the 130 run ticks off their
play:
- 116 had no frame (no track stepped at all). This covers the every-fourth-tick span 414 to 573.
- 10 were ticks where an Animate load missed. The Sim's controller waits while the others step,
  as iteration 136's rule had it.
- 4 were end ticks after the play was done.
So a Sim's play steps once per frame, except on a tick its Animate's load misses.
Plays run natively (iteration 163). Each track's play, (controller, track, animation), is carried
by the engine from its first frame with `anim::track_step`. Its inputs are the animation's rate,
frames and end mode, and the logged speeds (+72 and the controller's +80). On home-a17.log, all
531,991 steps and 531,312 carried places are the same to the bit. A play starts at 0 (632), or
at its last frame when it runs backward (12). It keeps its place over 1 or 2 frames without a
step (47). One controller can hold the same animation on two tracks at once, so a play is keyed
by its track too.
A track's speeds (iteration 164). On home-a17.log:
- The objects' controllers hold the game speed at +80: 1, 0 paused (ticks 23-84, 2048-4911,
  from 5630) or 5 in fast-forward (426-563, the span with a frame every fourth tick). It is the
  same for every object controller in a frame. Their tracks' +72 is 1 (or 0 for 463 records of
  still tracks).
- The two Sims' controllers hold 1 at +80. Their tracks' +72 is their animator's speed (the '#'
  records, the animator's owner being the controller - 0x350): the same in magnitude on 52,379
  of 58,944 records, negative on backward plays.
- The other 6,565 Sim records hold +72 at 0: happyFace held its whole play, and walking,
  standing, carrying and sitting loops stopped for part of theirs.
Where the hold and the stop come from is the next question.
A track's speed set (iteration 165). The track speed (+72) is written in three places: the
track constructor (fn_80156340, 1), Anim_SetTrackSpeed (8015AB80: the controller, a track id
looked up at the controller's +44, the speed) and the stack locals of the blend code. The
observer now logs '^' for every Anim_SetTrackSpeed call (home-a18.log, 562,616 steps as
home-a17's). All 172 calls on the replay come through fn_801C22C8 (return 801C2334). It sets
the speed only when it differs from the track's current one, and some of those calls go to
objects' tracks. On the 58,944 Sim track records, +72 is:
- its own last set: 54,030, all the 0s among them (happyFace's hold, the loops' stops);
- the animator's speed on a track never set: 4,362;
- the animator's speed over an older own set: 552.
Who sets a track's speed (iteration 166). The '^' record now also carries the wrapper
fn_801C22C8's caller (home-a19.log, 562,616 steps). The 172 sets come from:
- fn_800ABE20, the most (6 call sites, 800ABE84 to 800ABFC4);
- fn_800AF06C (800AF190) and fn_800AF964 (800AFA58);
- fn_800B0ADC (800B0BB4: the -1 sets, backward plays; 800B0BE4).
The speeds set are 0, 1 and 5, the game speeds, so these are the game speed reaching the
tracks. On the Sims' tracks:
- 36,561 records are still while the game is paused (the animator's speed is 0 too);
- 6,554 are happyFace, held at 0 at any game speed;
- 474 are still on frames where the game speed is changing (the objects not yet agreeing);
- 24 are short stops at speed 1.
The animator's six play slots (iteration 167). fn_800ABE20 runs on a Sim's animator. When its
speed (+124) differs from the last one (+128), it sets the new speed on six tracks, each named
by a slot of the animator. Each track is looked up at the controller's +892 and set through
the controller's +816. By the animations the sets reach on home-a19.log, the slots are:
- +224: the standing (or sitting) loop, the base idle (61 sets: a2o-/c2o-standing-loop, a2o-sit-loop);
- +188: the walking loop (9: a2o-walking-loop, -female-loop, c2o-walking-loop);
- +184: the turn (a2o-standing-turn-90-cw);
- +164: the current action, the Animate's own animation (51 over 23 animations);
- +212: the action before it, still blending out (6);
- +220: the carry overlay (16: a2o-rarm-carry-loop).
Elsewhere:
- happyFace is set by fn_800AF06C (800AF190, 3 sets: the face, held at 0).
- The walk's turns are set by fn_800AF964.
- Backward (-1) and new-play sets come from fn_800B0ADC.
A new action and the face (iteration 168). fn_800B0ADC(animator, animation, backward) starts
an action:
- It takes a fresh track id (+192) into the action slot +164 and starts the play on it
  (fn_801C1FC0).
- Backward (r5 = 1): it sets the speed to minus the animator's (+124), and the place through
  fn_801C21C8.
- Forward: it sets the animator's speed.
- Then fn_801C234C, with a constant.
The wrapper sets only on a change and a new track starts at 1. So on home-a19.log a forward
action at speed 1 makes no set, and the 14 forward sets are 0 (13, paused) and 5 (1). All 12
backward sets (-1) are the 12 plays whose first step is from the last frame. fn_800AF06C
starts the face (slot +800) with a constant speed, the 802A8E44 single: 0 on all 3 of its
sets, so happyFace is held at its first frame.
The engine's Sim animator (iteration 169). `anim::SimAnimator` follows the game speed, and on a
change it sets that speed on the Sim's tracks other than the face. A new action gets the speed,
or minus it when its Animate operand is backward (flags bit 2), and the face gets
`FACE_SPEED` (0). It is checked on home-a19.log, with the game speed taken from the objects'
controllers. Of the 58,944 Sim track records:
- 58,253 are the same.
- 441 are within 3 frames of a game speed change or at loading: the Sims' tracks take the new
  speed a frame before the objects' controllers show it.
- 225 are backward idle transitions (a2o-idle-neutral-trans-*). They come from the idle code,
  not an Animate, so they have no operand.
- 3 are an eat loop's direction, and 22 are short holds at 0 (standing, carry, sit, stove).
The direction of every Animate (iteration 170). fn_800B0ADC has one caller: the Animate
primitive (fn_800AC854, 800ACE4C), with r5 = its operand's flags bit 2. The idle transitions
are Animates too, ones naming their animation by a variable (flags bit 4). Their direction is
the operand of the Sim's own Animate run started up to 10 ticks before the play. Walks and
turns come from routing, not from Animate, so they always run forward. The engine's animator
now matches 58,447 of the 58,944 Sim track records on home-a19.log. Left:
- 441 at game speed changes. A one-frame lookahead of the objects' speed makes it worse
  (455), so the Sims' change frame is not simply the next frame.
- 31 idle transitions.
- 3 of an eat loop.
- 22 short holds at 0.
The animator's speed in the log's order (iteration 171). Drive each Sim's `SimAnimator` from its
own speed records ('#', the owner + 0x350, at their place in the log) instead of the objects'
controllers. Then all 58,944 Sim track records on home-a19.log have the right +72, including
the 441 at game speed changes, the idle transitions and the short holds. The 17,759 records
change only:
- at the game speed changes: pause 1-2, 23-85 and 2048-4912, fast-forward 426-567;
- on one-tick dips to 0 for a single Sim, at 18, 208, 994, 1007, 1208, 1418, 1553, 1618, 1632,
  1700, 5013, 5026 and others. These are the ticks its Animate's load misses (iteration 162):
  the animator stops while its animation loads.
So a Sim's animator speed is the game speed, and 0 on a tick its Animate waits on a load.
The animator's speed from the engine's state (iteration 172). Some object controllers never
leave 1 at +80, so the game speed comes from the controllers that follow it, the ones whose
+80 ever changes. With that game speed (the tick's frames) and 0 on a Sim's load-miss tick,
17,177 of the 17,198 '#' records on home-a19.log are predicted. The 21 left are the one-tick
dips. All 13 dip ticks have an Animate load miss that tick, but tying the miss to its Sim
through the Sim's Animate run finds only 6. So the rule holds (a Sim's animator stops on the
tick its Animate's load misses), but which Sim a miss belongs to needs the observer. The '0'
ask names no Sim, and the Animate runs are named runs only.
Whose animator a load miss stops (iteration 173). A miss belongs to the Animate step logged
next, and on home-a19.log that Sim's animator dips on all 13 miss ticks. The other Sim dips too
on 8 of them: foodprep at 208, starteat/eatloop/stopeat at 994/1007/1208, idle4 at 1553, the
easel at 5013/5026, and fidget-1b at tick 1. It does not dip on 5 idle misses (fidget-1c at 18,
trans-lstand at 1418, the idle transitions at 1618/1632, fidget-1a at 1700). Counted on the
17,198 '#' records:
- stopping only the missing Sim: 17,187;
- stopping every Sim: 17,191.
Neither is exact, so what stops the other Sim is still to find, from the writer of the
animator's +124.
The animator's speed source (iteration 174). Anim_SpeedUpdate (800AB820) writes the animator's
speed (+124) from the game state object at r13 - 23196 (80ecdcf4 on home-a20.log, 562,616
steps). The '#' record now also logs that object and its four virtual functions. If any of
three stop tests is nonzero, the speed is the 802A8BC8 constant:
- fn_8023860C (+148): +1208, then fn_80239C54, a further virtual (+440) and +1090;
- fn_80239E10 (+108): +1428 = 0 and +1424 != 0;
- fn_80238570 (+156): +1432.
Otherwise the speed level, the s16 at +1088 (fn_80238468, +76), picks one of five constants:
0, -1, -2, -3 or other. The update returns early when r13 - 28672 is 0, the same flag
fn_801C22C8 tests. These are global, not per Sim. So a dip of one Sim but not the other means
the state changes between the two Sims' updates in the tick.
The animator's stop rule (iteration 175). Anim_SpeedUpdate has a second path (L_800ABB58),
taken when r13 - 28672 is 0, which it is on every record. That path stops the animator
(+124 = the 802A8BC4 constant, 0) when:
- the global count at r13 - 31628 is above 0 (loads pending),
- and the animator is not exempt: its +16 is not 1 and its +28 is not 5.

Otherwise the game state's stop tests and speed level apply, as on the first path. The '#'
record now logs the mode, the count, and the animator's +16 and +28, as they were before the
update (home-a22.log, 562,616 steps). With the game speed from the pausable controllers, this
predicts all 17,198 '#' records:
- 30 records stop with loads pending and the animator not exempt;
- 10 run on with loads pending because the animator is exempt.

So a Sim stops on another Sim's load miss unless it is exempt. That explains the dips of both
Sims and of one.
What the stop rule reads (iteration 176). On home-a22.log the pending count (r13 - 31628) is 1
on exactly the 13 Animate load-miss ticks (1, 18, 208, 994, 1007, 1208, 1418, 1553, 1618,
1632, 1700, 5013, 5026) and 0 otherwise. The animator's +16 and +28 are its mode:
- +16 = 1 while routing, with +28 1 (start: turns), 2 (walking, 9,000 records) and 3 (stop);
- +16 = 0 with +28 = 4 while playing Animate actions and idles (7,902 records);
- +28 = 5 is rare (2 records).
So the rule is: on a tick an Animate's load misses, every Sim's animator stops except the ones
walking a route. The engine knows both: its Animate's load waits and its routing state.
The stop rule from the engine's state (iteration 177). `SimAnimator::speed_for(game_speed,
load_miss, routing)` is the game speed, or 0 on a load-miss tick unless the Sim is walking a
route. Its inputs come from the engine's side only:
- the game speed of the pausable controllers;
- the Animate load-miss ticks;
- routing, meaning no Animate run of the Sim covers the tick.

It gives 17,197 of the 17,198 animator speed records on home-a22.log. The one left is a Sim
in an Animate the run list misses (not routing by the log's +16).
The rotation reads (iteration 178). The observer hooks the rotation decoder as Anim_RotStream
(801B5F48, '!'). It records the stream, the place, the stream's words +0..+44 (the window's
times +8/+12, frames +16, the packed header at +28..+36), the mode byte +33, the window's 16
words (+48..+108, four quaternions) and the four floats out. The first 300,000 reads of
home-a23.log (562,616 steps; up to about tick 600) cover 851 streams, all in a track's pose.
- Every result is a unit quaternion.
- 294,193 reads are mode 0 and 5,807 are mode 1 (the blend, as for the vec3 streams).
- The window's four quaternions are not a plain slerp, nlerp, squad or Bezier of the place:
  the best fit is 21% within 1e-4. So the evaluation is to be read from the code.
The rotation window (iteration 179). fn_801B5F48 steps its window like the vec3 streams (key
times at +8/+12, t = (place - k0) / (k1 - k0)). In mode 0 it evaluates the window's four
quaternions w0..w3 (+48, +64, +80, +96):
- m = lerp(w1, w2, t);
- the result is lerp(lerp(w0, m, t), lerp(m, w3, t), t), normalized.
Each lerp is a + (b - a) * t in separate single operations. The normalization builds the
squared length as y*y, then x*x, z*z and w*w fused in, each rounded to single. Then it takes
sqrt (fn_8010DF80), k = 1 / length (802C35B4 = 1.0), and each component times k (a zero
length leaves it as is). `anim::rotation_window` gives all 294,193 mode-0 reads on
home-a23.log to the bit. A plain slerp, nlerp, squad or Bezier matched at best 21%, and a
division instead of 1/length times only half. Mode 1, the blend, starts from w3 - w0.
The rotation blend (iteration 180). Mode 1 (after a jump, as for the vec3 streams) is
lerp(w0, w3, t), normalized the same way as the window (fn_801B5320 scales the difference by t
in single operations). `anim::rotation_blend` gives all 5,807 mode-1 reads on home-a23.log
to the bit. With `rotation_window`, every one of the 300,000 rotation reads is the game's.
Rotation streams from the bits (iteration 182, `anim::RotHeader`, `anim::RotStream`). The
rotation stream's init is fn_801B5DC8 (the vec3's fn_801BC318's sibling):
- The header has 20 bits of key count (+24), then 5 bits each for the gap's width (+30), the
  bias's width (+32) and the point component's width (+31, 0 meaning 32). The first record
  starts 35 bits after the header.
- A record is the gap, the bias, y, z, w (signed), then one sign bit for x. Its stride is gap +
  bias + 3 x point + 1. The scales are 1 / (2^(w-1) - 1) in single (801ADBCC), and the bias's
  is 0.0 with no bias. Key k's frame is the sum of (gap + 1) over keys 1..=k, and the frames
  (+16) are that sum over every key, key 0 included.
- The point (801B6B40): x = sqrt(1 - s), s being z*z with y*y then w*w fused in, each rounded
  to single; x is 0 when 1 - s <= 0, and the sign bit negates it.
- The window (801B6C44) holds P_k, key k's outgoing control, key k+1's incoming control and
  P_k+1. At the first key the control is unit(P + (P_next - P) * (bias + 1) / 2), at the last
  unit(P + (P_prev - P) * (1 - bias) / 2). Between, X and Y are the moves toward the previous
  and next keys, weighted -(bias + 1) / 2 and (1 - bias) / 2, M = unit(X + 0.5 (Y - X)), and the
  control is unit(P + (s * r * -2) (M - P)), r being this side's frame gap over both, s +1 in,
  -1 out. A zero length gives the identity (801B52F8). The constants are 802C35F0..3614.
- Stepping (801B6638, 801B6774, 801B68AC, 801B6918) is the vec3 stream's, the same blend after a
  jump; the last place is reset to +/-FLT_MAX.
On home-a23.log (`LOCKSTEP_ANIM_ROTATIONS`) all 628 headers decode from their animation, and
all 300,000 reads' windows and results come out the same to the bit from the places alone. The
rotation streams start at the negated node word 0 (all 628), not word 3 as said above: word 3,
negative on every node, is something else.
A node's seven channels (iteration 183). EAnimController::Animate (801581C8) reads a node
record's words +4..+28 (the engine's node words 0..6): +4 the rotation (801B5F48), +8, +12 and
+20 vec3 streams (801BC4E0), +16 a flag stream (801BD4D4, read only when the node state's bit
0x10 is set), +24 and +28 scalar streams (801BD7BC, init 801BD614; never read on the home
replay). The flag stream (init 801BD4C8) is the bits object and the start bit (the negated node
word 3). Its read is the bit at trunc(start + place), 0 past the bit count (801AD898): one bit
a frame. On home-a24.log all 2,285 flag streams and 300,000 reads match (`anim::FlagStream`,
`LOCKSTEP_ANIM_FLAGS`). Per node, Animate builds a 64-byte local pose from the channels, then
with the weight (the track's +76 times its +152 entry for the node) skips it at 0, layers it
(Layer 80158964) when the track's +8 has bit 0x100, copies it at weight 1, else blends it (Blend
801587D4) into the node state (64 bytes a node). The observer's '=' records (home-a25.log, the
first 40,000 poses) hold every node state before and after the pose.
The bones (iteration 186, `skeleton.rs`, `LOCKSTEP_ANIM_BONES`). EAnimController::ComputeMatrices
(80158B60) turns the final node states into one matrix a node. A character node (180 bytes,
from characte.arc by name) holds +0 the parent, +16 the rest translation, +28 the rest rotation
(x, y, z, w), +44 and +108 two matrices (the second the rest matrix), +172 a flag word. A state's
flags: bit 0 posed (else the node takes its parent's matrix), bit 1 a scale, bit 2 the rest path.
A posed node's local matrix is its rotation (801B48D0), rows scaled by the state's scale
(801B28F0), moved by minus the rest translation (801B2988) and then by the rest translation plus
the state's (801B345C); node = local x parent (801B2888, PSMTX44Concat), node 0's parent the root
matrix; the 3x3 times the controller's scale (+84) when it is not 1 (801B28B8). ComputeMatrices
writes the matrices' translations back over the states' words 5-7. All 262,948 nodes of the
20,000 bone records on home-a26.log come out to the bit, and all 36 logged skeletons are
characte.arc's. The rest path (state bit 2) is not met on the home replay, and the controller's
scale (seven powers of two, 2^-17 to 2^-11) and mode word (+96) are inferred, not logged.
Posing (iteration 187, `pose.rs`, `LOCKSTEP_ANIM_POSES`). The 64-byte node state: flags, the
rotation (x, y, z, w; channel 0), the translation (channel 2, default 0), the scale (channel 1,
default 1), a vector (channel 4, default 1) and two scalars (channels 5 and 6, defaults 1 and 10,
802B7F40). Flag bits: 0x1 rotation and translation, 0x8 scale, 0x10 uneven scale, 0x100 the
flag channel's (the stream's bit inverted, 80158540; the channel is read when the state's 0x10
is clear), 0x20/0x40/0x80 channels 4/5/6. The fold (801586B4): weight 0 skips, the track's +8
bit 0x100 layers (Layer 80158964), weight 1 copies, else Blend (801587D4). Blend ORs the flags,
turns the rotation the shorter way and takes a + (b - a) w normalized when the dot is >= 0.999
(802C3578), else a slerp (801B4B50; sin and acos, 8010DB14, fdlibm's in single); translation
and scale s + (l - s) w, words 11-15 untouched. Layer ORs the flags, rotation slerp(identity,
q, w) (801B5154) times the state's, translation s + w l, scale s + (s l - s) w. A track's
animation can change within a tick, so a pose's is the one of its last step. On home-a25.log,
with the weights found from the states, 618,836 of 618,895 node states come out to the bit.
Posing with the logged weights (iteration 189). The pose record now carries the track's weight
(+76), flags (+8), +124 and node weights (+152). A node's weight is the track's times its own.
The first non-layer track posed after a reset of the node states (their +88 is 0) sets +88 and
poses at weight 1.0 whatever its weight (Animate 80158254..80158278, `pose::base_weight`). The
root fix (FixAccumulationForRootNode 80158184) takes the translation x from +124. A stream's
blend flag is left over in memory until its first window step sets it. With them, all node
states of the 39,999 poses on home-a28.log come out to the bit: 474,452 copies, 22,587 blends,
1,354 layers and 120,662 at weight 0 (`LOCKSTEP_ANIM_POSES`). The weights come from the track's
SetWeight (fn_80159D9C, clamped) and a mask's array (fn_80162F7C).
A Sim's tracks from the engine's state (A1, `figure::tracks`; the numbers are in "The Sims from
the engine's state"). The animator's tracks that pose a Sim at ticks 1,039 and 1,040 are its
action (weight 1, the base), its carrying loop (weight 1, node weights 1 on nodes 47 to 51)
and the face layer (weight the mood motive over 100, node weight 1 on node 16); the standing
loop steps but is not posed. The node weights of the first 643 ticks' '=' records are two
patterns only (happyFace 57 zeros and a 1; the carrying loop 53 zeros and five 1s); the
face's weight is the Sim's motive 3 over 100 every tick (the '=' records stop at tick 643,
so ticks 1,039 on are checked by the bones, not the weights).
Parts on their bones (iterations 188-190, `draw.rs`, `LOCKSTEP_ANIM_DRAWS`, home-a29.log). Of
18,809 world matrices drawn on ticks 1000-1014, 13,590 are a bone matrix of their tick to the
bit. A strip's world matrix is the one logged after it (the strip is recorded when its command
starts). A part's bone is its node word when it is 0 or more, else its batch's first matrix slot
(slot s is bone s); a batch without slots is a one-piece mesh drawn at its controller's bone 0.
All 3,296 tied parts are on the bone their matrix is (0 different). 2,780 batches are rigid (one
slot, one world matrix, a bone's); 642 are blended (3 or 4 slots), with no world matrix: how
their matrices are loaded is not in the log, as the 'p' cap (20,000) is spent on views and
worlds. The Sims' parts at tick 1000 include the bodies, heads' parts (eyes on bone 15), shoes,
jewellery and shadow_adult.
The skinned parts (iteration 191, home-a31.log). With every draw of ticks 1000-1014 recorded
(`BUSTIN_TREE_DRAW_TICKS`, the caps raised), all 13,939 bone-drawn strips tie to their models.arc
part on the bone their matrix is. The blended batches (3 or 4 slots) load no GX matrix: the
game skins them on the CPU (fn_801A6ADC, the loop at 801A6C48), blending each position through
up to four bone matrices at the device's +0x4548 (3x4 rows) by the vertex's four weight bytes,
the normal by the heaviest bone's. All 4,712 matrices the 2,926 logged primitives use are bone
matrices to the bit: 4,416 of their tick, 296 of a bone set within 2 ticks.
The reference skinner (iteration 196, tools only: `lockstep/anim/skin.rs`, `LOCKSTEP_ANIM_SKIN`).
The engine will skin on the GPU (plan.md, "Optimized as it is written"); the original's CPU
loop (fn_801A6ADC at 801A6C48) is kept as a reference in the lockstep tool. A position is the
sum over the vertex's nonzero weight bytes of each bone's row r, fma(m[r][2], z, m[r][0] x) +
fma(m[r][3], 1, m[r][1] y), times the weight w / 255 (802C1890), fused into the sum; the normal is
turned by the heaviest bone (the first largest weight, carried from the vertex before when it has
none), normalized by a reciprocal square root and one Newton step (0.5, 3.0), times 127 (802C1898)
and stored as s8 (clamped, truncated). Sent per vertex: three big-endian floats, three s8, then
the colour, texture coordinate and generated ones as present. On home-a32.log all 200,127
vertices of 2,926 primitives come out to the bit, positions and normals. Not compared: colours
and texture coordinates (inputs not logged) and the sphere-mapped coordinates (none here).
What a Sim wears (iteration 197). Sim_SetModels (800591CC) sets a Sim's model part slots (the
Sim object's +1092..+1128, each a part's name hash; a row of the loaded models' pointers sits
before them). It reads the person's description (+0 -> +236 -> +108, fn_8022A45C) and the part
tables of the Sim's age and gender (fn_8018021C by name: "ChildFemale" and three siblings), and
each slot is a table's entry at a description byte: +1092 table +28 at byte 9 (glasses or
jewellery), +1100 +24 at byte 11 (hair or hat), +1104 +0 at 12 (upper body), +1108 +4 at 13 (lower
body), +1112 +8 at 14 (shoes), +1116 +12 at 23 (eyes), +1120 +16 at 24 (nose), +1124 +20 at 25
(mouth), +1128 +32 at 15. Found by a memory snapshot at tick 1,000 (the drawn parts' hashes side by
side in each Sim) and the watch build naming the store. The observer's '?' records (home-a33.log:
2 Sims, club-a1.log: 9) hold each dressing.
Outfits from the data (iteration 198, `outfit.rs`, `LOCKSTEP_ANIM_OUTFITS`). A made Sim's
description is the save's "User" chunk, found through the Sim's neighbour (person data 31 the
neighbour id; the neighbour's GUID is the chunk's after "resU"): two UTF-16 names, then 20 bytes,
byte 0 the gender (1 male), byte 1 the age (1 adult), then the original's description bytes 8
to 25 (the part indices at 9, 11-15 and 23-25; 8, 10 and 16-22 not yet understood). The part
tables are CreateASim's `Sim::Table` rows "AdultMale", "AdultFemale", "ChildMale" and
"ChildFemale". On home-a33.log and club-a1.log every description from the save and all 99 slots
(18 + 81) come out the same; the slots from the engine's own description for every Sim it has.
The lamps from the engine alone (iterations 199-200, `lamps.rs`, `LOCKSTEP_ANIM_LAMPS`). A lamp's
light sits at its node's bone translation (all 400 on home-a28.log), the node state's words 5-7
once ComputeMatrices writes them back; its colour, factor and radius are the node's channels
4-6. The object keeps its animation controller at +848. Its root matrix is rows (sin t, cos t),
(cos t, -sin t), t = 45° x facing in single-precision radians, at the lot's placement
(`lamps::object_matrix`; all 11 of home's roots the same). The light nodes are those with
channels 4, 5 and 6, posed from the object's idle animation (word 2 of its resources record,
`ObjResources::idle`; home's lamps play static two-frame animations). From the engine alone, on
home: all 389 placed lamp lights' places, colours and reaches; the 11 own lights in order; all
1,364 grid cells; 3,916 window fills (nearest list, mix and colour); and 149,472 light grid sums,
bit for bit. Home's identity holds with the run's lamps too (K1, below).
Which animation a lamp poses (K1). The record the object's resources point at (`ObjResources::idle`
is its first entry) is a list of 32-byte states, each with the record's key in its first word and
that state's animation in its third (the wall lamp: off, on; the floor lamp: A_00, A_01; the dance
floor: idle, on, inuse, strobe; the laser show: idle, comfort, energy, fun). The object's graphic
number, its data field 0, which its scripts set, indexes it (`ObjResources::animation`,
`lamps::object_graphic`); a graphic the list lacks takes the idle one. Nothing plays it by an
Animate step: the club's lamp objects run no Animate primitive (checked on club.log, no Animate
step on a non-Sim), they set the graphic by an expression and refresh with Update (7). The save has
the wall and floor lamps at graphic 1 (lit); the run sets the dance floor to 1 at tick 21 and the
laser show to 3 at tick 6.
The scalar channels (K1, words 5 and 6, `anim::ScalarStream`, Anim_ScalarStream 801BD7BC). A vector
stream with one value: the key count (20 bits), the widths of the gap, the bias and the value (5
bits each, 0 is 32), the value's range and minimum as two floats, then records of gap, bias and
value (the logged laser stream: record 13 bits, a gap of 1 and a value of 12, 2 keys, 80 at frame 0
and 44 at frame 1, read 44.72 at place 0.98 as a straight fall). The window, tangents and curve are
the vector streams' (`Keys3`, `channel3`); the engine reads a stream from its start at each place
(a window whose blend flag was set by a jump is not told apart: the 9 logged reads, all
curved). `pose::TrackStreams::local` returns a pose with them (no `Unsupported` any more).
From the engine alone on the club (`LOCKSTEP_WINDOW_LIGHTS=work/logs/club-rooms.log` on club.log;
the study replays club.log natively to the rebuild's and the grid build's ticks for the
graphics, `engine_lamps_at`): own lights in order 14 of 15 places and reaches (was 14) and 15 of 15
colours (was 1 with the idle poses; 11 with the states; the dance floor's three and the laser's
with the scalar streams), the grid build's cells 1,386 of 1,386 (was 1,258; the laser's radius 80 at
place 0 gives its reach 40), light grid sums 57,444 of 57,444 on the points with the lamps placed.
Home is the same as before (own lights 11 of 11, 1,364 cells, 3,916 fills, 149,472 sums).
Left: the laser's place after tick 6. It plays "fun" from its state change, two frames per game
frame (60 a second by 0.033), and the logged places are 2, 3.95, 5.95, 7.9, ... (the light moves
with the beam: its x 24.56 to 24.03, height 3.84 to 4.07 over ticks 6 to 22); a game tick holds one
or two of the render's frames (22 frames for 17 ticks), which the simulation does not keep, so the
engine poses the lamps at place 0 (one light of 15: its place, not its colour or reach, at the last
rebuild at tick 28). `lamps::poses_at` and `bones_at` take a place; `on_lot` does not yet.
Shared code changed in K1: `ObjResources` keeps its states (`states`, no longer `Copy`), `lamps::on_lot`
takes the graphic of a leader, `pose::TrackStreams::local` returns a pose (the scalar channels are read;
Sims' tracks have none, so their poses are unchanged: the replay and lockstep figures are the same).

## The engine's draw list

Stage 5c makes the frame from the engine's own state. Its first step is the draw list: which
model parts the original draws for the lot's objects, walls, fences and its own models, each
with its world matrix, made by `drawlist` (engine library, GPU-free, deterministic) from the
world's objects, the lot's walls and the disc's data, never from the log's draw records. The
renderer will use it next.

A draw is a part of a model record (`draw::PartIndex`: record, node, batch) and a world matrix:
3 x 4 rows, the translation last, the 3 x 3 the rotation times the record's scale (the 16-bit
positions' unit: 1/32768 to 1/256, `Record::scale`). Four kinds: a rigid object part (opcode 56),
an object's display list (66), a wall piece, a lot display list.

The study: `LOCKSTEP_FRAME_DRAWS=TICK lockstep LOG ISO --card CARD` (`lockstep/frame_draws.rs`;
the log needs the draw records of the tick: `home-a39.log` has ticks 1,039 and 1,040) lists the
log's draws of the tick (each 'w' strip found in `models.arc` by its first bytes, its world matrix
the first 0xF1 in the gap after it, as the renderer pairs them) and matches each to the engine's
list by part (a batch of the same positions: many wall records share bytes) and then by matrix
(translation within 1e-4 tiles, the 3 x 3 within 1e-4 of its own scale). Set aside and counted
apart: the Sims (skinned parts, their outfits' rigid parts, their lighting-off shadows: 831 draws,
which include the lighting-off pass the trees, walls and objects get as shadow receivers), the
portrait (57), draws whose model is not in `models.arc` (4), objects a Sim holds (`Carry_`,
`Prop_`, the diamond: 7). The floors are 'g' records and `scene::floor_runs`. Switches for the
rest: LOCKSTEP_FRAME_DETAIL (the objects' resources and the records that differ),
_FRAME_SHOW=KIND (list the missing and off ones), _FRAME_EXTRA, _FRAME_WALLS, _FRAME_RECORDS=PREFIX,
_FRAME_MINE=NAME, _FRAME_HOUSE (the lot's house number, default 1).

Tick 1,039 (home, the engine's lot as `lockstep` loads it; tick 1,040 gives the same figures):
log 599 in scope, engine 785. Before any of it (the first run, the objects' first model by
definition alone) the study had 90 the same, 160 different, 354 missing; now:

| | log | same | DIFFERENT | missing | extra (off screen) |
|---|---|---|---|---|---|
| rigid object part | 272 | 262 | 3 | 7 | 181 (142) |
| object display list | 101 | 95 | 0 | 6 | 32 (5) |
| wall piece | 216 | 197 | 0 | 19 | 5 (0) |
| lot display list | 10 | 10 | 0 | 0 | 0 |
| all | 599 | 564 | 3 | 32 | 218 (147) |

What each kind needed.

Objects. The model is not always the definition's own: `ObjResources::model` names a record in
`models.arc` for some (the trees, the beds); for others it names the skeleton (lamps, the
characters' ids) or is 0, and the record is the first word of the resources' record
(`ObjResources::key`: the TV's `0898 31b6`, the counters', the windows') or, failing that, one of
the models its dataset lists. A dataset (`datasets.arc`'s `RD_` entry) keeps a `Models` section:
the name, a zero, a count, then for each model its id (the CRC of its name), its size, four zero
bytes and the record (`drawlist::dataset_models`). Every distinct model the parts of an
object's group name is drawn once at the object's matrix (the bed's two halves). The world
matrix is the object's root (`lamps::object_matrix`) times the bone of the part's node
(`Part::bone`) from the object's idle animation (`lamps::bones_at`, the same path as the lamps'
lights), scaled by the record; a part without a bone is drawn with the root. A node whose pose
has the flag bit 8 is hidden (a plant's spare leaves, a lamp's lit shade; `NODE_HIDDEN`).

The animation clock. An object plays its idle animation from the start of the run, looping,
`frames = rate x tick / 30 + 1`: the trees' sway (201 frames at 60 a second) is at frame 69 on
tick 1,039, and with that clock the 144 draws of the mulberries and the 18 of the birch that the log
makes in scope (150 of the first run's 160 matrices off) are the same, on ticks 1,039 and 1,040
(`drawlist::idle_place`; the +1 is the first tick's own).

Walls. Each piece of `scene::wall_pieces` draws its family's model (`wall_straight`, `_door`,
`_standard_window`/`_privacy_window` by the window object on the edge: `WINDOW_WALLS`, with
`_left_extends`/`_right_extends` by `WallPiece::form`) at the piece's place turned by its angle:
the 3 x 3 is [cos a, -sin a; sin a, cos a], not the objects' mirrored turn. Of a record's four or
five sub-models all are drawn but the one whose hash is `0906522f` (`UNDRAWN_WALL_SUB`: a
5-vertex piece), when the part faces the eye (`WallPiece::faces`); the 6-vertex piece, with the
part's face and its 5-vertex end, is in that group whether the part is indoors or not (the cap of
"## Wall patterns", drawn apart from the face and outdoors only, is not seen on tick 1,039: no
draw of a part that does not face the eye). The eye is the logged view's, as the camera at the
tick is not the engine's own yet. A fence is one display list at its place turned as an object
(`FENCE_MODELS`: home's style 2, `Picket_Fence_straight`; the other styles' models are not seen).

Lot models. The house's outer lot model (`HouseData` row, `quickdata::House::outer_lot`; home is
house 1, the lot's global 10, which the loaded lot does not keep: the study takes it from
LOCKSTEP_FRAME_HOUSE) is drawn at the origin with its record's scale: 10 of 10.

Culling. The renderer clips, so the list carries every object (the original draws what its own
test keeps). The study counts an extra as off screen when the part's place projects outside a box
fitted to the log's draws at this tick (`VISIBLE`: x 0 to 1.04, y -0.19 to 0.63 of the screen;
the log's own draws span x 0.006 to 1.034, y -0.179 to 0.620; none of them is outside it). The
original's test is a bounding volume of the record (its 72 bytes of `bounds`, not decoded); a
sphere about the part's mesh radius, the first try, culled 5 to 7 of the log's own draws (the
trees' top leaves), so it was dropped.

Left, with counts (tick 1,039).

- 3 DIFFERENT: the grandfather clock's pendulum, the smoke detector and the burglar alarm. Their
  idle animation is not what they play; a script's animation (a swing, a blink) is. Needs the
  objects' running animation, which the engine does not keep (only the Sims').
- 7 missing rigid: the aquarium's fish (22 vertices each), hidden in its idle pose and swimming
  under their own animation: the same cause.
- 6 missing object lists: the counters' variants with a sink's hole
  (`Tiled_Counter_Sink_Hole(Straight)`, `(Epikouros)`) at the two tiles beside the sink (22.5,
  23.5 and 23.5, 23.5): the engine draws the plain counter there (and 6 extras of it). Which
  variant a counter takes from the sink on it is not known.
- 0 missing wall draws, 3 extra (L2: the diagonals' and the gap's forms, "A part's form" in "Walls"):
  the 3 are the wall_straight piece at (13.5, 28.5), turned 180, at the screen's top edge (y -0.001),
  which the log's screen test culls (the same cause as the objects there). The wall draws that are
  the same went 197 to 216.
- 218 extra, 147 of them off screen by the fitted box. The other 71 in the box are objects the
  original does not draw in the main pass: parts at the screen's top edge (the alarm clock, the
  back wall's windows, lamps, the sconces: bases within 0.05 of the edge), the 3 birch's crowns
  and the scooter, parked at (23, 3.5), which the original does not draw (the same one the
  first survey found undrawn).

Shared code changed: `lamps::idle_bones` split into `bones_at`/`poses_at` (the lamps' lights the
same; `LOCKSTEP_ANIM_LAMPS` unchanged), `draw::PartIndex` keeps each record's archive id and
gives `record_by_id`, `parts_of`, and `Part::index`.

## The renderer

`engine/render` (package `housefire-render`, its own target directory and lock file: the
simulation crate has no GPU dependency) is the engine's own Vulkan renderer, raw `ash`, headless.
Its API is a `Frame` (`frame.rs`): a camera (view, projection) and a list of draws, each a mesh,
a world matrix, a material (combine mode, two layers' textures and samplers, blend, depth, cull,
alpha test, sphere map) and a light set (ambient, directionals, points, or vertex colours, or
black). Meshes are uploaded once (`Renderer::load_meshes`: compact vertices, 16-bit positions and
coordinates and 8-bit normals scaled in the shader, one vertex and one index buffer), textures
once (`load_texture`: RGBA8 with the mip levels the format holds whole), and a frame is one
command buffer: the per-draw numbers are one storage buffer indexed by the instance, the lights
are summed per pixel in the fragment shader, and no vertex is touched on the CPU. Offscreen:
colour and an id buffer (the last draw to cover each pixel), read back.

`render-check LOG ISO FRAME` (`HF_FRAMES`: the folder of the recomp's frame dumps) builds the
frame from the log's records (`scene.rs`), renders it at 640 x 448 and compares it with
`world_NNNNN.png` (the picture before the 2D layer), writing `r1/ours_N.png` and `r1/diff_N.png`.
`home-a36.log` has the draw records of frames 2400 and 2401 (ticks 1039 and 1040), with the
GX state of each draw (';').

How the records make a frame. The log is a stream; a draw's state is what was set before it and
what it set up itself. Strip k (a 'w' record, an opcode 56 strip or an opcode 66 display list)
takes the material ('m') and light set ('[') logged before it (the last of its tick in "gap k",
the records with k strips before them), the texture loads ('l') up to it (the last of each map
carries over, as the device's state does), and its world matrix, view and projection from the
first such record of its tick in gap k + 1 (the draw's own `ERC_ApplyState`; none there: the
state carried from before). The part is found by the arrays' first bytes (`draw::PartIndex::find`;
a display list also by its list's bytes). Draws through another projection than the lot's
camera are the interface's (class "overlay"); a lit-off light set (flag 0x4000) is a projected
shadow; a part blended over several bones is a Sim's; the 'g' records are the floors' strips.

What is drawn, as the device's combiner stages made it (`erc_material.cpp`):

- one layer, textured: the texture times the light's colour, doubled and clamped (or the texture
  alone, doubled, for an unlit layer); untextured: the light's colour;
- two layers: a cross-fade by the second layer's factor, or layer 1 over layer 0 by layer 0's
  alpha or its own, or added by layer 0's alpha (blended over the picture or added to it); the
  second layer's blend byte pairs 2 and 2 (added through the game's table, a global) are not
  drawn and none occurs in the frames;
- a second layer whose uv source is 2 with lighting on is a sphere map: the view-space position
  reflected about the normal (`vertex_programs.cpp`'s `bustin_sphere`), the second texture read
  there and added to the colour by weight 1, the alpha so far, or the factor;
- lights (`erc_lights.cpp`): the ambient and each light's colour times its tint, as bytes;
  directionals turned by the device's matrix (to the light is the negated direction); points
  moved into view space by the view the device had, their colour divided by k0 + k1 d (k0 1, k1
  the range times 0.1); the diffuse term clamped at 0; the sum clamped to 1; texture colour times
  that, doubled. A set with no lights and flag 0x8 clear is the vertex colours; flag 0x4000 black
  at alpha 127/255;
- textures are the 'x' record by image address, found in `textures.arc` or `datasets.arc` by the
  hash of the first level. A palette texture has two IA8 palettes: the device samples the image
  through each (texture maps m and m + 1) and its stages (fn_8019DC7C: a yellow and a blue
  constant, texture swaps) take red from the first palette's intensity, green from its alpha, blue
  from the second's intensity and the alpha from its alpha (read from the stage setup, and
  checked by the bench's pixels). The filters are the GXTexObj's mode word as the hardware codes
  it (bit 2 of the minifying code linear, its low bits the mip levels: 0 none, 1 nearest, 2
  linear), the wraps its low four bits;
- the layer flag word (bytes 4 to 8 of a layer in the 'm' record) has 0x40 for the translucent
  materials (windows, the aquarium, the bottle lamp, water): drawn blended by source alpha with
  depth writes on and no alpha test. Checked: with it the aquarium's pixels off go from 255 to 12
  and the bottle lamp's and the diamond's to 0.

Figures (`render-check`, RGB, within 4 in every channel; the colour of a pixel our rigid draws
cover last, against the reference; frame 2400, 2401 the same to a pixel or two):

| class (pixels it covers last) | pixels | within 4 | PSNR |
|---|---|---|---|
| rigid draws (strips and display lists) | 79,447 | 97.62% (1,890 beyond) | 30.87 dB |
| floors' own pixels | 205,803 | 82.57% | 25.87 dB |
| Sims' own pixels (skinned parts, the portrait's draws) | 1,470 | 88.50% (169 beyond) | 21.62 dB |
| the portrait's square (the logged viewport) | 3,080 | 93.99% | 22.28 dB |
| shadows' own pixels (the last Z-off draw), the Sims' included | 32,189 | 67.73% | 27.31 dB |
| the whole frame | 286,720 | 86.77% | 26.74 dB |

(Before the floors and shadows: rigid 92.93%, the whole frame 28.21%. Before the Sims (R2b): rigid
93.96% with the portrait's square the road's, floors 82.54%, shadows 67.61%, whole 85.76%.)

Draws by class, frame 2400 (2401 the same): rigid model strips 333, display-list model draws 330,
skinned 85, projected shadows 692 (658 and the Sims' 34: `shadow_adult` is a skinned part drawn with lighting off), the floors' strips 98 and the mask passes
98 (the 'g' records of opcode 42), overlay 57 (the portrait's: R2b), other commands 2 (two 'g' records with
no vertices, not a draw), unidentified 1. 56 draws have a texture not found among the game's
(made at run time; 47 pixels, the Persian rug).

What the remaining rigid pixels are (frame 2400, now 1,900; before R2b 4,880, of which 3,116 were the
Sim's portrait square, 49..104 x 347..402, painted over the road by a later draw): 1,091 the birch tree's leaves (below);
466 the windows' glass (from 1,302 before the floors: its colour now blends over the right
floor); 80 the wall caps, and the rest scattered (the Sims' clothes, 26 and 14 pixels, are
skinned parts).

The GX state of each draw is in the log since home-a36 (';', after each ERC_ApplyState: the
applied copy at 0x80357DB8, so what GX holds, since the context's own Z and alpha fields are
applied only with the blend group): Z mode, alpha compare, Z compare location, blend mode and
flags, cull. A strip takes the first ';' of its tick in gap k + 1 (`with_state` in scene.rs);
every drawn strip of frames 2400/2401 has one. What frames 2400-2401 show (3,504 records):
cull is always none and Z always less-or-equal (R1's stand-ins were right); opaque materials
test alpha >= 127 (GEQUAL 127, not R1's 0.5 = 128), opaque display lists mostly test nothing,
some draws test >= 102 or >= 12, the blended (flag 0x40) ones >= 5 or >= 0 with source alpha /
inverse; Z-off passes with destination-alpha / inverse (1,316 records, flags 0x148) and a
multiply by the picture (source factor 2, 196) are not among the rigid draws drawn so far (R2's
shadows and floors). With the state, the rigid figure is 92.93% within 4 (80,876 pixels, 5,716
beyond; frame 2400), from R1's 91.82%. The record also carries the SDK's copies of the pixel
engine's registers (__GXData +0x1D0 cmode0, +0x1D4 cmode1, +0x1D8 zmode, +0x1DC peCtrl;
offsets from GXSetColorUpdate, GXSetDstAlpha and GXSetPixelFmt's stores). In the frames: the
EFB is RGBA6_Z24 (peCtrl 1; bit 6, Z before texturing, on most opaque draws), so it keeps a
destination alpha. Opaque draws write colour and alpha (cmode0 0x1C) with a constant
destination alpha (cmode1 0x100: alpha 0; 0x17F: 0x7F, 208 records); blended ones 0x4BD. The
shadows are two passes with Z off: a mask (cmode0 0x215: alpha writes only, no colour, source
factor the picture's colour, destination zero; 98 a frame) and the shadow draws (0x6FD: colour
and alpha, source x destination alpha + picture x (1 - destination alpha); the 658 "lighting
off" draws a frame). The registers are as ERC_ApplyState left them; a draw method that sets GX
itself after it (a constant alpha for the mask?) is not seen. Fog is not shadowed in __GXData
and not recorded.

### The floors and the shadows (R2a)

Floors. Each 'g' record of opcode 42 is a draw of its logged vertices (`Log::strip_vertices`,
positions in the lot's frame: kept in 64ths of a tile in a 16-bit mesh and scaled back by the
draw's matrix, texture coordinates in 512ths: a run's u reaches -19), lit by their vertex
colours (0xRRGGBBAA, as the floor draws their lighting: 'Lighting::VertexColours'), with the
camera's view and projection and a world matrix of identity. Its texture loads are those counted
before it in the record (`others_before`; the strips' loads still go by the gap), its GX state is
the first ';' after it: the state with the same strips before and `others_before` = its index
+ 1. The first gap's 74 'm' records are the floors' materials, one each in order (73 floors and
the first strip's): a gap with as many materials as floors takes them so, and the terrain's are
two layers (a cross-fade by the second one's factor, the ordinary two-layer path): the street
and the road, the sidewalk and the wood draw right (the sidewalk's and the road's probed pixels match to within 2); the grass is `terrain_mom_house_detail` (64 x 64, repeated a tile) cross-faded with
`terrain_mom_house_color`, a 256 x 256 aerial picture of the neighbourhood read at the position
through the texture matrices (the material's uv source 3): GX_TEXMTX0 then GX_PTTEXMTX0, both in
every ';' record (F1: `StateRec::tex_mtx`; home's terrain draws hold TEXMTX0 = identity and
PTTEXMTX0 = [[0.0083, 0, 0, 0.333], [0, 0.0093, 0, -0.676]], so u = 0.0083 x + 0.333 and
v = 0.0093 y - 0.676 with x, y in tiles; 1/256-scale TEXMTX0s belong to the shadow batch's
draws). Log mode builds the uv from them (`scene::logged_position_uv`): floors 85.51 -> 97.33%
within 4, whole 88.89 -> 97.47% (frame 2400). The four floats are the terrain shader's own
(its tail's last four words: u0, v0, su, sv; `Shader::position_uv`), which engine mode uses
(`scene::shader_position_uv`): floors 84.58 -> 96.22%, whole 87.19 -> 95.60%. The earlier fit
(`TERRAIN_COLOUR_*`, u = x / 153 + 0.34) is removed. The floors were 81.73% within 4 before F1; what is left is
the trilinear filtering of the detail (the console's level choice is not the GPU's); and the porch.

What the passes are. The frame is a sequence of batches, each: floors (opaque, constant
destination alpha 0x7F: cmode1 0x17F), their shadow draws (class "lighting off": blend
destination alpha / inverse, Z off), then the mask (the same floor strips again, cmode0 0x215:
alpha writes only, blend source factor the destination colour and destination zero, cmode1
0x100), then the next batch's floors. Four batches on frame 2400: the lot's ground (floors 0..72,
570 shadow draws, the mask from the 73rd floor record), the rooms of the house (shadow draws of the
Sims and objects: `shadow_adult`) and two small ones. Objects (all strips of the models) come after the last batch, so a shadow only
ever lies on a floor (the receiver's alpha 0x7F is the stencil: a shadow darkens by (1 - alpha):
half) and never on a model drawn later. The mask pass draws no colour (our pipelines mask the
channels); its constant alpha 0 (as recorded) cleans the floor's alpha for the next batch. Its
effect on these frames' pictures is none (the same figures with or without), so it is drawn as
recorded and costs nothing visible. Colour and alpha write masks (cmode0 bits 3, 4), the
constant destination alpha (cmode1 bit 8, low byte) and the id buffers' second channel (for
draws with the depth test off) are in `Material` and the pipeline key (`renderer.rs`); the
constant goes out as the fragment's alpha (the shader), taken as it is by an alpha blend of
(1, 0), except where the colour blend itself reads the fragment's alpha (windows, water): the
alpha written is then the fragment's own.

The '}' records (home-a37; the GX calls a draw method makes itself, `Log::gx_calls`) say the same
as the state records and nothing more: before each batch SetColorUpdate(1), SetAlphaUpdate(1),
SetDstAlpha(on, 0x7F) for the floors; (1, 1, on 0) for the shadow draws; (0, 1, on 0) for the
mask; then back. The constant is the stencil: a floor leaves 0x7F, the first shadow over it
darkens it by half and leaves 0, so overlapping shadows do not stack. gcn-recomp's renderers did
not implement the constant (BP 0x42); they do now (the constant is the fragment's alpha, stored
unblended; where the colour blend reads the fragment's alpha, the blended objects drawn after the
last batch, the EFB keeps its alpha instead: exact would take dual-source blending, which their
coverage attachment rules out). Against the reference frames made so (work/frames, from
home-a40.log, the same log as home-a39): shadows 67.73 -> 86.59%, floors 82.57 -> 85.51%, whole
frame 86.77 -> 88.89% (frame 2400), with no stacking fit. Left fitted (`shadow_of`): the shadows' polygon texture (their
alpha) is not applied, flat black at the light set's 127/255 (86.59%, the same with the texture applied).

The terrain's second layer: the '}' records give the texgen (SetTexCoordGen2: coordinate 1,
2x4 matrix from the position, matrix 0x1E, one a floor strip) and the one load of
GX_TEXMTX0 for them (an identity: ERC_ApplyState loads the model matrix there, which is the
identity for the floors), so u, v before the second transform are the position in tiles. The
second transform is the post matrix of dual-texture (0x40, from the device's projection): it is
loaded by ERC_ApplyState's native path, not through the hooked GXLoadTexMtxImm, so it is not in
the '}' records; the ';' records' applied copy (+0x674, +0x6A4) holds both (F1).

Shadows are 67.61% within 4, floors 82.54%, not the 90%: of the shadows' 10,394 pixels off the
wall caps' hatching is the largest (the reference's wall shadows are one flat half, ours show
the overlaps of the triangles as two tones); the floors' 35,938 off pixels are the terrain
(about 24,000 in 12 strips: the detail and the aerial texture's alignment, the GPU's trilinear
against the console's) and the porch (the house's shadow there is near black in the reference
and half in ours; not counted separately, unverified).

Open: the birch tree's leaves (1,091 pixels of the rigid draws) are darker in the reference at
pixels where its own shadow draws lie (down to 0, ours up to 120), but every shadow draw of the
log comes before the tree's strips (strips 6 to 575 against 1425 and later), and draws after the
mask only are the interface's. Moving the shadow and mask passes after the models (to darken
the leaves) made the rigid figure worse (93.44%; checked) and is not in. Two 'g' records at the end of
the lists (G196 and 197, no opcode 42: `[4, 0x81070B60, 0x28000000]`) are not decoded: not
vertex strips (unverified: display-list calls).

`render-check` prints the floors' and the shadows' own pixels (count, within 4, PSNR; the pixels
whose last depth-tested draw is a floor, or whose last Z-off draw is a shadow), and writes
`r1/passes_N.png` (red: where shadow draws lie, blue: the mask). `HF_FLOORS=n` lists the n
floor or shadow draws with most pixels off.

### The Sims and the portrait (R2b)

GPU skinning. A blended part (3 or 4 matrix slots) is drawn from the vertices of its model, never
touched on the CPU: `skin.comp` (a compute pass, one dispatch for each skinned draw, before the
render pass) blends each vertex's s16 position through the draw's four bones by its four weight
bytes and turns its normal by the heaviest bone's, into a buffer of vertices (position and normal,
32 bytes) that the vertex shader (`model.vert` built with `-DSKINNED`) reads by the vertex index
plus the draw's offset; the colour and texture coordinates stay in the mesh. The meshes are
uploaded once (`Renderer::load_meshes`: weights go in a second buffer, two words a vertex), the
bones (a '$' record's four 3x4 matrices, as logged, to the bit: they carry the 1/4096 scale and the
Sim's place, so the draw's world matrix is the identity) and the output are per frame. The
reference skinner, `housefire::skin` (`engine/src/skin.rs`, moved out of the lockstep study
unchanged: `LOCKSTEP_ANIM_SKIN` gives the same 308 primitives, 21,066 vertices, 0 positions and 0
normals different before and after), computes in the original's own fused arithmetic; the GPU does
it in single floats. `render-check` prints the comparison for every logged '+' primitive it drew:
frame 2400, 152 of the tick's 154, 10,442 vertices, max position error 3.8e-6 lot units, normals
within the quantisation of the bytes (0.008 of a unit); none over 1e-3. The 2 left are strips whose
part is not found (below).

Not the models' arrays. The models' positions are not always the ones the original had in
memory when it drew: for the Sims' faces (eyes, noses, mouths) a few vertices differ (12 primitives
of frame 2400, errors to 6e-3 lot units against the log; shaped at run time, unverified), so a
skinned mesh takes its positions, normals and weights from the draw's '+' record (the first 4,000
in the draw window, which this frame is in) and the texture coordinates from the model
(`skinned_mesh` in scene.rs; a mesh for each distinct content). A '+' record's 'heaviest' is the
bone carried into its first vertex. Three skinned strips (strips 705, 719, 762: 80, 17, 11
vertices) have no model found: their first position bytes are not a model's (the morphed
vertices, probably), so they are not drawn, and two of them have a '+' record that is not compared.

The portrait. The 57 overlay draws are the Sim's portrait in the corner (the head, hair, glasses
and jacket: skinned and rigid parts, drawn through their own camera, `use_view`, and a projection of
(8.144, 0, 8.144, 0, -0.001, -0.5005): 1:1), after the lot's draws, over the road. Its viewport is
the state record's (`StateRec::viewport`, the context's +0x6D4: left, top, width, height, near,
far): (49.92, 346.30, 54.08, 55.55, 0, 1) on the portrait's draws, the lot's (0, 0, 640, 448)
elsewhere; a draw takes its own state's, as a float viewport with the scissor its whole pixels
(`Viewport::pixels`). The fitted 49, 347, 56 x 56 square it replaces was a pixel and a half off,
and that was most of what looked wrong: with the logged viewport the glasses, the hair's edge and
the face parts line up (Sims' pixels 38.5% with the square, 88.5% with the viewport). The backdrop
is still fitted: `PORTRAIT_BACKDROP` 50, 347, 54 x 56, colour (218, 238, 254), a fill before the
portrait's draws. The 2D layer draws it (the 'g' records G196 and G197 at the same place are
display-list calls whose bytes the log does not hold; their states have viewports shifted by half a
pixel, 50.42, 346.80). To take it from the log: the bytes of the two display lists (at the
addresses in the records' words 1 and 5) and their texture. The renderer takes a per-draw
projection and viewport (`Draw::projection`, `Draw::viewport`; a draw with a viewport clears the
depth there first, and the frame's backdrop is filled once) and the projection is in the per-draw
record, not a push constant.

The Sims' textures (R2b2). A Sim's skin, face, hair and clothes are one 128 x 128 C8 image made at
run time, in no archive; `Log::texels` ('`') holds its first level (hash, bytes) and `Log::tlut_loads`
('{', GXLoadTlut) the palette in each hardware slot. A texture object's TLUT name (the word at
+0x18 of the 'l' record's object) is the slot, and the palette is the latest load of that slot
before the draw (`tlut_loads`' tick and strips_before: slot 11 at strip 700, 12 at 779, 15 at 1440
in frame 2400); the same image is loaded through different slots by different draws (names 12 and
15 for the male Sim's) with equal entries. The palette is RGB5A3 (the GXTlutObj's format word,
bits 10-11 = 2: `TlutLoadRec::format` checked: the entries start 0x8000, 0x8c42, ...: opaque
colours), one palette, not the archive's textures' two IA8 palettes (no second map is bound and
no 'l' of the Sims' draws names a second palette): the plain decode, `runtime_levels` in scene.rs
(decoded once for each hash and palette). With it the skinned and overlay draws have all their
textures (0 draws "not found", from 194).

What is left. The Sims' own pixels (1,470: 1,410 the portrait, 60 in the house) are 88.50% (frame
2401: 89.05%). The 169 beyond are not broken down by part yet (the before-viewport breakdown had the hair and the face parts as the
largest; unverified now); the portrait square is 93.99%. Three skinned strips have no part
found (above), and the backdrop is fitted. The Sims' shadows are in the shadows' class (67.61 to
67.73%).

## The frame from the engine's state

The last step of stage 5: `render-check` with `HF_ENGINE=1` (and `--card CARD` for the home lot) makes
the lot's model draws, the rigid object parts, the objects' display lists, the walls and the lot's
own model, from the engine's world instead of the log's records. (W1, at the end of the section,
added the floors, the shadows and the camera; S1, the last subsection, the Sims and the portrait.)
The log serves for three more things the engine cannot run
to: which lot of the card the log ran on (`lotpick::pick_lot`, as lockstep), the animation clock
(`tick`) and the game clock (the passes the player's dialog stopped, `PassRec::stopped_at`).

Where each part comes from (library: `drawlist`, `lighting`, `shader::index`; renderer:
`engine_scene.rs`):

- Draws and matrices: `drawlist::lot_draws_of` (objects, walls, fences, diagonal walls, the lot's
  model). The placeholder `tom_debug` that an object with no model and no dataset has as its
  resources' first word is not drawn (it was 5 draws and 10,876 pixels of green).
- Materials: the sub-model's shader (`shaders.arc` by the sub-model's hash, `shader::index`: by the
  entry's id and by the CRC of its name), its layers as the device's material record
  (`material_of`, the factor byte 0xFF kept as 0x80), their textures by the layer's id in
  `textures.arc` or the datasets' LFXTs. A wall face (the 18 or 23 vertex list) takes its pattern's
  shader (`quickdata::wall_patterns` by `WallPiece::pattern`), not the model's `_garbage`. Against
  the log's materials at the same world matrix and vertex count (`HF_MATDIFF=1`): of about 550 matched
  draws the layer count, combine mode, blend and textures are the same but for 18 two-layer draws
  of the lattice door (the log tests alpha, the engine does not), 2 of the bed, the rug's depth
  write and 4 of the outer lot's destination alpha. The samplers are the one assumed for all
  (repeat both ways, linear, the mip levels the texture has) and equal the logged GXTexObj's on every
  draw but the easel's (clamp); the LFXT header's flags word is not decoded.
- GX state, as the records show it: Z less-or-equal with writes, no culling; an opaque one-layer
  material whose first texture has clear pixels tests alpha at least the shader's first layer's
  float times 255 (0.5 gives 127, 0.4 the rose bush's 102), a blended one (flag 0x40) at least
  that (0.02 gives 5); the destination alpha constant is 0 for objects and walls, 0x7F for the
  lot's own model (`engine_scene::state_of`; the rug, the only object with 0x7F and no depth write,
  is run-time textured and not told apart).
- Lights (`lighting::LotLights`, built from `light`'s ported pieces; the study `HF_LIGHTSTUDY=1`
  prints both sets in world space):
  - the clock is the lot's saved one (7:04:00 at home) plus two seconds for each tick before the
    frame whose pass ran to its end (`World::clock_pass`): 7:36:32 at tick 1,039, where the sun's
    direction matches the logged one (to 3 places; the original is at 7:36:40, four seconds later:
    the first tick's own is not counted);
  - outdoors (room 0): the ambient colour times the time of day's factor, the sun (colour times
    factor, direction `light::sun_direction`, negated and turned by the view), tinted by 0.5 (both
    the light and ambient tint: the logged `tints`); the logged ambient and sun colours match to
    the byte;
  - indoors: the ambient colour times `light::room_ambient` (day 0.3 and night 0.2, times the
    room's window brightness: the door and window objects in the room over its tiles), no sun;
    0.122/0.153 in the main rooms and 0.125/0.157 in the bedroom, the logged ones;
  - lights: each window's two lights (`light::window_light_places`, `window_light_params`, the
    sun and nearest lamps' mix `window_mix`) and the lamps' (`lamps::on_lot`), sorted into the rooms'
    ranges as Light_Rebuild does (`light::sort_lights`); an object gets the four of
    `light::cell_lights` at the light grid's point nearest it (rounded to whole tiles), in the room of the tile before its place when
    it is on an edge (a door at y = 24.0 is in tile 23): the window lights' colours (0.302, 0.302,
    0.275 ...) match the log's to the byte; 72 of the 98 object places have the four lights the
    log's set has (66 with the exact place, 71 with the grid point above); a lamp's light is its
    colour times the tint, its falloff 1 + 0.1 x reach x distance.
  - a wall piece or diagonal is lit from the grid points at the two ends of its edge
    (`LotLights::along`, `drawlist::edge_of`), two lights from each end, so a light both ends
    share is in the set twice, as the log's sets are ([a, b, a, c]). The two are not the cell's
    first two (that gave 44 of 84 wall, diagonal and fence sets):
    they are the cell's first slot and the first of slots 1 and 2 that is of the other kind (a
    lamp, an own light, index 256 and up, against a window's light), else slot 1
    (`LotLights::end_pair`). So a cell [window, window, lamp, lamp] gives (0, 2), [window, lamp,
    ...] (0, 1), and [window, window, window, lamp] (0, 1): the lamp in the last slot is left out
    (home, frame 2400; the club's walls are not checked). The rule is
    fitted to the log; no single grid point gives such a pair, and why the original stops at slot
    2 is not read (the walls' light set is not made by the object path, 0x8017BF80, which copies a
    cell's lights for a place and room; its callers are 0x80053734 and 0x801648B8).
  - a fence stands on its edge and takes all four lights of the grid point at the edge's first end
    (`LotLights::fence`), like an object. (A fence's place is the
    middle of its edge, a wall piece's the tile's centre.) All 84 wall, diagonal and fence sets
    of the home lot are the log's (`HF_LIGHTSTUDY=1` with `HF_ORACLE=1` prints `LSUM` per kind,
    and `LD` for each set that differs). The objects' sets: 69 of 79 places (`LSUM object`),
    unchanged. What differs: the doors and windows on an edge (the room is not found by one rule:
    the log's room is the one across the edge for windows at y = 14.0 and 15.0, the one before it for
    the windows at x = 13.0, the back wall at y = 24.0 and the door at y = 15.0, not by `room_at`),
    the trash can, the rose bushes and the cacti outdoors (a cell one tile off, or a place that is
    not the object's own position), and the outer lot's model (no light set logged).
- Diagonal walls (`drawlist::diagonal_draws`; they were the largest of C1's leftovers): the face
  of the tile's `wall_diagonal` model that turns to the eye. The model is turned 45 degrees in its
  own frame, so the face's normal is taken from the 5 seen tiles (a "/" faces (-1, -1) at turn 0 and
  (1, 1) at 180, a "\" (1, -1) at 90 and (-1, 1) at -90); its pattern is the tile's byte on the sides
  opposite (the exterior siding of the corner). The forms that reach past an end
  (`_left_extends` ...) and the cap forms at a wall's free end are drawn by the form rule
  (`scene::diagonal_form`; "A part's form"). The sink counters, the aquarium's fish and the pendulum are
  as before.

Figures, frame 2400 (2401 the same to 10 pixels), the pixels the engine's model draws cover last,
within 4 in every channel:

| | log's records | engine's state | engine's state with the log's light sets |
|---|---|---|---|
| rigid draws' pixels | 79,447: 97.63% | 81,608: 84.73% (12,464 beyond) | 86.62% (10,921 beyond) |
| whole frame | 88.89% | 85.28% | 85.81% |

L1 (the wall and fence light sets equal the log's): engine mode, rigid draws 89.17% to 91.32% on
2400 (91.31% on 2401), whole frame 85.92% to 86.51% (86.50%); log mode unchanged.

L2 (the walls' forms): engine mode, rigid draws 91.32% to 92.32% on 2400 (92.31% on 2401), whole
frame 86.51% to 86.71% (86.70%); log mode unchanged (97.63%, 88.89%). The windows' remaining error
is their light sets ("What is left").

The engine mode is **below** the aim of 91.82% for the rigid pixels (C3 took it from 84.73% to
86.64% on 2400, 86.62% on 2401). How it got there (frame 2400): the draws with the log's light sets
as an oracle and a destination alpha of 0 for all 84.1% (from 40.2% with the model's own shader on
the walls and the placeholder `tom_debug` drawn); with the engine's light sets 69.7%, 80.65% with
the walls lit from their edge's ends, 83.06% with the diagonal walls (which hid the rooms behind
them), 83.56% with the doors' edge rule, 83.86% with the alpha reference from the shader's float,
84.73% with the nearest grid point; C3: 86.12% without the objects whose resources name no
dataset (the scooter), 86.31% with the engine's draws after the floors, 86.64% with the windows'
room across an edge. The light sets are worth about 2.5% now (89.18% with the log's).

What C3 found (frame 2400, pixels of the rigid draws beyond 4):

- **The scooter (1,319 pixels, fixed).** Its `ObjDefinition::ResData` row (flags 0x82000000) has
  no dataset (word +0x10 is 0): the model is in `models.arc` alone. The original draws the objects
  that have a dataset; `drawlist::object_models` now draws nothing for an object whose resources
  name none (it was "no model and no dataset" only). Lockstep's frame draws: 570 same, 3 DIFFERENT,
  26 missing (564, 3, 32 before).
- **Draw order against the floors (151 pixels, and the glass).** The log draws every floor
  strip and shadow mask (indices up to 398 of the frame) and then the objects. C2 put its block
  where the log's first model draw was (the outer lot model, index 74), so a window's glass (it
  writes depth) drawn before the later floors hid the floors behind it: dark panes. `splice` now
  puts the block at the first model draw after the last floor strip or mask. (The original's own
  order of the objects and walls is spatial, not opaque-then-blended: at the back wall it goes
  tile by tile, a wall piece, then the objects on its tile, the window's two frame meshes then its
  glass. `HF_ORDER=1` lists the log's order; no linear key over the place fits it, 121 of 201
  steps at best, so the engine keeps the opaque-then-blended order.)
- **Windows' light (about 330 pixels).** A window in a wall at x = 13.0 or y = 14.0 is on the
  edge of an outdoor tile and a room: `LotLights::room_at` takes the tile before the edge, or the
  room across it when the tile before is outdoors (the engine lit them by the sun). The doors
  lose 63 pixels by it (the walnut door 946 to 1,009: the log's choice for a door is not this).

What is left, in pixels of the 10,725 beyond (frame 2400, 2401 within 15):

- The windows' objects (`_NEW_Single_Hung_Window`, 4,124 pixels): L3 found the rule. The log lights
  each of a window's three meshes (10, 52 and 53 vertices) with the set of the room that the
  mesh's middle (the mean of its vertices at the draw's matrix, `drawlist::mesh_centre`, `Draw::centre`)
  is in: the frame's 52 vertices are 0.03 on one side of the wall plane, the 53-vertex glass 0.03 on
  the other and the 10-vertex quad on the plane (0.002 off). The window turned the other way (at
  (13, 17.5) and (23.5, 24)) has them the other way round, so no rule of the window or wall is
  needed. The set of the room across is the inside's (ambient 0.122, no sun) or the outside's
  (0.161, the sun, the outdoor room's four lights at the grid point of the window's place); both
  were made already, only the room was chosen by the place (`LotLights::at_in`: the room from the
  mesh's middle, the grid point from the object's place). All 30 window meshes of home at 2400 have
  the log's set (the engine's room by tile, `room_at`, at the middle). In engine mode: rigid 92.32% to
  94.09% (2401: 92.31% to 94.07%), whole 86.71% to 87.19% (86.70% to 87.18%).
  `HF_LIGHTSTUDY=1` with `HF_ORACLE=1` prints `LW` for each window mesh (middle, log's ambient and
  sun), and `LD` for each mesh whose four lights differ from the log's, with the grid points
  within 3 tiles that would give the log's set. `LSUM object` counts meshes now, not places
  (before L3: 308 same, 35 differ; after: 328 same, 15 differ; walls 84 of 84 unchanged).
  What is left of the objects (15 meshes): the trash can (2 meshes), the deckchair at (23.5, 17.5)
  (1), the lattice door (1; the walnut doors are right now), the cacti at (24.5, 24.5) and
  (24.5, 13.5) (3 meshes each), the rose bushes at (19.5, 6.5) and (19.5, 10.5) (2 each) and the
  outer lot's model (no light set logged). They are ties of the grid point: the log's x is 20 at
  (19.5, 6.5) and 19 at (19.5, 10.5) for the same place (the meshes' middles are 19.525, 19.505 and
  19.475, 19.495, but the trash can's 19.514 and 19.494 both take 19). Tried: the grid point from
  the middle (24 differ), from the floor of the place (57), rounding ties to even (30). The
  original's lookup (0x80169ABC, read in the recomp's generated C) is
  `trunc(x + 0.5)`, `trunc(y + 0.5)` of a position vector its caller (0x80053734 with the record's
  +896) gives, clamped to 0..38, then the cell's four room bytes searched for the object's room (an
  unlisted room gives a default entry): the position is not the place or a mesh middle, and the
  cell's room list (`light::CellRooms`) is not used by the engine's `cell`, which takes any room.
- 2,147 + 851 + 1,228 + 459 of the walls, the windowed walls, the diagonals and the fence
  (3 to 5 levels off): the light sets (L1: the walls' and fences' now equal the log's).
- 678 + 78 of the money trunk (object 248) and the cheat gnome (105), drawn by the engine, not
  by the original. Checked and ruled out: the log's object list (both in it), flags (2 and 2,048,
  others with 2 are drawn), the objects' data fields (only field 33 = 8, with 36 = 20 and 37 = 64,
  tells them from every drawn object: the mark of the scooter's parts too: a catalogue class, not
  a hide flag), selectors ('s' records exist for both, with their dataset ids), ResData rows (same
  flags and form as drawn objects), the object's address, level, attributes and temps. Their
  bounds sphere and box (the record's 72 bytes: centre and radius as 4 floats, then two boxes of
  min and max, the second the union of the sub-models' vertex boxes in the record's raw units)
  are inside the view by any radius: the trunk's place is at x 0.058, the gnome's at x -0.004 of
  the screen, both at the left edge, where no logged object has its place (the leftmost is the
  mulberry's at 0.062, which is a tree with a radius of 0.12). A test of the object's place
  against a box (x from about 0.06 on) would hide both, but the scooter's bounds are too large
  (4 by 7 tiles for an object of about one: its parts are on bones) to say anything. The rule is not found.
- 1,009 of the walnut door (its light set at the room boundary), 723 of the outer lot's (286 with
  the log's lights), 571 of the lattice door (two layers, the log tests alpha), 269 of the book
  case, 207 of the aquarium, the rest in small parts.

Shared code changed in C3: `drawlist::object_models` (an object with no dataset draws nothing),
`lighting::LotLights::room_at` (an edge's room), `engine_scene::splice` (the block's place);
render-check gained `HF_ORDER`, `HF_SKIP` and `HF_SKIPTEX`. Log mode unchanged (97.63/97.62%,
whole 88.89%). Before that, in C2: `drawlist::Draw` gained `origin`, `object`, `shader` and `edge`; `wall_draws`
takes the wall patterns; `lot_draws_of` makes the lot's whole list (lockstep's `frame_draws` uses
it: its figures are unchanged, 564 the same); `is_sims` and `is_held` moved to `drawlist`;
`lot::pick_lot` moved to `lotpick`; `shader::index`; `Assets` (the renderer's) keeps the textures
by name and archive id. The log mode's figures are unchanged (97.63/97.62%, whole 88.89%).

### W1: the floors, the shadows and the camera from the engine

`render-check` in the engine mode now makes everything of the frame but the Sims (the skinned
parts and their shadows, 85 + 34 draws) and the portrait (57 draws) from the engine; the log is
the replay's inputs, the lot choice, the clocks, and the camera's parameters. The order is the
original's: for each room's batch (rooms 0 to 3 on frame 2400) its floors, in the first batch the
lot's own opaque draws (the ground the shadows fall on), the batch's shadow draws, then the floors
again as the mask (`engine_scene::assemble`); then the models, opaque, the Sims, blended, and the
portrait.

Figures, frames 2400 and 2401, within 4 in every channel (the base is C3's engine mode with the
log's floors, Sims, shadows and camera):

| | log mode | base | W1 (2400) | W1 (2401) |
|---|---|---|---|---|
| whole frame | 88.89% | 85.88% | 85.92% | 85.91% |
| rigid draws' pixels | 97.63% | 86.64% | 89.17% | 89.15% |
| floors' own pixels | 85.51% | 85.57% | 84.69% | 84.67% |
| shadows' own pixels | 86.59% | 79.33% | 78.55% | 78.57% |
| Sims' own pixels (log's draws) | 88.50% | 88.52% | 88.52% | 88.98% |

(The floors' own pixels count what the last depth-tested draw is a floor: 207,268 now, 205,000
before, since the porch's floor now shows at the walls' feet, below.)

Camera. `engine_scene::camera`: the live parameters of the last 'c' record at or before the tick
through `scene::Camera::view` and `Camera::projection` (the wanted distance): the logged view to
3.8e-6 and the projection exactly. The engine has no parameters of its own at tick 1,039: a lot
loaded from a save has no player input to ease the camera from (`Camera::ease`, `turn`, `zoom`
and `centre_on` take it), so the parameters are the log's, through the engine's `Camera`, not the
logged matrix. Taking them from a replayed run of the player's stick and cursor is not built.

Floors (`housefire::floors`, `scene::terrain_runs`, `LotLights::floor_group`, `floor_colours`).
The runs are `scene::floor_runs` and the terrain's (the tiles 1 to 37 each way with no floor,
`TERRAIN_TILES`, the half tiles at a diagonal as the floors'); the strips of the lot's 39 points:
98 on frame 2400, **all 98 the log's strips vertex for vertex** (position and texture
coordinates; before the limit to the 39 points there were 18 more, the road's columns past the
lot, which the original does not draw). A run's room is the room of its first tile (batches 0 to
3: the outdoors 73 strips, 15, 5 and 5). Each corner's colour is `light::vertex_colour` of the
light grid's sum there (`light::cell_lights` of the corner's point for the strip's room, then
`point_light` of those lights) with the room's group base and scale (`light::group_colour` from
the shader's diffuse and ambient and the room's ambient and, outdoors, the sun's light times its
downward part): of the 2,934 corners the log holds, 2,440 exactly and 494 within 4, none beyond
(`HF_FLOORCMP=1`), at the engine's clock of 7:36:32 (the original's relight was a few seconds
before its own). The material is the shader's (the terrain's two layers cross-faded, the second
read at the position by the shader's own map (`Shader::position_uv`); an indoor floor's reflection), Z
less-or-equal with writes, no alpha test (the shader's flag would give the wood one the logged
draws do not have: +0.2% of the whole frame), a destination alpha of 0x7F. The mask draws (the floor
again: alpha written 0, no colour, the floor's texture clamped and alpha tested at 5) come after
each batch's shadows. The floors' pixels are as the log's (85.51%): the terrain's alignment.

Shadows (`drawlist::wall_shadows`, `diagonal_shadows`, `object_shadows`; `engine_scene::
shadow_material`, `shadow_view`, `Assets::engine_shadows`). What the 692 logged shadow draws are:
every part of what casts, drawn again as its vertex strip (not the display list: `mesh_of(batch,
false)`) at its lit draw's world matrix, in one material (flat black, blended by the destination
alpha, Z off, alpha test at least 5: all 692 draws have it), through a view that shears the lot's
z onto the ground. Found against the log's draws (world matrix and vertex count): 649 planned,
of the log's 656 not the Sims' 641 the same (world matrix and vertex count); the 15 left are the 5
diagonals' (below), the aquarium's 7 fish, a held tray and newspaper, a sink's second piece and
the engine has 8 the log has not (the diagonals', the trunk's and the gnome's).

- Casters: every wall piece on an outdoor tile (`WallPiece::cap`) and fence, seen or not, with
  all parts of the plain wall model (not the opening's: a window or door piece has the plain
  piece's four sub-models, 5, 6, 5 and 18 vertices); the diagonal tiles' outdoor face; the
  objects whose resource row has the bit 0x02000000 or 0x10000000 and neither 0x40000000 (hung
  on a wall) nor 0x20000000 (the counters): furniture, plants, the beds, the sinks; not the rug,
  doors and windows (rows 0x01000000). Read off home's 17 kinds of row (`NO_SHADOW_FLAGS`,
  `SHADOW_CASTER_FLAGS`).
- Lift: a part drawn at its object's own matrix (no skeleton) is raised 0.1 (`SHADOW_LIFT`); a
  part on a bone is where the bone is.
- Batches: the pieces' and the outdoor objects' in room 0's (571 draws on frame 2400), an
  object's in its room's (`LotLights::room_of_place`: 43, 26 and 9; the log's batches have 570,
  87 less the Sims' 36, 26 and 9).
- Culling: the pieces are drawn when their place is in the screen box (`drawlist::in_view`, moved
  from lockstep's `frame_draws`: the fitted box of the lit draws); an object's are not culled (a
  tree's crown reaches the screen from a place off it).
- The view. Not the camera's: the third column of the logged 3 x 4 view is the camera's x column
  times a plus its y column times b: a shear of the lot's z onto the ground, a point of height z
  moved by z (a, b). Frame 2400's are (0.7188, -0.1843) for the 341 draws of the outdoor objects
  (the trees, the bushes, the mailbox ...), (0.8855, -0.2271) for the wall pieces' and fences', and a small different vector for each
  indoor object (a Sim's too). The sun there is (0.8223, -0.2109, -0.5285): the shears are its x
  and y over 1.144 and over 0.9286, neither the sun's own vertical part. (K5 found the forms of
  all three: the walls' is the sun's z plus 0.4, the objects' a linear fit of two lots' suns, the
  indoor objects' an inverse-power sum over their light set: `engine_scene::wall_shear`,
  `sun_shear`, `light_shear`, "K5" below.)
- The pieces' meshes cover the ground in triangles with gaps; the reference's wall shadows on the
  ground are whole: the log mode has the same (86.59% there with the log's own draws), the
  polygon texture's alpha is not applied (`shadow_of`).
- The diagonals' shadows are of the form's model now (`diagonal_form` of the face on the
  outdoor side), no longer the plain one.
- The money trunk's and the gnome's shadows are drawn (the engine draws the objects, the original
  does not).

What else the work found.

- The walls' 7-vertex pieces (a wall's foot; a door's 10, a window's 7) name the junk texture
  `_garbage` in their shader, and the engine drew them opaque black over the walls' feet (a band
  at the front of the house): none of the log's draws names it, and the log's probe at a pixel of
  the band has the porch floor as the last draw. They are not drawn now (`PLACEHOLDER_TEXTURE`):
  rigid 86.64 to 89.17%. The drawlist is untouched by it: lockstep's frame-draws figures are unchanged (570 the
  same; the study matches by part, so which of its 26 missing draws are these is not checked). The floor shows at the wall's foot, where the
  original has darker: what the original draws there is not found (the figure of the whole frame is
  the same with the black pieces, 85.93%).
- The engine's polyshadow-textured wall pieces (73) are drawn in the lit pass as the log's 74.

The Sims from the engine's state (S1: `sims.rs`, `housefire::figure`; `HF_NO_SIMS=1` keeps the log's).
Frame 2400 (tick 1,039), the Sims' own pixels 88.08% (log mode 88.50, the log's Sims in engine
mode 88.87), the portrait's square 93.96% (log mode 93.99), the whole frame 87.19% (36,739 pixels
beyond, 36,729 with the log's Sims); frame 2401: 88.36% (the log's: 89.33), whole 87.18%. Log mode
unchanged (rigid 97.63%, whole 88.89%). With the layered pose and the engine's own heading (A1,
below; the portrait's yaw refitted): the Sims' own pixels 92.50% (frame 2401: 92.97%), the
portrait's square 96.10% (96.36%), and in the current engine mode the whole frame 97.23% (97.23%;
before A1 97.21% and 97.20%, the Sims' 88.08% and 88.36%); log mode unchanged (whole 97.47%).
What the engine makes, from the run (`Replay` to tick
T-1, `Replay::sim_plays(T)` for the plays):
- Who and where: the Sims on the lot (`world`'s persons), each one's description from the save
  through its neighbour (person data 31: `figure::Cast`), place from x16 and y16 (24.5, 19.0 and
  24.5, 22.5, as the logged roots), facing its heading in eighths. The Sim's turn is the
  walker's (Route_Follow's +56, not logged): at a walk's end it turns, 3 degrees a frame, to
  face the plan's last point, the one after the walk's end (`routing::turn_frames` checks the
  frames that takes). So the heading is the step from the last plan's last point but one (the
  Sim's own place) to its last point (`Replay::sim_faces`, `figure::heading_along`: the eighth
  of that step's turn), else the facing the lot gave it. The `facing` field is stale after a
  walk (the cook at the stove has 0 and stands at 6; the male's 4 is right): both Sims'
  headings are the ones that put their bones on the logged matrices at ticks 1,039 and 1,040
  (`sims::headings_from_log` stays as the study that fits them).
- Which picture: the animation frames of ticks before T and the first of T's own (a tick has one
  or two: 2398 at 1,039, 2399 and 2400 at 1,040; the pictures show the pose after 2398 and 2399;
  one frame more or less loses 28 points of the Sims' pixels); `Replay::sim_plays` runs them on a
  copy of the animator, the run is not moved.
- The pose: the Sim's tracks in the animator's order (`figure::tracks`, `figure::node_states`:
  each track's local pose per node, `pose::TrackStreams`, folded into the node states from the
  default pose by `pose::fold`, a node's weight its track's times its own; backward plays from
  the last frame down, `pose_place`), under the root and the controller's scale 2^-12
  (`figure::BONE_SCALE`, the '<' records'), the bones by `skeleton::bone_matrices`. The tracks
  (the log's '8' and '(' records at ticks 1,039 and 1,040 show 4 a Sim, 3 posed):
  1. the Animate play (weight 1, the base: the first track that is not a layer);
  2. the carrying loop (`a2o-rarm-carry-loop`, `c2o-` for a child) when the Sim holds an object
     (another object's container is the Sim): weight 1, held at its last frame (end mode 1),
     node weights 1 on nodes 47 to 51 (the right arm) and 0 on the rest, so it copies the arm;
  3. the face (`happyFace`, a layer: the track's +8 has bit 0x100, held at frame 0 by its speed
     0): the track weight is the Sim's mood motive (index 3) over 100 (the logged weight at each
     of the 20 changes on home-a39.log's first 643 ticks, by eye, is the motive's value of the
     tick over 100 to a few ulps; the other motives' weights are not tried), node weights 1 on node 16 only (the neck), 0 on the rest.
  The standing loop runs but is not posed while an action plays (no '(' record for it: weight
  0). Against the logged matrices of ticks 1,039 and 1,040 (the '$' skinned primitives' four
  bones and the worlds of the parts on one bone; LOCKSTEP_ANIM_BONES, `anim/sims.rs`): the
  action alone gave 23 of the male's 58 bones and 26 of the cook's equal to 1e-4 (37 of the 65
  logged matrices of tick 1,039 equal some Sim's bone); with the carrying loop and the face,
  53 and 47 (62 of 65: the 3 others are the identity matrix of another draw, the carried
  plate's matrix on the hand, and the male's head: below). The bones left are none a logged
  matrix is near: the root nodes 0, 1, 55 to 57 (the nearest logged matrix is another bone's,
  0.04 to 0.3 tile), the cook's thumbs and their prop nodes 45, 46, 53, 54 and her hair nodes
  37, 38 (0.06 or less: a neighbour's). A bone drawn with a wrong matrix would leave its
  logged one unmatched. The one real difference: the male's head (the matrix of the parts that
  hang from it, hair and glasses) at tick T equals the engine's head of tick T-1 (0.027 tile
  off), the skinned head bones of tick T's own; the cook's heads equal the current one. Not
  explained (drawing the portrait's rigid parts from the tick before's bones did not raise
  the picture's figures).
- Parts: the outfit's nine slots through `PartTables::dress` and the models' records
  (`models.arc` by the name's CRC), the draws in the log's order (hair, eyes, nose, mouth, upper,
  lower, shoes, accessory; the portrait: eyes, nose, mouth, hair, upper, the glasses). A batch on
  one bone takes that bone's matrix as its world (3 x 4 rows of the 4 x 4, `figure::rows`), a
  blended one (3 or 4 matrices, weights) the GPU skinning through its slots' bones
  (`skinned_mesh_of`: the model's positions, normals and the batch's weights; the log's '+'
  records differed only in the faces' few vertices). Materials from the shader (`_______BLANK`:
  one layer; the jewellery's own texture and alpha test).
- Lights: the room's ambient, the four lights of the light grid's point of the Sim's own tile
  (floor of its place, not the round of an object: both logged sets, 4 lights each, equal), and
  a white 0.4 directional that every logged Sim set has (`SIM_FILL`, world direction (-0.753,
  -0.293, -0.589): measured in the one view, so fixed in the world here: unverified for another
  camera). The portrait's lights are the log's constants (ambient 0.2, two directionals in its
  own view), its projection and viewport the log's.
- The portrait's camera: in front of the Sim, 1.1973 away at 1.6014 high, pitch 0.0825 and yaw
  -0.00015 (`PORTRAIT_*`): the first three from the logged view (eye (24.5010, 17.8027, 1.6014),
  pitch 0.0835), the pitch and yaw then refined on the picture (Sims' pixels beyond 4: 412 to 207).
  The original's head-following is not read; at one heading and place only.
- The accessory (glasses) is drawn in the portrait and, as the log's last strip, once more in the
  lot's view with the portrait's light (the original's own repeat, kept).
- Shadows: `shadow_adult` through the same bones, flat (indoors) or sheared (outdoors) like the
  objects', in the Sim's room batch.
- The window: `view` takes the wardrobe, the plays and the headings each tick; its shot of tick
  1,039 against render-check's engine mode (`ours_2400.png`): 99.99% within 4 (39 pixels beyond;
  98.89% before); the frames 100.00% within 33.3 +- 2 ms (1,170 frames; the whole frame 4.5 ms mean
  on the PC, 2.8 ms of it the build).
Still from the log: the cook's heading, which Sim the portrait shows (the log's overlay
draws name its parts: `portrait_sim`; the engine has no player), the frames' places (`k`
records), and what the Sims hold (Prop_Spoon, Carry_Soup_in_Pan, Carry_Snack_Chips,
Carry_Newspaper and the plumb bob: 5 draws, a few pixels). Left: the standing loop and the
masks, the face, the arms' carrying loops, the head's look; the held objects.

Shared code changed in S1: `Replay::sim_plays` (and `anim::Animator::plays`, `Play::place`,
`Play::backward`; the frames' loop of `AnimReplay` split into `run_frames`, which also runs on a
copy), the new `figure` module, `engine_scene` (`save_file`, `Engine::sims`, `assemble`,
`Builder::build_with`) and `sims.rs`. The replay figures, lockstep, cargo test and clippy hold
(`runchecks` 10 of 10, the replay of home-a39.log the same with the catalog); the log mode is
unchanged.

T1, the Sims' textures from the disc: the engine mode and `view` now make each Sim's 128 x 128 C8
image and palette themselves (`SimSkins::with_disc`; HF_LOG_SKINS=1 keeps the log's), by the
description and the part tables (see "The Sim's run-time texture", under "Run-length textures"):
the cook's and the male's are the log's byte for byte (0 of 16,384 texels and 0 of 256 palette
entries differ for each). What is still the log's is the sampler, which is the same on every logged
draw and a constant now (clamped, bilinear, one level), and, in the log mode, the textures. The
figures of render-check's engine mode do not move: 2400 the Sims' own pixels 88.08%, the whole frame
97.21%; 2401 88.36%, 97.20% (log mode 97.47%). Shared code changed in T1: `outfit.rs` (the part
tables keep the first four words of each entry; `PartTables::entry`), `sims.rs` (the engine's
textures in `engine_sims`), `view.rs` and `render-check.rs` (`with_disc`); the new `sim_texture` and
`quantizer` modules and lockstep's `LOCKSTEP_SIM_TEXTURES=1` study. `runchecks` 10 of 10, both
crates' tests and clippy hold.

Shared code changed in W1: `drawlist` (`LotDraws::shadows`, `wall_shadows`, `diagonal_shadows`,
`object_shadows`, `wall_record`, `VISIBLE` and `in_view` moved here from lockstep's
`frame_draws`, which uses them), `lighting::LotLights` (`floor_group`, `floor_colours`,
`room_of_place`), `scene::terrain_runs`, the new module `floors`; the renderer's `engine_scene`
(`camera`, `assemble` in place of `splice`, `engine_floors`, `engine_shadows`, `bind_shader`),
`scene.rs` (three items made `pub(crate)`). The simulation is untouched: replay and lockstep 8
of 8 and cargo test and clippy hold (`runchecks` 10 of 10); lockstep's frame draws are unchanged
(570 the same, 3 different, 26 missing); the log mode is unchanged (rigid 97.63%, whole 88.89%).


### E1: the gap between the engine mode and the log mode

Frame 2400 (2401 within 0.02), engine mode before and after, with log mode (97.99 / 97.33 / 97.84 /
97.47%, unchanged): rigid 94.09 -> 97.02%, floors 96.22 -> 97.35%, shadows 91.76 -> 95.90%,
whole 95.60 -> 97.21% (8,012 pixels beyond; 12,621 before). Three causes, each found by the
pixels wrong only in the engine mode (HF_PROBE in each mode, HF_SPHERE, HF_COLOURS):

- The wall's foot (the 6-vertex part, the log's 7-vertex draw: its shader names `_garbage`) is
  drawn with the wall's pattern, as the log's draws have it (the same texture as the face's), not left out
  (`drawlist::WALL_FACE_VERTICES` 18 to 6, the 5-vertex piece keeps its own shader): the floor
  no longer shows at the walls' feet (a band along the front of the house, and the diagonals').
  Whole 95.60 -> 96.44%, floors 96.22 -> 97.35%, shadows 91.76 -> 93.82% (the shadows' pixels
  there were the floor's).
- A sphere-mapped layer (texgen 2: the reflecting doors, the lattice door, the range, the mirrors,
  the bathtub) keeps its factor byte 0xFF (1.0 in the log's draws); only the crossfades' 0xFF is
  0x80 (`engine_scene::RUNTIME_FACTOR`, `SPHERE_TEXGEN`). The walnut door 946 px off to 0, the
  lattice door 571 to 108. Whole 96.44 -> 96.94%.
- The lights' tint takes the shader's material factors: a light's colour byte is
  trunc(0.5 x diffuse x colour + 0.5), the ambient's 0.5 x the material's ambient (the head's words
  7-9 and 4-6, `Shader::material`; 1.0 for nearly all). The window glass (10 vertices) has a
  diffuse of 0.2, the bathtub 0.5: the log's point colours of those draws are 0.195 and 0.5 of
  the engine's (the 4 lights of 8 glass meshes, all checked by `HF_COLOURS`). Windows 899 px off to
  226. `lighting_for` takes the factors (the Sims' pass gives None). Whole 96.94 -> 97.21%.

Left (engine mode, frame 2400; px of the models off): the money trunk 678 and the gnome 78
(and their shadows 436 + 68; the trunk at screen x 0.0617 against the leftmost drawn object's
0.0618: no screen box tells them apart, so no rule), the outer lot 619, the window glass 226, the
counter 158, the wall caps 127, the lattice door 108, the aquarium 105, the birch 123 (log mode 1,091). Shadows: the
indoor objects' shears are not found. What the logged shears are (`HF_SHEARS=1`, per shadow draw
and its twin's light set): the 341 outdoor objects' and the 228 walls' and fences' are the sun's
x and y over 1.144 and 0.9286 (frame 2400's sun: 0.8223, -0.2109, -0.5285; 0.9286 - 0.5285 is
0.4001, but 1.144 - 0.5285 is 0.6155: no constant); each indoor draw has its own small vector, the same for every draw on
one light set (the clocks and the end table on one grid point: -0.173, -0.123), so it is a function of the draw's four lights.
Not it (fitted against the 34 indoor places with the log's own four lights): the nearest or the
strongest light's direction to the object's place over any one height, a sum of the four
directions weighted by rank, by reach^2/distance^2, by colour, or by 1/distance (angle errors 0.2 to 0.8
rad), a per-slot linear mix of the four (residual half the vectors' size). The 75 indoor
objects keep no shear. Other clocks have no frame dumps (the other logs have no draw
records, a33's frame 1 is tick 1,001, the same clock), so the divisors are fitted at this one.

The studies added to `render-check`: `HF_SHEARS` (the logged shadow shears; in the engine mode
with each planned shadow's twin, its place and light set), `HF_PLACES` (the objects' screen
places and whether the log has them), `HF_SPHERE` (the sphere-mapped draws and their factors in
both modes), `HF_COLOURS` (with `HF_LIGHTSTUDY` and `HF_ORACLE`: the log's light colours over the
engine's, with the shader's material). Shared code: `drawlist::WALL_FACE_VERTICES` (the lot's
wall pieces' shader), `engine_scene::lighting_for` (a third argument), `Inputs::lighting`
(takes the material factors): the log mode, lockstep's frame draws (583 the same, the output
identical to before) and `runchecks` (10 of 10) are unchanged.

### K2: the club from the engine's state

`render-check HF_ENGINE=1 ... --lot 15` (club-d3000.log, frame 3001, tick 964; the recomp's world dumps
are black on the club, so the reference is the whole frame with the 2D layer: the portrait square, 3,080
pixels, cannot match). Engine mode went from 2.94% to 71.42% whole (log mode 93.79%, unchanged). Fixes in
order, with the whole frame after each:
- the lot: `--lot N` is lot N of the disc's own save (the club), house N + 1 (the club is house 16), no
  card; with it the camera, floors and objects were already the club's (2.94%).
- the rooms' ambient: an inside room's level (0.3 day, 0.2 night, blended across 18:00, times its
  window brightness) applies to the colour (0.8, 0.8, 1.0), not the time of day's ambient colour
  (0.6, 0.6, 0.8 at 18:35): logged ambient 25, 25, 31 of 255 in every room (`lighting::ROOM_AMBIENT_COLOUR`,
  fitted; home's 7:36 has the same colour in both) 2.94% to 5.90%.
- the animations the objects play: the club's scripts set a graphic number (data field 0) that picks the
  animation (`ObjResources::animation`): the dance floor's beams and light pools, the laser show. The
  run (`engine_scene::run_to`, the replay to the tick before) gives each object's graphic and the render
  frames (the log's display copies) since it changed; `drawlist::Plays` carries them into the objects'
  poses and the lamps' lights. The animations' own clocks are not kept by the simulation, so the place of
  a moving lamp is found from the light grid the original last built (the log's 'e' records hold each
  light's position): the first run of quarter frames whose light positions are the build's (to 0.012),
  carried to the tick by `frames_per_copy` (rate / 30: the laser's 60 a second is 2 frames a copy, the
  logged places 72.5 and 108.5 at copies 2,997 and 3,015). The draws' matrices then equal the log's
  (checked for the dance floor's 62 + 120 + 47 + 40 + 4-vertex parts) and the light sets of the objects
  are the log's but for a place 0.1 off. 5.90% to 57.4%.
- the cones and pools (shader layer blend byte 2) add (source alpha, one) and write no depth; the
  reflection layer of a floor (`reflect_test_06`) reads one point, the terrain shader's offset (the
  logged position matrices have scales of 5e-7), not the position: 57.4% to 71.5%.
- the Sims: the Sims' descriptions are the played save's made Sims (a card, `--card`, was needed); the
  log's dressing records ('?', 16 words of the description each) give them from the log itself:
  `Cast::complete_from` gives a Sim the save does not describe the logged description no Sim has (with no
  card, all 9 in the order of the world: the records come in the Sims' creation order, the same as the
  card's for 8 of 9; the ninth is the story Sim of open.md's "Fran", whom no file describes).
K3 (the club's Sims' headings, 71.42% to 72.41% whole, Sims' own 19.01% to 37.26%): a Sim with no walk
to its place faced the facing field read as a heading; a heading is the other way round (`figure::heading_of_facing`:
facing 0 -> 2, 4 -> 6, 6 -> 4; the bones of 4 of the 8 Sims the log draws equal ours at that heading).
Study aids in render-check: `HF_HEADINGS=1` (per Sim, the heading whose bones equal the most of the logged
'$' matrices: `sims::bones_matched`, 48 to 55 of 58 for the 8 Sims, so poses are right and only headings
differ), `HF_SKIPSIM=id`, `HF_SKINDRAWS=1`. Found, not fixed: (1) the Sims 177, 174, 171 and 178 turned
after their walk or their start (the logged headings 1, 4, 1, 1; ours 2 to 5) by what the engine does not
keep (facing is the save's, a walk's end sets nothing in the world); (2) Sim 176 (a robe, at (18.5, 17.5), the
one in front of the DJ booth in ours) has no skinned draw in the log at all (0 of 58 bones equal at any heading):
the game does not draw her though she is on screen; she alone has data field 34 set among the visible
ones, unverified as the cause (about 3,000 px); (3) the pool table's felt: the log's draw is the same record with the
club's texture `game_pooltable_club_x` (flags 8, 72) where the engine's shader gives `aristoscratch_pool_table`:
a texture the lot's dataset overrides, not another model record.
K4 (the club, 72.41% to 80.53% whole; rigid 69.37% to 80.87%, floors 85.80% to 87.81%, shadows 51.18% to 69.17%, Sims 37.26% to 38.06%):
- an object draws the model its own dataset lists first (`drawlist::object_models`): the club's pool table's ResData
  names the stock table's model (`AristoScratch_Pool_Table`, id 868669be) while its dataset `RD_game_pooltable_x_club`
  lists the club's own (`bf4f0a81`, shader `game_pooltable_club_x`, the key word): the felt, 72.41% to 75.44%.
- an opaque draw tests its texture's alpha (at least the shader's float) only when its first layer's word has bit 4
  (0x1c: trees, fences, glass, headphones; 0x18 the club's black wood and the DJ booth's grille, which the log draws
  untested; `engine_scene::LAYER_ALPHA_TEST`), not whenever the texture has clear pixels: the booth's front panel and
  the door, 75.44% to 76.20%.
- the logged frame is one display copy past the copy count at its tick (`DISPLAY_COPIES_AFTER_TICK`, 1; at 0.5 77.96%,
  0.9 80.55%, 1 80.53%, 1.1 79.82%, 2 75.57%): the moving lamps' cones, pools and flares were at the first of the
  frame's two passes' places (rotation 1-2% off the shown pass), now within 0.1%; 76.20% to 80.53%.
- the log's frame holds the lot's draws twice (strips below and from 2,146 in this frame: two passes of floors, shadows,
  objects) and the picture is the second: with the first pass's model draws left out log mode is 97.62% (not 93.79%)
  and the dance floor 2,165 of 87,447 off (`HF_DROPSTRIP=0,2146`). The two passes are two frames: tick 964
  holds display frames 3000 and 3001 (a tick holds one or two). The observer now writes its '|' record also
  before a draw record when the display copies changed (recomp, src/tree_log.cpp `note_frames`), the parser
  keeps them by the strips before (`Log::frame_strips`), and render-check takes a frame's strips from its
  record to the next, leaving the tick's other frame's model draws out (log mode only). On club-d3000b.log
  (the same run, recorded again with the new records; work/frames-club3, the full frames as world_): frame
  3001 is strips 1,845..3,690, log mode 97.62% whole, rigid 98.65%; engine mode 80.53% as before. Home's
  frames are one to a tick: both modes' PNGs the same.
- a layer with lighting `unlit` (the cones, pools, flares, a street light) has lighting off in 9 of the log's 10 such
  draws; setting it changed no pixel (the combiner ignores the lights for it), so it is not applied.
Studies (render-check): `HF_MODELMAP=name` (that model's pixels, off ones red, and by texture), `HF_MATS=name` (its draws'
matrices in draw order), `HF_STATES=1` (draws counted by model, texture, blend, alpha test, lighting), `HF_SHADER=prefix`
(a shader's layers), `HF_SIMSTATE=1` (each Sim's non-zero fields), `HF_DROPSTRIP=lo,hi`.
Left (pixels beyond 4 of 286,720: 55,833): the dance floor's draws 23,424 of 86,392 (the cones and pools under the Sims,
where the Sims' own pixels and the robe Sim are wrong: 6,360 cyan and 11,164 pink cone pixels), the DJ booth 9.2k (its
moving parts differ in x by 0.03-0.05; the robe Sim and her pink cone over it), the robe Sim (176: drawn by the engine,
not by the game; her state differs from the other Sims' in data field 34 = 1 and person data 10 = 1000, 15 = 200, 67 = 1;
no rule found, field 34 is the motives gate's at the club, not a draw flag), the four Sims' headings, the bar 1.2k,
the portrait square 3k.
Home: log mode identical; engine mode 97.23% to 97.24% (31 pixels, rigid 2,391 to 2,360 beyond; the cause is not isolated: the objects now
play their graphic's animation, and the room ambient's colour is the one constant of home's 7:36).
Left, club engine mode (pixels beyond 4 in every channel of 81,954): the objects whose textures
a lot's dataset overrides (the pool table's felt is `game_pooltable_club_x-02` in the log, the sub-model's own
shader in the engine, ~7,000), the DJ booth's parts (~15,000: z-fighting of coplanar faces and its parts'
states), the Sims' headings and the DJ's place (the DJ stands behind the booth in the log; the portrait's
look), and the shadows (42,000: the indoor objects' shears, "What is left" of E1).

K5 (the club, 80.53% to 87.59% whole; rigid 80.87% to 88.78%, floors 87.81% to 94.28%, shadows 69.17% to 84.26%, Sims 38.06% to 38.05%;
log mode club 97.62% and home 97.47% unchanged, home engine mode 97.24% to 97.26%, its shadows 95.90% to 95.89%, rigid 97.07% to 97.09%):
- Shadows' shears (`engine_scene::wall_shear`, `sun_shear`, `light_shear`; studies `HF_SHEARS`, `HF_SIMSHEAR`). The club's sun
  is (-0.4190, 0.8903, -0.2) (z set, not normalised), home's (0.8223, -0.2109, -0.5285); the logged shears: walls and fences
  -0.6984, 1.4839 (club) and 0.8855, -0.2271 (home), i.e. the sun's x and y over its |z| + 0.4 on both lots (0.6000 and 0.9285
  against 0.6000 and 0.9286); outdoor objects and Sims' shadows -0.5086, 1.0806 and 0.7188, -0.1843, divisors 0.8239 and 1.144,
  which are 0.9744 |z| + 0.629 (two points: the slope and the base are fitted to them, `SHADOW_SHEAR_SCALE` 1.0262 over
  `SHADOW_SHEAR_BASE` 0.6455). Indoors (the club: 26 objects of the main room, the Sims' shadows) the shear is the same form on
  V, the sum over the draw's four lights of (place - light) / distance^1.7, with the light's position in 3D and the object
  on the floor: shear = 1.0262 V_xy / (|V_z| + 0.6455). 26 objects within 0.02 of the logged shear (their length 0.2 to 0.9);
  no colour or reach weights (they made it worse); the power is fitted (1.5 to 2.0 fit about as well); the nearest-light,
  centroid and per-light-weight forms are worse (0.05 to 0.18). The Sims' own: the log's 9 shears within 0.05 of the engine's
  (the Sim's tile centre against its real place). A multi-tile object's shear is taken at its footprint's middle, its
  light set at its leader's tile (`Plan::place_of`; the booth's logged -0.352, -0.060: -0.348, -0.071 against -0.392, -0.189 from
  the leader's tile): club whole 87.52% to 87.59%. The shadows of both lots are drawn with them: club whole 80.53% to 81.30%
  (the floors, 87.81% to 90.51%, take the Sims' and objects' shadows), home unchanged. Not explained: why the divisors
  (0.4; 0.9744 and 0.629; the power 1.7).
- Lights of an object are those of its LEADER'S tile, not of the middle of its footprint (`scene::Placement::lit_at`; the
  draw's `origin`): a booth of three tiles, a dance floor of 25, the doors of two. The booth's leader is at (17.5, 16.5), its
  footprint's middle (17.5, 17.5); the log's four lights of its draws are the lamp at (14.5, 19.5), (20.55, 18.81), (24.18, 15.61) and the lamp at (22.5, 13.5),
  which the grid point (18, 17) gives (in another order) and (18, 18) does not (the fourth is the cyan lamp at (22.75, 18.8)): club whole 81.46% to 85.51%, rigid 81.12% to 87.60%, home 97.25% to 97.26%.
  (The one-tile objects' places are unchanged: a tile's centre is x.5 and the point is rounded, x.5 up. A tie in y that
  floors, as the doors', gives 173 pixels where 960 are off: the doors' two parts have two tiles, the leader's tile is 13.5, 17.5
  and the logged set is the grid point (14, 17); unexplained, and the same floor in y makes the home lot's bookcase, counter
  and sofa fall by 0.22% whole. `HF_CELLSTUDY` lists the grid points whose set is the log's.)
- The lamps' lights: the grid's membership is from the lamps' places at the last build (`Play::anchor_at_build`,
  `Plays::place_at_build`: the anchored place before the display copies since), the lit draws read the lights where the frame
  has them (`LotLights::follow_lamps`; the logged lit draws show a lamp at 20.54 and 20.55 in the two frames of tick 964, the
  build's place being 20.58): club whole 81.30% to 81.56%.
- Floor coverings (`engine_scene::split_lot`, `lies_flat`, `COVERING_HEIGHT` 0.05; studies `HF_FLATSTUDY`, `HF_ROWFLAGS`,
  `HF_LISTLIGHT`, `HF_LISTMODEL=SKINNED`): an opaque object draw whose mesh lies flat on the ground (every vertex under 0.05
  high: the dance floor's plane at 0.018, home's rug at 0.010; every other opaque mesh of both lots reaches 0.05 or more) is drawn
  in its room's batch right after the floors and before the shadows, with the ground's destination alpha 0x7F and no depth write
  (the log's draw 938: depth true/false, dst 127; the order "Floor, RigidStrip/electronics_dance_floor/op, Shadow ..."), and the
  object casts no shadow (the log has none of the dance floor's, whose other 39 parts are blended lights; the engine drew 40).
  The Sims' shadows on the dance floor were hidden by it before: club whole 85.56% to 87.52%, floors 89.75% to 94.05%.
- Rows' flags do not tell the dance floor from the other objects (0x02000000, like the booth's); `ROWFLAGS` of the club's kinds are
  in the study output.
Left (pixels beyond 4 of 286,720: 35,582): the five Sims (about 12k: the poses and headings, a black arm on the dancer, the
portrait square 3k, which cannot match); the robe Sim the game does not draw (176; skipping her gives +0.26%; the log has no draw
of her, no rule found in her state); the dance floor's 16.9k of 86,388 (its pools and cones under the Sims, the moving parts'
places); the bar 1.2k, the booth 1.2k and the doors 960 (the tie above); the counter 0.5k; the selection ring under the portrait
Sim is in the reference's 2D layer (also in log mode).

## The window

`view LOG ISO --card CARD [--lot N] [--ticks N] [--frame-times FILE] [--shot TICK FILE]`
(`engine/render/src/bin/view.rs`) runs the engine's own simulation of the lot and draws each tick
from its state: a window (winit 0.30, Wayland or X11; the surface is made through ash from its raw
handles) with a Vulkan swapchain, FIFO, 4 images, a UNORM format. The renderer draws its picture
as before and blits it onto the swapchain image (`Renderer::draw` with a `Present`).

The simulation is `housefire::replay::Replay` (the library's; the `replay` binary is its command
line): `Replay::new` loads the lot and the log's inputs, `step` runs one tick, `world` reads it.
`run` is `new`, `step` to the end, `finish`, so the replay's figures are as they were. `view`
passes the animation catalog (the Animate primitive native, as `replay` does on a log with the
frames), for the Sims' plays: the replay's own figures are the same with it (home-a39.log: the
same 265 lines).

Per frame (tick T, 30 a second on a fixed schedule from the first frame, a late one does not move
the rest): the camera is the log's last 'c' record at or before T (`engine_scene::camera`); the
world is the one the run has when T begins (T-1 ticks run, the clock's passes before T, as
render-check counts them); `engine_scene::Builder` makes the draws (`Lot::plan`, then the
engine's draws, floors and shadows in the original's order); the meshes and textures not seen
before go to the GPU (`Caches`: 243 meshes and 94 textures for the 1,200 ticks); the floors' strips
are the frame's own meshes (`Renderer::set_transient_meshes`, one host-visible set rewritten each
frame: the light moves their colours every tick, so as persistent meshes they grew by 36 a tick);
then tick T runs. The Sims (their parts, shadows and the portrait) are the engine's too, from
the run (S1, "The frame from the engine's state"); what they hold and the interface are not drawn.

Measured (home-a39.log, the answer-key run: gamescope headless 640x448 at 30, an RX 9070 XT, 1,200
ticks, frames 31 to 1,200): the frames begin 33.33 ms apart (99.83% within 2 ms of it; the one
outlier is the shot's frame, drawn twice and written as PNG), the frame's whole CPU time 7.0 ms
mean (p99 8.8; the waits for the GPU included), of it the tick 0.22 ms mean (max 3.7), the build of
the frame 2.84 ms (p99 3.5), the GPU's own 0.15 ms (max 0.22). `HF_VALIDATE=1`: no validation
messages (gamescope's window layer forces VK_EXT_swapchain_maintenance1 on, which asks for the
instance extension VK_EXT_surface_maintenance1; `view` enables it when listed).

The shot of tick 1,039 against render-check's engine mode of the same frame (`work/frames/s1/
ours_2400.png`, both with the Sims and the portrait from the engine): 99.99% of the pixels within 4
(39 beyond). Before the Sims (`work/frames/w1/ours_2400.png`) 98.89%, 3,028 of the 3,175 beyond in
the portrait's square.
The running world at tick 1,039 against the loaded save: the same 206 objects (none new or gone);
204 non-Sim objects' places differ (the six listed by `HF_WORLD_DIFF=1` are each x16 and y16 +8:
a tile's corner as the save keeps it, its centre as the run has it; the draws match, so the draw
list does not see it), the 2 Sims moved; 10 objects' data differ (listed by `HF_WORLD_DIFF=1`);
the clock is the same, 7:36:32 (the loaded 7:04:00 and 1,038 passes less the stopped ones, as
`engine_scene::plan` counts them).

## The budget

Measured with `view` (stage 5d, W3; `Renderer::stats`, printed at the `--shot` tick, and the
frame-time columns) on the home lot, tick 1039, 640 x 448, 1,200 ticks, on the PC (a Radeon,
radv, under headless gamescope): 1,549 draws (776 of the lot's models, 98 floor strips, 649
shadows; the Sims not drawn yet), 97,363 triangles, 2.55 MB of textures (RGBA8, every level)
and 1.50 MB of meshes resident. The GPU's time 0.14 ms a frame (p99 0.22); the CPU's: the tick
0.22 ms (worst 3.7), building the frame from the world 2.66 ms, the whole frame 6.7 ms with the
wait for the GPU. With the Sims (S1): 1,790 draws (the 1,549 and 241 of the Sims' parts and
shadows), 113,518 triangles, 2.96 MB of textures and 1.87 MB of meshes; the frame's build 2.81 ms
mean, the whole frame 4.5 ms.

Against the RK3566 (Mali-G52 2EE, two cores at 800 MHz; four Cortex-A55), by the figures, not
yet run there: 2.9 M triangles and, at an overdraw of about 3, under 30 M pixels a second are a
small part of what the GPU does, and 4 MB of resident data fits easily. The CPU is the risk: the
A55 has measured about 6.7x the PC on the planner (docs/plan.md), so the frame build would be
about 18 ms of the 33 ms frame, beside the tick's 1.5 ms (worst ~25 ms past the load), and 1,549
draw calls a frame on the Mali driver are to be measured. The way down: build the draws of
objects that did not move once and keep them (only the Sims, animated objects and the floors'
light change each tick), and the shadows in one draw per batch. The device run is the user's
(SSH, as for the benchmark): an aarch64 build of `view` with `--ticks 1200 --frame-times`.


The build after P1 (same run, the shot's bytes and the figures unchanged): the build column's mean
2.89 ms to 1.24 ms (rows after the first 30; pacing still 100% within 33.3 +- 2 ms, 0 validation
lines). Where the 2.89 went, by a timing of the stages (the build, then the other costs): the
plan 1.36 ms (the draw list 0.53, the floors' strips 0.65: made again each frame because the time of
day moves their colours; the cell lookups, which a rebuilt light set did again), the model draws
0.77, the floors' draws 0.12, shadows 0.09, Sims 0.15, assembling 0.17, `sim_plays` 0.08. What
brought it down, in order of gain: the floors keep the light grid's cells while the lights' places
stand and the grid's source geometry is kept once (not collected for each lookup): 2.85 to 2.10;
the objects' guid lists made once a frame (they walked the world for each placement), the mesh
centres made at load (the part's mean position), the idle animations read without a copy of the
animation each time (`lamps::poses_at`) and an object's idle pose kept while its matrix and frame
stand, the floors' light summed once for each shared corner: 2.10 to 1.78; the Sims' plays copied
alone for the look one frame ahead (`anim::Plays`, 0.09 to 0.002 ms), the part found by its index
and not by a search, the record, texture and floor names shared (`Arc<str>`), the per-frame
lookups on a plain integer hash (`fasthash`), a shader's textures and material kept: to 1.40;
the cells of lights found for objects' places kept with the light set while the geometry stands:
to 1.24. What is left: the object parts' loop (~0.25 ms, mostly the 43 animated objects' poses),
the floors' strips and meshes made anew each frame (0.2 ms), assembling the list (0.13) and the
Sims (0.13). On the A55 at 6.7x the PC that is about 8.3 ms of the frame, no longer the risk;
unmeasured there. Not done: the models' arrays as plain vertex arrays (only the mean position is
made at load: the meshes are made once per batch, not each frame, so they are not in the frame's
cost), and one stepper for `Stream` and `RotStream`.

## Sound

Code: `engine/src/sound.rs`; the check: `assets ISO sounds [NAME [--wav OUT]]` and
`sounds_decode_as_the_answer_key` in `engine/tests/disc.rs`.

**Where.** Two ordinary archives (`arc::entries`): `samples.arc` (6,663 entries, each one mono
sample) and `audiostr.arc` (39 music streams). `sounds.arc` and `scripts.arc` are 4 bytes each
(empty). The scripts name samples in mixed case, so the bank (`SoundBank`) finds names without
regard to case, and by the archives' resource ids (the word before the offset, as for every
archive).

**The format.** Nintendo DSP-ADPCM, one channel per block: a 0x60-byte big-endian header, then the
data in 8-byte frames (a predictor/scale byte, then 14 samples of 4 bits). Header: +0 sample count,
+4 nibble count (frames' header nibbles included), +8 rate, +0xC loop flag (u16), +0xE format (0 =
ADPCM; every entry is), +0x10 loop start and +0x14 loop end (nibble addresses), +0x18 current
address (2), +0x1C 16 coefficients (i16), +0x3C gain, +0x3E the first frame's predictor/scale, +0x40
and +0x42 the history, +0x44 the loop's predictor/scale, +0x46 and +0x48 the loop's history. The data
is `ceil(nibbles / 2)` bytes after the header.

**The decode** is the DSP accelerator as `gcn-recomp`'s `ax.cpp` models it (`Accel::sample`, format
0), in integers. At each 16-nibble frame start the frame's first byte is the predictor/scale (low 4
bits the shift, bits 4 to 6 the coefficient pair) and the address skips the two header nibbles; a
sample is `(1 << shift) * t + ((0x400 + c1 * yn1 + c2 * yn2) >> 11)`, `t` the signed nibble, clamped
to [-0x7FFF, 0x7FFF] (not -0x8000); then `yn2 = yn1`, `yn1 = sample`. Decoding starts at nibble 2
with the header's history. A one-shot decode yields the header's sample count. Loop playback:
past the loop end the address goes back to the loop start with the loop's predictor/scale and
history (for the streams the accelerator keeps the history, for samples it reloads it). Checked
against the answer key `work/sound/ref.txt` (made by an independent Python decoder: every 200th
sample entry, and 1,000,000 samples of channel 1 of `sbo_pop4` and `loadloop`): all 36 hashes,
rates and counts match.

**Streams.** 24,000 Hz mono, or 48,000 Hz stereo stored as two whole channel blocks one after the
other, the second with its own header: it starts at the first block's end (0x60 plus the data)
rounded up to 32 bytes (`sbo_bb4`: 8,202,528; `sbo_bb8`: 8,630,784, as found). The reader takes
the second block only if its header has the same sample count, nibble count and rate.

**Found.** No entry has its loop flag set. Every header does carry a loop span of the whole sound
(start 2, end the last nibble), as the DSP tools write it, so loop playback is the game's voice
deciding (the sound's name says `loop` where it is meant to); `Sound::looped` therefore jumps back
over the header's span whatever the flag.

**Counts** (`assets ISO sounds`): 6,702 sounds (6,663 + 39), none failing to parse or decoding a
count other than its header's; 1,178,788,465 channel samples in all (about 3.7 s in release for
the lot). By rate and channels: 1 at 8,000 Hz; 124 at 11,025; 9 at 22,000; 6,523 at 22,050; 31 at
24,000; 3 at 44,100 (all mono); 11 at 48,000 (all stereo, streams). Of the 39 streams, 11 are
stereo at 48,000 and 28 mono at 24,000 (derived: the 3 other 24,000 Hz entries are in samples.arc).

**The sound events** (`snd::` tables of quick data's `SimsObjects`; found 2026-11-19 by reading
them, `tools/quickdata.py`). The scripts and animations never name a sample: they name an
*event*, and EA's sound layer resolves it through these tables.
- `snd::EventMapping` (1,969 named rows of 8 bytes): an event's name (mixed case, some with a
  leading space), then a kind byte and a pointer. Kind 1 (1,900 rows) points to its
  `snd::Track`; kind 30 (49, the DJ booth's music) also has a track; kinds 0, 19, 23, 43 and 101
  (19 rows: `radio_next_song`, `tv_turn_off`, `piano_play`, ...) have none (commands).
- `snd::Track` (1,884 rows of 20 bytes): flags (`0800828A`; `0800829A` for the voices), parameters
  (`64001040`, ...), then the choice: word 2 a pointer to a hit table by voice (below), or word 3
  a pointer to one of: a `snd::Patch` row (one sample), a `snd::GlobalHitlist` row (a random
  list), a `snd::VoxHitlist` row (male, female), a `snd::HitPatchList` row (11 tracks; not
  read); word 4 a `snd::TrackData` row: a script of the sound's behaviour (73 rows: ops such as
  `05 05 2710`, `05 06 01F4`: a chance of 500 in 10,000 for `counter_voxchop`; not decoded).
- `snd::Patch` (6,897 rows of 4 bytes): a sample's id (the CRC-32 of its name in capitals, which
  is `samples.arc`'s entry id).
- `snd::GlobalHitlist` (2,789 rows of 4 bytes): a pointer to the first of `count` Patch pointers,
  the count in the word before. Row 100 is {7: COUNTER_SCHOP1..7}.
- A hit table (word 2 of a track): a count in the word before it, then that many pointers to
  `snd::GlobalHitlist` rows, indexed by the Sim's voice; a null is no sound. A plain random
  event has a count of 1 (`counter_chop`: row 100). A voice event has 3 or 11 entries
  (`counter_voxchop`: 0, 0, then rows 372, 361, 361, 362, ...: two nulls, then groups of three,
  male, female, female). The tables are packed one after another (so the words after a table
  are the next one's count and entries). `snd::VoxHitlist` (619 rows of 12 bytes: the male
  list, the female list, a zero) is the vox form of a single patch. `sound_events.rs` reads all
  of this.
- `snd::NamedEventMapping` (17 named rows): the engine's own events by name (`Nite`,
  `PlayPiano`, `Reaper`, `footstep_ash`, `footstep_hard`, `footstep_hard_noshoe`, ...).
- Of the objects' FWAV chunks (6,345 in the disc's 744 IFF files, 961 distinct names), 951
  names are events (case-insensitive); none is a sample's name only; 10 are neither (an empty
  name, `genie_disappear`, `monkey_hut_open`/`close`, and six `vox_..._robot`).

### The mix

`mixer.rs` is the engine's own mixer, built to give the recompiled game's mixer
(`hw/ax.cpp`) its results to the bit. It mixes in frames of 5 ms (160 samples at 32 kHz), a
millisecond (32 samples) at a time: each running voice takes its next samples from sample memory
(DSP-ADPCM in nibble addresses, PCM16 or PCM8; the end, the loop and the loop's restored history),
converts them to the output rate (linear interpolation on a 16.16 ratio and a carried fraction, or
a hold), scales them by its own level and ramp, and adds them to the main, surround and auxiliary
buses through nine send levels (each with its own ramp), the clamp of each product at 32767 and the
two layouts of the mixer-control word (the plain one and Pro Logic II). Parameter updates
(per millisecond, by field) change a voice between milliseconds. The output clamps main L and R to
32767 and passes the surround as mixed. The voices are the engine's own structs; only the check's
adaptor (`bin/mixcheck.rs`) knows the game's 93-word parameter block.

Check: `mixcheck DUMP` runs the 17,646 frames of the home lot's recorded mixer inputs (the voice
blocks with their updates, the sample memory's uploads, the setup and buffer records) and compares
each frame's left, right and surround with the dump's output: `mix: frames 17646, the same 17646`.
It also compares each voice's state after its frame with the dump's next block of it: 454 of the
14,936 blocks differ, in words the game writes itself between frames (a started voice's addresses,
coefficients and header, mixer control and level changes), and the output does not depend on them,
since each frame reads its voices from the dump. One surprise in the reference: the interpolation's
product `(last[3] - last[2]) * frac` can overflow 32 bits, so the engine wraps it as the original
does. The run takes 0.08 s for 88 s of audio, 0.02 s of it reading the file.

### The voices

Code: `engine/src/sound_player.rs` (the sound calls to mixer voices), `engine/src/axdump.rs` (the
dump's records, the 93-word block layout and the replay shared with `mixcheck`), the check
`voicecheck LOG DUMP ISO` (the log of the same run as the dump: `work/sound/ax.log`, `ax.dump`).
The player takes the calls as the observer logs them (acquire, play, stop, params), copies a
play's ADPCM bytes into its own sample memory and keeps one mixer voice per handle; `advance` is
the sound library's sync between two 5 ms frames, `playing` the voices the next frame takes,
`mix` mixes them. Figures (home log, 95 plays, 17,646 frames):

    voices: starts 94, paired 94, settings the same 10527 of 10813 (voice-frames)
    mix: frames 17646, the same 17495, first different 11056
      difference of left and right over the first 12000 frames: peak 5, rms 0.05

The dump has 98 "starts" by the rule "a block turns running or its address goes back"; 4 of them
are a looping voice wrapping, so 94 starts (7 more are the music stream's restarts, left out).
Each play is paired with the dump's start of a sample of its size (the data's bytes: the end
address is the last nibble of the last byte) in the window 30 frames before to 300 after the call;
all 94 pair, and the 95th play is a second `UI_accept` in the same game frame on the same handle
(a restart in place: one start in the dump). A voice-frame is "the same" if the ratio, loop flag,
start offset (the address of the first sample, 2 nibbles in), volume, volume delta, the nine send
levels and deltas and the mixer control all equal the dump's block. The mix replays the dump's
setup, aux and CPU buffers and mixes the engine's voices; the music stream is the dump's own
voice (music is not the player's yet).

**Rules found** (each is a comment in `sound_player.rs`):
- **Timing.** The log's frame counter and the dump's AX frames: 200 AX frames a second over the
  game's 30000/1001 frames a second, 6.6733 AX frames per game frame (calls 468 game frames apart
  are 3,123 AX frames apart). A sound starts at once (9 plays, 9.2 to 9.8 AX frames *before* the
  counter says: UI sounds) or **about 1.15 s (230.2 to 231.2 frames) after the call** (86 plays:
  every object's sound, and `UI_SPEED_*`). The spread is one frame: the start is a whole frame.
  A later call of a voice shows in the dump `floor(delta * 6.6733)` frames after the start's
  block, the target block being the second frame after the call's own (below).
- **Ratio.** `floor(rate * 65536 / 32028.66)`: 22,050 Hz gives 0xB03D, the 48,000 Hz music
  0x17FA8; the two fix the output rate between 32,028.654 and 32,028.671 Hz (not 32,000). The
  other rates (8,000, 11,025, 24,000, 44,100) do not occur in the dump: unchecked.
- **Voice at a start.** Running, source 1 (linear), the sample's coefficients, the first frame's
  predictor/scale and history, the address of nibble 2, `end` the last nibble of the last byte,
  the loop address nibble 2 of the sample with the loop flag from the object's script (not in
  the calls: 2 plays loop, `FRIDGE_CHEAP_LOOP` and `STOVE_BUBBLE`; the check takes it from the
  dump). Volume 0 and a fade-in, or 0x7FFF if the first params call's word 3 is 1.0 (the UI's
  second and later plays; its first plays and `UI_SPEED_*` carry 0 or noise and fade in).
- **The library's sync.** Nothing changes during a frame. A start: block `S` has the levels the
  voice was left with (a new voice: 0x5A9D, 0x5BAA, mixer control 0x4003; an earlier one's
  leftovers otherwise), `S + 1` has the 0x4000 mark and, for a fade-in, volume delta 0xCC
  (0x7FFF / 160), and `S + 2` is settled (volume 0x7FFF, delta 0, control 3, or 0 if both
  levels are 0). A change of levels at any time is two blocks: the ramp block (old levels,
  delta `trunc((new - old) / 160)`, control = 0x4000 | 8 | the old low bits) and the target block
  (new levels, delta 0, control 3 or 0). A call at the game frame whose time is `t` shows its
  ramp block at `t - 1` and its target at `t`; the play's own params (same frame as the play)
  change at `S + 1` and `S + 2`.
- **Voices are pooled.** A voice a stop frees keeps its levels and control (0 and 0 after the
  stops, which come with params of 0) and is the first taken by the next handle; the handle gets
  a voice at its first play (the acquire reserves none), and a UI sound's voice is the one an
  object's stop in the same frame has freed, the object's play taking a new one.
- **Send levels.** `level(d) = trunc(32768 * 10^(d / 200))` for `d` in tenths of a dB, 0 at
  -960: all 65 distinct main-send levels of the dump are exactly these (test
  `levels_of_the_dump`). The two floats of the params (words 1 and 2, repeated in 5 and 6) are a
  position `x` and a volume `v`: `d_left = trunc(100 ln v + 200 log10 cos(pan pi / 254))`,
  `d_right` the same with `sin`, `pan = round(127 (1 - |x| / (2 v)))` (the sign of `x` does not
  matter): `v = 1`, `x = v` gives pan 64 and the dump's 0x5A9D and 0x5BAA (-3.0 and -2.9 dB:
  a sin/cos pan law in tenths of a dB, as the SDK's MIX library has it, unverified), the dogs'
  0.5742 and 0.4561 give -86/-85 and -109/-108 (exact). The volume's scale is `100 ln v` tenths
  of a dB, not `200 log10 v` (found by fitting the 30 params changes: 1.1513 times as many
  tenths per decade as a dB scale). The send is 0 below a volume of 0.1 (see below).

**What remains** (settings the same 10,527 of 10,813: 286 voice-frames differ, 602 differences
of words; the check's `VOICECHECK_DIFFS=1` lists each):
- **The moving sounds' levels: 4 voices (footsteps), 270 voice-frames.** The pan law and the
  volume scale above fit 10 of the 30 params changes of the dump to the tenth of a dB (python
  fit on the log and the dump; not in the repo), and the rest by 1 to 11 tenths: of the 501
  level differences on these voices, 263 are 1 tenth, 75 are 2, 93 are 3, 27 are 4, 39 are 5 to
  11 and 4 are 19 to 71 (a fade's last frames, where the cut of silence differs). The pan
  follows `|x| / v` linearly and the volume `ln v`, but their roundings (an integer pan, a
  quantised volume, integer tables) are not found: a 127-step volume, and floor/round/ceil on
  the volume, the two tables and the sum, gave no exact fit (best 15 of 30; a search of ~1,000
  variants, so that figure may be chance). Unverified: the original's volume-to-dB and pan code
  (the params call goes on to `MIXSetInput` and `MIXSetPan`, as the 0.1 dB steps suggest) would
  settle it; the calls do not show it.
- **The silence threshold.** A volume of 0.1416 sounds (a footstep's fade-out), 0.084 does not;
  the player cuts below 0.1, a round value in the gap (not a measured one).
- **A voice's leftovers at a start: 16 voice-frames.** Two plays at the AX frames 12223 and
  12229 (`FOOTSTEP_MEDIUM4`, `VOX_SING_HAPPY_SHORTF1`) take a new voice and an old one in the dump
  where the player gives the old one and then the new: the order of the voice takers in the
  frame differs from the log's (cause not found). Delaying a stop's free by 1 to 5 frames
  changed nothing; 6 or more made it worse.
- **The sub-frame start.** The offsets of the 86 late plays spread over 1.0 frame (230.2 to
  231.2): the game's frame counter is read at a different moment of the AX thread each time
  (the original's threads: not in any log). The check anchors each voice's later calls on its own
  start; a free-running engine cannot know the start's frame to within one, and a call landing a
  frame off the dump's would show as a level or delta difference above (not separated out).
- **The loop flag and the music** are not the player's: the loop flag is in the object's script
  (the player takes it as an argument), and the 7 restarts of the 48 kHz looping music stream
  (ratio 0x17FA8) are the music packet's. The mix takes the stream from the dump.
- The `mix` figure's 151 different frames start at the first footstep (frame 11056); they come
  from the level differences above (the 16 leftover frames included). Not checked separately.

### The sound out

Code: `engine/src/sound_params.rs` (the params rule), `sound_front.rs` (the simulation's side),
`sound_run.rs` (the mixer's side and the music), `engine/render/src/audio.rs` (the device and the
WAV), `play`'s `--mute`, `--wav FILE`, `--device`. Figures: `replay home-full-s.log ISO --card CARD`
(the `sound params:` lines; `REPLAY_SHOW_PARAMS=1` lists the misses and the events' track words).

**The path.** The script's PlaySound and KillSounds (`Alone::set_sound`, `take_sound_requests`:
the request, its object, the event its FWAV chunk names and the emitter's place, as `replay/sound.rs`
makes them) go to `SoundFront`, which draws the sample of the event's list from the presentation's
own numbers, applies the rules of "Sound starts" (a sounding handle is not started again, except
`counter_chop`'s; a voice event plays 500 times in 10,000), and makes `Command`s: play, stop,
params, music. `SoundRun` takes them between frames of 5 ms and mixes them (`SoundPlayer`, the
voices the dump checks, and the music) into 32 kHz stereo. `play` runs it on a mixer thread that
mixes 100 ms ahead into a ring; the device's callback (`cpal`, ALSA, which PipeWire and Pulse serve)
takes from the ring, linear interpolation from 32 kHz to the device's rate, and allocates
nothing (the ring is full size from the start, the lock is only tried, an empty ring is silence).
The commands reach the mixer through a short-lock queue (1,024 commands). No device (or `--mute`,
or a headless run without `--wav`): silent, said once. `--headless --wav FILE`: the mixer is
run in the tick (6.67 frames a tick) and the file written at the end, with `play: sound tick T
event E sample S` printed for each start; `--device` plays a headless run through the device
(faster than real time: only to see that one opens).
- The interface: `UI_accept` when a menu opens and when an entry is chosen, `UI_move` when the
  highlight moves (a handle each, restarted in place; `x = volume = 1`).
- The music: `loadloop` from the start for 5 s (chosen: the sandbox has no load screen). A stream
  is decoded once and played as a voice per channel (two for a stereo stream, hard left and right)
  from a double buffer in the top 256 KB of the sample memory, a half refilled when the voice
  leaves it, as the original refills its stream buffers; its level is -6 dB (chosen).

**The params rule** (`sound_params.rs`; the answer key is home-full-s.log's 411 params calls,
words 1 `x` and 2 `volume`, against the object's place at the request and the camera's 'c' record
of the call's tick):
- A track whose flags word (the `snd::Track`'s word 0) has bit 15 clear (`0800028A` the interface's,
  `0800029A` the `dialog_*` voices) is not in the world: `x = volume = 1`. The others
  (`0800828A`, the footsteps' `0800800A`) are heard from the emitter's place.
- A sound out of range has `x = volume = 1/1024`: the first call of every object sound in the log
  but one, and the last of a fade. A sound that is stopped ends with 0, 0.
- In range, `volume = 1 - d / range`, `d` the distance from the camera's eye (`Camera::eye`,
  not its target) to the emitter, and `x` the offset along the camera's right-hand direction over
  20 tiles. The range is the track's parameter word's low byte (`64001040`: 0x40, every track
  but one; `vox_meal_good` has `640010C8`) times 0.22 tiles. Fitted, not read from the game: the
  0.22 and the 20 come from `vox_meal_good` (0.454 and 0.328 at 24.0 tiles and 6.6 tiles of offset: the
  one exact place with a nearby sound); that every 0x40 track is at the floor at 19.6 tiles and
  more (the chops, the door, the chair at 24.0, the kitchen at 27.4) bounds the range from above;
  the footsteps' 0.3 at about 10 tiles from the eye (the walker's place is only known to a few
  tiles) fits `1 - 10 / 14.08`. The eye matters: at the default zoom (17.7 tiles) the middle of the
  view is out of range of an ordinary track, so in the log the kitchen is silent; zoomed in (8
  tiles) it is not.
- Figure (home-full-s.log): of 411 params calls, **213 are of a sound with a known emitter** (the
  interface's, the dialog voices, the objects' after the 57 starts matched), and **191 of them
  are the rule's exactly or within 0.005** (188 exact, 3 within: `vox_meal_good`'s first call, 0.452 for
  0.454 and 0.333 for 0.328); 22 miss, all the lamp sound's (the five lamps share one handle and
  the log's params of it follow a lamp near the camera, which of the five is not known: the rule
  gives the first lamp's place, 25 tiles away). The other 198 are the footsteps' and the dog's
  (no native emitter place: the walker's follower place is in the log's 'Z' records only to the
  tile and the Sim is not matched in them). `play` sends a sound's params each frame its values
  change (the original's change every frame the camera moves).
- Not modelled: the original's slew (its volume moves by 7 to 11 over 1024 a frame as the camera
  does, which the rule gets from the camera's own motion), the pan law's use of `|x|` (so a sound
  dead centre is hard to one side: `sound_player.rs`'s fit), `z` (objects are at height 0).

**Deliberate differences** (for the lockstep and replay checks to skip): a sound starts at once
(the original's object sounds start about 1.15 s after the request); the variant is drawn from the
presentation's numbers, not the game's; a sound's handle is the front's own (the original's are
the sound layer's pool); KillSounds ends the sounds the object asked.

**What is missing.** Footsteps: the walk's foot posts (`Snd_Post`, `footstep_*`, the floor's event
table at 802A8E80) come from the route follower's clock, which `Alone::motions` does not give
(its `Walking(time)` is the stand-in's walk with no foot notes), so the run has none. The
stereo's stations (`sbo_<station>1-5`): the stereo's state (the tuned station, on or off) is not
an event the run sees (`radio_next_song` and `piano_play` are command events with no sample),
so nothing starts them. The pan law's rounding (`docs/assets.md`, "The voices") is still the fit.

## Menus and HUD (Apt)

`flashes.arc` (61 entries) holds the menus' and HUD's movies, `fonts.arc` (6) their text fonts.
Read natively by `engine/src/apt/` (the player is in "The player (U2a)" below); `assets ISO apt` reads all 67 and
ends with `apt: movies 61, fonts 6, unknown 0`, where *unknown* counts every byte nothing read,
every character kind, item kind, opcode, constant type, geometry record and texture header
this reader has no meaning for (`Flash::unknown`, `Font::read` failing). `assets ISO apt NAME`
lists one movie (its characters with their shapes, each frame's items, each action's
instructions with the constant pool's strings in place) or one font. The formats are those of
the PC games' Apt (the open-source OpenSAGE project reads them) except that these are
big-endian, with binary geometry, and the memory-image layout below.

### Bundle (a `flashes.arc` entry)

| Offset | |
|---|---|
| 0 | `BIGF` |
| 4 | the whole size, little-endian |
| 8 | the file count, big-endian |
| 12 | the header's size (16 + the entries) |
| 16 | per file: offset, size (32 bits each), the NUL-ended name |

Zero bytes pad the header and each file to a word. A bundle has `NAME.apt`, `NAME.const`,
`NAME.geo`, one `NNN.tgq` for each image character (793 in all), and a placeholder `*.wav` (at the
bundle's end, size 0xFFFFFFFF: no file).

### The constants (`NAME.const`)

| Offset | |
|---|---|
| 0 | `Apt constant file`, 0x1a, 2 zero bytes |
| 0x14 | where the movie is in the `.apt` |
| 0x18 | the count, then at 0x1c the table's offset (always 0x20) |
| 0x20 | per constant: type, value (32 bits each); the strings follow, NUL-ended, word-padded |

Types: 1 a string (the value is its offset in this file; 9,126), 3 undefined (232, value 0), 4 a
register (10, value 0), 5 a boolean (886), 6 a float (1,915, the bits), 7 an integer (3,932), 8 a
constant-pool index (27,760). The first constants of a movie are the strings of its first
`ConstantPool`; every later one is one value of one `Push`, in the order the code has them.

### The movie (`NAME.apt`)

A memory image: records point to each other by file offset (from the file's start); an array is
a count and a pointer (an empty one points at whatever comes next); a string is NUL-ended and
word-padded; every record starts with its kind word and the signature 0x09876543 (items have no
signature). The file starts with `Apt Data`, 0x1a and 3 zero bytes (12 bytes); the movie is where
the `.const` says (not always right after: six movies keep their exports first).

| Movie, word | |
|---|---|
| 0, 1 | 9, the signature |
| 2, 3 | the frame count, the frames' pointer |
| 4 | 0 |
| 5, 6 | the character count, the characters' pointer (an array of record pointers) |
| 7, 8, 9 | the stage width, height (640 x 448; 550 x 400 in the five `f_*`; 16 x 16 in `bt_*`), the milliseconds a frame lasts (27, 32, 33, 41, 50 or 83) |
| 10, 11 | the import count (always 0), a pointer |
| 12, 13 | the export count, the pointer: pairs of (name pointer, character id), 65 in all: `buildSim` 2, `dialog` 1, `muDialog` 7, `options` 41, `p_goals` 6, `trans` 8 |
| 14 | 0 |

Character 0 is the movie. A frame is (item count, pointer to the item pointers); a sprite's
and the movie's frames are alike.

| Character | Words after kind and signature |
|---|---|
| 1 shape (803) | bounds (4 floats: left, top, right, bottom), the geometry id |
| 2 text (2) | bounds, font character id, alignment, colour 0xRRGGBBAA, height (float), read only, multiline, word wrap (3 words), initial text pointer, variable pointer |
| 3 font (1) | name pointer (a `fonts.arc` entry), 0, 0 |
| 4 button (2) | is-menu word, bounds, triangle count, vertex count, vertices (2 floats), triangles (3 half words each), record count, records, action count (0 in both), actions pointer (an empty array), 0 |
| 5 sprite (1,530) | frame count, frames pointer, 0 |
| 7 image (793) | the image id (the `NNN.tgq`) |
| 9 movie (61) | only character 0 |

A button record is 17 words: flags, character, depth, 2 by 2 matrix (4 floats), translation (2),
colour multiplier (4 floats), colour offset (4 floats).

| Item (a pointer in the frame's array) | Words after the kind |
|---|---|
| 1 action (1,183) | the instruction stream's pointer |
| 2 frame label (579) | name pointer, flags (always 7), frame number |
| 3 place object (19,235), 16 words | flags, depth, character id (-1 for none), matrix a, b, c, d (4 floats), translation (2), colour multiplier 0xRRGGBBAA, colour offset 0x00RRGGBB, ratio (float), name pointer, clip depth (-1 for none), clip actions pointer |
| 4 remove object (1,123) | depth |
| 5 background colour (61) | 0xRRGGBBAA |

Place flags: 1 move, 2 has character, 4 has matrix, 8 has colour (the multiplier and offset), 0x10
has ratio, 0x20 has name, 0x40 has clip depth, 0x80 has clip actions. A field its flag does not
name holds its default (matrix identity, colour 0xFFFFFFFF, the rest 0 or -1). Clip actions (4
items name them, one with any): a pointer to (count, pointer to records of event flags, key
code, stream pointer); the one set has 2 records with events 0x40 and 0x80 and key 0.

### Actions

Flash's ActionScript bytecode. A stream is opcode bytes one after another up to a 0 (then zero
bytes to the word); an opcode of 0x80 or more has operand words, which start on the next word
boundary after it. 47 different opcodes are used (counts over all movies):

- no operands: Add2 1,379, CallFunction 164, CallMethod 1,912, CloneSprite 6, Decrement 51,
  DefineLocal 450, DefineLocal2 82, Delete 135, Divide 738, Enumerate2 5, Equals2 841,
  GetMember 5,835, GetUrl2 77 (no operand word here, not even a flags byte), GetVariable 12,735,
  Greater 239, Increment 154, InitArray 65, InitObject 37, Less2 176, Modulo 2, Multiply 406,
  NewObject 5, Not 1,617, Play 112, Pop 2,261, PushDuplicate 197, RandomNumber 2, RemoveSprite 4,
  Return 176, SetMember 3,405, SetVariable 2,046, Stop 636, StrictEquals 64, StringAdd 39,
  StringEquals 11, Subtract 588, ToNumber 321, Trace 5;
- 0x81 GotoFrame (175): the frame; 0x9f GotoFrame2 (16): a flags word (always 0); 0x87
  SetRegister (10): the register (always 0); 0x94 With (1): the body's size in bytes (the body
  follows in the same stream);
- 0x88 ConstantPool (407) and 0x96 Push (25,766): a count and a pointer to that many constant
  indices (the `.const`'s table: each gives the value's type and value);
- 0x99 BranchAlways (614) and 0x9d BranchIfTrue (1,627): a signed byte offset from the end of the
  instruction (landing on an instruction's first byte, or the stream's End);
- 0x9b DefineFunction (991): name pointer (0 for none), parameter count, pointer to that many
  string pointers (0 in every one), the body's size, the words 0x98765432 and 0x12345678, then
  the body's bytes in line (decoded as its own list; a branch in it stays in it).

The decoder is `apt::actions::decode`; all 66,585 instructions of the 61 movies decode with every
branch landing on an instruction. The movies call into the engine through `_root`, `extern`,
`controller` and the like by name; what those names do is the player's (U2's) business.

### Shapes (`NAME.geo`)

| Offset | |
|---|---|
| 0 | `Apt PCGL geometry file`, 0x1a, 0 (24 bytes) |
| 0x18 | the shape count; 0x1c the table's offset (0x20) |
| table | per shape (12 bytes): the geometry id, the fill count, a pointer to the fill pointers |

A fill record is (kind, count, vertices' pointer, colour 0xRRGGBBAA, ...): kind 1 solid
(5 fills): `count` triangles; kind 5 textured (840): the same, then a 4 by 4 matrix (16 floats,
translation in the last row's first two) and the image id and a 0: a vertex times the matrix is
the point in the image, in texels (checked: all 840 stay inside their images); kind 6 lines (1):
`count` segments. Vertices are 2 floats each, 3 (or 2) per triangle (segment). 803 shapes,
846 fills, 1,777 triangles.

### Images (`NNN.tgq`)

Little-endian words, unlike the rest: `TXFL`, 16 zero bytes, width and height (16 bits each,
powers of two), the bytes 00 01 00 00 00 02 08 20 (256 colours, 8 bits a texel, 32 bits a
palette entry), 4 zero bytes (the header is 36 bytes), then width * height palette indices in
rows from the top (checked by drawing a ring), 256 palette entries of R, G, B, A, then 8 bytes: the
picture's own width and height as 32-bit words (it sits in the top left of the stored size, as
the 8 by 48 rectangles in 8 by 64 images). Every file is exactly 44 + width * height + 1,024
bytes. 793 images, 2,226,048 texels. The text's pictures are not here but in `textures.arc`
(below).

### Fonts (`fonts.arc`)

An entry is the name, a NUL, then big-endian words that are *not* word-aligned (they start
right after the name's NUL):

| | |
|---|---|
| 3 words | 0, 1 for Latin fonts and 0 for CJK, the record count (pages and glyphs, 96 in `systemfont`) |
| 36 bytes | a signature, the same in all six except byte 29, the page count |
| 6 words | the code count N, the text size (12, or 24), a second size (21, or 37, not placed), 1, the highest code plus one (0x7f, 0xff, 0xfff, 0x7ff), N again |
| N pairs | (character code, glyph number), glyph numbers rising |
| 1 + P words | the page count P, then the page numbers 2 to P + 1 |
| records | each a 0xb7 byte and 3 words: P page records (the CRC-32 of the page texture's name in capitals, its width, height), then glyph records (x0, x1 on the page, a word: the page in its low half, a row in its high) |
| 6 words | the tail: 55, -2, 67, 30, 1, 0 in `systemfont`; 44, 3, 0, 17, 1, 0 in the others (not placed) |

The page textures are `textures.arc` entries named `font_NAME_sSIZE_pN`: all 34 page hashes match
(`systemfont` 1 page of 256 by 128, `frankfur` and `onestroke_script` 1 of 512 by 512 and 512 by
256, `china` 15, `korea` 6, `japan` 10, 512 by 512). A code's glyph is `glyphs[number - pages -
2]` (checked in the Latin fonts: 'I' is narrower than 'A'); the last glyph record has no code.
Counts: 94, 206, 173, 2,938, 1,359 and 1,995 codes; 6,771 glyph records in all. The vertical
metrics (the glyph's height and its offsets) are not in the entry: `shaders.arc` has a small
`font_NAME_sSIZE_pN` entry for each page (34 in all) that was not read; they may be what the
text draw needs besides these.

### The player (U2a)

`engine/src/apt/player.rs` (clips and timelines), `vm.rs` (the script machine), `value.rs`,
`display.rs` (the display list), `host.rs` (the game's side), `play.rs` (the driver over a log).
No drawing and no GPU: `Player::display_list()` is what the drawing (U2b) takes. The game drives
it as it drives its movies: `load(path)` (the first load is the root, the wrapper; the others
fill the clip a movie asked for), `call(name, args)`, `advance()` (one movie frame),
`set_extern(name, value)` (what the game gives the movies), `take_commands()` (what the movies
sent back: `(command, argument)` text). `assets ISO apt-play LOG [--to FRAME] [--list] [--diff]
[--probe CLIP.PATH] [--rate R]` runs it over a log's loads and calls and prints
`apt-play: commands N, the same S, first different F (frame, ours, the log's)`; `--list` prints
the display list at the end, `--diff` each command as the same, missing or extra, `--probe` a
clip's frame and variables; `APT_TRACE=1` (`=2` with each variable set) prints what ran.

**Figure** (home run, `ui-dump.log` and `ui-3000.log` give the same, the first 1,400 frames):
commands 244, the same 228, first different frame 741. All but one of the 228 are sent in the
very frame the game logged them (the other is a frame early). What is left, and why:

- the 10 menu item places `button0..3_x` and `button3_y` (twice): the game's values have a last
  digit more than ours (0.499688 against 0.499687; the sums 320 + -0.2 and 121.5 + 119.75 in
  single or double precision both print the lower one). Not found; the logic is the same;
- 3 key presses (`selected_item` at 842 and 866, `menu_command` at 1100): the game sends keys to
  the movies by its own path, not logged; the player has no input (`Key.getCode` is 0, the clips'
  key events 0x40 and 0x80 do not run);
- `done_loading` (the load bar's last animation frame says it): ours comes one frame before the
  game's, so the second screen's copy at the game's frame is not ours; `dialog_button_accept_x`
  at 1109 differs in the sixth decimal (a single against double precision sum, as the menu items'); one `hud_draw_info_text` at
  1336 (the game's HUD says it once more after its dialog is gone).

**The rate.** One movie frame for each game frame, in every movie whatever its own
milliseconds (27 to 83; 33 in all the menus): the dialog says `dialog_ready` in its frame 5, 7
game frames after its load, which is 5 frames and the 2 below. Measured over all 244 commands
(`frame offsets` line of `apt-play`). Inside a game frame the order is: `advance`, the movies
that start now, the game's loads, the game's values, the game's calls (the log keeps no order
among them; a load is taken as soon as a movie has asked for it, so a call after a load of
the same frame finds the movie, as `loadMainMenu` after `f_menu` does).

**Loads.** A movie asks for another with `loadMovie` (the script's `GetUrl2`: URL and target
clip; an empty URL unloads the target). The request waits until the game logs the load of that
name (case aside); then every clip waiting for it is filled, and the movie starts **2 game
frames after the log's load**, except `f_*` (the functions movies: at once, but what their start
sends reaches the game 2 frames late). A movie the game still has in memory (another clip holds
it) is not logged again: the copy takes 3 movie frames (the running one counting), and loading
an `f_` movie finishes the copies waiting (all fitted to the frames of the commands: `bt_GC`
into the `key` of every button, 4 frames from a button's `createButton` to its first report,
1 when `f_menu` loads in the same frame).

**How a call finds its movie.** `Player::call` looks for a function of that name in the
variables of the root (the wrapper) and then of each clip in it (the loaded movies). The wrapper
has the layers and buttons (`showContentLayer`, `showDialogLayer`, `createButton`,
`showDialog`, `showLoadBar`, `mu_showDialogBox`, `loadFunctions(name)`, which loads `f_NAME` into
`controller`); `f_menu` defines `loadMainMenu`, `unloadMainMenu`, `resetMenu` on `_root`,
`f_create` `showCAS`, `hideCAS`, `f_hud` `showHUD`, `updateHUD`, `setPlayerMode`; the loaded
`mudialog` has `reset`. The wrapper's layers are clips of the root (`load_bar`, `content`,
`controller`, `dialog`, `mu_dialog`, `anim`, `keys`, `dpad`, and the hidden `button_template`
that `createButton` duplicates); `showContentLayer` and its kind set a layer's `_visible`.

**Commands.** A movie sends one by setting a member of `extern`: the player sends
`(member, value as text)`, whole numbers as integers, others with six decimals, booleans as
`true`/`false`; the value is not stored (what the movies read from `extern` is only what the
game set). Names with a leading underscore are the wrapper's own (`_debug`, `_f`, `_r`) and not
sent: the game's observer shows none. Every screen the game has up hears every command and the
observer logs each hearing, so a command comes once for each screen that logs any in its frame
(`Host::as_heard`; 2 in the frames where the create or load-bar screen is up beside the
dialog's).

**Where it differs from Flash** (each needed for the game's commands):
the movies are of the SWF 5 generation: `undefined` is 0 as a number and empty as text;
a parameter a call does not pass is not declared, so its name reads and writes the clip's
variable (`createButton("accept")` leaves `orientation` set for every later button);
a frame's script runs before the scripts of the clips the frame places (items in file order);
going to the frame a clip is at does nothing, going back keeps what the same item placed (a
loaded `key` stays loaded); `gotoAndPlay(n)` counts from 1, `GotoFrame` from 0; numbers are
double precision (positions are read from the file's single-precision matrices); the colour in
a place item is 0xAARRGGBB (shadows `0xFF000000`, fades `0xCCFFFFFF`), not the
0xRRGGBBAA of the reader's note; `GetUrl2` is `loadMovie` (no `FSCommand:` target is used);
`Math.random` and `random` use a fixed sequence (the simulation stays reproducible).

**The game's side (`host.rs`, `Player::set_reads`).** What the game gives the movies is what
they read of it by name (`extern.dialog_x`, the button places, `current_load`, the HUD's `ra`,
`ma`, `ct`...): the observer logs each read and the text it returned (`MovieReadRec`, the
`'` record). The player answers a movie's read at game frame `f` with the answer the game logged
for that name at `f` (the last of them), and with nothing (undefined) when none is logged there:
the game keeps no value past the call that needs it (a button's place, once the button is
destroyed, reads as 0 again, as the log's first set of reports shows). Nothing is fitted: the
fitted tables of U2a (the menu's item counts, the dialogs' sizes and places, `current_load`,
the buttons' places taken from the commands the movies sent) are gone, and the figure did not
move (228 of 244). Values are text, as the game returns them. Two answers differ from a time
rule that was tried first: taking the last answer logged *up to* the frame (any age) gives 226,
because a button's old place lingers; taking the *k*th answer of a name for the *k*th read gives
218 (our movies read a value a different number of times). What stays the game's and is not
logged: which screens are up, so a command comes once for each screen that logs any in its
frame (`Host::as_heard`; 2 in the frames where the create or load-bar screen is up beside the
dialog's).


**Not done.** Key and mouse input and the clips' event handlers; text is not laid out here (the
display list gives each text field's string: a script's `text`, the variable it is bound to, or
its own initial text; the glyphs and the dialogs' words, which are the game's, are drawn in
"Drawing the movies (U2b)"); `hitTest` is false; `getBounds`,
`createEmptyMovieClip`, sounds are not there; a clip's `_width` and `_height` are its shapes'
and texts' bounds (not the glyphs'); `With` is done. Every call or member the player does not
know is listed once by `apt-play` as `stub:` (here: `toggleKey` on a clip that does not exist yet,
`reset`/`update`/`init`/`gotoAndStop`/`forceUpdate` on clips or `undefined`, `positionOfButtons`
(a method the menu names wrongly), `setPlayerMode` on the HUD before it is there): the movies
call them and the original does nothing too.

**The display list** (`display_list()`): shapes and texts back to front (a clip's children by
depth), each with its movie, character id, the matrix to the stage (a, b, c, d, tx, ty as the
Flash format has it), the colour transform (multiplier and offset, 0 to 1, from the clip's
place item and `_alpha`, composed down the tree), the string of a text, its depth and its clip
depth (a masking layer's end) and its `layer` (the name of the root's child it is under). Hidden clips (`_visible`) and clips not in the tree are left
out; buttons and images are not items (the wrapper's buttons are hit areas only; a button's up state
is not drawn yet).

### Drawing the movies (U2b)

`engine/render`: `apt_pass.rs` (the GPU pass), `apt_draw.rs` (the display list made into
triangles, the glyph layout), `bin/ui-check.rs` (the check against the game's frames). The engine
crate stays GPU-free; the player gives the display list and the drawing takes it.

**The pass.** One pipeline, triangles with a position in clip space (the drawing scales the
640 by 448 stage to the picture, so any size works: ui-check renders at 640 by 448 only to
compare), a texture coordinate, the fill's colour (the tint), the clip's colour transform
(multiplier and offset, from the display list) and a flag for premultiplied textures. Fragment:
`clamp(texel * tint * mul + add)`, then premultiplied by its alpha and blended with `one, one
minus source alpha` over the picture (straight alpha blending; the picture's own alpha is kept).
A solid fill uses a white 1 by 1 texture. Textures are uploaded once, each when first needed:
an image of a movie by (movie, id) from its 8-bit palette; a font page by its hash. The
textured fill's 4 by 4 matrix is used as the notes above give it (a vertex row times it, in
texels; divided by the stored size, not the picture's). Images and boxes are filtered bilinearly,
font pages by the nearest texel (the font's strokes are 2 pixels wide and the game's are not
blurred: 90.0% against 89.5% within 4 on the four dialog frames).

**What the movies do not draw, and what the game draws.** The dialogs', menus' and HUD's words
are not in the movies (they hold 2 text characters, in `buildSim`): the game draws them itself
with `ERFont::DoDraw` from its own string tables, one line at a time, and the observer logs each
call (`TextDrawRec`, the 0x80 record; `game_text.rs` makes them into lines for the pass, the
`HF_UI_TEXTS` file and its per-line fits are gone). What a record gives and how it is used:

- **font**: the hash in the font object's second word is the CRC-32 of the font's name
  (`67f664fb` is `FRANKFUR`, the one font drawn in the dump); the third word (5, 9, 14, 22)
  changes between screens and is not the size (not placed);
- **place**: `x`, `y` are the line's top left as fractions of the 640 by 448 screen, used as
  logged (plus the 2 pixels below);
- **the size words**: the four floats at the size word are two points. The first is the second
  moved by the shadow's offset (+0.0031, +0.0045 = 2 pixels each way). The second is the point
  the game aligned the line at: the line is centred on its x when the draw's x is left of it
  (`0.5` for the dialog's lines, a button's own x for `YES`) and has its middle on its y when
  the draw's y is above it (the buttons' labels, the HUD's clock); a body line has both equal to
  its place (left aligned, top). So the line's width is `2 (x_aligned - x)`: the scale across
  is taken from it (texels of the line by glyph columns, `SPACE` for a space), no per-line fit;
- **the shadow pass**: every line the dialogs draw comes twice (`DrawDs`): first a record with
  size 0 two pixels down and right (black), then the line with the size words. A record with
  size 0 and no other after it is a plain draw (no shadow): the pill's title "Customize A Sim"
  and the copyright lines; it is drawn dark navy;
- **the lag**: the picture of frame N shows what the log's frame N - 2 drew, the text as the
  movies (below).

What is still fitted (named in `game_text.rs` and `apt_draw.rs`, each with its sweep): the scale
down (`CENTRED_HEIGHT` 32.6 texels: the lines that have a middle y give `2 (y_aligned - y)`
over the scale across; the font's own height is 37), the plain draws' scale (0.62) and colour
(21, 17, 50), the 2 pixels the glyphs sit above their logged top (`TOP_OFFSET`), the advance of
a space (`SPACE` 11 texels; the widths of the logged lines give 10.6 to 12), the coverage
gains (1.2 for the glyphs, 3 for the shadow). The rest of the glyph model (a glyph's quad is
its columns by the band, the page stored upside down, nearest texels, advance = columns) is
U2b's. The game's strokes still differ: its glyphs are bolder and crisper and the shadow
blacker than the palette's intensity gives, a faint dark halo shows around the glyphs'
top and left edges, and a long line drifts by up to 1 pixel against ours: advance = columns
is not the game's (a per-glyph advance, not in the font's records read; paddings of 1 to 3
texels, other space widths, a bigger quad, whole-pixel quads, an outline of eight black copies,
the second palette, squared coverage, bilinear pages were each tried and fit less or the same).
Nor do the pictures show the wrapper's `keys` layer: the movies report where their buttons are
(the `*_button_*_x/_y` commands) and the player draws the button clips (the pills with the A
and B pictures) and the D-pad as the movies place them, as the game does.

**Glyphs.** A font page is a 4-bit paletted picture, 16 colours, kept as two IA8 palettes of
16 (the first half and the second of the entry's palette). The first has intensity = alpha at
each index (0, 1, 2, 3, 6, 12, 24, 47, 109, 99, 217, 162, 208, 232, 244, 255: not monotone),
the second has alpha 0 at 8 and 10: the pass uses the first as a white, premultiplied glyph
(the text is white and the black is its own pass, a record of its own: above). The pages are
stored upside down: a glyph's band (`row * line` to `(row + 1) * line` of the page, line 37 for
the 24-point fonts) is drawn with its last row on top. A glyph's quad is its columns (`x0..x1`)
and the whole band, placed by the line's top left; the advance is the columns' width; a space
(no glyph) is `SPACE` texels. One line is one record (the game breaks lines itself: the dialog's
body is a record a line). The page texel is 0.58 pixels of the stage in the dialogs' text, taken
per line from its aligned width (0.55 to 0.75 over the lines of the dump, the buttons' and the
HUD's smaller, the chooser's title bar bigger).

**The colour offset is not used.** A place item has a colour multiplier (0xAARRGGBB) and an
offset; the game's draw uses the multiplier only. The HUD's pills are placed with a multiplier
of (0.65, 0.65, 1) and a blue offset of 254 (it would make them white-blue); the game shows
the picture's blue (frame 1390 from 55% to 89% within 4 and 1300 from 84% to 88% on that change alone).

**The pictures lag the log by 2 frames** (`HF_UI_LAG`, default 2): the picture of frame N is the
movies' state, and the text, of the log's frame N - 2 (the load bar's frame 1194: 100% within 4
at 2, 41% at 1, 30% at 0; the "Loading from" lines the log still has at 1194 are not in the
picture of 1194). It is one lag for everything the game draws, text and movies alike (the text records have no
movie start in them: it is the capture's, not the player's 2-frame movie start; unverified
against the capture's own timing).

**Dialog boxes.** The box is the dialog movie's own shapes stretched by the `extern` sizes the
game sets before `showDialog` (`dialog_box_width`, `dialog_box_height`, `dialog_x`,
`dialog_y`, `dialog_title_bar_width`; the mu dialog's `mu_dialog_*`). They are the game's own
numbers, read from the log (`MovieReadRec`): the pictures and the commands now use the same
ones (U2b's `PICTURE` table, fitted to the pictures, gave 0.60028 and 0.55012 for the sizes the
game reads as 0.6 and 0.55: the game's numbers were right).

**The check** (`ui-check LOG ISO FRAME...`, `HF_FRAMES` the folder with `ui/`; `HF_UI_OUT` the
output folder, default `$HF_FRAMES/ui-out`; `HF_UI_HIST=1` a histogram of the errors). The 2D
layer is the pixels where the game's frame differs from its world picture by more than 4 in a
channel; ours is drawn over the world picture. Per frame it prints the layer, the share within
4, and `over`: the pixels we draw where the game's frame is the world picture (over-drawing;
small is good). The portrait box of frames 1300 and later is masked. The dump was recorded
with the port's own interface off, so the world pictures are the game's (the HUD frame 1390's
is no longer black and is scored; 760, the title, still is not: its logo and gradient are not
movies and the frame is not listed here).

**Figures** (`ui-check` on `ui-dump.log`, 640 by 448, within 4 of the game's frame on the 2D layer):

| frame | what | layer pixels | within 4 | over |
|---|---|---|---|---|
| 930 | "Load save data?" dialog | 87,241 | 96.09% | 686 |
| 978 | "Please wait" box | 137,683 | 94.74% | 124 |
| 1050 | Sim chooser: pill, icons, arrows, D-pad, buttons | 33,103 | 87.48% | 7 |
| 1122 | "Are you sure?" dialog | 102,381 | 92.07% | 11 |
| 1194 | load bar | 3,736 | 100.00% | 2 |
| 1300 | Mom's dialog | 80,711 | 88.45% | 18 |
| 1390 | HUD | 9,211 | 90.49% | 2 |

Total over the 7 frames: 454,066 layer pixels, **92.71%** within 4 (U2b's figure on the older
dump was 91.30% over 6 frames; the same measure on this dump before the change was 78.76%),
`over` 850 (the aim of 95% is not reached). What is left, by size: (1) the glyphs, about 3/4
of the misses (above: the stroke model, the advance, the game's shadow); (2) 1050's chooser
and 1390's HUD are drawn from the movies' own pieces and match but for the title text and small
icon shades; (3) the gradients and box edges of U2b (a row or two at the bottom shadow of each
box); (4) the 686 `over` pixels of 930 (not looked into).
Not drawn: line fills (`Fill::Lines`), clip layers (`clip_depth`), the movies' own text
characters (2, in `buildSim`).

### The game's feed (U3)

`engine/src/hud.rs` (`Hud`, one per player; `PlayerView` the player's Sim, clock and funds) makes
what the game gives the HUD's and the dialog's movies each presentation frame; the study is
`LOCKSTEP_HUD=1 lockstep LOG ISO --card CARD` (`bin/lockstep/hud.rs`: the replay's world frame by
frame against the log's calls, reads and text records; the log gives it only the frames' ticks,
the movies' commands and the speed setting, which the tracks' speeds stand for). On ui-play.log
(1,317 frames in play): calls 0 differences, read answers 3, text strings 0, text places 564 (U4 answers the three reads with measures fitted to Mom's dialog and draws the title and label at their measured sizes: reads 0, text places 284; "In the sandbox (U4)").

Where the game computes each value:

- **Calls.** `showHUD` (0x80087F08) and `setPlayerMode("1p")` (0x8008800C) when play begins;
  `showDialog` / `hideDialog` with the Dialog node opening and being answered; the speed buttons
  in `fn_80088890` (0x80088E78 and its neighbours): on a change of the setting (the manager's
  +76: -2 fast, -3 fastest; its pause tests) the other two buttons are hidden and the new one
  shown (normal: all three hidden), a frame after the animations take the setting;
  `updateHUD("1", "updateMoodMeter")` at 0x8008893C when the player's mood as a whole number
  differs from the one the HUD last showed (every frame; the HUD keeps the one it began with).
  The layer calls come in the draw (`fn_80089B44`): `hideDialogLayer showContentLayer
  showDialogLayer` (0x80089D7C) every frame once the movie has sent `hud_ready`, `hideContentLayer
  showDialogLayer showContentLayer` (0x8008A28C) every frame a dialog is up.
- **Reads** (`Screen_Read`, 0x8008BFDC the HUD, 0x800445EC the dialog; a name the handler does
  not own answers ""). `om`: entry 3 of the player's Sim's motive table (a float, cut to an
  integer; the engine's own motives give 19, 18, 17, ... as the log has them). `ma`: "true" when
  one of eight motives (the table's entries 7, 8, 5, 14, 6, 9, 15, 13: hunger, hygiene, energy,
  social, comfort, bladder, fun, room) as a share of -100..100 (`fn_800B5350`) is at most 0.05
  (a float at 0x802A4298), else "false". `ra`: relationship alert, true when a relationship of
  the player's Sim has a value below -50 (the scan over the lot's persons, `fn_800B830C` and
  `fn_800B80CC`; not native, always "false", as the log). `ct`: skill gain, true when one of the
  six skills (the Sim's entries 10 to 15 divided by 100) rose since the last read (not native,
  "false"). `two_player` "0"; the dialog's `dialog_x`, `dialog_y`, `dialog_box_width` are doubles at
  0x80298FF0, 0x80298FF8, 0x80298FE0 (0.2, 0.18, 0.6), `number_of_buttons` and
  `is_critical_dialog` the dialog's own. Read but not made: `dialog_box_height` (the text block's
  size from `fn_80044174` plus 0.1), `dialog_title_bar_width` (`ERFont::DoGetStringSize` of the
  title at `SetSize(20, 1)`) and `UI_button_width` (`fn_800441C0`), all of the font's own
  measure.
- **Text.** The funds "$ 1,030": the lot's global 13 less what the Sims spent (AlterBudget,
  `World::funds_spent`; the home lot's global 13 is 1,030 and the log's text turns to 1,025 at the
  fridge's 5, tick 931; unverified that the game reads this global: its getter, the manager's
  slot 200, was not followed); the clock "H:MM" in 12 hours (`fn_802066F4`: hour % 12, 12 for 0,
  PM from 12) and "AM"/"PM": the clock is the world's (hour global 0, minute global 5), shown as
  it stands after the tick. Their places: the aligned point is the movies' bar (`money_bar_x/y`,
  `time_bar_x/y`), the clock 0.048 left of the time bar and "AM" 0.016 right of it (floats at
  0x802AABC0 and 0x802AABC4), left aligned; the money centred on its bar; all centred on y by the
  text height 0.0355 (0x802A3764, within 4e-5 of what the log's lines give; unverified). The width
  of a HUD line is 13/24 (`SetSize(13, 1)`, a float at 0x802AABB0, over the font's size 24) times
  the glyph columns less 2 texels a character (fitted to the two funds strings, 0.03 pixels).
  Each line comes twice, the black pass 2 pixels down and right first. The dialog: its title is
  the Dialog node's operand byte 6 (counted from 1) of the object's STR# 301, its text byte 2, the
  tokens (`$NameLocal:1` the name of the Sim with the frame's local 1) filled as `Alone::dialog_text`
  does; the text is broken into lines by a width (not the game's), centred on the window's middle
  and a pixel (a fit), the line widths by a three-number model solved from Mom's three lines.
  The dialog's texts are drawn from the frame its movie sends `dialog_ready`, the button's label
  ("OK") from `dialog_button_accept_ready`.

What does not reach the log's places: the dialog's title and button label (the font's width at
other `SetSize` sizes) and its second and third lines (the game's line pitch is 19.26 pixels
after the first line and 20.0 after the second: the line's height is not in the font records
read); the first line matches to 1e-4.

### In the sandbox (U4)

The play bin shows the game's own interface and no stand-in (`standin.rs`, the CPU overlay and its
shaders are gone). `engine/src/apt/ui.rs` (`Ui`, GPU-free) runs the movies from the engine's feed;
`engine/render` draws them (`Renderer::interface()` is an `AptPass` over the scene, `play.rs` gives
it the display list of `Ui::display_list()` and the lines of `game_text::native_lines`).

**One frame** (`Ui::step`, one for each presentation frame, the order the log-run has): the
movies advance, the movies' starts and the game's loads (a movie that asked for another with
`loadMovie` is loaded in the frame after its call, in the same frame if the request came from a
movie's own frame: `Player::pending_loads`), the commands the movies sent go to the `Hud`
(`Hud::command`: the places of the bars, `hud_ready`, `dialog_ready`, `dialog_key_press`), the
`Hud`'s calls (update then draw: `showHUD`, `showDialog`, the speed buttons, the mood meter, the
layers), the pause screen's and the menu's calls, and the lines of text come back
(`hud::Text`: top left, shadow pass, the width and height the game measured, plain or not). What
the movies read of the game is answered by the host each frame (`Player::set_answers`): the
HUD's `om ma ra ct two_player`, the dialog's `number_of_buttons dialog_x ...`, and the pause
screen's `pause_mode`, `pause_enable_buy_build`, `title_bar_width`. The start is what the game does
at a lot's start: the wrapper, the front end's `createButton` calls whose stale `orientation`
("center") the dialog's button inherits (without them the dialog's button is placed by a
different rule and the OK pill is 57 pixels right of the game's: found by comparing the commands),
`loadFunctions("hud")`, and `showHUD` (`Ui::begin_play`: the sandbox has no load bar, so 6 frames
in, the dump's 39).

**How input reaches the movies.** The wrapper's `keys` clip has actions for key down (0x40) and
key up (0x80) on its place item; they test `Key.getCode` (37 to 40 the arrows, 88 A, 79 B, 84
the triangle, 83 the square, 306 and the L and R codes) and call `keyPress_*` of the clip the
wrapper's `targetClip` names (`keyPress_x` of the dialog or `p_main` movie). `Player::key(code,
down)` runs the actions of every clip placed with them with `Key.getCode` the code; `Ui::press`
is a down and an up. The game's own path to the movies is not logged (the log has the effects: the
dialog's movie says `extern.dialog_key_press = "accept"` for A, which the `Hud` keeps for the
sandbox to answer the dialog; the pause screen's keys have no command in the log). The pad's A and
B and D-pad, Enter, Esc and the arrows go through it when the dialog or the pause screen has the
focus (`Cmd::Key`); a click on the dialog's pills is A or B (`Ui::dialog_hit`, the places the
movie reported).

**The pause screen** (Space or the pad's Start opens it, Space, Esc or B closes it; `speed` stays
0): `Ui::open_pause` makes the calls of the log's frame 2061 (`hideHUD`, `showPause`, the eight
`drawButton(name, "-1", "-1", ".1")`); the movies load `pause` and `p_main`, `draw_main` is
heard and 2 frames later the two buttons are drawn (`drawButton` SELECT at 0.4, BACK at 0.7, y
0.905); closing is `hidePause`, `destroyButton` of the eight, `showHUD`. The commands the movies
send in these frames are the log's (checked: 2064 to 2066 the same, in the same frames). The
screen's own lines (title "GOALS", the labels, the funds line) are drawn by the game: their
widths are the log's numbers (the title bar's `title_bar_width` 0.102652, the buttons'
`drawButton` widths 0.093525 and 0.070360, the label drawn that wide and centred on the button),
not a measure (the glyph columns do not give them). The title is always the first section's.

**The interaction menu.** Its pill is not a movie's piece and has no movie traffic: the log's
only record of the floor menu is the 0x80 text "Go Here" (a plain draw, no shadow pass, dark
navy, at 0.14, 0.29), and the pill, its A and the text are in the game's *world* picture
(`frames/menu/world_01920.png`, recorded with the port's own interface off) as they are in its
frame: the game draws the pie menu itself in its 3D/2D pass, not through Apt. Its pieces cannot
be sourced from the movies; what the sandbox draws for it stands in: the wrapper's own button
pill (the pause screen's A pill, `drawButton("accept", x, y, w)` in a "menu" context, the same
artwork family) on the highlighted entry of a vertical list of plain navy labels
(`Ui::show_menu`, `menu_hit`: rows 44 pixels apart; the pill reaches 43 pixels left of the label's
middle and 18 right of it as the pause screen's does). A multi-entry pie menu is not in any log,
so the list is a stand-in in layout too.

**The check** (`ui-check LOG ISO --native --card CARD FRAME...`, with `HF_UI_DIR` the frames'
folder under `HF_FRAMES`): `native_pictures` runs a replay of the log's world (`Replay`, as
`LOCKSTEP_HUD` does) and the `Ui` over it; the log gives the frames' ticks and, as a player's
presses, the lot's start (the frames of `loadFunctions` and the first `showHUD`) and the pause's
open and close (`showPause`, `hidePause`); the dialog opens and closes with the world's Dialog
node. `HF_UI_COMMANDS=1` prints the commands the native movies sent (to diff against the log's),
`HF_UI_TEXTS=1` the native lines and the log's records, `HF_UI_LOGREADS=1` answers the reads
from the log. Figures (within 4 on the 2D layer, 640 by 448; native against the log-fed run on
the same frame):

| frame | what | native | log-fed |
|---|---|---|---|
| 1300 (ui-dump) | Mom's dialog | 88.23% | 88.45% |
| 1390 (ui-dump) | HUD | 89.89% | 90.49% |
| 1920 (ui-menu) | the "Go Here" menu | 70.67% | 71.10% |
| 2100 (ui-menu) | the pause screen | 84.34% | 85.07% |

What is left of the native gap: 1920's pill and text are in the world picture (the log-fed run
draws its text over the game's, `over` 585; the native run draws nothing for the menu, `over` 5);
2100's second funds line "$ 45,566" (the family's funds in the pause screen, at the clock's place)
is not drawn: the save's family record of the home (house 1) has 16,727 and 45,551 as its first two
words, 45,551 15 less than the game's number, unverified as the source and not used; the
dialog's title, button and box measures (`dialog_title_bar_width`, `UI_button_width`,
`dialog_box_height`) are fitted to Mom's dialog, the one the logs read in play (named in
`hud.rs`). The commands of 1209 to 1340 match the log's except `wrapper_ready` (before the log's
handler exists), `done_loading` (the load bar's end, not in the sandbox) and a late
`hud_draw_info_text`.

### Left over

Read but not placed: the font's second size word and tail, the glyph row, the last record's
page word; a place object's colour offset and ratio (they fit the flags 8 and 0x10; the ratios
are tiny floats such as 1.0e-4; the colour is 0xAARRGGBB, found by the player); the clip actions' events 0x40 and 0x80 and the key word; the
`GetUrl2` and `GotoFrame2` flags (no operand / always 0: the player found `GetUrl2` is
`loadMovie` with the URL and the target on the stack, `GotoFrame2` a frame by number or label); the
button's is-menu word and its record words; the `.tgq` header's 00 02 pair. The movies' own
text strings are in the constant pools, not in a table: the HUD's and menus' wording comes from
the game's strings and the text characters' variables (2 text characters only).
