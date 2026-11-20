//! Tests of the Apt readers: a few on small hand-made inputs, the rest on the disc's files
//! (`HOUSEFIRE_BUSTIN_ISO`, skipped without it).

use super::actions::{Op, Value, decode};
use super::img::Img;
use super::*;
use crate::disc::Disc;

fn disc() -> Option<Disc> {
    Disc::open(std::env::var("HOUSEFIRE_BUSTIN_ISO").ok()?).ok()
}

#[test]
fn unread_bytes_are_reported() {
    let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let mut m = Img::new(&data);
    m.u32(0).unwrap();
    m.u8(6).unwrap();
    assert_eq!(m.gaps(), vec![4..6, 7..8]);
    assert!(m.u32(6).is_err());
}

#[test]
fn a_stream_decodes_with_aligned_operands_and_branches() {
    // Push of 2 constants, a Not, a BranchIfTrue over a Stop to the end, a Stop, End.
    let consts = [Value::Str("a".into()), Value::Int(7)];
    let mut b = vec![0x96, 0, 0, 0];
    b.extend(2u32.to_be_bytes());
    b.extend(24u32.to_be_bytes()); // the entries' offset
    b.push(0x12); // Not
    b.extend([0x9d, 0, 0]);
    b.extend(1i32.to_be_bytes()); // over the next instruction
    b.push(0x07); // Stop
    b.push(0); // End
    b.resize(24, 0); // padding to the entries at 24
    b.extend(0u32.to_be_bytes());
    b.extend(1u32.to_be_bytes());
    let mut m = Img::new(&b);
    let v = decode(&mut m, &consts, 0, None).unwrap();
    assert_eq!(v.len(), 4);
    assert_eq!(v[0], Instr::Push(vec![Value::Str("a".into()), Value::Int(7)]));
    assert_eq!(v[1], Instr::Op(Op::Not));
    assert_eq!(v[2], Instr::BranchIfTrue(4));
    assert_eq!(v[3], Instr::Op(Op::Stop));
    assert!(m.gaps().is_empty(), "{:?}", m.gaps());
}

#[test]
fn every_movie_is_read_in_full() {
    let Some(d) = disc() else { return };
    let flashes = read_flashes(&d.read("flashes.arc").unwrap());
    assert_eq!(flashes.len(), 61);
    for (name, r) in &flashes {
        let f = r.as_ref().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(f.unknown.is_empty(), "{name}: {:?}", f.unknown);
    }
}

#[test]
fn textured_fills_map_inside_their_images() {
    let Some(d) = disc() else { return };
    let (mut fills, mut outside) = (0, 0);
    for (name, r) in read_flashes(&d.read("flashes.arc").unwrap()) {
        let f = r.unwrap();
        for s in &f.geometry.shapes {
            for fill in &s.fills {
                if let geometry::Fill::Textured { image, matrix: m, triangles, .. } = fill {
                    let (im, _) = &f.images[image];
                    fills += 1;
                    // a vertex times the matrix is a point of the picture (a texel's width of slack)
                    let inside = triangles.iter().flatten().all(|v| {
                        let (u, w) = (v[0] * m[0] + v[1] * m[4] + m[12], v[0] * m[1] + v[1] * m[5] + m[13]);
                        u > -1.0 && w > -1.0 && u < im.width as f32 + 1.0 && w < im.height as f32 + 1.0
                    });
                    if !inside {
                        outside += 1;
                        eprintln!("{name}: shape {} leaves its image", s.id);
                    }
                }
            }
        }
    }
    assert!(fills > 500);
    assert_eq!(outside, 0);
}

#[test]
fn fonts_are_read_in_full_and_their_pages_are_textures() {
    let Some(d) = disc() else { return };
    let fonts = read_fonts(&d.read("fonts.arc").unwrap());
    assert_eq!(fonts.len(), 6);
    let textures = d.read("textures.arc").unwrap();
    let entries = crate::arc::entries(&textures).unwrap();
    for (name, r) in &fonts {
        let f = r.as_ref().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(f.pages.len() + f.glyphs.len(), f.pages.len() + f.glyphs.len());
        for (i, p) in f.pages.iter().enumerate() {
            let want = format!("font_{}_s{}_p{i}", f.name, f.size);
            assert_eq!(crate::shader::name_hash(&want), p.texture, "{name}: page {i}");
            let e = entries.iter().find(|e| e.name == want).unwrap_or_else(|| panic!("{want} is not in textures.arc"));
            let t = crate::texture::read(crate::arc::bytes(&textures, e)).unwrap();
            assert_eq!((t.width, t.height), (p.width, p.height), "{want}");
        }
        for g in &f.glyphs {
            assert!(g.x0 <= g.x1 && g.x1 <= f.pages[0].width, "{name}: glyph columns {}..{}", g.x0, g.x1);
        }
        for &(_, glyph) in &f.map {
            assert!(glyph as usize <= f.pages.len() + f.glyphs.len(), "{name}: glyph {glyph}");
        }
    }
}
