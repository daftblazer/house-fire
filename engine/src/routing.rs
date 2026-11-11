//! Routing: where a Sim can walk (`docs/simulation.md`, "Routing"). Measured on the recompiled
//! Bustin' Out's home log (the Sims' places, 'P').

use std::collections::VecDeque;

use crate::save::LotArray;
use crate::world::World;

/// The edge's bit on the tile it leaves, and on the tile it enters.
fn leaving(dx: i32, dy: i32) -> u8 {
    match (dx, dy) {
        (1, _) => 8,
        (-1, _) => 2,
        (_, 1) => 4,
        (_, -1) => 1,
        _ => 0,
    }
}
fn entering(dx: i32, dy: i32) -> u8 {
    leaving(-dx, -dy)
}
fn key(x: i32, y: i32, bit: u8) -> u32 {
    (x as u32) << 16 | (y as u32) << 8 | bit as u32
}

/// The tile in front of an object of this facing (`Object::facing`): 0 towards -x, 2 towards
/// +y, 4 towards +x, 6 towards -y (an odd facing, between two: `None`). Measured on the home
/// log's walks: a Sim stands at the -x side of the fridge, the counters and the stove (0), at the
/// -y side of the easel (6); 2 and 4 by the turn between them.
pub fn front(facing: i16) -> Option<(i32, i32)> {
    match facing & 7 {
        0 => Some((-1, 0)),
        2 => Some((0, 1)),
        4 => Some((1, 0)),
        6 => Some((0, -1)),
        _ => None,
    }
}

/// The places a routing slot offers around its object: for each direction its flags name (bit
/// k: k eighths of a turn from the object's front, by its facing), the place that far away at
/// the slot's optimal proximity, with k. A diagonal's two parts are each the proximity times
/// the square root of a half, cut to a whole sixteenth (16 gives 11: the home log's goals round
/// the paper girl). Places in sixteenths, as the objects', in the order of the bits.
pub fn slot_places(slot: &crate::library::Slot, x16: i32, y16: i32, facing: i16) -> Vec<(u8, (i32, i32))> {
    let p = slot.optimal_proximity;
    let diagonal = (p as f32 * std::f32::consts::FRAC_1_SQRT_2) as i32;
    let mut out = Vec::new();
    for k in 0..8u8 {
        if slot.flags & (1 << k) == 0 {
            continue;
        }
        let f = facing + k as i16;
        let d = match front(f) {
            Some(d) => (d.0 * p, d.1 * p),
            None => {
                let (a, b) = (front(f - 1).unwrap(), front(f + 1).unwrap());
                ((a.0 + b.0) * diagonal, (a.1 + b.1) * diagonal)
            }
        };
        out.push((k, (x16 + d.0, y16 + d.1)));
    }
    out
}

/// A slot's goal's score (the goal's third word), as fn_8024E480 gives it: 135 for the routing
/// slots of the home log (their gradient 0, so no fall with distance), and with the slot's flag
/// 0x2000 a random part, `random % 20 - 10` (Sim_Random, from 8024F030); 90 more when the request
/// names the object its goals are for (the goals' last word's high half: its id; measured, tick
/// 1790's second request on the table, not yet read in the builder). Returns its range.
pub fn slot_goal_score(slot: &crate::library::Slot, names_object: bool) -> std::ops::RangeInclusive<u32> {
    let base = if names_object { 225 } else { 135 };
    if slot.flags & 0x2000 != 0 { base - 10..=base + 9 } else { base..=base }
}

/// The builder's pick of a request's goals from its open places (fn_8024E480 at 8024F504): up
/// to `cap` times (the request's +164, 48 for the home log's), a draw `random % n` into the open
/// places (n of them, in the builder's order); a place already taken passes to the next one not
/// taken, none past the last (that draw then takes nothing). The goals are the places so taken, in
/// the order taken. Returns their indices.
pub fn slot_pick(n: usize, draws: &[u32], cap: usize) -> Vec<usize> {
    let mut taken = vec![false; n];
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    for &r in draws.iter().take(cap) {
        let mut i = r as usize % n;
        while i < n && taken[i] {
            i += 1;
        }
        if i < n {
            taken[i] = true;
            out.push(i);
        }
    }
    out
}

