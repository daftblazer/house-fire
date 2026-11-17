//! The engine's saved data: the neighbourhood file (Bustin' Out's Runtime/default.ngh), its
//! lots, and the save stream they are written in (`docs/lots.md`; `tools/ngh.py`, `recon.py`,
//! `lot.py` and `objm.py` are the same, in Python).

use crate::iff::FormatError;

type Result<T> = std::result::Result<T, FormatError>;

fn err<T>(what: &str) -> Result<T> {
    Err(FormatError(what.to_string()))
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

const WIDTHS8: [u32; 4] = [2, 4, 6, 8];
const WIDTHS16: [u32; 4] = [5, 8, 13, 16];
const WIDTHS32: [u32; 4] = [6, 11, 21, 32];
const BLOCK: usize = 0x1000;
const HEADER: usize = 28;

/// The save stream (The Sims 2's ReconBuffer): bits from the top of each byte; with compression
/// on, a value is 0 (bit 0) or bit 1, a two-bit width code and that many bits, sign-extended
/// (8-bit 2/4/6/8, 16-bit 5/8/13/16, 32-bit 6/11/21/32); a mark pads to a byte and is a plain
/// 32-bit word. Reads past the end are an error.
pub struct SaveStream<'a> {
    d: &'a [u8],
    pos: usize,
    cur: u8,
    mask: u8,
    pub compressed: bool,
}

impl<'a> SaveStream<'a> {
    pub fn new(d: &'a [u8], pos: usize) -> Self {
        SaveStream { d, pos, cur: 0, mask: 0, compressed: false }
    }
    fn bit(&mut self) -> Result<u32> {
        if self.mask == 0 {
            let Some(&c) = self.d.get(self.pos) else { return err("save stream ends") };
            self.cur = c;
            self.pos += 1;
            self.mask = 0x80;
        }
        let b = (self.cur & self.mask != 0) as u32;
        self.mask >>= 1;
        Ok(b)
    }
    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = v << 1 | self.bit()?;
        }
        Ok(v)
    }
    fn packed(&mut self, widths: &[u32; 4]) -> Result<i32> {
        if self.bit()? == 0 {
            return Ok(0);
        }
        let w = widths[self.bits(2)? as usize];
        let v = self.bits(w)?;
        Ok(if w < 32 && v & (1 << (w - 1)) != 0 { (v as i64 - (1i64 << w)) as i32 } else { v as i32 })
    }
    fn raw(&mut self, n: usize) -> Result<u32> {
        self.mask = 0;
        if self.pos + n > self.d.len() {
            return err("save stream ends");
        }
        let v = (0..n).fold(0u32, |v, i| v | (self.d[self.pos + i] as u32) << (8 * i));
        self.pos += n;
        Ok(v)
    }
    pub fn r8(&mut self) -> Result<i32> {
        if self.compressed { self.packed(&WIDTHS8) } else { Ok(self.raw(1)? as i8 as i32) }
    }
    pub fn r16(&mut self) -> Result<i32> {
        if self.compressed { self.packed(&WIDTHS16) } else { Ok(self.raw(2)? as i16 as i32) }
    }
    pub fn r32(&mut self) -> Result<i32> {
        if self.compressed { self.packed(&WIDTHS32) } else { Ok(self.raw(4)? as i32) }
    }
    pub fn rbool(&mut self) -> Result<bool> {
        Ok(self.r8()? != 0)
    }
    pub fn rfloat(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.r32()? as u32))
    }
    /// A plain little-endian word.
    pub fn mark(&mut self) -> Result<u32> {
        self.raw(4)
    }
    /// On to the next byte.
    pub fn pad(&mut self) {
        self.mask = 0;
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
}

/// LZSS as the neighbourhood file packs its blocks: flag bytes (low bit first, 1 a literal),
/// references of a 12-bit position and a length of 3-18, a 4096-byte ring from 0xFEE. The
/// unpacked bytes, and how many packed bytes they took.
pub fn unpack_lzss(src: &[u8], size: usize) -> Result<(Vec<u8>, usize)> {
    let mut ring = [0u8; 4096];
    let (mut r, mut i, mut flags) = (0xFEEusize, 0usize, 0u32);
    let mut out = Vec::with_capacity(size);
    let put = |out: &mut Vec<u8>, ring: &mut [u8; 4096], c: u8, r: &mut usize| {
        out.push(c);
        ring[*r] = c;
        *r = (*r + 1) & 4095;
    };
    while out.len() < size {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let Some(&f) = src.get(i) else { return err("packed block ends early") };
            flags = f as u32 | 0xFF00;
            i += 1;
        }
        if flags & 1 != 0 {
            let Some(&c) = src.get(i) else { return err("packed block ends early") };
            i += 1;
            put(&mut out, &mut ring, c, &mut r);
        } else {
            if i + 1 >= src.len() {
                return err("packed block ends early");
            }
            let (lo, hi) = (src[i] as usize, src[i + 1] as usize);
            i += 2;
            let pos = lo | (hi & 0xF0) << 4;
            let mut k = 0;
            while k < (hi & 0x0F) + 3 && out.len() < size {
                let c = ring[(pos + k) & 4095];
                put(&mut out, &mut ring, c, &mut r);
                k += 1;
            }
        }
    }
    Ok((out, i))
}

