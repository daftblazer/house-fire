//! The run alone (`replay --alone`): the simulation runs by itself, with no log after the start.
//!
//! The start is `drive::setup`'s: the lot loaded from the card (or the disc) and the load pass,
//! for which the log is still read (docs/simulation.md, "The run alone": the tick-0 calls, which
//! function of which object, and the sleeps the saves lack). After it nothing of the log is
//! used: no comparison, no pauses, no commands, no dialog answers; Random draws from the
//! engine's own generator (`World::rng`), seeded by `--seed`. The run never stops at a parting.
//! A primitive that is not native gets its neutral answer and is counted by kind; a script error,
//! a runaway loop (the interpreter's iteration limit) and a panic end one object's stack, not the
//! run, and are counted by tree and kind.

use std::collections::BTreeMap;

use crate::interp::Host;
use crate::library::Library;
use crate::save::{LotChunks, Neighbour};
use crate::world::{ObjectKey, World};
use super::drive::{DriveHost, Driven, alone_sim, setup};
use super::start::RunWith;
use crate::walker::Walker;

/// What one Sim did.
#[derive(Default)]
pub(super) struct SimStats {
    pub started: u32,
    pub finished: u32,
    /// The depths of its open interactions' frames.
    pub open: Vec<usize>,
    /// Tiles walked (the path of its place a tick at a time, while on the lot).
    walked: f64,
    last_at: Option<(i32, i32)>,
    low: Vec<f32>,
    last: Vec<f32>,
}

/// What a Sim shows of itself (`Alone::motions`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// An animation the Sim's Animate plays (or last played, held, while its interaction goes on):
    /// the animation's id, how many frames along it is and whether it plays backward.
    Playing(u32, f32, bool),
    /// Walking: seconds into the walk cycle (the walk's animation is the figure's to choose).
    Walking(f32),
    /// Standing about, in no animation of its own.
    Idle,
}