/// The places a routing slot of a range of proximities offers (an area): for each direction its
/// flags name (bit k, `k` eighths of a turn from the object's front), the points of that
/// direction's own grid (forward and sideways steps of the slot's resolution) within its eighth of
/// the turn, the sideways step from minus the forward one up to but not including it (so the
/// eight wedges tile the area), between the least and greatest proximity; a diagonal's grid
/// turned with it, its places rounded to the sixteenth. In the builder's order (fn_8024E480): by
/// direction, then forward step, then sideways step, each place scored with its own random draw
/// when the slot is flagged (`slot_goal_score`), before any is taken out as closed. Measured on
/// the home log: 47 places a direction, all 48 goals of the paper girl's slot 8 (16 to 64 at 8)
/// at tick 931 with their scores from the draws in this order.
pub fn slot_area(slot: &crate::library::Slot, x16: i32, y16: i32, facing: i16) -> Vec<(u8, (i32, i32))> {
    let (lo, hi, res) = (slot.min_proximity, slot.max_proximity, slot.resolution.max(1));
    let mut out = Vec::new();
    for k in 0..8u8 {
        if slot.flags & (1 << k) == 0 {
            continue;
        }
        let f = facing + k as i16;
        // the direction's forward and sideways units, and their scale (a diagonal's sum of two)
        let (u, w, scale) = match (front(f), front(f + 2)) {
            (Some(u), Some(w)) => (u, w, 1.0f32),
            _ => {
                let add = |a: (i32, i32), b: (i32, i32)| (a.0 + b.0, a.1 + b.1);
                (add(front(f - 1).unwrap(), front(f + 1).unwrap()), add(front(f + 1).unwrap(), front(f + 3).unwrap()), std::f32::consts::FRAC_1_SQRT_2)
            }
        };
        for ahead in (0..=hi).step_by(res as usize) {
            for side in (-ahead..ahead).filter(|s| s % res == 0) {
                let d2 = ahead * ahead + side * side;
                if d2 < lo * lo || d2 > hi * hi {
                    continue;
                }
                let at = |a: i32, b: i32| ((ahead * a + side * b) as f32 * scale).round() as i32;
                out.push((k, (x16 + at(u.0, w.0), y16 + at(u.1, w.1))));
            }
        }
    }
    out
}

/// An object's footprint on its tile, {x0, y0, x1, y1} in sixteenths: the tile less its
/// definition's footprint mask (OBJD field 75), whose nibble k (the low first) is the inset of
/// the side k quarter turns from the object's front (`front(facing + 2k)`). Measured on the home
/// log's obstacles (`docs/simulation.md`, "The footprints"): the trees' quarters turning with
/// their facing, the armchair's 1 at each side, the jukebox's 6 at its back. A definition's width
/// (OBJD field 65; 0 for most) narrows it by (16 - width) / 2 more on every side: the outdoor
/// chair's 14 (8 at +4 with its mask of 3), the tron's and the trunk's 8.
pub fn footprint(mask: u16, width: u16, x16: i32, y16: i32, facing: i16) -> [i32; 4] {
    let (x, y) = (x16 & !15, y16 & !15);
    let narrow = if width == 0 || width >= 16 { 0 } else { (16 - width as i32) / 2 };
    let mut r = [x + narrow, y + narrow, x + 16 - narrow, y + 16 - narrow];
    for k in 0..4 {
        let inset = ((mask >> (4 * k)) & 15) as i32;
        match front((facing & 6) + 2 * k as i16) {
            Some((-1, 0)) => r[0] += inset,
            Some((1, 0)) => r[2] -= inset,
            Some((0, -1)) => r[1] += inset,
            Some((0, 1)) => r[3] -= inset,
            _ => {}
        }
    }
    r
}

/// The cost the planner gives an obstacle that blocks.
pub const BLOCKS: i32 = i32::MAX;
/// The cost of an obstacle that may be walked through (a door's tile, a tree's quarters).
pub const WALK_THROUGH: i32 = 10;

