//! A frame's display list: what the tree of clips shows, back to front, with each piece's
//! matrix and colour in the movie's pixels. The drawing is the renderer's.

use std::rc::Rc;

use super::movie::Character;
use super::player::{ClipKind, Color, IDENTITY, Matrix, Player, compose};
use super::value::ClipId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A shape of the movie's `.geo` file.
    Shape,
    Text,
}

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayItem {
    /// The movie the character is of (its `flashes.arc` name).
    pub movie: Rc<str>,
    pub character: u32,
    pub kind: ItemKind,
    /// From the character's own space to the movie's stage (a, b, c, d, tx, ty).
    pub matrix: Matrix,
    pub color: Color,
    /// A text's string (a script's, the variable's it is bound to, or the character's own).
    pub text: Option<String>,
    /// The depth the piece was placed at in its clip, and its clip depth if it masks.
    pub depth: i32,
    pub clip_depth: i32,
    /// The name of the root's child (a layer of the wrapper: `content`, `dialog`, `keys`, ...)
    /// the piece is under; empty for the root's own.
    pub layer: Rc<str>,
}

impl Player {
    /// The display list of the root movie, back to front.
    pub fn display_list(&self) -> Vec<DisplayItem> {
        let mut out = Vec::new();
        if let Some(r) = self.root {
            self.list_clip(r, &IDENTITY, &Color::IDENTITY, &Rc::from(""), &mut out);
        }
        out
    }

    fn list_clip(&self, id: ClipId, parent: &Matrix, pcol: &Color, layer: &Rc<str>, out: &mut Vec<DisplayItem>) {
        let c = self.clip(id);
        if !c.visible || !c.alive {
            return;
        }
        let m = compose(parent, &c.matrix);
        let col = pcol.then(&c.color);
        let mut item = |kind, text| {
            out.push(DisplayItem { movie: c.flash.name.as_str().into(), character: c.ch, kind, matrix: m, color: col, text, depth: c.depth, clip_depth: c.clip_depth, layer: layer.clone() })
        };
        match c.kind {
            ClipKind::Shape => item(ItemKind::Shape, None),
            ClipKind::Text => {
                let text = match (&c.text, c.flash.movie.characters.get(c.ch as usize)) {
                    (Some(t), _) => Some(t.to_string()),
                    (None, Some(Character::Text(t))) => self.bound_text(id, t.variable.as_deref()).or_else(|| t.initial_text.clone()),
                    _ => None,
                };
                item(ItemKind::Text, text);
            }
            ClipKind::Image | ClipKind::Button => {}
            ClipKind::Sprite => {
                for &k in c.children.values() {
                    // the root's children are the layers
                    let below: Rc<str> = if Some(id) == self.root { self.clip(k).name.as_str().into() } else { layer.clone() };
                    self.list_clip(k, &m, &col, &below, out);
                }
            }
        }
    }

    /// The contents of the variable a text field is bound to, found from the clip holding it.
    fn bound_text(&self, id: ClipId, var: Option<&str>) -> Option<String> {
        let var = var.filter(|v| !v.is_empty())?;
        let mut owner = self.clip(id).parent?;
        let (path, name) = var.rsplit_once(['.', '/']).unwrap_or(("", var));
        if !path.is_empty() {
            owner = self.resolve_path(owner, path)?;
        }
        self.clip(owner).props.get(name).map(|v| v.to_text().to_string())
    }
}