/// What a run alone counts.
#[derive(Default)]
pub struct Tally {
    neutral: BTreeMap<(u16, &'static str, i32), u32>,
    errors: BTreeMap<String, u32>,
    sims: BTreeMap<i16, SimStats>,
    /// Stacks of Sims restarted at their bottom frame after an error.
    pub(super) restarted: u32,
    /// The turns going on (LookTowards): the ticks left and the direction it ends in.
    turns: BTreeMap<i16, (u32, i16)>,
    /// The walks going on, by the walking object.
    pub(super) walks: BTreeMap<i16, Walker>,
    /// The speed each walker's last walk left it (the next starts at it).
    speeds: BTreeMap<i16, f32>,
    /// The way (a unit step) a Sim's route crosses at the door whose portal it was sent through.
    crossing: BTreeMap<i16, (i32, i32)>,
    /// How long each walker has walked, in seconds of the walk cycle (the figure's phase).
    walk_time: BTreeMap<i16, f32>,
    /// Walks planned, arrived, and asked for with no way.
    pub(super) planned: u32,
    pub(super) arrived: u32,
    pub(super) no_way: u32,
    /// Walks to a goal outside the room of the object they go to, and legs of a walk through a
    /// wall (a closed edge): the figures of the walk's being native.
    outside_room: usize,
    through_wall: u32,
}

/// The direction (object data 1) of a step.
fn direction_of(dx: f32, dy: f32) -> i16 {
    crate::routing::facing_direction(f32::atan2(-dx, dy))
}

impl DriveHost<'_> {
    /// GotoRelativePosition (27) and GotoRoutingSlot (45) of a run alone, natively as the
    /// original runs them (`docs/simulation.md`, "Routing" and "The run alone"): the planner's
    /// request (`routing::slot_request`, `relative_request`), the goal it takes
    /// (`planner::choose_goal`) and the way to it (`coarse_way`, `refine`, `point_after_goal`) on
    /// the rects the world gives (`route_map`); a route that changes room at a door pushes the
    /// door's portal function, one to a slot for sitting the seat's sit function; else the Sim's
    /// walker (`walker::Walker`) walks the way a frame at a time (`DriveHost::walker_frames`) and the
    /// primitive yields until it is let go. A request with no goal, or no way: false, as the
    /// original goes on in its frame. (Not as the original: no one asked to move out of a slot's
    /// places, and no run on after a door's walk.)
    pub(super) fn go(&mut self, interp: &mut crate::interp::Interpreter, n: &crate::bhav::Node) -> crate::interp::PrimResult {
        use crate::interp::PrimResult;
        use crate::planner::{choose_goal, coarse_way, point_after_goal, refine};
        use crate::routing::{relative_request, slot_request, Goal, RequestWorld};
        let (me, lib) = (self.me, self.lib);
        let id = self.world.get(me).id();
        if let Some(w) = self.tally.as_ref().and_then(|t| t.walks.get(&id)) {
            if !w.released() {
                return PrimResult::Yield;
            }
            let t = self.tally.as_mut().expect("alone");
            if let Some(w) = t.walks.remove(&id) {
                t.speeds.insert(id, w.speed());
            }
            t.arrived += 1;
            return PrimResult::True;
        }
        let so = interp.top().stack_object;
        let o = self.world.get(me);
        if !o.on_lot() {
            return PrimResult::False;
        }
        let (from, my_facing) = ((o.x16, o.y16), o.facing());
        let Some(t) = self.world.find_object(so) else { return PrimResult::True };
        let (at, facing, target_is_door, target_id, leader) = ((t.x16, t.y16), t.facing(), t.door, t.id(), t.leader);
        let tile = |p: (i32, i32)| (p.0 >> 4, p.1 >> 4);
        let busy: std::collections::HashSet<(i32, i32)> = self.world.objects().filter(|x| !x.person && x.id() != target_id && (leader == 0 || x.leader != leader) && x.on_lot() && x.level == 1).map(|x| tile((x.x16, x.y16))).collect();
        let occupied = |p: (i32, i32)| busy.contains(&tile(p));
        let obstacles = super::super::route_map::obstacles(&self.world, id, Some(target_id), &[], super::super::route_map::outdoors(&self.world, from, &[at]), &|o| (o.x16, o.y16));
        let rw = RequestWorld { walls: &self.world.walls, at, occupied: &occupied, obstacles: &obstacles };
        // the request: its goals, and what the plan adds after the goal (the request's kind and flags)
        let (goals, kind, flags): (Vec<Goal>, i32, i32) = if n.opcode == 45 {
            let slot = t.file.and_then(|file| lib.routing_slot(file, self.world.definition(t), n.operand16(1), n.operand16(0), &interp.top().params)).copied();
            let Some(slot) = slot else { return PrimResult::True };
            // (a slot that names no direction, the club's dance floor: its object's own place, a stand-in)
            let crossing = if target_is_door { self.tally.as_mut().and_then(|t| t.crossing.remove(&id)) } else { None };
            let goals = if let Some((dx, dy)) = crossing {
                // a portal's crossing step: a tile on, the way the route goes
                vec![Goal { place: (from.0 + 16 * dx, from.1 + 16 * dy), score: 135, bit: 0 }]
            } else if slot.flags & 0xFF == 0 {
                vec![Goal { place: at, score: 135, bit: 0 }]
            } else {
                slot_request(&slot, facing, false, &rw, &mut self.world.rng)
            };
            (goals, slot.facing, slot.flags)
        } else {
            let direction = n.operand[2] as i8;
            (relative_request(direction, facing, &rw), n.operand[3] as i8 as i32, if (0..8).contains(&direction) { 1 << direction } else { 0 })
        };
        let places: Vec<(i32, i32)> = goals.iter().map(|g| g.place).collect();
        if super::study::has("REPLAY_ALONE_SHOW_NO_WAY") {
            println!("goals of object {id} tick {}: {places:?} (me {from:?} room {}, object {target_id} at {at:?} room {})", self.tick, self.world.walls.room(from.0 >> 4, from.1 >> 4), self.world.walls.room(at.0 >> 4, at.1 >> 4));
        }
        let (rects, growth) = super::super::route_map::search_for(&self.world, id, Some(target_id), from, &places, &|o| (o.x16, o.y16));
        // the goal the planner takes (its backward search of several), the way to it, its points
        let ways = (|| {
            let goals3: Vec<(i32, i32, i32)> = goals.iter().map(|g| (g.place.1, g.place.0, g.score)).collect();
            let pick = if goals3.len() == 1 { 0 } else { choose_goal(&rects, growth, (from.1, from.0), &goals3)? };
            let &(gy, gx, _) = goals3.get(pick)?;
            let way = coarse_way(&rects, growth, (from.1, from.0), (gy, gx))?;
            Some(((gx, gy), refine(&way).iter().map(|&(y, x)| (x, y)).collect::<Vec<(i32, i32)>>()))
        })();
        let Some((goal, mut points)) = ways else {
            let t = self.tally.as_mut().expect("alone");
            t.no_way += 1;
            if super::study::has("REPLAY_ALONE_SHOW_NO_WAY") {
                println!("no way: tick {} object {id} at {from:?} (tile {:?}) to object {target_id} opcode {}", self.tick, tile(from), n.opcode);
            }
            return PrimResult::False;
        };
        let walked = points.len();
        if super::study::has("REPLAY_ALONE_SHOW_WALKS") {
            println!("walk: tick {} object {id} op {} to object {target_id} at {at:?}: from {from:?} goal {goal:?} of {} goals, points {points:?}", self.tick, n.opcode, goals.len());
        }
        points.extend(point_after_goal(kind, flags, at, Some(facing as i32), my_facing as i32, &points));
        // A route that changes room at a door pushes the door's portal function (not the portal's
        // own walk to the door), a route to a seat the seat's sit function, and stays at its node
        if !target_is_door
            && let Some(door) = self.world.portal_on_route(&points)
            && let Some(tree) = self.world.function_tree(door, crate::world::PORTAL_FUNCTION)
        {
            let (did, dfile) = (self.world.get(door).id(), self.world.get(door).file);
            // The way the route crosses (the tiles of its first change of room): the portal's own
            // crossing step (below) goes that way. The original's portal chooses its slot by a side
            // test (its 8221, on the door's attribute 1) the engine sets; its rule is not read, and
            // the slots of a door two tiles wide lie along it, so the way is taken from the route.
            let w = &self.world.walls;
            let step = points.windows(2).map(|p| ((p[0].0 >> 4, p[0].1 >> 4), (p[1].0 >> 4, p[1].1 >> 4))).find(|&(a, b)| {
                let (r0, r1) = (w.room(a.0, a.1), w.room(b.0, b.1));
                r0 != r1 && r0 != 0 && r1 != 0
            });
            if let Some((a, b)) = step {
                self.tally.as_mut().expect("alone").crossing.insert(id, ((b.0 - a.0).signum(), (b.1 - a.1).signum()));
            }
            if interp.push(lib, tree, &[]) {
                interp.top().stack_object = did;
                interp.top().file = dfile;
                return PrimResult::Continue;
            }
        }
        if n.opcode == 45
            && let Some(chair) = self.world.seat_at_routing_slot(n, &interp.top().params, so, &points)
            && let Some(tree) = self.world.function_tree(chair, crate::world::SIT_FUNCTION)
        {
            let (cid, cfile) = (self.world.get(chair).id(), self.world.get(chair).file);
            let params: Vec<i16> = interp.defaults().iter().take(4).copied().collect();
            if interp.push(lib, tree, &params) {
                interp.top().stack_object = cid;
                interp.top().file = cfile;
                return PrimResult::Continue;
            }
        }
        // the walker: its line the way to the goal, then a turn to face the point past it
        let walls = &self.world.walls;
        let through = points[..walked].windows(2).filter(|p| !walls.line_open(p[0], p[1])).count();
        let outside = walls.room(goal.0 >> 4, goal.1 >> 4) != walls.room(at.0 >> 4, at.1 >> 4);
        let age = o_age(self.world.get(me), lib);
        let t = self.tally.as_mut().expect("alone");
        t.planned += 1;
        t.outside_room += usize::from(outside);
        t.through_wall += through as u32;
        let to_f = |p: &(i32, i32)| (p.0 as f32, p.1 as f32);
        let speed = t.speeds.get(&id).copied().unwrap_or(0.0);
        let walker = Walker::new(points[..walked].iter().map(to_f).collect(), points.get(walked).map(to_f), speed, my_facing, crate::routing::walk_top(age));
        t.walks.insert(id, walker);
        PrimResult::Yield
    }