/// An object's place among the planner's obstacles ({x0, y0, x1, y1, cost}, sixteenths), or none:
/// one with no extent (flags 0x4) or in another is left out; one that may be walked through
/// (flags 0x10) is its footprint at a small cost; any other blocks, its footprint grown on each
/// side by half the width (`half`: 3 in Bustin' Out). Not for persons (another rule).
pub fn obstacle(o: &crate::world::Object, mask: u16, width: u16, half: i32) -> Option<[i32; 5]> {
    use crate::world::{DATA_CONTAINER, DATA_FLAGS};
    let flags = o.data[DATA_FLAGS];
    if flags & 0x4 != 0 || o.data[DATA_CONTAINER] != 0 {
        return None;
    }
    let f = footprint(mask, width, o.x16, o.y16, o.facing());
    Some(if flags & 0x10 != 0 { [f[0], f[1], f[2], f[3], WALK_THROUGH] } else { [f[0] - half, f[1] - half, f[2] + half, f[3] + half, BLOCKS] })
}

/// An obstacle's footprint before it was grown: a blocking one less `half` on each side.
pub fn ungrown(r: &[i32; 5], half: i32) -> [i32; 4] {
    if r[4] == BLOCKS { [r[0] + half, r[1] + half, r[2] - half, r[3] - half] } else { [r[0], r[1], r[2], r[3]] }
}

/// Whether two rectangles {x0, y0, x1, y1} share some area (touching edges do not).
pub fn overlaps(a: &[i32; 4], b: &[i32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Whether the segment from a to b passes through the inside of rectangle r ({x0, y0, x1,
/// y1}; its edges and corners do not count): clipped to the open interior (Liang-Barsky).
pub fn segment_enters(a: (i32, i32), b: (i32, i32), r: &[i32; 4]) -> bool {
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, (a.0 - r[0]) as f64), (dx, (r[2] - a.0) as f64), (-dy, (a.1 - r[1]) as f64), (dy, (r[3] - a.1) as f64)] {
        if p == 0.0 {
            if q <= 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t)
            } else {
                t1 = t1.min(t)
            }
        }
    }
    t1 - t0 > 1e-9
}

/// The shortest line from `start` to `goal` (sixteenths) that keeps out of the blocking
/// obstacles' insides and crosses no wall (with `half` > 0, keeping that far from the walls too), going from corner to corner of the obstacles (a
/// visibility graph, searched by Dijkstra): its points, the ends included; none if there is no
/// such line. Obstacles that may be walked through are not in its way.
pub fn corner_path(start: (i32, i32), goal: (i32, i32), obstacles: &[[i32; 5]], walls: &Walls, half: i32) -> Option<Vec<(i32, i32)>> {
    weighed_path(start, goal, obstacles, walls, half, &[], 1.0)
}

/// How much of the segment from a to b lies inside rectangle r ({x0, y0, x1, y1}).
pub fn inside_length(a: (i32, i32), b: (i32, i32), r: &[i32; 4]) -> f64 {
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, (a.0 - r[0]) as f64), (dx, (r[2] - a.0) as f64), (-dy, (a.1 - r[1]) as f64), (dy, (r[3] - a.1) as f64)] {
        if p == 0.0 {
            if q < 0.0 {
                return 0.0;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t)
            } else {
                t1 = t1.min(t)
            }
        }
    }
    (t1 - t0).max(0.0) * (dx * dx + dy * dy).sqrt()
}

/// How many times a length off the paved regions counts against one on them. Fitted, not the
/// planner's own figure (not yet found in its code): any from 2.5 to 3.5 gives 15 of the home
/// log's 16 checked plans by the planner's corners (`docs/simulation.md`).
pub const OFF_PAVED_WEIGHT: f64 = 3.0;

