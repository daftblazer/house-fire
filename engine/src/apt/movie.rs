//! A movie: the `.apt` file's characters (shapes, images, sprites, text, buttons), its frames
//! of items (actions, labels, placing and removing characters), and the `.const` file's
//! constants (`docs/assets.md`, "Menus and HUD (Apt)").
//!
//! The `.apt` is a memory image: records point to each other by file offset, an array is a
//! count and a pointer (an empty one points wherever the next thing is), a string is
//! NUL-ended and padded to a word. Every record kind starts with its kind word and the
//! signature 0x09876543.

use super::actions::{self, Instr, Value};
use super::img::{Img, Res};

/// The word after each character's kind.
const SIGNATURE: u32 = 0x0987_6543;

/// Place-object flags (`Place::flags`): which of the record's fields the item sets.
pub mod place {
    pub const MOVE: u32 = 0x01;
    pub const HAS_CHARACTER: u32 = 0x02;
    pub const HAS_MATRIX: u32 = 0x04;
    pub const HAS_COLOR: u32 = 0x08;
    pub const HAS_RATIO: u32 = 0x10;
    pub const HAS_NAME: u32 = 0x20;
    pub const HAS_CLIP_DEPTH: u32 = 0x40;
    pub const HAS_CLIP_ACTIONS: u32 = 0x80;
}

/// A bounding box: left, top, right, bottom.
pub type Bounds = [f32; 4];

/// Places or moves a character in the display list (`flags` says which fields are meant).
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub flags: u32,
    pub depth: i32,
    /// The character id, or -1.
    pub character: i32,
    /// The 2 by 2 part of the matrix: a, b, c, d as the Flash format has them.
    pub matrix: [f32; 4],
    pub translation: [f32; 2],
    /// The colour multiplier, 0xRRGGBBAA (all ones when not set).
    pub color: u32,
    /// The colour offset (0x00RRGGBB).
    pub color_add: u32,
    /// The ratio of a morph (0 when not set).
    pub ratio: f32,
    pub name: Option<String>,
    /// The clip depth, -1 for none.
    pub clip_depth: i32,
    pub clip_actions: Vec<ClipAction>,
}

/// Actions a placed clip runs on events.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipAction {
    /// The event flags word.
    pub events: u32,
    /// The key code word (0 in the one movie that has them).
    pub key: u32,
    pub actions: Vec<Instr>,
}

/// What a frame holds, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Action(Vec<Instr>),
    /// Names the frame; `flags` is 7 in every one.
    Label { name: String, flags: u32, frame: u32 },
    Place(Box<Place>),
    Remove { depth: i32 },
    /// The stage colour, 0xRRGGBBAA.
    Background(u32),
    /// An item kind this reader has no meaning for.
    Unknown(u32),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Frame {
    pub items: Vec<Item>,
}

