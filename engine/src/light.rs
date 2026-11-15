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

/// How far a window's two lights sit from its wall (the original's local offset, 0.8 across).
pub const WINDOW_LIGHT_OFFSET: f32 = 0.8;

/// A window's two lights' places on the floor plan (the original's window light update: the
/// window's transform applied to (0, +-0.8, 0)). The transform sits on the window's wall: its
/// leading tile's centre moved half a tile the way it faces (facing 0: +x, 2: -y, 4: -x, 6:
/// +y; 2 not seen in the logs). The lights are 0.8 either side of that, along the facing: the
/// first in front, the second behind.
pub fn window_light_places(tile: (i32, i32), facing: u8) -> Option<[(f32, f32); 2]> {
    let (dx, dy) = match facing {
        0 => (1.0f32, 0.0f32),
        2 => (0.0, -1.0),
        4 => (-1.0, 0.0),
        6 => (0.0, 1.0),
        _ => return None,
    };
    let c = (tile.0 as f32 + 0.5 + 0.5 * dx, tile.1 as f32 + 0.5 + 0.5 * dy);
    let at = |s: f32| (c.0 + s * WINDOW_LIGHT_OFFSET * dx, c.1 + s * WINDOW_LIGHT_OFFSET * dy);
    Some([at(1.0), at(-1.0)])
}

/// The time of day's keys (the original builds them in code, 0x8004B784, into 0x802E60C4): an
/// hour, the sun's colour and factor, the ambient's colour and factor, and a last value (`TimeOfDay::extra`).
pub const TIME_KEYS: [(i32, [f32; 9]); 10] = [
    (0, [0.7, 0.7, 1.0, 0.35, 0.3, 0.3, 1.0, 0.25, 0.35]),
    (4, [0.7, 0.7, 1.0, 0.4, 0.5, 0.5, 1.0, 0.35, 0.2]),
    (5, [1.0, 0.4, 0.8, 1.25, 0.6, 0.5, 1.0, 0.4, 0.01]),
    (6, [1.0, 0.5, 0.6, 1.55, 0.5, 0.5, 1.0, 0.6, 0.35]),
    (7, [1.0, 1.0, 0.7, 1.3, 0.8, 0.8, 1.0, 0.4, 0.5]),
    (12, [1.0, 1.0, 0.7, 1.3, 0.8, 0.8, 1.0, 0.4, 0.65]),
    (17, [1.0, 1.0, 0.7, 1.3, 0.8, 0.8, 1.0, 0.4, 0.5]),
    (18, [1.0, 0.7, 0.25, 1.55, 0.6, 0.6, 0.8, 0.5, 0.5]),
    (19, [1.0, 0.7, 0.25, 1.1, 0.6, 0.6, 0.8, 0.6, 0.01]),
    (20, [0.7, 0.7, 1.0, 0.4, 0.5, 0.5, 1.0, 0.35, 0.2]),
];

/// A day's seconds: a span or elapsed time below 0 wraps by it.
pub const DAY_SECONDS: f32 = 86400.0;

/// The time of day's light at a clock time (the original's Light_TimeOfDay, 0x8004A1F4, its
/// colour part; the sun's direction is not ported yet).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeOfDay {
    pub sun: [f32; 3],
    pub sun_factor: f32,
    pub ambient: [f32; 3],
    pub ambient_factor: f32,
    /// The keys' last value at the time (given out through the call's last argument).
    pub extra: f32,
}

/// The key for an hour (0x8004A194): the one at the hour, else the one before the first later
/// key (wrapping round), else the last.
pub fn time_key(hour: i32) -> usize {
    for (i, (h, _)) in TIME_KEYS.iter().enumerate() {
        if hour == *h {
            return i;
        }
        if hour < *h {
            return (i + 9) % 10;
        }
    }
    9
}

