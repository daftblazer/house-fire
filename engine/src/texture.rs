//! Textures as Bustin' Out keeps them (`textures.arc`'s entries): an `LFXT` record, its name, a
//! 20-byte header and the texels, mip levels and palette after it (`docs/assets.md`,
//! "Textures"). Read here as the disc has them; decoding the texels is the renderer's.

use crate::arc::be16;

/// How a texture's texels are stored (the header's first byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 0x81: 4 bits a texel, in 8-byte blocks of 4 by 4 (two 16-bit colours, two bits a texel).
    Blocks,
    /// 0x82: 16 bits a texel.
    Bits16,
    /// 0x85: 32 bits a texel.
    Bits32,
    /// 0x89: 4-bit indices into a palette.
    Palette4,
    /// 0x8a: 8-bit indices into a palette.
    Palette8,
}

impl Kind {
    fn of(b: u8) -> Option<Kind> {
        Some(match b {
            0x81 => Kind::Blocks,
            0x82 => Kind::Bits16,
            0x85 => Kind::Bits32,
            0x89 => Kind::Palette4,
            0x8a => Kind::Palette8,
            _ => return None,
        })
    }
    pub fn bits(self) -> u32 {
        match self {
            Kind::Blocks | Kind::Palette4 => 4,
            Kind::Palette8 => 8,
            Kind::Bits16 => 16,
            Kind::Bits32 => 32,
        }
    }
}

/// A texture's header and where its parts are in the entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub name: String,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    /// Mip levels, the full-size one counted (1 when the header gives 0).
    pub levels: u32,
    /// The header's flags word (+12: wrap and filter, not read yet).
    pub flags: u16,
    /// The texels' offset in the entry and their size, every level.
    pub texels: (usize, usize),
    /// The palette's offset and its entry count (4 bytes an entry), for the palette kinds.
    pub palette: Option<(usize, usize)>,
}

/// The bytes of a level `w` by `h` texels of `bits` each.
fn level_size(w: u32, h: u32, bits: u32) -> usize {
    (w as usize * h as usize * bits as usize).div_ceil(8)
}

/// Reads an entry: `None` unless it is an `LFXT` of a kind known whose parts fill it exactly.
pub fn read(e: &[u8]) -> Option<Texture> {
    read_prefix(e).filter(|&(_, end)| end == e.len()).map(|(t, _)| t)
}

/// Reads an `LFXT` at the start of `e` (one kept inside other data, `datasets.arc`'s): the
/// texture and where it ends, if its parts fit.
pub fn read_prefix(e: &[u8]) -> Option<(Texture, usize)> {
    if e.get(..4)? != b"LFXT" {
        return None;
    }
    let len = e.get(20..)?.iter().position(|&c| c == 0)?;
    let name: String = e[20..20 + len].iter().map(|&c| c as char).collect();
    let h = 20 + len + 1;
    let kind = Kind::of(*e.get(h)?)?;
    if e[h + 1] as u32 != kind.bits() {
        return None;
    }
    let (width, height) = (be16(e, h + 2)? as u32, be16(e, h + 4)? as u32);
    let colours = be16(e, h + 8)? as usize;
    let flags = be16(e, h + 12)?;
    let levels = (be16(e, h + 14)? as u32).max(1);
    let start = h + 20;
    let size: usize = (0..levels).map(|l| level_size((width >> l).max(1), (height >> l).max(1), kind.bits())).sum();
    let palette = matches!(kind, Kind::Palette4 | Kind::Palette8).then_some((start + size, colours));
    let end = start + size + palette.map_or(0, |p| p.1 * 4);
    (end <= e.len()).then_some((Texture { name, kind, width, height, levels, flags, texels: (start, size), palette }, end))
}

/// A texel as the renderer takes it: RGBA, red in the low byte.
pub type Rgba = u32;

fn rgba(r: u32, g: u32, b: u32, a: u32) -> Rgba {
    r | g << 8 | b << 16 | a << 24
}

/// GX's RGB565: 5, 6 and 5 bits widened by repeating their top bits.
pub fn rgb565(v: u16) -> Rgba {
    let (r, g, b) = ((v >> 11) as u32 & 31, (v >> 5) as u32 & 63, v as u32 & 31);
    rgba(r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255)
}

/// GX's RGB5A3: opaque 5-5-5 with the top bit set, else 3 bits of alpha and 4-4-4.
pub fn rgb5a3(v: u16) -> Rgba {
    if v & 0x8000 != 0 {
        let (r, g, b) = ((v >> 10) as u32 & 31, (v >> 5) as u32 & 31, v as u32 & 31);
        rgba(r << 3 | r >> 2, g << 3 | g >> 2, b << 3 | b >> 2, 255)
    } else {
        let (a, r, g, b) = ((v >> 12) as u32 & 7, (v >> 8) as u32 & 15, (v >> 4) as u32 & 15, v as u32 & 15);
        rgba(r * 17, g * 17, b * 17, a << 5 | a << 2 | a >> 1)
    }
}

/// One CMPR block's four colours (DXT1: two 565 colours, then two between them, or one
/// between and transparent black when the first is not the greater).
fn cmpr_colours(c0: u16, c1: u16) -> [Rgba; 4] {
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let ch = |c: Rgba, s: u32| (c >> s) & 0xFF;
    let mix = |w0: u32, w1: u32, d: u32| [0, 8, 16].map(|s| (w0 * ch(p0, s) + w1 * ch(p1, s)) / d);
    if c0 > c1 {
        let (a, b) = (mix(2, 1, 3), mix(1, 2, 3));
        [p0, p1, rgba(a[0], a[1], a[2], 255), rgba(b[0], b[1], b[2], 255)]
    } else {
        let m = mix(1, 1, 2);
        [p0, p1, rgba(m[0], m[1], m[2], 255), rgba(m[0], m[1], m[2], 0)]
    }
}

