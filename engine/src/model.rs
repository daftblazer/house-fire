//! Models as Bustin' Out keeps them (`models.arc`'s records, `docs/assets.md`, "Models"): read so
//! far for the first sub-model of a record, of the two kinds known. The rest of a record (its
//! tail: strip indices, bounds, further sub-models) is kept as bytes until it is read.

/// The arrays a sub-model has, each as the record keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubModel {
    /// The byte its header starts with (1: positions, texture coordinates, normals; 0: those
    /// and colours).
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
}

/// A record's head and first sub-model.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub name: String,
    /// The positions' scale (1/4096 for most objects).
    pub scale: f32,
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
    let texcoords = take(4 * count)?;
    let colours = if kind == 0 { Some(take(4 * count)?) } else { None };
    let normals = take(3 * count)?;
    Some(Model { name, scale, first: SubModel { kind, count, positions, texcoords, colours, normals }, tail: p })
}
