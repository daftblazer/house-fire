//! A Sim's run-time texture: the image the original composes when it dresses a Sim (Sim_SetModels
//! at 800591CC calls the composer at 8005D018), from the run-length textures of `rletextu.arc`.
//!
//! The composer:
//! 1. picks a stack of up to 15 layers from the description (`layer_stack`): the skin by gender,
//!    age and skin tone, then the layers of each worn part (the textures a part's table entry
//!    names), each layer's palette tinted by one of the description's colour bytes;
//! 2. composes the stack texel by texel into a 256 x 256 picture, each layer over the ones below
//!    by its alpha (`over`), the first layer opaque;
//! 3. blurs it to 128 x 128 (a 3 x 3 kernel of 1 2 1 / 2 4 2 / 1 2 1 sixteenths, centred on the
//!    even texels);
//! 4. reduces the 128 x 128 picture to a palette of up to 256 colours and indexes it (`quantizer`),
//!    the indices in the console's C8 tiles (8 texels by 4).
//!
//! GPU-free and deterministic: floats only in the tints, as the original's.

use crate::outfit::{Description, PartTables, Slot};
use crate::quantizer::Quantizer;
use crate::rle_texture::{self, RleTexture};

/// The composed picture's side before the blur, and the image's after it.
pub const FULL: usize = 256;
pub const SIDE: usize = FULL / 2;

/// How a layer's palette is tinted: by one of the description's colour bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// Untouched.
    None,
    /// A skin tone (0 to 3; any other value the lightest's offsets): hue, saturation and
    /// lightness offsets (the original's table of eight, four in use).
    Skin(u8),
    /// A clothes colour: a hue turn of a tenth for each three, then darker, as is or lighter
    /// by the remainder; 1 is the layer's own colours, 30 to 32 greys.
    Clothes(u8),
    /// A hair colour: one of eleven hue, saturation and lightness offsets (1 is the layer's own).
    Hair(u8),
}

/// A layer of the stack: the resource id of its run-length texture and its tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer {
    pub id: u32,
    pub tint: Tint,
}

/// The two run-time conditions the layer stack depends on besides the description (the original's
/// Sim object's: the word at 0x5A0, whether the clothes layers take their colours).
#[derive(Clone, Copy, Debug)]
pub struct Conditions {
    pub tinted_clothes: bool,
}

/// The skin layer's resource, by gender, age and skin tone (the description's byte 16; the
/// original's 8005C83C): the tones 0 to 2, then every other value the fourth.
fn skin_id(d: &Description) -> u32 {
    const IDS: [[u32; 4]; 4] = [
        // child female, child male, adult female, adult male; by tone
        [0x98ED_FAE4, 0xAF38_0F6D, 0x4831_DBBD, 0x895F_E3FE],
        [0x2BCA_7663, 0x2266_A150, 0xC56F_7580, 0x9555_1443],
        [0x5E63_299A, 0xC8E1_4DAF, 0x2FE8_997F, 0x8373_3B2C],
        [0x29F2_8D35, 0xF14E_2FD6, 0x1647_FB06, 0x3A3E_11FD],
    ];
    let tone = (d.looks[16 - 8] as usize).min(3);
    IDS[d.adult as usize * 2 + d.male as usize][tone]
}

/// The layer stack of a Sim, in the order the composer lays it (the original's 8005CA04 and the
/// loads of 8005D018): the layers with no resource (id 0) are left out.
pub fn layer_stack(tables: &PartTables, d: &Description, cond: Conditions) -> Vec<Layer> {
    let byte = |b: usize| d.looks[b - 8];
    let word = |slot: Slot, w: usize| tables.entry(d, slot).map_or(0, |e| e[w]);
    let tone = byte(16);
    let clothes = |b: usize, on: bool| if on { Tint::Clothes(byte(b)) } else { Tint::None };
    let hair = if d.adult && d.male { Tint::Hair(byte(17)) } else { Tint::None };
    let stack = [
        (skin_id(d), Tint::None),
        (word(Slot::Upper, 2), clothes(18, cond.tinted_clothes)),
        (word(Slot::Upper, 3), Tint::None),
        (word(Slot::Shoes, 2), clothes(20, true)),
        (word(Slot::Lower, 2), clothes(19, cond.tinted_clothes)),
        (word(Slot::Lower, 3), Tint::None),
        (word(Slot::Eyes, 3), Tint::Skin(tone)),
        (word(Slot::Nose, 2), Tint::Skin(tone)),
        (word(Slot::Mouth, 2), Tint::Skin(tone)),
        (0xC9E9_21E7, Tint::Clothes(1)),
        (0xD578_82B9, Tint::Clothes(byte(22))),
        (word(Slot::Extra, 2), hair),
        (word(Slot::Extra, 3), Tint::None),
        (word(Slot::Head, 2), Tint::Hair(byte(17))),
        (word(Slot::Head, 3), Tint::None),
    ];
    stack.iter().filter(|(id, _)| *id != 0).map(|&(id, tint)| Layer { id, tint }).collect()
}

