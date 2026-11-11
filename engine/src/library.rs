//! The object files a game's scripts run from, and the lookups the scripts make in them: trees
//! by id (`docs/interpreter.md`, "Tree ids") and constants (scope 26).

use std::collections::BTreeMap;

use crate::bhav::{Tree, TreeKind, bhav_order, read_bhav, tree_kind};
use crate::disc::Disc;
use crate::iff::{FormatError, IffFile, Order, Reader, latin1};
use crate::save::SaveStream;
use crate::{FileId, TreeId};

/// An object definition (OBJD): its version and 16-bit fields as the file has them. Fields
/// 12-13 are the GUID (low half first).
#[derive(Clone, Debug, Default)]
pub struct Definition {
    pub id: i16,
    pub version: u32,
    pub fields: Vec<u16>,
}

impl Definition {
    pub fn guid(&self) -> u32 {
        if self.fields.len() > 13 { self.fields[12] as u32 | (self.fields[13] as u32) << 16 } else { 0 }
    }
    pub fn field(&self, i: usize) -> u16 {
        self.fields.get(i).copied().unwrap_or(0)
    }
    /// The interaction table (TTAB chunk id) the object uses, of its file's: field 5. Measured
    /// on the home log's three autonomous choices (`docs/simulation.md`, "Autonomy").
    pub fn interaction_table(&self) -> u16 {
        self.field(5)
    }
    /// Its slots (SLOT chunk id): field 18, 0 for none (1,197 of the 1,681 definitions in files
    /// with a SLOT name one of them, 483 have 0).
    pub fn slot_table(&self) -> u16 {
        self.field(18)
    }
}

/// A definition, by its file and its place among the file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefId {
    pub file: FileId,
    pub index: usize,
}

/// A slot (SLOT, version 10: 70 bytes each, the PC's layout): where a Sim stands to use the
/// object (type 3, a routing slot) or where things go on it (type 0). A routing slot's flags
/// are the directions it may be taken from (1 the front, 2 the front right, ... 128 the front
/// left, turning with the object), at the proximities between min and max (sixteenths of a
/// tile, optimal first); `docs/simulation.md`, "The routing slots".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub kind: u16,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub standing: i32,
    pub sitting: i32,
    pub ground: i32,
    pub flags: i32,
    pub snap: i32,
    pub min_proximity: i32,
    pub max_proximity: i32,
    pub optimal_proximity: i32,
    pub max_size: i32,
    pub gradient: f32,
    pub height: i32,
    pub facing: i32,
    pub resolution: i32,
}

/// What an interaction advertises for a motive: a minimum, a delta and the personality field
/// that scales it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Advert {
    pub min: i16,
    pub delta: i16,
    pub personality: u16,
}

/// An interaction of a table (TTAB): the action and guard trees, its flags, how its appeal
/// falls off with distance (attenuation code and value), the motive level below which a Sim
/// considers it by itself (autonomy threshold), and what it advertises, by motive.
#[derive(Clone, Debug, Default)]
pub struct Interaction {
    pub action: u16,
    pub guard: u16,
    pub flags: u32,
    pub string_index: i32,
    pub attenuation_code: i32,
    pub attenuation_value: f32,
    pub autonomy_threshold: i32,
    pub joining_index: i32,
    pub adverts: Vec<Advert>,
}

