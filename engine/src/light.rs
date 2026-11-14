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

/// What a room's light falls back to when its record has none (the original's 0x802A1240).
pub const NO_LIGHT: f32 = 0.0;

/// A room's ambient and light triples from its 80-byte light record (big-endian words; the
/// original's Light_RoomTriples): the ambient colour (words 4-6) when word 7 is set, and the
/// light's colour (words 12-14) times its downward part (the direction's z, word 10, negated, at
/// least `NO_LIGHT`) when word 15 is set; `NO_LIGHT` in each channel otherwise.
pub fn room_triples(record: &[u32; 20]) -> ([f32; 3], [f32; 3]) {
    let f = |i: usize| f32::from_bits(record[i]);
    let ambient = if record[7] != 0 { [f(4), f(5), f(6)] } else { [NO_LIGHT; 3] };
    let light = if record[15] != 0 {
        let down = -f(10);
        // (only a lesser value is raised: NaN stays, as the original's compare)
        let down = if down < NO_LIGHT { NO_LIGHT } else { down };
        [f(12), f(13), f(14)].map(|c| (f64::from(down) * f64::from(c)) as f32)
    } else {
        [NO_LIGHT; 3]
    };
    (ambient, light)
}

/// The constant a floor group's colours are scaled by (the original's 0x802A1244).
pub const GROUP_K: f32 = 128.0;

/// A floor group's colour base and scale (the original's Light_GroupScale) from its material's two
/// triples (diffuse at +0x44, the ambient's at +0x54) and the room's ambient and light: base =
/// (diffuse x light + ambient's x ambient) x K, fused; scale = diffuse x K, its blue taking the
/// diffuse's green as the original does.
pub fn group_colour(diffuse: [f32; 3], ambient_k: [f32; 3], ambient: [f32; 3], light: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let m = |a: f32, b: f32| (f64::from(a) * f64::from(b)) as f32;
    let base = [0, 1, 2].map(|i| {
        let a = m(ambient_k[i], ambient[i]);
        m(f64::from(diffuse[i]).mul_add(f64::from(light[i]), f64::from(a)) as f32, GROUP_K)
    });
    let scale = [m(diffuse[0], GROUP_K), m(diffuse[1], GROUP_K), m(diffuse[1], GROUP_K)];
    (base, scale)
}

/// A floor vertex's colour from its point's light: each channel min(255, the light times the
/// group's scale plus its base, truncated; fused), alpha 0x80 (the original's Light_RoomFloor).
pub fn vertex_colour(sum: [f32; 3], base: [f32; 3], scale: [f32; 3]) -> [u8; 4] {
    let c = |k: usize| {
        let v = f64::from(sum[k]).mul_add(f64::from(scale[k]), f64::from(base[k])) as f32;
        // (fctiwz: toward zero, saturating; then only the top clamped, the low byte stored)
        let t = v as i32;
        if t > 255 { 255 } else { t as u8 }
    };
    [c(0), c(1), c(2), 0x80]
}

/// The grid points a floor entry's vertices take their light from (the light grid is 39 points a
/// row), in the strip's order: a leading half tile's corner (its flag 0x40 on the row, 0x80 the
/// row after), then each x's two (the row, the next), then a trailing half's corner likewise.
pub fn entry_points(word: u32) -> Vec<(u32, u32)> {
    let [_, row, first, last] = word.to_be_bytes();
    let (row, lead, trail) = (u32::from(row), first & 0xC0, last & 0xC0);
    let (first, last) = (u32::from(first & 0x3F), u32::from(last & 0x3F));
    let mut pairs = (last as i32 - first as i32) + if lead == 0 { 2 } else { 1 };
    if trail != 0 {
        pairs -= 1;
    }
    let mut p = Vec::new();
    let mut x = first;
    if lead != 0 {
        p.push((x, if lead == 0x40 { row } else { row + 1 }));
        x += 1;
    }
    for _ in 0..pairs.max(0) {
        p.push((x, row));
        p.push((x, row + 1));
        x += 1;
    }
    if trail != 0 {
        p.push((x, if trail == 0x40 { row } else { row + 1 }));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vertex_colour_is_truncated_and_clamped_with_alpha_0x80() {
        assert_eq!(vertex_colour([0.5, 1.0, 10.0], [100.0, 0.9, 0.0], [100.0, 100.0, 100.0]), [150, 100, 255, 0x80]);
        // below 0 is not clamped: the original stores the integer's low byte (-3 -> 0xFD)
        assert_eq!(vertex_colour([0.0; 3], [-3.0, 0.0, 255.9], [1.0; 3]), [0xFD, 0, 255, 0x80]);
    }

    #[test]
    fn a_floor_entry_takes_its_corners_in_strip_order() {
        // row 5, x 2..=3, no half tiles: (2,5),(2,6),(3,5),(3,6),(4,5),(4,6)
        assert_eq!(entry_points(0x0005_0203), vec![(2, 5), (2, 6), (3, 5), (3, 6), (4, 5), (4, 6)]);
        // a leading half (0x40: on the row) and a trailing half (0x80: the row after)
        assert_eq!(entry_points(0x0005_4283), vec![(2, 5), (3, 5), (3, 6), (4, 6)]);
    }

    #[test]
    fn a_rooms_light_is_its_colour_times_the_downward_part() {
        let mut rec = [0u32; 20];
        for (i, v) in [(4, 0.25f32), (5, 0.5), (6, 1.0), (10, -0.5), (12, 2.0), (13, 1.0), (14, 0.5)] {
            rec[i] = v.to_bits();
        }
        assert_eq!(room_triples(&rec), ([0.0; 3], [0.0; 3]));
        rec[7] = 1;
        rec[15] = 1;
        assert_eq!(room_triples(&rec), ([0.25, 0.5, 1.0], [1.0, 0.5, 0.25]));
        // a light from below gives none
        rec[10] = 0.5f32.to_bits();
        assert_eq!(room_triples(&rec).1, [0.0; 3]);
    }

    #[test]
    fn a_group_colour_is_the_originals_blue_scale_included() {
        let (base, scale) = group_colour([1.0, 0.5, 0.25], [1.0; 3], [0.25, 0.5, 1.0], [0.5; 3]);
        assert_eq!(base, [96.0, 96.0, 144.0]);
        // the blue scale takes the diffuse's green, as the original
        assert_eq!(scale, [128.0, 64.0, 64.0]);
    }

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
