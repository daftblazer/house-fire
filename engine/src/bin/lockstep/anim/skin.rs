//! The CPU reference skinner (LOCKSTEP_ANIM_SKIN): the original's skinned-vertex loop (the loop
//! at 0x801A6C48 in fn_801A6ADC) computed from the logged inputs ('$' bone matrices, '+'
//! positions, weights and normals) and checked to the bit against the bytes the loop sent. The
//! shipping engine skins on the GPU; this is what that path is later held to.
//!
//! Not checked: colours and texture coordinates (copied through, and their inputs are not
//! logged) and the generated texture coordinates (a sphere map through two device matrices the
//! log does not hold); a primitive with those is counted apart.

use crate::*;
use housefire::skin::{skin, FLAG_COLOURS, FLAG_GENERATED_UV, FLAG_NORMALS, FLAG_UV};
use housefire::tree_log::Log;

/// A vertex's size in what the loop sends: three big-endian floats, then the normal (3), colour
/// (4), texture coordinate (4) and generated coordinate (two floats) as flagged.
fn sent_stride(flags: u32) -> usize {
    let has = |f: u32| flags & f != 0;
    12 + if has(FLAG_NORMALS) { 3 } else { 0 }
        + if has(FLAG_COLOURS) { 4 } else { 0 }
        + if has(FLAG_UV) { 4 } else { 0 }
        + if has(FLAG_GENERATED_UV) && has(FLAG_NORMALS) { 8 } else { 0 }
}

#[derive(Default)]
struct Counts {
    primitives: usize,
    vertices: usize,
    different_positions: usize,
    different_normals: usize,
}

pub fn skin_study(log: &Log) {
    // by (normals, generated uv)
    let mut by_kind: BTreeMap<(bool, bool), Counts> = BTreeMap::new();
    let mut malformed = 0;
    for v in &log.skin_vertices {
        let Some(rec) = log.skins.get(v.skin) else { continue };
        let n = v.count as usize;
        let stride = sent_stride(v.flags);
        if v.positions.len() != n * 8 || v.weights.len() != n * 4 || v.sent.len() != n * stride {
            malformed += 1;
            continue;
        }
        let got = skin(&rec.bones, v.heaviest as usize, n, &v.positions, &v.weights, &v.normals);
        let c = by_kind.entry((v.flags & FLAG_NORMALS != 0, v.flags & FLAG_GENERATED_UV != 0)).or_default();
        c.primitives += 1;
        c.vertices += n;
        for (g, sent) in got.iter().zip(v.sent.chunks(stride)) {
            let differs = g.position.iter().zip(sent.chunks(4)).any(|(f, b)| f.to_bits().to_be_bytes() != b);
            c.different_positions += usize::from(differs);
            if let Some(nrm) = g.normal {
                c.different_normals += usize::from(nrm.map(|b| b as u8) != sent[12..15]);
            }
        }
    }
    println!("skinned vertices from the reference ({} primitives, {malformed} malformed):", log.skin_vertices.len());
    let (mut dp, mut dn) = (0, 0);
    for ((normals, generated), c) in &by_kind {
        let kind = format!("{} normals{}", if *normals { "with" } else { "without" }, if *generated { ", generated uv set apart (its coordinates not checked)" } else { "" });
        println!("  {kind}: {} primitives, {} vertices, {} positions DIFFERENT, {} normals DIFFERENT", c.primitives, c.vertices, c.different_positions, c.different_normals);
        dp += c.different_positions;
        dn += c.different_normals;
    }
    println!("  positions DIFFERENT: {dp}, normals DIFFERENT: {dn}");
}