#[derive(Default)]
pub struct ObjectFile {
    /// On the disc.
    pub path: String,
    pub iff: IffFile,
    /// BHAV by id.
    pub trees: BTreeMap<u16, TreeId>,
    /// BCON by id.
    pub constants: BTreeMap<u16, Vec<i16>>,
    /// STR# by id (every language's, in turn).
    pub strings: BTreeMap<u16, Vec<String>>,
    /// Named tuning values (FCNS) by chunk id: the global file's motive rates (1), autonomy (2),
    /// rooms and light (3), the family's score (4); `docs/simulation.md`, "The motives".
    pub tuning: BTreeMap<u16, Vec<(String, f32)>>,
    /// BHAV ids by their chunk labels.
    pub tree_by_name: BTreeMap<String, u16>,
    /// From the interaction tables (TTAB): an interaction's action tree -> its guard (test)
    /// tree, and -> the tree its table names to run first (version 11's field for the table).
    pub guard_of: BTreeMap<u16, u16>,
    pub prefix_of: BTreeMap<u16, u16>,
    /// The file's interactions in its table's order (action, guard), if it has one TTAB.
    pub interactions: Vec<(u16, u16)>,
    pub ttabs: usize,
    /// TTAB by chunk id.
    pub tables: BTreeMap<u16, Vec<Interaction>>,
    /// A version-11 table's tree run before its guards, by chunk id.
    pub table_first: BTreeMap<u16, u16>,
    /// OBJD, in the file's order.
    pub definitions: Vec<Definition>,
    /// SLOT by chunk id.
    pub slots: BTreeMap<u16, Vec<Slot>>,
    /// Its function tables (OBJf by chunk id): each function's check tree and action tree (0
    /// for none).
    pub functions: BTreeMap<u16, Vec<(u16, u16)>>,
    /// The semi-global file it names.
    pub glob: String,
    /// That file, if there is one.
    pub semi: Option<FileId>,
}

/// A GLOB's name: length-prefixed ("\rPersonGlobals") or zero-ended ("person_sg").
pub fn glob_name(d: &[u8]) -> String {
    match d.first() {
        None => String::new(),
        Some(&n) if n < 0x20 => {
            if (n as usize) < d.len() { latin1(&d[1..1 + n as usize]) } else { String::new() }
        }
        Some(_) => latin1(&d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]),
    }
}

/// A tuning table (FCNS): 0, 1, "SNCF", the entry count, then each entry a name, a float and a
/// description, the strings zero-ended and padded to an even length (with 0xCD). Empty if the
/// chunk does not read to exactly its end.
fn read_tuning(d: &[u8], o: Order) -> Vec<(String, f32)> {
    let read = || -> Option<Vec<(String, f32)>> {
        if d.len() < 16 || &d[8..12] != b"SNCF" {
            return None;
        }
        let mut r = Reader::new(d, o);
        r.take(12).ok()?;
        let count = r.u32().ok()?;
        let mut p = 16;
        let zstring = |p: &mut usize| -> Option<String> {
            if *p >= d.len() {
                return None;
            }
            let n = d[*p..].iter().position(|&c| c == 0)?;
            let s = latin1(&d[*p..*p + n]);
            *p += n + 1;
            *p += *p & 1;
            Some(s)
        };
        let mut out = Vec::new();
        for _ in 0..count {
            let name = zstring(&mut p)?;
            if p + 4 > d.len() {
                return None;
            }
            let mut w = Reader::new(&d[p..p + 4], o);
            let v = w.f32().ok()?;
            p += 4;
            zstring(&mut p)?;
            out.push((name, v));
        }
        (p == d.len()).then_some(out)
    };
    read().unwrap_or_default()
}

/// A string table (STR#): its format (0 Pascal strings, -1 C strings, -2 pairs of C strings, -3
/// a language, a value and a comment, -4 sets by language of Pascal values and comments), then
/// the strings; the values, each language's in turn. A table that does not read is empty.
fn read_strings(d: &[u8], o: Order) -> Vec<String> {
    let mut out = Vec::new();
    if d.len() < 4 {
        return out;
    }
    let len = d.len();
    let mut p = 2usize;
    let cstr = |p: &mut usize| {
        let start = (*p).min(len);
        let q = start + d[start..].iter().position(|&c| c == 0).unwrap_or(len - start);
        let s = latin1(&d[start..q]);
        *p = (q + 1).min(len);
        s
    };
    let pstr = |p: &mut usize| {
        if *p >= len {
            return String::new();
        }
        let n = d[*p] as usize;
        *p += 1;
        let s = latin1(&d[*p..*p + n.min(len - *p)]);
        *p += n;
        s
    };
    let u16 = |p: &mut usize| -> i32 {
        if *p + 2 > len {
            return 0;
        }
        let v = match o {
            Order::Little => u16::from_le_bytes([d[*p], d[*p + 1]]),
            Order::Big => u16::from_be_bytes([d[*p], d[*p + 1]]),
        };
        *p += 2;
        v as i32
    };
    let fmt = match o {
        Order::Little => i16::from_le_bytes([d[0], d[1]]),
        Order::Big => i16::from_be_bytes([d[0], d[1]]),
    };
    if fmt == -4 {
        let sets = if p < len {
            p += 1;
            d[p - 1]
        } else {
            0
        };
        for _ in 0..sets {
            let mut n = u16(&mut p);
            while n > 0 && p < len {
                p += 1;
                out.push(pstr(&mut p));
                pstr(&mut p);
                n -= 1;
            }
        }
        return out;
    }
    let mut n = u16(&mut p);
    while n > 0 && p < len {
        match fmt {
            0 => out.push(pstr(&mut p)),
            -1 => out.push(cstr(&mut p)),
            -2 => {
                out.push(cstr(&mut p));
                cstr(&mut p);
            }
            -3 => {
                p += 1;
                out.push(cstr(&mut p));
                cstr(&mut p);
            }
            _ => break,
        }
        n -= 1;
    }
    out
}

