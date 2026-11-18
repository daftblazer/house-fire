use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Instant;

use crate::library::Library;
use crate::save::{LotChunks, Neighbour, read_lot_arrays};
use crate::simulator::{Simulator};
use crate::tree_log::{Log, PlanRec};
use crate::world::{ObjectKey, World};
use crate::{FileId, TreeId};
use super::study::{env, has, atoi, file_name};
use super::counts::{Counts, Bench, Shown, us};
use super::finder::{TreeFinder};
use super::host::{ReplayHost};
use super::start::RunWith;
use super::animate::AnimReplay;
use super::stacks::{stack_ids, place_quiet_stacks};
use super::walks::{native_plan_points, Heading};
use super::routing::routing;
use super::clock::{check_time_of_day};

/// The run natively, one tick at a time (`step`): the simulator and the host (the world and what
/// the replay keeps between steps), and the checks' state.
pub struct Replay<'l> {
    pub(super) lib: &'l Library,
    pub(super) log: Rc<Log>,
    /// The lot as it loads: the club's and the disc's copy of its house.
    pub(super) lot: (&'l LotChunks, Option<&'l LotChunks>),
    pub(super) h: ReplayHost<'l>,
    pub(super) consumed: BTreeSet<usize>,
    pub(super) sim: Simulator,
    pub(super) finder: TreeFinder<'l>,
    pub(super) unstarted: BTreeSet<ObjectKey>,
    pub(super) ids: BTreeMap<u32, i16>,
    pub(super) native_places: BTreeMap<(i16, u32), (i32, i32)>,
    pub(super) use_native_places: bool,
    pub(super) by_tick: Vec<Vec<usize>>,
    pub(super) frames: BTreeMap<u32, Vec<u32>>,
    pub(super) first_seen: BTreeMap<u32, u32>,
    pub(super) first_step: BTreeSet<u32>,
    pub(super) last_frame: HashMap<u32, (u32, TreeId)>,
    pub(super) unseen: BTreeSet<u32>,
    pub(super) delivered: BTreeSet<u32>,
    pub(super) frame_tree: HashMap<(u32, u32, u16), (TreeId, FileId)>,
    pub(super) new_observer: bool,
    pub(super) streak: usize,
    pub(super) best_streak: usize,
    pub(super) have_passes: bool,
    pub(super) engine_run: Vec<bool>,
    pub(super) place_at: usize,
    pub(super) motive_at: usize,
    pub(super) shown_motives: usize,
    pub(super) ev_env: Option<String>,
    pub(super) engine_list_at: usize,
    pub(super) time_at: usize,
    /// The route followers' facing (`Heading`), by follower, and how far into the log's steps of them the run is.
    pub(super) headings: HashMap<u32, Heading>,
    pub(super) heading_at: usize,
    pub(super) t: u32,
}

/// The run as `Replay` makes it, to its end.
/// One run of the club over the log: with bench, timed and not counted. The counts, or `None`
/// if the log does not start with the lot loaded.
/// `lot`: the lot as loaded, and the disc's copy of it (its house) if a save is loaded.
pub fn run<'l>(log: &Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], bench: Option<Bench>, with: RunWith) -> Option<(Counts, Option<Bench>)> {
    let mut run = Replay::new(log, lib, lot, globals, neighbours, bench, with)?;
    while run.step() {}
    Some(run.finish())
}