/// RGB (red in the low byte) to hue, saturation and lightness (the original's 8005E784; a grey
/// has hue 1).
fn rgb_to_hsl(rgb: u32) -> [f32; 3] {
    let r = (rgb & 0xFF) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f32 / 255.0;
    let b = ((rgb >> 16) & 0xFF) as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == min {
        return [1.0, 0.0, min];
    }
    let sum = f64::from(min) + f64::from(max);
    let delta = max - min;
    let ddelta = f64::from(delta);
    let l = (sum * 0.5) as f32;
    let s = if f64::from(l) <= 0.5 { ddelta / sum } else { ddelta / (2.0 - f64::from(max) - f64::from(min)) } as f32;
    let bc = (max - b) / delta;
    let rc = (max - r) / delta;
    let gc = (max - g) / delta;
    let h = if r == max {
        if g == min { 5.0 + f64::from(bc) } else { 1.0 - f64::from(gc) }
    } else if g == max {
        if r == min { 3.0 + f64::from(rc) } else { 3.0 - f64::from(bc) }
    } else if r == min {
        3.0 + f64::from(gc)
    } else {
        5.0 - f64::from(rc)
    } as f32;
    [h / 6.0, s, l]
}

/// Hue, saturation and lightness to RGB (the original's 8005E5BC).
fn hsl_to_rgb([h, s, l]: [f32; 3]) -> u32 {
    let v = if l <= 0.5 { l * (s + 1.0) } else { (l + s) - l * s };
    let (r, g, b);
    if v <= 0.0 {
        (r, g, b) = (0.0, 0.0, 0.0);
    } else {
        let m = (l + l) - v;
        let sv = (v - m) / v;
        let h = h * 6.0;
        let sextant = h as i32;
        let vsf = v * sv * (h - sextant as f32);
        let mid1 = m + vsf;
        let mid2 = v - vsf;
        (r, g, b) = match sextant {
            0 => (v, mid1, m),
            1 => (mid2, v, m),
            2 => (m, v, mid1),
            3 => (m, mid2, v),
            4 => (mid1, m, v),
            5 | 6 => (v, m, mid2),
            _ => (0.0, 0.0, 0.0),
        };
    }
    u32::from((r * 255.0) as i32 as u8) + (((g * 255.0) as i32 as u32) << 8 & 0xFF00) + (((b * 255.0) as i32 as u32) << 16 & 0xFF_0000)
}

/// The skin tones' hue, saturation and lightness offsets (the original's initialiser at
/// 8005FD00, from the constants at 8029BB60: the same table as the character creator's).
const SKIN_OFFSETS: [[f32; 3]; 4] = [[0.007, -0.006, -0.03], [0.015, 0.04, -0.1], [0.028, -0.16, -0.2], [0.03, -0.08, -0.4]];

/// The hair colours' offsets (8029B7EC to 8029B850, 8005E218).
const HAIR_OFFSETS: [[f32; 3]; 11] = [
    [-0.019445, 0.11, 0.1],
    [0.0, 0.0, 0.0],
    [-0.00833, -0.065, -0.14],
    [-0.03889, 0.19, -0.175],
    [-0.12, -0.13, -0.255],
    [-0.025, 0.005, -0.265],
    [-0.07778, -0.13, -0.35],
    [-0.46665, -0.13, -0.35],
    [-0.5, -1.0, 0.25],
    [0.0, -0.1, 0.13],
    [0.005555, 0.025, 0.15],
];

