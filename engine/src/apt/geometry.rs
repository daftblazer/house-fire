//! A movie's shapes: the `.geo` file, triangle lists with a colour, a texture and its matrix, or
//! line segments (`docs/assets.md`, "Menus and HUD (Apt)").

use super::img::{Img, Res};

pub type Point = [f32; 2];

/// One fill of a shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// Triangles of one colour (0xRRGGBBAA).
    Solid { color: u32, triangles: Vec<[Point; 3]> },
    /// Triangles filled from an image (the `NNN.tgq` of that character id), tinted by `color`;
    /// `matrix` is 4 by 4, row by row, its last row's first two floats a translation: a vertex
    /// times it is the point in the image, in texels.
    Textured { color: u32, image: u32, matrix: [f32; 16], triangles: Vec<[[f32; 2]; 3]> },
    /// Line segments of one colour.
    Lines { color: u32, lines: Vec<[Point; 2]> },
    /// A record kind this reader has no meaning for.
    Unknown(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeoShape {
    /// The id a shape character (`Character::Shape`) names it by.
    pub id: u32,
    pub fills: Vec<Fill>,
}

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub shapes: Vec<GeoShape>,
    /// What this reader could not account for (unread bytes, unknown record kinds).
    pub unknown: Vec<String>,
}

const MAGIC: &[u8] = b"Apt PCGL geometry file\x1a\0";

fn points<const N: usize>(m: &mut Img, p: usize, n: usize) -> Res<Vec<[Point; N]>> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut s = [[0.0; 2]; N];
        for (j, v) in s.iter_mut().enumerate() {
            *v = m.floats::<2>(p + 8 * (i * N + j))?;
        }
        out.push(s);
    }
    Ok(out)
}

impl Geometry {
    /// Reads a `.geo` file: the magic, the shape count and where the table of shapes is; each
    /// table entry the shape's id, its number of fills and where their record pointers are.
    pub fn read(data: &[u8]) -> Res<Geometry> {
        let mut m = Img::new(data);
        if m.take(0, MAGIC.len())? != MAGIC {
            return Err("not a geometry file".into());
        }
        let (n, table) = (m.u32(0x18)? as usize, m.ptr(0x1c)?);
        let mut g = Geometry::default();
        for i in 0..n {
            let e = table + 12 * i;
            let (id, count, list) = (m.u32(e)?, m.u32(e + 4)? as usize, m.ptr(e + 8)?);
            let mut fills = Vec::with_capacity(count);
            for j in 0..count {
                let r = m.ptr(list + 4 * j)?;
                let (kind, n, v, color) = (m.u32(r)?, m.u32(r + 4)? as usize, m.ptr(r + 8)?, m.u32(r + 12)?);
                fills.push(match kind {
                    1 => Fill::Solid { color, triangles: points::<3>(&mut m, v, n)? },
                    5 => {
                        let matrix = m.floats::<16>(r + 16)?;
                        let (image, zero) = (m.u32(r + 80)?, m.u32(r + 84)?);
                        if zero != 0 {
                            g.unknown.push(format!("shape {id}: a textured fill's last word is {zero:#x}, not 0"));
                        }
                        Fill::Textured { color, image, matrix, triangles: points::<3>(&mut m, v, n)? }
                    }
                    6 => Fill::Lines { color, lines: points::<2>(&mut m, v, n)? },
                    k => {
                        g.unknown.push(format!("shape {id}: record kind {k}"));
                        Fill::Unknown(k)
                    }
                });
            }
            g.shapes.push(GeoShape { id, fills });
        }
        g.unknown.extend(m.unread("geometry"));
        Ok(g)
    }

    /// A shape by id.
    pub fn shape(&self, id: u32) -> Option<&GeoShape> {
        self.shapes.iter().find(|s| s.id == id)
    }
}