/// What one TTAB chunk gives.
#[derive(Default)]
struct TtabRead {
    list: Vec<(u16, u16)>,
    table: Vec<Interaction>,
    first: u16,
}

/// An interaction table (TTAB): each interaction's action and guard trees (`docs/interpreter.md`;
/// `tools/chunks.py` reads the rest). Versions below 9 are plain fields in the file's order; 9
/// and up are packed as the save stream is (a code byte 1, then its compressed fields; 11 opens
/// with one field for the table). A table that stops reading keeps what it read before.
fn read_ttab(d: &[u8], o: Order, guard_of: &mut BTreeMap<u16, u16>, prefix_of: &mut BTreeMap<u16, u16>) -> TtabRead {
    let mut t = TtabRead::default();
    let _ = (|| -> Result<(), FormatError> {
        let mut r = Reader::new(d, o);
        let n = r.u16()?;
        if n == 0 || n > 4096 {
            return Ok(());
        }
        let version = r.u16()?;
        if version < 9 {
            for _ in 0..n {
                let mut e = Interaction { action: r.u16()?, guard: r.u16()?, ..Default::default() };
                let motives;
                if version <= 3 {
                    motives = r.u16()? as u32;
                    e.flags = r.u16()? as u32;
                } else {
                    motives = r.u32()?;
                    e.flags = r.u32()?;
                    e.string_index = r.u32()? as i32;
                }
                if motives > 64 {
                    return Ok(());
                }
                if version > 6 {
                    e.attenuation_code = r.u32()? as i32;
                }
                e.attenuation_value = r.f32()?;
                if version > 3 {
                    e.autonomy_threshold = r.u32()? as i32;
                    e.joining_index = r.u32()? as i32;
                }
                for _ in 0..motives {
                    e.adverts.push(if version > 6 {
                        Advert { min: r.s16()?, delta: r.s16()?, personality: r.u16()? }
                    } else {
                        Advert { delta: r.s16()?, ..Default::default() }
                    });
                }
                t.list.push((e.action, e.guard));
                if e.guard != 0 {
                    guard_of.entry(e.action).or_insert(e.guard);
                }
                t.table.push(e);
            }
            return Ok(());
        }
        if r.u8()? != 1 {
            return Ok(());
        }
        let mut f = SaveStream::new(&d[r.pos()..], 0);
        f.compressed = true;
        let first = if version >= 11 { f.r16()? as u16 } else { 0 };
        t.first = first;
        for _ in 0..n {
            let mut e = Interaction { action: f.r16()? as u16, guard: f.r16()? as u16, ..Default::default() };
            if first != 0 {
                prefix_of.entry(e.action).or_insert(first);
            }
            let motives = f.r32()?;
            if !(0..=64).contains(&motives) {
                return Ok(());
            }
            e.flags = f.r32()? as u32;
            e.string_index = f.r32()?;
            e.attenuation_code = f.r32()?;
            e.attenuation_value = f32::from_bits(f.r32()? as u32);
            e.autonomy_threshold = f.r32()?;
            e.joining_index = f.r32()?;
            for _ in 0..motives {
                e.adverts.push(Advert { min: f.r16()? as i16, delta: f.r16()? as i16, personality: f.r16()? as u16 });
            }
            t.list.push((e.action, e.guard));
            if e.guard != 0 {
                guard_of.entry(e.action).or_insert(e.guard);
            }
            t.table.push(e);
        }
        Ok(())
    })();
    t
}