    /// `n` frames of every walker (`Walker::frame`), each Sim then at the place and direction its
    /// walker gives it (the route follower's frame, `docs/simulation.md`, "The walker's step").
    pub(super) fn walker_frames(&mut self, n: u32) {
        let Some(t) = self.tally.as_mut() else { return };
        for (&id, w) in t.walks.iter_mut() {
            for _ in 0..n {
                w.frame();
            }
            if let Some(k) = self.world.find(id) {
                let o = self.world.get_mut(k);
                o.x16 = w.at.0.round() as i32;
                o.y16 = w.at.1.round() as i32;
                if let Some(d) = o.data.get_mut(crate::world::DATA_FACING) {
                    *d = w.direction;
                }
            }
        }
    }
}

/// A person's body age (the first of its file's body strings, STR# 200: "adult" or "child").
fn o_age<'a>(o: &crate::world::Object, lib: &'a Library) -> Option<&'a str> {
    o.file.and_then(|f| lib.file(f).strings.get(&200)).and_then(|t| t.first()).map(String::as_str)
}

impl Tally {
    pub(super) fn neutral(&mut self, opcode: u16, why: &'static str, tree: i32) {
        *self.neutral.entry((opcode, why, tree)).or_default() += 1;
    }

    pub(super) fn error(&mut self, tree: i32, node: u16, kind: &str, _object: i16) {
        *self.errors.entry(format!("error {kind} in tree {tree} node {node}")).or_default() += 1;
    }

    pub(super) fn sim(&mut self, id: i16) -> &mut SimStats {
        self.sims.entry(id).or_default()
    }
}

/// The primitives' names, where known (docs/primitives.md).
fn primitive_name(opcode: u16) -> String {
    let name = match opcode {
        0 => "Idle",
        2 => "Expression",
        4 => "Grab",
        6 => "ChangeSuit",
        7 => "Update",
        8 => "Random",
        11 => "DistanceTo",
        12 => "DirectionTo",
        13 => "PushAction",
        14 => "FindBestObjectForFunction",
        17 => "IdleForInput",
        18 => "RemoveObjectInstance",
        20 => "RunFunctionalTree",
        22 => "LookTowards",
        23 => "PlaySound",
        24 => "Relationship",
        26 => "Relationship2",
        27 => "GotoRelativePosition",
        28 => "CallNamedTree",
        29 => "SetMotiveDelta",
        31 => "SetToNext",
        32 => "TestObjectType",
        36 => "Dialog",
        41 => "SetBalloon",
        42 => "CreateObjectInstance",
        43 => "DropOnto",
        44 => "Animate",
        45 => "GotoRoutingSlot",
        46 => "Snap",
        48 => "KillSounds",
        49 => "NotifyStackObject",
        50 => "MakeActionString",
        56 => "CompleteGoal",
        _ => "",
    };
    if name.is_empty() { format!("primitive {opcode}") } else { format!("{name} ({opcode})") }
}

