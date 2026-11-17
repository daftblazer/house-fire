//! The engine's Sim textures against the log's (LOCKSTEP_SIM_TEXTURES=1): for each Sim the save
//! made, `sim_texture` composes the image and its palette from the disc and the Sim's
//! description; the log's run-time textures are the 128 x 128 C8 images its draws were set up
//! with ('`') through the palettes loaded in their TLUT slots ('{'). Each log texture is
//! paired with the Sim whose picture it is nearest to.
//!
//! Lines: the logged textures, then for each Sim and each pairing the mean absolute error of
//! the engine's picture (before the palette) against the log's image through its palette, and
//! once the quantizer is the original's, the number of differing indices and palette entries.

use housefire::disc::Disc;
use housefire::figure::Cast;
use housefire::outfit::Description;
use housefire::sim_texture::{self, Conditions};
use housefire::texture::{self, Kind};
use housefire::tree_log::Log;

/// A logged Sim texture: its image (C8, 128 x 128) and the palette its draw loaded.
struct Logged {
    tick: u32,
    texels: Vec<u8>,
    palette: Vec<u16>,
    format: u8,
}

fn logged(log: &Log) -> Vec<Logged> {
    let mut out: Vec<Logged> = Vec::new();
    for tl in &log.tex_loads {
        let image = tl.image();
        let Some(x) = log.textures.iter().rfind(|x| x.image == image && x.tick <= tl.tick).or_else(|| log.textures.iter().find(|x| x.image == image)) else { continue };
        let Some(t) = log.texels.iter().find(|t| t.hash == x.hash && t.format == 9 && t.width == 128 && t.height == 128) else { continue };
        let name = u32::from_be_bytes(tl.obj[24..28].try_into().unwrap());
        let Some(p) = log.tlut_loads.iter().rfind(|p| p.slot == name && (p.tick, p.strips_before) <= (tl.tick, tl.strips_before)) else { continue };
        if !out.iter().any(|o| o.texels == t.bytes && o.palette == p.entries) {
            out.push(Logged { tick: tl.tick, texels: t.bytes.clone(), palette: p.entries.clone(), format: p.format() });
        }
    }
    out
}

/// The logged image through its palette: red, green, blue.
fn decoded(l: &Logged) -> Vec<[u8; 3]> {
    let pal: Vec<u8> = l.palette.iter().flat_map(|e| e.to_be_bytes()).collect();
    texture::decode_indexed(Kind::Palette8, 128, 128, &l.texels, &pal, l.format).unwrap_or_default().iter().map(|&c| [c as u8, (c >> 8) as u8, (c >> 16) as u8]).collect()
}

pub fn study(log: &Log, d: &Disc, ngh: &[u8]) {
    let logs = logged(log);
    println!("sim textures: the log has {} 128 x 128 C8 images with their palettes", logs.len());
    for (i, l) in logs.iter().enumerate() {
        println!("  log {i}: first set up at tick {}, {} palette entries of format {}", l.tick, l.palette.len(), l.format);
    }
    let Some(cast) = Cast::read(d, ngh) else { return println!("sim textures: no cast") };
    let arc = d.read("rletextu.arc").unwrap_or_default();
    let made = housefire::save::read_created_sims(ngh).unwrap_or_default();
    let decoded: Vec<Vec<[u8; 3]>> = logs.iter().map(decoded).collect();
    for m in &made {
        let desc = Description::from_save(m);
        for tinted_clothes in [true, false] {
            let stack = sim_texture::layer_stack(&cast.tables, &desc, Conditions { tinted_clothes });
            let Some(pic) = sim_texture::picture(&arc, &stack) else {
                println!("sim {} {}: a layer is not in rletextu.arc ({} layers)", m.first_name, m.last_name, stack.len());
                continue;
            };
            let errs: Vec<String> = decoded
                .iter()
                .enumerate()
                .map(|(i, img)| {
                    let e: u64 = img.iter().zip(&pic).map(|(a, b)| a.iter().zip(b).map(|(&x, &y)| u64::from(x.abs_diff(y))).sum::<u64>()).sum();
                    format!("log {i}: {:.2}", e as f64 / (3 * 128 * 128) as f64)
                })
                .collect();
            println!("sim {} {} (male {}, adult {}, {} layers, tinted clothes {tinted_clothes}): mean abs error {}", m.first_name, m.last_name, desc.male, desc.adult, stack.len(), errs.join(", "));
        }
    }
}
