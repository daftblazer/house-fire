//! The game's single-precision maths, as its own code computes it, step for step (each step
//! rounded to single, the fused steps fused): fdlibm's float sine and cosine (0x8010E450,
//! 0x8010E288, with their kernels 0x8010E698 and 0x8010E5AC and the argument reduction
//! 0x8010E958) and the paired vector normalise (0x801221E4). The engine uses them where the
//! original's results are checked bit for bit (the sun's direction, `light::sun_direction`).

/// One single-precision step from a double result.
fn s(v: f64) -> f32 {
    v as f32
}

/// a x b + c, fused, rounded to single.
fn fma(a: f32, b: f32, c: f32) -> f32 {
    s(f64::from(a).mul_add(f64::from(b), f64::from(c)))
}

fn bits(x: f32) -> i32 {
    x.to_bits() as i32
}

/// The sine kernel on [-pi/4, pi/4] (x, its tail y, whether y is used).
fn kernel_sin(x: f32, y: f32, iy: bool) -> f32 {
    let ix = bits(x) & 0x7fff_ffff;
    if ix < 0x3200_0000 && x as i32 == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let mut r = fma(z, f32::from_bits(0x2f2e_c9d3), f32::from_bits(0xb2d7_2f34));
    r = fma(z, r, f32::from_bits(0x3638_ef1b));
    r = fma(z, r, f32::from_bits(0xb950_0d01));
    r = fma(z, r, f32::from_bits(0x3c08_8889));
    let s1 = f32::from_bits(0xbe2a_aaab);
    if !iy {
        fma(v, fma(z, r, s1), x)
    } else {
        let vr = v * r;
        let a = fma(y, 0.5, -vr);
        let b = fma(z, a, -y);
        x - (b - v * s1)
    }
}

/// The cosine kernel on [-pi/4, pi/4].
fn kernel_cos(x: f32, y: f32) -> f32 {
    let ix = bits(x) & 0x7fff_ffff;
    if ix < 0x3200_0000 && x as i32 == 0 {
        return 1.0;
    }
    let z = x * x;
    let mut p = fma(z, f32::from_bits(0xad47_d74e), f32::from_bits(0x310f_74f6));
    p = fma(z, p, f32::from_bits(0xb493_f27c));
    p = fma(z, p, f32::from_bits(0x37d0_0d01));
    p = fma(z, p, f32::from_bits(0xbab6_0b61));
    p = fma(z, p, f32::from_bits(0x3d2a_aaab));
    let r = z * p;
    let a = fma(z, r, -(x * y));
    if ix < 0x3e99_999a {
        1.0 - fma(z, 0.5, -a)
    } else {
        let qx = if ix > 0x3f48_0000 { 0.281_25 } else { f32::from_bits((ix - 0x0100_0000) as u32) };
        let hz = fma(z, 0.5, -qx);
        (1.0 - qx) - (hz - a)
    }
}

const PIO2_1: f32 = f32::from_bits(0x3fc9_0f80);
const PIO2_1T: f32 = f32::from_bits(0x3735_4443);
const PIO2_2: f32 = f32::from_bits(0x3735_4400);
const PIO2_2T: f32 = f32::from_bits(0x2e85_a308);
const PIO2_3: f32 = f32::from_bits(0x2e85_a300);
const PIO2_3T: f32 = f32::from_bits(0x248d_3132);
const INV_PIO2: f32 = f32::from_bits(0x3f22_f984);

/// The high words (masked to 0x7fffff00) of n x pi/2 for n 1 to 32.
const NPIO2_HW: [i32; 32] = [
    0x3fc9_0f00, 0x4049_0f00, 0x4096_cb00, 0x40c9_0f00, 0x40fb_5300, 0x4116_cb00, 0x412f_ed00, 0x4149_0f00, 0x4162_3100, 0x417b_5300, 0x418a_3a00, 0x4196_cb00, 0x41a3_5c00, 0x41af_ed00, 0x41bc_7e00,
    0x41c9_0f00, 0x41d5_a000, 0x41e2_3100, 0x41ee_c200, 0x41fb_5300, 0x4203_f200, 0x420a_3a00, 0x4210_8300, 0x4216_cb00, 0x421d_1400, 0x4223_5c00, 0x4229_a500, 0x422f_ed00, 0x4236_3600, 0x423c_7e00,
    0x4242_c700, 0x4249_0f00,
];