/// One lot of the neighbourhood: its chunks by FourCC (Arry x3, objt, ObjM).
#[derive(Clone, Debug, Default)]
pub struct LotChunks {
    /// Its block's offset in the file.
    pub block: u32,
    pub chunks: Vec<(String, Vec<u8>)>,
}

impl LotChunks {
    pub fn find(&self, kind: &str) -> Option<&[u8]> {
        self.chunks.iter().find(|(t, _)| t == kind).map(|(_, c)| c.as_slice())
    }
}

/// A block of the neighbourhood file: its section, count, unpacked data.
#[derive(Clone, Debug)]
pub struct NeighbourhoodBlock {
    pub offset: u32,
    pub section: u32,
    pub count: u32,
    /// A save's (memory_card_file), not the disc's.
    pub big_endian: bool,
    pub data: Vec<u8>,
}

/// The neighbourhood file's blocks; an error on a block that does not unpack from exactly its
/// stated size.
pub fn read_neighbourhood(f: &[u8]) -> Result<Vec<NeighbourhoodBlock>> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + HEADER <= f.len() {
        // The header's words: the disc's little-endian, a save's big-endian (its size, 28,
        // tells which).
        let big = le32(f, off + 4) as usize != HEADER && be32(f, off + 4) as usize == HEADER;
        let w = |i: usize| if big { be32(f, off + 4 * i) } else { le32(f, off + 4 * i) };
        // The sixth word is the format's version: Bustin' Out's 4, Castaway's 11.
        let (section, hs, un, pk, version) = (w(0), w(1) as usize, w(2) as usize, w(3) as usize, w(5));
        if hs == HEADER && version != 0 && version < 64 && pk != 0 && pk <= un && un < 1 << 24 && off + HEADER + pk <= f.len() {
            let (data, used) = unpack_lzss(&f[off + HEADER..off + HEADER + pk], un)?;
            if used != pk {
                return err("a block does not unpack from exactly its packed size");
            }
            out.push(NeighbourhoodBlock { offset: off as u32, section, count: w(6), big_endian: big, data });
        }
        off += BLOCK;
    }
    Ok(out)
}

/// A block's chunk table (a lot's, the neighbourhood's): n FourCCs (backwards), n offsets from
/// the block's start in the file (header included), n sizes.
pub fn chunk_table(b: &NeighbourhoodBlock) -> Result<LotChunks> {
    let d = &b.data;
    let mut n = 0;
    while 4 * n + 4 <= d.len() && d[4 * n..4 * n + 4].iter().all(u8::is_ascii_alphabetic) {
        n += 1;
    }
    if 12 * n > d.len() {
        return err("a chunk table runs past its block");
    }
    let word = |at: usize| if b.big_endian { be32(d, at) } else { le32(d, at) } as usize;
    let mut lot = LotChunks { block: b.offset, chunks: Vec::with_capacity(n) };
    for k in 0..n {
        let mut tag: Vec<u8> = d[4 * k..4 * k + 4].to_vec();
        if !b.big_endian {
            tag.reverse(); // the disc's stored backwards
        }
        let (at, size) = (word(4 * n + 4 * k), word(8 * n + 4 * k));
        if at < HEADER || at - HEADER + size > d.len() {
            return err("a chunk runs past its block");
        }
        lot.chunks.push((String::from_utf8_lossy(&tag).into_owned(), d[at - HEADER..at - HEADER + size].to_vec()));
    }
    Ok(lot)
}

/// Each lot's globals (its SIMI chunk, in section 0's chunk tables beside its HOUS, in lot
/// order): 16-bit values, little-endian as on the disc, after the chunk's header (0, a version,
/// 80) and its "IMIS" mark: the hour
/// (global 0), the day (1), the minute (5), the year (8), ...
pub fn read_lot_globals(file: &[u8]) -> Result<Vec<Vec<i16>>> {
    let mut out = Vec::new();
    for b in read_neighbourhood(file)?.iter().filter(|b| b.section == 0) {
        let t = chunk_table(b)?;
        for (tag, d) in &t.chunks {
            if tag != "SIMI" {
                continue;
            }
            // a header (0, a version: 80), the mark, then the values
            let Some(at) = d.windows(4).position(|w| w == b"IMIS") else { continue };
            let d = &d[at + 4..];
            out.push(d.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect());
        }
    }
    Ok(out)
}

