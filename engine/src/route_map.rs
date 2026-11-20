//! The planner's rects, natively: what the search (`planner::coarse_way`, `choose_goal`) is given
//! for a plan, built from the world (`docs/simulation.md`, "The planner's obstacles, logged",
//! "The search's own regions, logged"): the obstacles (the objects' footprints, the persons near
//! the Sim) first, then the search's own regions (the lot's border or the house's box, the
//! rooms closed to the plan, the walls, the paved floors).
//!
//! Points and rects here are in sixteenths. Obstacles and regions are {x0, y0, x1, y1, cost};
//! the search's `MapRect`s are the same turned (y before x).

use crate::planner::{Growth, MapRect};
use crate::routing::{obstacle, overlaps, ungrown, Walls, BLOCKS};
use crate::world::{Object, World};

/// Half the global width the planner grows a blocking obstacle by (the global at r13 - 28468 is 6
/// in the home log's obstacles: `PlanRec::obstacle_width`).
pub const OBSTACLE_HALF: i32 = 3;
/// A person's footprint round its place: 6 on a side (read off the logged obstacles).
pub const PERSON_HALF: i32 = 3;
/// A person farther than this from the planning Sim (three tiles, the constant at 802CFAAC) is
/// left out of the plan.
pub const PERSON_NEAR: i32 = 48;
/// How much the search's lookup grows a region that blocks (the lot's border (0, 0, 16, 624) is
/// (-7, -7, 23, 631) in the list the search reads).
pub const REGION_GROWTH: i32 = 7;
/// The cap of a cell's side when the search runs capped (the request's +140), and the cost of a
/// cell in no region (the request's +48: 3 in the yard's plans, 0 in the house's), by the home
/// log's two kinds of plan.
const CELL_CAP: i32 = 128;
const OPEN_COST_OUTDOORS: i32 = 3;
const OPEN_COST_INDOORS: i32 = 0;

/// The obstacles the search gathers for a plan of the Sim `sim_id` (to `target`, an object it
/// walks to, which a person is not an obstacle for): every object on the Sim's level that
/// `routing::obstacle` keeps, in the world's order, its place as `place` gives it (a person's
/// is where `place` says it stands), less those whose footprint overlaps the Sim's own; a person is the
/// person kept unless the plan's target or three tiles off (`PERSON_NEAR`). {x0, y0, x1, y1, cost}.
pub fn obstacles(world: &World, sim_id: i16, target: Option<i16>, goals: &[(i32, i32)], place: &dyn Fn(&Object) -> (i32, i32)) -> Vec<[i32; 5]> {
    let Some(sim) = world.find_object(sim_id) else { return Vec::new() };
    let at = place(sim);
    let sim_rect = [at.0 - PERSON_HALF, at.1 - PERSON_HALF, at.0 + PERSON_HALF, at.1 + PERSON_HALF];
    let mut out = Vec::new();
    for o in world.objects() {
        if !o.on_lot() || o.level != sim.level || o.id() == sim_id {
            continue;
        }
        let rect = if o.person {
            let p = place(o);
            let d2 = (p.0 - at.0).pow(2) + (p.1 - at.1).pow(2);
            if Some(o.id()) == target || d2 >= PERSON_NEAR * PERSON_NEAR {
                continue;
            }
            let h = PERSON_HALF + OBSTACLE_HALF;
            [p.0 - h, p.1 - h, p.0 + h, p.1 + h, BLOCKS]
        } else {
            let Some(d) = world.definition(o) else { continue };
            let Some(r) = obstacle(o, d.field(75), d.field(65), OBSTACLE_HALF) else { continue };
            r
        };
        // A person, or the object the plan goes to, standing on a place the request offers does not
        // close it (a slot that names no direction offers its object's own place, where the club's
        // dancers all stand): a stand-in, the original's rule for a goal inside an obstacle is not logged.
        let on_goal = (o.person || Some(o.id()) == target) && goals.iter().any(|g| g.0 > rect[0] && g.0 < rect[2] && g.1 > rect[1] && g.1 < rect[3]);
        if !on_goal && !overlaps(&ungrown(&rect, OBSTACLE_HALF), &sim_rect) {
            out.push(rect);
        }
    }
    out
}

/// A closed region's rectangles from the tiles a test names: each row's runs, joined with the
/// run below of the same span (as `Walls::paved_regions` joins its own).
fn tile_rects(w: usize, h: usize, wanted: impl Fn(usize, usize) -> bool) -> Vec<[i32; 4]> {
    let mut runs: Vec<(usize, usize, usize, usize)> = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if !wanted(x, y) {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && wanted(x, y) {
                x += 1;
            }
            if let Some(r) = runs.iter_mut().find(|r| r.0 == x0 && r.1 == x && r.3 == y) {
                r.3 = y + 1;
            } else {
                runs.push((x0, x, y, y + 1));
            }
        }
    }
    runs.into_iter().map(|(x0, x1, y0, y1)| [x0 as i32 * 16, y0 as i32 * 16, x1 as i32 * 16, y1 as i32 * 16]).collect()
}