impl<'l> Replay<'l> {
    /// Loads the lot and gets the run ready to step (`step`); `None` when the log does not start
    /// with the lot loaded.
    pub fn new(log: &Log, lib: &'l Library, lot: (&'l LotChunks, Option<&'l LotChunks>), globals: &[i16], neighbours: &[Neighbour], bench: Option<Bench>, with: RunWith) -> Option<Replay<'l>> {
        let RunWith { card_lot, goals, neighbourhood, cat } = with;
        let (club, house) = lot;
        let mut world = World::new(lib);
        world.load_lot(club).expect("the lot");
        let took_house = house.is_some_and(|h| world.take_house_from(h).unwrap_or(false));
        if has("REPLAY_SHOW_LOW_TREES") {
            for f in lib.files().iter().filter(|f| f.path.to_ascii_lowercase().contains("global")) {
                let t462 = f.trees.get(&462).map(|&t| lib.tree_at(t).nodes.first().map(|n| (n.opcode, n.operand)));
                println!("file {}: {} trees, 462 node 0 {t462:?}", f.path, f.trees.len());
            }
            let files: BTreeSet<FileId> = world.objects().filter_map(|o| o.file).collect();
            for f in files {
                let low: Vec<u16> = lib.file(f).trees.keys().copied().filter(|&id| id < 0x1000).collect();
                if !low.is_empty() {
                    println!("low trees in {}: {low:?}", lib.file(f).path);
                }
            }
        }
        if has("REPLAY_SHOW_SLOTTED") {
            for id in [197i16, 62] {
                if let Some(o) = world.find_object(id) {
                    println!("loaded {id}: person {} container {} slot {} at ({},{},{}) guid {:08x}", o.person, o.data[2], o.data[3], o.x16 >> 4, o.y16 >> 4, o.level, o.guid);
                }
            }
            for o in world.objects() {
                if o.data.get(3).is_some_and(|&s| s >= 0) && o.data.get(2) == Some(&0) {
                    let t = (o.x16 >> 4, o.y16 >> 4, o.level);
                    let mates: Vec<String> = world.objects().filter(|x| x.id() != o.id() && (x.x16 >> 4, x.y16 >> 4, x.level) == t).map(|x| format!("{}{}", x.id(), if x.person { "P" } else { "" })).collect();
                    println!("slotted, no container: {}{} slot {} at {t:?} x16 {} y16 {}; on its tile {mates:?}", o.id(), if o.person { "P" } else { "" }, o.data[3], o.x16, o.y16);
                }
            }
        }
        // the lot's globals as saved (its SIMI): the clock among them
        world.globals = globals.to_vec();
        if let Some(id) = env("REPLAY_SHOW_STACK").and_then(|v| v.parse::<i16>().ok())
            && let Some(o) = world.find_object(id)
        {
            {
                println!("file {:?} semi {:?}", o.file.map(|f| file_name(lib, f)), o.file.and_then(|f| lib.file(f).semi).map(|s| file_name(lib, s)));
                for f in &o.stack {
                    if let Some(t) = f.tree {
                        let tr = lib.tree_at(t);
                        println!("  frame {} node {}: node op {:?} stack object {}", tr.id, f.node, tr.nodes.get(f.node as usize).map(|n| (n.opcode, n.true_target, n.false_target)), f.stack_object);
                    }
                }
                println!("stack of {id}: {:?}", o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.file.map(|x| file_name(lib, x)))).collect::<Vec<_>>());
            }
        }
        if has("REPLAY_SHOW_REQUEUE") {
            for o in world.objects().filter(|o| o.person) {
                let diff: Vec<(usize, i16)> = o.person_data.iter().enumerate().filter(|(i, _)| *i < 80).map(|(i, &v)| (i, v)).collect();
                println!("requeue data {}: {:?}", o.id(), diff);
                println!("requeue entry {}: {:?}", o.id(), o.current);
                println!("requeue: {} current tree {} on {:?} stack {:?} pd32 {:?} pd29 {:?} data4 {:?} saved queue {}", o.id(), o.current.tree, o.current.objects, o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node)).collect::<Vec<_>>(), o.person_data.get(32), o.person_data.get(29), o.data.get(4), o.queue.len());
            }
        }
        world.requeue_current_on_community_lot();
        // The plans' points natively wherever the log carries the planner's inputs ('r', 'h'), so the
        // walk checks run on the native planner's ways (study REPLAY_LOGGED_PLANS: the log's instead)
        let mut native_plans = (0, 0);
        // --bench: the native planner's cost, each plan's whole request (goal choice, search,
        // refinement, the point after the goal) timed 21 times, the rounds interleaved over the plans
        // (a busy PC), each plan's median
        if bench.is_some() {
            let plans: Vec<&PlanRec> = log.plans.iter().filter(|pl| native_plan_points(pl, &world).is_some()).collect();
            let mut times: Vec<Vec<f64>> = vec![Vec::new(); plans.len()];
            for _ in 0..21 {
                for (k, pl) in plans.iter().enumerate() {
                    let t0 = Instant::now();
                    std::hint::black_box(native_plan_points(pl, &world));
                    times[k].push(us(t0));
                }
            }
            let mut medians: Vec<(f64, usize)> = times
                .iter_mut()
                .enumerate()
                .map(|(k, t)| {
                    t.sort_by(f64::total_cmp);
                    (t[t.len() / 2], k)
                })
                .collect();
            medians.sort_by(|a, b| a.0.total_cmp(&b.0));
            let total: f64 = medians.iter().map(|m| m.0).sum();
            if let Some(&(worst, k)) = medians.last() {
                let pl = plans[k];
                println!(
                    "bench: the planner on {} plans, us a plan (medians of 21): median {:.1}, worst {:.1} (tick {}, {} goals, {} points), all {:.1}",
                    plans.len(),
                    medians[medians.len() / 2].0,
                    worst,
                    pl.tick,
                    pl.goals.len(),
                    pl.points.len(),
                    total
                );
            }
        }
        let log_rc = if has("REPLAY_LOGGED_PLANS") {
            Rc::new(log.clone())
        } else {
            let mut l = log.clone();
            for pl in &mut l.plans {
                if let Some(p) = native_plan_points(pl, &world) {
                    native_plans.1 += usize::from(p == pl.points);
                    if p != pl.points && has("REPLAY_SHOW_PLAN_DIFFS") {
                        let a = pl.approach;
                        let facing = a.and_then(|a| world.find_object(a[5] as i16)).and_then(|o| o.data.get(1)).copied();
                        let sim = a.and_then(|a| world.find_object(a[7] as i16)).map(|o| (o.person, o.facing(), o.data.get(1).copied()));
                        let place = a.and_then(|a| log.places.iter().rfind(|q| q.id == a[7] as i16 && q.tick < pl.tick).map(|q| (q.x, q.y, q.level)));
                        println!("plan tick {}: native {p:?}, the log's {:?}; approach {a:?} chosen {} facing {facing:?} sim {sim:?} place {place:?}", pl.tick, pl.points, pl.chosen);
                    }
                    native_plans.0 += 1;
                    pl.points = p;
                }
            }
            Rc::new(l)
        };
        let log: &Log = &log_rc;
        if has("REPLAY_SHOW_SIMI") {
            println!("lot globals: {:?}", &globals[..globals.len().min(40)]);
        }
        if has("REPLAY_SHOW_RELATIONSHIPS") {
            for o in world.objects().filter(|o| !o.relationships.is_empty()) {
                for r in &o.relationships {
                    println!("relationship of {} with {}: {:?}", o.id(), r.with, r.values);
                }
            }
        }
        if has("REPLAY_SHOW_SAVED_STACKS") {
            for o in world.objects().filter(|o| o.person || !o.stack.is_empty() && env("REPLAY_SHOW_SAVED_STACKS").is_some_and(|v| v == "all")) {
                let fr: Vec<_> = o.stack.iter().map(|f| (f.tree.map(|t| lib.tree_at(t).id), f.node, f.stack_object, f.engine, f.params.clone())).collect();
                println!("saved stack {}: {fr:?}", o.id());
            }
        }
        if has("REPLAY_SHOW_OBJECTS") {
            for o in world.objects() {
                let mut l = format!("object {}: guid {:08x}, {} at {},{} facing {}; data", o.id(), o.guid, o.file.map_or("-".to_string(), |f| lib.file(f).path.clone()), o.x16, o.y16, o.data[1]);
                for v in &o.data {
                    l += &format!(" {v}");
                }
                println!("{l}");
            }
        }
        if has("REPLAY_SHOW_PAVED") {
            println!("paved regions natively: {:?}", world.walls.paved_regions());
        }
        if has("REPLAY_SHOW_WALL_BYTES") {
            // the second array's bytes (each tile's element) round the front yard, for study
            if let Ok(arrays) = read_lot_arrays(club) {
                for (n, a) in arrays.iter().enumerate() {
                    let es = a.element_size as usize;
                    println!("array {n}: {}x{}, {es} bytes a tile", a.width, a.height);
                    for y in 5..16 {
                        let row: Vec<String> = (15..25).map(|x| {
                            let i = (y * a.width + x) as usize * es;
                            a.bytes[i..i + es].iter().map(|b| format!("{b:02x}")).collect::<String>()
                        }).collect();
                        println!("  y {y:2}: {}", row.join(" "));
                    }
                }
            }
        }
        if let Some(id) = env("REPLAY_DUMP_TREE").and_then(|v| v.parse::<u16>().ok()) {
            for f in lib.files() {
                if let Some(&t) = f.trees.get(&id) {
                    println!("tree {id} in {}:", f.path);
                    for (n, node) in lib.tree_at(t).nodes.iter().enumerate() {
                        println!("  {n}: op {} -> {}/{} operand {:?}", node.opcode, node.true_target, node.false_target, &node.operand[..8]);
                    }
                }
            }
        }
        if let Some(v) = env("REPLAY_SHOW_STACK_DUMPS") {
            // study: the interpreters' first-step dumps ('k') of those whose first step is this tree
            println!("log steps: {}; dumps {}; first sims {:x?}", log.steps.len(), log.stack_dumps.len(), log.stack_dumps.iter().take(3).map(|d| (d.tick, d.sim, d.frame)).collect::<Vec<_>>());
            let want = atoi(&v) as u16;
            // or the interpreter of the step whose frame's low half is `want`
            let by_frame: Option<u32> = log.steps.iter().find(|x| x.frame & 0xFFFF == want as u32).map(|x| x.sim);
            if let Some(sm) = by_frame {
                println!("the step with frame ..{want:04x}: sim {sm:08x}; its dump {}", log.stack_dumps.iter().any(|d| d.sim == sm));
            }
            for d in &log.stack_dumps {
                let (tick, sim, frame, words, below) = (&d.tick, &d.sim, &d.frame, &d.words, &d.below);
                if by_frame.is_some_and(|sm| sm == *sim) {
                    println!("dump tick {tick} sim {sim:08x} frame {frame:08x}; words {words:08x?}");
                    for row in below.chunks(32).skip(8) {
                        println!("  {:02x?}", row);
                    }
                }
                let first = log.steps.iter().find(|x| x.sim == *sim);
                if has("REPLAY_SHOW_STACK_DUMP_FIRSTS") {
                    println!("dump sim {sim:08x} frame {frame:08x} first {:?}", first.map(|x| (x.tick, x.tree, x.node, x.frame)));
                }
                let Some(first) = first else { continue };
                if first.tree != want && first.frame & 0xFFFF != want as u32 {
                    continue;
                }
                println!("dump tick {tick} sim {sim:08x} frame {frame:08x} first {}:{}; words {:08x?}", first.tree, first.node, words);
                for row in below.chunks(32).skip(8) {
                    println!("  {:02x?}", row);
                }
            }
        }
        if let Some(pat) = env("REPLAY_SHOW_FILE") {
            // study: a file's function table and tree labels
            for f in lib.files().iter().filter(|f| f.path.contains(&pat)) {
                println!("file {}: functions {:?}; semi {:?}", f.path, f.functions, f.semi.map(|s| &lib.file(s).path));
                for (id, sl) in &f.slots {
                    println!("  slots {id}: {sl:?}");
                }
                for (id, t) in &f.tables {
                    println!("  table {id}: first {:?}; (action, guard, flags) {:?}", f.table_first.get(id), t.iter().map(|e| (e.action, e.guard, e.flags)).collect::<Vec<_>>());
                }
                for d in &f.definitions {
                    let hits: Vec<(usize, u16)> = (0..80).map(|i| (i, d.field(i))).filter(|(_, v)| f.functions.contains_key(v)).collect();
                    println!("  def {:08x}: fields naming a function table {hits:?}", d.guid());
                }
                let mut names: Vec<_> = f.tree_by_name.iter().map(|(n, id)| (*id, n.clone())).collect();
                names.sort();
                for (id, n) in names {
                    println!("  tree {id} {n:?}");
                }
            }
        }
        if let Some(pat) = env("REPLAY_TREE_NAMES") {
            // study: the BHAV labels matching a pattern, by file
            for f in lib.files() {
                for (name, id) in &f.tree_by_name {
                    if name.to_lowercase().contains(&pat.to_lowercase()) {
                        println!("tree name: {} {id} {name:?}", f.path);
                    }
                }
            }
        }
        if has("REPLAY_SHOW_PERSON_TABLE") {
            for f in lib.files().iter().filter(|f| f.path.ends_with("person_sg.iff")) {
                for (&id, t) in &f.tables {
                    println!("person table {id}: first {:?}; guards {:?}", f.table_first.get(&id), t.iter().map(|e| (e.guard, e.autonomy_threshold, e.flags)).collect::<Vec<_>>());
                }
            }
        }
        if let Some(dump) = env("REPLAY_DUMP_ARRAYS") {
            // the lot's three arrays, raw, for study
            let bytes: Vec<u8> = read_lot_arrays(club).unwrap_or_default().iter().flat_map(|a| a.bytes.iter().copied()).collect();
            let _ = std::fs::write(dump, bytes);
        }
        // The disc's club: its motives do not move by themselves (measured; World::motives_run).
        world.motives_run = card_lot || has("REPLAY_MOTIVES_RUN");
        world.neighbours = neighbours.to_vec();
        world.describe_persons();
        if has("REPLAY_NEIGHBOUR_MOTIVES") {
            world.take_neighbour_motives();
        }
        world.goals = goals;
        world.neighbourhood = neighbourhood;
        if has("REPLAY_SHOW_RELS") {
            // study: the relationships' value counts, the objects' and the neighbours'
            for k in world.order().to_vec() {
                let o = world.get(k);
                for r in &o.relationships {
                    println!("rel object {} -> {}: {} values {:?}", o.id(), r.with, r.values.len(), r.values);
                }
            }
            for x in &world.neighbours {
                for r in &x.relationships {
                    println!("rel neighbour {} -> {}: {} values {:?}", x.id, r.with, r.values.len(), r.values);
                }
            }
        }
        if log.lists.first().is_none_or(|l| l.ids.len() != world.len()) {
            println!(
                "the log does not start with the lot loaded ({} objects in its first list, {} in the lot)",
                log.lists.first().map_or(0, |l| l.ids.len()),
                world.len()
            );
            return None;
        }
        let quiet = bench.is_some();
        let mut h = ReplayHost {
            lib,
            world,
            log: Rc::clone(&log_rc),
            me: ObjectKey(0),
            step: 0,
            looks: HashMap::new(),
            bench,
            file_of_top: None,
            nested: Vec::new(),
            nested_pos: 0,
            natively_delivered: BTreeSet::new(),
            present: BTreeSet::new(),
            deferred: BTreeSet::new(),
            chosen: None,
            score_at: 0,
            on_check: None,
            counts: Counts { quiet, ..Default::default() },
            shown: Shown::default(),
            now: 0,
            routing_returns: BTreeMap::new(),
            pre_made: BTreeSet::new(),
            anim: None,
        };
        h.anim = cat.and_then(|c| AnimReplay::new(log, lib, c, &mut h.counts));
        for i in 0..native_plans.0 {
            h.counts.note(if i < native_plans.1 {
                "planner: a plan's points natively, in the log's place, the same as the log's"
            } else {
                "planner: a plan's points natively, in the log's place, NOT the log's"
            });
        }
        let consumed: BTreeSet<usize> = BTreeSet::new(); // log steps a native primitive's checks have stepped
        let sim = Simulator::new(lib);
        if took_house {
            h.counts.note("load: the lot's house (walls, floors) from the disc's copy, the save keeping none");
        }
        let finder = TreeFinder { lib, cache: HashMap::new(), shown: 0 };
        // stand-ins given a file before their first own step (the target of a player's command):
        // their first step still starts them as a stand-in's does
        let unstarted: BTreeSet<ObjectKey> = BTreeSet::new();
        let mut ids = stack_ids(log);
        place_quiet_stacks(log, &h.world, &mut ids, &mut h.counts, lib);
        let mut native_places: BTreeMap<(i16, u32), (i32, i32)> = BTreeMap::new();
        // a walking Sim's place from the native walk where there is one (study REPLAY_LOGGED_PLACES:
        // every place the log's)
        let use_native_places = !has("REPLAY_LOGGED_PLACES");
        // the routing runs' native returns, for the routing primitives' steps (its own counts dropped:
        // the full pass after the steps counts them)
        if !quiet && !log.places.is_empty() {
            let native = routing(log, lib, &h.world, &h.world, &mut Counts { quiet: true, ..Default::default() }, &ids);
            h.routing_returns = native.returns;
            native_places = native.places;
        }

        // The log's steps by tick, in order.
        let mut by_tick: Vec<Vec<usize>> = vec![Vec::new(); log.ticks as usize + 1];
        for (i, s) in log.steps.iter().enumerate() {
            if s.tick >= 1 && s.tick <= log.ticks {
                by_tick[s.tick as usize].push(i);
            }
        }
        let frames: BTreeMap<u32, Vec<u32>> = BTreeMap::new(); // the log's stacks: frame addresses, bottom first
        let first_seen: BTreeMap<u32, u32> = BTreeMap::new(); // stack -> tick of its first step
        let first_step: BTreeSet<u32> = BTreeSet::new(); // stacks whose first step was seen
        let last_frame: HashMap<u32, (u32, TreeId)> = HashMap::new();
        let unseen: BTreeSet<u32> = BTreeSet::new(); // placeholder addresses of saved frames below a first step
        let delivered: BTreeSet<u32> = BTreeSet::new(); // Sims whose first interaction was seen
        if has("REPLAY_SHOW_OFFLIST") {
            let want = [0x80ff7f78u32, 0x80ff8350, 0x80ff8728];
            for st in log.steps.iter().filter(|st| want.contains(&st.sim) && st.tick == 260) {
                println!("offlist {:x} tick {} {}:{} -> {} so {} reads {:?} params {:?}", st.sim, st.tick, st.tree, st.node, st.after, st.stack_object, st.lookups.iter().map(|v| (v.scope, v.data, v.value, v.object == st.sim)).collect::<Vec<_>>(), st.params);
            }
            println!("offlist ids: {:?}", want.iter().map(|p| (format!("{p:x}"), ids.get(p))).collect::<Vec<_>>());
        }
        let frame_tree: HashMap<(u32, u32, u16), (TreeId, FileId)> = HashMap::new();
        // a log of today's observer: its plans carry the search's region lists ('r')
        let new_observer = log.plans.iter().any(|p| !p.lookup_regions[0].is_empty());
        let (streak, best_streak) = (0usize, 0usize);

        // Which steps are turns: those in the tick's pass over the objects ('Q' records). The others
        // are the engine's own (the pie menu's checks on a Sim's stack: docs/simulation.md). A log
        // without them is taken as every step a turn.
        let have_passes = !log.passes.is_empty();
        let engine_run: Vec<bool> = log.steps.iter().map(|s| have_passes && !s.in_pass).collect();
        if !have_passes {
            h.counts.note("log: no pass records (an older log): every step taken as a turn, no tick as paused");
        }

        let place_at = 0; // the next 'P' record (places: the native walk's where it has one, else the log's)
        let motive_at = 0; // the next 'N' record
        let shown_motives = 0;
        let ev_env = env("REPLAY_EVALUATION");
        let engine_list_at = 1;
        // The time of day's light ('3' records, a light log's) from the engine's own clock: the
        // hour (global 0), the minute (global 5) and two seconds a pass counted (`clock_pass`), each
        // tick's calls against the clock after the tick (tick 0's before the first).
        let mut time_at = 0;
        check_time_of_day(log, 0, &h.world, &mut time_at, &mut h.counts);
        Some(Replay {
            lib,
            log: log_rc,
            lot,
            h,
            consumed,
            sim,
            finder,
            unstarted,
            ids,
            native_places,
            use_native_places,
            by_tick,
            frames,
            first_seen,
            first_step,
            last_frame,
            unseen,
            delivered,
            frame_tree,
            new_observer,
            streak,
            best_streak,
            have_passes,
            engine_run,
            place_at,
            motive_at,
            shown_motives,
            ev_env,
            engine_list_at,
            time_at,
            t: 0,
            headings: HashMap::new(),
            heading_at: 0,
        })
    }


    /// The world as the run has it now.
    pub fn world(&self) -> &World<'l> {
        &self.h.world
    }

    /// The Sims' Animate plays as the picture of `tick` shows them (`animator_for`): the Sim's
    /// object id, its animation's id, the play's place and whether it runs backward. The
    /// run itself is not moved (its steps apply the frames in their turn); empty when the run
    /// has no animation catalog.
    pub fn sim_plays(&self, tick: u32) -> Vec<(i16, u32, f32, bool)> {
        let Some(ar) = &self.h.anim else { return Vec::new() };
        let m = ar.animator_for(tick);
        m.iter().filter_map(|(sim, id, play)| Some((*self.ids.get(&sim)?, id, play.place(), play.backward()))).collect()
    }

    /// The tick the run is at (the last one stepped).
    pub fn tick(&self) -> u32 {
        self.t
    }
}