/// One state of a button: a character placed.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonRecord {
    pub flags: u32,
    pub character: u32,
    pub depth: u32,
    pub matrix: [f32; 4],
    pub translation: [f32; 2],
    pub color: [f32; 4],
    pub color_add: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub is_menu: u32,
    pub bounds: Bounds,
    /// The hit area.
    pub vertices: Vec<[f32; 2]>,
    pub triangles: Vec<[u16; 3]>,
    pub records: Vec<ButtonRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub bounds: Bounds,
    /// The character id of a font (`Character::Font`).
    pub font: u32,
    pub align: u32,
    /// 0xRRGGBBAA.
    pub color: u32,
    pub height: f32,
    pub read_only: bool,
    pub multiline: bool,
    pub word_wrap: bool,
    pub initial_text: Option<String>,
    /// The variable the text is bound to.
    pub variable: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Character {
    /// Character 0, the movie itself.
    Movie,
    Shape { bounds: Bounds, geometry: u32 },
    Image { image: u32 },
    Sprite { frames: Vec<Frame> },
    Text(Text),
    /// A font the texts name, by its `fonts.arc` entry.
    Font { name: String },
    Button(Button),
    /// A null entry.
    None,
    /// A kind this reader has no meaning for.
    Unknown(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Export {
    pub name: String,
    pub id: u32,
}

#[derive(Clone, Debug)]
pub struct Movie {
    /// The stage size.
    pub width: u32,
    pub height: u32,
    /// Milliseconds a frame lasts.
    pub ms_per_frame: u32,
    pub frames: Vec<Frame>,
    /// By character id.
    pub characters: Vec<Character>,
    pub exports: Vec<Export>,
    /// The `.const` file's constants.
    pub consts: Vec<Value>,
    /// What this reader could not account for.
    pub unknown: Vec<String>,
}

/// Reads the constant file: the magic, where the movie is in the `.apt`, the count, where the
/// table is; the table's (type, value) pairs. Types: 1 a string (the value points to it in this
/// file), 3 undefined, 4 a register, 5 a boolean, 6 a float, 7 an integer, 8 a constant pool
/// index.
pub fn read_consts(data: &[u8]) -> Res<(usize, Vec<Value>, Vec<String>)> {
    let mut m = Img::new(data);
    if m.take(0, 20)? != b"Apt constant file\x1a\0\0" {
        return Err("not a constant file".into());
    }
    let (movie, n, table) = (m.ptr(0x14)?, m.u32(0x18)? as usize, m.ptr(0x1c)?);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (t, v) = (m.u32(table + 8 * i)?, m.u32(table + 8 * i + 4)?);
        out.push(match t {
            1 => Value::Str(m.cstr(v as usize)?),
            3 if v == 0 => Value::Undefined,
            4 => Value::Register(v),
            5 if v <= 1 => Value::Bool(v == 1),
            6 => Value::Float(f32::from_bits(v)),
            7 => Value::Int(v as i32),
            8 => Value::Pool(v),
            _ => Value::Unknown(t, v),
        });
    }
    Ok((movie, out, m.unread("constants")))
}

struct Reader<'a, 'b> {
    m: Img<'a>,
    consts: &'b [Value],
    unknown: Vec<String>,
}

impl Reader<'_, '_> {
    fn bounds(&mut self, p: usize) -> Res<Bounds> {
        self.m.floats::<4>(p)
    }

    fn actions(&mut self, p: usize) -> Res<Vec<Instr>> {
        let v = actions::decode(&mut self.m, self.consts, p, None)?;
        self.note_unknown(&v, p);
        Ok(v)
    }

    fn note_unknown(&mut self, v: &[Instr], at: usize) {
        for i in v {
            match i {
                Instr::Unknown(op) => self.unknown.push(format!("opcode {op:#x} in the stream at {at:#x}")),
                Instr::DefineFunction { body, .. } => self.note_unknown(body, at),
                Instr::Push(vs) | Instr::ConstantPool(vs) => {
                    for v in vs {
                        if let Value::Unknown(t, x) = v {
                            self.unknown.push(format!("constant of type {t} value {x:#x} in the stream at {at:#x}"));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A frame list: `count` frames of (item count, pointer to the item pointers).
    fn frames(&mut self, count: usize, p: usize) -> Res<Vec<Frame>> {
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let (n, list) = (self.m.u32(p + 8 * i)? as usize, self.m.ptr(p + 8 * i + 4)?);
            let mut items = Vec::with_capacity(n);
            for j in 0..n {
                let at = self.m.ptr(list + 4 * j)?;
                items.push(self.item(at)?);
            }
            out.push(Frame { items });
        }
        Ok(out)
    }

    fn item(&mut self, p: usize) -> Res<Item> {
        let kind = self.m.u32(p)?;
        Ok(match kind {
            1 => {
                let at = self.m.ptr(p + 4)?;
                Item::Action(self.actions(at)?)
            }
            2 => {
                let name = self.m.opt_str(p + 4)?.unwrap_or_default();
                Item::Label { name, flags: self.m.u32(p + 8)?, frame: self.m.u32(p + 12)? }
            }
            3 => Item::Place(Box::new(self.place(p)?)),
            4 => Item::Remove { depth: self.m.i32(p + 4)? },
            5 => Item::Background(self.m.u32(p + 4)?),
            k => {
                self.unknown.push(format!("item kind {k} at {p:#x}"));
                Item::Unknown(k)
            }
        })
    }

    fn place(&mut self, p: usize) -> Res<Place> {
        let m = &mut self.m;
        let (flags, depth, character) = (m.u32(p + 4)?, m.i32(p + 8)?, m.i32(p + 12)?);
        let matrix = m.floats::<4>(p + 16)?;
        let translation = m.floats::<2>(p + 32)?;
        let (color, color_add, ratio) = (m.u32(p + 40)?, m.u32(p + 44)?, m.f32(p + 48)?);
        let name = m.opt_str(p + 52)?;
        let clip_depth = m.i32(p + 56)?;
        let clips = m.ptr(p + 60)?;
        let mut clip_actions = Vec::new();
        if clips != 0 {
            let (n, list) = (m.u32(clips)? as usize, m.ptr(clips + 4)?);
            let mut recs = Vec::new();
            for i in 0..n {
                let r = list + 12 * i;
                recs.push((m.u32(r)?, m.u32(r + 4)?, m.ptr(r + 8)?));
            }
            for (events, key, at) in recs {
                clip_actions.push(ClipAction { events, key, actions: self.actions(at)? });
            }
        }
        Ok(Place { flags, depth, character, matrix, translation, color, color_add, ratio, name, clip_depth, clip_actions })
    }

    fn button(&mut self, p: usize) -> Res<Button> {
        let m = &mut self.m;
        let is_menu = m.u32(p + 8)?;
        let bounds = m.floats::<4>(p + 12)?;
        let (tris, verts, vptr, tptr) = (m.u32(p + 28)? as usize, m.u32(p + 32)? as usize, m.ptr(p + 36)?, m.ptr(p + 40)?);
        let (nrec, rptr, nact, aptr, zero) = (m.u32(p + 44)? as usize, m.ptr(p + 48)?, m.u32(p + 52)?, m.ptr(p + 56)?, m.u32(p + 60)?);
        let _ = aptr;
        if nact != 0 || zero != 0 {
            self.unknown.push(format!("button at {p:#x} has {nact} actions and a last word {zero:#x}"));
        }
        let m = &mut self.m;
        let mut vertices = Vec::with_capacity(verts);
        for i in 0..verts {
            vertices.push(m.floats::<2>(vptr + 8 * i)?);
        }
        let mut triangles = Vec::with_capacity(tris);
        for i in 0..tris {
            triangles.push([m.u16(tptr + 6 * i)?, m.u16(tptr + 6 * i + 2)?, m.u16(tptr + 6 * i + 4)?]);
        }
        let mut records = Vec::with_capacity(nrec);
        for i in 0..nrec {
            let r = rptr + 68 * i;
            records.push(ButtonRecord {
                flags: m.u32(r)?,
                character: m.u32(r + 4)?,
                depth: m.u32(r + 8)?,
                matrix: m.floats::<4>(r + 12)?,
                translation: m.floats::<2>(r + 28)?,
                color: m.floats::<4>(r + 36)?,
                color_add: m.floats::<4>(r + 52)?,
            });
        }
        Ok(Button { is_menu, bounds, vertices, triangles, records })
    }

    fn text(&mut self, p: usize) -> Res<Text> {
        let m = &mut self.m;
        Ok(Text {
            bounds: m.floats::<4>(p + 8)?,
            font: m.u32(p + 24)?,
            align: m.u32(p + 28)?,
            color: m.u32(p + 32)?,
            height: m.f32(p + 36)?,
            read_only: m.u32(p + 40)? != 0,
            multiline: m.u32(p + 44)? != 0,
            word_wrap: m.u32(p + 48)? != 0,
            initial_text: m.opt_str(p + 52)?,
            variable: m.opt_str(p + 56)?,
        })
    }

    fn character(&mut self, p: usize) -> Res<Character> {
        let kind = self.m.u32(p)?;
        if self.m.u32(p + 4)? != SIGNATURE {
            return Err(format!("the character at {p:#x} lacks its signature"));
        }
        Ok(match kind {
            1 => Character::Shape { bounds: self.bounds(p + 8)?, geometry: self.m.u32(p + 24)? },
            2 => Character::Text(self.text(p)?),
            3 => {
                let name = self.m.opt_str(p + 8)?.unwrap_or_default();
                let (a, b) = (self.m.u32(p + 12)?, self.m.u32(p + 16)?);
                if a != 0 || b != 0 {
                    self.unknown.push(format!("font character at {p:#x}: words {a:#x}, {b:#x}"));
                }
                Character::Font { name }
            }
            4 => Character::Button(self.button(p)?),
            5 => {
                let (n, fp, zero) = (self.m.u32(p + 8)? as usize, self.m.ptr(p + 12)?, self.m.u32(p + 16)?);
                if zero != 0 {
                    self.unknown.push(format!("sprite at {p:#x}: last word {zero:#x}"));
                }
                Character::Sprite { frames: self.frames(n, fp)? }
            }
            7 => Character::Image { image: self.m.u32(p + 8)? },
            k => {
                self.unknown.push(format!("character kind {k} at {p:#x}"));
                Character::Unknown(k)
            }
        })
    }
}

impl Movie {
    /// Reads a movie from its `.apt` and `.const` files. The `unknown` list says what was not
    /// understood: kinds, opcodes, unread bytes.
    pub fn read(apt: &[u8], konst: &[u8]) -> Res<Movie> {
        let (base, consts, mut unknown) = read_consts(konst)?;
        let mut m = Img::new(apt);
        if m.take(0, 12)? != b"Apt Data\x1a\0\0\0" {
            return Err("not an apt file".into());
        }
        let (kind, sig) = (m.u32(base)?, m.u32(base + 4)?);
        if kind != 9 || sig != SIGNATURE {
            return Err("the movie is not where the constants say".into());
        }
        let (nframes, fptr, zero, nchars, cptr) = (m.u32(base + 8)? as usize, m.ptr(base + 12)?, m.u32(base + 16)?, m.u32(base + 20)? as usize, m.ptr(base + 24)?);
        let (width, height, ms_per_frame) = (m.u32(base + 28)?, m.u32(base + 32)?, m.u32(base + 36)?);
        let (nimports, _iptr, nexports, eptr, last) = (m.u32(base + 40)?, m.u32(base + 44)?, m.u32(base + 48)? as usize, m.ptr(base + 52)?, m.u32(base + 56)?);
        if zero != 0 || last != 0 {
            unknown.push(format!("the movie's reserved words are {zero:#x} and {last:#x}"));
        }
        if nimports != 0 {
            unknown.push(format!("{nimports} imports"));
        }
        let mut exports = Vec::new();
        for i in 0..nexports {
            let name = m.opt_str(eptr + 8 * i)?.unwrap_or_default();
            exports.push(Export { name, id: m.u32(eptr + 8 * i + 4)? });
        }
        let mut r = Reader { m, consts: &consts, unknown };
        let frames = r.frames(nframes, fptr)?;
        let mut characters = Vec::with_capacity(nchars);
        for i in 0..nchars {
            let at = r.m.ptr(cptr + 4 * i)?;
            characters.push(if at == 0 {
                Character::None
            } else if at == base {
                Character::Movie
            } else {
                r.character(at)?
            });
        }
        let mut unknown = std::mem::take(&mut r.unknown);
        unknown.extend(r.m.unread("apt"));
        unknown.extend(consts.iter().filter_map(|v| if let Value::Unknown(t, x) = v { Some(format!("constant of type {t}, value {x:#x}")) } else { None }));
        Ok(Movie { width, height, ms_per_frame, frames, characters, exports, consts, unknown })
    }
}