/// A hash (FNV-1a, 64 bits) of the world's state: each object's place, data, motives and tree
/// stack, the globals and the generator.
fn world_hash(d: &mut Driven) -> u64 {
    struct H(u64);
    impl H {
        fn put(&mut self, v: i64) {
            for b in v.to_le_bytes() {
                self.0 = (self.0 ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
        fn all(&mut self, v: &[i16]) {
            self.put(v.len() as i64);
            for &x in v {
                self.put(x as i64);
            }
        }
    }
    let mut h = H(0xcbf29ce484222325);
    let lib = d.host.lib;
    let keys: Vec<ObjectKey> = d.host.world.objects().filter_map(|o| d.host.world.find(o.id())).collect();
    for k in keys {
        let o = d.host.world.get(k);
        h.put(o.id() as i64);
        h.put(o.x16 as i64);
        h.put(o.y16 as i64);
        h.put(o.level as i64);
        h.all(&o.temps);
        h.all(&o.data);
        h.all(&o.attributes);
        h.all(&o.person_data);
        for m in o.motives.iter().chain(&o.motives_b) {
            h.put(m.to_bits() as i64);
        }
        h.put(o.queue.len() as i64);
        let frames: Vec<_> = d.sim.stack_of(&d.host.world, k).stack().iter().map(|f| (f.tree.map_or(-1, |t| lib.tree_at(t).id as i64), f.node, f.params.clone(), f.locals.clone(), f.stack_object)).collect();
        for (t, n, p, l, so) in frames {
            h.put(t);
            h.put(n as i64);
            h.all(&p);
            h.all(&l);
            h.put(so as i64);
        }
    }
    h.all(&d.host.world.globals);
    h.put(d.host.world.rng.state as i64);
    h.0
}

/// How long a run alone goes and the seed of its random numbers.
pub struct AloneRun {
    pub ticks: u32,
    pub seed: u64,
}

/// What a run alone found.
pub struct AloneReport {
    pub ticks: u32,
    pub of: u32,
    pub hash: u64,
}

/// A generator state for a seed (never 0; a splitmix64 step so that near seeds differ).
fn seed_state(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    (z ^ (z >> 31)) | 1
}

/// Each Sim's walk and motives after a tick.
fn watch(host: &mut DriveHost) {
    let seen: Vec<(i16, i32, i32, bool, Vec<f32>)> = host.world.objects().filter(|o| o.person).map(|o| (o.id(), o.x16, o.y16, o.on_lot(), o.motives.clone())).collect();
    let Some(t) = &mut host.tally else { return };
    for (id, x, y, on, motives) in seen {
        let s = t.sim(id);
        if s.low.is_empty() {
            s.low = motives.clone();
        }
        for (lo, &m) in s.low.iter_mut().zip(&motives) {
            *lo = lo.min(m);
        }
        s.last = motives;
        if on {
            if let Some((px, py)) = s.last_at {
                s.walked += f64::hypot((x - px) as f64, (y - py) as f64) / 16.0;
            }
            s.last_at = Some((x, y));
        } else {
            s.last_at = None;
        }
    }
}

/// A run alone, a tick at a time: the simulation from the loaded start, with no log. `alone` runs
/// it for a number of ticks and prints its figures; a front end (the sandbox's window) steps it
/// itself, queues the player's commands, asks the menu's checks and answers the dialogs.
pub struct Alone<'l> {
    d: Driven<'l>,
    lib: &'l Library,
    clock: crate::routing::FrameClock,
    /// The frames the tick in hand follows (the clock's).
    frames: u32,
    tick: u32,
}

/// A dialog open for the player (Dialog, opcode 36): who asked, and its node's operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogAsk {
    /// The object whose script opened it.
    pub object: i16,
    /// As the node has it (bytes 2 to 4 the text's and the buttons' strings in the object's
    /// STR# 301).
    pub operand: [u8; 8],
}

/// An interaction of a target's pie menu that passed the menu's checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    /// The interaction's action tree (what `queue_command` takes).
    pub action: u16,
    /// Its name from the object's strings.
    pub name: String,
}

/// "Get social eligibility" (PersonGlobals 8230): the check the pie menu of a person runs first.
const SOCIAL_ELIGIBILITY: u16 = 8230;

