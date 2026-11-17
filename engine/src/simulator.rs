//! The simulation tick: which objects' tree stacks run, and in what order
//! (`docs/simulation.md`). Measured on the recompiled Bustin' Out (BUSTIN_TREE_LOG) and read from
//! its tick, fn_80238150.

use crate::interp::{Host, Interpreter};
use crate::library::Library;
use crate::motives::{MotiveRates, update_motives};
use crate::primitives::apply_motive_deltas;
use crate::world::{DATA_LOCKOUT, Object, ObjectKey, Wake, World};

/// The host the objects' scripts run with: their primitives and the trees they name, and the
/// world they run on (which the host holds, so that a primitive can reach it).
pub trait SimHost<'lib>: Host {
    fn world(&mut self) -> &mut World<'lib>;
    /// The object whose turn begins: its stack's primitives are its.
    fn set_me(&mut self, me: ObjectKey);
    /// After each object's turn (for checks): the object, the steps it ran.
    fn on_turn(&mut self, _me: ObjectKey, _steps: u32) {}
}

/// An object that has a stack, with its id.
#[derive(Clone, Copy, Debug)]
struct Entry {
    id: i16,
    key: ObjectKey,
}

pub struct Simulator {
    /// The objects' running stacks, made on first need from the frames the object has (a lot's
    /// object: as the lot saved them). Each lives as long as its object in the simulator.
    runs: Vec<Option<Interpreter>>,
    /// The objects that have a stack, by id: rebuilt only when the world's objects change
    /// (`World::version`), so that a pass is a walk down a vector.
    order: Vec<Entry>,
    seen_version: u64,
    /// The highest id in use.
    top: i16,
    now: u32,
    cursor: i16,
    rates: Option<MotiveRates>,
}

/// An object's turn begins (the pass has reached it, asleep or not): a person's turns are
/// counted, and every 60th ends with its motives' own update (Person_SimMotives, 8022501C),
/// after its stack has run: in the log the turn's scripts read the motives as they were before
/// it. For a Sim of the family's, not a person with a file of its own (an NPC: the paper girl,
/// whose gate is set). Measured: `docs/simulation.md`, "The motives". Whether the turn ends
/// with the update.
fn begin_turn(rates: &Option<MotiveRates>, o: &mut Object) -> bool {
    if !o.person || o.definition.is_some() || rates.is_none() {
        return false;
    }
    if o.motives.len() < 16 || o.motives_b.len() < 16 || o.person_data.len() < 7 {
        return false;
    }
    o.turns = o.turns.wrapping_add(1);
    if !o.turns.is_multiple_of(60) {
        return false;
    }
    o.data.len() <= 34 || o.data[34] == 0 // the gate's object +106 (data field 34)
}

/// A turn's end. An object whose stack ran (not passed over asleep) counts its lockout down
/// (object data 25, which keeps its Idle from sleeping: `primitives::may_sleep`; Castaway's
/// `cXObject::Simulate`). A person's running motive changes (SetMotiveDelta's) apply at the end
/// of each of its turns, before the update (the log's first change is in the turn that set it),
/// then the update if due.
fn end_turn(rates: &Option<MotiveRates>, world: &mut World, k: ObjectKey, update_due: bool, ran: bool) {
    let (run, hour) = (world.motives_run, world.hour);
    let o = world.get_mut(k);
    if ran && let Some(v) = o.data.get_mut(DATA_LOCKOUT).filter(|v| **v != 0) {
        *v -= 1;
    }
    if !o.person || !run {
        return;
    }
    apply_motive_deltas(o);
    if update_due && let Some(r) = rates {
        let (p3, p6) = (o.person_data[3] as i32, o.person_data[6] as i32);
        update_motives(r, &mut o.motives, &mut o.motives_b, p3, p6, hour);
    }
}

impl Simulator {
    pub fn new(lib: &Library) -> Self {
        Simulator { runs: Vec::new(), order: Vec::new(), seen_version: u64::MAX, top: 0, now: 0, cursor: 0, rates: MotiveRates::from(lib) }
    }

    /// Where the next pass goes on from (0: id 1).
    pub fn cursor(&self) -> i16 {
        self.cursor
    }
    /// Ticks run.
    pub fn now(&self) -> u32 {
        self.now
    }

