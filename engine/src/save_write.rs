//! The writers of the engine's saved data: the inverse of `save.rs`'s readers (`docs/lots.md`,
//! "Saves"). A neighbourhood file's blocks are packed, checksummed and put at their places, and
//! the file goes back into a memory card's block chain.

use crate::iff::FormatError;
use crate::save::{
    LotChunks, LotObject, LotObjects, LotStringForm, NeighbourhoodBlock, RelationshipForm, SavedClass, SavedQueueEntry, SavedRelationship, VERSION,
    memory_card_file_blocks,
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