/// An OBJf chunk: 0, the version, "OBJf", the count, then a check tree and an action tree each.
fn read_functions(d: &[u8], o: Order) -> Option<Vec<(u16, u16)>> {
    let mut r = Reader::new(d, o);
    let mut read = || -> Result<Vec<(u16, u16)>, FormatError> {
        r.u32()?;
        r.u32()?;
        r.u32()?;
        let n = r.u32()?;
        (0..n).map(|_| Ok((r.u16()?, r.u16()?))).collect()
    };
    read().ok()
}

/// A SLOT chunk: 0, the version, "SLOT", the count, then the slots; `None` unless version 10
/// read to its end.
fn read_slots(d: &[u8], o: Order) -> Option<Vec<Slot>> {
    let mut r = Reader::new(d, o);
    let read = |r: &mut Reader| -> Result<Option<Vec<Slot>>, FormatError> {
        r.u32()?;
        let version = r.u32()?;
        r.u32()?;
        let n = r.u32()?;
        if version != 10 {
            return Ok(None);
        }
        let mut out = Vec::new();
        for _ in 0..n {
            let mut s = Slot { kind: r.u16()?, x: r.f32()?, y: r.f32()?, z: r.f32()?, ..Default::default() };
            s.standing = r.u32()? as i32;
            s.sitting = r.u32()? as i32;
            s.ground = r.u32()? as i32;
            s.flags = r.u32()? as i32;
            s.snap = r.u32()? as i32;
            s.min_proximity = r.u32()? as i32;
            s.max_proximity = r.u32()? as i32;
            s.optimal_proximity = r.u32()? as i32;
            s.max_size = r.u32()? as i32;
            r.u32()?; // always 0 on the disc
            s.gradient = r.f32()?;
            s.height = r.u32()? as i32;
            s.facing = r.u32()? as i32;
            s.resolution = r.u32()? as i32;
            out.push(s);
        }
        r.done()?;
        Ok(Some(out))
    };
    read(&mut r).ok().flatten()
}

/// An object's interaction table: the file it is in and its TTAB chunk id, and that table's
/// first tree (0 if none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectTable {
    pub file: Option<FileId>,
    pub table: Option<u16>,
    pub first: u16,
}

#[derive(Default)]
pub struct Library {
    files: Vec<ObjectFile>,
    trees: Vec<Tree>,
    by_name: BTreeMap<String, FileId>,
    by_guid: BTreeMap<u32, DefId>,
    global: Option<FileId>,
    bhav_failures: usize,
    tuning_failures: usize,
    slot_failures: usize,
}

