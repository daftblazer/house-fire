//! The interface overlay's picture: rectangles and text drawn on the CPU into a premultiplied
//! RGBA picture the size of the frame, which the renderer lays over the scene
//! (`overlay_pass::OverlayPass`). The text is `font8x8`'s public-domain 8x8 bitmap font; no
//! game data and no system fonts. The stand-in screens (`standin`) draw with this; the
//! original's own interface will replace them, not this.

use font8x8::{BASIC_FONTS, UnicodeFonts};

/// A colour, straight (not premultiplied) RGBA.
pub type Colour = [u8; 4];

/// A glyph's width and height in pixels, and the line's height the text is set on.
pub const GLYPH: i32 = 8;
pub const LINE: i32 = 10;

pub struct Overlay {
    width: i32,
    height: i32,
    /// Premultiplied RGBA, row after row.
    pixels: Vec<u8>,
}

impl Overlay {
    pub fn new(width: u32, height: u32) -> Overlay {
        Overlay { width: width as i32, height: height as i32, pixels: vec![0; (width * height * 4) as usize] }
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    /// The picture, premultiplied RGBA.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// A pixel laid over what is there (source over, premultiplied).
    fn blend(&mut self, x: i32, y: i32, c: Colour) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let a = u32::from(c[3]);
        let at = ((y * self.width + x) * 4) as usize;
        let px = &mut self.pixels[at..at + 4];
        for k in 0..3 {
            let src = u32::from(c[k]) * a / 255;
            px[k] = (src + u32::from(px[k]) * (255 - a) / 255).min(255) as u8;
        }
        px[3] = (a + u32::from(px[3]) * (255 - a) / 255).min(255) as u8;
    }

    /// A filled rectangle.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Colour) {
        for yy in y.max(0)..(y + h).min(self.height) {
            for xx in x.max(0)..(x + w).min(self.width) {
                self.blend(xx, yy, c);
            }
        }
    }

    /// A rectangle's outline, `t` pixels thick, inside it.
    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, c: Colour) {
        self.rect(x, y, w, t, c);
        self.rect(x, y + h - t, w, t, c);
        self.rect(x, y, t, h, c);
        self.rect(x + w - t, y, t, h, c);
    }

    /// The width a text takes.
    pub fn text_width(text: &str) -> i32 {
        text.chars().count() as i32 * GLYPH
    }

    /// A text with its top left at (x, y), in `c` over a one-pixel shadow.
    pub fn text(&mut self, x: i32, y: i32, text: &str, c: Colour) {
        self.glyphs(x + 1, y + 1, text, [0, 0, 0, c[3]]);
        self.glyphs(x, y, text, c);
    }

    fn glyphs(&mut self, x: i32, y: i32, text: &str, c: Colour) {
        for (i, ch) in text.chars().enumerate() {
            let rows = BASIC_FONTS.get(ch).or_else(|| BASIC_FONTS.get('?')).unwrap_or([0; 8]);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..8 {
                    if bits >> col & 1 != 0 {
                        self.blend(x + i as i32 * GLYPH + col, y + row as i32, c);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_covers_its_pixels_and_clips_at_the_edge() {
        let mut o = Overlay::new(16, 16);
        o.rect(12, 12, 8, 8, [255, 0, 0, 255]);
        assert_eq!(&o.pixels()[(15 * 16 + 15) * 4..(15 * 16 + 15) * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(&o.pixels()[(11 * 16 + 11) * 4..(11 * 16 + 11) * 4 + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn half_clear_black_over_white_halves_it() {
        let mut o = Overlay::new(2, 2);
        o.rect(0, 0, 2, 2, [255, 255, 255, 255]);
        o.rect(0, 0, 1, 1, [0, 0, 0, 128]);
        assert_eq!(o.pixels()[0], 127);
        assert_eq!(o.pixels()[3], 255);
    }

    #[test]
    fn text_sets_pixels_for_a_letter() {
        let mut o = Overlay::new(16, 16);
        o.text(0, 0, "I", [255, 255, 255, 255]);
        assert!(o.pixels().chunks(4).any(|p| p[3] == 255 && p[0] == 255));
    }
}