/// The goals the player has met (`goals::Goals::met`), from the neighbourhood's NGBH chunk: its
/// header (0, a version, "HBGN"), then little-endian 16-bit words as the original's goal manager
/// streams them (fn_801DB408): a first array (16, 64, 128 or 192 words by the version), then the
/// marks (none before version 0x4B, 34 words at it, 39 after). Empty if there is no NGBH.
pub fn read_goal_marks(file: &[u8]) -> Result<Vec<u16>> {
    for b in read_neighbourhood(file)?.iter().filter(|b| b.section == 1) {
        let t = chunk_table(b)?;
        let Some(c) = t.find("NGBH") else { continue };
        if c.len() < 12 || &c[8..12] != b"HBGN" {
            return err("not an NGBH");
        }
        let version = le32(c, 4);
        let first = match version {
            ..=0x45 => 16,
            0x46..=0x48 => 64,
            0x49 => 128,
            _ => 192,
        };
        let marks = match version {
            ..=0x4A => 0,
            0x4B => 34,
            _ => 39,
        };
        let at = 12 + 2 * first;
        let Some(words) = c.get(at..at + 2 * marks) else { return err("NGBH runs past its chunk") };
        return Ok(words.as_chunks::<2>().0.iter().map(|w| u16::from_le_bytes(*w)).collect());
    }
    Ok(Vec::new())
}

/// The neighbourhood's lots (section 3), each by its chunk table.
pub fn read_lots(file: &[u8]) -> Result<Vec<LotChunks>> {
    read_neighbourhood(file)?.iter().filter(|b| b.section == 3).map(chunk_table).collect()
}

/// A save on a GameCube memory card image (a raw 2 MB card): the file of `game` (its six-byte
/// game and maker code, "G4ME69") by the card's directory and block map; empty if the card has
/// none. Bustin' Out's save is the neighbourhood file as the game keeps it in memory: the same
/// blocks and chunks as the disc's, but its block headers and chunk tables big-endian (and the
/// FourCCs not reversed); the chunks' own contents are the disc's formats.
pub fn memory_card_file(card: &[u8], game: &str) -> Result<Vec<u8>> {
    // 8 KB blocks: 0 the header, 1-2 the directory (and its backup), 3-4 the block map; the
    // files from block 5. A directory entry (64 bytes): the game and maker code, ..., its first
    // block at +0x36 and its block count at +0x38 (big-endian); the map's entries (from +0x0A,
    // one for each block from 5) the next block of a file.
    const CARD_BLOCK: usize = 0x2000;
    if card.len() < 5 * CARD_BLOCK || game.len() != 6 {
        return Ok(Vec::new());
    }
    let be16 = |at: usize| u16::from_be_bytes([card[at], card[at + 1]]) as usize;
    let blocks = card.len() / CARD_BLOCK;
    for e in (CARD_BLOCK..2 * CARD_BLOCK).step_by(64) {
        if &card[e..e + 6] != game.as_bytes() {
            continue;
        }
        let mut b = be16(e + 0x36);
        let mut out = Vec::new();
        for _ in 0..be16(e + 0x38) {
            if b < 5 || b >= blocks {
                return err("a memory card file's blocks leave the card");
            }
            out.extend_from_slice(&card[b * CARD_BLOCK..(b + 1) * CARD_BLOCK]);
            b = be16(3 * CARD_BLOCK + 0x0A + 2 * (b - 5));
        }
        return Ok(out);
    }
    Ok(Vec::new())
}

/// One of a lot's three arrays (its Arry chunks): a width by height grid (64 by 64 on the disc)
/// of elements of a size (1, 8, 1), as bytes, row after row. Read from their run-length coding
/// (`docs/lots.md`, "The arrays").
#[derive(Clone, Debug, Default)]
pub struct LotArray {
    pub width: u32,
    pub height: u32,
    pub element_size: u32,
    pub bytes: Vec<u8>,
}

/// A lot's arrays; an error if one does not read to its chunk's end.
pub fn read_lot_arrays(lot: &LotChunks) -> Result<Vec<LotArray>> {
    let mut out = Vec::new();
    for (kind, c) in &lot.chunks {
        if kind != "Arry" {
            continue;
        }
        if c.len() < 20 {
            return err("Arry: no header");
        }
        let mut a = LotArray { width: le32(c, 4), height: le32(c, 8), element_size: le32(c, 12), bytes: Vec::new() };
        let need = a.width as usize * a.height as usize * a.element_size as usize;
        if need > 1 << 24 {
            return err("Arry: too large");
        }
        let mut p = 20;
        while a.bytes.len() < need {
            if p + 2 > c.len() {
                return err("Arry: runs past its chunk");
            }
            let h = le16(c, p) as usize;
            p += 2;
            if h & 0x8000 != 0 {
                // that many copies of one byte (padded to 2)
                let Some(&v) = c.get(p) else { return err("Arry: runs past its chunk") };
                a.bytes.resize(a.bytes.len() + (h & 0x7FFF), v);
                p += 2;
            } else {
                // that many bytes as they are (padded to even)
                if p + h > c.len() {
                    return err("Arry: runs past its chunk");
                }
                a.bytes.extend_from_slice(&c[p..p + h]);
                p += h + (h & 1);
            }
        }
        if a.bytes.len() != need || p != c.len() {
            return err("Arry: does not end at its chunk's end");
        }
        out.push(a);
    }
    Ok(out)
}