impl Library {
    /// Bustin' Out's: every IFF file under Runtime/. Their semi-global files are linked by name,
    /// except that the characters' PersonGlobals is person_sg, which the game uses
    /// (`docs/interpreter.md`, "Semi-global files").
    pub fn bustin_out(d: &Disc) -> Library {
        let mut lib = Library::default();
        for path in d.files().keys() {
            if !path.starts_with("Runtime/") || path.len() < 5 || !path[path.len() - 4..].eq_ignore_ascii_case(".iff") {
                continue;
            }
            let mut f = ObjectFile {
                path: path.clone(),
                iff: IffFile::parse(d.read(path).unwrap_or_default()),
                ..Default::default()
            };
            let iff = &f.iff;
            let o = bhav_order(iff);
            for c in iff.of(b"BHAV") {
                match read_bhav(c.id, iff.data(c), o) {
                    Ok(t) => {
                        f.trees.insert(c.id as u16, TreeId(lib.trees.len() as u32));
                        lib.trees.push(t);
                    }
                    Err(_) => lib.bhav_failures += 1,
                }
                f.tree_by_name.entry(c.label.clone()).or_insert(c.id as u16);
            }
            for c in iff.of(b"STR#") {
                f.strings.insert(c.id as u16, read_strings(iff.data(c), o));
            }
            for c in iff.of(b"FCNS") {
                let t = read_tuning(iff.data(c), o);
                if t.is_empty() {
                    lib.tuning_failures += 1;
                } else {
                    f.tuning.insert(c.id as u16, t);
                }
            }
            for c in iff.of(b"TTAB") {
                let t = read_ttab(iff.data(c), o, &mut f.guard_of, &mut f.prefix_of);
                if !t.table.is_empty() {
                    f.tables.insert(c.id as u16, t.table);
                }
                if t.first != 0 {
                    f.table_first.insert(c.id as u16, t.first);
                }
                // Several tables: which an object uses is not known yet.
                f.interactions = if f.ttabs == 0 { t.list } else { Vec::new() };
                f.ttabs += 1;
            }
            for c in iff.of(b"BCON") {
                // A count, a flags byte, then the values.
                let data = iff.data(c);
                if data.len() < 2 {
                    continue;
                }
                let mut r = Reader::new(data, o);
                let n = r.u8().unwrap_or(0);
                let _ = r.u8();
                let mut vals = Vec::new();
                while vals.len() < n as usize && r.left() >= 2 {
                    vals.push(r.s16().unwrap());
                }
                f.constants.insert(c.id as u16, vals);
            }
            for c in iff.of(b"OBJf") {
                if let Some(t) = read_functions(iff.data(c), o) {
                    f.functions.insert(c.id as u16, t);
                }
            }
            for c in iff.of(b"SLOT") {
                match read_slots(iff.data(c), o) {
                    Some(s) => {
                        f.slots.insert(c.id as u16, s);
                    }
                    None => lib.slot_failures += 1,
                }
            }
            for c in iff.of(b"GLOB") {
                f.glob = glob_name(iff.data(c));
            }
            for c in iff.of(b"OBJD") {
                let mut r = Reader::new(iff.data(c), o);
                if r.left() < 4 {
                    continue;
                }
                let version = r.u32().unwrap();
                let mut fields = Vec::with_capacity(r.left() / 2);
                while r.left() >= 2 {
                    fields.push(r.u16().unwrap());
                }
                f.definitions.push(Definition { id: c.id, version, fields });
            }
            // The first of a name.
            let slash = path.rfind('/').map_or(0, |s| s + 1);
            let id = FileId(lib.files.len() as u32);
            lib.by_name.entry(path[slash..path.len() - 4].to_ascii_lowercase()).or_insert(id);
            lib.files.push(f);
        }
        for k in 0..lib.files.len() {
            let mut g = lib.files[k].glob.to_ascii_lowercase();
            if g == "personglobals" {
                g = "person_sg".into();
            }
            if !g.is_empty() {
                lib.files[k].semi = lib.find(&g);
            }
        }
        lib.global = lib.find("global");
        for (k, f) in lib.files.iter().enumerate() {
            for index in 0..f.definitions.len() {
                lib.by_guid.entry(f.definitions[index].guid()).or_insert(DefId { file: FileId(k as u32), index });
            }
        }
        lib
    }

