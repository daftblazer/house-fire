//! The route planner's search: free cells cut from the lot's rects as the search needs them,
//! and an A* from cell to cell (`docs/simulation.md`, "Routing", the planner's search).
//!
//! Points and rects are in sixteenths, y before x (the order the planner keeps them in).

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A rect of the planner's map: half-open, y0..y1 by x0..x1, with its cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRect {
    pub y0: i32,
    pub x0: i32,
    pub y1: i32,
    pub x1: i32,
    pub cost: i32,
}

/// The cost of a rect nothing crosses.
pub const BLOCKED: i32 = i32::MAX;

impl MapRect {
    pub fn from_raw(r: &[i32; 5]) -> Self {
        MapRect { y0: r[0], x0: r[1], y1: r[2], x1: r[3], cost: r[4] }
    }

    fn overlaps(&self, o: &MapRect) -> bool {
        self.x0 < o.x1 && self.x1 > o.x0 && self.y0 < o.y1 && self.y1 > o.y0
    }

    fn inside(&self, o: &MapRect) -> bool {
        self.y0 >= o.y0 && self.y1 <= o.y1 && self.x0 >= o.x0 && self.x1 <= o.x1
    }

    fn side_mut(&mut self, i: usize) -> &mut i32 {
        match i {
            0 => &mut self.y0,
            1 => &mut self.x0,
            2 => &mut self.y1,
            _ => &mut self.x1,
        }
    }

    fn point(y: i32, x: i32) -> Self {
        MapRect { y0: y, x0: x, y1: y + 1, x1: x + 1, cost: 0 }
    }
}

/// How a search grows its cells: the request's settings.
#[derive(Clone, Copy, Debug)]
pub struct Growth {
    /// The longest side a cell may have, when the cap is on.
    pub cap: Option<i32>,
    /// The cost of a cell that lies in no rect.
    pub open_cost: i32,
}

/// The longest a search runs (the caller's budget is at most this many steps).
pub const SEARCH_STEPS: usize = 1000;

struct Map<'a> {
    rects: &'a [MapRect],
    growth: Growth,
    cells: Vec<MapRect>,
}

impl Map<'_> {
    /// The rect a cell is "in": the first blocked one it overlaps, else the last it overlaps.
    fn region(&self, r: &MapRect) -> Option<usize> {
        let mut last = None;
        for (i, g) in self.rects.iter().enumerate() {
            if r.overlaps(g) {
                if g.cost == BLOCKED {
                    return Some(i);
                }
                last = Some(i);
            }
        }
        last
    }

    fn blocked(&self, region: Option<usize>) -> bool {
        region.is_some_and(|i| self.rects[i].cost == BLOCKED)
    }

    fn in_cell(&self, r: &MapRect) -> Option<usize> {
        self.cells.iter().position(|c| r.overlaps(c))
    }

    /// Grows a cell from the point (y, x): each side in turn (y0, x0, y1, x1) by its step, from
    /// 32 (a quarter of the cap) and halved on each failure, the same side tried again until it
    /// holds or its step is 0. A move holds while the cell is in the seed's own region, unblocked,
    /// clear of every cell, inside that region and within the cap.
    fn grow(&mut self, y: i32, x: i32) -> Option<usize> {
        let mut r = MapRect::point(y, x);
        let home = self.region(&r);
        if self.blocked(home) || self.in_cell(&r).is_some() {
            return None;
        }
        let s = self.growth.cap.map_or(32, |c| (c / 4).max(1));
        let mut steps = [-s, -s, s, s];
        let (mut side, mut idle) = (0, 0);
        for _ in 0..200 {
            let d = steps[side];
            if d == 0 {
                idle += 1;
            } else {
                idle = 0;
                *r.side_mut(side) += d;
                let fits = self.region(&r) == home
                    && self.in_cell(&r).is_none()
                    && home.is_none_or(|h| r.inside(&self.rects[h]))
                    && self.growth.cap.is_none_or(|c| r.y1 - r.y0 <= c && r.x1 - r.x0 <= c);
                if !fits {
                    *r.side_mut(side) -= d;
                    steps[side] = d / 2;
                    continue;
                }
            }
            side = (side + 1) % 4;
            if idle > 3 {
                break;
            }
        }
        r.cost = home.map_or(self.growth.open_cost, |h| self.rects[h].cost);
        self.cells.push(r);
        Some(self.cells.len() - 1)
    }

    fn cell_at(&mut self, y: i32, x: i32) -> Option<usize> {
        self.in_cell(&MapRect::point(y, x)).or_else(|| self.grow(y, x))
    }

    /// Cuts the cells round cell `i`: walks once round it just outside, the x0 side in rising y,
    /// the y1 side in rising x, the x1 side in falling y and the y0 side in falling x, growing a
    /// cell from every point that is neither blocked nor in a cell yet.
    fn cut_round(&mut self, i: usize) {
        let c = self.cells[i];
        let walk = (c.y0..c.y1).map(|y| (y, c.x0 - 1))
            .chain((c.x0..c.x1).map(|x| (c.y1, x)))
            .chain((c.y0..c.y1).rev().map(|y| (y, c.x1)))
            .chain((c.x0..c.x1).rev().map(|x| (c.y0 - 1, x)));
        for (y, x) in walk {
            let p = MapRect::point(y, x);
            if !self.blocked(self.region(&p)) && self.in_cell(&p).is_none() {
                self.grow(y, x);
            }
        }
    }
}

/// The opening between two cells: their overlap with each grown by 1 (exclusive bounds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opening {
    pub y0: i32,
    pub x0: i32,
    pub y1: i32,
    pub x1: i32,
}

