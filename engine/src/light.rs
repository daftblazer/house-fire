//! The lot's light: the light falling on the floor's grid of points, summed from the lights each
//! point's cell lists for its room (the original's LightGrid_Sum, `docs/assets.md`).

/// A light as the grid sums it: its place, its height above the floor, its reach and its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub reach: f32,
    pub colour: [f32; 3],
}

impl Light {
    /// A light from the original's 32 bytes (as words): x, y, height, reach, colour.
    pub fn from_words(w: &[u32; 8]) -> Light {
        let f = |k: usize| f32::from_bits(w[k]);
        Light { x: f(0), y: f(1), height: f(2), reach: f(3), colour: [f(4), f(5), f(6)] }
    }
}

/// The light falling on the point (x, y) of the floor from its cell's lights: for each, its
/// colour times max(height / distance, 0) times min((reach / distance)^2, 1) -- light falling at
/// a slant, and falling off with distance. Rounded where the original rounds (single precision,
/// but the square root's reciprocal and the distance's fused multiply-add exact).
pub fn point_light(x: f32, y: f32, lights: &[Light]) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    for l in lights {
        let o = [l.x - x, l.y - y, l.height];
        let (yy, zz) = (o[1] * o[1], o[2] * o[2]);
        let d2 = (f64::from(o[0]).mul_add(f64::from(o[0]), f64::from(yy)) as f32) + zz;
        let inverse = 1.0 / f64::from(d2).sqrt();
        let mut slant = (f64::from(l.height) * inverse) as f32;
        // (not above 0, or NaN, as the original's compare-and-branch)
        if slant.is_nan() || slant <= 0.0 {
            slant = 0.0;
        }
        let mut falloff = (f64::from(l.reach) * inverse) as f32;
        falloff *= falloff;
        if falloff.is_nan() || falloff >= 1.0 {
            falloff = 1.0;
        }
        let k = slant * falloff;
        for (s, c) in sum.iter_mut().zip(l.colour) {
            *s += k * c;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_straight_above_within_reach_gives_its_colour() {
        let l = Light { x: 3.0, y: 4.0, height: 2.0, reach: 5.0, colour: [0.5, 0.25, 1.0] };
        assert_eq!(point_light(3.0, 4.0, &[l]), [0.5, 0.25, 1.0]);
        // farther than its reach: falling off, and at a slant
        let far = point_light(13.0, 4.0, &[l]);
        assert!(far[0] > 0.0 && far[0] < 0.5 * 0.3, "{far:?}");
        assert_eq!(point_light(0.0, 0.0, &[]), [0.0; 3]);
    }
}
