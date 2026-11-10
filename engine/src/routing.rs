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

/// An object's footprint on its tile, {x0, y0, x1, y1} in sixteenths: the tile less its
/// definition's footprint mask (OBJD field 75), whose nibble k (the low first) is the inset of
/// the side k quarter turns from the object's front (`front(facing + 2k)`). Measured on the home
/// log's obstacles (`docs/simulation.md`, "The footprints"): the trees' quarters turning with
/// their facing, the armchair's 1 at each side, the jukebox's 6 at its back.
pub fn footprint(mask: u16, x16: i32, y16: i32, facing: i16) -> [i32; 4] {
    let (x, y) = (x16 & !15, y16 & !15);
    let mut r = [x, y, x + 16, y + 16];
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
pub fn obstacle(o: &crate::world::Object, mask: u16, half: i32) -> Option<[i32; 5]> {
    use crate::world::{DATA_CONTAINER, DATA_FLAGS};
    let flags = o.data[DATA_FLAGS];
    if flags & 0x4 != 0 || o.data[DATA_CONTAINER] != 0 {
        return None;
    }
    let f = footprint(mask, o.x16, o.y16, o.facing());
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
