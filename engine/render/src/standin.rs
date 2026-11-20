//! The sandbox's stand-in interface: a plain HUD (the active Sim, its motives, the speed), a
//! pie menu that is a list, and a dialog box, drawn into the overlay's picture. They are
//! stand-ins, kept apart from the play loop and from the renderer: the original's own menus
//! and HUD come later and replace this module whole (`docs/plan.md`, "The playable sandbox").

use crate::overlay::{GLYPH, LINE, Overlay};

const PANEL: [u8; 4] = [16, 16, 24, 200];
const EDGE: [u8; 4] = [220, 220, 230, 255];
const TEXT: [u8; 4] = [240, 240, 240, 255];
const DIM: [u8; 4] = [160, 160, 170, 255];
const HIGHLIGHT: [u8; 4] = [70, 100, 170, 230];

/// The eight motives the bars show: the label and the place in a person's motives (the engine's
/// `motives::motive` names; the eighth, the room's, is the one at 3, inferred from the rates in
/// docs/simulation.md, "The motives").
pub const MOTIVE_BARS: [(&str, usize); 8] = [("Hunger", 7), ("Comfort", 6), ("Hygiene", 8), ("Bladder", 9), ("Energy", 5), ("Fun", 15), ("Social", 14), ("Room", 3)];

/// A motive runs from this to its top (as the logs' values do).
const MOTIVE_RANGE: (f32, f32) = (-100.0, 100.0);

/// What the HUD shows.
pub struct Hud<'a> {
    pub name: &'a str,
    /// The eight motives, in `MOTIVE_BARS`' order.
    pub motives: [f32; 8],
    /// Ticks a frame: 0 paused.
    pub speed: u32,
    pub tick: u32,
    /// The lot's clock, as "HH:MM".
    pub clock: String,
}

pub fn draw_hud(o: &mut Overlay, h: &Hud) {
    let w = 176;
    o.rect(6, 6, w, 14 + 8 * LINE + 4, PANEL);
    o.frame(6, 6, w, 14 + 8 * LINE + 4, 1, EDGE);
    o.text(12, 10, h.name, TEXT);
    let bar_x = 12 + 8 * GLYPH;
    for (k, (label, _)) in MOTIVE_BARS.iter().enumerate() {
        let y = 22 + k as i32 * LINE;
        o.text(12, y, &label[..label.len().min(7)], DIM);
        let (lo, hi) = MOTIVE_RANGE;
        let f = ((h.motives[k] - lo) / (hi - lo)).clamp(0.0, 1.0);
        let full = w - (bar_x - 6) - 8;
        o.rect(bar_x, y + 1, full, 6, [40, 40, 50, 255]);
        let colour = if f < 0.3 { [200, 60, 50, 255] } else if f < 0.55 { [210, 180, 60, 255] } else { [80, 190, 90, 255] };
        o.rect(bar_x, y + 1, (full as f32 * f) as i32, 6, colour);
    }
    let speed = if h.speed == 0 { "PAUSED".to_string() } else { format!("{}x", h.speed) };
    let line = format!("{}  {}  tick {}", h.clock, speed, h.tick);
    let x = o.width() - Overlay::text_width(&line) - 14;
    o.rect(x - 6, 6, o.width() - x, 14, PANEL);
    o.text(x, 10, &line, if h.speed == 0 { [255, 200, 90, 255] } else { TEXT });
}

/// The pie menu as a list: its title, its entries and where it was opened.
pub struct Menu<'a> {
    pub at: (i32, i32),
    pub title: &'a str,
    pub items: &'a [String],
    pub selected: usize,
}

const MENU_PAD: i32 = 4;
const ITEM_H: i32 = LINE + 2;

/// Where the menu is: its top left, width and height, kept inside the picture.
pub fn menu_rect(o_size: (i32, i32), m: &Menu) -> Rect {
    let widest = m.items.iter().map(|s| s.chars().count()).chain([m.title.chars().count()]).max().unwrap_or(1) as i32;
    let (w, h) = (widest * GLYPH + 2 * MENU_PAD, (m.items.len() as i32 + 1) * ITEM_H + 2 * MENU_PAD);
    (m.at.0.min(o_size.0 - w - 2).max(2), m.at.1.min(o_size.1 - h - 2).max(2), w, h)
}

/// The entry under a point, if the point is on one.
pub fn menu_item_at(o_size: (i32, i32), m: &Menu, x: i32, y: i32) -> Option<usize> {
    let (mx, my, w, h) = menu_rect(o_size, m);
    if x < mx || x >= mx + w || y < my || y >= my + h {
        return None;
    }
    let row = (y - my - MENU_PAD) / ITEM_H - 1;
    usize::try_from(row).ok().filter(|&r| r < m.items.len())
}