/// A neighbour of the neighbourhood (its NBRS chunk, the PC's layout: `docs/objects.md`, "The
/// neighbours"): its id, name (the character file's), GUID, person data (if saved) and its
/// relationships with other neighbours.
#[derive(Clone, Debug, Default)]
pub struct Neighbour {
    pub id: i16,
    pub name: String,
    pub guid: u32,
    pub person_data: Vec<i16>,
    pub relationships: Vec<NeighbourRelationship>,
}

#[derive(Clone, Debug, Default)]
pub struct NeighbourRelationship {
    pub with: i16,
    pub values: Vec<i32>,
}

/// A cursor over a chunk's little-endian values.
struct Cursor<'a> {
    c: &'a [u8],
    p: usize,
    what: &'static str,
}

impl Cursor<'_> {
    fn need(&self, n: usize) -> Result<()> {
        if self.p + n > self.c.len() { err(self.what) } else { Ok(()) }
    }
    fn i32(&mut self) -> Result<i32> {
        self.need(4)?;
        let v = le32(self.c, self.p) as i32;
        self.p += 4;
        Ok(v)
    }
    fn i16(&mut self) -> Result<i16> {
        self.need(2)?;
        let v = le16(self.c, self.p) as i16;
        self.p += 2;
        Ok(v)
    }
}

/// The neighbours, in id order; an error if NBRS does not read to its end.
pub fn read_neighbours(file: &[u8]) -> Result<Vec<Neighbour>> {
    let mut table = None;
    for b in read_neighbourhood(file)? {
        if b.section == 1 {
            table = Some(chunk_table(&b)?);
            break;
        }
    }
    let Some(c) = table.as_ref().and_then(|t| t.find("NBRS")) else { return err("the neighbourhood has no NBRS") };
    let mut r = Cursor { c, p: 0, what: "NBRS runs past its chunk" };
    r.need(16)?;
    if &c[8..12] != b"SRBN" {
        return err("not an NBRS");
    }
    r.p = 12;
    let slots = r.i32()?;
    if !(0..=4096).contains(&slots) {
        return err("NBRS: slots out of range");
    }
    let mut out = Vec::new();
    for _ in 0..slots {
        if r.i32()? == 0 {
            continue; // an empty slot
        }
        let mut n = Neighbour::default();
        r.i32()?; // the record's version (4)
        let Some(len) = c[r.p..].iter().position(|&b| b == 0) else { return err("NBRS: a name has no end") };
        n.name = String::from_utf8_lossy(&c[r.p..r.p + len]).into_owned();
        r.p += len + 1;
        r.p += r.p & 1;
        r.i32()?;
        if r.i32()? != 0 {
            // set: the person data follows
            for _ in 0..80 {
                n.person_data.push(r.i16()?);
            }
        }
        r.need(128)?;
        r.p += 128; // to be named (floats among them)
        n.id = r.i16()?;
        n.guid = r.i32()? as u32;
        r.i32()?;
        r.i32()?;
        let rels = r.i32()?;
        if !(0..=4096).contains(&rels) {
            return err("NBRS: relationships out of range");
        }
        for _ in 0..rels {
            r.i32()?;
            let with = r.i32()? as i16;
            let k = r.i32()?;
            if !(0..=64).contains(&k) {
                return err("NBRS: a relationship's values out of range");
            }
            let values = (0..k).map(|_| r.i32()).collect::<Result<_>>()?;
            n.relationships.push(NeighbourRelationship { with, values });
        }
        out.push(n);
    }
    if r.p != c.len() {
        return err("NBRS does not end at its chunk's end");
    }
    Ok(out)
}

/// A Sim of the neighbourhood made by its description (a "User" chunk of the neighbourhood's
/// section): the GUID of the neighbour it is (`Neighbour::guid`), its names, and its looks, 20
/// bytes: the gender (1 male), the age (1 adult), then 18 bytes of part indices and the like
/// (`outfit::Description`).
#[derive(Clone, Debug, Default)]
pub struct CreatedSim {
    pub guid: u32,
    pub first_name: String,
    pub last_name: String,
    pub looks: [u8; 20],
}