/// A hue shifted by `d` and wrapped into 0 to 1.
fn turn(h: f32, d: f32) -> f32 {
    let h = h + d;
    if h > 1.0 {
        h - 1.0
    } else if h < 0.0 {
        h + 1.0
    } else {
        h
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// A palette tinted: each colour with alpha, by `tint` (the original's 8005DFF0, 8005DDA4 and
/// 8005E218); a colour with no alpha becomes transparent black.
fn tinted(palette: &[u32], tint: Tint) -> Vec<u32> {
    palette
        .iter()
        .map(|&c| {
            let alpha = c & 0xFF00_0000;
            if alpha == 0 {
                return 0;
            }
            match tint {
                Tint::None => c,
                Tint::Skin(tone) => {
                    let [h, s, l] = rgb_to_hsl(c);
                    // a grey (hue 1) stays
                    if h >= 1.0 {
                        return c;
                    }
                    let [dh, ds, dl] = SKIN_OFFSETS[if (1..=3).contains(&tone) { usize::from(tone) } else { 0 }];
                    alpha + hsl_to_rgb([turn(h, dh), clamp01(s + ds), clamp01(l + dl)])
                }
                Tint::Hair(k) => {
                    if k == 1 {
                        return c;
                    }
                    let [h, s, l] = rgb_to_hsl(c);
                    let [dh, ds, dl] = HAIR_OFFSETS.get(usize::from(k)).copied().unwrap_or([0.0; 3]);
                    alpha + hsl_to_rgb([turn(h, dh), clamp01(s + ds), clamp01(l + dl)])
                }
                Tint::Clothes(k) => {
                    if k == 1 {
                        return c;
                    }
                    let [mut h, mut s, mut l] = rgb_to_hsl(c);
                    let step = if (30..=32).contains(&k) {
                        s = 0.0;
                        0.2
                    } else {
                        let t = f32::from(k / 3);
                        h = t.mul_add(0.1, h);
                        if h > 1.0 {
                            h -= 1.0;
                        }
                        0.15
                    };
                    match k % 3 {
                        0 => l = if l - step < 0.0 { 0.0 } else { l - step },
                        2 => l = if l + step > 1.0 { 1.0 } else { l + step },
                        _ => {}
                    }
                    alpha + hsl_to_rgb([h, s, l])
                }
            }
        })
        .collect()
}

/// A layer ready to read: its texels' palette indices and its tinted palette.
pub struct Plane {
    indices: Vec<u8>,
    palette: Vec<u32>,
}

impl Plane {
    pub fn new(t: &RleTexture, tint: Tint) -> Plane {
        Plane { indices: t.indices(), palette: tinted(&t.palette, tint) }
    }

    fn texel(&self, i: usize) -> u32 {
        self.indices.get(i).and_then(|&k| self.palette.get(usize::from(k))).copied().unwrap_or(0)
    }
}

/// `src` over `dst` (the original's 8005FAC0): the source's alpha is 0 for none, 1 to 127 a
/// weight of that much in 127ths of 255 (twice the alpha), 128 and over opaque; the result is
/// 24 bits.
fn over(dst: u32, src: u32) -> u32 {
    let a = src >> 24;
    let weight = if a & 0x80 != 0 { 255 } else { (a & 0x7F) << 1 };
    if weight == 255 {
        return src & 0xFF_FFFF;
    }
    let mix = |shift: u32| {
        let s = (src >> shift) & 0xFF;
        let d = (dst >> shift) & 0xFF;
        ((weight * s + (255 - weight) * d) / 255) << shift
    };
    mix(0) | mix(8) | mix(16)
}

/// The stack composed texel by texel into a 256 x 256 picture, red in the low byte of each word.
pub fn compose(planes: &[Plane]) -> Vec<u32> {
    let Some((base, rest)) = planes.split_first() else { return vec![0; FULL * FULL] };
    (0..FULL * FULL)
        .map(|i| {
            let mut c = base.texel(i) & 0xFF_FFFF;
            for p in rest {
                let v = p.texel(i);
                if v >> 24 != 0 {
                    c = over(c, v);
                }
            }
            c
        })
        .collect()
}

/// The composed picture blurred and halved: each output texel the 1 2 1 / 2 4 2 / 1 2 1 kernel
/// (sixteenths, rounded) about the picture's texel (2x, 2y), the edges repeating; as red, green
/// and blue bytes.
///
/// The original blurs the picture twice, once to build the palette and once to index (its rows
/// go through three buffers swapped by pointer, and after the first pass's 127 swaps the second
/// starts with two of them exchanged): in the second pass (`indexing`) the output's second row
/// has the picture's first row, not its second, before it.
pub fn halve(full: &[u32], indexing: bool) -> Vec<[u8; 3]> {
    let at = |x: usize, y: usize, c: u32| (full[y * FULL + x] >> (8 * c)) & 0xFF;
    let mut out = Vec::with_capacity(SIDE * SIDE);
    for y in 0..SIDE {
        // the row before the first repeats the first
        let before = if indexing && y == 1 { 0 } else { (2 * y).saturating_sub(1) };
        let rows = [before, 2 * y, 2 * y + 1];
        for x in 0..SIDE {
            let cols = [(2 * x).saturating_sub(1), 2 * x, (2 * x + 1).min(FULL - 1)];
            let mut px = [0u8; 3];
            for (c, o) in px.iter_mut().enumerate() {
                let mut sum = 8;
                for (ry, wy) in rows.iter().zip([1, 2, 1]) {
                    for (cx, wx) in cols.iter().zip([1, 2, 1]) {
                        sum += wy * wx * at(*cx, *ry, c as u32);
                    }
                }
                *o = (sum >> 4) as u8;
            }
            out.push(px);
        }
    }
    out
}

/// The 128 x 128 pictures of a Sim before they are reduced to a palette: the one its palette is
/// made from and the one its texels are indexed from (they differ in one row, `halve`).
pub struct Pictures {
    pub palette: Vec<[u8; 3]>,
    pub indexing: Vec<[u8; 3]>,
}

/// The pictures of a Sim from the disc's `rletextu.arc` (`arc` its bytes): `None` if a layer's
/// resource is not there.
pub fn pictures(arc: &[u8], stack: &[Layer]) -> Option<Pictures> {
    let entries = crate::arc::entries(arc)?;
    let planes: Option<Vec<Plane>> = stack
        .iter()
        .map(|l| {
            let e = entries.iter().find(|e| e.id == l.id)?;
            Some(Plane::new(&rle_texture::read(crate::arc::bytes(arc, e))?, l.tint))
        })
        .collect();
    let full = compose(&planes?);
    Some(Pictures { palette: halve(&full, false), indexing: halve(&full, true) })
}

/// A Sim's finished texture: its image as GX C8 indices (tiles of 8 texels by 4, tile after tile
/// across the rows) and its palette's entries (RGB5A3, as the original writes them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimTexture {
    pub indices: Vec<u8>,
    pub palette: Vec<u16>,
}

impl SimTexture {
    /// The image as RGBA texels (red in the low byte), row after row, through the palette as the
    /// console's decoder reads it.
    pub fn decode(&self) -> Option<Vec<crate::texture::Rgba>> {
        let mut palette = self.palette.clone();
        palette.resize(PALETTE_ENTRIES, 0);
        let bytes: Vec<u8> = palette.iter().flat_map(|e| e.to_be_bytes()).collect();
        crate::texture::decode_indexed(crate::texture::Kind::Palette8, SIDE as u32, SIDE as u32, &self.indices, &bytes, TLUT_RGB5A3)
    }
}

/// The octree parameter the composer builds its quantizer with.
const QUANTIZER_MEMORY: u32 = 0x7F00;

/// The palette's size, a C8 image's colours.
pub const PALETTE_ENTRIES: usize = 256;

/// The palette entries' format, the format 2 of a GXTlutObj (RGB5A3), and the high bit it sets on
/// a colour with no transparency.
const TLUT_RGB5A3: u8 = 2;
const RGB5A3_OPAQUE: u16 = 0x8000;

/// A C8 tile's texels: 8 across, 4 down.
const TILE_WIDTH: usize = 8;
const TILE_HEIGHT: usize = 4;

/// The picture reduced to a palette of up to 256 colours and indexed, as the composer does:
/// the palette made from the picture's colours in row order, then each texel's nearest entry.
pub fn reduce(pictures: &Pictures) -> SimTexture {
    let mut q = Quantizer::new(PALETTE_ENTRIES, QUANTIZER_MEMORY);
    for &px in &pictures.palette {
        q.add(px);
    }
    q.finish();
    // RGB5A3 with no transparency: the opaque bit and five bits of each colour
    let palette = (0..q.len())
        .map(|i| {
            let [r, g, b] = q.colour(i);
            RGB5A3_OPAQUE + (u16::from(r >> 3) << 10) + (u16::from(g >> 3) << 5) + u16::from(b >> 3)
        })
        .collect();
    let mut indices = vec![0u8; SIDE * SIDE];
    for (i, &px) in pictures.indexing.iter().enumerate() {
        let (x, y) = (i % SIDE, i / SIDE);
        let tile = (y / TILE_HEIGHT) * (SIDE / TILE_WIDTH) + x / TILE_WIDTH;
        indices[tile * TILE_WIDTH * TILE_HEIGHT + (y % TILE_HEIGHT) * TILE_WIDTH + x % TILE_WIDTH] = q.nearest(px);
    }
    SimTexture { indices, palette }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_goes_over_by_its_alpha() {
        let dst = 0x00_80_40_20;
        // opaque (alpha 0x80 and over): the source's colour
        assert_eq!(over(dst, 0xFF_01_02_03), 0x01_02_03);
        // alpha 0x40 is a weight of 128 of 255
        assert_eq!(over(dst, 0x40_FF_FF_FF) & 0xFF, (128 * 255 + 127 * 0x20) / 255);
        // alpha 1 is a weight of 2
        assert_eq!(over(dst, 0x01_00_00_00) & 0xFF, 0x20 * 253 / 255);
    }

    #[test]
    fn tints_keep_what_they_should() {
        let palette = [0u32, 0xFF_40_80_C0, 0xFF_10_10_10];
        // a colour with no alpha is transparent black, whatever the tint
        assert_eq!(tinted(&[0x00_12_34_56], Tint::Hair(3)), [0]);
        // 1 is the layer's own colours
        assert_eq!(tinted(&palette, Tint::Clothes(1)), palette);
        assert_eq!(tinted(&palette, Tint::Hair(1)), palette);
        // a grey stays through the skin tones; the alpha is always kept
        assert_eq!(tinted(&[0xFF_80_80_80], Tint::Skin(2))[0], 0xFF_80_80_80);
        assert_eq!(tinted(&[0xFF_40_80_C0], Tint::Clothes(5))[0] >> 24, 0xFF);
    }

    #[test]
    fn hsl_round_trips() {
        // (colours with red or blue the greatest: the original's conversion to a hue is not the
        // inverse of its conversion back for a green, nor for a hue of exactly 1)
        for rgb in [0x20_80_E0u32, 0xC0_40_10, 0x30_20_90] {
            let back = hsl_to_rgb(rgb_to_hsl(rgb));
            for shift in [0, 8, 16] {
                assert!(((back >> shift) & 0xFF).abs_diff((rgb >> shift) & 0xFF) <= 1, "{rgb:06x} came back {back:06x}");
            }
        }
    }

    #[test]
    fn halving_keeps_a_flat_picture_and_the_second_pass_its_odd_row() {
        let flat = vec![0x00_33_66_99; FULL * FULL];
        assert!(halve(&flat, false).iter().all(|&p| p == [0x99, 0x66, 0x33]));
        // row 0 darker than the rest: the kernel's row before output row 1 is the picture's row 1,
        // or in the second pass its row 0
        let rows: Vec<u32> = (0..FULL * FULL).map(|i| if i < FULL { 100 } else { 200 }).collect();
        let (a, b) = (halve(&rows, false), halve(&rows, true));
        assert!(b[SIDE][0] < a[SIDE][0]);
        assert!(a.iter().zip(&b).enumerate().all(|(i, (x, y))| i / SIDE == 1 || x == y));
    }

    #[test]
    fn indices_go_in_console_tiles() {
        // one colour: every index 0, the palette that colour in RGB5A3
        let flat = vec![[200u8, 100, 50]; SIDE * SIDE];
        let t = reduce(&Pictures { palette: flat.clone(), indexing: flat });
        assert_eq!(t.palette.len(), 1);
        assert_eq!(t.palette[0], 0x8000 + (25 << 10) + (12 << 5) + 6);
        assert!(t.indices.iter().all(|&i| i == 0));
    }
}
