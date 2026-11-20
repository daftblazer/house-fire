use std::collections::{BTreeMap, BTreeSet};

use crate::cfmt::g;
use crate::library::Library;
use crate::routing::{direction_angle, turn_towards, BLOCKS, FrameClock, OFF_PAVED_WEIGHT, Tile, turn_frames, WALK_FRAME, weighed_path, WALK_TOP_ADULT, WALK_TOP_CHILD, walk_top, slot_area, slot_goal_score, slot_pick, run_on_point, walk_frames, routing_return_tick, along_line, walk_rates, walk_step, footprint, front, obstacle, overlaps, segment_enters, slot_places, ungrown};
use crate::planner::{choose_goal, coarse_way, point_after_goal, refine, Growth, MapRect};
use crate::library::Slot;
use crate::tree_log::{Log, StepRec};
use crate::world::World;
use super::study::{env, has, file_name};
use super::counts::Counts;
use super::walks::native_plan_points;

/// A person's own part among the planner's tested objects ('J'): its offset from the listed
/// object the observer files it under.
pub(super) const PERSON_PART_OFFSET: i32 = -104;

/// The routing checks: the Sims' steps, the original's plans, its routing steps' slots, and the
/// walks against the native route.
/// The routing runs' native returns: (sim, tree, node) of the routing step, its run's first tick,
/// the tick it returns natively (its walk, turn and frames as `routing_return_tick` gives them).
pub(super) type RoutingReturns = BTreeMap<(u32, u16, u16), Vec<(u32, u32)>>;

/// What the routing pass gives the host: each routing run's native return tick, and each walking
/// Sim's native place at the ticks the log has one (by id and tick).
#[derive(Default)]
pub(super) struct NativeRouting {
    pub(super) returns: RoutingReturns,
    pub(super) places: BTreeMap<(i16, u32), (i32, i32)>,
}