/// A User chunk's string: a byte order mark (ff fe) then 16-bit characters, high byte first,
/// to a zero one; the string and where the chunk goes on.
fn read_user_string(c: &[u8], mut p: usize) -> Option<(String, usize)> {
    if c.get(p..p + 2)? != [0xff, 0xfe] {
        return None;
    }
    p += 2;
    let mut units = Vec::new();
    loop {
        let u = u16::from_be_bytes([*c.get(p)?, *c.get(p + 1)?]);
        p += 2;
        if u == 0 {
            return Some((String::from_utf16_lossy(&units), p));
        }
        units.push(u);
    }
}

/// The neighbourhood's made Sims, in file order (its "User" chunks, each: 0, the version (4),
/// a "resU" mark, the GUID, the first and the last name, the looks); one that does not read is
/// left out.
pub fn read_created_sims(file: &[u8]) -> Result<Vec<CreatedSim>> {
    let mut out = Vec::new();
    for b in read_neighbourhood(file)? {
        if b.section != 1 {
            continue;
        }
        for (tag, c) in chunk_table(&b)?.chunks {
            if tag != "User" || c.get(8..12) != Some(b"resU".as_slice()) {
                continue;
            }
            let Some(guid) = c.get(12..16).map(|g| le32(g, 0)) else { continue };
            let Some((first_name, p)) = read_user_string(&c, 16) else { continue };
            let Some((last_name, p)) = read_user_string(&c, p) else { continue };
            let Some(looks) = c.get(p..p + 20).and_then(|l| <[u8; 20]>::try_from(l).ok()) else { continue };
            out.push(CreatedSim { guid, first_name, last_name, looks });
        }
    }
    Ok(out)
}

/// An object type of a lot (objt): its GUID, six 16-bit fields (field 4 is the type number the
/// lot's objects name) and its name.
#[derive(Clone, Debug, Default)]
pub struct ObjectType {
    pub guid: u32,
    pub fields: [u16; 6],
    pub name: String,
}

pub fn read_object_types(c: &[u8]) -> Result<Vec<ObjectType>> {
    if c.len() < 12 || &c[8..12] != b"tjbo" {
        return err("not an objt");
    }
    let mut out = Vec::new();
    let mut p = 12;
    loop {
        if p + 4 > c.len() {
            return err("objt has no end");
        }
        let guid = le32(c, p);
        if guid == 0 {
            if p + 4 != c.len() {
                return err("objt does not end at its zero GUID");
            }
            return Ok(out);
        }
        if p + 16 > c.len() {
            return err("objt entry runs past the chunk");
        }
        let fields = std::array::from_fn(|k| le16(c, p + 4 + 2 * k));
        let Some(len) = c[p + 16..].iter().position(|&b| b == 0) else { return err("objt name has no end") };
        let name = String::from_utf8_lossy(&c[p + 16..p + 16 + len]).into_owned();
        p += 16 + len + 1;
        if (p - 12) % 2 != 0 {
            p += 1;
        }
        out.push(ObjectType { guid, fields, name });
    }
}

/// A frame of an object's saved tree stack (`docs/lots.md`, "An object's section"). The tree is
/// named by its id as the object's scripts name it; the file the original saves with it (an
/// index into its list of loaded files, which depends on load order) is not kept: a frame's
/// file is its object's.
#[derive(Clone, Debug, Default)]
pub struct SavedFrame {
    pub stack_object: i16,
    pub tree: u16,
    pub node: u16,
    pub params: Vec<i16>,
    pub locals: Vec<i16>,
    /// A frame the engine started (the original's +8).
    pub engine: bool,
}

/// What the loader makes an object of, which decides what its section holds after the base
/// object's part (the world gives it from the definition).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SavedClass {
    /// The base object only.
    #[default]
    Object,
    /// A part of a multi-tile object (its definition has a master id).
    Part,
    /// A part of a multi-tile door.
    Door,
    /// A character (no definition on the disc).
    Sim,
}

/// An entry of a Sim's interaction queue as saved (fn_801CFB68; Castaway's Interaction): an id,
/// three objects (the Sim, the interaction's target, which is the pushed frame's stack object,
/// and a third), the tree the interaction runs (in the target's file), the rest to be named (c
/// is 100 in every saved entry: a priority?).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedQueueEntry {
    pub a: i32,
    pub id: i32,
    pub b: i32,
    pub c: i32,
    pub flags: i32,
    pub objects: [i16; 3],
    pub shorts: [i16; 4],
    pub tree: i16,
    pub e: f32,
    /// Not saved: the interaction's place in the target's table, when the engine queued it
    /// (PushAction); `None` for a saved entry. An action can be in a table more than once, each
    /// with its own guard (the group meal's 4201: guards 4202 and 8248).
    pub interaction: Option<usize>,
    /// Not saved: its place in the queue's order, highest first (`primitives::push_action`): 3 a
    /// push of priority 1 (maximum), 2 one of priority 2, 1 a saved entry (one the lot re-queued
    /// among them: the club's Sim 178's 4104 behind every later push).
    pub rank: u8,
}