    pub fn files(&self) -> &[ObjectFile] {
        &self.files
    }
    pub fn file(&self, f: FileId) -> &ObjectFile {
        &self.files[f.0 as usize]
    }
    pub fn tree_at(&self, t: TreeId) -> &Tree {
        &self.trees[t.0 as usize]
    }
    pub fn global(&self) -> Option<FileId> {
        self.global
    }
    pub fn global_file(&self) -> Option<&ObjectFile> {
        self.global.map(|g| self.file(g))
    }
    /// By file name, any case, no .iff.
    pub fn find(&self, name: &str) -> Option<FileId> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }
    pub fn bhav_failures(&self) -> usize {
        self.bhav_failures
    }
    /// FCNS chunks not read to their end.
    pub fn tuning_failures(&self) -> usize {
        self.tuning_failures
    }
    /// SLOT chunks not of version 10 or cut short.
    pub fn slot_failures(&self) -> usize {
        self.slot_failures
    }

    /// A named tuning value of the global file's (FCNS), if it has one.
    pub fn tuning(&self, name: &str) -> Option<f32> {
        let g = self.global_file()?;
        g.tuning.values().flatten().find(|(n, _)| n == name).map(|&(_, v)| v)
    }

    /// The interaction table (its TTAB id in `file`) a definition uses: its field 5 names it; a
    /// multi-tile object's part (0xFFFF there) uses its master definition's (the file's
    /// definition with the same master id, field 8, and a table), and a file with one table
    /// that one. `None` if none.
    pub fn interactions_of(&self, d: &Definition, file: FileId) -> Option<u16> {
        let f = self.file(file);
        let mut id = d.interaction_table();
        if id == 0xFFFF
            && d.field(8) != 0
            && let Some(m) = f.definitions.iter().find(|m| m.field(8) == d.field(8) && m.interaction_table() != 0xFFFF)
        {
            id = m.interaction_table();
        }
        if id == 0xFFFF && f.tables.len() == 1 {
            return f.tables.keys().next().copied();
        }
        f.tables.contains_key(&id).then_some(id)
    }

    /// The interactions of a table [`interactions_of`](Self::interactions_of) or
    /// [`table_of`](Self::table_of) named.
    pub fn table(&self, file: FileId, id: u16) -> &[Interaction] {
        &self.file(file).tables[&id]
    }

    /// An object's table (its definition's, or a person's semi-global file's), the file it is
    /// in, and that table's first tree (0 if none).
    pub fn table_of(&self, file: Option<FileId>, person: bool, definition: Option<&Definition>) -> ObjectTable {
        let mut t = ObjectTable::default();
        let Some(file) = file else { return t };
        if person {
            t.file = self.file(file).semi;
            if let Some(s) = t.file.map(|s| self.file(s)).filter(|s| s.tables.len() == 1) {
                t.table = s.tables.keys().next().copied();
            }
        } else if let Some(d) = definition {
            t.file = Some(file);
            t.table = self.interactions_of(d, file);
        }
        if let (Some(f), Some(id)) = (t.file, t.table) {
            t.first = self.file(f).table_first.get(&id).copied().unwrap_or(0);
        }
        t
    }

    /// A definition by GUID (the first file's if several have it).
    pub fn definition(&self, guid: u32) -> Option<DefId> {
        self.by_guid.get(&guid).copied()
    }
    pub fn def(&self, d: DefId) -> &Definition {
        &self.file(d.file).definitions[d.index]
    }

    /// A tree as a script of `object` names it: global, the object's own, or its semi-global
    /// file's, by the id's range.
    pub fn tree(&self, object: Option<FileId>, id: u16) -> Option<TreeId> {
        let f = match tree_kind(id) {
            TreeKind::Global => self.global,
            TreeKind::Own => object,
            TreeKind::SemiGlobal => object.and_then(|o| self.file(o).semi),
            _ => None,
        }?;
        self.file(f).trees.get(&id).copied()
    }

    /// A constant as `object`'s scripts name it: the datum's top 9 bits a table (0-63 the
    /// object's own BCON 4096 on, 64-127 its semi-global file's 8192 on, 128 on the global
    /// file's 256 on), its low 7 bits the entry. Checked against the recompiled game.
    pub fn constant(&self, object: Option<FileId>, data: i16) -> Option<i16> {
        let table = (data as u16 >> 7) as u32;
        let entry = (data & 0x7F) as usize;
        let (f, id) = match table {
            0..64 => (object, 4096 + table),
            // the file's semi-global file, or the file itself when it is one (a semi-global
            // tree's frame)
            64..128 => (object.and_then(|o| self.file(o).semi.or(Some(o))), 8192 + table - 64),
            _ => (self.global, 256 + table - 128),
        };
        self.file(f?).constants.get(&(id as u16))?.get(entry).copied()
    }

    /// A slot a GotoRoutingSlot (opcode 45) names: its operand's first two 16-bit fields, the
    /// number and where it is from (1 the stack object's own SLOT, 2 the global file's); the
    /// number counts the routing slots (type 3) alone. 0 (a stack variable) is not modelled.
    pub fn routing_slot(&self, file: FileId, def: Option<&Definition>, scope: u16, number: u16) -> Option<&Slot> {
        let table = match scope {
            1 => self.file(file).slots.get(&def?.slot_table())?,
            2 => {
                let g = self.global_file()?;
                if g.slots.len() != 1 {
                    return None;
                }
                g.slots.values().next()?
            }
            _ => return None,
        };
        table.iter().filter(|s| s.kind == 3).nth(number as usize)
    }
}