/// The light at `hour`:`minute`:`second` (minute and second taken mod 60): t is the time since
/// the key over the span to the next (each wrapped by a day when below 0); a colour is next x t +
/// key x (1 - t), a factor and the last value next fused onto key x (1 - t).
pub fn time_of_day(hour: i32, minute: i32, second: i32) -> TimeOfDay {
    let i = time_key(hour);
    let j = (i + 1) % 10;
    let (a, b) = (&TIME_KEYS[i], &TIME_KEYS[j]);
    let wrap = |v: f32| if v < 0.0 { v + DAY_SECONDS } else { v };
    let span = wrap(((b.0 - a.0) * 3600) as f32);
    let since = wrap(((hour - a.0) * 3600 + (minute % 60) * 60 + second % 60) as f32);
    let t = since / span;
    let u = 1.0 - t;
    let colour = |k: usize| [b.1[k] * t + a.1[k] * u, b.1[k + 1] * t + a.1[k + 1] * u, b.1[k + 2] * t + a.1[k + 2] * u];
    let fused = |k: usize| f64::from(b.1[k]).mul_add(f64::from(t), f64::from(a.1[k] * u)) as f32;
    TimeOfDay { sun: colour(0), sun_factor: fused(3), ambient: colour(4), ambient_factor: fused(7), extra: fused(8) }
}

/// The sun's angle at a clock time (Light_TimeOfDay): a turn over the day from midnight, a
/// quarter by 4:00, three quarters by 20:00 (the hours between taken at even speed: 4:00 to
/// 20:00 over pi, the nights over pi/2 each), with the code's own pi and pi/2.
/// An inside room's ambient factor (the original's 0x8004B300): the lot's day and night values,
/// blended across the hours of 6:00 (night to day) and 18:00 (day to night) by the minute and
/// second over 3,600, day from 7:00 to 17:59 and night otherwise, times the room's window
/// brightness (`window_brightness`). Single precision step for step, the blend fused.
pub fn room_ambient(day: f32, night: f32, hour: i32, minute: i32, second: i32, brightness: f32) -> f32 {
    let into = || ((minute * 60 + second) as f32) / 3600.0;
    let level = match hour {
        6 => {
            let t = into();
            (f64::from(night).mul_add(f64::from(1.0 - t), f64::from(day * t))) as f32
        }
        18 => {
            let t = into();
            (f64::from(day).mul_add(f64::from(1.0 - t), f64::from(night * t))) as f32
        }
        7..=17 => day,
        _ => night,
    };
    level * brightness
}

/// A room's window brightness (the original's 0x80236270): 1 + 0.25 x the share of its tiles
/// with a window (kind 8), the share clamped to [0, 1]; fused as the code has it.
pub fn window_brightness(windows: u32, tiles: i32) -> f32 {
    let share = (windows as f32 / tiles as f32).clamp(0.0, 1.0);
    (f64::from(share).mul_add(f64::from(1.25f32 - 1.0), 1.0)) as f32
}

/// What a tile counts as in a room's window share (`window_brightness`): the original looks the
/// tile up in its grid of one object id per tile (0x802031B0) and counts it when that object is
/// of kind 8, a door or a window. The grid holds the tile's floor object when there is one (a
/// person standing there too, kind 2; not a door or window, not an object on another), so an
/// opening counts only on a tile with nothing standing on its floor. Measured: a Sim in an
/// archway (home's ticks 508-535) takes both its entries. Each item is (door or window,
/// person, on another object).
pub fn tile_counts_as_opening(objects: impl IntoIterator<Item = (bool, bool, bool)>) -> bool {
    let (mut opening, mut floor) = (false, false);
    for (door_or_window, person, contained) in objects {
        opening |= door_or_window;
        floor |= person || (!door_or_window && !contained);
    }
    opening && !floor
}

/// The room walk's half rooms: per split tile, (room a, side a, room b, side b), None free.
type HalfMap = std::collections::BTreeMap<(i32, i32), (Option<usize>, u8, Option<usize>, u8)>;