impl Default for SavedQueueEntry {
    fn default() -> Self {
        SavedQueueEntry { a: 0, id: 0, b: 0, c: 0, flags: 0, objects: [0; 3], shorts: [0; 4], tree: 0, e: 0.0, interaction: None, rank: 1 }
    }
}

/// An object's saved relationship with another (the save's RelArray): the other object's id and
/// the relationship's values.
#[derive(Clone, Debug, Default)]
pub struct SavedRelationship {
    pub with: i32,
    pub values: Vec<i32>,
}

/// A running change of a motive (a Sim's +1120 list, fn_80229D5C; SetMotiveDelta's): the
/// motive, its change a tick and the value it goes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotiveDelta {
    pub motive: i32,
    pub rate: f32,
    pub target: f32,
}

/// A Sim's +1180 list's entry (fn_8022989C): an object, a 32-bit value and a flag (to be named).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry1180 {
    pub object: i16,
    pub value: i32,
    pub flag: bool,
}

/// A lot's object as its table gives it, in the world's list order.
#[derive(Clone, Debug, Default)]
pub struct LotObject {
    pub id: i16,
    /// Two 32-bit values (to be named).
    pub a: i32,
    pub b: i32,
    /// Counted 32-bit values.
    pub extra: Vec<i32>,
    /// objt field 4.
    pub kind: i16,
    /// Its section of the stream.
    pub section: usize,
    pub section_end: usize,
    /// The start of its section (fn_801F1E30's header, then fn_801F1F20's tables): its
    /// attributes (as many as the save gives), its 8 temps and its 72 data fields.
    pub header: [i32; 8],
    pub attributes: Vec<i16>,
    pub temps: Vec<i16>,
    pub data: Vec<i16>,
    /// Then its tree stack, bottom first, its relationships, and the rest of the base object's
    /// part (to be named): a list of pairs, a 32-bit value, a list of 16-bit values.
    pub stack: Vec<SavedFrame>,
    pub relationships: Vec<SavedRelationship>,
    pub pairs: Vec<(i16, i16)>,
    pub shorts: Vec<i16>,
    /// Where the base object's part ends; the class's own part runs from here to the loader's
    /// int at the end of the section.
    pub base_end: usize,
    pub saved_class: SavedClass,
    /// A multi-tile object's part: its leader's id (0 for the leader itself).
    pub leader: i16,
    /// A Sim's own part: its person data (scope 18), its motives (scope 14), a second table of
    /// 16 floats (to be named), the entry it is on and its queue.
    pub person_data: Vec<i16>,
    pub motives: Vec<f32>,
    pub motives_b: Vec<f32>,
    pub current: SavedQueueEntry,
    pub queue: Vec<SavedQueueEntry>,
    pub motive_deltas: Vec<MotiveDelta>,
    pub list_1180: Vec<Entry1180>,
    /// Its +1160 list (fn_8022933C): only a count is saved; its elements are made at the load.
    pub list_1160: i32,
}

pub const OBJECT_DATA_FIELDS: usize = 72;
pub const OBJECT_TEMPS: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct LotObjects {
    pub objects: Vec<LotObject>,
    /// The lot's own part, after the objects.
    pub lot_section: usize,
    pub lot_section_end: usize,
    /// The lot's own part (the loader, fn_801FDE80, after the objects): an object, a list of
    /// objects (both to be named; empty on the disc) and a string (UTF-16; empty on the disc).
    pub lot_object: i16,
    pub lot_objects: Vec<i16>,
    pub lot_string: Vec<u16>,
}

/// ObjM's version on Bustin' Out's disc; the conditions are its loader's.
const VERSION: i32 = 80;

fn count(r: &mut SaveStream, limit: i32, what: &str) -> Result<i32> {
    let n = r.r32()?;
    if n < 0 || n > limit { err(what) } else { Ok(n) }
}

/// The object's relationships (its +192 member, fn_8023243C; Castaway names it RelMatrixImpl):
/// a value, then (when it is negative) a counted list of present-flagged entries (fn_80232E2C),
/// or (when not) that many entries. An entry (RelArray, fn_802326CC / fn_80233084): the other
/// object's id and a counted list of 32-bit values.
fn read_relationship(r: &mut SaveStream) -> Result<SavedRelationship> {
    let with = r.r32()?;
    let n = count(r, 4096, "ObjM: a relationship's values out of range")?;
    let values = (0..n).map(|_| r.r32()).collect::<Result<_>>()?;
    Ok(SavedRelationship { with, values })
}

