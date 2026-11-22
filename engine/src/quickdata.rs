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
    /// Where the row names' pointer array starts (0 for a table whose rows have none).
    pub names: usize,
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
        Self::read_language(b, 0)
    }

    /// The main image with a language's image after it, as the engine loads them (the
    /// language's at the main image's size rounded up to 4, both built for the same address);
    /// `lang` counts from 1, the first language's image, and 0 loads none. The interface's
    /// strings are in the language images.
    pub fn read_language(b: &[u8], lang: usize) -> Option<Image> {
        let (mut data, mut pointers, base) = Self::record(b, 0)?;
        if !pointers.contains(&0) {
            return None;
        }
        if lang > 0 {
            let (l, lp, _) = Self::record(b, lang)?;
            let at = (data.len() + 3) & !3;
            data.resize(at, 0);
            data.extend_from_slice(&l);
            pointers.extend(lp.into_iter().map(|p| at + p));
        }
        let mut img = Image { data, base, pointers, tables: BTreeMap::new() };
        img.read_tables()?;
        Some(img)
    }

    /// A record of the entry: its data, the offsets of its pointers and the address it was
    /// built for.
    fn record(b: &[u8], k: usize) -> Option<(Vec<u8>, HashSet<usize>, u32)> {
        if be32(b, 0)? != 24 || be32(b, 4)? as usize / 24 <= k {
            return None;
        }
        let w = |j: usize| be32(b, 24 * k + 4 + 4 * j).map(|v| v as usize);
        let (off, size, foff, fsize) = (w(0)?, w(1)?, w(2)?, w(3)?);
        let base = be32(b, 24 * k + 20)?;
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
        if pointers.iter().any(|&p| p + 4 > data.len()) {
            return None;
        }
        Some((data, pointers, base))
    }

    /// Reads the table directory of the main image.
    fn read_tables(&mut self) -> Option<()> {
        let img = self;
        let n = img.u32(4)? as usize;
        for t in 0..n.min(4096) {
            let d = 8 + 20 * t;
            let name = img.ptr(d)?;
            let end = img.data.get(name..)?.iter().position(|&c| c == 0)?;
            let name = img.data[name..name + end].iter().map(|&c| c as char).collect();
            let table = Table { rows: img.ptr(d + 4).unwrap_or(0), count: img.u32(d + 12)? as usize, row_size: img.u32(d + 16)? as usize, names: img.ptr(d + 8).unwrap_or(0) };
            img.tables.insert(name, table);
        }
        Some(())
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

    /// The zero-ended UTF-16 (big endian) string the pointer at `o` points to.
    pub fn wide_string(&self, o: usize) -> Option<String> {
        let b = self.data.get(self.ptr(o)?..)?;
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).take_while(|&u| u != 0).collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// The interface string a word of a row holds: the word at `o` points at a pointer to it.
    pub fn text(&self, o: usize) -> Option<String> {
        self.wide_string(self.ptr(o)?)
    }

    /// A string table of the interface (a language image's): each row points at a pointer to
    /// the row's string.
    pub fn strings(&self, table: &str) -> Vec<String> {
        self.rows(table).map(|r| self.text(r).unwrap_or_default()).collect()
    }

    /// The row of a table with this name (the row names the descriptor's third word points at,
    /// a pointer to a zero-ended string a row), its offset in the image.
    pub fn row_named(&self, table: &str, name: &str) -> Option<usize> {
        let t = self.tables.get(table).copied()?;
        if t.names == 0 {
            return None;
        }
        let k = (0..t.count).find(|&k| {
            self.ptr(t.names + 4 * k).and_then(|s| self.data.get(s..)).is_some_and(|b| b.starts_with(name.as_bytes()) && b.get(name.len()) == Some(&0))
        })?;
        Some(t.rows + k * t.row_size).filter(|&r| r + t.row_size <= self.data.len())
    }

    /// The rows of a table, each its offset in the image.
    pub fn rows(&self, name: &str) -> impl Iterator<Item = usize> + '_ {
        let t = self.tables.get(name).copied().unwrap_or(Table { rows: 0, count: 0, row_size: 0, names: 0 });
        (0..t.count).map(move |k| t.rows + k * t.row_size).filter(move |&r| r + t.row_size <= self.data.len())
    }
}