/// The rooms' tile lists (the original's room walk inside 0x80243C24, before `Room_SetTiles`),
/// in the order the rooms are made, with the engine's room for each list's tiles kept nowhere
/// (the walk's own ids). Tiles are taken in rows (y outer, x inner) over the lot (`size`: 39
/// square on both logged lots, from a virtual call not read yet, not the wall grid's 64). A tile
/// with a half free starts a room, walked depth first: a stack popped from the back.
///
/// A tile's state: not reached, reached (on the stack), one diagonal half reached (`HalfA`:
/// 0x10's −x/−y side or 0x20's +x/−y side; `HalfB` the other), shared (a half kept by an
/// earlier room in `halves`), or done. A pop of a tile not reached (a seed) marks it, pushes it
/// back and appends it. A reached tile is done. A half reached is set in `halves` (the setter
/// 0x80234588): the tile is done when the other half is the same room, shared otherwise. A
/// shared tile whose other half is free is appended and its half taken. Then the four
/// directions −x, +x, −y, +y, as the half's sides allow. An open neighbour (`open`) not done is
/// appended and pushed, again if it is already waiting: hence the repeats. Into a diagonal
/// tile, it goes to the half facing it. That half is skipped if it is taken, and the tile
/// becomes whole when its other half is already reached in this walk.
pub fn room_tile_lists(size: (i32, i32), diagonal: impl Fn(i32, i32) -> u8, open: impl Fn(i32, i32, i32, i32) -> bool) -> Vec<Vec<(i32, i32)>> {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Free,
        Reached,
        HalfA,
        HalfB,
        Shared,
        Done,
    }
    const DIRS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    // a half side's two open directions (by index into DIRS), sides 1 to 4
    const SIDE_OPEN: [[usize; 2]; 4] = [[1, 2], [1, 3], [0, 3], [0, 2]];
    let opposite = |k: u8| [3, 4, 1, 2][(k - 1) as usize];
    let (w, h) = size;
    let at = |x: i32, y: i32| (y * w + x) as usize;
    let mut state = vec![S::Free; (w * h).max(0) as usize];
    // per tile: (room a, side a, room b, side b), None for a free room
    let mut halves: HalfMap = std::collections::BTreeMap::new();
    let mut rooms: Vec<Vec<(i32, i32)>> = Vec::new();
    for y0 in 0..h {
        for x0 in 0..w {
            match state[at(x0, y0)] {
                S::Free | S::HalfA | S::HalfB => {}
                S::Shared if halves.get(&(x0, y0)).is_some_and(|e| e.2.is_none()) => {}
                _ => continue,
            }
            let room = rooms.len();
            let mut list = Vec::new();
            let mut stack = vec![(x0, y0)];
            while let Some((x, y)) = stack.pop() {
                let d = diagonal(x, y);
                let mut allowed = [true; 4];
                let side = |k: u8| -> [bool; 4] {
                    let mut a = [false; 4];
                    for i in SIDE_OPEN[(k - 1) as usize] {
                        a[i] = true;
                    }
                    a
                };
                // the half setter: true when the tile stays shared
                let set_half = |halves: &mut HalfMap, k: u8| -> bool {
                    match halves.get_mut(&(x, y)) {
                        None => {
                            halves.insert((x, y), (Some(room), k, None, opposite(k)));
                            true
                        }
                        Some(e) if e.0 == Some(room) => {
                            halves.remove(&(x, y));
                            false
                        }
                        Some(e) => {
                            e.2 = Some(room);
                            e.3 = k;
                            true
                        }
                    }
                };
                match state[at(x, y)] {
                    S::Done => continue,
                    S::Reached => state[at(x, y)] = S::Done,
                    S::Free => {
                        state[at(x, y)] = match d {
                            0x10 => S::HalfA,
                            0x20 => S::HalfB,
                            _ => S::Reached,
                        };
                        if d == 0x10 {
                            allowed = side(4);
                        } else if d == 0x20 {
                            allowed = side(3);
                        }
                        stack.push((x, y));
                        list.push((x, y));
                    }
                    s @ (S::HalfA | S::HalfB) => {
                        let k = match (d, s) {
                            (0x10, S::HalfA) => 4,
                            (0x10, _) => 2,
                            (_, S::HalfA) => 1,
                            _ => 3,
                        };
                        allowed = side(k);
                        state[at(x, y)] = if set_half(&mut halves, k) { S::Shared } else { S::Done };
                    }
                    S::Shared => {
                        let Some(&(_, _, b, sb)) = halves.get(&(x, y)) else { continue };
                        if b == Some(room) {
                            continue;
                        }
                        if b.is_none() {
                            list.push((x, y));
                        }
                        allowed = side(sb);
                        state[at(x, y)] = if set_half(&mut halves, sb) { S::Shared } else { S::Done };
                    }
                }
                for (i, (dx, dy)) in DIRS.into_iter().enumerate() {
                    let (nx, ny) = (x + dx, y + dy);
                    if !allowed[i] || nx < 0 || ny < 0 || nx >= w || ny >= h || !open(x, y, dx, dy) {
                        continue;
                    }
                    let sn = state[at(nx, ny)];
                    if sn == S::Done {
                        continue;
                    }
                    let dn = diagonal(nx, ny);
                    let new = if dn == 0 {
                        S::Reached
                    } else {
                        // the half facing the step: its side, and its state and the other's
                        let k: u8 = match (dn, i) {
                            (0x10, 0 | 2) => 2,
                            (0x10, _) => 4,
                            (_, 0 | 3) => 1,
                            _ => 3,
                        };
                        let (this, other) = if matches!(k, 4 | 1) { (S::HalfA, S::HalfB) } else { (S::HalfB, S::HalfA) };
                        match sn {
                            S::Free => this,
                            s if s == other => S::Reached,
                            S::Shared => {
                                let e = halves.get(&(nx, ny));
                                if e.is_some_and(|e| (e.1 == k && e.0.is_none()) || (e.3 == k && e.2.is_none())) {
                                    S::Shared
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        }
                    };
                    state[at(nx, ny)] = new;
                    list.push((nx, ny));
                    stack.push((nx, ny));
                }
            }
            rooms.push(list);
        }
    }
    rooms
}

pub fn sun_angle(hour: i32, minute: i32, second: i32) -> f32 {
    let (m, sec) = (minute % 60, second % 60);
    let f = f32::from_bits;
    let at = |h0: i32| (((hour - h0) * 60 + m) * 60 + sec) as f32;
    if (hour - 4) as u16 <= 15 {
        at(4) * f(0x4049_0fd0) / f(0x4761_0000) + f(0x3fc9_0fd0)
    } else if hour <= 3 {
        at(0) * f(0x3fc9_0fd0) / f(0x4661_0000)
    } else {
        at(20) * f(0x3fc9_0fd0) / f(0x4661_0000) + f(0x4096_cbdc)
    }
}

/// The sun's direction at a clock time (Light_TimeOfDay): straight up (0, 0, 1) turned by the
/// sun's angle about the axis (0.5, 0.5, 0.2) normalised (0x801B3024's rotation, the game's own
/// sine and cosine), normalised, pointed down (z made negative) and kept between z -0.8 and
/// -0.2. `None` only if the angle were beyond the game's sine's reach (never, the angle being
/// under 2 pi).
pub fn sun_direction(hour: i32, minute: i32, second: i32) -> Option<[f32; 3]> {
    let a = sun_angle(hour, minute, second);
    let axis = crate::mathf::normalize([0.5, 0.5, f32::from_bits(0x3e4c_cccd)]);
    let (sn, cs) = (crate::mathf::sin(a)?, crate::mathf::cos(a)?);
    let [x, y, z] = axis;
    let t = 1.0 - cs;
    let fma = |a: f32, b: f32, c: f32| f64::from(a).mul_add(f64::from(b), f64::from(c)) as f32;
    // the matrix's third row (what (0, 0, 1) turns to)
    let m20 = (x * z) * t + y * sn;
    let m21 = (y * z) * t - x * sn;
    let m22 = fma(z * z, t, cs);
    // (the original's product with (0, 0, 1) gives the row itself)
    let mut v = crate::mathf::normalize([m20, m21, m22]);
    if v[2] > 0.0 {
        v[2] = -v[2];
    }
    let (hi, lo) = (f32::from_bits(0xbe4c_cccd), f32::from_bits(0xbf4c_cccd));
    if v[2] > hi {
        v[2] = hi;
    }
    if v[2] < lo {
        v[2] = lo;
    }
    Some(v)
}

/// A room record's ambient and sun from its two light objects (the original's Light_RoomSun,
/// 0x801696F0, by their get-lights 0x801631C8 and 0x801636B4): words 0-6 from the ambient (a
/// position and reach of 0, then its colour x its factor), words 8-14 from the sun (its
/// direction, a reach of 0, its colour x its factor). A record without one keeps its words.
pub fn room_sun(record: &mut [f32; 20], ambient: Option<(f32, [f32; 3])>, sun: Option<(f32, [f32; 3], [f32; 3])>) {
    if let Some((k, c)) = ambient {
        record[..4].copy_from_slice(&[0.0; 4]);
        record[4..7].copy_from_slice(&c.map(|v| v * k));
    }
    if let Some((k, c, dir)) = sun {
        record[8..11].copy_from_slice(&dir);
        record[11] = 0.0;
        record[12..15].copy_from_slice(&c.map(|v| v * k));
    }
}

/// A lamp's light from its light node (an object's animation node of kind 0: words 11-13 its
/// colour, 14 a factor, 15 its radius): the colour is the node's times the factor, and the reach
/// twice the radius (the light object's +208) times the get-light's quarter (0x80165A90).
pub fn lamp_light(colour: [f32; 3], factor: f32, radius: f32) -> ([f32; 3], f32) {
    (colour.map(|c| factor * c), radius * 2.0 * 0.25)
}

/// The filler's squared length (the original's paired square magnitude, 0x80122228: x^2 and
/// y^2 rounded, then z^2 fused onto x^2, then y^2 added).
pub fn square_length(d: [f32; 3]) -> f32 {
    let xx = (f64::from(d[0]) * f64::from(d[0])) as f32;
    let yy = (f64::from(d[1]) * f64::from(d[1])) as f32;
    (f64::from(d[2]).mul_add(f64::from(d[2]), f64::from(xx)) as f32) + yy
}

/// A window light's mix of light (the original's Light_FillWindow, 0x801697F0): the sun when its
/// room is inside (not 0: the filler's own test is the light's vtable +48 room being 0, which
/// the log shows for every inside light), then up to 4 of its nearest lights (position, reach, colour; the list up to its first gap), each
/// colour times min(reach^2 / its squared distance, 1) (a zero distance taken as 0.0001), then
/// scaled down so its largest part is at most 1. The light's colour is the mix times its base.
pub fn window_mix(room: i32, sun: [f32; 3], at: [f32; 3], near: &[([f32; 3], f32, [f32; 3])]) -> [f32; 3] {
    let mut mix = [0.0f32; 3];
    if room != 0 {
        mix = [mix[0] + sun[0], mix[1] + sun[1], mix[2] + sun[2]];
    }
    for (p, reach, colour) in near.iter().take(4) {
        let mut d2 = square_length([p[0] - at[0], p[1] - at[1], p[2] - at[2]]);
        if d2 == 0.0 {
            d2 = RANK_INSTEAD;
        }
        let f = (reach * reach / d2).min(1.0);
        for k in 0..3 {
            mix[k] += f * colour[k];
        }
    }
    let most = mix.iter().fold(1.0f32, |m, &v| if v > m { v } else { m });
    let s = 1.0 / most;
    mix.map(|v| v * s)
}

/// A window's kind by its resources' key (`quickdata::ObjResources::key`; the original's table
/// in 0x801679CC, called by 0x80078A68 on the window object's +0x138; 0 for a key not listed). Home's storm windows are 0xD02B5C12
/// (1) and its small rectangle ones 0x92751A1A (2).
pub fn window_kind(key: u32) -> u32 {
    match key {
        0x08FD_5188 | 0x27B3_7C8F | 0x437C_7240 | 0xCDDD_1EDE | 0xD02B_5C12 | 0xD491_D417 => 1,
        0x9275_1A1A => 2,
        0x61B5_D7D6 | 0x040E_93A9 | 0x63BE_954C => 3,
        _ => 0,
    }
}

/// A window light's reach, base colour and height by the window's kind (1 to 3) and whether
/// the light's room is inside (the original's window light update: reach tables at r13 less
/// 29644, 29636 and 29652, colours at r13 less 29620, 29612 and 29628, the heights at
/// 0x802B9F50 and 0x802B9F58, a quarter of the table's reach given out). Kind 3's height is
/// not read yet.
pub fn window_light_params(kind: u32, inside: bool) -> Option<(f32, f32, Option<f32>)> {
    let i = usize::from(inside);
    let (reach, colour, height) = match kind {
        1 => ([7.0f32, 14.0], [0.24f32, 0.6], Some(2.0f32)),
        2 => ([6.0, 8.0], [0.22, 0.4], Some(2.5)),
        3 => ([12.0, 24.0], [0.28, 0.7], None),
        _ => return None,
    };
    Some((reach[i] * 0.25, colour[i], height))
}

/// A light object's kind (the original's light classes, by what they give a room).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    /// A room's ambient (its record's +0x1C).
    Ambient,
    /// The sun (a record's +0x3C).
    Sun,
    /// A lamp: a light of its own room's (the own range, from 256).
    Lamp,
    /// A window's light, shining into the room it is in from another (the other range, from 0).
    Window,
}

