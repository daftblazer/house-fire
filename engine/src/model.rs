//! Models as Bustin' Out keeps them (`models.arc`'s records, `docs/assets.md`, "Models"): read so
//! far for the first sub-model of a record, of the two kinds known. The rest of a record (its
//! tail: strip indices, bounds, further sub-models) is kept as bytes until it is read.

/// The arrays a sub-model has, each as the record keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubModel {
    /// The byte its header starts with: 1 when the batch names its matrices, 0 when it has none
    /// (an object's one-piece mesh).
    pub kind: u8,
    pub count: usize,
    /// Positions: three s16 and two more bytes a vertex, in the record's scale.
    pub positions: Vec<u8>,
    /// Texture coordinates: two 16-bit values a vertex (4096ths).
    pub texcoords: Vec<u8>,
    /// Colours (RGBA) for the kind that has them.
    pub colours: Option<Vec<u8>>,
    /// Normals: three s8 a vertex.
    pub normals: Vec<u8>,
    /// Unused: kept for the first-sub-model reader (`read`).
    pub values: Option<Vec<u8>>,
    /// Kind 0's GX display list (format bit 0x20): the vertices it draws, the indices a vertex
    /// (one for each attribute) and its bytes (0x9e, a triangle strip of vertex format 6, a 16-bit
    /// count, then the indices; padded to 32).
    pub list: Option<(usize, usize, Vec<u8>)>,
    /// The weights a vertex (0: one matrix for the batch; 2 or more: blended, `blend` keeping
    /// them).
    pub weights: u8,
    /// The matrices the batch uses: each a slot and its index in the batch.
    pub matrices: Vec<(u16, u8)>,
    /// A blended batch's weights: 2 bytes a weight a vertex, after the other arrays.
    pub blend: Vec<u8>,
}

/// A node of a record's tree: its parent (-1 for the root) and its sub-models' batches.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub parent: i32,
    pub subs: Vec<(u32, SubModel)>,
}

/// A whole record, read to its end: its nodes (each sub-model with its format) and its bounds
/// (the 72 bytes at the end).
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub name: String,
    pub scale: f32,
    pub nodes: Vec<Node>,
    pub bounds: Vec<u8>,
}

fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

/// `size` bytes at `*p`, moving `*p` past them.
fn take(b: &[u8], p: &mut usize, size: usize) -> Option<Vec<u8>> {
    let v = b.get(*p..*p + size)?.to_vec();
    *p += size;
    Some(v)
}

/// A sub-model block at `p`: its format, a hash, its matrix slots (a count, a byte each), a
/// float, then its batches, until the byte 6. A batch is its matrices (`01 00` slot, index, each),
/// three zero bytes, a 16-bit count and the arrays by the format; a byte from 2 to 5 before a
/// batch sets the weights a vertex for the batches after it (2 bytes a weight more a vertex, kept
/// in an array after the others). A display list (format 0x20) follows its one batch. Each batch is given as
/// a `SubModel`, with the format; and where the next block starts.
fn sub_model(b: &[u8], mut p: usize) -> Option<(Vec<(u32, SubModel)>, usize)> {
    let format = be32(b, p)?;
    let slots = be32(b, p + 8)? as usize;
    if slots > 64 {
        return None;
    }
    p += 4 + 4 + 4 + slots + 4;
    let mut weights = 0u8;
    let mut out = Vec::new();
    loop {
        let k = *b.get(p)?;
        if k == 6 {
            break;
        }
        if (2..=5).contains(&k) {
            weights = k;
            p += 1;
            continue;
        }
        if k > 1 {
            return None;
        }
        let mut matrices = Vec::new();
        while *b.get(p)? == 1 {
            matrices.push((be16(b, p + 1)?, *b.get(p + 3)?));
            p += 4;
        }
        let count = be16(b, p + 3)? as usize;
        p += 5;
        let positions = take(b, &mut p, 8 * count)?;
        let texcoords = if format & 0x08 != 0 { take(b, &mut p, 4 * count)? } else { Vec::new() };
        let colours = if format & 0x04 != 0 { Some(take(b, &mut p, 4 * count)?) } else { None };
        let normals = if format & 0x10 != 0 { take(b, &mut p, 3 * count)? } else { Vec::new() };
        let blend = take(b, &mut p, 2 * weights as usize * count)?;
        // the display list after the arrays: its vertex count (a word), the indices a vertex (a
        // byte), the list's size (a word, padded to 32), the word 3, the list
        let list = if format & 0x20 != 0 {
            let drawn = be32(b, p)? as usize;
            let per = *b.get(p + 4)? as usize;
            let size = be32(b, p + 5)? as usize;
            p += 13;
            Some((drawn, per, take(b, &mut p, size)?))
        } else {
            None
        };
        let kind = u8::from(!matrices.is_empty());
        out.push((format, SubModel { kind, count, positions, texcoords, colours, normals, values: None, list, weights, matrices, blend }));
        if format & 0x20 != 0 && *b.get(p)? != 6 {
            return None;
        }
    }
    Some((out, p + 1))
}