/// x less n x pi/2 as a head and tail, and n (the game's reduction up to 2^7 x pi/2; a larger x
/// is never asked of it here and gives `None`).
fn rem_pio2(x: f32) -> Option<(i32, f32, f32)> {
    let hx = bits(x);
    let ix = hx & 0x7fff_ffff;
    if ix <= 0x3f49_0fd8 {
        return Some((0, x, 0.0));
    }
    if ix < 0x4016_cbe4 {
        let (sign, n) = if hx > 0 { (1.0f32, 1) } else { (-1.0, -1) };
        let mut z = x - sign * PIO2_1;
        let (y0, y1);
        if (hx & 0x7fff_fff0) != 0x3fc9_0fd0 {
            y0 = z - sign * PIO2_1T;
            y1 = (z - y0) - sign * PIO2_1T;
        } else {
            z -= sign * PIO2_2;
            y0 = z - sign * PIO2_2T;
            y1 = (z - y0) - sign * PIO2_2T;
        }
        return Some((n, y0, y1));
    }
    if ix > 0x4349_0f80 {
        return None;
    }
    let t = x.abs();
    let n = fma(t, INV_PIO2, 0.5) as i32;
    let f = n as f32;
    let mut r = t - f * PIO2_1;
    let mut w = f * PIO2_1T;
    let mut y0 = r - w;
    if !(n < 32 && (ix & 0x7fff_ff00) != NPIO2_HW[(n - 1) as usize]) {
        let j = ix >> 23;
        if j - ((bits(y0) >> 23) & 0xff) > 8 {
            let t2 = r;
            w = f * PIO2_2;
            r = t2 - w;
            w = fma(f, PIO2_2T, -((t2 - r) - w));
            y0 = r - w;
            if j - ((bits(y0) >> 23) & 0xff) > 25 {
                let t3 = r;
                w = f * PIO2_3;
                r = t3 - w;
                w = fma(f, PIO2_3T, -((t3 - r) - w));
                y0 = r - w;
            }
        }
    }
    let y1 = (r - y0) - w;
    Some(if hx < 0 { (-n, -y0, -y1) } else { (n, y0, y1) })
}

/// The game's sinf (`None` beyond its reduction's range, never asked here).
pub fn sin(x: f32) -> Option<f32> {
    let (n, y0, y1) = rem_pio2(x)?;
    if n == 0 && y1 == 0.0 && y0.to_bits() == x.to_bits() {
        return Some(kernel_sin(x, 0.0, false));
    }
    Some(match n & 3 {
        0 => kernel_sin(y0, y1, true),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, y1, true),
        _ => -kernel_cos(y0, y1),
    })
}

/// The game's cosf.
pub fn cos(x: f32) -> Option<f32> {
    let (n, y0, y1) = rem_pio2(x)?;
    if n == 0 && y1 == 0.0 && y0.to_bits() == x.to_bits() {
        return Some(kernel_cos(x, 0.0));
    }
    Some(match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => -kernel_sin(y0, y1, true),
        2 => -kernel_cos(y0, y1),
        _ => kernel_sin(y0, y1, true),
    })
}

/// The paired vector normalise (0x801221E4): the squared length (x^2, y^2 rounded, z^2 fused onto
/// x^2, y^2 added), its reciprocal square root (the recompiled estimate is exact, one Newton step
/// after it as the code has it), each part scaled.
pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let xx = v[0] * v[0];
    let yy = v[1] * v[1];
    let len2 = fma(v[2], v[2], xx) + yy;
    let e = 1.0 / f64::from(len2).sqrt();
    let ee = s(e * e);
    let half = s(e * 0.5);
    let k = s(-f64::from(ee).mul_add(f64::from(len2), -3.0));
    let scale = k * half;
    v.map(|c| c * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_and_cosine_near_the_libraries() {
        for k in 0..200 {
            let x = k as f32 * 0.031_4;
            assert!((sin(x).unwrap() - x.sin()).abs() < 1e-6, "sin {x}");
            assert!((cos(x).unwrap() - x.cos()).abs() < 1e-6, "cos {x}");
        }
        assert_eq!(sin(1e6), None);
    }

    #[test]
    fn a_normalised_vector_has_length_one() {
        let n = normalize([0.5, 0.5, 0.2]);
        assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) - 1.0).abs() < 1e-6);
    }
}