/// The rooms' light ranges as the original's Light_Rebuild sorts the light objects: room by
/// room, the list walked in order, each lamp in the room taking the next own light (from 256,
/// at most 511) and each window in it whose other side is another room the next other light
/// (at most 255). `objects` gives each one's kind, room and (a window's) other room. Returns
/// each room's (other first, other end, own first, own end) and the lights' objects in order:
/// the other lights', then the own.
pub fn sort_lights(rooms: usize, objects: &[(LightKind, i32, i32)]) -> (Vec<[u32; 4]>, Vec<usize>, Vec<usize>) {
    let (mut other, mut own) = (Vec::new(), Vec::new());
    let mut ranges = Vec::new();
    for r in 0..rooms as i32 {
        let (o0, w0) = (other.len() as u32, 256 + own.len() as u32);
        for (k, &(kind, room, to)) in objects.iter().enumerate() {
            if room != r {
                continue;
            }
            match kind {
                LightKind::Lamp if own.len() < 256 => own.push(k),
                LightKind::Window if to != r && to >= 0 && to < rooms as i32 && other.len() < 256 => other.push(k),
                _ => {}
            }
        }
        ranges.push([o0, other.len() as u32, w0, 256 + own.len() as u32]);
    }
    (ranges, other, own)
}

/// The rooms added to the light grid at a lot's load, in the original's order: tiles x, y from 1
/// to the size less 1 (x inner), each adding its room to its corners (x, y), (x, y + 1),
/// (x + 1, y + 1), (x + 1, y). A diagonal's tile (`diagonal` its bits) splits them: its west
/// half (the room across its x - 1 edge) takes (x, y), (x, y + 1) and the far corner of its side
/// (0x10: (x + 1, y); 0x20: (x + 1, y + 1)), its east half (across x + 1) the near corner of the
/// other line end ((x, y + 1) for 0x10, (x, y) for 0x20), then (x + 1, y + 1) and (x + 1, y).
/// `room` gives a whole tile's room id (the original's numbering: the engine's rooms less one).
/// `size` is the lot's, as the original takes it from the lot (a virtual call not yet read): 39
/// on both logged lots (the wall arrays are 64 square), the light grid's points.
pub fn cell_room_adds(size: (u32, u32), room: impl Fn(i32, i32) -> u8, diagonal: impl Fn(i32, i32) -> u8) -> Vec<(i32, i32, u8)> {
    let mut adds = Vec::new();
    for y in 1..size.1 as i32 - 1 {
        for x in 1..size.0 as i32 - 1 {
            let bits = diagonal(x, y);
            if bits == 0 {
                let r = room(x, y);
                adds.extend([(x, y, r), (x, y + 1, r), (x + 1, y + 1, r), (x + 1, y, r)]);
                continue;
            }
            let (west, east) = (room(x - 1, y), room(x + 1, y));
            let (far, near) = if bits & 0x10 != 0 { ((x + 1, y), (x, y + 1)) } else { ((x + 1, y + 1), (x, y)) };
            adds.extend([(x, y, west), (x, y + 1, west), (far.0, far.1, west), (near.0, near.1, east), (x + 1, y + 1, east), (x + 1, y, east)]);
        }
    }
    adds
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
    fn the_sun_turns_over_the_day_and_points_down() {
        assert_eq!(sun_angle(0, 0, 0), 0.0);
        assert_eq!(sun_angle(4, 0, 0), f32::from_bits(0x3fc9_0fd0));
        assert!((sun_angle(12, 0, 0) - std::f32::consts::PI).abs() < 1e-5);
        for h in 0..24 {
            let d = sun_direction(h, 30, 0).unwrap();
            assert!((-0.8..=-0.2).contains(&d[2]), "{h}: {d:?}");
        }
    }

    #[test]
    fn the_time_of_day_runs_between_its_keys() {
        assert_eq!(time_key(0), 0);
        assert_eq!(time_key(3), 0);
        assert_eq!(time_key(13), 5);
        assert_eq!(time_key(23), 9);
        // at a key, its own light
        let d = time_of_day(7, 0, 0);
        assert_eq!((d.sun, d.sun_factor, d.extra), ([1.0, 1.0, 0.7], 1.3, 0.5));
        // 20:00 to 0:00 wraps over midnight: halfway at 22:00
        let d = time_of_day(22, 0, 0);
        assert!((d.extra - 0.275).abs() < 1e-6);
    }

    #[test]
    fn a_rooms_sun_and_ambient_come_from_their_objects() {
        let mut r = [9.0f32; 20];
        room_sun(&mut r, Some((0.4, [0.8, 0.8, 1.0])), None);
        assert_eq!(r[..7], [0.0, 0.0, 0.0, 0.0, 0.32000002, 0.32000002, 0.4]);
        assert_eq!(r[7..], [9.0; 13]);
        room_sun(&mut r, None, Some((1.3, [1.0, 1.0, 0.7], [0.8, -0.2, -0.5])));
        assert_eq!(r[8..15], [0.8, -0.2, -0.5, 0.0, 1.3, 1.3, 0.90999997]);
    }

    #[test]
    fn a_lamps_light_is_its_nodes_colour_by_its_factor() {
        assert_eq!(lamp_light([0.5019608, 0.5019608, 1.0], 0.5, 4.0), ([0.2509804, 0.2509804, 0.5], 2.0));
        assert_eq!(lamp_light([0.9, 0.9, 0.9], 0.0, 1.0), ([0.0; 3], 0.5));
    }

    #[test]
    fn a_window_mix_takes_the_sun_inside_and_scales_down() {
        let lamp = ([0.0, 0.0, 1.0], 2.0, [0.25, 0.25, 0.5]);
        // outside: the lamp in full (reach^2 / distance^2 over 1), no sun
        assert_eq!(window_mix(0, [1.3, 1.3, 0.91], [0.0; 3], &[lamp]), [0.25, 0.25, 0.5]);
        // inside: the sun added, then scaled down to a largest part of 1
        let m = window_mix(1, [1.3, 1.3, 0.91], [0.0; 3], &[lamp]);
        assert_eq!(m[0], 1.0);
        assert!(m[2] < 1.0);
        // a fifth listed light is not read
        assert_eq!(window_mix(0, [0.0; 3], [0.0; 3], &[lamp; 5]), window_mix(0, [0.0; 3], [0.0; 3], &[lamp; 4]));
    }

    #[test]
    fn a_windows_kind_follows_its_key() {
        assert_eq!(window_kind(0xD02B_5C12), 1);
        assert_eq!(window_kind(0x9275_1A1A), 2);
        assert_eq!(window_kind(0x61B5_D7D6), 3);
        assert_eq!(window_kind(0), 0);
    }

    #[test]
    fn a_window_lights_reach_is_a_quarter_of_its_kinds() {
        assert_eq!(window_light_params(1, false), Some((1.75, 0.24, Some(2.0))));
        assert_eq!(window_light_params(1, true), Some((3.5, 0.6, Some(2.0))));
        assert_eq!(window_light_params(2, true), Some((2.0, 0.4, Some(2.5))));
        assert_eq!(window_light_params(0, true), None);
    }

    #[test]
    fn a_windows_lights_sit_either_side_of_its_wall() {
        // facing 4 (-x) from tile (26, 16): the wall at x = 26
        assert_eq!(window_light_places((26, 16), 4), Some([(26.0 - 0.8, 16.5), (26.0 + 0.8, 16.5)]));
        // facing 6 (+y) from tile (23, 23): the wall at y = 24
        assert_eq!(window_light_places((23, 23), 6), Some([(23.5, 24.0 + 0.8), (23.5, 24.0 - 0.8)]));
        assert_eq!(window_light_places((1, 1), 1), None);
    }

    #[test]
    fn lights_are_sorted_room_by_room_in_list_order() {
        use LightKind::*;
        let objects = [(Lamp, 1, 0), (Window, 0, 1), (Ambient, 0, 0), (Lamp, 0, 0), (Window, 1, 1), (Window, 1, 0), (Lamp, 1, 0)];
        let (ranges, other, own) = sort_lights(2, &objects);
        // room 0: the window (1) and the lamp (3); room 1: the window to room 0 (5; 4 looks into
        // its own room) and the lamps (0, 6)
        assert_eq!(ranges, vec![[0, 1, 256, 257], [1, 2, 257, 259]]);
        assert_eq!(other, vec![1, 5]);
        assert_eq!(own, vec![3, 0, 6]);
    }

    #[test]
    fn a_diagonal_tile_splits_its_corners_between_its_halves() {
        // a 4 x 4 lot: tiles 1..=2 each way; (2, 1) a "/" diagonal between room 5 (west) and 7
        let room = |x: i32, _: i32| if x < 2 { 5 } else if x > 2 { 7 } else { 9 };
        let diag = |x: i32, y: i32| if (x, y) == (2, 1) { 0x10 } else { 0 };
        let adds = cell_room_adds((4, 4), room, diag);
        assert_eq!(&adds[..4], &[(1, 1, 5), (1, 2, 5), (2, 2, 5), (2, 1, 5)]);
        assert_eq!(&adds[4..10], &[(2, 1, 5), (2, 2, 5), (3, 1, 5), (2, 2, 7), (3, 2, 7), (3, 1, 7)]);
        assert_eq!(adds.len(), 4 * 3 + 6);
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

    #[test]
    fn an_opening_counts_unless_something_stands_on_its_floor() {
        assert!(tile_counts_as_opening([(true, false, false)]));
        assert!(tile_counts_as_opening([(true, false, false), (false, false, true)]));
        assert!(!tile_counts_as_opening([(true, false, false), (false, true, false)]));
        assert!(!tile_counts_as_opening([(true, false, false), (false, false, false)]));
        assert!(!tile_counts_as_opening([(false, false, false)]));
    }


    #[test]
    fn the_room_walk_repeats_tiles_reached_again() {
        let lists = room_tile_lists((2, 2), |_, _| 0, |_, _, _, _| true);
        assert_eq!(lists, vec![vec![(0, 0), (1, 0), (0, 1), (1, 1), (0, 0), (1, 0), (1, 1)]]);
    }


    #[test]
    fn a_diagonal_tile_is_walked_half_by_half() {
        // a 2 x 1 lot whose west tile is split (0x10): the seed's half opens −x and −y only, so
        // the east tile starts its own room, entering the split tile's other half (appended on
        // the step and again on its pop, its half still free then; that half opens +x, back to
        // the east tile still waiting)
        let lists = room_tile_lists((2, 1), |x, _| if x == 0 { 0x10 } else { 0 }, |_, _, _, _| true);
        assert_eq!(lists, vec![vec![(0, 0)], vec![(1, 0), (0, 0), (0, 0), (1, 0)]]);
    }

}
