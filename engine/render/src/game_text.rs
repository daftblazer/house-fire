//! The game's own text (`ERFont::DoDraw`, logged as `TextDrawRec`) made into lines for the
//! Apt pass to draw: the dialogs', menus' and HUD's words are not in the movies. What a record
//! has and what is taken from it (`docs/assets.md`, "Drawing the movies (U2b)"):
//!
//! - the font: the hash in the font object's second word is the CRC-32 of the font's name;
//! - the line's top left (`x`, `y`, fractions of the 640 by 448 screen): used as logged;
//! - the size words (four floats at the size word): the point the game aligned the line at
//!   twice, first moved by the shadow's offset (2 pixels right and down), then as given. A
//!   line is centred on that x when the draw's x is left of it, and put with its middle on
//!   that y when the draw's y is above it; the width and height the game worked out from the
//!   font's scale follow, so the scale is taken from them;
//! - a record with no size words, followed by one of the same text that has them, is the black
//!   shadow pass; one with none after it is a plain draw.

use crate::apt_draw::{AptAssets, STAGE, TextRun, advance};
use housefire::shader::name_hash;
use housefire::tree_log::TextDrawRec;

/// The glyph rows of a line in the font's texels (the font's `line`) and the share of them the
/// game's vertical centring uses: with the aligned y below the draw's y by `d` pixels the line's
/// height is `2 d`. Fitted: `2 d` over the scale across, for the logged lines that have both, is
/// 32.6 texels in 6 of 6 (against the font's 37).
const CENTRED_HEIGHT: f32 = 32.6;

/// The scale of a plain draw (no size words) across and down in pixels per texel. Fitted to the
/// width of "Customize A Sim" in frames 1050 and 1122 (sweep 0.58 to 0.64 on the within-4 share).
const PLAIN_SCALE: f32 = 0.62;

/// The colour of a plain draw, 0xRRGGBBAA: the darkest plateau of the strokes of "Customize A
/// Sim" in frame 1050 of the game's picture, (21, 17, 50).
const PLAIN_COLOR: u32 = 0x1511_32ff;

/// Pixels added to a line's logged top: the game's glyphs sit 2 pixels above where the band's
/// top puts them (sweep 0 to -4 on the within-4 share over the seven frames: -2 is the best in
/// each frame with text).
const TOP_OFFSET: f32 = -2.0;

/// The lines the game draws in game frame `frame`, back to front.
pub fn lines(assets: &AptAssets, records: &[TextDrawRec], frame: u32) -> Vec<TextRun> {
    let here: Vec<&TextDrawRec> = records.iter().filter(|r| r.frame == frame).collect();
    let mut out = Vec::new();
    for (i, r) in here.iter().enumerate() {
        let Some(font) = assets.fonts.keys().find(|n| name_hash(n) == r.font_words[1]) else { continue };
        let shadow = r.size == 0 && here.get(i + 1).is_some_and(|n| n.size != 0 && n.text == r.text);
        let scale = scale(assets, font, if shadow { here[i + 1] } else { r });
        let color = if shadow {
            0x0000_00ff
        } else if r.size == 0 {
            PLAIN_COLOR
        } else {
            0xffff_ffff
        };
        out.push(TextRun { text: r.text.clone(), font: font.clone(), x: r.x * STAGE.0, y: r.y * STAGE.1 + TOP_OFFSET, scale, color, shadow });
    }
    out
}

/// The scale a line was drawn at, across and down.
fn scale(assets: &AptAssets, font: &str, r: &TextDrawRec) -> [f32; 2] {
    let Some(f) = assets.fonts.get(font).filter(|_| r.size != 0) else { return [PLAIN_SCALE; 2] };
    let texels: f32 = r.text.chars().map(|c| advance(f, c)).sum();
    let (ax, ay) = (f32::from_bits(r.size_words[2]) * STAGE.0, f32::from_bits(r.size_words[3]) * STAGE.1);
    let (x, y) = (r.x * STAGE.0, r.y * STAGE.1);
    let across = (ax - x > 0.5 && texels > 0.0).then(|| 2.0 * (ax - x) / texels);
    let down = (ay - y > 0.5).then(|| 2.0 * (ay - y) / CENTRED_HEIGHT);
    match (across, down) {
        (Some(a), Some(d)) => [a, d],
        (Some(a), None) => [a, a],
        (None, Some(d)) => [d, d],
        (None, None) => [PLAIN_SCALE; 2],
    }
}

/// The lines the engine's own feed draws (`hud::Text`) as lines for the pass. The game's rules
/// are the logged lines' (above): a line with a width is as wide as the game measured it, one
/// with a height has its middle on its y, a plain one has no shadow pass.
pub fn native_lines(assets: &AptAssets, texts: &[housefire::hud::Text]) -> Vec<TextRun> {
    let Some(font) = assets.fonts.keys().find(|n| name_hash(n) == FONT_HASH) else { return Vec::new() };
    let f = &assets.fonts[font];
    texts
        .iter()
        .map(|t| {
            let texels: f32 = t.text.chars().map(|c| advance(f, c)).sum();
            let down = t.height.map(|h| h / CENTRED_HEIGHT);
            let across = t.width.filter(|_| texels > 0.0).map(|w| w / texels);
            let scale = match (across, down) {
                (Some(a), Some(d)) => [a, d],
                (Some(a), None) => [a, a],
                (None, Some(d)) => [d, d],
                (None, None) => [PLAIN_SCALE; 2],
            };
            let color = if t.shadow {
                0x0000_00ff
            } else if t.plain {
                PLAIN_COLOR
            } else {
                0xffff_ffff
            };
            TextRun { text: t.text.clone(), font: font.clone(), x: t.x * STAGE.0, y: t.y * STAGE.1 + TOP_OFFSET, scale, color, shadow: t.shadow }
        })
        .collect()
}

/// The hash of the one font the interface draws in (`FRANKFUR`, the CRC-32 of its name).
const FONT_HASH: u32 = 0x67f6_64fb;