/// `corner_path` weighing the line by the floor: a length outside the `paved` regions counts
/// `weight` times one inside them (`OFF_PAVED_WEIGHT`, fitted).
pub fn weighed_path(start: (i32, i32), goal: (i32, i32), obstacles: &[[i32; 5]], walls: &Walls, half: i32, paved: &[[i32; 4]], weight: f64) -> Option<Vec<(i32, i32)>> {
    let mut blocking: Vec<[i32; 4]> = obstacles.iter().filter(|r| r[4] == BLOCKS).map(|r| [r[0], r[1], r[2], r[3]]).collect();
    if half > 0 {
        blocking.extend(walls.wall_rects(half));
    }
    let inside = |p: (i32, i32)| blocking.iter().any(|r| p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3]);
    let mut nodes = vec![start, goal];
    for r in &blocking {
        for c in [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])] {
            if !inside(c) && !nodes.contains(&c) {
                nodes.push(c);
            }
        }
    }
    for r in paved {
        for c in [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3]), (r[0] + half, r[1] + half), (r[2] - half, r[1] + half), (r[0] + half, r[3] - half), (r[2] - half, r[3] - half)] {
            if !inside(c) && !nodes.contains(&c) {
                nodes.push(c);
            }
        }
    }
    let clear = |a: (i32, i32), b: (i32, i32)| !blocking.iter().any(|r| segment_enters(a, b, r)) && walls.line_open(a, b);
    let cost = |a: (i32, i32), b: (i32, i32)| {
        let len = (((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2)) as f64).sqrt();
        if paved.is_empty() {
            return len;
        }
        let on = paved.iter().map(|r| inside_length(a, b, r)).sum::<f64>().min(len);
        on + (len - on) * weight
    };
    let n = nodes.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![usize::MAX; n];
    let mut done = vec![false; n];
    dist[0] = 0.0;
    while let Some(u) = (0..n).filter(|&i| !done[i] && dist[i].is_finite()).min_by(|&i, &j| dist[i].total_cmp(&dist[j])) {
        if u == 1 {
            break;
        }
        done[u] = true;
        for v in 0..n {
            if done[v] || v == u {
                continue;
            }
            let d = dist[u] + cost(nodes[u], nodes[v]);
            if d < dist[v] && clear(nodes[u], nodes[v]) {
                dist[v] = d;
                prev[v] = u;
            }
        }
    }
    if !dist[1].is_finite() {
        return None;
    }
    let mut path = vec![goal];
    let mut at = 1;
    while at != 0 {
        at = prev[at];
        path.push(nodes[at]);
    }
    path.reverse();
    Some(path)
}

/// A walker's top speeds (sixteenths a second, constants of Bustin' Out's DOL at 802A8DB8, 802A8DC0,
/// 802A8DBC): an adult's, a child's, and the one its follower's +132 at 1 gives.
pub const WALK_TOP_ADULT: f32 = 19.840_021;
pub const WALK_TOP_CHILD: f32 = 17.280_02;
pub const WALK_TOP_FAST: f32 = 64.960_07;
/// A walker's top a second by its body's age: the first of its file's body strings (STR# 200,
/// "adult" or "child"). The original asks the person's body (fn_80059194, through the body's
/// skeleton entry); a child walks slower.
pub fn walk_top(body_age: Option<&str>) -> f32 {
    if body_age == Some("child") { WALK_TOP_CHILD } else { WALK_TOP_ADULT }
}
/// The walker's frame: the global its step reads (r13 - 29720), 0.033 s.
pub const WALK_FRAME: f32 = 0.033;
/// The time a walker takes to reach its top from a stand (its speed grows by the top over it).
pub const WALK_RAMP: f32 = 0.5;

/// A walker's top and growth a frame, in sixteenths a frame, from its top a second.
pub fn walk_rates(top_per_second: f32) -> (f32, f32) {
    (top_per_second * WALK_FRAME, top_per_second / WALK_RAMP * WALK_FRAME * WALK_FRAME)
}

/// A walker's step a frame (`Route_Follow`, 800AE0A4; `docs/simulation.md`, "The walker's
/// step"): from its speed the frame before (`speed`, 0 at its start) to this frame's, and the
/// distance it moves along its line. Walking (factor 1) the speed grows by `growth` a frame to
/// `top`, and near the end is no more than will let it stop there (sqrt(2 growth left));
/// the fast mode (factor 5) goes at its top times the factor from the first frame to the
/// last. Never past the end (`left`, sixteenths along the line).
pub fn walk_step(speed: f32, top: f32, growth: f32, factor: f32, left: f32) -> (f32, f32) {
    let v = if factor != 1.0 { top } else { top.min(speed + growth).min((2.0 * growth * left).sqrt()) };
    (v, (v * factor).min(left))
}

/// The simulation's tick against the walker's frame (`WALK_FRAME`): 0.042 s, measured (the home
/// log's ticks hold 1 or 2 frames, 14 in 11 ticks; `docs/simulation.md`, "Frames and ticks").
pub const TICK_TIME: f32 = 0.042;

/// The frames between ticks: the frame clock adds a frame's time each frame and a tick runs each
/// time it has gone a tick's further. `frames_to_tick` gives the frames before the next tick.
#[derive(Clone, Copy, Debug)]
pub struct FrameClock {
    pub time: f32,
}