pub fn draw_menu(o: &mut Overlay, m: &Menu) {
    let (x, y, w, h) = menu_rect((o.width(), o.height()), m);
    o.rect(x, y, w, h, PANEL);
    o.frame(x, y, w, h, 1, EDGE);
    o.text(x + MENU_PAD, y + MENU_PAD + 1, m.title, DIM);
    if m.items.is_empty() {
        o.text(x + MENU_PAD, y + MENU_PAD + ITEM_H + 1, "(nothing)", DIM);
    }
    for (i, item) in m.items.iter().enumerate() {
        let iy = y + MENU_PAD + (i as i32 + 1) * ITEM_H;
        if i == m.selected {
            o.rect(x + 1, iy, w - 2, ITEM_H, HIGHLIGHT);
        }
        o.text(x + MENU_PAD, iy + 1, item, TEXT);
    }
}

/// A rectangle: x, y, width, height.
pub type Rect = (i32, i32, i32, i32);

/// A dialog: its text and its buttons' labels.
pub struct Dialog<'a> {
    pub text: &'a str,
    pub buttons: &'a [String],
    pub selected: usize,
}

const WRAP: usize = 36;

/// The text broken into lines of at most `WRAP` characters at the spaces.
fn wrapped(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > WRAP {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines
}

/// The box, and each button's rectangle (x, y, w, h), centred in a picture of `size`.
pub fn dialog_layout(size: (i32, i32), d: &Dialog) -> (Rect, Vec<Rect>) {
    let lines = wrapped(d.text);
    let buttons_w: i32 = d.buttons.iter().map(|b| Overlay::text_width(b) + 16 + 8).sum();
    let w = (lines.iter().map(|l| Overlay::text_width(l)).max().unwrap_or(0)).max(buttons_w) + 24;
    let h = lines.len() as i32 * LINE + 12 + 2 * ITEM_H + 16;
    let (x, y) = ((size.0 - w) / 2, (size.1 - h) / 2);
    let mut bx = x + (w - buttons_w + 8) / 2;
    let by = y + h - ITEM_H - 10;
    let rects = d
        .buttons
        .iter()
        .map(|b| {
            let bw = Overlay::text_width(b) + 16;
            let r = (bx, by, bw, ITEM_H + 4);
            bx += bw + 8;
            r
        })
        .collect();
    ((x, y, w, h), rects)
}

/// The button under a point, if there is one.
pub fn dialog_button_at(size: (i32, i32), d: &Dialog, x: i32, y: i32) -> Option<usize> {
    dialog_layout(size, d).1.iter().position(|&(bx, by, bw, bh)| x >= bx && x < bx + bw && y >= by && y < by + bh)
}

pub fn draw_dialog(o: &mut Overlay, d: &Dialog) {
    let ((x, y, w, h), buttons) = dialog_layout((o.width(), o.height()), d);
    o.rect(x, y, w, h, PANEL);
    o.frame(x, y, w, h, 1, EDGE);
    for (i, line) in wrapped(d.text).iter().enumerate() {
        o.text(x + 12, y + 8 + i as i32 * LINE, line, TEXT);
    }
    for (i, ((bx, by, bw, bh), label)) in buttons.iter().zip(d.buttons).enumerate() {
        o.rect(*bx, *by, *bw, *bh, if i == d.selected { HIGHLIGHT } else { [50, 50, 64, 255] });
        o.frame(*bx, *by, *bw, *bh, 1, EDGE);
        o.text(bx + 8, by + 3, label, TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_a_menu_row_names_the_entry() {
        let items = vec!["Use".to_string(), "Look".to_string()];
        let m = Menu { at: (100, 100), title: "Fridge", items: &items, selected: 0 };
        let (x, y, _, _) = menu_rect((640, 448), &m);
        assert_eq!(menu_item_at((640, 448), &m, x + 5, y + MENU_PAD + ITEM_H + 3), Some(0));
        assert_eq!(menu_item_at((640, 448), &m, x + 5, y + MENU_PAD + 2 * ITEM_H + 3), Some(1));
        assert_eq!(menu_item_at((640, 448), &m, x + 5, y + MENU_PAD + 3), None);
        assert_eq!(menu_item_at((640, 448), &m, 0, 0), None);
    }

    #[test]
    fn a_menu_at_the_edge_stays_in_the_picture() {
        let items = vec!["A long entry's name".to_string()];
        let m = Menu { at: (630, 440), title: "T", items: &items, selected: 0 };
        let (x, y, w, h) = menu_rect((640, 448), &m);
        assert!(x + w <= 640 && y + h <= 448);
    }

    #[test]
    fn dialog_buttons_are_found_by_their_rectangles() {
        let buttons = vec!["Yes".to_string(), "No".to_string()];
        let d = Dialog { text: "Throw the party? It will be loud.", buttons: &buttons, selected: 0 };
        let (_, rects) = dialog_layout((640, 448), &d);
        let (bx, by, _, _) = rects[1];
        assert_eq!(dialog_button_at((640, 448), &d, bx + 2, by + 2), Some(1));
    }
}
