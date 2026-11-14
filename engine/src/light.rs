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

/// The squared distance a light's ranking takes when it is exactly this (the original's
/// 0x802BA7A8), and what it takes instead (0x802BA7AC).
pub const RANK_AT: f32 = 0.0;
pub const RANK_INSTEAD: f32 = 0.0001;

/// The squared distance between two points, rounded as the original's paired dot product.
pub fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let yy = (f64::from(d[1]) * f64::from(d[1])) as f32;
    let zz = (f64::from(d[2]) * f64::from(d[2])) as f32;
    (f64::from(d[0]).mul_add(f64::from(d[0]), f64::from(yy)) as f32) + zz
}

/// Ranks lights into a list kept strongest first (the original's Light_Rank). Each light (its
/// index from `first`, its position and reach) scores reach^2 / its squared distance from the
/// point, and goes in before the first slot it beats, the slots after moving down one and the
/// last dropping off. A slot is its index (the top 16 bits of its first word, the rest kept) and
/// its score.
pub fn rank(point: [f32; 3], first: u32, lights: &[([f32; 3], f32)], list: &mut [(u32, f32)]) {
    for (k, (at, reach)) in lights.iter().enumerate() {
        let mut d2 = distance2(*at, point);
        if d2 == RANK_AT {
            d2 = RANK_INSTEAD;
        }
        let r2 = (f64::from(*reach) * f64::from(*reach)) as f32;
        let score = r2 / d2;
        let Some(j) = list.iter().position(|s| score > s.1) else { continue };
        list.copy_within(j..list.len() - 1, j + 1);
        let index = (first + k as u32) & 0xFFFF;
        list[j] = ((index << 16) | (list[j].0 & 0xFFFF), score);
    }
}

/// The light grid's points a row (and rows): 0..=38 each way.
pub const GRID_SIZE: usize = 39;

/// The rooms each light grid point lists: up to four room ids, 0xFF ending them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRooms {
    cells: Vec<[u8; 4]>,
}

impl Default for CellRooms {
    fn default() -> Self {
        CellRooms { cells: vec![[0xFF; 4]; GRID_SIZE * GRID_SIZE] }
    }
}

impl CellRooms {
    /// A point's rooms (`None` off the grid).
    pub fn at(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let ok = |v: i32| (0..GRID_SIZE as i32).contains(&v);
        (ok(x) && ok(y)).then(|| self.cells[x as usize * GRID_SIZE + y as usize])
    }

    /// Adds a room to a point (the original's Light_AddCellRoom): into its first empty slot, or
    /// none when it is listed already; a full point and one off the grid are left alone.
    pub fn add(&mut self, x: i32, y: i32, room: u8) {
        let ok = |v: i32| (0..GRID_SIZE as i32).contains(&v);
        if !(ok(x) && ok(y)) {
            return;
        }
        let cell = &mut self.cells[x as usize * GRID_SIZE + y as usize];
        if let Some(slot) = cell.iter_mut().find(|s| **s == 0xFF || **s == room) {
            *slot = room;
        }
    }
}

/// The light grid's height, an empty slot's score and a taken one's (the original's 0x802BA7B8,
/// 0x802BA7C8 and 0x802BA7CC).
pub const GRID_Z: f32 = 0.0;
pub const SLOT_EMPTY: f32 = -1.0;
pub const SLOT_TAKEN: f32 = -2.0;

/// A light grid cell's four lights for one of its rooms (the original's Light_BuildGrid): the
/// room's own lights ranked into four slots, then, from the first slot past the filled ones (at
/// most the third), its other lights against what is left, then the slots taken strongest first
/// (the first of equals; with none above `SLOT_TAKEN`, the first slot again). An empty slot is
/// 0xFFFF. `own` and `other` are index ranges into `lights` (position, reach).
pub fn cell_lights(point: [f32; 3], own: (u32, u32), other: (u32, u32), lights: &[([f32; 3], f32)]) -> Option<[u16; 4]> {
    let range = |r: (u32, u32)| lights.get(r.0 as usize..r.1.max(r.0) as usize);
    let (own_l, other_l) = (range(own)?, range(other)?);
    let mut list = [(0xFFFF_0000u32, SLOT_EMPTY); 4];
    rank(point, own.0, own_l, &mut list);
    let filled = list.iter().take_while(|s| s.0 >> 16 != 0xFFFF).count().min(2);
    rank(point, other.0, other_l, &mut list[filled..]);
    let mut out = [0u16; 4];
    for o in &mut out {
        let mut best = (0, SLOT_TAKEN);
        for (k, s) in list.iter().enumerate() {
            if s.1 > best.1 {
                best = (k, s.1);
            }
        }
        list[best.0].1 = SLOT_TAKEN;
        *o = (list[best.0].0 >> 16) as u16;
    }
    Some(out)
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
    fn a_cell_lists_each_room_once_and_at_most_four() {
        let mut rooms = CellRooms::default();
        for r in [3, 1, 3, 2, 4, 5] {
            rooms.add(5, 6, r);
        }
        assert_eq!(rooms.at(5, 6), Some([3, 1, 2, 4]));
        rooms.add(39, 0, 1);
        rooms.add(-1, 0, 1);
        assert_eq!(rooms.at(39, 0), None);
        assert_eq!(rooms.at(38, 38), Some([0xFF; 4]));
    }

    #[test]
    fn a_cell_keeps_two_places_for_the_other_lights() {
        // three strong own lights (0-2) and two weaker others (3-4): the third own light is
        // ranked again with the others for the last two places; slots come out strongest first
        let lights = [
            ([0.0, 0.0, 1.0], 4.0),
            ([0.0, 0.0, 1.0], 3.0),
            ([0.0, 0.0, 1.0], 2.5),
            ([0.0, 0.0, 1.0], 2.0),
            ([0.0, 0.0, 1.0], 1.0),
        ];
        assert_eq!(cell_lights([0.0; 3], (0, 3), (3, 5), &lights), Some([0, 1, 2, 3]));
        // no own lights: the others fill all four, the rest empty
        assert_eq!(cell_lights([0.0; 3], (0, 0), (3, 5), &lights), Some([3, 4, 0xFFFF, 0xFFFF]));
        assert_eq!(cell_lights([0.0; 3], (0, 9), (3, 5), &lights), None);
    }

    #[test]
    fn lights_are_ranked_strongest_first_and_the_weakest_drop_off() {
        let mut list = vec![(0x0000_ABCD, 0.0f32); 2];
        // scores 1/4 (index 5), 4/1 (6) and 1/1 (7, displacing 5 off the end)
        let lights = [([2.0, 0.0, 0.0], 1.0), ([1.0, 0.0, 0.0], 2.0), ([0.0, 1.0, 0.0], 1.0)];
        rank([0.0; 3], 5, &lights, &mut list);
        assert_eq!(list, vec![(0x0006_ABCD, 4.0), (0x0007_ABCD, 1.0)]);
        // a light on the point scores by the guard distance
        let mut one = vec![(0, 0.0f32)];
        rank([0.0; 3], 0, &[([0.0; 3], 1.0)], &mut one);
        assert_eq!(one[0].1, 1.0 / 0.0001);
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
