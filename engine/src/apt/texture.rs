//! A movie's images: the `NNN.tgq` files, 8-bit palette images (`docs/assets.md`, "Menus and
//! HUD (Apt)"). The header and footer words are little-endian, unlike the rest of the disc.

use std::ops::Range;

use super::img::{Img, Res};

/// An image's header and where its parts are in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The stored size (a power of two on each side).
    pub width: u32,
    pub height: u32,
    /// The picture's own size, inside the stored one (the footer).
    pub used_width: u32,
    pub used_height: u32,
    /// The palette index of each texel, `width` by `height`, row after row, from the top.
    pub texels: Range<usize>,
    /// 256 colours of 4 bytes, R, G, B, A.
    pub palette: Range<usize>,
}

/// The header words after the size, as every file has them: 256 colours, a constant half word
/// (0x0200 as little-endian bytes 0x00 0x02), 8 bits a texel and 32 bits a palette entry.
const HEADER_REST: [u8; 8] = [0, 1, 0, 0, 0, 2, 8, 0x20];

impl Image {
    /// Reads a `TXFL` file whose parts fill it exactly.
    pub fn read(data: &[u8]) -> Res<Image> {
        let mut m = Img::new(data);
        if m.take(0, 4)? != b"TXFL" {
            return Err("not a TXFL image".into());
        }
        if m.take(4, 16)?.iter().any(|&b| b != 0) {
            return Err("the header's bytes 4 to 20 are not zero".into());
        }
        let size = m.take(0x14, 4)?;
        let (width, height) = (u16::from_le_bytes([size[0], size[1]]) as u32, u16::from_le_bytes([size[2], size[3]]) as u32);
        if m.take(0x18, 8)? != HEADER_REST {
            return Err("the header's format words are not 256 colours of 8 bits and 32".into());
        }
        if m.take(0x20, 4)?.iter().any(|&b| b != 0) {
            return Err("the header's last word is not zero".into());
        }
        let (texels, palette) = (0x24..0x24 + (width * height) as usize, 0x24 + (width * height) as usize..0x24 + (width * height) as usize + 1024);
        m.take(texels.start, texels.len())?;
        m.take(palette.start, palette.len())?;
        let foot = m.take(palette.end, 8)?;
        let (used_width, used_height) = (u32::from_le_bytes(foot[..4].try_into().unwrap()), u32::from_le_bytes(foot[4..].try_into().unwrap()));
        if m.gaps().iter().any(|g| !g.is_empty()) {
            return Err(format!("{} bytes after the footer", data.len() - palette.end - 8));
        }
        if used_width > width || used_height > height {
            return Err("the footer's size is larger than the stored one".into());
        }
        Ok(Image { width, height, used_width, used_height, texels, palette })
    }
}
