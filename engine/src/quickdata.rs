//! Quick data: the original's tables baked into images (`quickdat.arc`'s entries, such as
//! `SimsObjects`), read for what the engine needs from them (`docs/assets.md`; the full format in
//! `tools/quickdata.py`).
//!
//! An entry starts with records of 24 bytes (the record size, the image's offset and size, its
//! fix-ups' offset and size, the address it was built for); the first is the main image. The
//! fix-ups list the image's pointers; a pointer is read back as an offset into the image. The
//! image begins with its table directory: (Bustin' Out) a pointer, the table count, then from 8
//! a descriptor of 20 bytes a table: its name, its rows, its row names, row count, row size.

use std::collections::{BTreeMap, HashSet};

fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

/// One table: where its rows start in the image, how many, how long each.
#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub rows: usize,
    pub count: usize,
    pub row_size: usize,
}

/// A quick data entry's main image.
pub struct Image {
    pub data: Vec<u8>,
    base: u32,
    pointers: HashSet<usize>,
    pub tables: BTreeMap<String, Table>,
}

impl Image {
    /// The main image of a quick data entry, or `None` if `b` is not one.
    pub fn read(b: &[u8]) -> Option<Image> {
        if be32(b, 0)? != 24 {
            return None;
        }
        let w = |k: usize| be32(b, 4 + 4 * k).map(|v| v as usize);
        let (off, size, foff, fsize) = (w(0)?, w(1)?, w(2)?, w(3)?);
        let base = be32(b, 20)?;
        let data = b.get(off..off + size)?.to_vec();
        let fix = b.get(foff..foff + fsize)?;
        let mut pointers = HashSet::new();
        let mut i = 0;
        while i + 4 <= fix.len() {
            let v = be32(fix, i)? as usize;
            i += 4;
            match v & 3 {
                0 => {
                    pointers.insert(v);
                }
                1 => {
                    let at = be32(fix, i)? as usize;
                    i += 4;
                    if v >> 2 > size / 4 {
                        return None;
                    }
                    pointers.extend((0..v >> 2).map(|k| at + 4 * k));
                }
                k => pointers.extend((0..k).map(|j| (v & !3) + 4 * j)),
            }
        }
        if pointers.iter().any(|&p| p + 4 > data.len()) || !pointers.contains(&0) {
            return None;
        }
        let mut img = Image { data, base, pointers, tables: BTreeMap::new() };
        let n = img.u32(4)? as usize;
        for t in 0..n.min(4096) {
            let d = 8 + 20 * t;
            let name = img.ptr(d)?;
            let end = img.data.get(name..)?.iter().position(|&c| c == 0)?;
            let name = img.data[name..name + end].iter().map(|&c| c as char).collect();
            let table = Table { rows: img.ptr(d + 4).unwrap_or(0), count: img.u32(d + 12)? as usize, row_size: img.u32(d + 16)? as usize };
            img.tables.insert(name, table);
        }
        Some(img)
    }

    pub fn u32(&self, o: usize) -> Option<u32> {
        be32(&self.data, o)
    }

    /// The offset the pointer at `o` points to (`None` for a null or a word that is no pointer).
    pub fn ptr(&self, o: usize) -> Option<usize> {
        if !self.pointers.contains(&o) {
            return None;
        }
        let t = self.u32(o)?.checked_sub(self.base)? as usize;
        (t <= self.data.len()).then_some(t)
    }

    /// The rows of a table, each its offset in the image.
    pub fn rows(&self, name: &str) -> impl Iterator<Item = usize> + '_ {
        let t = self.tables.get(name).copied().unwrap_or(Table { rows: 0, count: 0, row_size: 0 });
        (0..t.count).map(move |k| t.rows + k * t.row_size).filter(move |&r| r + t.row_size <= self.data.len())
    }
}

/// What an object draws with: the dataset it loads (an archive entry's id, `arc::Entry::id`) and
/// its model's name hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjResources {
    pub dataset: u32,
    pub model: u32,
    /// The row's second word: its bit 0x40000000 set for an object hung on a wall (drawn 0.45
    /// tile towards the wall, `scene.rs`).
    pub flags: u32,
    /// The first word of what the row's first word points at (0 without one): the name hash
    /// the original keys a window's kind by (`light::window_kind`).
    pub key: u32,
}

impl ObjResources {
    pub fn on_wall(&self) -> bool {
        self.flags & 0x4000_0000 != 0
    }
}

/// Each object's resources by GUID, from `SimsObjects`: an `ObjDefinition` row (196 bytes, the
/// GUID at +0x1c) points (+0xc0) at an `ObjDefinition::ResData` row (48 bytes: flags at +4, the
/// dataset's id at +0x10, the model's hash at +0x18, a pointer at +0 to a record that starts
/// with a name hash). Definitions without one are left out.
pub fn object_resources(img: &Image) -> BTreeMap<u32, ObjResources> {
    img.rows("ObjDefinition")
        .filter_map(|r| {
            let res = img.ptr(r + 0xc0)?;
            Some((img.u32(r + 0x1c)?, ObjResources { dataset: img.u32(res + 0x10)?, model: img.u32(res + 0x18)?, flags: img.u32(res + 4)?, key: img.ptr(res).and_then(|k| img.u32(k)).unwrap_or(0) }))
        })
        .collect()
}

/// The wall styles that are fences, from `SimsTiles`: its `FenceData` rows (16 bytes: a hash, a
/// price, a name, the style).
pub fn fence_styles(img: &Image) -> Vec<u32> {
    img.rows("FenceData").filter_map(|r| img.u32(r + 12)).collect()
}

/// The wall patterns, from `SimsTiles`: its one `WallSet` row points at a list of `WallTile` rows
/// (20 bytes: a value, a name, the pattern's shader's name hash, two more), a pattern's index
/// into it; each pattern's shader hash.
pub fn wall_patterns(img: &Image) -> Vec<u32> {
    let Some(t) = img.tables.get("WallTile").copied() else { return Vec::new() };
    let Some(list) = img.rows("WallSet").next().and_then(|r| img.ptr(r)) else { return Vec::new() };
    (0..)
        .map_while(|k| img.ptr(list + 4 * k))
        .take_while(|&row| row >= t.rows && row < t.rows + t.count * t.row_size)
        .filter_map(|row| img.u32(row + 8))
        .collect()
}

/// A floor pattern: its shader's archive id and its kind (+16: 6 for the road's, 7 the sidewalk's
/// and street's, 3 or 4 for most inside floors; what it is used for is not known yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorPattern {
    pub shader: u32,
    pub kind: u32,
}

/// The floor patterns, from `SimsTiles`: its one `FloorSet` row points at a list of `FloorTile`
/// rows (24 bytes: a value, a name, a value, the shader's archive id at +12, the kind at +16, a
/// last word), a tile's floor (the lot's first array) an index into it.
pub fn floor_patterns(img: &Image) -> Vec<FloorPattern> {
    let Some(t) = img.tables.get("FloorTile").copied() else { return Vec::new() };
    let Some(list) = img.rows("FloorSet").next().and_then(|r| img.ptr(r)) else { return Vec::new() };
    (0..)
        .map_while(|k| img.ptr(list + 4 * k))
        .take_while(|&row| row >= t.rows && row < t.rows + t.count * t.row_size)
        .filter_map(|row| Some(FloorPattern { shader: img.u32(row + 12)?, kind: img.u32(row + 16)? }))
        .collect()
}

