//! The menus' and HUD's movies (Apt): `flashes.arc`'s bundles read into plain data, and
//! `fonts.arc`'s fonts (`docs/assets.md`, "Menus and HUD (Apt)"). Read only: nothing here plays
//! a movie.
//!
//! Every reader keeps what it did not understand as text in an `unknown` list: bytes nothing
//! read, a kind or an opcode it has no meaning for. An empty list is a file read in full.

pub mod actions;
pub mod bundle;
pub mod display;
pub mod dump;
pub mod font;
pub mod geometry;
mod img;
pub mod movie;
pub mod play;
pub mod player;
pub mod texture;
pub mod value;
mod vm;

use std::collections::BTreeMap;

use actions::Instr;
use geometry::{Fill, Geometry};
use img::Res;
use movie::{Character, Item, Movie};
use texture::Image;

/// One bundle of `flashes.arc`: a movie with its shapes and images.
#[derive(Clone, Debug)]
pub struct Flash {
    pub name: String,
    pub movie: Movie,
    pub geometry: Geometry,
    /// The images by character id: the header, and the file's bytes it points into.
    pub images: BTreeMap<u32, (Image, Vec<u8>)>,
    /// Everything not accounted for: the movie's, the shapes', the bundle's, and links between
    /// them that lead nowhere.
    pub unknown: Vec<String>,
}

/// Reads one `flashes.arc` entry.
pub fn read_flash(name: &str, data: &[u8]) -> Res<Flash> {
    let b = bundle::Bundle::read(data)?;
    let mut unknown = b.unread.clone();
    let (mut apt, mut konst, mut geo) = (None, None, None);
    let mut images = BTreeMap::new();
    for f in &b.files {
        let bytes = b.bytes(data, f);
        match f.name.rsplit_once('.') {
            Some((n, "apt")) if n == name => apt = Some(bytes),
            Some((n, "const")) if n == name => konst = Some(bytes),
            Some((n, "geo")) if n == name => geo = Some(bytes),
            Some((n, "tgq")) if n.len() == 3 && n.bytes().all(|c| c.is_ascii_digit()) => {
                let id: u32 = n.parse().unwrap();
                match Image::read(bytes) {
                    Ok(i) => {
                        images.insert(id, (i, bytes.to_vec()));
                    }
                    Err(e) => unknown.push(format!("image {id}: {e}")),
                }
            }
            Some(("*", "wav")) if f.size == 0 => {}
            _ => unknown.push(format!("file {} ({} bytes) is of no kind this reader knows", f.name, f.size)),
        }
    }
    let (Some(apt), Some(konst), Some(geo)) = (apt, konst, geo) else {
        return Err("the bundle lacks its .apt, .const or .geo".into());
    };
    let movie = Movie::read(apt, konst)?;
    let geometry = Geometry::read(geo)?;
    unknown.extend(movie.unknown.iter().cloned());
    unknown.extend(geometry.unknown.iter().cloned());
    // the links: shapes to geometry, images to files, exports to characters
    for (i, c) in movie.characters.iter().enumerate() {
        match c {
            Character::Shape { geometry: g, .. } if geometry.shape(*g).is_none() => unknown.push(format!("shape character {i} names geometry {g}, which the .geo lacks")),
            Character::Image { image } if !images.contains_key(image) => unknown.push(format!("image character {i} names image {image}, which the bundle lacks")),
            _ => {}
        }
    }
    for s in &geometry.shapes {
        for f in &s.fills {
            if let Fill::Textured { image, .. } = f
                && !images.contains_key(image)
            {
                unknown.push(format!("geometry {} fills from image {image}, which the bundle lacks", s.id));
            }
        }
    }
    for e in &movie.exports {
        if e.id as usize >= movie.characters.len() {
            unknown.push(format!("export {} names character {}, past the {} there are", e.name, e.id, movie.characters.len()));
        }
    }
    Ok(Flash { name: name.to_string(), movie, geometry, images, unknown })
}

/// What a movie holds, counted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub characters: BTreeMap<&'static str, usize>,
    pub frames: usize,
    pub items: BTreeMap<&'static str, usize>,
    pub instructions: usize,
    /// Each opcode's name or code, by use.
    pub opcodes: BTreeMap<String, usize>,
    pub shapes: usize,
    pub fills: usize,
    pub triangles: usize,
    pub images: usize,
    pub image_texels: usize,
    pub unknown: usize,
}