/// The search's own regions for a plan between places in the rooms `rooms`, {x0, y0, x1, y1, cost},
/// and whether the plan is outdoors (its domain the whole lot) or indoors.
///
/// Outdoors (a plan with an end outside the house; the home log's yard plans): the lot's border
/// (four strips a tile wide), the house's rooms the plan has no end in (closed to it; each as the
/// rectangles of its tiles), the walls as zero-width lines (each closed edge, a door's opening
/// left open), the diagonal walls' tiles as two points, and the paved floors at cost 0. Indoors
/// (both ends in the house's rooms; the home log's house plans): the same for a box round the
/// plan's rooms, two tiles out, its border a ring a tile wide, every tile of it that is in
/// no room of the plan (a room not its own, the yard) closed, the walls and diagonals; no paved
/// floors. The rule is the closest the logs give: the original's own regions differ in how the
/// rectangles are cut (`docs/simulation.md`).
pub fn regions(walls: &Walls, rooms: &[u16]) -> (Vec<[i32; 5]>, bool) {
    let (w, h) = walls.size();
    let (w, h) = (w as usize, h as usize);
    let indoors = |x: usize, y: usize| walls.indoors(x as i32, y as i32);
    let room_at = |x: usize, y: usize| walls.room(x as i32, y as i32);
    let in_house = |r: u16| (0..h).any(|y| (0..w).any(|x| indoors(x, y) && room_at(x, y) == r));
    let outside = rooms.iter().filter(|&&r| !in_house(r)).count();
    // outdoors: the plan has an end outside the house
    let outdoors = outside > 0;
    let mut out: Vec<[i32; 5]> = Vec::new();
    let mut add = |r: [i32; 4], cost: i32| out.push([r[0], r[1], r[2], r[3], cost]);
    if outdoors {
        let (lw, lh) = (w as i32 * 16, h as i32 * 16);
        add([0, 0, lw, 16], BLOCKS);
        add([0, lh - 16, lw, lh], BLOCKS);
        add([0, 0, 16, lh], BLOCKS);
        add([lw - 16, 0, lw, lh], BLOCKS);
        // the house closed to a plan between two places outside it
        if outside == rooms.len() {
            for r in tile_rects(w, h, |x, y| indoors(x, y) && room_at(x, y) != 0) {
                add(r, BLOCKS);
            }
        }
    } else {
        // the box: the house's extent, two tiles out, within the lot; its tiles outside the house closed
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| indoors(x, y)) {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        }
        if x1 > x0 {
            let (x0, y0, x1, y1) = (x0.saturating_sub(2), y0.saturating_sub(2), (x1 + 2).min(w), (y1 + 2).min(h));
            let (px0, py0, px1, py1) = (x0 as i32 * 16, y0 as i32 * 16, x1 as i32 * 16, y1 as i32 * 16);
            add([px0, py0, px1, py0 + 16], BLOCKS);
            add([px0, py1 - 16, px1, py1], BLOCKS);
            add([px0, py0, px0 + 16, py1], BLOCKS);
            add([px1 - 16, py0, px1, py1], BLOCKS);
            let inner = |x: usize, y: usize| x > x0 && x + 1 < x1 && y > y0 && y + 1 < y1;
            for r in tile_rects(w, h, |x, y| inner(x, y) && !indoors(x, y) && room_at(x, y) != 0) {
                add(r, BLOCKS);
            }
        }
    }
    for r in walls.wall_rects(0) {
        add(r, BLOCKS);
    }
    // a diagonal wall's tile: the two points a quarter and three quarters along its line
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (px, py) = (x * 16, y * 16);
            let points = match walls.diagonal_bits(x, y) {
                0 => continue,
                0x10 => [(px + 4, py + 4), (px + 12, py + 12)],
                _ => [(px + 12, py + 4), (px + 4, py + 12)],
            };
            for p in points {
                add([p.0, p.1, p.0, p.1], BLOCKS);
            }
        }
    }
    if outdoors {
        for r in walls.paved_regions() {
            add(r, 0);
        }
    }
    (out, outdoors)
}

/// The search's rects for a plan: the obstacles, then the regions with those that block grown by
/// `REGION_GROWTH`, turned to the search's (y, x) order; with the growth settings that go with
/// the kind of plan.
pub fn search_rects(obstacles: &[[i32; 5]], regions: &[[i32; 5]], outdoors: bool) -> (Vec<MapRect>, Growth) {
    let mut rects: Vec<MapRect> = obstacles.iter().map(|r| MapRect { y0: r[1], x0: r[0], y1: r[3], x1: r[2], cost: r[4] }).collect();
    rects.extend(regions.iter().map(|r| {
        let g = if r[4] == BLOCKS { REGION_GROWTH } else { 0 };
        MapRect { y0: r[1] - g, x0: r[0] - g, y1: r[3] + g, x1: r[2] + g, cost: r[4] }
    }));
    let growth = if outdoors { Growth { cap: Some(CELL_CAP), open_cost: OPEN_COST_OUTDOORS } } else { Growth { cap: None, open_cost: OPEN_COST_INDOORS } };
    (rects, growth)
}

/// Everything a plan's search is given, natively: the rects and settings for the Sim `sim_id`
/// going from `start` to places `goals` (sixteenths; the rooms of both ends decide which of the
/// house is closed), to the object `target` if any.
pub fn search_for(world: &World, sim_id: i16, target: Option<i16>, start: (i32, i32), goals: &[(i32, i32)], place: &dyn Fn(&Object) -> (i32, i32)) -> (Vec<MapRect>, Growth) {
    let room = |p: (i32, i32)| world.walls.room(p.0 >> 4, p.1 >> 4);
    let mut rooms: Vec<u16> = std::iter::once(start).chain(goals.iter().copied()).map(room).filter(|&r| r != 0).collect();
    rooms.sort_unstable();
    rooms.dedup();
    let (regions, outdoors) = regions(&world.walls, &rooms);
    search_rects(&obstacles(world, sim_id, target, goals, place), &regions, outdoors)
}