impl<'l> Alone<'l> {
    /// The start: `drive::setup`'s, then the engine's own (the autonomy level of the household's
    /// Sims, the random generator seeded by `seed`). None if the log does not start with the lot.
    pub fn new(log: &crate::tree_log::Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], with: RunWith, seed: u64) -> Option<Alone<'l>> {
        let mut d = setup(log, lib, lot, globals, neighbours, with, Some(Tally::default()))?;
        // (a panic in a stack's turn is counted, not printed, unless studied)
        if !super::study::has("REPLAY_ALONE_SHOW_ERRORS") {
            std::panic::set_hook(Box::new(|_| {}));
        }
        if let Some(p) = d.host.parting.take() {
            println!("alone: the load parted from the log at object {} tree {} node {} (the run goes on): {}", p.object, p.tree, p.node, p.text.chars().take(300).collect::<String>());
        }
        d.host.expected.clear();
        // The autonomy level (person data 36): the original's engine reads it (no script does) and
        // the logs show 50 for the home Sims; the saves hold 0. A household Sim with none gets 50
        // (the free will of the lot's own family); the visitors' mark (32) keeps the others out.
        let keys: Vec<ObjectKey> = d.host.world.objects().filter(|o| o.person).filter_map(|o| d.host.world.find(o.id())).collect();
        for k in keys {
            let o = d.host.world.get_mut(k);
            if o.person_data.get(32) == Some(&0)
                && let Some(v) = o.person_data.get_mut(36)
                && *v == 0
            {
                *v = 50;
            }
        }
        d.host.world.rng.state = seed_state(seed);
        d.host.world.run_made = true;
        Some(Alone { d, lib, clock: crate::routing::FrameClock::default(), frames: 0, tick: 0 })
    }

    /// Study (REPLAY_ALONE_SHOW_TREE=OBJECT:TREE,..., or OBJECT:c for its constants, at the tick
    /// REPLAY_ALONE_SHOW_TREE_AT, default 1): the trees' nodes as OBJECT's file has them.
    fn show_trees(&mut self) {
        let Some(v) = super::study::env("REPLAY_ALONE_SHOW_TREE") else { return };
        for item in v.split(',') {
            let Some((o, t)) = item.split_once(':') else { continue };
            let file = if o == "g" { self.lib.global() } else { self.d.host.world.find_object(super::study::atoi(o) as i16).and_then(|o| o.file) };
            if t == "i" {
                if let Some(k) = self.d.host.world.find(super::study::atoi(o) as i16) {
                    let (lib, i) = (self.lib, self.d.sim.stack_of(&self.d.host.world, k));
                    let frames: Vec<String> = i.stack().iter().map(|f| format!("{}:{}", f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node)).collect();
                    println!("object {o}: stack {} asleep {} done {}", frames.join(" "), i.asleep(), i.done());
                    let key = alone_sim(super::study::atoi(o) as i16);
                    if let Some(ar) = &self.d.host.anim
                        && let Some((id, play)) = ar.m.play(key)
                    {
                        println!("object {o}: plays animation {id} ({}) at {} finished {} backward {} play {play:?}", ar.cat.by_id.get(&id).map_or("?", |a| &a.name), play.place(), play.finished(), play.backward());
                    }
                }
                if let Some(ob) = self.d.host.world.find_object(super::study::atoi(o) as i16) {
                    println!("object {o}: person {} file {:?} at ({}, {}) on lot {} data {:?} person data {:?}", ob.person, file.map(|f| self.lib.file(f).path.clone()), ob.x16, ob.y16, ob.on_lot(), ob.data, ob.person_data);
                }
                continue;
            }
            if let Some(m) = t.strip_prefix('m') {
                // the trees of the file with an expression on motive M (scope 14 or 15), written to
                let (m, Some(f)) = (super::study::atoi(m) as u16, file) else { continue };
                for (id, &tid) in &self.lib.file(f).trees {
                    for (i, n) in self.lib.tree_at(tid).nodes.iter().enumerate() {
                        if n.opcode == 2 && [14, 15].contains(&n.operand[6]) && n.operand16(0) == m && n.operand[5] != 0 {
                            println!("tree {id} node {i}: {:?}", n.operand);
                        }
                    }
                }
                continue;
            }
            if t == "c" {
                let all: Vec<_> = (0..256).map(|i| self.lib.constant(file, i)).collect();
                println!("constants of object {o}'s file, data 0 on: {all:?}");
                continue;
            }
            match self.lib.tree(file, super::study::atoi(t) as u16) {
                Some(tid) => {
                    println!("tree {t} of object {o} ({}):", file.map_or("-", |f| self.lib.file(f).path.rsplit('/').next().unwrap_or("")));
                    for (i, n) in self.lib.tree_at(tid).nodes.iter().enumerate() {
                        println!("  {i}: op {} t {} f {} {:?}", n.opcode, n.true_target, n.false_target, n.operand);
                    }
                }
                None => println!("tree {t} of object {o}: not found"),
            }
        }
    }

    /// What each Sim on the lot shows, for the picture: a Sim walking (the stand-in's walk) is
    /// `Walking`; one whose Animate plays an animation, or whose interaction goes on after the
    /// last one ended (it holds the last pose), `Playing`; any other stands (`Idle`).
    pub fn motions(&self) -> Vec<(i16, Motion)> {
        let (host, ar) = (&self.d.host, self.d.host.anim.as_ref());
        let Some(t) = host.tally.as_ref() else { return Vec::new() };
        host.world
            .objects()
            .filter(|o| o.person && o.on_lot())
            .map(|o| {
                let id = o.id();
                let motion = if t.walks.contains_key(&id) {
                    Motion::Walking(t.walk_time.get(&id).copied().unwrap_or(0.0))
                } else {
                    let open = t.sims.get(&id).is_some_and(|s| !s.open.is_empty());
                    match ar.and_then(|a| a.m.play(alone_sim(id))) {
                        Some((anim, play)) if open || !play.answered() => Motion::Playing(anim, play.place(), play.backward()),
                        _ => Motion::Idle,
                    }
                };
                (id, motion)
            })
            .collect()
    }

    /// The catalog of the animations (a figure's walk and stand are found in it by name).
    pub fn catalog(&self) -> Option<&crate::anim::Catalog> {
        self.d.host.anim.as_ref().map(|a| &a.cat)
    }

    /// Whether the run keeps the scripts' sound requests for a front end (`take_sound_requests`).
    pub fn set_sound(&mut self, on: bool) {
        self.d.host.sound_out = on.then(Vec::new);
    }

    /// The sound requests made since the last call (PlaySound and KillSounds), in order.
    pub fn take_sound_requests(&mut self) -> Vec<super::sound::SoundRequest> {
        self.d.host.sound_out.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// The ticks run so far.
    pub fn ticks(&self) -> u32 {
        self.tick
    }

    pub fn world(&self) -> &World<'l> {
        &self.d.host.world
    }

    pub fn world_mut(&mut self) -> &mut World<'l> {
        &mut self.d.host.world
    }

    /// The tree ids and nodes of a Sim's stack, bottom first (for a front end that shows what a
    /// Sim is doing, and for checks).
    pub fn frames_of(&mut self, id: i16) -> Vec<(i32, u16)> {
        let lib = self.lib;
        let Some(k) = self.d.host.world.find(id) else { return Vec::new() };
        self.d.sim.stack_of(&self.d.host.world, k).stack().iter().map(|f| (f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node)).collect()
    }

    /// A player is there: a Dialog opens and waits for `answer_dialog`, instead of being answered at
    /// once with its first button.
    pub fn set_player(&mut self, on: bool) {
        self.d.host.player = on;
    }

    /// The dialog waiting for the player's answer, if one is.
    pub fn dialog(&self) -> Option<DialogAsk> {
        self.d.host.asked.map(|(object, operand)| DialogAsk { object, operand })
    }

    /// The text a dialog shows and its buttons' labels: the dialog object's STR# 301, the text at
    /// operand byte 2, a button at each of bytes 3 and 4 that is set, else one "OK".
    pub fn dialog_text(&self, ask: &DialogAsk) -> (String, Vec<String>) {
        let strings = self.d.host.world.find_object(ask.object).and_then(|o| o.file).and_then(|f| self.lib.file(f).strings.get(&301));
        let get = |i: u8| strings.and_then(|s| s.get(usize::from(i))).filter(|t| !t.is_empty()).cloned();
        let text = get(ask.operand[2]).unwrap_or_else(|| format!("(dialog {} of object {})", ask.operand[2], ask.object));
        let mut buttons: Vec<String> = [3, 4].iter().filter(|&&b| ask.operand[b] != 0).map(|&b| get(ask.operand[b]).unwrap_or_else(|| "...".to_string())).collect();
        if buttons.is_empty() {
            buttons.push("OK".to_string());
        }
        (text, buttons)
    }

    /// The player's answer to the open dialog: true for the first button. The object takes the
    /// primitive's true way at its next turn (`primitives::dialog`).
    pub fn answer_dialog(&mut self, first_button: bool) {
        if self.d.host.asked.is_some() {
            self.d.host.world.dialog_answer = Some(first_button);
        }
    }

    /// The player's command to a Sim (`World::queue_player_command`). False if the Sim, the target
    /// or the interaction is not found.
    pub fn queue_command(&mut self, sim: i16, target: i16, action: u16) -> bool {
        let Some(k) = self.d.host.world.find(sim) else { return false };
        self.d.host.world.queue_player_command(k, target, action)
    }

    /// An object the player's click makes (the "go here" marker) of the definition with `guid`, at
    /// a place (sixteenths) and level: made as CreateObjectInstance makes one, its init function
    /// (OBJf entry 0) run on it as a check would be, as the maker's step runs it. Its id.
    ///
    /// The checks run on it start with parameter 0 = 0 where every other target's start with 1: the
    /// marker's "Go here test" (4109) is false for 1 (a test of the parameter, then of the marker's
    /// attribute 0, to false) and passes for 0, and the original delivers the player's go-here
    /// (the club log's 4096 at 6593), so the original's check of such a click does not start with 1.
    /// A deliberate difference of the played run, for lockstep to skip (docs/plan.md, "The
    /// playable sandbox").
    pub fn make_object(&mut self, guid: u32, at: (i32, i32), level: i32) -> Option<i16> {
        let host = &mut self.d.host;
        let k = crate::primitives::make_of_guid(&mut host.world, guid)?;
        let o = host.world.get_mut(k);
        (o.x16, o.y16, o.level) = (at.0, at.1, level);
        let id = o.id();
        let file = o.file;
        host.player_targets.push(id);
        if let Some(init) = host.world.function_tree(k, 0) {
            host.me = k;
            host.run_check(init, file, id, &[]);
        }
        Some(id)
    }

    /// The interactions of `target`'s pie menu for `sim`, by the engine's checks as the original's
    /// menu runs them (docs/simulation.md, "The player's commands"): the table's first tree, then
    /// each interaction's guard in the table's order ("hide menu" taken as a no), and for a person
    /// "get social eligibility" first. The checks run as the Sim's, on the target.
    pub fn menu(&mut self, sim: i16, target: i16) -> Vec<MenuEntry> {
        let (lib, host) = (self.lib, &mut self.d.host);
        let (Some(sk), Some(t)) = (host.world.find(sim), host.world.find_object(target)) else { return Vec::new() };
        let (tfile, tperson, tid) = (t.file, t.person, t.id());
        let tab = lib.table_of(tfile, tperson, host.world.definition(t));
        let (Some(f), Some(table)) = (tab.file, tab.table) else { return Vec::new() };
        host.me = sk;
        let defaults = host.world.get(sk).temps.to_vec();
        let check = |host: &mut DriveHost<'l>, id: u16| -> bool {
            if id == 0 {
                return true;
            }
            let ok = lib.tree(tfile, id).and_then(|t| host.run_check(t, tfile, tid, &defaults)) == Some(true);
            ok && host.world.get(sk).data.get(crate::world::DATA_HIDE_INTERACTION).is_none_or(|&h| h == 0)
        };
        if (tperson && !check(host, SOCIAL_ELIGIBILITY)) || !check(host, tab.first) {
            return Vec::new();
        }
        let names = lib.file(f).pie_strings.get(&table);
        let mut out = Vec::new();
        for e in lib.table(f, table) {
            if check(host, e.guard) {
                let name = usize::try_from(e.string_index).ok().and_then(|i| names.and_then(|n| n.get(i))).cloned().unwrap_or_else(|| format!("interaction {}", e.action));
                out.push(MenuEntry { action: e.action, name });
            }
        }
        out
    }

    /// One tick: the animations' frames at the normal speed, then the simulation's pass.
    pub fn tick(&mut self) {
        self.tick += 1;
        if i64::from(self.tick) == super::study::env("REPLAY_ALONE_SHOW_TREE_AT").map_or(1, |v| super::study::atoi(&v)) {
            self.show_trees();
        }
        let d = &mut self.d;
        let t = self.tick;
        d.host.tick = t;
        // the animations' frames the tick follows, by the native clock at the normal speed
        self.frames = self.clock.frames_to_tick_at(1.0);
        if let Some(ar) = &mut d.host.anim {
            let frames = self.frames;
            for _ in 0..frames {
                ar.m.frame(|_| 1.0);
            }
            if let Some(tally) = &mut d.host.tally {
                let walkers: Vec<i16> = tally.walks.keys().copied().collect();
                tally.walk_time.retain(|id, _| walkers.contains(id));
                for id in walkers {
                    *tally.walk_time.entry(id).or_default() += frames as f32 * crate::routing::WALK_FRAME;
                }
            }
        }
        d.host.stopped = false;
        d.host.world.stop_pass = false;
        // the route followers' frames: all but the last of the tick's frames before it, the last
        // after it (the frame whose time makes the tick due runs the tick, then its followers)
        let frames = self.frames;
        d.host.walker_frames(frames.saturating_sub(1));
        d.sim.tick(&mut d.host);
        d.host.walker_frames(frames.min(1));
        d.host.world.clock_pass(d.host.stopped);
        watch(&mut d.host);
        if let Some(every) = super::study::env("REPLAY_ALONE_SHOW_SIMS").map(|v| super::study::atoi(&v) as u32)
            && every > 0
            && t.is_multiple_of(every)
        {
            // study: each Sim's stack every N ticks
            let lib = self.lib;
            let sims: Vec<i16> = d.host.world.objects().filter(|o| o.person).map(|o| o.id()).collect();
            for id in sims {
                let Some(k) = d.host.world.find(id) else { continue };
                let i = d.sim.stack_of(&d.host.world, k);
                let frames: Vec<String> = i.stack().iter().map(|f| format!("{}:{}", f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node)).collect();
                let top = i.stack().last().map(|f| (f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize)).map(|n| (n.opcode, n.operand)), f.params.clone(), f.locals.clone()));
                let (queue, motives) = (d.host.world.get(k).queue.len(), d.host.world.get(k).motives.iter().skip(5).map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
                println!("tick {t} sim {id}: {} {}{} queue {queue} motives {motives} deltas {:?} m9 {} top {top:?}", frames.join(" "), if i.asleep() { "asleep " } else { "" }, if i.done() { "DONE" } else { "" }, d.host.world.get(k).motive_deltas, d.host.world.get(k).motives[9]);
            }
        }
    }

    /// The world's hash (as `--alone` prints it).
    pub fn hash(&mut self) -> u64 {
        world_hash(&mut self.d)
    }

    /// The run's figures printed, as `--alone` ends: `of` the ticks asked.
    pub fn report(mut self, of: u32) -> AloneReport {
        let (done, lib) = (self.tick, self.lib);
        let hash = world_hash(&mut self.d);
        let mut d = self.d;
        let t = d.host.tally.take().unwrap_or_default();
        let neutral_total: u32 = t.neutral.values().sum();
        let error_total: u32 = t.errors.values().sum();
        println!("alone: ticks {done} of {of}, errors {error_total}, neutral answers {neutral_total}");
        let mut kinds: Vec<(u32, String)> = t.neutral.iter().map(|(&(op, why, tree), &n)| (n, format!("neutral {}: {why}, in tree {tree}", primitive_name(op)))).collect();
        kinds.extend(t.errors.iter().map(|(k, &n)| (n, k.clone())));
        kinds.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (n, k) in kinds.iter().take(40) {
            println!("  {n:>8}  {k}");
        }
        println!("alone: motives shown are the table's 5 to 15 (5 energy, 6 comfort, 7 hunger, 8 hygiene, 9 bladder, 11 sleep state, 14 social, 15 fun; 0 to 4 are no motives)");
        for (id, s) in &t.sims {
            let name = d.host.world.find_object(*id).and_then(|o| o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied()).and_then(|n| d.host.world.neighbours.iter().find(|x| x.id == n)).map_or(String::new(), |n| n.name.clone());
            let f = |v: &[f32]| v.iter().skip(5).take(11).map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" ");
            let at = d.host.world.find(*id).map_or("gone".to_string(), |k| {
                let i = d.sim.stack_of(&d.host.world, k);
                let top = i.stack().last().map_or((-1, 0), |fr| (fr.tree.map_or(-1, |t| lib.tree_at(t).id as i32), fr.node));
                format!("stack {} deep at {}:{}{}{}", i.stack().len(), top.0, top.1, if i.asleep() { " asleep" } else { "" }, if i.done() { " DONE" } else { "" })
            });
            println!("sim {id} {name}: interactions started {} finished {}, walked {:.1} tiles, motives low [{}] last [{}], {at}", s.started, s.finished, s.walked, f(&s.low), f(&s.last));
        }
        println!("alone: walks planned {}, arrived {}, no way {}; Sim stacks restarted after an error {}", t.planned, t.arrived, t.no_way, t.restarted);
        println!("alone: walks to a place outside its object's room {}", t.outside_room);
        println!("alone: walk legs through a wall {}", t.through_wall);
        println!("alone: hash {hash:016x}");
        AloneReport { ticks: done, of, hash }
    }
}