impl Stats {
    pub fn add(&mut self, o: &Stats) {
        for (k, v) in &o.characters {
            *self.characters.entry(k).or_default() += v;
        }
        for (k, v) in &o.items {
            *self.items.entry(k).or_default() += v;
        }
        for (k, v) in &o.opcodes {
            *self.opcodes.entry(k.clone()).or_default() += v;
        }
        self.frames += o.frames;
        self.instructions += o.instructions;
        self.shapes += o.shapes;
        self.fills += o.fills;
        self.triangles += o.triangles;
        self.images += o.images;
        self.image_texels += o.image_texels;
        self.unknown += o.unknown;
    }
}

/// The opcode a name or number stands for in `Stats::opcodes`.
fn opcode_name(i: &Instr) -> String {
    match i {
        Instr::Op(o) => format!("{o:?}"),
        Instr::GotoFrame(_) => "GotoFrame".into(),
        Instr::GotoFrame2(_) => "GotoFrame2".into(),
        Instr::SetRegister(_) => "SetRegister".into(),
        Instr::ConstantPool(_) => "ConstantPool".into(),
        Instr::Push(_) => "Push".into(),
        Instr::BranchAlways(_) => "BranchAlways".into(),
        Instr::BranchIfTrue(_) => "BranchIfTrue".into(),
        Instr::With { .. } => "With".into(),
        Instr::DefineFunction { .. } => "DefineFunction".into(),
        Instr::End => "End".into(),
        Instr::Unknown(c) => format!("?{c:#x}"),
    }
}

fn count_instrs(v: &[Instr], s: &mut Stats) {
    for i in v {
        s.instructions += 1;
        *s.opcodes.entry(opcode_name(i)).or_default() += 1;
        if let Instr::DefineFunction { body, .. } = i {
            count_instrs(body, s);
        }
    }
}

fn count_frames(frames: &[movie::Frame], s: &mut Stats) {
    for f in frames {
        s.frames += 1;
        for it in &f.items {
            let kind = match it {
                Item::Action(a) => {
                    count_instrs(a, s);
                    "action"
                }
                Item::Label { .. } => "label",
                Item::Place(p) => {
                    for c in &p.clip_actions {
                        count_instrs(&c.actions, s);
                    }
                    "place"
                }
                Item::Remove { .. } => "remove",
                Item::Background(_) => "background",
                Item::Unknown(_) => "unknown",
            };
            *s.items.entry(kind).or_default() += 1;
        }
    }
}

impl Flash {
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        count_frames(&self.movie.frames, &mut s);
        for c in &self.movie.characters {
            let kind = match c {
                Character::Movie => "movie",
                Character::Shape { .. } => "shape",
                Character::Image { .. } => "image",
                Character::Sprite { frames } => {
                    count_frames(frames, &mut s);
                    "sprite"
                }
                Character::Text(_) => "text",
                Character::Font { .. } => "font",
                Character::Button(_) => "button",
                Character::None => "null",
                Character::Unknown(_) => "unknown",
            };
            *s.characters.entry(kind).or_default() += 1;
        }
        s.shapes = self.geometry.shapes.len();
        for sh in &self.geometry.shapes {
            s.fills += sh.fills.len();
            for f in &sh.fills {
                s.triangles += match f {
                    Fill::Solid { triangles, .. } => triangles.len(),
                    Fill::Textured { triangles, .. } => triangles.len(),
                    _ => 0,
                };
            }
        }
        s.images = self.images.len();
        s.image_texels = self.images.values().map(|(i, _)| (i.width * i.height) as usize).sum();
        s.unknown = self.unknown.len();
        s
    }
}

/// Reads every bundle of a `flashes.arc`.
pub fn read_flashes(arc_data: &[u8]) -> Vec<(String, Res<Flash>)> {
    crate::arc::entries(arc_data)
        .unwrap_or_default()
        .iter()
        .map(|e| (e.name.clone(), read_flash(&e.name, crate::arc::bytes(arc_data, e))))
        .collect()
}

/// Reads every font of a `fonts.arc`.
pub fn read_fonts(arc_data: &[u8]) -> Vec<(String, Res<font::Font>)> {
    crate::arc::entries(arc_data).unwrap_or_default().iter().map(|e| (e.name.clone(), font::Font::read(crate::arc::bytes(arc_data, e)))).collect()
}

#[cfg(test)]
mod tests;
