//! A font of `fonts.arc`: which glyph a character code has, and where on its page each glyph
//! is (`docs/assets.md`, "Menus and HUD (Apt)"). The pictures are `textures.arc` entries
//! named `font_NAME_sSIZE_pN` (their CRC-32 is each page's `texture`).
//!
//! The entry is a string of big-endian words that are not word-aligned: the name's NUL, then
//! the fields in a row.

use super::img::{Img, Res};

/// A page of glyph pictures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// The CRC-32 of the texture's name in capitals (`shader::name_hash`).
    pub texture: u32,
    pub width: u32,
    pub height: u32,
}

/// A glyph: the columns it takes on its page, and a row word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub x0: u32,
    pub x1: u32,
    /// The third word of the record: 0 for the first rows of a page, up to 7 in
    /// `systemfont`'s 128-high page (a row of 16 texels?), not placed against the original.
    pub row: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    pub name: String,
    /// 1 for the three Latin fonts, 0 for the three of CJK.
    pub latin: u32,
    /// The text size in the textures' names (`s12`, `s24`).
    pub size: u32,
    /// The second size-like word (21 for `systemfont`, 37 for the others), not placed.
    pub line: u32,
    /// The highest code plus one that a table slot exists for (0x7f, 0xff, 0xfff, 0x7ff).
    pub max_code: u32,
    /// (character code, glyph number), in the order the glyphs are laid out.
    pub map: Vec<(u32, u32)>,
    pub pages: Vec<Page>,
    /// The glyph records after the pages' (the file's count of both is `pages + glyphs`).
    pub glyphs: Vec<Glyph>,
    /// The six words that end the entry (`systemfont`: 0x37, -2, 0x43, 0x1e, 1, 0), not placed.
    pub tail: [i32; 6],
}

/// The 36 bytes after the three counts: the same in all six fonts but the page count's byte
/// (index 29).
const SIGNATURE: [u8; 36] = [
    0xf7, 0x6b, 0x62, 0x8f, 0, 0, 0, 0, 0, 0, 0xa8, 0x1a, 0xd9, 0xed, 0, 0, 0, 0, 0, 1, 0x4b, 0xd0, 0x4b, 0xa7, 0, 0, 0, 0, 0, 1, 0x59, 0x8d, 0xe3, 0xb1, 0, 0,
];

impl Font {
    /// Reads an entry that its fields fill exactly.
    pub fn read(data: &[u8]) -> Res<Font> {
        let mut m = Img::new(data);
        let name = m.cstr_unpadded(0)?;
        let mut p = name.len() + 1;
        let word = |m: &mut Img, p: &mut usize| -> Res<u32> {
            let v = m.u32(*p)?;
            *p += 4;
            Ok(v)
        };
        if word(&mut m, &mut p)? != 0 {
            return Err("the first word is not 0".into());
        }
        let latin = word(&mut m, &mut p)?;
        let records = word(&mut m, &mut p)? as usize;
        let sig = m.take(p, 36)?;
        p += 36;
        let page_byte = sig[29] as usize;
        let mut expect = SIGNATURE;
        expect[29] = sig[29];
        if sig != expect {
            return Err("the signature bytes differ from the other fonts'".into());
        }
        let count = word(&mut m, &mut p)? as usize;
        let (size, line, one, max_code, again) = (word(&mut m, &mut p)?, word(&mut m, &mut p)?, word(&mut m, &mut p)?, word(&mut m, &mut p)?, word(&mut m, &mut p)? as usize);
        if one != 1 || again != count {
            return Err("the code table's header is not as in the other fonts".into());
        }
        let mut map = Vec::with_capacity(count);
        for _ in 0..count {
            map.push((word(&mut m, &mut p)?, word(&mut m, &mut p)?));
        }
        let pages_n = word(&mut m, &mut p)? as usize;
        if pages_n != page_byte {
            return Err("the page count differs from the signature's".into());
        }
        for i in 0..pages_n {
            if word(&mut m, &mut p)? != i as u32 + 2 {
                return Err("the page numbers are not 2, 3, ...".into());
            }
        }
        let mut pages = Vec::new();
        let mut glyphs = Vec::new();
        for i in 0..records {
            if m.u8(p)? != 0xb7 {
                return Err(format!("record {i} does not start with 0xb7"));
            }
            p += 1;
            let (a, b, c) = (word(&mut m, &mut p)?, word(&mut m, &mut p)?, word(&mut m, &mut p)?);
            if i < pages_n {
                pages.push(Page { texture: a, width: b, height: c });
            } else {
                glyphs.push(Glyph { x0: a, x1: b, row: c });
            }
        }
        let mut tail = [0i32; 6];
        for t in &mut tail {
            *t = word(&mut m, &mut p)? as i32;
        }
        if p != data.len() {
            return Err(format!("{} bytes after the tail", data.len() - p));
        }
        Ok(Font { name, latin, size, line, max_code, map, pages, glyphs, tail })
    }
}