impl FrameClock {
    pub fn frames_to_tick(&mut self) -> u32 {
        let mut frames = 0;
        loop {
            if self.time >= TICK_TIME {
                self.time -= TICK_TIME;
                return frames;
            }
            self.time += WALK_FRAME;
            frames += 1;
        }
    }
}

/// A walker's turn a frame once it has arrived, to face what it walks to (the follower's state 3):
/// 3 degrees (measured on the home log's walks: 90 degrees in 31 frames, 180 in 61).
pub const TURN_PER_FRAME: f32 = std::f32::consts::PI / 60.0;

/// The frames a walker stays arrived (state 3) turning through `angle` radians before it is done
/// (state 4): a frame for each whole 3 degrees, and one.
pub fn turn_frames(angle: f32) -> u32 {
    // (a hundredth of a step's slack: the angle here from the last step's way, the original's
    // from its heading, which a right angle gives whole)
    (angle / TURN_PER_FRAME + 0.01) as u32 + 1
}

/// A point `s` along a line (its points, sixteenths) from leg `i` at fraction `t` of it, never
/// past its end.
pub fn along_line(points: &[(f32, f32)], mut i: usize, mut t: f32, mut s: f32) -> (f32, f32) {
    while i + 1 < points.len() {
        let (a, b) = (points[i], points[i + 1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let left = (1.0 - t) * len;
        if s <= left || i + 2 == points.len() {
            let u = if len > 0.0 { t + s.min(left) / len } else { t };
            return (a.0 + u * (b.0 - a.0), a.1 + u * (b.1 - a.1));
        }
        s -= left;
        i += 1;
        t = 0.0;
    }
    points.last().copied().unwrap_or_default()
}

/// A tile, (x, y).
pub type Tile = (i32, i32);

/// A door: an opening, the tile it is left from (x + dx, y + dy beyond) and the two rooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Door {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
    pub from: u16,
    pub to: u16,
}

/// A lot's walls: the second array's first byte a tile (`docs/lots.md`, "The arrays"), a bit an
/// edge of the tile: 1 towards y - 1, 2 towards x - 1, 4 towards y + 1, 8 towards x + 1 (each
/// wall seen from both its tiles); 0x10 and 0x20 the diagonal walls. A door opens the wall
/// between its two parts' tiles. The ground floor's (level 1) only, as the lot has one array.
#[derive(Clone, Debug, Default)]
pub struct Walls {
    width: u32,
    height: u32,
    edges: Vec<u8>,
    /// (x, y, the edge's bit) of each door's opening.
    open: Vec<u32>,
    wall_edges: usize,
    room: Vec<u16>,
    rooms: usize,
    doors: Vec<Door>,
    /// Each tile's floor (the first array's byte: 0x00 grass, 0x0a the sidewalk, 0x1d a path, ...)
    /// and whether it is inside (the third array's 1).
    floors: Vec<u8>,
    inside: Vec<bool>,
}

impl Walls {
    pub fn new(arrays: &[LotArray], world: &World) -> Walls {
        let mut w = Walls::default();
        let Some(a) = arrays.get(1).filter(|a| a.element_size >= 1) else { return w };
        w.width = a.width;
        w.height = a.height;
        let es = a.element_size as usize;
        // The array is row after row of y, x across a row.
        w.edges = (0..(a.width * a.height) as usize).map(|i| a.bytes.get(i * es).copied().unwrap_or(0)).collect();
        let n = (a.width * a.height) as usize;
        w.floors = arrays.first().map_or(vec![0; n], |f| (0..n).map(|i| f.bytes.get(i * f.element_size as usize).copied().unwrap_or(0)).collect());
        w.inside = arrays.get(2).map_or(vec![false; n], |f| (0..n).map(|i| f.bytes.get(i * f.element_size as usize) == Some(&1)).collect());
        w.wall_edges = w.edges.iter().map(|e| (e & 0x0F).count_ones() as usize).sum();
        // The doors: each pair of a door's parts on two neighbouring tiles opens the edge between.
        let doors: Vec<_> = world.objects().filter(|o| o.door && o.on_lot()).collect();
        for p in &doors {
            for q in &doors {
                if std::ptr::eq(*p, *q) || q.leader != p.leader {
                    continue;
                }
                let (x, y) = (p.x16 >> 4, p.y16 >> 4);
                let (dx, dy) = ((q.x16 >> 4) - x, (q.y16 >> 4) - y);
                if dx.abs() + dy.abs() != 1 || dx < 0 || dy < 0 {
                    continue; // each opening once, from its lower tile
                }
                let k = key(x, y, leaving(dx, dy));
                if !w.open.contains(&k) {
                    w.open.push(k);
                    w.doors.push(Door { x, y, dx, dy, from: 0, to: 0 });
                }
            }
        }
        w.find_rooms();
        w
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        if self.inside(x, y) { self.edges[y as usize * self.width as usize + x as usize] } else { 0 }
    }

    /// Whether there is a wall between tile (x, y) and its neighbour (x + dx, y + dy), the
    /// doors' openings left out.
    pub fn wall_between(&self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        self.at(x, y) & leaving(dx, dy) != 0 || self.at(x + dx, y + dy) & entering(dx, dy) != 0
    }

    /// Whether a Sim on tile (x, y) can step to its neighbour (x + dx, y + dy), one of dx and dy
    /// 0 and the other ±1. Off the grid is open (the lot's edge is not a wall here).
    /// Whether a straight line between two places (sixteenths) crosses no wall: each tile it
    /// steps into from the last (stepped along it a sixteenth at a time) open from it, a diagonal
    /// step by either way round.
    pub fn line_open(&self, a: (i32, i32), b: (i32, i32)) -> bool {
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        let mut t = (a.0 >> 4, a.1 >> 4);
        for i in 1..=steps {
            let p = (a.0 + (b.0 - a.0) * i / steps, a.1 + (b.1 - a.1) * i / steps);
            let u = (p.0 >> 4, p.1 >> 4);
            if u == t {
                continue;
            }
            let (dx, dy) = (u.0 - t.0, u.1 - t.1);
            let ok = if dx != 0 && dy != 0 {
                (self.can_step(t.0, t.1, dx, 0) && self.can_step(t.0 + dx, t.1, 0, dy)) || (self.can_step(t.0, t.1, 0, dy) && self.can_step(t.0, t.1 + dy, dx, 0))
            } else {
                self.can_step(t.0, t.1, dx, dy)
            };
            if !ok {
                return false;
            }
            t = u;
        }
        true
    }

    pub fn can_step(&self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        !self.wall_between(x, y, dx, dy)
            || self.open.contains(&key(x, y, leaving(dx, dy)))
            || self.open.contains(&key(x + dx, y + dy, entering(dx, dy)))
    }

    /// A tile split by a diagonal wall (0x10 or 0x20): closed to a Sim's steps.
    pub fn diagonal(&self, x: i32, y: i32) -> bool {
        self.at(x, y) & 0x30 != 0
    }

    /// A shortest route from tile to tile, a step at a time to a neighbouring tile (x or y by 1)
    /// through open edges, round tiles `blocked` names (the goal excepted) and the diagonal
    /// walls' tiles: the tiles from the start to the goal, both included; empty if none. A
    /// breadth-first search, its neighbours in the order +x, -x, +y, -y.
    pub fn route(&self, from: Tile, to: Tile, blocked: Option<&dyn Fn(Tile) -> bool>) -> Vec<Tile> {
        if self.width == 0 || !self.inside(from.0, from.1) || !self.inside(to.0, to.1) {
            return Vec::new();
        }
        let w = self.width as usize;
        let index = |t: Tile| t.1 as usize * w + t.0 as usize;
        let mut prev = vec![usize::MAX; self.edges.len()];
        let mut q = VecDeque::from([from]);
        prev[index(from)] = index(from);
        const STEPS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        while let Some(c) = q.pop_front() {
            if c == to {
                let mut out = vec![c];
                let mut i = index(c);
                while i != index(from) {
                    i = prev[i];
                    out.push(((i % w) as i32, (i / w) as i32));
                }
                out.reverse();
                return out;
            }
            for (dx, dy) in STEPS {
                let n = (c.0 + dx, c.1 + dy);
                if !self.inside(n.0, n.1) || prev[index(n)] != usize::MAX || !self.can_step(c.0, c.1, dx, dy) {
                    continue;
                }
                if n != to && (self.diagonal(n.0, n.1) || blocked.is_some_and(|b| b(n))) {
                    continue;
                }
                prev[index(n)] = index(c);
                q.push_back(n);
            }
        }
        Vec::new()
    }

    /// Each closed wall edge (no door open in it) as a rectangle grown by `half` on every side,
    /// {x0, y0, x1, y1} in sixteenths: the walls as the planner keeps clear of them. Diagonal
    /// walls are not among them.
    pub fn wall_rects(&self, half: i32) -> Vec<[i32; 4]> {
        let mut out = Vec::new();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                // the edges towards x + 1 and y + 1 (each wall once)
                if x + 1 < self.width as i32 && !self.can_step(x, y, 1, 0) {
                    let e = (x + 1) * 16;
                    out.push([e - half, y * 16 - half, e + half, (y + 1) * 16 + half]);
                }
                if y + 1 < self.height as i32 && !self.can_step(x, y, 0, 1) {
                    let e = (y + 1) * 16;
                    out.push([x * 16 - half, e - half, (x + 1) * 16 + half, e + half]);
                }
            }
        }
        out
    }

    /// The paved regions outdoors, {x0, y0, x1, y1} in sixteenths: the outdoor tiles whose floor
    /// is not grass (0x00) nor none (0xff), as rectangles (each row's runs, then the rows below with
    /// the same run joined). The planner's search keeps them at cost 0 and its line prefers them
    /// (`docs/simulation.md`, "The search's own regions").
    pub fn paved_regions(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as usize, self.height as usize);
        let paved = |x: usize, y: usize| {
            let i = y * w + x;
            !self.inside[i] && self.floors[i] != 0x00 && self.floors[i] != 0xFF
        };
        let mut runs: Vec<(usize, usize, usize, usize)> = Vec::new(); // x0, x1 (exclusive), y0, y1
        for y in 0..h {
            let mut x = 0;
            while x < w {
                if !paved(x, y) {
                    x += 1;
                    continue;
                }
                let x0 = x;
                while x < w && paved(x, y) {
                    x += 1;
                }
                // joins the run above it with the same span, if one ends on the row above
                if let Some(r) = runs.iter_mut().find(|r| r.0 == x0 && r.1 == x && r.3 == y) {
                    r.3 = y + 1;
                } else {
                    runs.push((x0, x, y, y + 1));
                }
            }
        }
        runs.into_iter().map(|(x0, x1, y0, y1)| [x0 as i32 * 16, y0 as i32 * 16, x1 as i32 * 16, y1 as i32 * 16]).collect()
    }

    pub fn wall_edges(&self) -> usize {
        self.wall_edges
    }
    pub fn openings(&self) -> usize {
        self.open.len()
    }

    /// The rooms: the regions the walls close (a door's opening left closed), numbered from 1 in
    /// the order of their first tile (y, then x); 0 off the grid and on a diagonal wall's tile
    /// (split between two rooms). The lot's third array is not the rooms: it marks the inside
    /// (1) and the outside (7) only, the house's rooms all 1.
    fn find_rooms(&mut self) {
        self.room = vec![0; self.edges.len()];
        let w = self.width as usize;
        const STEPS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        for y0 in 0..self.height as i32 {
            for x0 in 0..self.width as i32 {
                let i0 = y0 as usize * w + x0 as usize;
                if self.room[i0] != 0 || self.diagonal(x0, y0) {
                    continue;
                }
                self.rooms += 1;
                let r = self.rooms as u16;
                let mut q = VecDeque::from([(x0, y0)]);
                self.room[i0] = r;
                while let Some((x, y)) = q.pop_front() {
                    for (dx, dy) in STEPS {
                        let (nx, ny) = (x + dx, y + dy);
                        if !self.inside(nx, ny) {
                            continue;
                        }
                        let n = ny as usize * w + nx as usize;
                        if self.room[n] != 0 || self.diagonal(nx, ny) || self.wall_between(x, y, dx, dy) {
                            continue;
                        }
                        self.room[n] = r;
                        q.push_back((nx, ny));
                    }
                }
            }
        }
        for k in 0..self.doors.len() {
            let d = self.doors[k];
            self.doors[k].from = self.room(d.x, d.y);
            self.doors[k].to = self.room(d.x + d.dx, d.y + d.dy);
        }
    }

    pub fn room(&self, x: i32, y: i32) -> u16 {
        if !self.inside(x, y) || self.room.is_empty() { 0 } else { self.room[y as usize * self.width as usize + x as usize] }
    }
    pub fn rooms(&self) -> usize {
        self.rooms
    }
    pub fn doors(&self) -> &[Door] {
        &self.doors
    }
}