    /// An object's running stack, made on first need from the frames the object has, its own
    /// parameters the object's temps as they are then.
    /// The wakes the turns asked for (`World::wakes`), in order.
    pub fn apply_wakes(&mut self, world: &mut World) {
        let lib = world.library();
        for (id, how) in std::mem::take(&mut world.wakes) {
            let Some(k) = world.find(id) else { continue };
            let Some(Some(i)) = self.runs.get_mut(k.0 as usize) else { continue };
            if i.stack().is_empty() {
                continue;
            }
            let (sleep, f) = i.sleep_and_sleeper();
            match how {
                Wake::Lockout => sleep.wake_early(f),
                Wake::Ended => {
                    // the stack's node: an Idle's sleep ends; an IdleForInput's count is 0
                    let op = f.tree.and_then(|t| lib.tree_at(t).nodes.get(f.node as usize)).map(|n| n.opcode);
                    if matches!(op, Some(0) | Some(17)) {
                        if let Some(p) = f.params.first_mut() {
                            *p = 0;
                        }
                        if op == Some(0) {
                            sleep.end();
                        }
                    }
                }
            }
        }
    }

    pub fn stack_of(&mut self, world: &World, k: ObjectKey) -> &mut Interpreter {
        let at = k.0 as usize;
        if at >= self.runs.len() {
            self.runs.resize_with(at + 1, || None);
        }
        self.runs[at].get_or_insert_with(|| {
            let o = world.get(k);
            let mut i = Interpreter::main();
            // On a community lot (the club) the saved stacks restart, each from its bottom
            // frame at its first node (176 of the club log's 182 first steps); a home lot
            // resumes them (docs/simulation.md, "The club's stacks restart").
            let mut frames = o.stack.clone();
            // (but an object whose data field 4 is 0, the scenario controllers 2 and 14,
            // resumes its stack there too)
            let keep = o.data.get(4) == Some(&0);
            if world.is_community_lot() && !keep {
                frames.truncate(1);
                if let Some(f) = frames.first_mut() {
                    f.node = 0;
                    // no stack object yet (the saved one is not carried: 71 club steps); its
                    // locals stay the saved ones (zeroing them puts 21 more off)
                    f.stack_object = 0;
                }
            }
            i.restore(frames.clone());
            i.set_defaults(o.temps.to_vec());
            // An object with no saved stack (made during play) starts as the original's: its main
            // tree (its function table's entry 1) with its init tree (entry 0) above it, an engine
            // frame whose return leaves main at its first node (the meal's 4099 "init single
            // plate", then 4193 "main - regular meal")
            let lib = world.library();
            if frames.is_empty() && !o.person
                && let Some(table) = o.file.and_then(|f| lib.file(f).functions.values().next())
            {
                let tree = |k: usize| table.get(k).filter(|e| e.1 != 0).and_then(|e| lib.tree(o.file, e.1));
                if let (Some(init), Some(main)) = (tree(0), tree(1)) {
                    if i.push(lib, main, &[]) {
                        i.top().file = o.file;
                    }
                    if i.push(lib, init, &[]) {
                        i.top().file = o.file;
                        i.top().engine = true;
                    }
                }
            }
            // A stack saved at an Idle node was asleep there and its sleep has run out (the
            // save keeps no sleep): its first Idle step wakes, true whatever its parameter
            // (docs/simulation.md, "A saved stack asleep at its Idle").
            if frames.last().and_then(|f| f.tree.and_then(|t| lib.get_tree(t)?.nodes.get(f.node as usize))).is_some_and(|n| n.opcode == 0) {
                i.sleep().sleep(0, usize::MAX);
            }
            i
        })
    }

    fn has_run(&self, k: ObjectKey) -> bool {
        self.runs.get(k.0 as usize).is_some_and(Option::is_some)
    }

    /// The object is gone.
    pub fn forget(&mut self, k: ObjectKey) {
        if let Some(r) = self.runs.get_mut(k.0 as usize) {
            *r = None;
        }
        self.seen_version = u64::MAX;
    }

