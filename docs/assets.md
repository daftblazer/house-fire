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
