//! The skinned-vertex blend of the original's loop at 0x801A6C48 (in fn_801A6ADC): each position
//! through up to four bone matrices by the vertex's four weight bytes, the normal by the
//! heaviest bone's. Computed in the original's own arithmetic (fused multiply-adds, the
//! reciprocal square root with one Newton step), so it matches what the loop sent to the bit
//! (`LOCKSTEP_ANIM_SKIN`); the renderer's GPU skinning is held to it.

use crate::anim::fma32;

/// The weight byte's scale, the constant at 0x802C1890: 1/255 as a single.
const WEIGHT_SCALE: f32 = 1.0 / 255.0;
/// The normal's scale, the constant at 0x802C1898: 127.
const NORMAL_SCALE: f32 = 127.0;
/// PSVECNormalize's 0.5 and 3.0 (the small-data constants below r13 at -0x4F1C and -0x4F18).
const HALF: f32 = 0.5;
const THREE: f32 = 3.0;

pub const FLAG_NORMALS: u32 = 1;
pub const FLAG_COLOURS: u32 = 2;
pub const FLAG_UV: u32 = 4;
pub const FLAG_GENERATED_UV: u32 = 8;

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