fn read_relationships(r: &mut SaveStream, o: &mut LotObject) -> Result<()> {
    let a = r.r32()?;
    if a >= 0 {
        if a > 4096 {
            return err("ObjM: relationships out of range");
        }
        for _ in 0..a {
            o.relationships.push(read_relationship(r)?);
        }
        return Ok(());
    }
    if VERSION > 78 {
        r.r32()?;
    }
    for _ in 0..count(r, 4096, "ObjM: relationship entries out of range")? {
        if r.r32()? != 0 {
            // an int: the entry is there
            o.relationships.push(read_relationship(r)?);
        }
    }
    Ok(())
}

/// After the data: a zero, the tree stack (fn_8023F928, frames fn_8023E9A8, file fn_801F961C),
/// the relationships, the pairs (fn_801F22D0), a 32-bit value, the shorts.
fn read_base_rest(r: &mut SaveStream, o: &mut LotObject) -> Result<()> {
    r.r16()?;
    let frames = r.r32()?;
    if !(0..=64).contains(&frames) {
        return err("ObjM: a tree stack out of range");
    }
    r.r32()?; // its capacity
    for _ in 0..frames {
        let stack_object = r.r16()? as i16;
        let tree = r.r16()? as u16;
        let node = r.r16()? as u16;
        let nlocals = r.r8()? & 0xFF;
        let nparams = r.r8()? & 0xFF;
        let params = (0..nparams).map(|_| r.r16().map(|v| v as i16)).collect::<Result<_>>()?;
        let locals = (0..nlocals).map(|_| r.r16().map(|v| v as i16)).collect::<Result<_>>()?;
        let engine = r.r32()? != 0;
        r.r16()?; // the original's file index
        o.stack.push(SavedFrame { stack_object, tree, node, params, locals, engine });
    }
    read_relationships(r, o)?;
    for _ in 0..r.r16()?.max(0) {
        let a = r.r16()? as i16;
        o.pairs.push((a, r.r16()? as i16));
    }
    r.r32()?;
    for _ in 0..r.r16()?.max(0) {
        o.shorts.push(r.r16()? as i16);
    }
    Ok(())
}

fn read_queue_entry(r: &mut SaveStream) -> Result<SavedQueueEntry> {
    let mut e = SavedQueueEntry { a: r.r32()?, id: r.r32()?, ..Default::default() };
    for o in &mut e.objects {
        *o = r.r16()? as i16; // fn_801FDD0C
    }
    e.b = r.r32()?;
    for v in &mut e.shorts {
        *v = r.r16()? as i16;
    }
    e.c = r.r32()?;
    e.tree = r.r16()? as i16;
    e.e = r.rfloat()?;
    e.flags = r.r32()?; // from version 62
    Ok(e)
}

/// A Sim's own part (fn_80224C88), for version 80.
fn read_sim(r: &mut SaveStream, o: &mut LotObject) -> Result<()> {
    // Its +1136 member (fn_800AD6E8): a count of (bool, 32-bit) pairs, five values.
    for _ in 0..r.r16()?.max(0) {
        r.rbool()?;
        r.r32()?;
    }
    r.rbool()?;
    r.rfloat()?;
    r.r32()?;
    r.r32()?;
    r.r32()?;
    for _ in 0..16 {
        o.motives.push(r.rfloat()?);
    }
    for _ in 0..16 {
        o.motives_b.push(r.rfloat()?);
    }
    for _ in 0..80 {
        o.person_data.push(r.r16()? as i16);
    }
    // The +1160 list: its count only (each element's read, fn_80249B80, reads nothing).
    o.list_1160 = r.r32()?;
    if !(0..=1024).contains(&o.list_1160) {
        return err("ObjM: a Sim's +1160 list out of range");
    }
    o.current = read_queue_entry(r)?;
    let n = r.r32()?;
    if !(0..=8).contains(&n) {
        return err("ObjM: a Sim's queue out of range");
    }
    for _ in 0..n {
        o.queue.push(read_queue_entry(r)?);
    }
    // The +1180 list (fn_8021A92C each): an object, a 32-bit value, a bool.
    let k = r.r32()?;
    if !(0..=1024).contains(&k) {
        return err("ObjM: a Sim's +1180 list out of range");
    }
    for _ in 0..k {
        let object = r.r16()? as i16;
        let value = r.r32()?;
        o.list_1180.push(Entry1180 { object, value, flag: r.rbool()? });
    }
    // The +1120 list (fn_8021A598 each): the motive deltas.
    let k = r.r32()?;
    if !(0..=1024).contains(&k) {
        return err("ObjM: a Sim's +1120 list out of range");
    }
    for _ in 0..k {
        let motive = r.r32()?;
        let rate = r.rfloat()?;
        o.motive_deltas.push(MotiveDelta { motive, rate, target: r.rfloat()? });
    }
    Ok(())
}

