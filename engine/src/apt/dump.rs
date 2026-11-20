//! A readable listing of a movie, for `assets ISO apt NAME`.

use std::fmt::Write;

use super::Flash;
use super::actions::{Instr, Value};
use super::geometry::Fill;
use super::movie::{Character, Frame, Item};

fn show_value(v: &Value, pool: &[Value]) -> String {
    match v {
        Value::Str(s) => format!("{s:?}"),
        Value::Float(f) => format!("{f}"),
        Value::Undefined => "undefined".into(),
        Value::Register(r) => format!("r{r}"),
        Value::Bool(b) => format!("{b}"),
        Value::Int(i) => format!("{i}"),
        Value::Pool(i) => match pool.get(*i as usize) {
            Some(Value::Str(s)) => format!("{s:?}"),
            Some(o) => show_value(o, &[]),
            None => format!("pool[{i}]"),
        },
        Value::Unknown(t, x) => format!("?type{t}:{x:#x}"),
    }
}

/// Lists instructions, one a line, branch targets as the instruction's index. `pool` is the last
/// constant pool seen, which `Push`es of pool entries are shown through.
fn instrs(out: &mut String, v: &[Instr], pool: &mut Vec<Value>, ind: usize) {
    for (n, i) in v.iter().enumerate() {
        let pad = " ".repeat(ind);
        let line = match i {
            Instr::Op(o) => format!("{o:?}"),
            Instr::GotoFrame(f) => format!("GotoFrame {f}"),
            Instr::GotoFrame2(f) => format!("GotoFrame2 {f:#x}"),
            Instr::SetRegister(r) => format!("SetRegister r{r}"),
            Instr::ConstantPool(vs) => {
                *pool = vs.clone();
                format!("ConstantPool [{}]", vs.iter().map(|v| show_value(v, &[])).collect::<Vec<_>>().join(", "))
            }
            Instr::Push(vs) => format!("Push {}", vs.iter().map(|v| show_value(v, pool)).collect::<Vec<_>>().join(", ")),
            Instr::BranchAlways(t) => format!("BranchAlways -> {t}"),
            Instr::BranchIfTrue(t) => format!("BranchIfTrue -> {t}"),
            Instr::With { size } => format!("With {size} bytes"),
            Instr::DefineFunction { name, params, body } => {
                let _ = writeln!(out, "{pad}{n:3}  DefineFunction {} ({}) {{", name.as_deref().unwrap_or("-"), params.join(", "));
                instrs(out, body, pool, ind + 4);
                let _ = writeln!(out, "{pad}     }}");
                continue;
            }
            Instr::End => "End".into(),
            Instr::Unknown(c) => format!("UNKNOWN opcode {c:#x}"),
        };
        let _ = writeln!(out, "{pad}{n:3}  {line}");
    }
}

fn frames(out: &mut String, fs: &[Frame], pool: &mut Vec<Value>, ind: usize) {
    let pad = " ".repeat(ind);
    for (n, f) in fs.iter().enumerate() {
        let _ = writeln!(out, "{pad}frame {n}:");
        for it in &f.items {
            match it {
                Item::Action(a) => {
                    let _ = writeln!(out, "{pad}  action:");
                    instrs(out, a, pool, ind + 4);
                }
                Item::Label { name, flags, frame } => {
                    let _ = writeln!(out, "{pad}  label {name:?} (flags {flags}, frame {frame})");
                }
                Item::Place(p) => {
                    let _ = writeln!(
                        out,
                        "{pad}  place flags {:#x} depth {} char {} matrix {:?} at {:?} color {:#010x} add {:#08x} ratio {} name {:?} clip_depth {}",
                        p.flags, p.depth, p.character, p.matrix, p.translation, p.color, p.color_add, p.ratio, p.name, p.clip_depth
                    );
                    for c in &p.clip_actions {
                        let _ = writeln!(out, "{pad}    on events {:#x} key {}:", c.events, c.key);
                        instrs(out, &c.actions, pool, ind + 6);
                    }
                }
                Item::Remove { depth } => {
                    let _ = writeln!(out, "{pad}  remove depth {depth}");
                }
                Item::Background(c) => {
                    let _ = writeln!(out, "{pad}  background {c:#010x}");
                }
                Item::Unknown(k) => {
                    let _ = writeln!(out, "{pad}  UNKNOWN item kind {k}");
                }
            }
        }
    }
}

/// The listing of a movie.
pub fn dump(f: &Flash) -> String {
    let m = &f.movie;
    let mut out = String::new();
    let _ = writeln!(out, "movie {}: {}x{}, {} ms a frame, {} characters, {} frames", f.name, m.width, m.height, m.ms_per_frame, m.characters.len(), m.frames.len());
    let _ = writeln!(out, "constants: {}", m.consts.len());
    for e in &m.exports {
        let _ = writeln!(out, "export {:?} = character {}", e.name, e.id);
    }
    let _ = writeln!(out, "\ncharacters:");
    let mut pool = Vec::new();
    for (i, c) in m.characters.iter().enumerate() {
        match c {
            Character::Movie => {
                let _ = writeln!(out, "  {i}: movie");
            }
            Character::Shape { bounds, geometry } => {
                let _ = writeln!(out, "  {i}: shape bounds {bounds:?} geometry {geometry}");
                if let Some(s) = f.geometry.shape(*geometry) {
                    for fill in &s.fills {
                        match fill {
                            Fill::Solid { color, triangles } => {
                                let _ = writeln!(out, "      solid {color:#010x}, {} triangles", triangles.len());
                            }
                            Fill::Textured { color, image, matrix, triangles } => {
                                let _ = writeln!(out, "      textured image {image} tint {color:#010x}, {} triangles, matrix {matrix:?}", triangles.len());
                            }
                            Fill::Lines { color, lines } => {
                                let _ = writeln!(out, "      lines {color:#010x}, {}", lines.len());
                            }
                            Fill::Unknown(k) => {
                                let _ = writeln!(out, "      UNKNOWN fill kind {k}");
                            }
                        }
                    }
                }
            }
            Character::Image { image } => {
                let _ = match f.images.get(image) {
                    Some((im, _)) => writeln!(out, "  {i}: image {image} ({}x{}, picture {}x{})", im.width, im.height, im.used_width, im.used_height),
                    None => writeln!(out, "  {i}: image {image} (no file)"),
                };
            }
            Character::Sprite { frames: fs } => {
                let _ = writeln!(out, "  {i}: sprite, {} frames", fs.len());
                frames(&mut out, fs, &mut pool, 6);
            }
            Character::Text(t) => {
                let _ = writeln!(out, "  {i}: text {t:?}");
            }
            Character::Font { name } => {
                let _ = writeln!(out, "  {i}: font {name:?}");
            }
            Character::Button(b) => {
                let _ = writeln!(out, "  {i}: button {b:?}");
            }
            Character::None => {
                let _ = writeln!(out, "  {i}: null");
            }
            Character::Unknown(k) => {
                let _ = writeln!(out, "  {i}: UNKNOWN kind {k}");
            }
        }
    }
    let _ = writeln!(out, "\nmovie frames:");
    frames(&mut out, &m.frames, &mut pool, 2);
    if !f.unknown.is_empty() {
        let _ = writeln!(out, "\nunknown:");
        for u in &f.unknown {
            let _ = writeln!(out, "  {u}");
        }
    }
    out
}
