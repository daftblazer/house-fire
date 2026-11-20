//! The movies' drawing: what the Apt player lists (`housefire::apt::display`), made into the
//! triangles the pass (`apt_pass`) draws: a shape's fills, an image's texels through the fill's
//! matrix, a text's glyphs laid out from its font. Positions go to clip space at the picture's
//! size, so the movies' 640 by 448 stage can be drawn at any resolution.

use crate::apt_pass::{AptPass, Batch, TextureId, Vertex};
use housefire::apt::display::{DisplayItem, ItemKind};
use housefire::apt::font::Font;
use housefire::apt::geometry::Fill;
use housefire::apt::movie::Character;
use housefire::apt::player::Matrix;
use housefire::apt::{self, Flash};
use housefire::disc::Disc;
use housefire::{arc, shader, texture};
use std::collections::HashMap;
use std::rc::Rc;

/// The size of the stage the movies are made for.
pub const STAGE: (f32, f32) = (640.0, 448.0);

/// What the drawing reads from the disc: the movies, the fonts and their pages' picture.
pub struct AptAssets {
    pub flashes: Vec<(String, Rc<Flash>)>,
    pub fonts: HashMap<String, Font>,
    textures_arc: Vec<u8>,
    pages: HashMap<u32, usize>,
}

impl AptAssets {
    pub fn new(disc: &Disc) -> AptAssets {
        let flashes = apt::read_flashes(&disc.read("flashes.arc").unwrap_or_default()).into_iter().filter_map(|(n, r)| r.ok().map(|f| (n, Rc::new(f)))).collect();
        let fonts = apt::read_fonts(&disc.read("fonts.arc").unwrap_or_default()).into_iter().filter_map(|(n, r)| r.ok().map(|f| (n, f))).collect();
        let textures_arc = disc.read("textures.arc").unwrap_or_default();
        let mut pages = HashMap::new();
        for e in arc::entries(&textures_arc).unwrap_or_default() {
            if let Some(t) = texture::read(arc::bytes(&textures_arc, &e))
                && t.name.to_ascii_lowercase().starts_with("font_")
            {
                pages.insert(shader::name_hash(&t.name), e.offset as usize);
            }
        }
        AptAssets { flashes, fonts, textures_arc, pages }
    }

    /// A font page's picture, premultiplied white: the page is 4-bit indices into the first of the
    /// entry's two IA8 palettes, whose intensity is its alpha (`docs/assets.md`,
    /// "Drawing the movies (U2b)").
    fn page(&self, hash: u32, shadow: bool) -> Option<(u32, u32, Vec<u8>)> {
        let at = *self.pages.get(&hash)?;
        let b = &self.textures_arc[at..];
        let (t, _) = texture::read_prefix(b)?;
        if t.kind != texture::Kind::Palette4 {
            return None;
        }
        let src = &b[t.texels.0..];
        let (po, n) = t.palette?;
        let coverage = texture::decode_indexed(t.kind, t.width, t.height, src, b.get(po..po + n * 2)?, 0)?;
        let gain = if shadow { SHADOW_GAIN } else { GLYPH_GAIN };
        Some((t.width, t.height, coverage.iter().flat_map(|&a| [((f32::from((a >> 24) as u8) * gain).min(255.0)) as u8; 4]).collect()))
    }
}

/// Makes the triangles of display lists, uploading each texture the first time it is used.
pub struct Painter<'a> {
    assets: &'a AptAssets,
    flashes: HashMap<&'a str, &'a Rc<Flash>>,
    white: Option<TextureId>,
    images: HashMap<(&'a str, u32), TextureId>,
    pages: HashMap<(u32, bool), (TextureId, u32, u32)>,
}

/// One line of the game's own text: the game draws the dialogs' and menus' words itself
/// (`ERFont::DoDraw`), a line at a time, with a font of `fonts.arc`.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    /// The font's name in `fonts.arc` (`frankfur`).
    pub font: String,
    /// The stage point the line's top left is at (the game's own place, as logged).
    pub x: f32,
    pub y: f32,
    /// Pixels of the stage for each texel of the font's page, across and down.
    pub scale: [f32; 2],
    /// 0xRRGGBBAA, the glyphs' tint.
    pub color: u32,
    /// The game's shadow pass (black, under the line).
    pub shadow: bool,
}