/// Where a step from one cell to another crosses: the centre of their opening, each axis
/// rounded toward zero; none when they do not touch.
fn crossing(a: &MapRect, b: &MapRect) -> Option<((i32, i32), Opening)> {
    let (ly, lx) = ((a.y0 - 1).max(b.y0 - 1), (a.x0 - 1).max(b.x0 - 1));
    let (hy, hx) = ((a.y1 + 1).min(b.y1 + 1), (a.x1 + 1).min(b.x1 + 1));
    (ly < hy && lx < hx).then(|| (((ly + hy) / 2, (lx + hx) / 2), Opening { y0: ly, x0: lx, y1: hy, x1: hx }))
}

/// A search's way: its points (start, crossings, goal) and each crossing's opening.
#[derive(Clone, Debug, PartialEq)]
pub struct Way {
    pub points: Vec<(i32, i32)>,
    pub openings: Vec<Opening>,
}

fn distance(a: (i32, i32), b: (i32, i32)) -> f32 {
    (((a.0 - b.0) as f32).powi(2) + ((a.1 - b.1) as f32).powi(2)).sqrt()
}

/// A step's weight: the two cells' costs' mean plus one, rounded down.
pub fn step_weight(a: i32, b: i32) -> f32 {
    ((a + b + 2) >> 1) as f32
}

#[derive(PartialEq)]
struct Open(f32, usize);

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// The search's coarse way from `start` to `goal` (both (y, x)): the crossings it goes through,
/// start and goal included; none when it finds no way within its steps. `rects` are the lot's
/// rects as the search sees them (the obstacles, then the regions, the blocked ones grown).
pub fn coarse_way(rects: &[MapRect], growth: Growth, start: (i32, i32), goal: (i32, i32)) -> Option<Way> {
    let mut map = Map { rects, growth, cells: Vec::new() };
    let first = map.cell_at(start.0, start.1)?;
    map.cells[first].cost = 0;
    let last = map.cell_at(goal.0, goal.1);
    // node per cell, and one past them for the goal
    let goal_node = usize::MAX;
    let h = |p: (i32, i32)| distance(p, goal);
    let mut g: Vec<Option<f32>> = vec![None; map.cells.len()];
    let mut point: Vec<(i32, i32)> = vec![(0, 0); map.cells.len()];
    let mut opening: Vec<Option<Opening>> = vec![None; map.cells.len()];
    let mut parent: Vec<Option<usize>> = vec![None; map.cells.len()];
    let mut closed: Vec<bool> = vec![false; map.cells.len()];
    let (mut goal_g, mut goal_parent): (Option<f32>, Option<usize>) = (None, None);
    g[first] = Some(0.0);
    point[first] = start;
    let mut open = BinaryHeap::from([Open(h(start), first)]);
    for _ in 0..SEARCH_STEPS {
        let Open(_, c) = open.pop()?;
        if c == goal_node {
            let (mut points, mut openings) = (vec![goal], Vec::new());
            let mut n = goal_parent;
            while let Some(i) = n {
                points.push(point[i]);
                openings.extend(opening[i]);
                n = parent[i];
            }
            points.reverse();
            openings.reverse();
            return Some(Way { points, openings });
        }
        if closed[c] {
            continue;
        }
        closed[c] = true;
        map.cut_round(c);
        let n = map.cells.len();
        g.resize(n, None);
        point.resize(n, (0, 0));
        opening.resize(n, None);
        parent.resize(n, None);
        closed.resize(n, false);
        let gc = g[c].unwrap_or(0.0);
        let here = map.cells[c];
        for b in 0..n {
            if b == c || closed[b] || map.cells[b].cost == BLOCKED {
                continue;
            }
            let Some((p, o)) = crossing(&here, &map.cells[b]) else { continue };
            let ng = gc + (distance(point[c], p) + 1.0) * step_weight(here.cost, map.cells[b].cost);
            if g[b].is_none_or(|old| ng < old) {
                g[b] = Some(ng);
                point[b] = p;
                opening[b] = Some(o);
                parent[b] = Some(c);
                open.push(Open(ng + h(p), b));
            }
        }
        if Some(c) == last {
            let ng = gc + (distance(point[c], goal) + 1.0) * step_weight(here.cost, here.cost);
            if goal_g.is_none_or(|old| ng < old) {
                goal_g = Some(ng);
                goal_parent = Some(c);
                open.push(Open(ng, goal_node));
            }
        }
    }
    None
}

/// The plan's points from a search's way: each crossing slides along its opening (across a
/// thin opening the other axis stays the crossing's own) to where the line between its two
/// neighbours meets it, rounded to the nearest and kept inside the opening, over and over until
/// nothing moves.
pub fn refine(way: &Way) -> Vec<(i32, i32)> {
    let mut pts = way.points.clone();
    for _ in 0..50 {
        let mut moved = false;
        for i in 1..pts.len().saturating_sub(1) {
            let Some(o) = way.openings.get(i - 1) else { break };
            let (a, b) = (pts[i - 1], pts[i + 1]);
            let along = |from: i32, to: i32, at: f32, wf: i32, wt: i32| -> i32 {
                let t = if wt != wf { (at - wf as f32) / (wt - wf) as f32 } else { 0.5 };
                (from as f32 + t * (to - from) as f32 + 0.5).floor() as i32
            };
            let q = if o.y1 - o.y0 <= 2 {
                let y = pts[i].0;
                (y, along(a.1, b.1, y as f32, a.0, b.0).clamp(o.x0 + 1, o.x1 - 1))
            } else {
                let x = pts[i].1;
                (along(a.0, b.0, x as f32, a.1, b.1).clamp(o.y0 + 1, o.y1 - 1), x)
            };
            if q != pts[i] {
                pts[i] = q;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    pts
}