    fn refresh(&mut self, world: &World) {
        if self.seen_version == world.version() {
            return;
        }
        self.seen_version = world.version();
        let by_id = world.by_id();
        self.top = by_id.keys().next_back().copied().unwrap_or(0);
        // The runs of objects that are gone go with them.
        let mut live = vec![false; self.runs.len()];
        for v in by_id.values() {
            if let Some(l) = live.get_mut(v.0 as usize) {
                *l = true;
            }
        }
        for (r, l) in self.runs.iter_mut().zip(live) {
            if !l {
                *r = None;
            }
        }
        self.order.clear();
        for (&id, &key) in by_id {
            if id <= 0 || (world.get(key).stack.is_empty() && !self.has_run(key)) {
                continue;
            }
            self.stack_of(world, key);
            self.order.push(Entry { id, key });
        }
    }

    /// The first entry with an id at least this.
    fn first_from(&self, id: i16) -> usize {
        self.order.partition_point(|e| e.id < id)
    }

    fn start(&self) -> usize {
        self.first_from(if self.cursor > 0 { self.cursor } else { 1 })
    }

    /// For checks that step the stacks themselves: the objects the pass would reach, in order
    /// (to the highest id; a stop is not foreseen), with whether each is asleep.
    pub fn pass_order(&mut self, world: &World) -> Vec<(ObjectKey, bool)> {
        self.refresh(world);
        self.order[self.start()..]
            .iter()
            .filter_map(|e| {
                let i = self.runs[e.key.0 as usize].as_ref().expect("a run");
                (!i.done()).then(|| (e.key, i.asleep()))
            })
            .collect()
    }

    /// The pass done by hand: an object passed over.
    pub fn pass_over(&mut self, world: &World, k: ObjectKey) {
        self.stack_of(world, k).pass_over();
    }

    /// A turn begins (by hand): whether it ends with the motives' update.
    pub fn begin_turn(&self, world: &mut World, k: ObjectKey) -> bool {
        begin_turn(&self.rates, world.get_mut(k))
    }
    /// A turn ends (by hand).
    pub fn end_turn(&self, world: &mut World, k: ObjectKey, update_due: bool, ran: bool) {
        end_turn(&self.rates, world, k, update_due, ran)
    }

    /// The pass's end: at an id, or 0 for through.
    pub fn end_pass(&mut self, world: &mut World, stopped_at: i16) {
        self.cursor = stopped_at;
        world.stop_pass = false;
        self.now += 1;
    }

    /// In a pass done by hand: whether a turn asked it to stop since the last `end_pass`.
    pub fn stop_asked(&self, world: &World) -> bool {
        world.stop_pass
    }

    /// One tick: the pass over the objects, by id, from where the last one stopped (or id 1)
    /// up to the highest id in use when the pass starts (an object made during it waits for the
    /// next). A sleeping stack is passed over, which counts its sleep down; any other is stepped
    /// until it yields or stops. A turn that asks for it (`World::stop_pass`: a dialog waiting
    /// for the player) ends the pass there, and the next tick's goes on from that object.
    ///
    /// As the original's cursor (its +1424) does, each object is looked at when its id comes up,
    /// so a turn that wakes a later object gives it its turn in this pass.
    pub fn tick<'lib, H: SimHost<'lib>>(&mut self, host: &mut H) {
        self.refresh(host.world());
        let top = self.top;
        let mut stopped_at = 0;
        let mut i = self.start();
        while i < self.order.len() && self.order[i].id <= top {
            let e = self.order[i];
            let interp = self.runs[e.key.0 as usize].as_mut().expect("a run");
            if interp.done() {
                i += 1;
                continue;
            }
            let motives_due = begin_turn(&self.rates, host.world().get_mut(e.key));
            if interp.asleep() {
                interp.pass_over();
                end_turn(&self.rates, host.world(), e.key, motives_due, false);
                i += 1;
                continue;
            }
            interp.reset_iterations();
            host.set_me(e.key);
            // a call that gives no parameters passes the object's temps as they are now (the
            // original's interpreter keeps a pointer to them, +44)
            interp.copy_defaults(&host.world().get(e.key).temps);
            let steps = interp.run(host, 1 << 20);
            end_turn(&self.rates, host.world(), e.key, motives_due, true);
            self.apply_wakes(host.world());
            host.on_turn(e.key, steps);
            if host.world().stop_pass {
                stopped_at = e.id;
                break;
            }
            // A turn that made, removed or renumbered objects: on from the next id.
            if self.seen_version != host.world().version() {
                self.refresh(host.world());
                i = self.first_from(e.id.wrapping_add(1));
            } else {
                i += 1;
            }
        }
        self.end_pass(host.world(), stopped_at);
    }
}