/// The run alone, `ticks` ticks from the loaded start, printing its figures.
pub fn alone<'l>(log: &crate::tree_log::Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], with: RunWith, run: AloneRun) -> Option<AloneReport> {
    let AloneRun { ticks, seed } = run;
    let mut a = Alone::new(log, lib, lot, globals, neighbours, with, seed)?;
    for _ in 0..ticks {
        a.tick();
    }
    Some(a.report(ticks))
}

impl DriveHost<'_> {
    /// The autonomy evaluation (opcode 3) of a run alone: the candidates' checks run natively,
    /// each passing one valued, the list sorted and a pick from it with the world's own
    /// generator (`autonomy_list`, `autonomy_pick`: the original takes entry `random % 4`); the
    /// stack object is the chosen object and the action is kept for Find Best Action (30). With
    /// nothing chosen the stack object stays.
    pub(super) fn evaluate(&mut self, interp: &mut crate::interp::Interpreter) -> crate::interp::PrimResult {
        use crate::autonomy::{Valued, autonomy_candidates, autonomy_list, autonomy_pick, candidate_value, interaction_score, run_evaluation, score_curves};
        let (me, lib) = (self.me, self.lib);
        let cands = autonomy_candidates(&self.world, me);
        let defaults = interp.defaults().to_vec();
        let checks = run_evaluation(lib, self, &cands, &defaults);
        let curves = score_curves(lib);
        let person = self.world.get(me);
        let base = interaction_score(&curves, None, person, &person.motives);
        let mut values: Vec<Valued> = Vec::new();
        for c in checks.iter().filter(|c| c.passed == Some(true)) {
            let Some(ci) = c.candidate else { continue };
            let cand = &cands[ci];
            let mut adverts = cand.interaction(lib).adverts.to_vec();
            for (m, a) in adverts.iter_mut().enumerate() {
                if let Some(&d) = c.ad_range.get(m) {
                    a.delta = d;
                }
            }
            let value = candidate_value(lib, &curves, person, self.world.get(cand.object), cand, &adverts, base);
            values.push(Valued { value, candidate: ci });
        }
        let visitor = person.person_data.get(32).is_some_and(|&v| v != 0);
        if super::study::has("REPLAY_ALONE_SHOW_EVAL") {
            println!("evaluation by {} at tick {}: {} candidates ({} listed), {} checks, {} passed, {} valued, base {base}, pd32 {:?} pd36 {:?}", self.world.get(me).id(), self.tick, cands.len(), cands.iter().filter(|c| c.listed).count(), checks.len(), checks.iter().filter(|c| c.passed == Some(true)).count(), values.len(), self.world.get(me).person_data.get(32), self.world.get(me).person_data.get(36));
        }
        let list = autonomy_list(lib, values, visitor);
        let random = self.world.rng.next_u32();
        self.chosen = None;
        if let Some(at) = autonomy_pick(lib, &list, random) {
            let c = &cands[list[at].candidate];
            interp.top().stack_object = c.id;
            self.chosen = lib.tree(c.object_file, c.interaction(lib).action).map(|t| (c.id, t, c.object_file));
        }
        crate::interp::PrimResult::True
    }
}