/// The advance of a space in the font's texels (a glyph's record is its columns on the page; a
/// space has none): fitted to the widths of the game's logged lines (`game_text.rs`).
pub const SPACE: f32 = 11.0;

/// How far the pen moves after a character, in texels of the page.
pub fn advance(font: &Font, c: char) -> f32 {
    if c == ' ' { SPACE } else { font.glyph(c as u32).map_or(0.0, |g| (g.x1 - g.x0) as f32) }
}

/// How much the page's coverage is raised for the game's glyphs (clamped to 1) and for their
/// black shadow: the game's strokes are crisper and its shadow blacker than the palette's
/// intensity as alpha gives (sweeps over the seven frames: gains 1 to 10, the within-4 share
/// peaks at 1.2 and 3).
const GLYPH_GAIN: f32 = 1.2;
const SHADOW_GAIN: f32 = 3.0;

/// The wrapper's layers the game's pictures show nothing of: the key pictures and the D-pad,
/// and the buttons the wrapper makes for each screen (`dialog_button_accept`...): the movies
/// report where their buttons are and the game draws those itself.
const HIDDEN_LAYERS: [&str; 1] = ["keys"];

/// The triangles and runs of one frame.
#[derive(Default)]
pub struct Drawing {
    pub vertices: Vec<Vertex>,
    pub batches: Vec<Batch>,
}

impl Drawing {
    fn push(&mut self, texture: TextureId, v: [Vertex; 3]) {
        match self.batches.last_mut() {
            Some(b) if b.texture == texture => b.count += 3,
            _ => self.batches.push(Batch { texture, first: self.vertices.len() as u32, count: 3 }),
        }
        self.vertices.extend_from_slice(&v);
    }
}