pub(super) fn routing(log: &Log, lib: &Library, world: &World, loaded: &World, counts: &mut Counts, ids: &BTreeMap<u32, i16>) -> NativeRouting {
    let mut returns: RoutingReturns = BTreeMap::new();
    let mut native_places: BTreeMap<(i16, u32), (i32, i32)> = BTreeMap::new();
    let walls = &world.walls;
    let show = has("REPLAY_SHOW_ROUTING");
    counts.add("routing: the lot's wall edges (each seen from both tiles)", walls.wall_edges());
    counts.add("routing: the doors' openings", walls.openings());
    counts.add("routing: the rooms (the regions the walls close)", walls.rooms());
    if show {
        for d in walls.doors() {
            println!("door {},{} +{},{}: room {} to {}", d.x, d.y, d.dx, d.dy, d.from, d.to);
        }
    }
    let mut tile: BTreeMap<i16, Tile> = BTreeMap::new();
    for p in &log.places {
        let Some(o) = world.find_object(p.id) else { continue };
        if !o.person || p.x < 0 || p.y < 0 {
            continue;
        }
        let (x, y) = (p.x >> 4, p.y >> 4);
        if let Some(&(tx, ty)) = tile.get(&p.id)
            && (tx, ty) != (x, y)
        {
            let (dx, dy) = (x - tx, y - ty);
            if dx.abs() + dy.abs() != 1 {
                counts.note("routing: a Sim's move not to a neighbouring tile (not a step)");
                if show {
                    println!("Sim {} moved {tx},{ty} -> {x},{y} (tick {})", p.id, p.tick);
                }
            } else {
                let ok = walls.can_step(tx, ty, dx, dy);
                counts.note(if ok { "routing: a Sim's step between tiles open natively" } else { "routing: a Sim's step through a WALL natively" });
                if ok && walls.wall_between(tx, ty, dx, dy) {
                    counts.note("routing: a Sim's step through a door's opening");
                }
                if !ok && show {
                    println!("Sim {} through a wall: {tx},{ty} -> {x},{y} (tick {})", p.id, p.tick);
                }
            }
        }
        tile.insert(p.id, (x, y));
    }
    // The original's own planned routes ('G': waypoints in sixteenths): every tile change along
    // their lines open natively (a corner crossed: either way round), and the objects whose
    // tiles they cross.
    let mut crossed: BTreeMap<String, usize> = BTreeMap::new();
    for pl in &log.plans {
        if pl.points.len() < 2 {
            continue;
        }
        let mut open = true;
        let mut i = 1;
        while i < pl.points.len() && open {
            let (x0, y0) = pl.points[i - 1];
            let (x1, y1) = pl.points[i];
            let n = (x1 - x0).abs().max((y1 - y0).abs());
            let (mut tx, mut ty) = (x0 >> 4, y0 >> 4);
            let mut k = 1;
            while k <= n && open {
                let (nx, ny) = ((x0 + (x1 - x0) * k / n) >> 4, (y0 + (y1 - y0) * k / n) >> 4);
                k += 1;
                if nx == tx && ny == ty {
                    continue;
                }
                let (dx, dy) = (nx - tx, ny - ty);
                open = if dx.abs() + dy.abs() == 1 {
                    walls.can_step(tx, ty, dx, dy)
                } else {
                    (walls.can_step(tx, ty, dx, 0) && walls.can_step(tx + dx, ty, 0, dy)) || (walls.can_step(tx, ty, 0, dy) && walls.can_step(tx, ty + dy, dx, 0))
                };
                if !open && show {
                    println!("plan at tick {} through a wall: {tx},{ty} -> {nx},{ny}", pl.tick);
                }
                tx = nx;
                ty = ny;
                for o in world.objects() {
                    if !o.person && o.level == 1 && o.on_lot() && (o.x16 >> 4) == nx && (o.y16 >> 4) == ny
                        && let Some(f) = o.file
                    {
                        *crossed.entry(file_name(lib, f)).or_default() += 1;
                    }
                }
            }
            i += 1;
        }
        counts.note(if open { "routing: an original's planned route open natively (walls, doors)" } else { "routing: an original's planned route through a WALL natively" });
        // The rooms along it (Walls::room): each change of room between two waypoints a single
        // step through a door's opening (the route broken at its doors), each piece between
        // within one room (its waypoints' tiles; a diagonal wall's tile, 0, either).
        let mut at_doors = true;
        let mut changes = 0;
        for i in 1..pl.points.len() {
            let (x0, y0) = (pl.points[i - 1].0 >> 4, pl.points[i - 1].1 >> 4);
            let (x1, y1) = (pl.points[i].0 >> 4, pl.points[i].1 >> 4);
            let (r0, r1) = (walls.room(x0, y0), walls.room(x1, y1));
            if r0 == r1 || r0 == 0 || r1 == 0 {
                continue;
            }
            changes += 1;
            let (dx, dy) = (x1 - x0, y1 - y0);
            let door = walls.doors().iter().any(|d| (d.x == x0 && d.y == y0 && d.dx == dx && d.dy == dy) || (d.x == x1 && d.y == y1 && d.dx == -dx && d.dy == -dy));
            if !door {
                at_doors = false;
                if show {
                    println!("plan at tick {} changes room {r0} -> {r1} not at a door: {x0},{y0} -> {x1},{y1}", pl.tick);
                }
            } else {
                counts.note("routing: a plan's change of room at a door, a waypoint either side");
            }
        }
        if changes != 0 {
            counts.note(if at_doors { "routing: a plan through rooms broken at each door" } else { "routing: a plan changing rooms NOT at a door" });
        } else {
            counts.note("routing: a plan within one room");
        }
    }
    // Every routing step (GotoRoutingSlot, opcode 45): the routing slot its operand names (the
    // number, where from), found natively in the stack object's SLOT or the global one.
    let raw45 = |s: &StepRec| -> Option<[u8; 12]> {
        let t = log.tree_of(s)?;
        let at = s.node as usize * 12;
        if at + 12 > t.raw.len() {
            return None;
        }
        let raw: [u8; 12] = t.raw[at..at + 12].try_into().unwrap();
        ((u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF) == 45).then_some(raw)
    };
    // study (REPLAY_SHOW_ROUTE_OPS): each routing step that walks (opcode 27 or 45, result 1), its
    // operand bytes
    if has("REPLAY_SHOW_ROUTE_OPS") {
        for s in log.steps.iter().filter(|s| s.result == 1) {
            if let Some(t) = log.tree_of(s) {
                let at = s.node as usize * 12;
                if at + 12 <= t.raw.len() {
                    let op = u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF;
                    if matches!(op, 27 | 45) {
                        println!("route op tick {} sim {} opcode {op} operands {:02x?} stack {}", s.tick, ids.get(&s.sim).copied().unwrap_or(-1), &t.raw[at + 4..at + 12], s.stack_object);
                    }
                }
            }
        }
    }
    if has("REPLAY_SHOW_27") {
        for s in log.steps.iter().filter(|s| s.result == 1) {
            let Some(t) = log.tree_of(s) else { continue };
            let at = s.node as usize * 12;
            if at + 12 > t.raw.len() || !matches!(u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, 27 | 45) {
                continue;
            }
            let sim = ids.get(&s.sim).copied().unwrap_or(-1);
            let Some(pl) = log.plans.iter().find(|p| p.tick == s.tick && p.approach.is_some_and(|a| a[7] as i16 == sim)) else { continue };
            let o = world.find_object(s.stack_object).map(|o| (o.x16, o.y16, o.facing(), o.person));
            println!("op27 tick {} sim {sim} op {} tree {} node {} operand {:02x?} params {:?} stack object {} {o:?}; plan from {:?} approach {:?} goals {:?} chosen {} points {:?}", s.tick, u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, s.tree, s.node, &t.raw[at + 4..at + 12], s.params, s.stack_object, pl.points.first(), pl.approach, pl.goals, pl.chosen, pl.points);
        }
    }
    for s in &log.steps {
        let Some(raw) = raw45(s) else { continue };
        let number = u16::from_be_bytes([raw[4], raw[5]]);
        let scope = u16::from_be_bytes([raw[6], raw[7]]);
        let o = world.find_object(s.stack_object);
        let Some((o, file)) = o.and_then(|o| o.file.map(|f| (o, f))) else {
            counts.note("routing: a routing step's stack object not on the lot natively");
            continue;
        };
        let slot = lib.routing_slot(file, world.definition(o), scope, number, &s.params);
        counts.note(match (slot.is_some(), scope) {
            (true, 0) => "routing: a routing step's slot found natively (the object's own, its number a parameter's)",
            (true, 1) => "routing: a routing step's slot found natively (the object's own)",
            (true, _) => "routing: a routing step's slot found natively (the global one)",
            _ => "routing: a routing step's slot NOT found natively",
        });
    }
    // A plan that ends at its routing step's object (the step's stack object, in the plan's
    // tick, at the plan's last point): the waypoint before it the tile in front of the object
    // (`front`, by its facing), one tile away; the Sim walks there, then turns to the object.
    for pl in &log.plans {
        if pl.points.len() < 3 {
            continue;
        }
        let end = pl.points[pl.points.len() - 1];
        let stand = pl.points[pl.points.len() - 2];
        let target = log.steps.iter().filter(|s| s.tick == pl.tick).find_map(|s| {
            let t = log.tree_of(s).filter(|t| t.tree == s.tree)?;
            let at = s.node as usize * 12;
            let op = u16::from_be_bytes([*t.raw.get(at)?, *t.raw.get(at + 1)?]) & 0x7FFF;
            let o = world.find_object(s.stack_object)?;
            (matches!(op, 27 | 45) && (o.x16, o.y16) == end).then_some(o)
        });
        let Some(o) = target else {
            counts.note("routing: a plan not ending at its routing step's object");
            continue;
        };
        let want = front(o.facing()).map(|(dx, dy)| (o.x16 + 16 * dx, o.y16 + 16 * dy));
        counts.note(if want == Some(stand) {
            "routing: a plan ending at its object, from the tile in front of it (its facing)"
        } else {
            "routing: a plan ending at its object, NOT from the tile in front of it"
        });
        if want != Some(stand) && show {
            println!(
                "plan at tick {} to object {} (facing {}): from {},{}, in front {:?}",
                pl.tick,
                o.id(),
                o.facing(),
                stand.0,
                stand.1,
                want
            );
        }
    }
    // A routing step's goal natively (GotoRoutingSlot, a literal or global slot): of the places
    // its slot offers (`slot_places`), the nearest the Sim (from the plan's start) whose tile
    // the walls let it reach and no other object stands on; against the plan the step made in
    // its tick (its start the Sim's place) by the step that walks: its end, or the tile before
    // the object where it ends at the object.
    let place_at = |id: i16, tick: u32| log.places.iter().rfind(|p| p.id == id && p.tick <= tick).map(|p| (p.x, p.y));
    for s in &log.steps {
        let Some(raw) = raw45(s) else { continue };
        // The step that walks (yields, result 1) made the plan; one that ends (a return to its
        // tree's true or false target) only shares the tick.
        if s.result != 1 {
            continue;
        }
        let (number, scope) = (u16::from_be_bytes([raw[4], raw[5]]), u16::from_be_bytes([raw[6], raw[7]]));
        let Some(sim) = ids.get(&s.sim).copied() else { continue };
        let Some(start) = place_at(sim, s.tick) else { continue };
        // the plan this step's Sim made (by its id where the log names the planner, 'h')
        let sim_id = ids.get(&s.sim).copied();
        let Some(pl) = log.plans.iter().find(|p| {
            p.tick == s.tick && p.points.first() == Some(&start) && p.points.len() >= 2 && p.approach.is_none_or(|a| Some(a[7] as i16) == sim_id)
        }) else {
            continue;
        };
        let Some(o) = world.find_object(s.stack_object) else {
            // An object not on the lot natively: its goals (logs with 'H') checked by their shape
            // alone, round the plan's end (its object) as some facing and proximity give them.
            if let Some(&center) = pl.points.last().filter(|_| !pl.goals.is_empty()) {
                let bits: i32 = pl.goals.iter().map(|g| (g[3] & 0xFF) as i32).fold(0, |a, b| a | b);
                let want: Vec<(u8, (i32, i32))> =
                    pl.goals.iter().map(|g| ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32))).collect();
                let fits = (0..8).any(|f| {
                    (1..=64).any(|p| {
                        let slot = Slot { flags: bits, optimal_proximity: p, ..Slot::default() };
                        slot_places(&slot, center.0, center.1, f) == want
                    })
                });
                counts.note(if fits {
                    "routing: a request's goals round an object not on the lot natively as a slot's (some facing and proximity)"
                } else {
                    "routing: a request's goals round an object not on the lot natively NOT a slot's"
                });
            }
            continue;
        };
        let Some(file) = o.file else { continue };
        let Some(slot) = lib.routing_slot(file, world.definition(o), scope, number, &s.params) else { continue };
        let target = place_at(o.id(), s.tick).unwrap_or((o.x16, o.y16));
        let end = pl.points[pl.points.len() - 1];
        let want = if end == target { pl.points[pl.points.len() - 2] } else { end };
        let tile = |p: (i32, i32)| (p.0 >> 4, p.1 >> 4);
        let occupied = |p: (i32, i32)| {
            world.objects().any(|x| {
                !x.person && x.id() != o.id() && x.on_lot() && x.level == 1 && tile((x.x16, x.y16)) == tile(p)
            })
        };
        // a person faces the way it last stepped (its last two logged places before the step,
        // eighths of a turn: 0 to -x, 2 to +y, 4 to +x, 6 to -y), else as the save has it
        let facing = {
            let mut seen = log.places.iter().filter(|p| p.id == o.id() && p.tick <= s.tick).rev();
            match (o.person, seen.next(), seen.next()) {
                (true, Some(b), Some(a)) => match ((b.x - a.x).signum(), (b.y - a.y).signum()) {
                    (-1, 0) => 0,
                    (-1, 1) => 1,
                    (0, 1) => 2,
                    (1, 1) => 3,
                    (1, 0) => 4,
                    (1, -1) => 5,
                    (0, -1) => 6,
                    (-1, -1) => 7,
                    _ => o.facing(),
                },
                _ => o.facing(),
            }
        };
        let offered = slot_places(slot, target.0, target.1, facing);
        let mut places = offered.clone();
        // A place across a wall from its object (another room, though a door may lead there) is
        // not offered, nor one another object (not a person) stands on.
        let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
        places.retain(|&(_, p)| !occupied(p) && room(p) == room(target));
        // The request's goals (logs with 'H'): a slot of one proximity offers exactly the open
        // places, in order, each with its direction's bit; one of a range offers an area (not yet).
        if !pl.goals.is_empty() {
            // a goal's score (its third word), for the goals that are the slot's own places: the
            // slot's, with its random part
            let score = |counts: &mut Counts, g: &[u32; 4]| {
                counts.note(if slot_goal_score(slot, g[3] >> 16 != 0).contains(&g[2]) {
                    "routing: a slot goal's score natively the original's (135, 90 more for a named object, the random part when flagged)"
                } else {
                    "routing: a slot goal's score natively NOT the original's"
                });
            };
            if slot.min_proximity == slot.max_proximity {
                let same = places.len() == pl.goals.len()
                    && places.iter().zip(&pl.goals).all(|(&(k, p), g)| (g[0] as i32, g[1] as i32) == p && g[3] & 0xFF == 1 << k);
                if same {
                    pl.goals.iter().for_each(|g| score(counts, g));
                }
                counts.note(if same {
                    "routing: a request's goals natively the open places of its slot, in order, with their bits"
                } else {
                    "routing: a request's goals natively NOT the open places of its slot"
                });
                if !same && has("REPLAY_SHOW_GOALS") {
                    let theirs: Vec<_> = pl.goals.iter().map(|g| ((g[3] & 0xFF).trailing_zeros(), g[0] as i32, g[1] as i32)).collect();
                    println!("slot goals tick {}: object {} at {target:?} facing {}: native places {places:?} (offered {offered:?}); the request's {theirs:?}", pl.tick, o.id(), o.facing());
                    println!("   the step: sim {} tree {} number {number} scope {scope} stack object {} raw {raw:?}; the plan's planner {:?}", ids.get(&s.sim).copied().unwrap_or(-1), s.tree, s.stack_object, pl.approach.map(|a| a[7]));
                    println!("   object 60's places: {:?}", log.places.iter().filter(|p| p.id == 60).map(|p| (p.tick, p.x, p.y)).collect::<Vec<_>>());
                    for id in [12i16, 197, 198, 202, 237] {
                        println!("   person {id} at tick {}: {:?}", pl.tick, log.places.iter().rfind(|p| p.id == id && p.tick <= pl.tick).map(|p| (p.x, p.y, p.tick)));
                    }
                    // which slot's area holds the request's goals (the object's table and the global one)
                    let tables = [world.definition(o).and_then(|d| lib.file(file).slots.get(&d.slot_table())), lib.global_file().and_then(|g| g.slots.values().next())];
                    for (t, table) in tables.iter().enumerate() {
                        for (n, sl) in table.iter().flat_map(|t| t.iter()).filter(|s| s.kind == 3).enumerate() {
                            let area = slot_area(sl, target.0, target.1, o.facing());
                            let inside = pl.goals.iter().filter(|g| area.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count();
                            if inside > 0 {
                                println!("   table {t} slot {n}: {inside} of {} goals in its area of {} ({sl:?})", pl.goals.len(), area.len());
                            }
                            // a range slot: the centre and facing whose area holds the most goals
                            if inside > 1 && sl.min_proximity != sl.max_proximity {
                                let mut best = (0, (0, 0), 0);
                                for dx in (-64..=64).step_by(8) {
                                    for dy in (-64..=64).step_by(8) {
                                        for f in 0..8 {
                                            let a = slot_area(sl, target.0 + dx, target.1 + dy, f);
                                            let k = pl.goals.iter().filter(|g| a.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count();
                                            if k > best.0 {
                                                best = (k, (target.0 + dx, target.1 + dy), f);
                                            }
                                        }
                                    }
                                }
                                println!("   table {t} slot {n}: best {} of {} goals with the area round {:?} facing {}", best.0, pl.goals.len(), best.1, best.2);
                                let at_target: Vec<usize> = (0..8).map(|f| {
                                    let a = slot_area(sl, target.0, target.1, f);
                                    pl.goals.iter().filter(|g| a.iter().any(|&(_, q)| q == (g[0] as i32, g[1] as i32))).count()
                                }).collect();
                                println!("   table {t} slot {n}: round the target {target:?}, goals in the area by facing 0..7: {at_target:?}");
                            }
                        }
                    }
                }
            } else {
                // a slot of a range: each goal one of its area's places, with its direction's bit
                // (by the object's native facing, else by the facing the goals' bits give: the
                // paper girl's save facing is stale)
                let fits = |f: i16| {
                    let area = slot_area(slot, target.0, target.1, f);
                    pl.goals.iter().filter(|g| area.contains(&((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32)))).count()
                };
                let native = fits(o.facing());
                let (facing, n) = if native == pl.goals.len() { (o.facing(), native) } else { (0..8).map(|f| (f, fits(f))).max_by_key(|x| x.1).unwrap() };
                for i in 0..pl.goals.len() {
                    counts.note(match (i < n, facing == o.facing()) {
                        (true, true) => "routing: a goal of a range slot natively one of its area's places (its bit's grid)",
                        (true, false) => "routing: a goal of a range slot one of its area's places (its bit's grid; the facing the bits give)",
                        _ => "routing: a goal of a range slot NOT one of its area's places",
                    });
                }
                // the open places among them, which the original draws its goals from at random:
                // those in the object's room, strictly inside no blocking obstacle of the plan (as
                // logged; an obstacle's edge is open) and on or in no wall's rectangle
                let on_or_in = |p: (i32, i32), r: &[i32]| p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3];
                let strictly_in = |p: (i32, i32), r: &[i32]| p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3];
                let wall_rects = walls.wall_rects(3);
                let open: Vec<(u8, (i32, i32))> = slot_area(slot, target.0, target.1, facing)
                    .into_iter()
                    .filter(|&(_, p)| room(p) == room(target))
                    .filter(|&(_, p)| !pl.obstacles.iter().any(|r| r[4] == BLOCKS && strictly_in(p, &r[..4])) && !wall_rects.iter().any(|r| on_or_in(p, r)))
                    .collect();

                for g in &pl.goals {
                    let goal = ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32));
                    if open.contains(&goal) {
                        score(counts, g);
                    }
                    counts.note(if open.contains(&goal) {
                        "routing: a goal of a range slot among its area's open places (its room, clear of the obstacles and walls)"
                    } else {
                        "routing: a goal of a range slot NOT among its area's open places"
                    });
                }
                // the scores by the builder's draws (logs with 'R' site 2): each place in the area's
                // order takes the next draw, a direction perhaps stopping short (its classifier,
                // fn_80248F4C, not yet read: the one short direction's count taken from the draws'
                // total); each goal's score that of its place
                let draws: Vec<u32> = log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 2).map(|r| r.value).collect();
                if slot.flags & 0x2000 != 0 && !draws.is_empty() {
                    let area = slot_area(slot, target.0, target.1, facing);
                    let per: Vec<usize> = (0..8u8).map(|k| area.iter().filter(|x| x.0 == k).count()).collect();
                    let short = draws.len() as i64 - per.iter().sum::<usize>() as i64;
                    // which direction stops short: the one whose shortening fits the goals best
                    let score_with = |sk: usize, sn: usize| {
                        let mut it = draws.iter();
                        // (direction, place, score)
                        let mut scored: Vec<(u8, (i32, i32), u32)> = Vec::new();
                        for (k, &count) in per.iter().enumerate() {
                            let n = if k == sk { sn } else { count };
                            for &(d, place) in area.iter().filter(|x| x.0 as usize == k).take(n) {
                                if let Some(&r) = it.next() {
                                    scored.push((d, place, 135 + r % 20 - 10));
                                }
                            }
                        }
                        pl.goals
                            .iter()
                            .filter(|g| scored.iter().any(|&(d, p, sc)| (d, p) == ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32)) && sc == g[2]))
                            .count()
                    };
                    let best = (0..8)
                        .filter(|&k| short <= 0 && per[k] as i64 + short >= 0)
                        .map(|k| (score_with(k, (per[k] as i64 + short) as usize), k))
                        .max();
                    if let Some((hits, sk)) = best {
                        // the pick (logs with 'R' site 3): the goals, in order, the places the draws
                        // take (`slot_pick`, the cap the goals' count) among the open ones the
                        // builder reached
                        let picks: Vec<u32> = log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 3).map(|r| r.value).collect();
                        let reached: Vec<(u8, (i32, i32))> = (0..8usize)
                            .flat_map(|k| {
                                let n = if k == sk { (per[k] as i64 + short) as usize } else { per[k] };
                                area.iter().filter(move |x| x.0 as usize == k).take(n).copied()
                            })
                            .filter(|x| open.contains(x))
                            .collect();
                        if !picks.is_empty() {
                            let taken = slot_pick(reached.len(), &picks, pl.goals.len());
                            for (i, g) in pl.goals.iter().enumerate() {
                                let goal = ((g[3] & 0xFF).trailing_zeros() as u8, (g[0] as i32, g[1] as i32));
                                counts.note(if taken.get(i).map(|&j| reached[j]) == Some(goal) {
                                    "routing: a range slot's goal natively the place the pick's draw takes, in order"
                                } else {
                                    "routing: a range slot's goal NOT the place the pick's draw takes"
                                });
                            }
                        }
                        if has("REPLAY_SHOW_GOALS") {
                            println!("area draws at tick {}: {} draws, direction {sk} short by {}", pl.tick, draws.len(), -short);
                        }
                        // study (REPLAY_DUMP_AREA): the scored places in order, open or not
                        if has("REPLAY_DUMP_AREA") {
                            let mut it = draws.iter();
                            for (k, &count) in per.iter().enumerate() {
                                let n = if k == sk { (count as i64 + short) as usize } else { count };
                                for &(d, p) in area.iter().filter(|x| x.0 as usize == k).take(n) {
                                    if let Some(&r) = it.next() {
                                        println!("scored\t{d}\t{}\t{}\t{}\t{}", p.0, p.1, 135 + r % 20 - 10, open.contains(&(d, p)) as u8);
                                    }
                                }
                            }
                            for r in log.randoms.iter().filter(|r| r.tick == pl.tick && r.site == 3) {
                                println!("pick\t{}", r.value);
                            }
                            for g in &pl.goals {
                                println!("goal\t{}\t{}\t{}\t{}", (g[3] & 0xFF).trailing_zeros(), g[0], g[1], g[2]);
                            }
                        }
                        for i in 0..pl.goals.len() {
                            counts.note(if i < hits {
                                "routing: a range slot goal's score its place's draw, in the builder's order (one direction's count from the draws)"
                            } else {
                                "routing: a range slot goal's score NOT its place's draw in the builder's order"
                            });
                        }
                    }
                }
                if has("REPLAY_SHOW_GOALS") {
                    println!("area at tick {}: facing {facing} (native {}), {n} of {} goals among {} places, {} open; slot {slot:?}", pl.tick, o.facing(), pl.goals.len(), slot_area(slot, target.0, target.1, facing).len(), open.len());
                }
            }
        }
        let d2 = |p: &(i32, i32)| (p.0 - start.0).pow(2) + (p.1 - start.1).pow(2);
        // a slot of one proximity: the nearest open place; one of a range (its goals the request's,
        // checked above): the one the backward search takes of them
        let got = if slot.min_proximity != slot.max_proximity && pl.goals.len() > 1 && !pl.lookup_regions[0].is_empty() {
            let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
            let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
            let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
            choose_goal(&rects, growth, (start.1, start.0), &goals).map(|i| (goals[i].1, goals[i].0))
        } else {
            places.iter().map(|x| x.1).min_by_key(d2)
        };
        counts.note(if got == Some(want) {
            "routing: a routing step's goal natively the plan's (the nearest open place of its slot)"
        } else {
            "routing: a routing step's goal natively NOT the plan's"
        });
        if let Some(g) = usize::try_from(pl.chosen).ok().and_then(|i| pl.goals.get(i)) {
            counts.note(if got == Some((g[0] as i32, g[1] as i32)) {
                "routing: a routing step's goal natively the one the planner took"
            } else {
                "routing: a routing step's goal natively NOT the one the planner took"
            });
        }
        if has("REPLAY_SHOW_GOALS") {
            for &(k, p) in &offered {
                let on: Vec<String> = world
                    .objects()
                    .filter(|x| tile((x.x16, x.y16)) == tile(p))
                    .map(|x| format!("{} {} level {} person {} lot {}", x.id(), x.file.map_or("-".to_string(), |f| file_name(lib, f)), x.level, x.person, x.on_lot()))
                    .collect();
                println!("   place {k} {p:?} tile {:?} room {}: {on:?}", tile(p), walls.room(tile(p).0, tile(p).1));
            }
            let logged: Vec<(u32, (i32, i32))> = pl.goals.iter().map(|g| (g[3] & 0xFF, (g[0] as i32, g[1] as i32))).collect();
            println!(
                "goals at tick {}: slot {number} ({scope}) flags {:x} proximity {}..{} of {} facing {}: offered {offered:?}, open {places:?}, logged {logged:?} took {}; native {got:?}, the plan's {want:?}",
                s.tick,
                slot.flags,
                slot.min_proximity,
                slot.max_proximity,
                o.id(),
                o.facing(),
                pl.chosen
            );
            if let Some(t) = world.definition(o).and_then(|d| lib.file(file).slots.get(&d.slot_table())) {
                for (i, sl) in t.iter().enumerate() {
                    println!("   table slot {i}: {sl:?}");
                }
            }
            println!("   slot fields {slot:?}; scores {:?}; last words {:x?}", pl.goals.iter().map(|g| g[2]).collect::<Vec<_>>(), pl.goals.iter().map(|g| g[3]).collect::<Vec<_>>());
        }
    }
    // The obstacles the planner gathered for each plan (logs with 'X'): their shapes (a cost of
    // 0x7FFFFFFF grown by half the width, a cost of 10 not), and how they meet the native
    // objects (study: REPLAY_SHOW_OBSTACLES, each with the objects whose place lies in it).
    // a request of several goals: the goal it takes natively (the backward search)
    for pl in log.plans.iter().filter(|p| p.goals.len() > 1 && p.coarse.len() >= 2 && !p.lookup_regions[0].is_empty()) {
        let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
        let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
        let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
        let (sx, sy) = pl.coarse[0];
        let ours = choose_goal(&rects, growth, (sy, sx), &goals);
        counts.note(if ours.map(|i| i as i32) == Some(pl.chosen) {
            "planner: a request of several goals natively takes the planner's (the backward search)"
        } else {
            "planner: a request of several goals natively NOT the planner's"
        });
        if ours.map(|i| i as i32) != Some(pl.chosen) && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
            println!("goal choice tick {}: native {ours:?}, the planner's {} of {}", pl.tick, pl.chosen, goals.len());
        }
    }
    for pl in log.plans.iter().filter(|p| p.approach.is_some()) {
        if let Some(p) = native_plan_points(pl, world) {
            counts.note(if p == pl.points {
                "planner: a plan natively, search to its point after the goal, all its points the original's"
            } else {
                "planner: a plan natively NOT all its points the original's"
            });
        }
    }
    // The planner's search natively (logs with 'r'): on the rects and settings the plan's last
    // search had, from its coarse way's start to its end, the coarse way point by point.
    for pl in log.plans.iter().filter(|p| p.coarse.len() >= 2 && !p.lookup_regions[0].is_empty()) {
        let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
        let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
        let turned = |&(x, y): &(i32, i32)| (y, x);
        let theirs: Vec<(i32, i32)> = pl.coarse.iter().map(turned).collect();
        let way = coarse_way(&rects, growth, theirs[0], theirs[theirs.len() - 1]);
        let ours = way.as_ref().map(|w| w.points.clone());
        let same = ours.as_ref() == Some(&theirs);
        // and the plan's points ('G', in (x, y)) from it: the refined way, then the one the
        // plan adds after the goal
        if let Some(w) = &way {
            let refined: Vec<(i32, i32)> = refine(w).iter().map(|&(y, x)| (x, y)).collect();
            let fits = pl.points.len() >= refined.len() && pl.points[..refined.len()] == refined[..];
            counts.note(if fits {
                "planner: a plan's points ('G') natively the refined way, point by point (but the one after the goal)"
            } else {
                "planner: a plan's points ('G') natively NOT the refined way"
            });
            if !fits && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
                println!("refine tick {}: theirs {:?}; native {refined:?}", pl.tick, pl.points);
            }
            // the point the plan adds after its goal (logs with 'h')
            if let (true, Some(a)) = (fits, pl.approach) {
                let facing = world.find_object(a[5] as i16).and_then(|o| o.data.get(1)).map(|&d| d as i32);
                // a plan that took no goal (the request's +88 left at -1) adds none
                let ours = if pl.chosen < 0 { None } else { point_after_goal(a[0], a[1], (a[3], a[4]), facing, a[2], &refined) };
                let theirs = pl.points.get(refined.len()).copied();
                counts.note(if ours == theirs {
                    "planner: a plan's point after its goal (or none) natively its own"
                } else {
                    "planner: a plan's point after its goal (or none) natively NOT its own"
                });
                if ours != theirs && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
                    println!("after goal tick {}: kind {} facer {} facing {facing:?} theirs {theirs:?}; native {ours:?}", pl.tick, a[0], a[5]);
                }
            }
        }
        counts.note(if same {
            "planner: a plan's search ('I') natively its coarse way, point by point"
        } else {
            "planner: a plan's search ('I') natively NOT its coarse way"
        });
        if !same && !counts.quiet && has("REPLAY_SHOW_SEARCH") {
            println!("search tick {}: theirs {theirs:?}; native {ours:?}", pl.tick);
            if let Some(&(sx, sy)) = pl.coarse.first() {
                // the regions (y0, x0, y1, x1, cost) that hold the start, in each set
                for (set, rs) in [("regions", &pl.regions), ("lookup 0", &pl.lookup_regions[0]), ("lookup 1", &pl.lookup_regions[1])] {
                    for (k, r) in rs.iter().enumerate().filter(|(_, r)| r[0] <= sy && sy < r[2] && r[1] <= sx && sx < r[3]) {
                        println!("   start ({sy}, {sx}) in {set} {k} of {}: {r:?}", rs.len());
                    }
                }
            }
        }
    }
    if has("REPLAY_SHOW_REGIONS") {
        println!("native closed edges: {:?}", walls.wall_rects(0));
        println!("native rooms: {}", walls.rooms());
        for y in 10..32 {
            println!("row {y:2}: {}", (8..30).map(|x| format!("{}{}{}", walls.room(x, y), if walls.indoors(x, y) { "i" } else { "." }, if walls.diagonal(x, y) { "d" } else { " " })).collect::<Vec<_>>().join(" "));
        }
        for pl in log.plans.iter().filter(|p| !p.lookup_regions[0].is_empty() || !p.regions.is_empty()) {
            println!("   points {:?} goals {:?} coarse {:?}", pl.points, pl.goals.iter().map(|g| (g[0], g[1], g[2])).collect::<Vec<_>>(), pl.coarse);
            println!("plan tick {} start {:?} goals {} chosen {}: D {} regions, lookup0 {} lookup1 {}", pl.tick, pl.points.first(), pl.goals.len(), pl.chosen, pl.regions.len(), pl.lookup_regions[0].len(), pl.lookup_regions[1].len());
            for r in &pl.regions { println!("   D {r:?}"); }
            for r in &pl.lookup_regions[1] { println!("   L1 {r:?}"); }
            for r in &pl.obstacles { println!("   X {r:?}"); }
            for r in &pl.lookup_regions[0] { println!("   L0 {r:?}"); }
        }
    }
    let show_obstacles = has("REPLAY_SHOW_OBSTACLES");
    for pl in log.plans.iter().filter(|p| !p.obstacles.is_empty()) {
        if show_obstacles {
            println!("obstacles tick {} width {} ({} rects)", pl.tick, pl.obstacle_width, pl.obstacles.len());
        }
        let half = pl.obstacle_width / 2;
        let inside = |o: &crate::world::Object, r: &[i32; 5]| o.x16 >= r[0] && o.x16 < r[2] && o.y16 >= r[1] && o.y16 < r[3];
        let solid = |o: &&crate::world::Object| o.on_lot() && o.level == 1 && !o.person;
        for r in &pl.obstacles {
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            let grown = r[4] == i32::MAX;
            let core = if grown { (w - 2 * half, h - 2 * half) } else { (w, h) };
            counts.note(if core.0 > 0 && core.1 > 0 && core.0 <= 16 && core.1 <= 16 {
                "routing: an obstacle a part of a tile (blocking: grown by half the width)"
            } else {
                "routing: an obstacle NOT a part of a tile grown as its cost says"
            });
            let objs: Vec<String> = world
                .objects()
                .filter(solid)
                .filter(|o| inside(o, r))
                .map(|o| format!("{} {} f{}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.facing()))
                .collect();
            if show_obstacles {
                println!("   {:?} {w}x{h} cost {}: {objs:?}", &r[..4], r[4]);
            }
        }
        // Each obstacle's object (logs with 'J'): the objects the loop kept, in order, one a
        // rectangle (one may be dropped, as overlapping the Sim: the pairing that best fits).
        let kept: Vec<i16> = pl.tested.iter().filter(|t| !t.1).map(|t| t.0).collect();
        if !kept.is_empty() {
            let core = |r: &[i32; 5]| if r[4] == i32::MAX { [r[0] + half, r[1] + half, r[2] - half, r[3] - half] } else { [r[0], r[1], r[2], r[3]] };
            // an object's place: a person's as logged at the plan's tick (the walk is not yet
            // native), any other's natively
            let place = |id: i16| world.find_object(id).map(|o| if o.person { place_at(id, pl.tick).unwrap_or((o.x16, o.y16)) } else { (o.x16, o.y16) });
            let on_tile = |id: i16, r: &[i32; 5]| {
                let c = core(r);
                place(id).is_some_and(|p| {
                    let person = world.find_object(id).is_some_and(|o| o.person);
                    // a person's core is 6 on a side, centred on its place; another's within its tile
                    if person { c == [p.0 - 3, p.1 - 3, p.0 + 3, p.1 + 3] } else { p.0 >> 4 == c[0] >> 4 && p.1 >> 4 == c[1] >> 4 }
                })
            };
            let skips: Vec<Option<usize>> = if kept.len() == pl.obstacles.len() + 1 { (0..kept.len()).map(Some).collect() } else { vec![None] };
            let pairing = |skip: Option<usize>| -> Vec<i16> { kept.iter().enumerate().filter(|&(i, _)| Some(i) != skip).map(|(_, &id)| id).collect() };
            let best = skips
                .iter()
                .map(|&sk| pairing(sk))
                .max_by_key(|ids| ids.iter().zip(&pl.obstacles).filter(|(id, r)| on_tile(**id, r)).count())
                .unwrap_or_default();
            counts.add("routing: obstacles paired with the objects the loop kept (as many, or one dropped)", (best.len() == pl.obstacles.len()) as usize);
            for (&id, r) in best.iter().zip(&pl.obstacles) {
                let c = core(r);
                // its footprint natively (not a person's): the tile less the definition's mask
                if let Some(o) = world.find_object(id).filter(|o| !o.person) {
                    let mask = world.definition(o).map_or(0, |d| d.field(75));
                    let width = world.definition(o).map_or(0, |d| d.field(65));
                    if show_obstacles && footprint(mask, width, o.x16, o.y16, o.facing()) != c {
                        println!("   footprint miss: object {id} mask {mask:04x} facing {}: native {:?}, the log's {c:?}", o.facing(), footprint(mask, width, o.x16, o.y16, o.facing()));
                    }
                    counts.note(if footprint(mask, width, o.x16, o.y16, o.facing()) == c {
                        "routing: an obstacle's footprint natively its object's (the tile less its OBJD mask and width)"
                    } else {
                        "routing: an obstacle's footprint natively NOT its object's"
                    });
                }
                let what = match world.find_object(id) {
                    None => "routing: an obstacle's object not in the native world",
                    Some(_) if on_tile(id, r) => "routing: an obstacle's object natively on its tile (the footprint within it)",
                    Some(_) => "routing: an obstacle's object natively NOT on its tile",
                };
                counts.note(what);
                if show_obstacles {
                    let o = world.find_object(id);
                    println!(
                        "   obstacle {:?} cost {} core {}x{} at +{},+{} of tile {},{}: object {id} {} native at {:?} defn version {:?} f75 {:04x}",
                        &r[..4],
                        r[4],
                        c[2] - c[0],
                        c[3] - c[1],
                        c[0] & 15,
                        c[1] & 15,
                        c[0] >> 4,
                        c[1] >> 4,
                        o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                        o.map(|o| (o.x16, o.y16, o.level, o.facing())),
                        o.and_then(|o| world.definition(o)).map(|d| (d.version, d.fields.len())),
                        o.and_then(|o| world.definition(o)).map_or(0, |d| d.field(75))
                    );
                }
            }
        }
        // The obstacles natively, in the loop's order (the objects it asked about, as logged):
        // each not a person by `routing::obstacle`, a person as the log keeps it (6 on a side
        // round its logged place, grown); against the logged list where it has as many (not
        // dropping one that overlaps the Sim: not yet native), rectangle by rectangle.
        if !kept.is_empty() {
            // the Sim that plans: the person nearest the plan's start (its place as logged, within
            // a tile); its footprint 6 on a side round its place
            let start = pl.points.first().copied();
            let sim_rect = start.and_then(|s| {
                world
                    .objects()
                    .filter(|o| o.person)
                    .filter_map(|o| place_at(o.id(), pl.tick))
                    .map(|p| ((p.0 - s.0).pow(2) + (p.1 - s.1).pow(2), p))
                    .filter(|&(d, _)| d <= 256)
                    .min()
                    .map(|(_, p)| [p.0 - 3, p.1 - 3, p.0 + 3, p.1 + 3])
            });
            let native: Vec<[i32; 5]> = pl
                .tested
                .iter()
                .filter_map(|&(id, out, _)| {
                    // an object not in the native world holds its place (as the log keeps it)
                    let Some(o) = world.find_object(id) else { return (!out).then_some([0; 5]) };
                    if o.person {
                        // its place as the planner sees it, mid-tick: the one logged before the
                        // plan's tick ('P' is written as a tick starts; the club's tick 56)
                        let p = log.places.iter().rfind(|q| q.id == id && q.tick < pl.tick).map(|q| (q.x, q.y)).unwrap_or((o.x16, o.y16));
                        return (!out).then_some([p.0 - 3 - half, p.1 - 3 - half, p.0 + 3 + half, p.1 + 3 + half, BLOCKS]);
                    }
                    // an object replay's world holds without its definition (one the engine made,
                    // taken from the log's list: the club's smoke-puff effect) has no footprint
                    // here: none, as the original's loop left it out
                    world.definition(o)?;
                    obstacle(o, world.definition(o).map_or(0, |d| d.field(75)), world.definition(o).map_or(0, |d| d.field(65)), half)
                })
                // one whose footprint (before it is grown) overlaps the Sim's is dropped
                .filter(|r| *r == [0; 5] || !sim_rect.is_some_and(|s| overlaps(&ungrown(r, half), &s)))
                .collect();
            // an object not in the native world ([0; 5]) holds its place, unless the planner made
            // no rectangle for it (the next native one is the planner's here): then it is dropped
            let native: Vec<[i32; 5]> = {
                let (mut kept_rects, mut j) = (Vec::new(), 0);
                for (i, r) in native.iter().enumerate() {
                    if *r == [0; 5] && native.len() > pl.obstacles.len() && native.get(i + 1).is_some_and(|n| pl.obstacles.get(j) == Some(n)) {
                        continue;
                    }
                    kept_rects.push(*r);
                    j += 1;
                }
                kept_rects
            };
            let hit = native.iter().zip(&pl.obstacles).filter(|(a, b)| a == b).count();
            let unknown = native.iter().filter(|a| **a == [0; 5]).count();
            let miss = native.len().min(pl.obstacles.len()) - hit - unknown;
            if show_obstacles {
                for (k, (a, b)) in native.iter().zip(&pl.obstacles).enumerate().filter(|(_, (a, b))| a != b && **a != [0; 5]) {
                    println!("      rectangle {k} of tick {}: native {a:?}, the planner's {b:?}, object {:?}", pl.tick, pl.tested.iter().filter(|t| !t.1).nth(k).map(|t| t.0));
                }
                for (a, b) in native.iter().zip(&pl.obstacles).filter(|(a, b)| a != b && **a != [0; 5]) {
                    println!("   native obstacle {a:?}, the planner's {b:?}");
                }
            }
            // A person among the obstacles natively: left out when it is what the plan goes to
            // (the stack object of the tick's routing step that walks: opcode 27 or 45, result
            // 1) or more than 3 tiles from the planning Sim (the constant at 802CFAAC, 3.0; the
            // logged places, the walk not native); against the loop's answers.
            let targets: Vec<i16> = log
                .steps
                .iter()
                .filter(|s| s.tick == pl.tick && s.result == 1)
                .filter(|s| {
                    log.tree_of(s).is_some_and(|t| {
                        let at = s.node as usize * 12;
                        t.tree == s.tree && at + 12 <= t.raw.len() && matches!(u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, 27 | 45)
                    })
                })
                .map(|s| s.stack_object)
                .collect();
            if let Some(sr) = sim_rect {
                let c = (sr[0] + 3, sr[1] + 3);
                // only a person's own part (104 bytes before its listed object, in the log): an
                // entry the observer files under a person by nearness alone (the paper girl's
                // other object, 356 past hers, another class) is no person
                for &(id, out, _) in pl.tested.iter().filter(|t| t.2 == PERSON_PART_OFFSET) {
                    let Some(p) = world.find_object(id).filter(|o| o.person).and_then(|_| place_at(id, pl.tick)) else { continue };
                    let d2 = (p.0 - c.0).pow(2) + (p.1 - c.1).pow(2);
                    let native_out = targets.contains(&id) || d2 >= 48 * 48;
                    counts.note(if native_out == out {
                        "routing: a person natively kept or left out as the loop did (its target, or 3 tiles off)"
                    } else {
                        "routing: a person natively NOT kept or left out as the loop did"
                    });
                    if show_obstacles && native_out != out {
                        println!("   person {id} at {p:?}, {:.1} from the Sim: the loop {}, natively {}; targets {targets:?}", (d2 as f64).sqrt(), if out { "left out" } else { "kept" }, if native_out { "left out" } else { "kept" });
                    }
                }
            }
            // study: each person the loop asked about, its distance from the planning Sim and
            // whether it was kept
            if show_obstacles {
                for &(id, out, _) in pl.tested.iter().filter(|t| t.2 == PERSON_PART_OFFSET) {
                    if let Some(o) = world.find_object(id).filter(|o| o.person)
                        && let (Some(p), Some(s)) = (place_at(id, pl.tick), sim_rect)
                    {
                        let c = (s[0] + 3, s[1] + 3);
                        let d = (((p.0 - c.0).pow(2) + (p.1 - c.1).pow(2)) as f64).sqrt();
                        println!("   person {id} ({}) at {p:?}, {d:.1} from the Sim at {c:?} (start {:?}, goal {:?}): {}", o.id(), pl.points.first(), pl.points.last(), if out { "left out" } else { "kept" });
                    }
                }
            }
            if show_obstacles {
                println!("   native list tick {}: {} rects, the planner's {}, kept {}, the Sim's {sim_rect:?}, {hit} the same", pl.tick, native.len(), pl.obstacles.len(), kept.len());
                if native.len() != pl.obstacles.len() {
                    let ids: Vec<String> = pl
                        .tested
                        .iter()
                        .map(|&(id, out, off)| {
                            let o = world.find_object(id);
                            let nat = o.map(|o| {
                                if o.person { "P".to_string() } else { format!("f{:x}c{}", o.data.get(8).copied().unwrap_or(0), o.data.get(2).copied().unwrap_or(0)) }
                            });
                            format!("{id}{}{}{}", if out { "-" } else { "+" }, if off == PERSON_PART_OFFSET { "p" } else { "" }, nat.unwrap_or("?".into()))
                        })
                        .collect();
                    println!("      tested (id, + kept / - left out, native flags and container): {}", ids.join(" "));
                }
                let only_native: Vec<_> = native.iter().filter(|r| !pl.obstacles.contains(r)).collect();
                let only_theirs: Vec<_> = pl.obstacles.iter().filter(|r| !native.contains(r)).collect();
                if !only_native.is_empty() || !only_theirs.is_empty() {
                    println!("   tick {}: only natively {only_native:?}; only the planner's {only_theirs:?}", pl.tick);
                }
            }
            counts.add("routing: obstacles of objects not in the native world (the log's kept)", unknown);
            // The plan's waypoints between its ends, by what they are: a corner of one of the
            // native obstacles (as the planner has them where natively unknown), or not
            let corners: Vec<(i32, i32)> = native
                .iter()
                .zip(&pl.obstacles)
                .map(|(a, b)| if *a == [0; 5] { b } else { a })
                .flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])])
                .collect();
            // The plan within one room natively (`corner_path`) against the planner's: the corners
            // of the obstacles it passes, in order (its other points apart), to the place it
            // walks to (before the object it then faces).
            let obstacles_here: Vec<[i32; 5]> = native.iter().zip(&pl.obstacles).map(|(a, b)| if *a == [0; 5] { *b } else { *a }).collect();
            if pl.points.len() >= 2 && native.len() == pl.obstacles.len() {
                let start = pl.points[0];
                let end = *pl.points.last().unwrap();
                let in_blocking = |p: (i32, i32)| obstacles_here.iter().any(|r| r[4] == BLOCKS && p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3]);
                let goal = if in_blocking(end) && pl.points.len() >= 3 { pl.points[pl.points.len() - 2] } else { end };
                let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
                let same_room = room(start) == room(goal);
                {
                    // weighed by the paved regions natively from the floor (`OFF_PAVED_WEIGHT`, a
                    // fitted weight; study HF_PAVED_WEIGHT to try another)
                    let weight: f64 = env("HF_PAVED_WEIGHT").and_then(|v| v.parse().ok()).unwrap_or(OFF_PAVED_WEIGHT);
                    let path = weighed_path(start, goal, &obstacles_here, walls, half, &walls.paved_regions(), weight);
                    let is_corner = |p: &(i32, i32)| obstacles_here.iter().filter(|r| r[4] == BLOCKS).any(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])].contains(p));
                    let theirs: Vec<(i32, i32)> = pl.points.iter().copied().filter(is_corner).collect();
                    let ours: Vec<(i32, i32)> = path.as_ref().map_or(Vec::new(), |p| p.iter().copied().filter(is_corner).collect());
                    counts.note(match (same_room, path.is_some() && ours == theirs) {
                        (true, true) => "routing: a plan within a room natively by the obstacles' corners as the planner's",
                        (true, false) => "routing: a plan within a room natively NOT by the planner's corners",
                        (false, true) => "routing: a plan through a door natively by the obstacles' corners as the planner's",
                        (false, false) => "routing: a plan through a door natively NOT by the planner's corners",
                    });
                    if show_obstacles {
                        println!("   corner path tick {}: native {path:?}; the planner's corners {theirs:?}, points {:?}", pl.tick, pl.points);
                    }
                    // against the planner's own coarse way through its map (logs with 'I')
                    if !pl.coarse.is_empty() {
                        let same = path.as_ref().is_some_and(|p| *p == pl.coarse);
                        counts.note(if same {
                            "routing: a plan's coarse way (the search's, 'I') natively the corner path, point by point"
                        } else {
                            "routing: a plan's coarse way (the search's, 'I') NOT the native corner path"
                        });
                        // study: each coarse point moved to the nearest corner of a logged
                        // obstacle (any cost) within 12, against the plan's points
                        if has("REPLAY_SHOW_COARSE") && !counts.quiet {
                            let corners: Vec<(i32, i32)> = pl.obstacles.iter().flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])]).collect();
                            let snapped: Vec<String> = pl.coarse.iter().map(|&p| {
                                let near = corners.iter().min_by_key(|c| (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2)).copied();
                                match near {
                                    Some(c) if (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2) <= 144 => format!("{p:?}->{c:?}{}", if pl.points.contains(&c) { "+" } else { "" }),
                                    _ => format!("{p:?}{}", if pl.points.contains(&p) { "+" } else { "" }),
                                }
                            }).collect();
                            println!("snap tick {}: {}", pl.tick, snapped.join(" "));
                            // and against the search's own rectangles (logs with 'D'): the nearest
                            // of their corners to each coarse point
                            let dc: Vec<(i32, i32)> = pl.regions.iter().flat_map(|r| [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])]).collect();
                            let near: Vec<String> = pl.coarse.iter().map(|&p| {
                                dc.iter().min_by_key(|c| (c.0 - p.0).pow(2) + (c.1 - p.1).pow(2)).map_or(format!("{p:?}"), |c| format!("{p:?}~{c:?}"))
                            }).collect();
                            println!("regions tick {} ({} rects): {}", pl.tick, pl.regions.len(), near.join(" "));
                        }
                        if has("REPLAY_SHOW_COARSE") && !counts.quiet {
                            println!("coarse tick {}: the search's {:?}; native {path:?}; the plan {:?}", pl.tick, pl.coarse, pl.points);
                        }
                    }
                }
            }
            // Each leg of the plan keeps out of the blocking obstacles' insides (natively)
            // (but the last into the object it ends at: the Sim turns to face it there)
            for (i, leg) in pl.points.windows(2).enumerate() {
                let (a, b) = (leg[0], leg[1]);
                let through = native.iter().filter(|r| r[4] == BLOCKS).find(|r| segment_enters(a, b, &[r[0], r[1], r[2], r[3]]));
                let last = i + 2 == pl.points.len();
                counts.note(match through {
                    None => "routing: a plan's leg clear of the native blocking obstacles",
                    Some(r) if last && b.0 > r[0] && b.0 < r[2] && b.1 > r[1] && b.1 < r[3] => {
                        "routing: a plan's last leg into the obstacle it ends in (the object faced)"
                    }
                    Some(_) => "routing: a plan's leg through a native blocking obstacle",
                });
                if show_obstacles && let Some(r) = through {
                    println!("   leg of tick {} {a:?} to {b:?} through {r:?}", pl.tick);
                }
            }
            let goals: Vec<(i32, i32)> = pl.goals.iter().map(|g| (g[0] as i32, g[1] as i32)).collect();
            for (i, w) in pl.points.iter().enumerate().skip(1).take(pl.points.len().saturating_sub(2)) {
                let corner = corners.contains(w);
                let (prev, next) = (pl.points[i - 1], pl.points[i + 1]);
                // on the line between its neighbours (to within a sixteenth)
                let cross = (w.0 - prev.0) as i64 * (next.1 - prev.1) as i64 - (w.1 - prev.1) as i64 * (next.0 - prev.0) as i64;
                let len = (((next.0 - prev.0).pow(2) + (next.1 - prev.1).pow(2)) as f64).sqrt();
                let between = len > 0.0 && (cross.abs() as f64) / len <= 1.0;
                let tile_centre = w.0 & 15 == 8 && w.1 & 15 == 8;
                counts.note(if corner {
                    "routing: a waypoint (not an end) a corner of an obstacle"
                } else if goals.contains(w) {
                    "routing: a waypoint (not an end) the request's goal"
                } else if between {
                    "routing: a waypoint (not an end) on the line between its neighbours"
                } else if tile_centre {
                    "routing: a waypoint (not an end) a tile's centre (a door's sides)"
                } else {
                    "routing: a waypoint (not an end) none of these"
                });
                let corner = corner || goals.contains(w) || between || tile_centre;
                if show_obstacles && !corner {
                    let prev = pl.points[i - 1];
                    let next = pl.points.get(i + 1).copied().unwrap_or(*w);
                    println!("   waypoint {i} of tick {} {w:?} (from {prev:?} to {next:?}): not a corner", pl.tick);
                }
            }
            counts.add("routing: obstacles natively the planner's, rectangle by rectangle", hit);
            counts.add("routing: obstacles natively NOT the planner's", miss + native.len().abs_diff(pl.obstacles.len()));
            counts.note(if native.len() == pl.obstacles.len() && hit + unknown == native.len() {
                "routing: a plan's obstacles natively all the planner's (but those of objects not native)"
            } else {
                "routing: a plan's obstacles natively NOT all the planner's (an object not native, or a footprint)"
            });
        }
        // study (REPLAY_DUMP_TESTED): each object the loop asked about, its label (left out, or
        // kept with its rectangle's cost) and its definition's fields, for the rules' search
        if has("REPLAY_DUMP_TESTED") && !kept.is_empty() && kept.len() == pl.obstacles.len() {
            let mut costs = std::collections::BTreeMap::new();
            let mut k = 0;
            for &(id, out, _) in &pl.tested {
                if out {
                    continue;
                }
                if let Some(r) = pl.obstacles.get(k) {
                    costs.entry(id).or_insert(r[4]);
                }
                k += 1;
            }
            for &(id, out, _) in &pl.tested {
                let Some(o) = world.find_object(id) else { continue };
                let label = if out { "out".to_string() } else { costs.get(&id).map_or("kept?".to_string(), |c| format!("cost{c}")) };
                let fields = world.definition(o).map(|d| d.fields.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default();
                let data = o.data.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                println!("tested\t{}\t{id}\t{}\t{label}\t{}\t{fields}\t{data}", pl.tick, o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.person as u8);
            }
        }
        // the native objects within the obstacles' extent that none covers
        let (x0, y0) = pl.obstacles.iter().fold((i32::MAX, i32::MAX), |a, r| (a.0.min(r[0]), a.1.min(r[1])));
        let (x1, y1) = pl.obstacles.iter().fold((i32::MIN, i32::MIN), |a, r| (a.0.max(r[2]), a.1.max(r[3])));
        for o in world.objects().filter(solid).filter(|o| o.x16 >= x0 && o.x16 < x1 && o.y16 >= y0 && o.y16 < y1) {
            let covered = pl.obstacles.iter().any(|r| inside(o, r));
            counts.note(if covered {
                "routing: a native object in the obstacles' extent covered by one"
            } else {
                "routing: a native object in the obstacles' extent covered by none"
            });
            if show_obstacles && !covered {
                println!("   none for {} {} at {},{}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.x16, o.y16);
            }
        }
    }
    // The walk (study: REPLAY_SHOW_WALKS): the planning Sim's logged places from its plan's tick
    // to its next plan (or 600 ticks), each against the plan's line: its distance from it and
    // how far along it (sixteenths).
    let show_walks = has("REPLAY_SHOW_WALKS") && !counts.quiet;
    for (k, pl) in log.plans.iter().enumerate() {
        let Some(&start) = pl.points.first() else { continue };
        let Some(sim) = world
            .objects()
            .filter(|o| o.person)
            .filter_map(|o| place_at(o.id(), pl.tick).map(|p| ((p.0 - start.0).pow(2) + (p.1 - start.1).pow(2), o.id())))
            .filter(|&(d, _)| d <= 256)
            .min()
            .map(|(_, id)| id)
        else {
            continue;
        };
        let until = log.plans[k + 1..].iter().find(|q| q.points.first().is_some_and(|&a| place_at(sim, q.tick) == Some(a))).map_or(pl.tick + 600, |q| q.tick);
        // the line's legs and their lengths
        let legs: Vec<((f64, f64), (f64, f64))> = pl.points.windows(2).map(|w| ((w[0].0 as f64, w[0].1 as f64), (w[1].0 as f64, w[1].1 as f64))).collect();
        let along = |p: (i32, i32)| -> (f64, f64) {
            // (distance from the line, length along it to the nearest point)
            let (mut best, mut at, mut before) = (f64::INFINITY, 0.0, 0.0);
            for &(a, b) in &legs {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = (dx * dx + dy * dy).sqrt();
                let t = if len == 0.0 { 0.0 } else { (((p.0 as f64 - a.0) * dx + (p.1 as f64 - a.1) * dy) / (len * len)).clamp(0.0, 1.0) };
                let q = (a.0 + t * dx, a.1 + t * dy);
                let d = ((p.0 as f64 - q.0).powi(2) + (p.1 as f64 - q.1).powi(2)).sqrt();
                if d < best {
                    best = d;
                    at = before + t * len;
                }
                before += len;
            }
            (best, at)
        };
        let mut last: Option<f64> = None;
        let mut line = String::new();
        for r in log.places.iter().filter(|r| r.id == sim && r.tick > pl.tick && r.tick <= until) {
            let (d, s) = along((r.x, r.y));
            counts.note(if d <= 1.5 {
                "routing: a walking Sim's place on its plan's line (within a sixteenth and a half)"
            } else {
                "routing: a walking Sim's place NOT on its plan's line"
            });
            if show_walks {
                line += &format!(" {}:{:.1}{}", r.tick, last.map_or(s, |l| s - l), if d > 1.5 { format!("(off {d:.1})") } else { String::new() });
            }
            last = Some(s);
        }
        if show_walks {
            println!("walk of {sim} on the plan of tick {} ({} points) to tick {until}:{line}", pl.tick, pl.points.len());
            // its speed: the length along the line over the ticks, from its first move to its
            // last (sixteenths a tick)
            let moves: Vec<(u32, f64)> = log.places.iter().filter(|r| r.id == sim && r.tick > pl.tick && r.tick <= until).map(|r| (r.tick, along((r.x, r.y)).1)).collect();
            if let (Some(a), Some(b)) = (moves.first(), moves.last())
                && b.0 > a.0
            {
                println!("   speed {:.3} a tick over {} ticks ({:.1} sixteenths), first move at tick {} ({} after the plan)", (b.1 - a.1) / (b.0 - a.0) as f64, b.0 - a.0, b.1 - a.1, a.0, a.0 - pl.tick);
                let mid: Vec<&(u32, f64)> = moves.iter().filter(|m| m.0 >= a.0 + 10 && m.0 + 10 <= b.0).collect();
                if let (Some(c), Some(d)) = (mid.first(), mid.last())
                    && d.0 > c.0 + 20
                {
                    println!("   cruising {:.4} a tick over {} ticks", (d.1 - c.1) / (d.0 - c.0) as f64, d.0 - c.0);
                }
            }
        }
    }
    // study (REPLAY_SHOW_PERSONS): the persons' data, for the child's
    if has("REPLAY_SHOW_PERSONS") {
        for o in world.objects().filter(|o| o.person) {
            println!("person {} {}: {:?}", o.id(), o.file.map_or("-".to_string(), |f| file_name(lib, f)), o.person_data);
        }
    }
    // The walker's steps natively (`walk_step`), each from its logged place along the line it
    // walks (the latest plan whose line it is on, less the last leg into a faced object when the
    // plan has one), against the logged place after it. Its top and growth from the DOL's
    // constants (`walk_rates`); its largest logged step only tells an adult from a child.
    let mut speed: BTreeMap<u32, f32> = BTreeMap::new();
    let mut tops: BTreeMap<u32, f32> = BTreeMap::new();
    // which person a follower walks: the one whose logged place at a tick it moves in is its own
    // (to a sixteenth); its top natively from its file's body strings (`walk_top`)
    let mut walker_of: BTreeMap<u32, i16> = BTreeMap::new();
    {
        let mut at = 0;
        for f in &log.follows {
            if walker_of.contains_key(&f.follower) || (f.before.x, f.before.y) == (f.after.x, f.after.y) {
                continue;
            }
            while at < log.places.len() && log.places[at].tick < f.tick {
                at += 1;
            }
            let here = log.places[at..].iter().take_while(|p| p.tick == f.tick);
            let found: Vec<i16> = here.filter(|p| ((p.x as f32) - f.after.x).abs() < 1.0 && ((p.y as f32) - f.after.y).abs() < 1.0).map(|p| p.id).collect();
            if let [id] = found[..] {
                walker_of.insert(f.follower, id);
            }
        }
    }
    let native_top: BTreeMap<u32, f32> = walker_of
        .iter()
        .filter_map(|(&fo, &id)| {
            let o = loaded.objects().find(|o| o.id() == id)?;
            let age = o.file.and_then(|fi| lib.file(fi).strings.get(&200)).and_then(|t| t.first()).map(String::as_str);
            Some((fo, walk_top(age)))
        })
        .collect();
    for f in log.follows.iter().filter(|f| f.before.factor == 1.0) {
        let d = ((f.after.x - f.before.x).powi(2) + (f.after.y - f.before.y).powi(2)).sqrt();
        let t = tops.entry(f.follower).or_insert(0.0);
        *t = t.max(d);
    }
    // checked: each walker's native top against the logged steps' (its largest step at normal
    // speed nearest the adult's or the child's)
    for (fo, &seen) in &tops {
        let logged = [WALK_TOP_ADULT, WALK_TOP_CHILD].into_iter().min_by(|a, b| (a * WALK_FRAME - seen).abs().total_cmp(&(b * WALK_FRAME - seen).abs())).unwrap();
        counts.note(match native_top.get(fo) {
            Some(&n) if n == logged => "routing: a walker's top natively the original's (its body's age, STR# 200)",
            Some(_) => "routing: a walker's top natively NOT the original's",
            None => "routing: a walker whose person is not found (its top not checked)",
        });
    }
    let mut plan_at = 0;
    // the walk run free: a native place a follower, set at its walk's start (state 1) and then
    // moved only by the native steps (with its own native speed), against the logged place after
    // each frame that moves
    let mut free: BTreeMap<u32, ((f32, f32), f32)> = BTreeMap::new();
    // the native speed each walk starts from (by follower and tick), for the whole walks below
    let mut start_speed: BTreeMap<(u32, u32), f32> = BTreeMap::new();
    // Natively, whether a walk runs on: the walker's object data 9 (Route_Follow's test, fn_801F5F4C)
    // as its scripts set it (an expression "my object data 9 := literal": scope 3, data 9, op 5,
    // its value from the node's operands) since its last routing primitive ended (GotoRoutingSlot
    // or GotoRelativePosition returning: the primitive clears it, Route_Run at 80220918).
    let mut data9: BTreeMap<i16, Vec<(u32, i16)>> = BTreeMap::new();
    for st in &log.steps {
        let Some(t) = log.tree_of(st) else { continue };
        let at = st.node as usize * 12;
        if at + 12 > t.raw.len() {
            continue;
        }
        let raw = &t.raw[at..at + 12];
        let op = u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF;
        let Some(&who) = ids.get(&st.sim) else { continue };
        let be = |i: usize| i16::from_be_bytes([raw[i], raw[i + 1]]);
        if op == 2 && be(4) == 9 && raw[9] == 5 && raw[10] == 3 && raw[11] == 7 {
            data9.entry(who).or_default().push((st.tick, be(6)));
        } else if matches!(op, 27 | 45) && st.result != 1 {
            data9.entry(who).or_default().push((st.tick, 0));
        }
    }
    let data9_at = |who: i16, tick: u32| data9.get(&who).and_then(|v| v.iter().rfind(|e| e.0 <= tick)).is_some_and(|e| e.1 != 0);
    // for each walker frame, whether its walk is followed by state 5 (the first frame from state
    // 4 after it going to 5): found backwards through the log
    let mut runs_on = vec![false; log.follows.len()];
    {
        let mut next4: BTreeMap<u32, u32> = BTreeMap::new();
        for (i, f) in log.follows.iter().enumerate().rev() {
            if f.before.state == 4 {
                next4.insert(f.follower, f.after.state);
            }
            runs_on[i] = next4.get(&f.follower) == Some(&5);
        }
    }
    // checked: at each walk's end (state 2 to 3), the native data 9 against the log's run on
    for (i, f) in log.follows.iter().enumerate().filter(|(_, f)| f.before.state == 2 && f.after.state == 3) {
        let Some(&who) = walker_of.get(&f.follower) else {
            counts.note("routing: a walk's end whose walker is not found (its run on not checked)");
            continue;
        };
        counts.note(if data9_at(who, f.tick) == runs_on[i] {
            "routing: a walk's run on natively the original's (its walker's object data 9 as its scripts set it)"
        } else {
            "routing: a walk's run on natively NOT the original's"
        });
    }
    // state 5's point natively (`run_on_point`), from the latest plan whose last waypoint but
    // one is where the walker enters it, against the follower's +116/+120 after
    for f in log.follows.iter().filter(|f| f.before.state == 4 && f.after.state == 5) {
        let at = (f.before.x, f.before.y);
        let plan = log.plans.iter().rev().filter(|p| p.tick <= f.tick && p.points.len() >= 2).find(|p| {
            let q = p.points[p.points.len() - 2];
            (q.0 as f32 - at.0).abs() < 0.5 && (q.1 as f32 - at.1).abs() < 0.5
        });
        let Some(p) = plan else {
            counts.note("routing: a walker's state 5 with no plan ending there (not checked)");
            continue;
        };
        let (q, l) = (p.points[p.points.len() - 2], p.points[p.points.len() - 1]);
        let nat = run_on_point((q.0 as f32, q.1 as f32), (l.0 as f32, l.1 as f32), f.before.mode == 1);
        let got = f.after.point;
        counts.note(if (nat.0 - got.0).abs() < 0.01 && (nat.1 - got.1).abs() < 0.01 {
            "routing: a walker's state-5 point natively the original's (towards the plan's last waypoint, 3 or 9 at most)"
        } else {
            "routing: a walker's state-5 point natively NOT the original's"
        });
        if show_walks {
            println!("state 5 at tick {} {:x}: at {at:?}, plan {:?}, native {nat:?}, the original's {got:?}, +132 {}", f.tick, f.follower, p.points, f.before.mode);
        }
    }
    for f in &log.follows {
        while plan_at < log.plans.len() && log.plans[plan_at].tick <= f.tick {
            plan_at += 1;
        }
        let (b, a) = (f.before, f.after);
        if b.state == 1 {
            // its speed the native one the follower's last walk left (step by step)
            let v = speed.get(&f.follower).copied().unwrap_or(0.0);
            free.insert(f.follower, ((b.x, b.y), v));
            start_speed.insert((f.follower, f.tick), v);
        }
        // (a walk starts at the speed the follower's last one stopped at: its +144 is not reset)
        if (b.x, b.y) == (a.x, a.y) {
            continue;
        }
        // a frame of state 5 (the move on to the follower's +116/+120 after a walk): natively the
        // walk's own step (`walk_step`) towards its point, braking for it unless its object data 9
        // is still set; its speed the one the next walk starts from
        if b.state == 5 || a.state == 5 {
            let step = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            let carried = speed.get(&f.follower).copied().unwrap_or(0.0);
            let left = ((a.point.0 - b.x).powi(2) + (a.point.1 - b.y).powi(2)).sqrt();
            let Some(&per_second) = native_top.get(&f.follower) else { continue };
            let (top, growth) = walk_rates(per_second);
            let runs_on = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
            let (v, s) = walk_step(carried, top, growth, b.factor, if runs_on { f32::INFINITY } else { left });
            let native = s.min(left);
            counts.note(if (native - step).abs() < 0.001 {
                "routing: a walker's state-5 step natively the original's (the walk's step to its point)"
            } else {
                "routing: a walker's state-5 step natively NOT the original's"
            });
            if show_walks && (native - step).abs() >= 0.001 {
                println!("state 5 step tick {} {:x}: native {native:.4} (carried {carried:.4}, left {left:.3}, runs on {runs_on}), the original's {step:.4}", f.tick, f.follower);
            }
            speed.insert(f.follower, v);
            continue;
        }
        // the walker's top a second: the DOL's adult or child constant (which: the one nearest
        // its logged full step; the original's test, through the follower's +8, not yet read)
        let Some(&per_second) = native_top.get(&f.follower) else { continue };
        let (top, growth) = walk_rates(per_second);
        // the line it is on
        let runs_on_now = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
        let found = log.plans[..plan_at].iter().rev().take(6).find_map(|pl| {
            let pts: Vec<(f32, f32)> = pl.points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
            // the walk ends at the request's chosen goal: a plan's last leg past it, into what the
            // Sim then faces in the same room, is not walked (one through a door is)
            let goal = usize::try_from(pl.chosen).ok().and_then(|i| pl.goals.get(i)).map(|g| (g[0] as i32, g[1] as i32));
            let room = |p: (i32, i32)| walls.room(p.0 >> 4, p.1 >> 4);
            let last = *pl.points.last().unwrap_or(&(0, 0));
            let faced = pl.points.len() >= 3 && goal.is_some_and(|g| pl.points[pl.points.len() - 2] == g && room(last) == room(g));
            // a walk that runs on (its walker's data 9 set) ends at the plan's last point but one
            // too: state 5 goes on from there towards the last
            let walk = if pts.len() >= 3 && (faced || pl.obstacles.is_empty() || runs_on_now) { &pts[..pts.len() - 1] } else { &pts[..] };
            let mut best: Option<(f32, usize, f32)> = None;
            for i in 0..walk.len().saturating_sub(1) {
                let (p, q) = (walk[i], walk[i + 1]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 0.0 { (((b.x - p.0) * dx + (b.y - p.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = ((b.x - p.0 - t * dx).powi(2) + (b.y - p.1 - t * dy).powi(2)).sqrt();
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, i, t));
                }
            }
            best.filter(|x| x.0 < 0.05).map(|(_, i, t)| (walk.to_vec(), i, t))
        });
        // the walk runs on without braking when its walker goes straight on into state 5 (the
        // move to its +116/+120): when its object data 9 is set, natively from its scripts
        let on_past = runs_on_now;
        let Some((walk, i, t)) = found else {
            counts.note("routing: a walker's step not on a logged plan's line (set aside)");
            continue;
        };
        let leg = |j: usize| ((walk[j + 1].0 - walk[j].0).powi(2) + (walk[j + 1].1 - walk[j].1).powi(2)).sqrt();
        let left = (i..walk.len() - 1).map(leg).sum::<f32>() - t * leg(i);
        // no brake when it runs on (the step still ends at the walk's end)
        let brake_left = if on_past { f32::INFINITY } else { left };
        let (v, s) = walk_step(speed.get(&f.follower).copied().unwrap_or(0.0), top, growth, b.factor, brake_left);
        let s = s.min(left);
        speed.insert(f.follower, v);
        let at = along_line(&walk, i, t, s);
        // the free-running place: on the same line, from where the native walk has it
        if let Some(((fx, fy), fv)) = free.get(&f.follower).copied() {
            let loc = (0..walk.len().saturating_sub(1))
                .map(|j| {
                    let (p, q) = (walk[j], walk[j + 1]);
                    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                    let l2 = dx * dx + dy * dy;
                    let t = if l2 > 0.0 { (((fx - p.0) * dx + (fy - p.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    (((fx - p.0 - t * dx).powi(2) + (fy - p.1 - t * dy).powi(2)).sqrt(), j, t)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((d, j, t)) = loc.filter(|l| l.0 < 0.05) {
                let _ = d;
                let fleft = (j..walk.len() - 1).map(leg).sum::<f32>() - t * leg(j);
                let (nv, ns) = walk_step(fv, top, growth, b.factor, if on_past { f32::INFINITY } else { fleft });
                let ns = ns.min(fleft);
                let nat = along_line(&walk, j, t, ns);
                free.insert(f.follower, (nat, nv));
                let off = ((nat.0 - a.x).powi(2) + (nat.1 - a.y).powi(2)).sqrt();
                if show_walks && off >= 0.01 && ((fx - b.x).powi(2) + (fy - b.y).powi(2)).sqrt() < 0.01 {
                    println!("free walk parts tick {} {:x} state {}->{} factor {}: native {nat:?} speed {nv:.4} (was {fv:.4}), the original's ({:.3},{:.3}) step {:.4}", f.tick, f.follower, b.state, a.state, b.factor, a.x, a.y, ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt());
                }
                counts.note(if off < 0.01 {
                    "routing: a walk run free natively at the original's place (to a hundredth of a sixteenth)"
                } else {
                    "routing: a walk run free natively NOT at the original's place"
                });
            }
        }
        let off = ((at.0 - a.x).powi(2) + (at.1 - a.y).powi(2)).sqrt();
        if show_walks && off >= 0.003 {
            println!(
                "walk miss runs on {on_past} tick {} {:x} state {}->{} factor {} at ({:.3},{:.3}) to ({:.3},{:.3}): native ({:.3},{:.3}) step {s:.4} speed {v:.4} left {left:.3} leg {i}/{}",
                f.tick, f.follower, b.state, a.state, b.factor, b.x, b.y, a.x, a.y, at.0, at.1, walk.len() - 1
            );
        }
        counts.note(if off < 0.003 {
            "routing: a walker's step natively the original's (to a thousandth of a sixteenth)"
        } else {
            "routing: a walker's step natively NOT the original's"
        });
    }
    // the frames before each tick by the native clock (logs with ',' records)
    let mut clock_frames: BTreeMap<u32, u32> = BTreeMap::new();
    // The frame clock against the original's own (',' records): from the first tick's clock,
    // each tick's frames and the clock after it natively, the speed setting an input.
    {
        let ticks = super::animate::native_clock(log);
        if let Some(first) = log.clock.first().filter(|_| !ticks.is_empty()) {
            clock_frames.insert(first.0, 0);
        }
        let mut different = 0;
        for t in &ticks {
            clock_frames.insert(t.tick, t.frames);
            if !t.same() {
                different += 1;
                if different <= 5 {
                    println!("clock different at tick {}: speed {} -> {}, frames native {} log {}, clock native {} log {}", t.tick, t.speed_before, t.speed, t.frames, t.log_frames, t.ms, t.log_ms);
                }
            }
        }
        if !ticks.is_empty() {
            counts.add("clock: a tick's frames and the clock after it natively the original's, to the bit", ticks.len() - different);
            counts.add("clock: a tick's frames or clock DIFFERENT", different);
        }
    }
    let mut native_frame_ticks: Vec<u32> = Vec::new();
    // Frames and ticks (`FrameClock`): the frames each tick holds natively against a walker's
    // frames in each tick of the log, from the first tick it has any; the clock's phase there is
    // not known (the log starts in play): the best of a thousandth's steps
    {
        let mut per: BTreeMap<u32, u32> = BTreeMap::new();
        // the walker with the most frames
        let mut by: BTreeMap<u32, usize> = BTreeMap::new();
        for f in &log.follows {
            *by.entry(f.follower).or_default() += 1;
        }
        let first_follower = by.iter().max_by_key(|e| e.1).map(|e| *e.0);
        for f in log.follows.iter().filter(|f| Some(f.follower) == first_follower) {
            *per.entry(f.tick).or_default() += 1;
        }
        // the game's speed each tick (the walkers' factor as logged, an input): the clock's ticks
        // run that much faster
        let mut speed_at: BTreeMap<u32, f32> = BTreeMap::new();
        for f in &log.follows {
            speed_at.insert(f.tick, f.before.factor);
        }
        let speed_from = |t: u32| speed_at.range(..=t).next_back().map_or(1.0, |e| *e.1);
        if let (Some((&first, _)), Some((&last, _))) = (per.first_key_value(), per.last_key_value()) {
            // the clock starts afresh after the tick before a change of speed
            let frames_at = |clock: &mut FrameClock, t: u32| {
                let frames = clock.frames_to_tick_at(speed_from(t));
                if speed_from(t + 1) != speed_from(t) {
                    clock.restart();
                }
                frames
            };
            // with the original's clock logged, the native clock from its first value (no phase
            // to fit); else the best phase
            let logged = !clock_frames.is_empty();
            let frames_at = |clock: &mut FrameClock, t: u32| if logged { clock_frames.get(&t).copied().unwrap_or(0) } else { frames_at(clock, t) };
            let best = (0..if logged { 1 } else { 42 })
                .map(|ph| {
                    let mut clock = FrameClock { ms: ph as f32 };
                    let n = (first..=last).filter(|&t| per.get(&t) == Some(&frames_at(&mut clock, t))).count();
                    (n, ph)
                })
                .max()
                .unwrap_or_default();
            counts.add("routing: ticks a walker moves in", per.len());
            counts.add("routing: ticks whose walker frames are natively the original's (the frame clock)", best.0);
            // the tick of each frame natively, from the first (the clock at its phase)
            let mut clock = FrameClock { ms: best.1 as f32 };
            for t in first..=last + 200 {
                for _ in 0..frames_at(&mut clock, t) {
                    native_frame_ticks.push(t);
                }
            }
        }
    }
    // The routing primitives' runs (opcode 27 or 45): a Sim's step at one tree and node that
    // yields (result 1) tick after tick, then returns: (sim, tree, node, first tick, return tick,
    // its result)
    let routing_runs: Vec<(u32, u16, u16, u32, u32, u8)> = {
        let mut open: BTreeMap<(u32, u16, u16), u32> = BTreeMap::new();
        let mut out = Vec::new();
        for s in &log.steps {
            let Some(t) = log.tree_of(s) else { continue };
            let at = s.node as usize * 12;
            if t.tree != s.tree || at + 12 > t.raw.len() || !matches!(u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF, 22 | 27 | 45) {
                continue;
            }
            let key = (s.sim, s.tree, s.node);
            if s.result == 1 {
                open.entry(key).or_insert(s.tick);
            } else if let Some(first) = open.remove(&key) {
                out.push((s.sim, s.tree, s.node, first, s.tick, s.result));
            }
        }
        out
    };
    // LookTowards' turn-only routes (opcode 22, towards the stack object): the follower that
    // starts (state 1) at the run's first tick where its person stands turns from the person's
    // direction (`direction_angle`) towards the object's place a frame at a time (`turn_towards`
    // in state 3), with no walk; the primitive returns as a routing one does, the tick after the
    // follower's frame after the one it is done in
    for r in &routing_runs {
        let Some(s) = log.steps.iter().find(|s| (s.sim, s.tree, s.node, s.tick) == (r.0, r.1, r.2, r.3)) else { continue };
        let Some(t) = log.tree_of(s) else { continue };
        let at = s.node as usize * 12;
        if u16::from_be_bytes([t.raw[at], t.raw[at + 1]]) & 0x7FFF != 22 {
            continue;
        }
        let Some(&id) = ids.get(&r.0) else { continue };
        let place = |who: i16| log.places.iter().rfind(|p| p.id == who && p.tick <= r.3).map(|p| (p.x, p.y));
        let direction = log.directions.iter().rfind(|d| d.1 == id && d.0 < r.3).map(|d| d.2);
        let (Some(me), Some(target), Some(direction)) = (place(id), place(s.stack_object), direction) else {
            counts.note("routing: a LookTowards' turn without its person's place, its object's or its direction");
            continue;
        };
        let Some(k) = log.follows.iter().position(|f| f.tick >= r.3 && f.before.state == 1 && (f.before.x - me.0 as f32).abs() < 1.0 && (f.before.y - me.1 as f32).abs() < 1.0) else {
            counts.note("routing: a LookTowards' turn with no follower starting where its person stands");
            continue;
        };
        let follower = log.follows[k].follower;
        let frames: Vec<_> = log.follows[k..].iter().filter(|g| g.follower == follower).collect();
        let way = ((target.0 - me.0) as f32, (target.1 - me.1) as f32);
        let mut angle = direction_angle(direction);
        let mut done = None;
        for (j, g) in frames.iter().enumerate().skip(1) {
            angle = turn_towards(angle, way, false, WALK_FRAME * g.before.factor, g.before.factor);
            if angle == (-way.0).atan2(way.1) {
                done = Some(j);
                break;
            }
        }
        let logged_done = frames.iter().position(|g| g.before.state == 3 && g.after.state == 4);
        let ret = done.and_then(|j| frames.get(j + 1)).map(|g| g.tick + 1);
        if show_walks {
            println!("look turn: sim {id} tick {}..{} direction {direction} way {way:?}: done frame {done:?} (logged {logged_done:?}), return {ret:?}; frames {:?}", r.3, r.4, frames.iter().take(4).map(|g| (g.tick, g.before.state, g.after.state)).collect::<Vec<_>>());
        }
        if let Some(ret) = ret {
            returns.entry((r.0, r.1, r.2)).or_default().push((r.3, ret));
        }
        counts.note(if ret == Some(r.4) {
            "routing: a LookTowards' turn natively returning on the original's tick"
        } else {
            "routing: a LookTowards' turn natively NOT returning on the original's tick"
        });
    }
    if show_walks {
        for r in &routing_runs {
            println!("routing run: sim {:x} tree {} node {} yields from tick {} to its return at {} (result {}) ({} steps)", r.0, r.1, r.2, r.3, r.4, r.5,
                log.steps.iter().filter(|st| (st.sim, st.tree, st.node) == (r.0, r.1, r.2) && st.tick >= r.3 && st.tick <= r.4).count());
        }
    }
    // A routing primitive's return: after its walker is done (state 4) the follower runs one
    // frame more, and the primitive returns on the tick after that frame's (the frames as logged:
    // the frame clock's); against the routing runs' returns
    for (k, f) in log.follows.iter().enumerate() {
        if !(f.before.state == 3 && f.after.state == 4) {
            continue;
        }
        let Some(next) = log.follows[k + 1..].iter().find(|g| g.follower == f.follower) else { continue };
        let predicted = next.tick + 1;
        let run = routing_runs.iter().filter(|r| r.3 <= f.tick && r.4 >= f.tick).min_by_key(|r| r.4.abs_diff(predicted));
        let Some(run) = run else { continue };
        counts.note(if run.4 == predicted {
            "routing: a routing primitive's return natively the original's tick (a frame after its walker is done)"
        } else {
            "routing: a routing primitive's return natively NOT the original's tick"
        });
        if show_walks && run.4 != predicted {
            println!("return: walker done at tick {}, its next frame at {}, predicted {predicted}, the run's return {}", f.tick, next.tick, run.4);
        }
    }
    // The turn at a walk's end (`turn_frames`): the frames a follower stays arrived (state 3)
    // against the angle from its last step's way to what it faces (the plan's last point past its
    // end), each walk's end that turns (a door's crossing walks on: set aside, its next plan at once)
    {
        let mut last_way: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
        let mut arrived: BTreeMap<u32, (u32, u32, (f32, f32))> = BTreeMap::new();
        // each follower's walk: its start's tick and place, its speed then, and its frames to arrive
        let mut walk_starts: BTreeMap<u32, (u32, (f32, f32), f32, u32)> = BTreeMap::new();
        // the ticks of a follower's frames from its walk's start (the frame clock as it ran)
        if has("REPLAY_SHOW_PLACE_OFFSETS") {
            // study: each Sim's logged place against its walker's last float that tick (the walker
            // mapped by its places: the follower whose position rounds to the place most often)
            let mut last: BTreeMap<(u32, u32), (f32, f32)> = BTreeMap::new();
            for f in &log.follows {
                last.insert((f.follower, f.tick), (f.after.x, f.after.y));
            }
            let mut hist: BTreeMap<i32, usize> = BTreeMap::new();
            for p in &log.places {
                let best = last.range((0, p.tick)..).filter(|((_, t), _)| *t == p.tick).map(|(_, &(x, y))| ((p.x as f32 - x).powi(2) + (p.y as f32 - y).powi(2)).sqrt()).fold(f32::INFINITY, f32::min);
                if best.is_finite() {
                    *hist.entry((best * 10.0) as i32).or_default() += 1;
                }
            }
            println!("place - nearest walker that tick, in tenths: {hist:?}");
        }
        if has("REPLAY_SHOW_FOLLOWS") {
            for f in log.follows.iter().filter(|f| (2036..=2041).contains(&f.tick) || (4920..=4935).contains(&f.tick)) {
                println!("follow tick {} {:x}: state {} -> {}, at ({:.3}, {:.3}) -> ({:.3}, {:.3}) factor {} -> {}", f.tick, f.follower, f.before.state, f.after.state, f.before.x, f.before.y, f.after.x, f.after.y, f.before.factor, f.after.factor);
            }
            for p in log.places.iter().filter(|p| (4920..=4925).contains(&p.tick)) {
                println!("any place tick {} {} ({}, {})", p.tick, p.id, p.x, p.y);
            }
            for p in log.places.iter().filter(|p| p.id == 237 && ((2036..=2041).contains(&p.tick) || (4920..=4935).contains(&p.tick))) {
                println!("place tick {} 237 ({}, {})", p.tick, p.x, p.y);
            }
        }
        let mut frame_ticks: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let all_frames: Vec<(u32, u32)> = log.follows.iter().map(|g| (g.follower, g.tick)).collect();
        // the game's speed each frame of a follower's walk (the follower's +124: 0 paused, 1, or 5
        // fast; the player's, the same for every walker at once), an input to the walk
        let mut speeds: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
        let mut speed_now: BTreeMap<u32, f32> = BTreeMap::new();
        for (at, f) in log.follows.iter().enumerate() {
            let (b, a) = (f.before, f.after);
            if b.state == 1 {
                // the native speed it starts from (that the walker check carried), else its last step
                let v = start_speed.get(&(f.follower, f.tick)).copied().unwrap_or_else(|| speed_now.get(&f.follower).copied().unwrap_or(0.0));
                walk_starts.insert(f.follower, (f.tick, (b.x, b.y), v, 0));
                speeds.insert(f.follower, Vec::new());
                let me = f.follower;
                frame_ticks.insert(me, std::iter::once(f.tick).chain(all_frames[at + 1..].iter().filter(|g| g.0 == me).map(|g| g.1)).collect());
            }
            if b.state <= 2
                && let Some(w) = walk_starts.get_mut(&f.follower)
            {
                w.3 += 1;
                speeds.entry(f.follower).or_default().push(b.factor);
            }
            if (a.x, a.y) != (b.x, b.y) {
                speed_now.insert(f.follower, ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt());
            }
            if (a.x, a.y) != (b.x, b.y) {
                last_way.insert(f.follower, (a.x - b.x, a.y - b.y));
            }
            if b.state == 2 && a.state == 3 {
                arrived.insert(f.follower, (f.tick, 0, (a.x, a.y)));
            }
            if b.state == 3
                && let Some(e) = arrived.get_mut(&f.follower)
            {
                e.1 += 1;
            }
            if b.state == 3 && a.state == 4
                && let Some((t0, n, end)) = arrived.remove(&f.follower)
            {
                let faced = log.plans.iter().rev().filter(|p| p.tick <= t0 && p.points.len() >= 3).find_map(|p| {
                    let q = p.points[p.points.len() - 2];
                    ((q.0 as f32 - end.0).abs() < 1.0 && (q.1 as f32 - end.1).abs() < 1.0).then(|| p.points[p.points.len() - 1])
                });
                // a walk that runs on (its walker's data 9 set) does not turn: one frame arrived,
                // then state 5 (as a turn of no angle)
                let runs_on_end = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
                let angle = if runs_on_end {
                    0.0
                } else {
                    let (Some(faced), Some(w)) = (faced, last_way.get(&f.follower).copied()) else {
                        if show_walks {
                            println!("walk end at tick {t0} {:x} set aside: no plan with its end as its last point but one (end {end:?})", f.follower);
                        }
                        continue;
                    };
                    let a1 = w.1.atan2(w.0);
                    let a2 = (faced.1 as f32 - end.1).atan2(faced.0 as f32 - end.0);
                    let pi = std::f32::consts::PI;
                    let angle = ((a2 - a1 + pi).rem_euclid(2.0 * pi) - pi).abs();
                    // a door's crossing: a plan within 5 ticks while the same routing run goes on
                    let crossing = log.plans.iter().any(|p| p.tick > t0 && p.tick <= t0 + 5 && routing_runs.iter().any(|r| r.3 <= t0 && r.4 > p.tick + 5));
                    if crossing {
                        if show_walks {
                            println!("walk end at tick {t0} {:x} set aside: a plan follows within 5 ticks (end {end:?})", f.follower);
                        }
                        continue;
                    }
                    angle
                };
                counts.note(match (runs_on_end, turn_frames(angle) == n) {
                    (false, true) => "routing: a walk's end turning natively the original's frames (3 degrees a frame)",
                    (false, false) => "routing: a walk's end turning natively NOT the original's frames",
                    (true, true) => "routing: a run-on walk's end natively the original's frames (one arrived)",
                    (true, false) => "routing: a run-on walk's end natively NOT the original's frames",
                });
                if show_walks {
                    println!("turn at tick {t0}: {:.1} degrees, {n} frames, natively {}; done at tick {}", angle.to_degrees(), turn_frames(angle), f.tick);
                }
                // the whole walk natively: from its start (state 1) along its line at the native
                // speed to its end, then the turn, against the original's frames from 1 to 4
                if let Some(&(start_tick, start, v0, walked)) = walk_starts.get(&f.follower) {
                    let line = log.plans.iter().rev().filter(|p| p.tick <= start_tick).find_map(|p| {
                        let pts: Vec<(f32, f32)> = p.points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
                        let j = pts.iter().position(|q| (q.0 - end.0).abs() < 0.5 && (q.1 - end.1).abs() < 0.5)?;
                        if let Some(i) = pts.iter().position(|q| (q.0 - start.0).abs() < 0.5 && (q.1 - start.1).abs() < 0.5) {
                            // from where the walker is (a run on can leave it a little past
                            // the plan's point: 237 at 2038, 377.309 for 377)
                            return (j > i).then(|| std::iter::once(start).chain(pts[i + 1..=j].iter().copied()).collect::<Vec<_>>());
                        }
                        // a walk begun where a run on left its walker, past the plan's start: from
                        // there through the plan's points after its start, less the sixteenth the
                        // planner nudged off the start (start + the sign of the third's way)
                        let s0 = *pts.first()?;
                        let here = ((start.0 - s0.0).powi(2) + (start.1 - s0.1).powi(2)).sqrt();
                        let sign = |v: f32| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
                        let nudged = pts.len() >= 3 && pts[1] == (s0.0 + sign(pts[2].0 - s0.0), s0.1 + sign(pts[2].1 - s0.1));
                        let k = if nudged { 2 } else { 1 };
                        (here < 5.0 && j >= k).then(|| std::iter::once(start).chain(pts[k..=j].iter().copied()).collect())
                    });
                    if line.is_none() && show_walks {
                        println!("walk end at tick {t0} {:x}: no plan line from its start {start:?} (tick {start_tick}) to its end {end:?}", f.follower);
                    }
                    if let Some(line) = line {
                        let total: f32 = line.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
                        let per_second = native_top.get(&f.follower).copied().unwrap_or(WALK_TOP_ADULT);
                        let (wtop, growth) = walk_rates(per_second);
                        let runs_on = walker_of.get(&f.follower).is_some_and(|&w| data9_at(w, f.tick));
                        let game = speeds.get(&f.follower).cloned().unwrap_or_default();
                        let (frames, _) = walk_frames(total, v0, wtop, growth, runs_on, &game);
                        let native = frames + turn_frames(angle);
                        // the Sim's place each tick natively: the walker's distance along the line
                        // after the tick's last frame, its point rounded, against the logged places
                        if let (Some(&id), Some(ticks)) = (walker_of.get(&f.follower), frame_ticks.get(&f.follower)) {
                            let (mut v, mut gone) = (v0, 0.0f32);
                            let mut gone_after: Vec<f32> = Vec::with_capacity(frames as usize);
                            for k in 0..frames as usize {
                                let factor = game.get(k).copied().unwrap_or(1.0);
                                if factor > 0.0 {
                                    let left = total - gone;
                                    let (nv, st) = walk_step(v, wtop, growth, factor, if runs_on { f32::INFINITY } else { left });
                                    v = nv;
                                    gone += st.min(left);
                                }
                                gone_after.push(gone);
                            }
                            if show_walks {
                                // study: each Sim with places during the walk, and how many the walk gives
                                let mut by: BTreeMap<i16, (usize, usize)> = BTreeMap::new();
                                for r in log.places.iter().filter(|r| r.tick > start_tick && r.tick <= f.tick) {
                                    let e = by.entry(r.id).or_default();
                                    e.1 += 1;
                                    if let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                        e.0 += usize::from(((x + 0.5).floor() as i32, (y + 0.5).floor() as i32) == (r.x, r.y));
                                    }
                                }
                                println!("   walk from {start_tick} (follower {:x}, mapped to {id}): places by Sim (fit, of) {by:?}; frames {} ticks {:?}..; line {:?}", f.follower, gone_after.len(), &ticks[..ticks.len().min(3)], &line[..line.len().min(6)]);
                            }
                            // the Sims it walks: the follower's, and any other whose places during the
                            // walk the walk gives at least half the time (two Sims logged moving as one)
                            let walked: BTreeSet<i16> = {
                                let mut by: BTreeMap<i16, (usize, usize)> = BTreeMap::new();
                                for r in log.places.iter().filter(|r| r.tick > start_tick && r.tick <= f.tick) {
                                    let e = by.entry(r.id).or_default();
                                    e.1 += 1;
                                    if let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                        e.0 += usize::from(((x + 0.5).floor() as i32, (y + 0.5).floor() as i32) == (r.x, r.y));
                                    }
                                }
                                by.into_iter().filter(|&(sim, (fit, of))| sim == id || 2 * fit >= of).map(|(sim, _)| sim).collect()
                            };
                            for r in log.places.iter().filter(|r| walked.contains(&r.id) && r.tick > start_tick && r.tick <= f.tick) {
                                let id = r.id;
                                let Some(k) = ticks.iter().take(gone_after.len()).rposition(|&t| t <= r.tick) else { continue };
                                let (x, y) = along_line(&line, 0, 0.0, gone_after[k]);
                                // to the nearest (truncating puts 816 of 1,186 off)
                                let ours = ((x + 0.5).floor() as i32, (y + 0.5).floor() as i32);
                                if has("REPLAY_SHOW_WALK_MISS") && ours != (r.x, r.y) {
                                    let around: Vec<String> = (k.saturating_sub(3)..(k + 4).min(ticks.len()).min(gone_after.len())).map(|j| format!("{}{}:{}@{:.2}", if j == k { "*" } else { "" }, j, ticks[j], gone_after[j])).collect();
                                    println!("walk float {id} tick {}: native ({x:.3}, {y:.3}), log {:?}, walked {:.3}; start {start_tick}; frames {}", r.tick, (r.x, r.y), gone_after[k], around.join(" "));
                                }
                                native_places.insert((id, r.tick), ours);
                                counts.note(if ours == (r.x, r.y) {
                                    "routing: a walking Sim's place each tick natively the original's (the walker's, rounded)"
                                } else {
                                    "routing: a walking Sim's place each tick natively NOT the original's"
                                });
                                if ours != (r.x, r.y) && show_walks {
                                    let at = |k: usize| {
                                        let (x, y) = along_line(&line, 0, 0.0, gone_after[k.min(gone_after.len() - 1)]);
                                        ((x, y), ((x + 0.5).floor() as i32, (y + 0.5).floor() as i32))
                                    };
                                    println!("   place of {id} at tick {}: native {ours:?} (exact {:?}), the original's {:?}; a frame before {:?}, after {:?}", r.tick, at(k).0, (r.x, r.y), at(k.saturating_sub(1)).1, at(k + 1).1);
                                }
                            }
                        }
                        let original = walked + n;
                        // the routing primitive natively: its return the tick after its walker's
                        // frame after done (frame `native`, from the walk's start), against the run
                        // that holds the walk
                        if let Some(ticks) = frame_ticks.get(&f.follower)
                            && let Some(ret) = routing_return_tick(ticks, frames, turn_frames(angle))
                            && let Some(run) = routing_runs.iter().filter(|r| r.3 <= start_tick + 1 && r.4 >= f.tick).min_by_key(|r| r.4 - r.3)
                        {
                            returns.entry((run.0, run.1, run.2)).or_default().push((run.3, ret));
                            counts.note(if run.4 == ret {
                                "routing: a routing primitive natively returning on the original's tick (its walk and turn native)"
                            } else {
                                "routing: a routing primitive natively NOT returning on the original's tick"
                            });
                            // with the frame clock native too: the walker's frames from its start's
                            // (its tick and its place among that tick's frames as logged)
                            let within = ticks.iter().take_while(|&&t| t == start_tick).count();
                            let before = log.follows.iter().filter(|g| g.follower == f.follower && g.tick == start_tick).count() - within;
                            if let Some(first_at) = native_frame_ticks.iter().position(|&t| t == start_tick) {
                                let from = &native_frame_ticks[(first_at + before).min(native_frame_ticks.len())..];
                                let nat = routing_return_tick(from, frames, turn_frames(angle));
                                if show_walks && nat != Some(run.4) {
                                    let logged = &ticks[..ticks.len().min((frames + turn_frames(angle) + 2) as usize)];
                                    let native = &from[..from.len().min(logged.len())];
                                    let diff: Vec<(usize, u32, u32)> = logged.iter().zip(native).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).take(6).collect();
                                    println!("clock run from tick {start_tick}: native return {nat:?}, the original's {}; frames (index, logged tick, native tick) differing {diff:?}", run.4);
                                }
                                counts.note(if nat == Some(run.4) {
                                    "routing: a routing primitive natively returning on the original's tick (the frame clock native too)"
                                } else {
                                    "routing: a routing primitive natively NOT returning on the original's tick (the frame clock native)"
                                });
                            }
                            // each of the run's steps: natively a yield before its return tick, the
                            // return on it
                            for st in log.steps.iter().filter(|st| (st.sim, st.tree, st.node) == (run.0, run.1, run.2) && st.tick >= run.3 && st.tick <= run.4) {
                                let native_yield = st.tick < ret;
                                counts.note(if native_yield == (st.result == 1) {
                                    "routing: a routing primitive's step natively as the original's (a yield, or its return)"
                                } else {
                                    "routing: a routing primitive's step natively NOT as the original's"
                                });
                            }
                        }
                        counts.note(if native == original {
                            "routing: a whole walk natively the original's frames (walk and turn)"
                        } else {
                            "routing: a whole walk natively NOT the original's frames"
                        });
                        if show_walks {
                            println!("whole walk from tick {start_tick}: {total:.1} long, natively {frames} + {} frames, the original's {walked} + {n}", turn_frames(angle));
                        }
                    }
                }
            }
        }
    }
    // Each plan with the routing primitives its Sim ran in its tick (study: REPLAY_SHOW_PLANS):
    // opcode 27 or 45, the stack object and its place, the operand, the result.
    if has("REPLAY_SHOW_PLANS") {
        for pl in &log.plans {
            let (Some(a), Some(b)) = (pl.points.first(), pl.points.last()) else { continue };
            let who = world.objects().find(|o| o.person && (o.x16, o.y16) == *a).map_or(-1, |o| o.id() as i32);
            println!("plan tick {} from {},{} to {},{} ({} points), Sim at its start {who}", pl.tick, a.0, a.1, b.0, b.1, pl.points.len());
            let pts: Vec<String> = pl.points.iter().map(|&(x, y)| format!("{}/{}", x as f32 / 16.0, y as f32 / 16.0)).collect();
            println!("   points {}", pts.join(" "));
            let gs: Vec<String> = pl.goals.iter().map(|g| format!("{}/{} {:x} {:x}", g[0] as i32 as f32 / 16.0, g[1] as i32 as f32 / 16.0, g[2], g[3])).collect();
            println!("   goals (took {}) {}", pl.chosen, gs.join(", "));
            for s in log.steps.iter().filter(|s| s.tick == pl.tick) {
                let Some(t) = log.tree_of(s) else { continue };
                let at = s.node as usize * 12;
                if at + 12 > t.raw.len() || t.tree != s.tree {
                    continue;
                }
                let raw = &t.raw[at..at + 12];
                let op = u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF;
                if op != 27 && op != 45 {
                    continue;
                }
                let o = world.find_object(s.stack_object);
                let mut l = format!(
                    "   op {op} by {} in {}:{} on {} ({} at {},{}) operand",
                    ids.get(&s.sim).map_or(-1, |&i| i as i32),
                    s.tree,
                    s.node,
                    s.stack_object,
                    o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                    o.map_or(0, |o| o.x16),
                    o.map_or(0, |o| o.y16)
                );
                for x in &raw[4..12] {
                    l += &format!(" {x:02x}");
                }
                l += &format!(" -> {} result {}; params", s.after, s.result);
                for p in &s.params {
                    l += &format!(" {p}");
                }
                println!("{l}");
            }
        }
    }
    // The routing steps (GotoRoutingSlot, opcode 45): the slot they name, of the stack object's
    // SLOT, against the next plan's end (study: REPLAY_SHOW_SLOTS).
    if has("REPLAY_SHOW_SLOTS") {
        let mut gi = 0;
        for s in &log.steps {
            let Some(raw) = raw45(s) else { continue };
            while gi < log.plans.len() && log.plans[gi].tick < s.tick {
                gi += 1;
            }
            if gi >= log.plans.len() || log.plans[gi].tick != s.tick {
                continue; // a plan made in this step's tick
            }
            let o = world.find_object(s.stack_object);
            let mut l = format!(
                "route step tick {}: object {} ({} at {},{}), operand",
                s.tick,
                s.stack_object,
                o.and_then(|o| o.file).map_or("-".to_string(), |f| file_name(lib, f)),
                o.map_or(0, |o| o.x16),
                o.map_or(0, |o| o.y16)
            );
            for b in &raw[4..12] {
                l += &format!(" {b:02x}");
            }
            l += ", params";
            for p in &s.params {
                l += &format!(" {p}");
            }
            let pl = &log.plans[gi];
            if let (Some(a), Some(b)) = (pl.points.first(), pl.points.last()) {
                l += &format!("; next plan tick {} from {},{} to {},{}", pl.tick, a.0, a.1, b.0, b.1);
            }
            l += &format!(" result {}; lookups", s.result);
            for v in &s.lookups {
                l += &format!(" ({},{})={}", v.scope, v.data, v.value);
            }
            println!("{l}");
            if let Some(f) = o.and_then(|o| o.file) {
                for v in lib.file(f).slots.values() {
                    for (k, sl) in v.iter().enumerate() {
                        if sl.kind == 3 {
                            println!(
                                "   slot {k}: flags {:x}, proximity {}-{} ({}), facing {}, offset {},{}",
                                sl.flags,
                                sl.min_proximity,
                                sl.max_proximity,
                                sl.optimal_proximity,
                                sl.facing,
                                g(sl.x as f64, 6),
                                g(sl.y as f64, 6)
                            );
                        }
                    }
                }
            }
        }
    }
    if show {
        for (name, n) in &crossed {
            println!("planned routes cross a tile of {name}: {n}");
        }
    }
    // Each walk (a Sim's tile changes, a pause of 40 ticks or more ending one) against the native
    // route between its ends (Walls::route, no object blocking yet).
    let mut seq: BTreeMap<i16, Vec<(u32, Tile)>> = BTreeMap::new();
    for p in &log.places {
        let Some(o) = world.find_object(p.id) else { continue };
        if !o.person || p.x < 0 || p.y < 0 {
            continue;
        }
        let t = (p.x >> 4, p.y >> 4);
        let v = seq.entry(p.id).or_default();
        if v.last().is_none_or(|l| l.1 != t) {
            v.push((p.tick, t));
        }
    }
    for (id, v) in &seq {
        let mut start = 0;
        for i in 1..=v.len() {
            if i < v.len() && v[i].0 - v[i - 1].0 < 40 {
                continue;
            }
            // A walk: v[start - 1 or start] .. v[i - 1] (from the tile it stood on).
            let from = if start > 0 { start - 1 } else { start };
            if i - 1 > from {
                let logged: Vec<Tile> = v[from..i].iter().map(|x| x.1).collect();
                let jump = logged.windows(2).any(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs() != 1);
                if !jump {
                    let r = walls.route(logged[0], logged[logged.len() - 1], None);
                    let same_length = r.len() == logged.len();
                    counts.note(if same_length { "routing: a walk as long as the native route (walls only)" } else { "routing: a walk NOT as long as the native route (walls only)" });
                    if r == logged {
                        counts.note("routing: a walk's tiles the native route's");
                    }
                    if !same_length && show {
                        let mut l = String::new();
                        for t in &r {
                            l += &format!(" {},{}", t.0, t.1);
                        }
                        println!("{l}");
                    }
                    if show {
                        println!(
                            "walk of {id} from tick {}: {} steps, native {} ({},{} -> {},{})",
                            v[from].0,
                            logged.len() - 1,
                            r.len().saturating_sub(1),
                            logged[0].0,
                            logged[0].1,
                            logged[logged.len() - 1].0,
                            logged[logged.len() - 1].1
                        );
                    }
                }
            }
            start = i;
        }
    }
    if show_walks {
        for r in &routing_runs {
            if !returns.get(&(r.0, r.1, r.2)).is_some_and(|v| v.iter().any(|x| x.0 == r.3)) {
                println!("routing run not modelled: sim {:x} tree {} node {} from tick {} to {}", r.0, r.1, r.2, r.3, r.4);
            }
        }
    }
    NativeRouting { returns, places: native_places }
}