/// What an object draws with: the dataset it loads (an archive entry's id, `arc::Entry::id`) and
/// its model's name hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjResources {
    pub dataset: u32,
    pub model: u32,
    /// The row's second word: its bit 0x40000000 set for an object hung on a wall (drawn 0.45
    /// tile towards the wall, `scene.rs`).
    pub flags: u32,
    /// The first word of what the row's first word points at (0 without one): the name hash
    /// the original keys a window's kind by (`light::window_kind`).
    pub key: u32,
    /// The third word of that record: the id (an `animatio.arc` entry's) of the animation the
    /// object's controller plays when it is idle (a lamp's, one still pose of two frames); 0
    /// without one.
    pub idle: u32,
    /// The animation ids of every state the object's graphic number picks from (its data field
    /// 0): the record is a list of 32-byte states, each with the record's key in its first word
    /// and its animation in its third (the first is the idle one). Empty without a record.
    pub states: Vec<u32>,
}

impl ObjResources {
    /// The animation id of graphic `graphic` (the idle one for a graphic the list lacks or leaves
    /// without an animation).
    pub fn animation(&self, graphic: i16) -> u32 {
        usize::try_from(graphic).ok().and_then(|g| self.states.get(g)).copied().filter(|&id| id != 0).unwrap_or(self.idle)
    }

    pub fn on_wall(&self) -> bool {
        self.flags & 0x4000_0000 != 0
    }
}

/// The most states an object's record is read for (the longest on the disc has 5).
const MAX_STATES: usize = 16;

/// Each object's resources by GUID, from `SimsObjects`: an `ObjDefinition` row (196 bytes, the
/// GUID at +0x1c) points (+0xc0) at an `ObjDefinition::ResData` row (48 bytes: flags at +4, the
/// dataset's id at +0x10, the model's hash at +0x18, a pointer at +0 to a record that starts
/// with a name hash, then its idle animation's id at +8). Definitions without one are left out.
pub fn object_resources(img: &Image) -> BTreeMap<u32, ObjResources> {
    img.rows("ObjDefinition")
        .filter_map(|r| {
            let res = img.ptr(r + 0xc0)?;
            let record = img.ptr(res);
            let states = record.map(|k| (0..MAX_STATES).map(|i| k + 32 * i).take_while(|&e| img.u32(e) == img.u32(k)).filter_map(|e| img.u32(e + 8)).collect()).unwrap_or_default();
            Some((img.u32(r + 0x1c)?, ObjResources { dataset: img.u32(res + 0x10)?, model: img.u32(res + 0x18)?, flags: img.u32(res + 4)?, key: record.and_then(|k| img.u32(k)).unwrap_or(0), idle: record.and_then(|k| img.u32(k + 8)).unwrap_or(0), states }))
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

/// A house's models, from `SimsUI`'s `HouseData` (17 rows of 44 bytes, one a house number, the
/// lot's global 10): its outer lot model (the surroundings, `models.arc` by id, word 9) and its
/// terrain shader (the lot's bare ground, `shaders.arc` by id, word 10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct House {
    pub outer_lot: u32,
    pub terrain: u32,
}

/// The houses by house number (row k is house k).
pub fn houses(img: &Image) -> Vec<House> {
    let Some(t) = img.tables.get("HouseData").copied() else { return Vec::new() };
    if t.row_size < 44 {
        return Vec::new();
    }
    img.rows("HouseData").filter_map(|r| Some(House { outer_lot: img.u32(r + 36)?, terrain: img.u32(r + 40)? })).collect()
}

/// Each house's goals (by house number, as `houses`), each goal's mark (`goals::Goals`): row
/// word 8 points at the house's list of 12-byte goal records (its count in the word before it),
/// a record's word 2 at the goal's definition, whose 16-bit value at +6 is the mark (the
/// original's CompleteGoal, 80211338, reads it as bit (mark & 15) of word (mark >> 4)).
pub fn house_goals(img: &Image) -> Vec<Vec<u16>> {
    let Some(t) = img.tables.get("HouseData").copied() else { return Vec::new() };
    if t.row_size < 44 {
        return Vec::new();
    }
    let mark = |g: usize| Some((img.u32(img.ptr(g + 8)? + 4)? & 0xFFFF) as u16);
    img.rows("HouseData")
        .map(|r| {
            let Some(list) = img.ptr(r + 32) else { return Vec::new() };
            let n = list.checked_sub(4).and_then(|c| img.u32(c)).unwrap_or(0) as usize;
            (0..n).map_while(|k| mark(list + 12 * k)).collect()
        })
        .collect()
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

