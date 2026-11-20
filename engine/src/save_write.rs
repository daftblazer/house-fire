//! The writers of the engine's saved data: the inverse of `save.rs`'s readers (`docs/lots.md`,
//! "Saves"). A neighbourhood file's blocks are packed, checksummed and put at their places, and
//! the file goes back into a memory card's block chain.

use crate::iff::FormatError;
use crate::save::{
    CreatedSim, Family, LotArray, LotChunks, LotGlobals, LotObject, LotObjects, LotStringForm, NeighbourTable, NeighbourhoodBlock, NeighbourhoodChunk,
    ObjectType, RelationshipForm, SavedClass, SavedQueueEntry, SavedRelationship, VERSION, chunk_table, memory_card_file_blocks, parse_array,
    parse_created_sim, parse_family, parse_lot_globals, parse_neighbours, parse_ngbh, read_object_types,
};

type Result<T> = std::result::Result<T, FormatError>;

fn err<T>(what: &str) -> Result<T> {
    Err(FormatError(what.to_string()))
}

const BLOCK: usize = 0x1000;
const HEADER: usize = 28;

/// CRC-32 (zlib's: the reflected polynomial 0xEDB88320, initial and final xor 0xFFFFFFFF).
pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = !0u32;
    for p in parts {
        for &b in *p {
            c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    !c
}

const RING: usize = 4096;
const LOOKAHEAD: usize = 18;
const MIN_MATCH: usize = 3;
/// The tree's "no node" (the ring's size; the roots, one for each first byte, follow it).
const NIL: usize = RING;

/// The packer's search tree: one binary tree of the ring's strings for each first byte, ordered
/// by the bytes that follow, and the best match of the string just put in.
struct MatchTree {
    text: Vec<u8>,
    left: Vec<usize>,
    right: Vec<usize>,
    dad: Vec<usize>,
    match_len: usize,
    match_pos: usize,
}

impl MatchTree {
    fn new() -> Self {
        MatchTree {
            text: vec![0; RING + LOOKAHEAD - 1],
            left: vec![NIL; RING + 1],
            right: vec![NIL; RING + 257],
            dad: vec![NIL; RING + 1],
            match_len: 0,
            match_pos: 0,
        }
    }

    /// Puts the string at ring position `r` in its tree; the first longest match met on the way
    /// down is the best one (a later, equal string takes the older one's place in the tree).
    fn insert(&mut self, r: usize) {
        let mut cmp = 1i32;
        let mut p = RING + 1 + self.text[r] as usize;
        self.left[r] = NIL;
        self.right[r] = NIL;
        self.match_len = 0;
        loop {
            if cmp >= 0 {
                if self.right[p] != NIL {
                    p = self.right[p];
                } else {
                    self.right[p] = r;
                    self.dad[r] = p;
                    return;
                }
            } else if self.left[p] != NIL {
                p = self.left[p];
            } else {
                self.left[p] = r;
                self.dad[r] = p;
                return;
            }
            let mut i = 1;
            while i < LOOKAHEAD {
                cmp = self.text[r + i] as i32 - self.text[p + i] as i32;
                if cmp != 0 {
                    break;
                }
                i += 1;
            }
            if i > self.match_len {
                self.match_pos = p;
                self.match_len = i;
                if i >= LOOKAHEAD {
                    break;
                }
            }
        }
        self.dad[r] = self.dad[p];
        self.left[r] = self.left[p];
        self.right[r] = self.right[p];
        let (l, rr) = (self.left[p], self.right[p]);
        self.dad[l] = r;
        self.dad[rr] = r;
        let d = self.dad[p];
        if self.right[d] == p {
            self.right[d] = r;
        } else {
            self.left[d] = r;
        }
        self.dad[p] = NIL;
    }

    fn delete(&mut self, p: usize) {
        if self.dad[p] == NIL {
            return;
        }
        let q;
        if self.right[p] == NIL {
            q = self.left[p];
        } else if self.left[p] == NIL {
            q = self.right[p];
        } else {
            let mut x = self.left[p];
            if self.right[x] != NIL {
                while self.right[x] != NIL {
                    x = self.right[x];
                }
                let (dx, lx) = (self.dad[x], self.left[x]);
                self.right[dx] = lx;
                self.dad[lx] = dx;
                self.left[x] = self.left[p];
                let lp = self.left[p];
                self.dad[lp] = x;
            }
            self.right[x] = self.right[p];
            let rp = self.right[p];
            self.dad[rp] = x;
            q = x;
        }
        self.dad[q] = self.dad[p];
        let d = self.dad[p];
        if self.right[d] == p {
            self.right[d] = q;
        } else {
            self.left[d] = q;
        }
        self.dad[p] = NIL;
    }
}

/// LZSS, the inverse of `unpack_lzss`, as the neighbourhood file's packer makes it (measured:
/// the packed bytes of the cards' blocks, `docs/lots.md`, "Saves"): the classic encoder with a
/// binary tree of the ring's strings (Okumura's), a ring that starts as zeros, matches of 3 to
/// 18 bytes, a match under 3 bytes a literal.
pub fn pack_lzss(data: &[u8]) -> Vec<u8> {
    let mut t = MatchTree::new();
    let mut input = data.iter().copied();
    let (mut s, mut r) = (0usize, RING - LOOKAHEAD);
    let mut len = 0;
    while len < LOOKAHEAD {
        let Some(c) = input.next() else { break };
        t.text[r + len] = c;
        len += 1;
    }
    if len == 0 {
        return Vec::new();
    }
    for i in 1..=LOOKAHEAD {
        t.insert(r - i);
    }
    t.insert(r);
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    let (mut flag_at, mut bit) = (0, 8);
    loop {
        if bit == 8 {
            flag_at = out.len();
            out.push(0);
            bit = 0;
        }
        t.match_len = t.match_len.min(len);
        let step = if t.match_len < MIN_MATCH {
            out[flag_at] |= 1 << bit;
            out.push(t.text[r]);
            1
        } else {
            out.push(t.match_pos as u8);
            out.push((t.match_pos >> 4 & 0xF0) as u8 | (t.match_len - MIN_MATCH) as u8);
            t.match_len
        };
        bit += 1;
        let mut i = 0;
        while i < step {
            let Some(c) = input.next() else { break };
            t.delete(s);
            t.text[s] = c;
            if s < LOOKAHEAD - 1 {
                t.text[s + RING] = c;
            }
            s = (s + 1) & (RING - 1);
            r = (r + 1) & (RING - 1);
            t.insert(r);
            i += 1;
        }
        while i < step {
            t.delete(s);
            s = (s + 1) & (RING - 1);
            r = (r + 1) & (RING - 1);
            len -= 1;
            if len > 0 {
                t.insert(r);
            }
            i += 1;
        }
        if len == 0 {
            break;
        }
    }
    out
}

/// A block as it is put in a file: the 28-byte header (section, 28, unpacked size, packed size,
/// the checksum, the version 4, the count) and the packed bytes. The checksum is the CRC-32 of
/// the header with its own word zero, then the packed bytes.
pub fn encode_block(b: &NeighbourhoodBlock) -> Vec<u8> {
    let packed = pack_lzss(&b.data);
    let word = |v: u32| if b.big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut out = Vec::with_capacity(HEADER + packed.len());
    for v in [b.section, HEADER as u32, b.data.len() as u32, packed.len() as u32, 0, b.version, b.count] {
        out.extend_from_slice(&word(v));
    }
    let crc = crc32(&[&out, &packed]);
    out[16..20].copy_from_slice(&word(crc));
    out.extend_from_slice(&packed);
    out
}

/// A neighbourhood file from its blocks: `template` is the file's bytes where there are no
/// blocks (a save's banner, icons and thumbnail, and the stale bytes the game leaves after each
/// packed block; zeros for a file made from nothing), and each block is put at its offset. The
/// file is the template's size.
pub fn assemble_file(template: &[u8], blocks: &[NeighbourhoodBlock]) -> Result<Vec<u8>> {
    let mut f = template.to_vec();
    let mut end = 0;
    for b in blocks {
        let at = b.offset as usize;
        let e = encode_block(b);
        if at % BLOCK != 0 || at < end || at + e.len() > f.len() {
            return err("a block does not fit its place");
        }
        f[at..at + e.len()].copy_from_slice(&e);
        end = at + e.len();
    }
    Ok(f)
}

/// Puts a file of the size of `game`'s file back into the card's block chain (the directory and
/// the block map as they are); the card with the file replaced.
pub fn replace_card_file(card: &[u8], game: &str, file: &[u8]) -> Result<Vec<u8>> {
    let chain = memory_card_file_blocks(card, game)?;
    const CARD_BLOCK: usize = 0x2000;
    if chain.len() * CARD_BLOCK != file.len() {
        return err("the file is not the size of the card's file");
    }
    let mut out = card.to_vec();
    for (k, &b) in chain.iter().enumerate() {
        out[b * CARD_BLOCK..(b + 1) * CARD_BLOCK].copy_from_slice(&file[k * CARD_BLOCK..(k + 1) * CARD_BLOCK]);
    }
    Ok(out)
}

/// The blocks of a neighbourhood file that the game's sections are (0-3); a save's 0xDEADBEEF
/// thumbnail block is not one, and stays in the template.
pub fn file_blocks(f: &[u8]) -> Result<Vec<NeighbourhoodBlock>> {
    Ok(crate::save::read_neighbourhood(f)?.into_iter().filter(|b| b.section <= 3).collect())
}

/// A block's data from its chunk table (`save::chunk_table`'s inverse): the n FourCCs (reversed
/// on the disc), the n offsets from the block's start in the file (header included), the n sizes,
/// the n 16-bit values, the table padded to 4, then the chunks, each padded to 4.
pub fn encode_chunk_table(t: &LotChunks, big_endian: bool) -> Vec<u8> {
    let n = t.chunks.len();
    let word = |v: usize| if big_endian { (v as u32).to_be_bytes() } else { (v as u32).to_le_bytes() };
    let mut out = Vec::new();
    for (tag, _) in &t.chunks {
        let mut tag = tag.as_bytes().to_vec();
        if !big_endian {
            tag.reverse();
        }
        out.extend_from_slice(&tag);
    }
    let mut at = HEADER + (14 * n + 3) / 4 * 4;
    for (_, c) in &t.chunks {
        out.extend_from_slice(&word(at));
        at += (c.len() + 3) / 4 * 4;
    }
    for (_, c) in &t.chunks {
        out.extend_from_slice(&word(c.len()));
    }
    for &id in &t.ids {
        out.extend_from_slice(&if big_endian { id.to_be_bytes() } else { id.to_le_bytes() });
    }
    let mut padding = t.padding.iter().copied();
    let mut pad = |out: &mut Vec<u8>| {
        while out.len() % 4 != 0 {
            out.push(padding.next().unwrap_or(0));
        }
    };
    pad(&mut out);
    for (_, c) in &t.chunks {
        out.extend_from_slice(c);
        pad(&mut out);
    }
    out
}

/// The save stream's writer (`save::SaveStream`'s inverse): bits from the top of each byte; with
/// compression on, a value is a 0 bit (zero) or a 1 bit, a two-bit width code and the value's
/// low bits at the smallest of the four widths that holds it as a signed number (measured: every
/// value of every lot of the cards and the disc is so packed).
pub struct SaveWriter {
    out: Vec<u8>,
    nbits: u32,
    pub compressed: bool,
}

const WIDTHS8: [u32; 4] = [2, 4, 6, 8];
const WIDTHS16: [u32; 4] = [5, 8, 13, 16];
const WIDTHS32: [u32; 4] = [6, 11, 21, 32];

impl SaveWriter {
    pub fn new() -> Self {
        SaveWriter { out: Vec::new(), nbits: 0, compressed: false }
    }
    fn bits(&mut self, v: u32, n: u32) {
        for k in (0..n).rev() {
            if self.nbits % 8 == 0 {
                self.out.push(0);
            }
            if v >> k & 1 != 0 {
                *self.out.last_mut().unwrap() |= 0x80 >> (self.nbits % 8);
            }
            self.nbits += 1;
        }
    }
    fn packed(&mut self, v: i32, widths: &[u32; 4]) {
        if v == 0 {
            self.bits(0, 1);
            return;
        }
        let code = widths.iter().position(|&w| w >= 32 || (-(1i64 << (w - 1))..1i64 << (w - 1)).contains(&(v as i64))).unwrap_or(3);
        let w = widths[code];
        self.bits(1, 1);
        self.bits(code as u32, 2);
        self.bits(if w == 32 { v as u32 } else { v as u32 & ((1 << w) - 1) }, w);
    }
    fn raw(&mut self, v: u32, n: usize) {
        self.pad();
        self.out.extend_from_slice(&v.to_le_bytes()[..n]);
    }
    pub fn w8(&mut self, v: i32) {
        if self.compressed { self.packed(v as i8 as i32, &WIDTHS8) } else { self.raw(v as u32, 1) }
    }
    pub fn w16(&mut self, v: i32) {
        if self.compressed { self.packed(v as i16 as i32, &WIDTHS16) } else { self.raw(v as u32, 2) }
    }
    pub fn w32(&mut self, v: i32) {
        if self.compressed { self.packed(v, &WIDTHS32) } else { self.raw(v as u32, 4) }
    }
    pub fn wbool(&mut self, v: bool) {
        self.w8(v as i32);
    }
    pub fn wfloat(&mut self, v: f32) {
        self.w32(v.to_bits() as i32);
    }
    /// A plain little-endian word, from the next byte.
    pub fn mark(&mut self, v: u32) {
        self.raw(v, 4);
    }
    /// On to the next byte.
    pub fn pad(&mut self) {
        self.nbits = 0;
    }
    pub fn pos(&self) -> usize {
        self.out.len()
    }
    /// Overwrites a mark written at `at`.
    pub fn patch_mark(&mut self, at: usize, v: u32) {
        self.out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn push_bytes(&mut self, b: &[u8]) {
        self.pad();
        self.out.extend_from_slice(b);
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }
}

impl Default for SaveWriter {
    fn default() -> Self {
        Self::new()
    }
}

fn write_relationship(w: &mut SaveWriter, r: &SavedRelationship) {
    w.w32(r.with);
    w.w32(r.values.len() as i32);
    for &v in &r.values {
        w.w32(v);
    }
}

fn write_relationships(w: &mut SaveWriter, o: &LotObject) {
    match &o.relationship_form {
        RelationshipForm::Counted => {
            w.w32(o.relationships.len() as i32);
            for r in &o.relationships {
                write_relationship(w, r);
            }
        }
        RelationshipForm::Sparse { head, second, flags } => {
            w.w32(*head);
            w.w32(*second);
            w.w32(flags.len() as i32);
            let mut entries = o.relationships.iter();
            for &f in flags {
                w.w32(f);
                if f != 0 && let Some(r) = entries.next() {
                    write_relationship(w, r);
                }
            }
        }
    }
}

fn write_base_rest(w: &mut SaveWriter, o: &LotObject) {
    w.w16(o.before_stack as i32);
    w.w32(o.stack.len() as i32);
    w.w32(o.stack_capacity);
    for f in &o.stack {
        w.w16(f.stack_object as i32);
        w.w16(f.tree as i16 as i32);
        w.w16(f.node as i16 as i32);
        w.w8(f.locals.len() as i32);
        w.w8(f.params.len() as i32);
        for &v in &f.params {
            w.w16(v as i32);
        }
        for &v in &f.locals {
            w.w16(v as i32);
        }
        w.w32(if !f.engine { 0 } else if f.engine_value != 0 { f.engine_value } else { 1 });
        w.w16(f.file as i32);
    }
    write_relationships(w, o);
    w.w16(o.pairs.len() as i32);
    for &(a, b) in &o.pairs {
        w.w16(a as i32);
        w.w16(b as i32);
    }
    w.w32(o.after_pairs);
    w.w16(o.shorts.len() as i32);
    for &v in &o.shorts {
        w.w16(v as i32);
    }
}

fn write_queue_entry(w: &mut SaveWriter, e: &SavedQueueEntry) {
    w.w32(e.a);
    w.w32(e.id);
    for &o in &e.objects {
        w.w16(o as i32);
    }
    w.w32(e.b);
    for &v in &e.shorts {
        w.w16(v as i32);
    }
    w.w32(e.c);
    w.w16(e.tree as i32);
    w.wfloat(e.e);
    w.w32(e.flags);
}

fn write_sim(w: &mut SaveWriter, o: &LotObject) {
    let m = &o.member_1136;
    w.w16(m.pairs.len() as i32);
    for &(flag, v) in &m.pairs {
        w.w8(flag as i32);
        w.w32(v);
    }
    w.w8(m.flag as i32);
    w.wfloat(m.value);
    for &v in &m.ints {
        w.w32(v);
    }
    for &v in &o.motives {
        w.wfloat(v);
    }
    for &v in &o.motives_b {
        w.wfloat(v);
    }
    for &v in &o.person_data {
        w.w16(v as i32);
    }
    w.w32(o.list_1160);
    write_queue_entry(w, &o.current);
    w.w32(o.queue.len() as i32);
    for e in &o.queue {
        write_queue_entry(w, e);
    }
    w.w32(o.list_1180.len() as i32);
    for e in &o.list_1180 {
        w.w16(e.object as i32);
        w.w32(e.value);
        w.w8(e.flag as i32);
    }
    w.w32(o.motive_deltas.len() as i32);
    for d in &o.motive_deltas {
        w.w32(d.motive);
        w.wfloat(d.rate);
        w.wfloat(d.target);
    }
}

fn write_part(w: &mut SaveWriter, o: &LotObject, door: bool) {
    for &v in &o.part_values {
        w.w32(v);
    }
    w.w16(o.leader as i32);
    if door {
        w.w32(o.door_floats.len() as i32);
        for &f in &o.door_floats {
            w.wfloat(f);
        }
    }
}

fn write_object_section(w: &mut SaveWriter, o: &LotObject) {
    w.compressed = true;
    for &v in &o.header[..7] {
        w.w32(v);
    }
    w.w16(o.header[7]);
    w.w16(o.attributes.len() as i32);
    for &v in &o.attributes {
        w.w16(v as i32);
    }
    for &v in o.temps.iter().chain(&o.data) {
        w.w16(v as i32);
    }
    write_base_rest(w, o);
    match o.saved_class {
        SavedClass::Object => {}
        SavedClass::Part => write_part(w, o, false),
        SavedClass::Door => write_part(w, o, true),
        SavedClass::Sim => write_sim(w, o),
    }
    w.w32(o.loader_int);
    w.pad();
}

/// An ObjM chunk from its model (`save::read_lot_objects`'s inverse): the header (0, the
/// version, "MjbO"), the stream's packing byte, the object table, then the mark chain with each
/// object's section and the lot's own.
pub fn encode_lot_objects(l: &LotObjects) -> Vec<u8> {
    let mut w = SaveWriter::new();
    w.compressed = false;
    w.w8(l.compressed as i32);
    w.compressed = l.compressed;
    for o in &l.objects {
        w.w16(o.id as i32);
        w.w32(o.a);
        w.w32(o.b);
        w.w8(o.extra.len() as i32);
        for &v in &o.extra {
            w.w32(v);
        }
        w.w16(o.kind as i32);
    }
    w.w16(0);
    for o in &l.objects {
        let at = w.pos();
        w.mark(0);
        write_object_section(&mut w, o);
        let next = w.pos() as u32;
        w.patch_mark(at, next);
    }
    w.mark(l.last_mark);
    w.compressed = true;
    w.w16(l.lot_object as i32);
    w.w32(l.lot_objects.len() as i32);
    for &o in &l.lot_objects {
        w.w16(o as i32);
    }
    w.pad();
    if w.pos() % 2 == 1 {
        w.push_bytes(&[l.lot_pad]);
    }
    let little = l.lot_string_form == LotStringForm::MarkFeFf;
    match l.lot_string_form {
        LotStringForm::Plain => {}
        LotStringForm::MarkFfFe => w.push_bytes(&[0xFF, 0xFE]),
        LotStringForm::MarkFeFf => w.push_bytes(&[0xFE, 0xFF]),
    }
    for &u in l.lot_string.iter().chain(&[0]) {
        w.push_bytes(&if little { u.to_le_bytes() } else { u.to_be_bytes() });
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(VERSION as u32).to_le_bytes());
    out.extend_from_slice(b"MjbO");
    out.extend_from_slice(&w.into_bytes());
    out
}

/// A run of equal bytes this long or longer is a run in an array's coding, a shorter one is
/// kept in the literal bytes around it (measured: the shortest run in the arrays of the cards
/// and the disc is 8, and no shorter one is ever left out of a literal).
const MIN_RUN: usize = 8;

/// An Arry chunk from its array (`save::parse_array`'s inverse): 0, the width and height, the
/// element size, a 1, then the bytes as runs (a 16-bit header with its top bit set, that many
/// copies of the byte after it, padded to 2) and literals (a header, that many bytes, padded to
/// even), alternating.
pub fn encode_array(a: &LotArray) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [0, a.width, a.height, a.element_size, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let d = &a.bytes;
    let literal = |out: &mut Vec<u8>, bytes: &[u8]| {
        for part in bytes.chunks(0x7FFF) {
            out.extend_from_slice(&(part.len() as u16).to_le_bytes());
            out.extend_from_slice(part);
            if part.len() & 1 != 0 {
                out.push(0);
            }
        }
    };
    let (mut i, mut lit_from) = (0, 0);
    while i < d.len() {
        let mut j = i + 1;
        while j < d.len() && d[j] == d[i] {
            j += 1;
        }
        if j - i >= MIN_RUN {
            literal(&mut out, &d[lit_from..i]);
            let mut left = j - i;
            while left > 0 {
                let n = left.min(0x7FFF);
                out.extend_from_slice(&(0x8000 | n as u16).to_le_bytes());
                out.extend_from_slice(&[d[i], 0]);
                left -= n;
            }
            lit_from = j;
        }
        i = j;
    }
    literal(&mut out, &d[lit_from..]);
    out
}

/// An objt chunk from its types (`save::read_object_types`'s inverse): 0, the version (2), the
/// mark "tjbo", then for each type its GUID, six 16-bit fields and its name, ended by a zero,
/// the entries padded to even, and a zero GUID.
pub fn encode_object_types(types: &[ObjectType]) -> Vec<u8> {
    let mut out = header(2, b"tjbo");
    for t in types {
        out.extend_from_slice(&t.guid.to_le_bytes());
        for f in t.fields {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.extend_from_slice(t.name.as_bytes());
        out.push(0);
        if (out.len() - 12) % 2 != 0 {
            out.push(t.pad);
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn header(version: u32, mark: &[u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(mark);
    out
}

/// A SIMI chunk from a lot's globals.
pub fn encode_lot_globals(g: &LotGlobals) -> Vec<u8> {
    let mut out = header(g.version, b"IMIS");
    out.extend(g.values.iter().flat_map(|v| v.to_le_bytes()));
    out
}

/// An NGBH chunk from the neighbourhood's data.
pub fn encode_ngbh(n: &NeighbourhoodChunk) -> Vec<u8> {
    let mut out = header(n.version, b"HBGN");
    out.extend(n.words.iter().flat_map(|v| v.to_le_bytes()));
    out.extend(n.marks.iter().flat_map(|v| v.to_le_bytes()));
    out.extend_from_slice(&n.tail);
    out
}

/// A FAMI chunk from a family.
pub fn encode_family(f: &Family) -> Vec<u8> {
    let mut out = header(f.version, b"IMAF");
    for v in [f.house, f.id, f.words[0], f.words[1], f.words[2], f.words[3], f.members.len() as i32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for &m in &f.members {
        out.extend_from_slice(&m.to_le_bytes());
    }
    out.extend_from_slice(&f.tail);
    out
}

fn user_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&[0xFF, 0xFE]);
    for u in s.encode_utf16().chain([0]) {
        out.extend_from_slice(&u.to_be_bytes());
    }
}

/// A User chunk from a made Sim.
pub fn encode_created_sim(c: &CreatedSim) -> Vec<u8> {
    let mut out = header(c.version, b"resU");
    out.extend_from_slice(&c.guid.to_le_bytes());
    user_string(&mut out, &c.first_name);
    user_string(&mut out, &c.last_name);
    out.extend_from_slice(&c.looks);
    out.extend_from_slice(&c.rest);
    out
}

/// An NBRS chunk from the neighbour table (`save::parse_neighbours`'s inverse): a slot for each
/// of the table's, empty (a zero word) or a neighbour's record.
pub fn encode_neighbours(t: &NeighbourTable) -> Vec<u8> {
    let mut out = header(t.version, b"SRBN");
    let w32 = |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&v.to_le_bytes());
    w32(&mut out, t.slots as i32);
    for slot in 0..t.slots {
        let Some(n) = t.neighbours.iter().find(|n| n.slot == slot) else {
            w32(&mut out, 0);
            continue;
        };
        w32(&mut out, n.slot_flag);
        w32(&mut out, n.record_version);
        out.extend_from_slice(n.name.as_bytes());
        out.push(0);
        if out.len() & 1 != 0 {
            out.push(n.name_pad);
        }
        w32(&mut out, n.word_a);
        w32(&mut out, n.person_data_flag);
        if n.person_data_flag != 0 {
            for k in 0..80 {
                out.extend_from_slice(&n.person_data.get(k).copied().unwrap_or(0).to_le_bytes());
            }
        }
        // the motives as the original keeps them: each word's bytes big-endian
        for v in n.motives.iter().chain(&n.motives_b) {
            out.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        out.extend_from_slice(&n.id.to_le_bytes());
        w32(&mut out, n.guid as i32);
        w32(&mut out, n.word_b);
        w32(&mut out, n.word_c);
        w32(&mut out, n.relationships.len() as i32);
        for r in &n.relationships {
            w32(&mut out, r.word_d);
            w32(&mut out, r.with as i32);
            w32(&mut out, r.values.len() as i32);
            for &v in &r.values {
                w32(&mut out, v);
            }
        }
    }
    out
}

/// A block's data written back from the models of what the engine reads: the chunk table with
/// each chunk with a reader (the arrays, the object types, the objects, the lots' globals, the
/// neighbourhood's data, the neighbours, the families, the made Sims) encoded from its model and
/// the others (HOUS, TATT, DUMP: not decoded) as the bytes they are; a thumbnail block (section
/// 2) as it is. `objects` reads a lot's ObjM (it needs the library's classes).
pub fn rewrite_block(b: &NeighbourhoodBlock, objects: impl Fn(&LotChunks) -> Result<LotObjects>) -> Result<Vec<u8>> {
    if b.section == 2 {
        return Ok(b.data.clone());
    }
    let mut t = chunk_table(b)?;
    let lot = t.clone();
    for (tag, c) in &mut t.chunks {
        let bytes = match tag.as_str() {
            "Arry" => Some(encode_array(&parse_array(c)?)),
            "objt" => Some(encode_object_types(&read_object_types(c)?)),
            "ObjM" => Some(encode_lot_objects(&objects(&lot)?)),
            "SIMI" => parse_lot_globals(c).map(|g| encode_lot_globals(&g)),
            "NGBH" => Some(encode_ngbh(&parse_ngbh(c)?)),
            "NBRS" => Some(encode_neighbours(&parse_neighbours(c)?)),
            "FAMI" => parse_family(c).map(|f| encode_family(&f)),
            "User" => parse_created_sim(c).map(|s| encode_created_sim(&s)),
            _ => None,
        };
        if let Some(bytes) = bytes {
            *c = bytes;
        }
    }
    Ok(encode_chunk_table(&t, b.big_endian))
}

/// Whether the block at `offset` of a file has the checksum its header states (the header read
/// as `save::read_neighbourhood` does, big-endian in a save).
pub fn verify_block(f: &[u8], offset: usize) -> bool {
    let Some(h) = f.get(offset..offset + HEADER) else { return false };
    let big = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize != HEADER;
    let w = |i: usize| {
        let b = [h[4 * i], h[4 * i + 1], h[4 * i + 2], h[4 * i + 3]];
        if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
    };
    let Some(packed) = f.get(offset + HEADER..offset + HEADER + w(3) as usize) else { return false };
    let mut zeroed = h.to_vec();
    zeroed[16..20].fill(0);
    crc32(&[&zeroed, packed]) == w(4)
}
