//! Textures as Bustin' Out keeps them (`textures.arc`'s entries): an `LFXT` record, its name, a
//! 20-byte header and the texels, mip levels and palette after it (`docs/assets.md`,
//! "Textures"). Read here as the disc has them; decoding the texels is the renderer's.

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

fn be16(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(p..p + 2)?.try_into().ok()?))
}

/// The bytes of a level `w` by `h` texels of `bits` each.
fn level_size(w: u32, h: u32, bits: u32) -> usize {
    (w as usize * h as usize * bits as usize).div_ceil(8)
}

/// Reads an entry: `None` unless it is an `LFXT` of a kind known whose parts fill it exactly.
pub fn read(e: &[u8]) -> Option<Texture> {
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
    (end == e.len()).then_some(Texture { name, kind, width, height, levels, flags, texels: (start, size), palette })
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