fn unpack(c: u32) -> [f32; 4] {
    [(c >> 24) as f32 / 255.0, ((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
}

fn apply(m: &Matrix, p: [f32; 2]) -> [f32; 2] {
    let (x, y) = (f64::from(p[0]), f64::from(p[1]));
    [(m[0] * x + m[2] * y + m[4]) as f32, (m[1] * x + m[3] * y + m[5]) as f32]
}

impl<'a> Painter<'a> {
    pub fn new(assets: &'a AptAssets) -> Painter<'a> {
        Painter { assets, flashes: assets.flashes.iter().map(|(n, f)| (n.as_str(), f)).collect(), white: None, images: HashMap::new(), pages: HashMap::new() }
    }

    fn page(&mut self, pass: &mut AptPass, hash: u32, shadow: bool) -> Option<(TextureId, u32, u32)> {
        if let Some(p) = self.pages.get(&(hash, shadow)) {
            return Some(*p);
        }
        let (w, h, rgba) = self.assets.page(hash, shadow)?;
        let t = (pass.add_texture(w, h, &rgba, false), w, h);
        self.pages.insert((hash, shadow), t);
        Some(t)
    }

    /// Lays out a line's glyphs and adds their triangles.
    pub fn text(&mut self, pass: &mut AptPass, out: &mut Drawing, run: &TextRun) {
        let assets = self.assets;
        let Some(font) = assets.fonts.get(&run.font) else { return };
        let (sx, sy) = (2.0 / STAGE.0, 2.0 / STAGE.1);
        let tint = unpack(run.color);
        let band = font.line as f32;
        let top = run.y;
        let mut x = run.x;
        for c in run.text.chars() {
            let advance = advance(font, c);
            let cols = font.glyph(c as u32).map_or(0.0, |g| (g.x1 - g.x0) as f32);
            if let Some(g) = font.glyph(c as u32)
                && let Some(page) = font.pages.get(g.page as usize)
                && let Some((tex, pw, ph)) = self.page(pass, page.texture, run.shadow)
            {
                let (u0, u1) = (g.x0 as f32 / pw as f32, g.x1 as f32 / pw as f32);
                // the page is stored upside down: the band's last row is the glyph's top
                let (v_top, v_bot) = (((g.row as f32 + 1.0) * band) / ph as f32, (g.row as f32 * band) / ph as f32);
                let (x1, y1) = (x + cols * run.scale[0], top + band * run.scale[1]);
                let corner = |px: f32, py: f32, u: f32, v: f32| Vertex { pos: [px * sx - 1.0, py * sy - 1.0], uv: [u, v], tint, mul: [1.0; 4], add: [0.0; 4], premultiplied: 1.0, _pad: 0.0 };
                let (tl, tr, bl, br) = (corner(x, top, u0, v_top), corner(x1, top, u1, v_top), corner(x, y1, u0, v_bot), corner(x1, y1, u1, v_bot));
                out.push(tex, [tl, tr, bl]);
                out.push(tex, [tr, br, bl]);
            }
            x += advance * run.scale[0];
        }
    }

    fn white(&mut self, pass: &mut AptPass) -> TextureId {
        *self.white.get_or_insert_with(|| pass.add_texture(1, 1, &[255; 4], true))
    }

    fn image(&mut self, pass: &mut AptPass, movie: &'a str, id: u32) -> Option<(TextureId, u32, u32)> {
        let flash = self.flashes.get(movie)?;
        let (img, bytes) = flash.images.get(&id)?;
        let tex = match self.images.get(&(movie, id)) {
            Some(t) => *t,
            None => {
                let pal = &bytes[img.palette.clone()];
                let rgba: Vec<u8> = bytes[img.texels.clone()].iter().flat_map(|&i| [pal[i as usize * 4], pal[i as usize * 4 + 1], pal[i as usize * 4 + 2], pal[i as usize * 4 + 3]]).collect();
                let t = pass.add_texture(img.width, img.height, &rgba, true);
                self.images.insert((movie, id), t);
                t
            }
        };
        Some((tex, img.width, img.height))
    }

    /// The triangles of a display list (in clip space: any picture size).
    pub fn draw(&mut self, pass: &mut AptPass, list: &[DisplayItem]) -> Drawing {
        let mut out = Drawing::default();
        let (sx, sy) = (2.0 / STAGE.0, 2.0 / STAGE.1);
        let clip = |p: [f32; 2]| [p[0] * sx - 1.0, p[1] * sy - 1.0];
        for item in list {
            if HIDDEN_LAYERS.contains(&&*item.layer) {
                continue;
            }
            let Some(flash) = self.flashes.get(&*item.movie).copied() else { continue };
            let mul = item.color.mul.map(|v| v as f32);
            // The game's draw uses a clip's colour multiplier and not its offset: the HUD's pills
            // are placed with a multiplier of (0.65, 0.65, 1) and a blue offset of 254, and the
            // game shows the picture's own blue (the offset would make it white-blue).
            let add = [0.0; 4];
            // (the text items of the movies' own text characters are not drawn yet: the dialogs'
            // words are the game's, `text`)
            if let (ItemKind::Shape, Some(Character::Shape { geometry, .. })) = (item.kind, flash.movie.characters.get(item.character as usize)) {
                let Some(shape) = flash.geometry.shape(*geometry) else { continue };
                for fill in &shape.fills {
                    match fill {
                        Fill::Solid { color, triangles } => {
                            let white = self.white(pass);
                            let tint = unpack(*color);
                            for t in triangles {
                                let v = t.map(|p| Vertex { pos: clip(apply(&item.matrix, p)), uv: [0.5, 0.5], tint, mul, add, premultiplied: 0.0, _pad: 0.0 });
                                out.push(white, v);
                            }
                        }
                        Fill::Textured { color, image, matrix, triangles } => {
                            let Some((tex, w, h)) = self.image(pass, &flash.name, *image) else { continue };
                            let tint = unpack(*color);
                            for t in triangles {
                                let v = t.map(|p| {
                                    let u = p[0] * matrix[0] + p[1] * matrix[4] + matrix[12];
                                    let vv = p[0] * matrix[1] + p[1] * matrix[5] + matrix[13];
                                    Vertex { pos: clip(apply(&item.matrix, p)), uv: [u / w as f32, vv / h as f32], tint, mul, add, premultiplied: 0.0, _pad: 0.0 }
                                });
                                out.push(tex, v);
                            }
                        }
                        Fill::Lines { .. } | Fill::Unknown(_) => {}
                    }
                }
            }
        }
        out
    }
}
