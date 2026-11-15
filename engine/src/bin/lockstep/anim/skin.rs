//! The CPU reference skinner (LOCKSTEP_ANIM_SKIN): the original's skinned-vertex loop (the loop
//! at 0x801A6C48 in fn_801A6ADC) computed from the logged inputs ('$' bone matrices, '+'
//! positions, weights and normals) and checked to the bit against the bytes the loop sent. The
//! shipping engine skins on the GPU; this is what that path is later held to.
//!
//! Not checked: colours and texture coordinates (copied through, and their inputs are not
//! logged) and the generated texture coordinates (a sphere map through two device matrices the
//! log does not hold); a primitive with those is counted apart.

use crate::*;
use housefire::anim::fma32;
use housefire::tree_log::Log;

/// The weight byte's scale, the constant at 0x802C1890: 1/255 as a single.
const WEIGHT_SCALE: f32 = 1.0 / 255.0;
/// The normal's scale, the constant at 0x802C1898: 127.
const NORMAL_SCALE: f32 = 127.0;
/// PSVECNormalize's 0.5 and 3.0 (the small-data constants below r13 at -0x4F1C and -0x4F18).
const HALF: f32 = 0.5;
const THREE: f32 = 3.0;

const FLAG_NORMALS: u32 = 1;
const FLAG_COLOURS: u32 = 2;
const FLAG_UV: u32 = 4;
const FLAG_GENERATED_UV: u32 = 8;

/// One skinned vertex: the blended position and, with normals, the three quantised bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skinned {
    pub position: [f32; 3],
    pub normal: Option<[i8; 3]>,
}

fn s16_be(p: &[u8]) -> f32 {
    f32::from(i16::from_be_bytes([p[0], p[1]]))
}

/// A bone's row through a point: PSMTXMultVec's order, (m0*x + m2*z fused) + (m1*y + m3 fused).
fn transform_row(m: &[f32], x: f32, y: f32, z: f32) -> f32 {
    fma32(m[2], z, m[0] * x) + fma32(m[3], 1.0, m[1] * y)
}

/// A normal's rotation by a bone's 3x3: PSMTXMultVecSR's order, (m0*x + m1*y) with m2*z fused.
fn rotate_row(m: &[f32], x: f32, y: f32, z: f32) -> f32 {
    fma32(m[2], z, m[0] * x + m[1] * y)
}

/// A float stored through a signed-byte quantiser with no scale: clamped, truncated, NaN to 0.
fn quantise_s8(f: f32) -> i8 {
    if f.is_nan() { 0 } else { f.clamp(-128.0, 127.0) as i8 }
}

/// PSVECNormalize's inverse length: the reciprocal square root (exact here, as in the
/// recompiled code) refined by one Newton step, in the rounding order of the paired singles.
fn inverse_length(sq: f32) -> f32 {
    let est = 1.0 / f64::from(sq).sqrt();
    let e2 = (est * est) as f32;
    let half_est = (est * f64::from(HALF)) as f32;
    let corr = (-f64::from(e2).mul_add(f64::from(sq), -f64::from(THREE))) as f32;
    corr * half_est
}

/// Skins a primitive's vertices. `positions` are 8 bytes a vertex (s16 x, y, z big-endian, 2
/// spare), `weights` 4 a vertex, `normals` 4 a vertex (s8 x, y, z) or empty; `heaviest` is the
/// bone carried in from the vertex before.
pub fn skin(bones: &[[f32; 12]; 4], heaviest: usize, count: usize, positions: &[u8], weights: &[u8], normals: &[u8]) -> Vec<Skinned> {
    let mut heavy = heaviest & 3;
    (0..count)
        .map(|i| {
            let p = &positions[i * 8..];
            let (x, y, z) = (s16_be(p), s16_be(&p[2..]), s16_be(&p[4..]));
            let mut acc = [0.0f32; 3];
            let mut max_weight = 0;
            for (j, &wb) in weights[i * 4..i * 4 + 4].iter().enumerate() {
                if wb == 0 {
                    continue;
                }
                let w = f32::from(wb) * WEIGHT_SCALE;
                for (r, a) in acc.iter_mut().enumerate() {
                    *a = fma32(transform_row(&bones[j][r * 4..], x, y, z), w, *a);
                }
                if wb > max_weight {
                    max_weight = wb;
                    heavy = j;
                }
            }
            let normal = (!normals.is_empty()).then(|| {
                let n = &normals[i * 4..];
                let (vx, vy, vz) = (f32::from(n[0] as i8), f32::from(n[1] as i8), f32::from(n[2] as i8));
                let m = &bones[heavy];
                let r: [f32; 3] = core::array::from_fn(|k| rotate_row(&m[k * 4..], vx, vy, vz));
                let inv = inverse_length(fma32(r[2], r[2], r[0] * r[0]) + r[1] * r[1]);
                r.map(|c| quantise_s8(c * inv * NORMAL_SCALE))
            });
            Skinned { position: acc, normal }
        })
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> [f32; 12] {
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    }

    #[test]
    fn full_weight_identity_keeps_position() {
        let bones = [identity(); 4];
        let mut pos = vec![0u8; 8];
        pos[..2].copy_from_slice(&100i16.to_be_bytes());
        pos[2..4].copy_from_slice(&(-7i16).to_be_bytes());
        let out = skin(&bones, 0, 1, &pos, &[255, 0, 0, 0], &[]);
        assert!((out[0].position[0] - 100.0).abs() < 1e-3);
        assert!((out[0].position[1] + 7.0).abs() < 1e-4);
        assert_eq!(out[0].normal, None);
    }

    #[test]
    fn normal_is_scaled_to_127_and_zero_normal_is_zero() {
        let bones = [identity(); 4];
        let pos = vec![0u8; 16];
        let out = skin(&bones, 0, 2, &pos, &[0; 8], &[0, 0, 64, 0, 0, 0, 0, 0]);
        assert_eq!(out[0].normal, Some([0, 0, 127]));
        assert_eq!(out[1].normal, Some([0, 0, 0]));
    }

    #[test]
    fn heaviest_is_carried_and_first_wins_ties() {
        let mut b1 = identity();
        b1[0] = -1.0;
        let bones = [identity(), b1, identity(), identity()];
        let pos = vec![0u8; 16];
        // vertex 0: a tie between bones 0 and 1 goes to bone 0; vertex 1 has no weight and keeps it.
        let out = skin(&bones, 1, 2, &pos, &[10, 10, 0, 0, 0, 0, 0, 0], &[64, 0, 0, 0, 64, 0, 0, 0]);
        assert_eq!(out[0].normal.unwrap()[0], 127);
        assert_eq!(out[1].normal.unwrap()[0], 127);
    }
}
