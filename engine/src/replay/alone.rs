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

use crate::library::Library;
use crate::save::{LotChunks, Neighbour};
use crate::world::ObjectKey;
use super::drive::{DriveHost, Driven, setup};
use super::start::RunWith;

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

/// What a run alone counts.
#[derive(Default)]
pub struct Tally {
    neutral: BTreeMap<(u16, &'static str, i32), u32>,
    errors: BTreeMap<String, u32>,
    sims: BTreeMap<i16, SimStats>,
    /// The objects waiting at a Dialog nobody answers.
    pub(super) waiting: std::collections::BTreeSet<i16>,
    /// Stacks of Sims restarted at their bottom frame after an error.
    pub(super) restarted: u32,
    /// The turns going on (LookTowards): the ticks left and the direction it ends in.
    turns: BTreeMap<i16, (u32, i16)>,
    /// The walks going on, by the walking object.
    pub(super) walks: BTreeMap<i16, Walk>,
    /// Walks planned, arrived, and asked for with no way.
    pub(super) planned: u32,
    pub(super) arrived: u32,
    pub(super) no_way: u32,
}

/// One walk: the points of its line in sixteenths and the one it goes to; where it ends it
/// faces `face`.
pub(super) struct Walk {
    path: Vec<(f32, f32)>,
    to: usize,
    at: (f32, f32),
    face: Option<(i32, i32)>,
}

/// A walker's step a tick, in sixteenths: its top a second over the tick's time (natively a
/// constant speed: no ramp and no frames; the replay's walker is the log's).
const STEP_PER_TICK: f32 = crate::routing::WALK_TOP_ADULT * crate::routing::TICK_TIME;

/// The direction (object data 1) of a step.
fn direction_of(dx: f32, dy: f32) -> i16 {
    crate::routing::facing_direction(f32::atan2(-dx, dy))
}

impl DriveHost<'_> {
    /// GotoRelativePosition (27) and GotoRoutingSlot (45) of a run alone: the walk planned over
    /// the lot's tiles (`Walls::route`: open edges, doors' openings; no object blocks and no
    /// door's portal function is pushed), the Sim stepping along its line a tick at a time and
    /// yielding until it is there. 45: the nearest of the stack object's slot's places by route;
    /// 27: the tile in front of the stack object (its operand is not read).
    pub(super) fn go(&mut self, interp: &mut crate::interp::Interpreter, n: &crate::bhav::Node) -> crate::interp::PrimResult {
        use crate::interp::PrimResult;
        let (me, lib) = (self.me, self.lib);
        let id = self.world.get(me).id();
        let so = interp.top().stack_object;
        if !self.tally.as_ref().is_some_and(|t| t.walks.contains_key(&id)) {
            let o = self.world.get(me);
            if !o.on_lot() {
                return PrimResult::False;
            }
            let from = (o.x16, o.y16);
            let target = self.world.find_object(so).map(|t| (t.x16, t.y16));
            let goals: Vec<(i32, i32)> = match self.world.find_object(so) {
                None => return PrimResult::True,
                Some(t) if n.opcode == 45 => {
                    let slot = t.file.and_then(|file| lib.routing_slot(file, self.world.definition(t), n.operand16(1), n.operand16(0), &interp.top().params));
                    match slot {
                        // (a slot that names no direction, the club's dance floor: its object's own place)
                        Some(slot) => match crate::routing::slot_places(slot, t.x16, t.y16, t.facing()) {
                            places if places.is_empty() => vec![(t.x16, t.y16)],
                            places => places.into_iter().map(|p| p.1).collect(),
                        },
                        None => return PrimResult::True,
                    }
                }
                Some(t) => {
                    let d = crate::routing::front(t.facing()).unwrap_or((0, 0));
                    vec![(t.x16 + d.0 * 16, t.y16 + d.1 * 16)]
                }
            };
            let tile = |p: (i32, i32)| (p.0 >> 4, p.1 >> 4);
            if super::study::has("REPLAY_ALONE_SHOW_NO_WAY") {
                println!("goals of object {id} tick {}: {goals:?} (me {from:?} room {}, size {:?})", self.tick, self.world.walls.room(from.0 >> 4, from.1 >> 4), self.world.walls.size());
            }
            let mut best: Option<(Vec<crate::routing::Tile>, (i32, i32))> = None;
            for g in goals {
                if g.0 < 0 || g.1 < 0 {
                    continue;
                }
                let route = if tile(g) == tile(from) { vec![tile(g)] } else { self.world.walls.route(tile(from), tile(g), None) };
                if !route.is_empty() && best.as_ref().is_none_or(|b| route.len() < b.0.len()) {
                    best = Some((route, g));
                }
            }
            let t = self.tally.as_mut().expect("alone");
            let Some((route, goal)) = best else {
                t.no_way += 1;
                if super::study::has("REPLAY_ALONE_SHOW_NO_WAY") {
                    println!("no way: tick {} object {id} at {from:?} (tile {:?}) to object {so} opcode {}", self.tick, tile(from), n.opcode);
                }
                return PrimResult::False;
            };
            t.planned += 1;
            let mut path: Vec<(f32, f32)> = route.iter().skip(1).map(|&(x, y)| (x as f32 * 16.0 + 8.0, y as f32 * 16.0 + 8.0)).collect();
            path.push((goal.0 as f32, goal.1 as f32));
            t.walks.insert(id, Walk { path, to: 0, at: (from.0 as f32, from.1 as f32), face: target });
        }
        let t = self.tally.as_mut().expect("alone");
        let w = t.walks.get_mut(&id).expect("planned");
        let mut left = STEP_PER_TICK;
        let mut dir = None;
        while left > 0.0 && w.to < w.path.len() {
            let goal = w.path[w.to];
            let (dx, dy) = (goal.0 - w.at.0, goal.1 - w.at.1);
            let dist = f32::hypot(dx, dy);
            if dist <= left {
                w.at = goal;
                left -= dist;
                w.to += 1;
            } else {
                w.at = (w.at.0 + dx / dist * left, w.at.1 + dy / dist * left);
                left = 0.0;
            }
            if dist > 0.0 {
                dir = Some(direction_of(dx, dy));
            }
        }
        let (x, y, arrived, face) = (w.at.0.round() as i32, w.at.1.round() as i32, w.to >= w.path.len(), w.face);
        if arrived {
            t.walks.remove(&id);
            t.arrived += 1;
        }
        let dir = if arrived { face.filter(|f| *f != (x, y)).map(|f| direction_of((f.0 - x) as f32, (f.1 - y) as f32)).or(dir) } else { dir };
        let o = self.world.get_mut(me);
        o.x16 = x;
        o.y16 = y;
        if let (Some(d), Some(v)) = (dir, o.data.get_mut(crate::world::DATA_FACING)) {
            *v = d;
        }
        if arrived { PrimResult::True } else { PrimResult::Yield }
    }
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

/// The run alone, `ticks` ticks from the loaded start, printing its figures.
pub fn alone<'l>(log: &crate::tree_log::Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], with: RunWith, run: AloneRun) -> Option<AloneReport> {
    let AloneRun { ticks, seed } = run;
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
    let mut done = 0;
    let mut clock = crate::routing::FrameClock::default();
    for t in 1..=ticks {
        d.host.tick = t;
        // the animations' frames the tick follows, by the native clock at the normal speed
        if let Some(ar) = &mut d.host.anim {
            for _ in 0..clock.frames_to_tick_at(1.0) {
                ar.m.frame(|_| 1.0);
            }
        }
        d.host.stopped = false;
        d.host.world.stop_pass = false;
        d.sim.tick(&mut d.host);
        d.host.world.clock_pass(d.host.stopped);
        watch(&mut d.host);
        if let Some(every) = super::study::env("REPLAY_ALONE_SHOW_SIMS").map(|v| super::study::atoi(&v) as u32)
            && every > 0
            && t % every == 0
        {
            // study: each Sim's stack every N ticks
            let sims: Vec<i16> = d.host.world.objects().filter(|o| o.person).map(|o| o.id()).collect();
            for id in sims {
                let Some(k) = d.host.world.find(id) else { continue };
                let i = d.sim.stack_of(&d.host.world, k);
                let frames: Vec<String> = i.stack().iter().map(|f| format!("{}:{}", f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node)).collect();
                let top = i.stack().last().map(|f| (f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize)).map(|n| (n.opcode, n.operand)), f.params.clone(), f.locals.clone()));
                println!("tick {t} sim {id}: {} {}{} top {top:?}", frames.join(" "), if i.asleep() { "asleep " } else { "" }, if i.done() { "DONE" } else { "" });
            }
        }
        done = t;
    }
    let hash = world_hash(&mut d);
    let t = d.host.tally.take().unwrap_or_default();
    let neutral_total: u32 = t.neutral.values().sum();
    let error_total: u32 = t.errors.values().sum();
    println!("alone: ticks {done} of {ticks}, errors {error_total}, neutral answers {neutral_total}");
    let mut kinds: Vec<(u32, String)> = t.neutral.iter().map(|(&(op, why, tree), &n)| (n, format!("neutral {}: {why}, in tree {tree}", primitive_name(op)))).collect();
    kinds.extend(t.errors.iter().map(|(k, &n)| (n, k.clone())));
    kinds.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (n, k) in kinds.iter().take(15) {
        println!("  {n:>8}  {k}");
    }
    for (id, s) in &t.sims {
        let name = d.host.world.find_object(*id).and_then(|o| o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied()).and_then(|n| d.host.world.neighbours.iter().find(|x| x.id == n)).map_or(String::new(), |n| n.name.clone());
        let f = |v: &[f32]| v.iter().take(8).map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" ");
        let at = d.host.world.find(*id).map_or("gone".to_string(), |k| {
            let i = d.sim.stack_of(&d.host.world, k);
            let top = i.stack().last().map_or((-1, 0), |fr| (fr.tree.map_or(-1, |t| lib.tree_at(t).id as i32), fr.node));
            format!("stack {} deep at {}:{}{}{}", i.stack().len(), top.0, top.1, if i.asleep() { " asleep" } else { "" }, if i.done() { " DONE" } else { "" })
        });
        println!("sim {id} {name}: interactions started {} finished {}, walked {:.1} tiles, motives low [{}] last [{}], {at}", s.started, s.finished, s.walked, f(&s.low), f(&s.last));
    }
    println!("alone: walks planned {}, arrived {}, no way {}; Sim stacks restarted after an error {}; objects waiting at a Dialog {}", t.planned, t.arrived, t.no_way, t.restarted, t.waiting.len());
    println!("alone: hash {hash:016x}");
    Some(AloneReport { ticks: done, of: ticks, hash })
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
