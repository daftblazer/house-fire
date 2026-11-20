//! The writers of the engine's saved data: the inverse of `save.rs`'s readers (`docs/lots.md`,
//! "Saves"). A neighbourhood file's blocks are packed, checksummed and put at their places, and
//! the file goes back into a memory card's block chain.

use crate::iff::FormatError;
use crate::save::{LotChunks, NeighbourhoodBlock, memory_card_file_blocks};

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