/// Reads a whole record: the head, the node count (+0x19 past the name), each node's parent,
/// sub-model count and sub-models, and the 72 bytes of bounds that end it; `None` unless it reads
/// so to its last byte (the kinds of sub-model known).
pub fn read_record(b: &[u8]) -> Option<Record> {
    read_record_prefix(b).filter(|&(_, end)| end == b.len()).map(|(r, _)| r)
}

/// Reads a record at the start of `b` (one kept inside other data, `datasets.arc`'s): the record
/// and where it ends.
pub fn read_record_prefix(b: &[u8]) -> Option<(Record, usize)> {
    if b.get(..4)? != [0, 1, 0, 0] {
        return None;
    }
    let len = b.get(6..)?.iter().position(|&c| c == 0)?;
    let name: String = b[6..6 + len].iter().map(|&c| c as char).collect();
    let q = 6 + len + 1;
    if be32(b, q)? != 0x25 {
        return None;
    }
    // the head's entries (their count at +0x13, 0 for an object; the scenes' are 28 bytes each:
    // two words, 12 zero bytes, two floats, e.g. 1 and 80), then a byte, the scale and the nodes
    // 28 bytes a entry later
    let q = q + 28 * *b.get(q + 0x13)? as usize;
    let scale = f32::from_bits(be32(b, q + 0x15)?);
    let count = be32(b, q + 0x19)? as usize;
    if count > 256 {
        return None;
    }
    let mut p = q + 0x1d;
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let parent = be32(b, p)? as i32;
        let n = be32(b, p + 4)? as usize;
        if n > 256 {
            return None;
        }
        p += 8;
        let mut subs = Vec::with_capacity(n);
        for _ in 0..n {
            let (batches, next) = sub_model(b, p)?;
            subs.extend(batches);
            p = next;
        }
        nodes.push(Node { parent, subs });
    }
    let bounds = b.get(p..p + 72)?.to_vec();
    Some((Record { name, scale, nodes, bounds }, p + 72))
}

/// A record's head and first sub-model.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub name: String,
    /// The positions' scale (1/4096 for most objects).
    pub scale: f32,
    /// The vertex format (the word at +0x25 past the name): 0x08 texture coordinates, 0x04
    /// colours, 0x10 normals, 0x20 a 16-bit value a vertex (0x02 always set).
    pub format: u32,
    pub first: SubModel,
    /// Where the bytes after the first sub-model's arrays begin.
    pub tail: usize,
}

fn be16(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(p..p + 2)?.try_into().ok()?))
}

/// Reads a record's head and first sub-model; `None` if it is not of a shape known.
pub fn read(b: &[u8]) -> Option<Model> {
    if b.get(..4)? != [0, 1, 0, 0] {
        return None;
    }
    let len = b.get(6..)?.iter().position(|&c| c == 0)?;
    let name: String = b[6..6 + len].iter().map(|&c| c as char).collect();
    let q = 6 + len + 1;
    if u32::from_be_bytes(b.get(q..q + 4)?.try_into().ok()?) != 0x25 {
        return None;
    }
    let scale = f32::from_bits(u32::from_be_bytes(b.get(q + 0x15..q + 0x19)?.try_into().ok()?));
    let format = u32::from_be_bytes(b.get(q + 0x25..q + 0x29)?.try_into().ok()?);
    let sub = q + 0x36;
    let kind = *b.get(sub)?;
    let (count, mut p) = match kind {
        1 => (be16(b, sub + 7)? as usize, sub + 9),
        0 => (be16(b, sub + 3)? as usize, sub + 5),
        _ => return None,
    };
    let mut take = |size: usize| -> Option<Vec<u8>> {
        let v = b.get(p..p + size)?.to_vec();
        p += size;
        Some(v)
    };
    let positions = take(8 * count)?;
    let texcoords = if format & 0x08 != 0 { take(4 * count)? } else { Vec::new() };
    let colours = if format & 0x04 != 0 { Some(take(4 * count)?) } else { None };
    let normals = if format & 0x10 != 0 { take(3 * count)? } else { Vec::new() };
    let values = if format & 0x20 != 0 { Some(take(2 * count)?) } else { None };
    Some(Model { name, scale, format, first: SubModel { kind, count, positions, texcoords, colours, normals, values, list: None, weights: 0, matrices: Vec::new(), blend: Vec::new() }, tail: p })
}