/// A multi-tile object's part (fn_801D3F50) and a door's (fn_8022DE18): its leader's id kept,
/// the rest read past (its tile offsets among it).
fn read_part(r: &mut SaveStream, o: &mut LotObject, door: bool) -> Result<()> {
    for _ in 0..6 {
        r.r32()?;
    }
    o.leader = r.r16()? as i16;
    if door {
        for _ in 0..count(r, 4096, "ObjM: a door's values out of range")? {
            r.rfloat()?;
        }
    }
    Ok(())
}

/// ObjM: the table, then the mark chain (one section per object, then the lot's). `class_of`
/// gives the class of an objt type number. Every section must end exactly at its mark.
pub fn read_lot_objects(m: &[u8], class_of: impl Fn(i16) -> SavedClass) -> Result<LotObjects> {
    const START: usize = 12;
    if m.len() < START + 1 || &m[8..12] != b"MjbO" {
        return err("not an ObjM");
    }
    let s = &m[START..];
    let mut r = SaveStream::new(s, 0);
    r.compressed = r.rbool()?;
    let mut out = LotObjects::default();
    loop {
        let id = r.r16()? as i16;
        if id == 0 {
            break;
        }
        let a = r.r32()?;
        let b = r.r32()?;
        let n = r.r8()? & 0xFF;
        let extra = (0..n).map(|_| r.r32()).collect::<Result<_>>()?;
        let kind = r.r16()? as i16;
        out.objects.push(LotObject { id, a, b, extra, kind, ..Default::default() });
    }
    // The mark chain: each mark the position of the next; one section per object, then the
    // lot's own part to the stream's end.
    let mut mark_at = r.pos();
    let mut next = r.mark()? as usize;
    let mut sections = Vec::new();
    loop {
        let start = mark_at + 4;
        if next <= mark_at || next + 4 > s.len() {
            sections.push((start, s.len()));
            break;
        }
        sections.push((start, next));
        mark_at = next;
        next = le32(s, next) as usize;
    }
    if sections.len() != out.objects.len() + 1 {
        return err("ObjM: not one section per object");
    }
    for (o, &(start, end)) in out.objects.iter_mut().zip(&sections) {
        o.section = START + start;
        o.section_end = START + end;
        let mut h = SaveStream::new(s, start);
        h.compressed = true;
        for i in 0..7 {
            o.header[i] = h.r32()?;
        }
        o.header[7] = h.r16()?;
        let n = h.r16()?;
        if !(0..=4096).contains(&n) {
            return err("ObjM: an object's attribute count is out of range");
        }
        for _ in 0..n {
            o.attributes.push(h.r16()? as i16);
        }
        for _ in 0..OBJECT_TEMPS {
            o.temps.push(h.r16()? as i16);
        }
        for _ in 0..OBJECT_DATA_FIELDS {
            o.data.push(h.r16()? as i16);
        }
        if o.data[11] != o.id {
            return err("ObjM: an object's data 11 is not its id");
        }
        read_base_rest(&mut h, o)?;
        if h.pos() > end {
            return err("ObjM: an object runs past its section");
        }
        o.base_end = START + h.pos();
        o.saved_class = class_of(o.kind);
        match o.saved_class {
            SavedClass::Object => {}
            SavedClass::Part => read_part(&mut h, o, false)?,
            SavedClass::Door => read_part(&mut h, o, true)?,
            SavedClass::Sim => read_sim(&mut h, o)?,
        }
        h.r32()?; // the loader's int
        h.pad();
        if h.pos() != end {
            return err("ObjM: an object's section does not end at its mark");
        }
    }
    let &(start, end) = sections.last().unwrap();
    out.lot_section = START + start;
    out.lot_section_end = START + end;
    let mut l = SaveStream::new(s, start);
    l.compressed = true;
    out.lot_object = l.r16()? as i16; // the manager's object reference (fn_801FDD0C)
    for _ in 0..count(&mut l, 4096, "ObjM: the lot's objects out of range")? {
        out.lot_objects.push(l.r16()? as i16);
    }
    l.pad(); // the string (Recon_String2): UTF-16 from an even position
    let mut p = l.pos() + (l.pos() & 1);
    let mut little = false;
    if p + 2 <= s.len() && s[p] == 0xFE && s[p + 1] == 0xFF {
        little = true;
        p += 2;
    } else if p + 2 <= s.len() && s[p] == 0xFF && s[p + 1] == 0xFE {
        p += 2;
    }
    loop {
        if p + 2 > s.len() {
            return err("ObjM: the lot's string has no end");
        }
        let u = if little { le16(s, p) } else { u16::from_be_bytes([s[p], s[p + 1]]) };
        p += 2;
        if u == 0 {
            break;
        }
        out.lot_string.push(u);
    }
    if p != s.len() {
        return err("ObjM: the lot's part does not end the stream");
    }
    Ok(out)
}