/// A 16-bit palette entry as GX reads it, by the palette's format (0 IA8: intensity in the
/// low byte, alpha in the high; 1 RGB565; 2 RGB5A3).
pub fn palette_colour(v: u16, format: u8) -> Rgba {
    match format {
        0 => {
            let (a, i) = ((v >> 8) as u32, v as u32 & 0xFF);
            rgba(i, i, i, a)
        }
        1 => rgb565(v),
        _ => rgb5a3(v),
    }
}

/// A palette texture's level: its indices (C4: 8 by 8 tiles of nibbles, the high first; C8: 8 by
/// 4 tiles of bytes) through `palette` (16-bit big-endian entries, as the game loads the first
/// half of the LFXT's palette) of `format`.
pub fn decode_indexed(kind: Kind, w: u32, h: u32, src: &[u8], palette: &[u8], format: u8) -> Option<Vec<Rgba>> {
    let (bw, bh) = match kind {
        Kind::Palette4 => (8, 8),
        Kind::Palette8 => (8, 4),
        _ => return None,
    };
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u32; w * h];
    let colour = |i: usize| -> Option<Rgba> { Some(palette_colour(u16::from_be_bytes([*palette.get(i * 2)?, *palette.get(i * 2 + 1)?]), format)) };
    let mut p = 0usize;
    for by in 0..h.div_ceil(bh) {
        for bx in 0..w.div_ceil(bw) {
            for y in 0..bh {
                for x in 0..bw {
                    let i = if kind == Kind::Palette4 {
                        let b = *src.get(p + y * 4 + x / 2)?;
                        (if x & 1 == 1 { b & 0xF } else { b >> 4 }) as usize
                    } else {
                        *src.get(p + y * 8 + x)? as usize
                    };
                    let (px, py) = (bx * bw + x, by * bh + y);
                    if px < w && py < h {
                        out[py * w + px] = colour(i)?;
                    }
                }
            }
            p += 32;
        }
    }
    Some(out)
}

/// The kinds that need no palette, as the GX formats they are (16 bits RGB5A3, blocks CMPR,
/// 32 bits RGBA8): one level `w` by `h` from `src` (its tiles in GX order), row after row.
/// `None` for the palette kinds or data too short.
pub fn decode_level(kind: Kind, w: u32, h: u32, src: &[u8]) -> Option<Vec<Rgba>> {
    let (bw, bh) = match kind {
        Kind::Blocks => (8, 8),
        Kind::Bits16 | Kind::Bits32 => (4, 4),
        Kind::Palette4 | Kind::Palette8 => return None,
    };
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u32; w * h];
    let mut p = 0usize;
    let be16 = |b: &[u8], i: usize| -> Option<u16> { Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])) };
    for by in 0..h.div_ceil(bh) {
        for bx in 0..w.div_ceil(bw) {
            let (x0, y0) = (bx * bw, by * bh);
            let mut put = |x: usize, y: usize, c: Rgba| {
                if x0 + x < w && y0 + y < h {
                    out[(y0 + y) * w + x0 + x] = c;
                }
            };
            match kind {
                Kind::Bits16 => {
                    for i in 0..16 {
                        put(i % 4, i / 4, rgb5a3(be16(src, p + i * 2)?));
                    }
                    p += 32;
                }
                Kind::Bits32 => {
                    for i in 0..16 {
                        let k = p + i * 2;
                        let (a, r) = (*src.get(k)? as u32, *src.get(k + 1)? as u32);
                        let (g, b) = (*src.get(k + 32)? as u32, *src.get(k + 33)? as u32);
                        put(i % 4, i / 4, rgba(r, g, b, a));
                    }
                    p += 64;
                }
                Kind::Blocks => {
                    for sb in 0..4 {
                        let pal = cmpr_colours(be16(src, p)?, be16(src, p + 2)?);
                        let (sx, sy) = ((sb & 1) * 4, (sb >> 1) * 4);
                        for y in 0..4 {
                            let bits = *src.get(p + 4 + y)?;
                            for x in 0..4 {
                                put(sx + x, sy + y, pal[(bits >> (6 - 2 * x)) as usize & 3]);
                            }
                        }
                        p += 8;
                    }
                }
                Kind::Palette4 | Kind::Palette8 => unreachable!(),
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: u8, bits: u8, w: u16, h: u16, colours: u16, levels: u16, body: usize) -> Vec<u8> {
        let mut v = b"LFXT".to_vec();
        v.extend([0, 0, 0, 7]);
        v.extend([0u8; 12]);
        v.extend(b"t\0");
        v.extend([kind, bits]);
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend(0u16.to_be_bytes());
        v.extend(colours.to_be_bytes());
        v.extend([0, 0, 4, 0x93]);
        v.extend(levels.to_be_bytes());
        v.extend([0u8; 4]);
        v.resize(v.len() + body, 0);
        v
    }

    #[test]
    fn levels_and_palettes_fill_the_entry() {
        // 64 by 64, 16 bits, four levels: 8192 + 2048 + 512 + 128
        let t = read(&entry(0x82, 16, 64, 64, 0, 4, 10880)).unwrap();
        assert_eq!((t.levels, t.texels.1), (4, 10880));
        // 32 by 32, 4-bit indices, 16 colours of 4 bytes
        let t = read(&entry(0x89, 4, 32, 32, 16, 0, 512 + 64)).unwrap();
        assert_eq!(t.palette.map(|p| p.1), Some(16));
        assert!(read(&entry(0x89, 4, 32, 32, 16, 0, 512 + 63)).is_none());
    }
}