impl DriveHost<'_> {
    /// LookTowards (22), mode 2 (the only one the logs use): the Sim turns to face the stack
    /// object at the walker's turn rate (`turn_frames`, in the frames of the original's clock:
    /// 33 ms a frame, 42 a tick), yielding until it has, then true. Other modes: neutral.
    pub(super) fn look_towards(&mut self, interp: &mut crate::interp::Interpreter, n: &crate::bhav::Node) -> crate::interp::PrimResult {
        use crate::interp::PrimResult;
        let me = self.me;
        let id = self.world.get(me).id();
        if n.operand16(0) != 2 {
            return self.neutral(n, "mode not modelled");
        }
        let t = self.tally.as_mut().expect("alone");
        if let Some(turn) = t.turns.get_mut(&id) {
            if turn.0 > 0 {
                turn.0 -= 1;
                return PrimResult::Yield;
            }
            let dir = turn.1;
            t.turns.remove(&id);
            if let Some(v) = self.world.get_mut(me).data.get_mut(crate::world::DATA_FACING) {
                *v = dir;
            }
            return PrimResult::True;
        }
        let (me_o, so) = (self.world.get(me), self.world.find_object(interp.top().stack_object));
        let Some(so) = so else { return PrimResult::False };
        let (dx, dy) = ((so.x16 - me_o.x16) as f32, (so.y16 - me_o.y16) as f32);
        if dx == 0.0 && dy == 0.0 {
            return PrimResult::True;
        }
        let (from, to) = (crate::routing::direction_angle(me_o.facing()), f32::atan2(-dx, dy));
        let diff = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let ticks = (crate::routing::turn_frames(diff.abs()) as f32 * crate::routing::FRAME_MS / crate::routing::TICK_MS).ceil() as u32;
        t.turns.insert(id, (ticks, direction_of(dx, dy)));
        PrimResult::Yield
    }
}
