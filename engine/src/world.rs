//! The objects scripts run on, and what a script's variables are (scope and datum): the object
//! model. Its layout is the engine's own; what each scope names is the original's, measured on
//! the recompiled Bustin' Out (`docs/objects.md`).

use std::collections::BTreeMap;

use crate::expression::{Variable, Variables};
use crate::iff::FormatError;
use crate::interp::Frame;
use crate::library::{DefId, Definition, Library};
use crate::routing::Walls;
use crate::save::{
    read_lot_arrays, LotChunks, LotObjects, MotiveDelta, Neighbour, OBJECT_TEMPS, SavedClass, SavedQueueEntry, SavedRelationship, read_lot_objects,
    read_object_types,
};
use crate::{FileId, TreeId};

/// The scopes, by the PC game's names where the measurements agree with them.
pub mod scope {
    pub const MY_ATTRIBUTE: u8 = 0;
    pub const STACK_ATTRIBUTE: u8 = 1;
    pub const MY_DATA: u8 = 3;
    pub const STACK_DATA: u8 = 4;
    pub const GLOBAL: u8 = 6;
    pub const LITERAL: u8 = 7;
    pub const TEMP: u8 = 8;
    pub const PARAMETER: u8 = 9;
    pub const STACK_OBJECT: u8 = 10;
    pub const CHECK_TREE_AD_RANGE: u8 = 12;
    pub const MY_MOTIVE: u8 = 14;
    pub const STACK_MOTIVE: u8 = 15;
    pub const MY_PERSON_DATA: u8 = 18;
    pub const STACK_PERSON_DATA: u8 = 19;
    pub const STACK_SLOT: u8 = 16;
    pub const MY_SLOT: u8 = 20;
    pub const STACK_FUNCTION: u8 = 35;
    pub const MY_RELATIONSHIP: u8 = 40;
    pub const STACK_RELATIONSHIP: u8 = 41;
    pub const STACK_DEFINITION: u8 = 21;
    pub const STACK_ATTRIBUTE_BY_PARAMETER: u8 = 22;
    pub const ROOM_BY_TEMP0: u8 = 23;
    pub const NEIGHBOUR_IN_STACK_OBJECT: u8 = 24;
    pub const NEIGHBOUR_DATA_OF_STACK_OBJECT: u8 = 32;
    pub const LOCAL: u8 = 25;
    pub const STACK_SPRITE_FLAGS: u8 = 27;
    pub const CONSTANT: u8 = 26;
    pub const MY_PERSON_DATA_BY_TEMP: u8 = 30;
    pub const NEIGHBOURHOOD: u8 = 34;
}

/// The object data field the engine relies on: the object's id.
/// How a stack is woken (`World::wakes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// An expression touched the object's lockout (object data 25; Castaway's
    /// `cXObject::InterpValue`): a sleep ends early, its parameter what was left.
    Lockout,
    /// NotifyStackObject (opcode 49): an Idle's sleep ends (its first parameter 0, the Idle true
    /// when next stepped), an IdleForInput's count (its first parameter) is 0.
    Ended,
}

/// A person's turns so far (person data 27), counted by the engine at each turn's end.
pub const PERSON_TICKS: usize = 27;

/// The object data field a check tree sets to hide its interaction ("hide menu", Global 265):
/// the engine clears it on the checking Sim before each check it runs. Measured: all 119 reads
/// of it in the full logs' checks are 0 before the tree sets it.
pub const DATA_HIDE_INTERACTION: usize = 50;
pub const DATA_OBJECT_ID: usize = 11;
/// The object data field of its lockout: while set, its Idle does not sleep, it counts down a
/// tick at each turn its stack runs, and an expression that touches it wakes the object
/// (`primitives::may_sleep`, `simulator.rs`'s `end_turn`; `docs/primitives.md`, "Idle").
pub const DATA_LOCKOUT: usize = 25;
/// The function table's (OBJf) entry a door runs when a route goes through it (The Sims' "portal").
pub const PORTAL_FUNCTION: usize = 15;
/// The entry a seat runs when a Sim routes to a slot to sit at it (The Sims' "sit").
pub const SIT_FUNCTION: usize = 26;
/// The label of the tree a routing Sim runs to ask someone in its way to move.
pub const ASK_TO_MOVE: &str = "route ask to move animation";
/// The label of the tree a routing Sim runs while the person it asked is still in its way.
pub const ROUTE_WAIT: &str = "route waiting animation";
/// The label of the tree the engine runs over a Sim that spent money.
pub const MONEY_BALLOON: &str = "display money balloon";
/// The object data field that holds the way it faces (in eighths of a turn).
pub const DATA_FACING: usize = 1;
/// The object data field naming the object it sits in or on (0 none): a lamp on a table, the
/// sink in its counter.
pub const DATA_CONTAINER: usize = 2;
/// The slot of its container it is in.
pub const DATA_SLOT: usize = 3;
/// The object data field of its flags (0x4: no extent, not an obstacle; 0x10: may be walked
/// through, an obstacle at a small cost; `docs/simulation.md`, "Which objects block").
pub const DATA_FLAGS: usize = 8;
/// The person data field the engine relies on: a Sim's neighbour id (its NBRS id).
pub const PERSON_NEIGHBOUR_ID: usize = 31;
/// Person data 65: 1 for a female Sim (set at the load from the description: `World::describe_persons`).
pub const PERSON_FEMALE: usize = 65;
/// Person data 32: 1 for a Sim not of the player's family (`World::mark_visitors`).
pub const PERSON_VISITOR: usize = 32;
/// A Sim's own priority (person data 33): the priority of what it pushes with PushAction's kind 0
/// (Mom's saved 50 is her current meal's).
pub const PERSON_PRIORITY: usize = 33;
/// A queue entry's flag (the original's +68 bit 2, set by PushAction's operand byte 3 bit 4): it
/// interrupts whatever the Sim is doing, whatever the priorities.
pub const ENTRY_INTERRUPTS: i32 = 2;
/// The priority of the player's command: above any pushed one (an engine rule: the original's
/// player commands do not come through PushAction; nothing logged shows one cancelling another).
pub const PLAYER_PRIORITY: i32 = i32::MAX;
/// Global 10: the lot's house number (16 the club, 1 the home save's).
pub const GLOBAL_HOUSE: usize = 10;

#[derive(Clone, Debug, Default)]
pub struct Object {
    pub file: Option<FileId>,
    pub definition: Option<DefId>,
    pub guid: u32,
    /// Scope 8.
    pub temps: [i16; 8],
    /// Scope 3, 72 fields (11: the object's id).
    pub data: Vec<i16>,
    /// Scope 0, as many as the save gives (a lot's objects).
    pub attributes: Vec<i16>,
    pub person: bool,
    /// A multi-tile object's parts: the id of its leader (the leader's own id for itself); 0 for
    /// a single-tile object.
    pub leader: i16,
    /// A door's part (OBJD type 8, field 3 zero: a window has it set).
    pub door: bool,
    /// A window's part (OBJD type 8, field 3 set).
    pub window: bool,
    /// Scope 18.
    pub person_data: Vec<i16>,
    /// Scope 14, kept between -100 and 100.
    pub motives: Vec<f32>,
    pub motives_b: Vec<f32>,
    /// Its running motive changes (SetMotiveDelta's; the save's +1120 list): each a motive, its
    /// change a tick and the value it stops at, applied as each of its turns begins.
    pub motive_deltas: Vec<MotiveDelta>,
    /// The passes that have reached it (a person's motives update every 60th).
    pub turns: u32,
    /// Its place in sixteenths of a tile (the save's header 0 and 1, the original's FTilePt at
    /// its +0xC8; the observer's 'P' records): -16, -16 off the lot.
    pub x16: i32,
    pub y16: i32,
    pub level: i32,
    /// A Sim's interaction queue as the lot saved it: the entry it was on, the rest queued.
    pub current: SavedQueueEntry,
    pub queue: Vec<SavedQueueEntry>,
    /// The person a routing Sim last asked to move out of a slot's places (`ask_to_move_tree`):
    /// asked again while they stay, the Sim waits instead ("Asked to move").
    pub asked_to_move: Option<i16>,
    /// It spent money this turn (an AlterBudget that is not a test): the money balloon shows when
    /// its turn ends (`money_balloon_tree`).
    pub spent: bool,
    /// It was made during play (`World::make_object`), not loaded with the lot.
    pub made: bool,
    /// Its tree stack, bottom first: a lot's objects' as the lot saved it (a frame whose tree is
    /// not on the disc keeps no tree).
    pub stack: Vec<Frame>,
    /// Its relationships with other objects, as the lot saved them (the relationship system,
    /// Relationship2, is not native yet).
    pub relationships: Vec<SavedRelationship>,
    /// Its dynamic sprite flags (scope 27, The Sims' "dynamic sprite flags" of the stack
    /// object: a word per index, set by scripts and read by its graphics; not in the save as far
    /// as read, so its first values are taken from what scripts read).
    pub sprite_flags: Vec<i16>,
}

impl Object {
    pub fn new() -> Self {
        Object { level: 1, ..Default::default() }
    }
    pub fn id(&self) -> i16 {
        self.data.get(DATA_OBJECT_ID).copied().unwrap_or(0)
    }
    pub fn on_lot(&self) -> bool {
        self.x16 >= 0 && self.y16 >= 0
    }
    /// The way it faces: data field 1, in eighths of a turn (`routing::front`).
    pub fn facing(&self) -> i16 {
        self.data.get(DATA_FACING).copied().unwrap_or(0)
    }
}

/// An object of a world, by its slot: the same while it lives, whatever its id or its place in
/// the list, and never another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectKey(pub u32);

/// The game's random numbers (Random): the engine's own generator, not the original's
/// (`docs/plan.md`, "Same game, new engine"). Seeded per game; xorshift64*.
#[derive(Clone, Debug)]
pub struct Rng {
    pub state: u64,
}

impl Default for Rng {
    fn default() -> Self {
        Rng { state: 0x9E3779B97F4A7C15 }
    }
}

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        (self.state.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as u32
    }
    /// A number from 0 up to n (n above 0), without the bias of a plain modulo.
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }
}

/// What the simulation asks of the presentation (sound, the picture), in the order asked: the
/// front ends (not written yet) take them each frame. The simulation never waits on them, so
/// its run is the same with or without a front end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationEvent {
    /// The primitive: PlaySound 23, Update 7, KillSounds 48.
    pub opcode: u16,
    /// The object whose script asked.
    pub object: i16,
    /// As the node has it.
    pub operand: [u8; 8],
}

pub struct World<'lib> {
    lib: &'lib Library,
    slots: Vec<Option<Object>>,
    order: Vec<ObjectKey>,
    by_id: BTreeMap<i16, ObjectKey>,
    /// The same, indexed by id + 32768, for `find`.
    id_index: Vec<Option<ObjectKey>>,
    version: u64,
    pub rng: Rng,
    /// The lot's (`load_lot`).
    pub walls: Walls,
    pub presentation: Vec<PresentationEvent>,
    /// A dialog waiting for the player (Dialog): the object whose script opened it (0: none),
    /// and the player's answer once given (the front end sets it). While it waits, the tick's
    /// pass stops after that object's turn (stop_pass, which the simulator takes and clears).
    pub dialog_object: i16,
    pub dialog_answer: Option<bool>,
    pub stop_pass: bool,
    /// The clock's hour (the motive update reads it only for a sleeping person; the clock is
    /// not modelled yet).
    pub hour: i32,
    /// Whether the persons' motives move by themselves on this lot (the update and the running
    /// changes). At Mom's house they do (the home log); at the club they do not (its logs: 1 of
    /// 47 motive values changes). What decides it is the update gate's third test, a virtual
    /// call not identified yet (`docs/simulation.md`, "The motives"); until then the lot's
    /// loader says.
    pub motives_run: bool,
    /// Whether an object made during play runs its own main tree from the next pass (a run alone:
    /// the original's does; a replay takes its steps from the log, which asks for them).
    pub run_made: bool,
    /// Scope 6.
    pub globals: Vec<i16>,
    /// Scope 34.
    pub neighbourhood: Vec<i16>,
    /// The neighbourhood's, in id order (read_neighbours).
    pub neighbours: Vec<Neighbour>,
    /// Scope 12: one shared value per datum (only 15 is used).
    pub ad_range: Vec<i16>,
    /// The houses' goals and the player's progress (CompleteGoal).
    pub goals: crate::goals::Goals,
    /// The objects woken this turn, for the simulator to wake (`Simulator::apply_wakes`).
    pub wakes: Vec<(i16, Wake)>,
}

impl<'lib> World<'lib> {
    /// Each person's person data 65, whether it is female, from its neighbour's description
    /// (`Neighbour::female`; the person's neighbour id is its data 31), as the original's load of
    /// a Sim sets it (`save::read_descriptions`). A person with no description keeps its own.
    /// The original's copy of a neighbour's saved motives into its Sim at the load
    /// (`fn_801DBAC0`, its loop at 0x801DBCFC): both sets of 16, motive 11 of each 0.0 (the
    /// constant at 0x802C7B70), the floats as the original reads them (`Neighbour::motives`).
    /// Done for the neighbours of a family with no house (the player's: a housed family's motives
    /// are its lot's): the original's condition (0x801DBCA4, two of the manager's lists) is not
    /// read; this rule fits both logs (REPLAY_NO_NEIGHBOUR_MOTIVES leaves it out).
    pub fn take_neighbour_motives(&mut self) {
        let sets: Vec<(i16, Vec<f32>, Vec<f32>)> = self.neighbours.iter().filter(|n| n.motives.len() == 16 && n.motives_b.len() == 16 && n.house == Some(0)).map(|n| (n.id, n.motives.clone(), n.motives_b.clone())).collect();
        for k in self.order().to_vec() {
            let o = self.get_mut(k);
            if !o.person || o.motives.len() < 16 || o.motives_b.len() < 16 {
                continue;
            }
            let Some(&nid) = o.person_data.get(PERSON_NEIGHBOUR_ID) else { continue };
            let Some((_, a, b)) = sets.iter().find(|(id, _, _)| *id == nid && nid != 0) else { continue };
            for i in 0..16 {
                o.motives[i] = if i == 11 { 0.0 } else { a[i] };
                o.motives_b[i] = if i == 11 { 0.0 } else { b[i] };
            }
        }
    }

    /// The visitors' mark at the load (`fn_801DF844`, its store via 0x8022A30C): each Sim's
    /// person data 32 becomes 1 where it was 0, and 0 for the household (the original's test is
    /// a virtual call, unread; taken as the neighbours of the player's family, with no house, and
    /// of the lot's own, its house number global 10: both logs agree, home's 237 of house 1).
    pub fn mark_visitors(&mut self) {
        let lot_house = self.globals.get(GLOBAL_HOUSE).map(|&h| h as i32);
        let household: Vec<i16> = self.neighbours.iter().filter(|n| n.house == Some(0) || (n.house.is_some() && n.house == lot_house)).map(|n| n.id).collect();
        for k in self.order().to_vec() {
            let o = self.get_mut(k);
            if !o.person || o.person_data.len() <= PERSON_VISITOR {
                continue;
            }
            let nid = o.person_data.get(PERSON_NEIGHBOUR_ID).copied().unwrap_or(0);
            let v = &mut o.person_data[PERSON_VISITOR];
            *v = if nid != 0 && household.contains(&nid) { 0 } else if *v == 0 { 1 } else { *v };
        }
    }

    pub fn describe_persons(&mut self) {
        let female: Vec<(i16, bool)> = self.neighbours.iter().filter_map(|n| n.female.map(|f| (n.id, f))).collect();
        for k in self.order().to_vec() {
            let o = self.get_mut(k);
            if !o.person {
                continue;
            }
            let Some(&nid) = o.person_data.get(PERSON_NEIGHBOUR_ID) else { continue };
            if let Some(&(_, f)) = female.iter().find(|(id, _)| *id == nid && nid != 0)
                && let Some(d) = o.person_data.get_mut(PERSON_FEMALE)
            {
                *d = i16::from(f);
            }
        }
    }

    pub fn new(lib: &'lib Library) -> Self {
        World {
            lib,
            slots: Vec::new(),
            order: Vec::new(),
            by_id: BTreeMap::new(),
            id_index: vec![None; 1 << 16],
            version: 0,
            rng: Rng::default(),
            walls: Walls::default(),
            presentation: Vec::new(),
            dialog_object: 0,
            dialog_answer: None,
            stop_pass: false,
            hour: 12,
            motives_run: true,
            run_made: false,
            globals: Vec::new(),
            neighbourhood: Vec::new(),
            neighbours: Vec::new(),
            ad_range: Vec::new(),
            goals: Default::default(),
            wakes: Vec::new(),
        }
    }

    pub fn library(&self) -> &'lib Library {
        self.lib
    }

    /// An object's definition, if it has one.
    pub fn definition(&self, o: &Object) -> Option<&'lib Definition> {
        o.definition.map(|d| self.lib.def(d))
    }

    /// Objects are kept in the original's list order (the order SetToNext walks): a lot's in its
    /// table's order, an object made later in front of them all.
    pub fn add(&mut self, o: Object, front: bool) -> ObjectKey {
        let key = ObjectKey(self.slots.len() as u32);
        self.slots.push(Some(o));
        if front {
            self.order.insert(0, key);
        } else {
            self.order.push(key);
        }
        self.reindex(key);
        key
    }

    /// The objects' keys in the list's order.
    pub fn order(&self) -> &[ObjectKey] {
        &self.order
    }
    /// The objects in the list's order.
    pub fn objects(&self) -> impl Iterator<Item = &Object> + '_ {
        self.order.iter().map(|&k| self.get(k))
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }
    /// A community lot (the club): its house number (global 10) the global file's constant
    /// 19215, as the scripts test it (Global 462).
    pub fn is_community_lot(&self) -> bool {
        let lib = self.library();
        self.globals.get(10).is_some_and(|&h| lib.constant(lib.global(), 19215) == Some(h))
    }
    /// An object's function (its file's function table, OBJf, entry `k`): the action tree, if
    /// it has one.
    pub fn function_tree(&self, o: ObjectKey, k: usize) -> Option<TreeId> {
        let lib = self.library();
        let file = self.get(o).file?;
        let &(_, action) = lib.file(file).functions.values().next()?.get(k)?;
        (action != 0).then(|| lib.tree(Some(file), action)).flatten()
    }

    /// The door a route (its waypoints, sixteenths) goes through first: the leader of the door
    /// whose parts stand on the two tiles of the route's first change of room (the home log's
    /// two routes through a door, ticks 260 and 1901; docs/simulation.md, "A route through a
    /// door").
    pub fn portal_on_route(&self, points: &[(i32, i32)]) -> Option<ObjectKey> {
        let w = &self.walls;
        let (a, b) = points.windows(2).map(|p| ((p[0].0 >> 4, p[0].1 >> 4), (p[1].0 >> 4, p[1].1 >> 4))).find(|&(a, b)| {
            let (r0, r1) = (w.room(a.0, a.1), w.room(b.0, b.1));
            r0 != r1 && r0 != 0 && r1 != 0
        })?;
        let part = self.objects().find(|o| o.door && o.on_lot() && [a, b].contains(&(o.x16 >> 4, o.y16 >> 4)))?;
        self.find(part.leader)
    }

    /// The seat a GotoRoutingSlot (its node) leads to: where the slot it names (on the stack object)
    /// is one to sit at (its sitting field set) and an object with a sit function (`SIT_FUNCTION`)
    /// stands on its route's last tile (the home log's one, tick 1791: the dining table 47's slot
    /// 3, the chair 49; docs/simulation.md, "A seat at a routing slot").
    pub fn seat_at_routing_slot(&self, n: &crate::bhav::Node, params: &[i16], stack_object: i16, points: &[(i32, i32)]) -> Option<ObjectKey> {
        let lib = self.library();
        let o = self.find_object(stack_object)?;
        let slot = lib.routing_slot(o.file?, self.definition(o), n.operand16(1), n.operand16(0), params)?;
        if slot.sitting <= 0 {
            return None;
        }
        let &(x, y) = points.last()?;
        let tile = (x >> 4, y >> 4);
        let k = self.objects().find(|c| c.on_lot() && (c.x16 >> 4, c.y16 >> 4) == tile && c.id() != stack_object && c.file.and_then(|f| lib.file(f).functions.values().next()?.get(SIT_FUNCTION).copied()).is_some_and(|e| e.1 != 0))?;
        self.find(k.id())
    }

    /// The person in the way of a routing slot's places (sixteenths) when the route to them has
    /// no way: another person on the lot standing on one of the places' tiles, in the places'
    /// order (the home log's one, tick 1503: Sim 237 on the meal 198's slot place 0, Sim 12
    /// routing; docs/simulation.md, "Asked to move").
    pub fn person_on_places(&self, me: ObjectKey, places: &[(i32, i32)]) -> Option<ObjectKey> {
        let me_id = self.get(me).id();
        let at = |p: &(i32, i32)| self.objects().find(|o| o.person && o.on_lot() && o.id() != me_id && (o.x16 >> 4, o.y16 >> 4) == (p.0 >> 4, p.1 >> 4));
        let o = places.iter().find_map(at)?;
        self.find(o.id())
    }

    /// The tree a routing Sim runs to ask a person in its way to move: its semi-global file's
    /// "route ask to move animation" (person_sg's 8302).
    pub fn ask_to_move_tree(&self, me: ObjectKey) -> Option<TreeId> {
        self.semi_tree_named(me, ASK_TO_MOVE)
    }

    /// The tree a routing Sim runs while the person it asked to move is still in its way: its
    /// semi-global file's "route waiting animation" (person_sg's 8276).
    pub fn route_wait_tree(&self, me: ObjectKey) -> Option<TreeId> {
        self.semi_tree_named(me, ROUTE_WAIT)
    }

    /// The tree the engine runs over a Sim that spent money, when its turn ends: its semi-global
    /// file's "display money balloon" (person_sg's 8384; the home log's tick 931, after the fridge's
    /// AlterBudget).
    pub fn money_balloon_tree(&self, me: ObjectKey) -> Option<TreeId> {
        self.semi_tree_named(me, MONEY_BALLOON)
    }

    fn semi_tree_named(&self, me: ObjectKey, name: &str) -> Option<TreeId> {
        let lib = self.library();
        let f = lib.file(self.get(me).file?);
        let semi = lib.file(f.semi?);
        semi.tree_by_name.get(name).and_then(|id| semi.trees.get(id)).copied()
    }

    /// The priority a pushed interaction gets (PushAction's operand byte 2, the original's
    /// fn_80208338): 1 is 100, 2 is 2, 3 is 50, and 0 the pushing script's own Sim's person data 33
    /// (1 if it is no Sim). Other kinds are a script error there; here the lowest, 1.
    pub fn push_priority(&self, kind: u8, pusher: ObjectKey) -> i32 {
        match kind {
            0 => {
                let o = self.get(pusher);
                if o.person { o.person_data.get(PERSON_PRIORITY).copied().map_or(1, i32::from) } else { 1 }
            }
            1 => 100,
            2 => 2,
            3 => 50,
            _ => 1,
        }
    }

    /// The test IdleForInput asks the Sim (`ShouldInterrupt`, the original's fn_80225D98), as far as
    /// it is read: the queue's front entry has a higher priority than the Sim's current interaction
    /// (the entry's +40 against the Sim's +74, here `current.c`), or is flagged to interrupt (+68
    /// bit 2). Not modelled: the Sim's own cancel flags (+150, +1100) and a motive below -99.
    pub fn should_interrupt(&self, sim: ObjectKey) -> bool {
        let o = self.get(sim);
        o.queue.first().is_some_and(|e| e.c > o.current.c || e.flags & ENTRY_INTERRUPTS != 0)
    }

    /// The player's command to a Sim (the front end's: a click on an object's menu, or the floor's
    /// "go here" marker): the target's interaction with that action tree queued at the back of
    /// the Sim's queue, its place in the target's table known, for IdleForInput to deliver with
    /// its checks. False if the target or the interaction is not found.
    pub fn queue_player_command(&mut self, sim: ObjectKey, target: i16, action: u16) -> bool {
        let lib = self.library();
        let Some(t) = self.find_object(target) else { return false };
        let tab = lib.table_of(t.file, t.person, self.definition(t));
        let (Some(f), Some(id)) = (tab.file, tab.table) else { return false };
        let Some(k) = lib.table(f, id).iter().position(|e| e.action == action) else { return false };
        let me = self.get(sim).id();
        self.get_mut(sim).queue.push(SavedQueueEntry {
            objects: [me, target, 0],
            tree: action as i16,
            interaction: Some(k),
            c: PLAYER_PRIORITY,
            ..Default::default()
        });
        true
    }

    /// On a community lot the saved stacks restart from their bottom frame (`Simulator::stack_of`),
    /// so the interaction a Sim was running is lost there: the engine queues it again, first, and
    /// the Sim's IdleForInput delivers it (the club log's tick 2: six Sims, their saved current
    /// entries; docs/simulation.md, "The club's stacks restart"). Run once, after the lot and its
    /// globals are in.
    pub fn requeue_current_on_community_lot(&mut self) {
        if !self.is_community_lot() {
            return;
        }
        for &k in &self.order.clone() {
            let o = self.get_mut(k);
            if o.person && o.data.get(4) != Some(&0) && o.current.tree != 0 && o.person_data.get(32) != Some(&0) {
                let c = o.current;
                o.queue.insert(0, c);
            }
        }
    }
    /// A Sim's interaction has returned to its IdleForInput: no current entry (the original's
    /// is cleared to no target and no table place).
    pub fn end_interaction(&mut self, k: ObjectKey) {
        let o = self.get_mut(k);
        if o.person {
            o.current = crate::save::SavedQueueEntry::default();
        }
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub fn get(&self, k: ObjectKey) -> &Object {
        self.slots[k.0 as usize].as_ref().expect("a live object")
    }
    /// The object, if it has not been removed.
    pub fn get_live(&self, k: ObjectKey) -> Option<&Object> {
        self.slots.get(k.0 as usize)?.as_ref()
    }
    pub fn get_mut(&mut self, k: ObjectKey) -> &mut Object {
        self.slots[k.0 as usize].as_mut().expect("a live object")
    }

    /// A lot's objects as saved, each read by its class.
    pub fn read_objects(&self, lot: &LotChunks) -> Result<LotObjects, FormatError> {
        let (Some(objt), Some(objm)) = (lot.find("objt"), lot.find("ObjM")) else {
            return Err(FormatError("a lot without objt or ObjM".into()));
        };
        let guid_of: BTreeMap<i32, u32> = read_object_types(objt)?.iter().map(|t| (t.fields[4] as i32, t.guid)).collect();
        read_lot_objects(objm, |kind| {
            Self::saved_class(guid_of.get(&(kind as i32)).and_then(|&g| self.lib.definition(g)).map(|d| self.lib.def(d)))
        })
    }

    /// A lot's objects, as its table lists them: each with its id, its definition (by its
    /// type's GUID) and its file. Returns how many had no definition on the disc (the
    /// characters, whose definitions are made at run time).
    pub fn load_lot(&mut self, lot: &LotChunks) -> Result<usize, FormatError> {
        let Some(objt) = lot.find("objt") else { return Err(FormatError("a lot without objt".into())) };
        let guid_of: BTreeMap<i32, u32> = read_object_types(objt)?.iter().map(|t| (t.fields[4] as i32, t.guid)).collect();
        let mut missing = 0;
        // each object's saved frames' tree ids, for the pass below
        let mut saved_ids: Vec<(i16, Vec<u16>)> = Vec::new();
        for lo in self.read_objects(lot)?.objects {
            let mut o = Object::new();
            if let Some(&guid) = guid_of.get(&(lo.kind as i32)) {
                o.guid = guid;
                o.definition = self.lib.definition(guid);
                o.file = o.definition.map(|d| d.file);
            }
            // No definition on the disc: a character, whose definition is made at run time. Its
            // scripts are the template person's, which every Sim runs (its own trees, 4096 on;
            // the character files hold only its data).
            if o.definition.is_none() {
                missing += 1;
                o.person = true;
                o.file = self.lib.find("template_person");
            }
            // Its state as the lot saved it.
            o.x16 = lo.header[0];
            o.y16 = lo.header[1];
            o.data = lo.data;
            o.attributes = lo.attributes;
            for (t, &s) in o.temps.iter_mut().zip(lo.temps.iter().take(OBJECT_TEMPS)) {
                *t = s;
            }
            o.relationships = lo.relationships;
            if matches!(lo.saved_class, SavedClass::Part | SavedClass::Door) {
                o.leader = if lo.leader != 0 { lo.leader } else { lo.id };
            }
            // A door's part (type 8, field 3 zero); windows are type 8 too, with field 3 set
            // (measured: all 30 doors and 30 windows on the disc).
            o.door = lo.saved_class == SavedClass::Door && self.definition(&o).is_some_and(|d| d.field(3) == 0);
            o.window = lo.saved_class == SavedClass::Door && self.definition(&o).is_some_and(|d| d.field(3) != 0);
            if lo.saved_class == SavedClass::Sim {
                o.person = true; // an NPC with a definition keeps its own file
                o.person_data = lo.person_data;
                o.motives = lo.motives;
                o.motives_b = lo.motives_b;
                o.motive_deltas = lo.motive_deltas;
                o.current = lo.current;
                o.queue = lo.queue;
            }
            saved_ids.push((o.id(), lo.stack.iter().map(|sf| sf.tree).collect()));
            // The original's +8 on a saved frame is on the frame a return comes back into (281
            // under a delivered interaction, a routing step that pushed a door's portal): such a
            // return takes no transition. Here that is the returning frame's `engine`: each
            // saved flag moves to the frame above it (the home log's Sim 237: 8207's return into
            // the counter's 8208 node 13 goes on to node 8, a call's true way).
            let above: Vec<bool> = std::iter::once(false).chain(lo.stack.iter().map(|sf| sf.engine)).collect();
            for (k, sf) in lo.stack.into_iter().enumerate() {
                o.stack.push(Frame {
                    tree: self.lib.tree(o.file, sf.tree),
                    node: sf.node,
                    params: sf.params,
                    locals: sf.locals,
                    stack_object: sf.stack_object,
                    file: o.file,
                    engine: above[k],
                    ..Default::default()
                });
            }
            self.add(o, false);
        }
        // A saved frame above an IdleForInput node is the interaction it delivered: its tree is
        // in its target's file (the frame's stack object), not the Sim's own (the paper girl's
        // 4100 on object 8); the frames it called resolve in their own stack object's file where
        // it has the tree, else in the one below's (Sim 237: the meal's 4201, then on the counter
        // 54 8259 from food_sg, 8208 and 8207 from counter_sg). Once all objects are in (a
        // target may load after its Sim).
        for (id, ids) in saved_ids {
            let Some(k) = self.find(id) else { continue };
            let mut carried: Option<FileId> = None;
            let n = self.get(k).stack.len().min(ids.len());
            for (i, &saved_tree) in ids.iter().enumerate().take(n).skip(1) {
                let below = &self.get(k).stack[i - 1];
                let is_idle = below.tree.and_then(|t| self.lib.get_tree(t)?.nodes.get(below.node as usize).copied()).is_some_and(|n| n.opcode == 17);
                let so = self.get(k).stack[i].stack_object;
                if is_idle && so != id {
                    carried = None;
                } else if carried.is_none() {
                    continue;
                }
                let own = self.find_object(so).and_then(|t| t.file);
                let pick = own.filter(|&f| self.lib.tree(Some(f), saved_tree).is_some()).or(carried);
                let Some(file) = pick else { continue };
                if let Some(t) = self.lib.tree(Some(file), saved_tree) {
                    let f = &mut self.get_mut(k).stack[i];
                    f.tree = Some(t);
                    f.file = Some(file);
                    carried = Some(file);
                }
            }
        }
        self.walls = Walls::new(&read_lot_arrays(lot)?, self);
        Ok(missing)
    }

    /// A lot whose save keeps no walls (the club, lot 15, in a save: its arrays' wall bits all
    /// clear, its floors sparse) is built on its house as the disc has it: the walls, floors and
    /// insides from `house`'s arrays, the objects the save's. True if it took them.
    ///
    /// So are the controllers off the lot (its objects at -24, -24 that are not persons): their
    /// stacks are the disc's copy's, the save's left (the club's dream controller, saved at 4104
    /// node 9 in an Idle of 10000, is at node 32 in an Idle of 1 at the original's first tick, as
    /// the disc has it; an observer record of its stack then, 'k').
    pub fn take_house_from(&mut self, house: &LotChunks) -> Result<bool, FormatError> {
        if self.walls.wall_edges() != 0 {
            return Ok(false);
        }
        self.walls = Walls::new(&read_lot_arrays(house)?, self);
        let mut disc = World::new(self.lib);
        disc.load_lot(house)?;
        let keys: Vec<ObjectKey> = self.objects().filter(|o| !o.on_lot() && !o.person).filter_map(|o| self.find(o.id())).collect();
        for k in keys {
            let (id, guid) = (self.get(k).id(), self.get(k).guid);
            if let Some(d) = disc.find_object(id).filter(|d| d.guid == guid) {
                self.get_mut(k).stack = d.stack.clone();
            }
        }
        Ok(true)
    }

    /// The class the original's loader makes of an object of this definition (none: a
    /// character's): `docs/lots.md`, "The classes".
    pub fn saved_class(d: Option<&Definition>) -> SavedClass {
        // OBJD field 8, the master id (a multi-tile object's parts have one); field 7, the type
        // (8 a door). No definition (a character, made at run time) or a person's (type 2: an
        // NPC with a file of its own, the paper girl): a Sim.
        match d {
            None => SavedClass::Sim,
            Some(d) if d.field(7) == 2 => SavedClass::Sim,
            Some(d) if d.field(8) != 0 => {
                if d.field(7) == 8 { SavedClass::Door } else { SavedClass::Part }
            }
            Some(_) => SavedClass::Object,
        }
    }

    /// A lot unloaded: every object gone, the shared tables kept. The objects' keys are not
    /// used again (what keeps a run by key cannot take a new object for an old one).
    pub fn clear(&mut self) {
        self.slots.iter_mut().for_each(|s| *s = None);
        self.order.clear();
        self.by_id.clear();
        self.id_index.fill(None);
        self.version += 1;
    }

    /// The next id: the lowest not in use (a removed object's id comes back first).
    /// The clock after a tick's pass (the original's 0x80237D18 and 0x80237D60): a pass stopped
    /// at a dialog does not count; one that ran puts the second (global 6) up by 2, at 60 back to
    /// 0 and the minute (global 5) up, at 60 back to 0 and the hour (global 0) up. Measured: the
    /// home log's minute rises after the 30th unstopped pass (tick 93, the passes of 22-83
    /// stopped at the opening dialog), and stands still for the 2,864 stopped at the later one;
    /// the club's (from 18:15:24) at tick 18, then every 30. The time of day's light reads the
    /// three (`light::time_of_day`). (The day and midnight are not reached in the logs.)
    pub fn clock_pass(&mut self, stopped: bool) {
        if stopped {
            return;
        }
        if self.globals.len() <= 6 {
            self.globals.resize(7, 0);
        }
        self.globals[6] += 2;
        if self.globals[6] < 60 {
            return;
        }
        self.globals[6] = 0;
        self.globals[5] += 1;
        if self.globals[5] >= 60 {
            self.globals[5] = 0;
            self.globals[0] += 1;
        }
    }

    pub fn next_id(&self) -> i16 {
        // the lowest id not in use: a removed object's id comes back first (the home log's paper
        // girl, made again as 197, the newspaper as 201 below higher ids in use)
        let used: std::collections::BTreeSet<i16> = self.objects().map(Object::id).collect();
        (1..=i16::MAX).find(|i| !used.contains(i)).unwrap_or(i16::MAX)
    }

    /// An object made during play (CreateObject, the engine's effects): the next id, in front
    /// of the list.
    pub fn make_object(&mut self, mut o: Object) -> ObjectKey {
        if o.data.len() <= DATA_OBJECT_ID {
            o.data.resize(DATA_OBJECT_ID + 1, 0);
        }
        o.data[DATA_OBJECT_ID] = self.next_id();
        o.made = true;
        self.add(o, true)
    }

    pub fn remove(&mut self, k: ObjectKey) {
        self.unindex(k);
        self.order.retain(|&o| o != k);
        self.slots[k.0 as usize] = None;
        self.version += 1;
    }

    /// By object id (data 11).
    pub fn find(&self, id: i16) -> Option<ObjectKey> {
        self.id_index[(id as i32 + 32768) as usize]
    }
    pub fn find_object(&self, id: i16) -> Option<&Object> {
        self.find(id).map(|k| self.get(k))
    }

    /// Called when an object's id changes (its data 11 is written).
    pub fn reindex(&mut self, k: ObjectKey) {
        self.unindex(k);
        let id = self.get(k).id();
        if id != 0 {
            self.by_id.insert(id, k);
            self.id_index[(id as i32 + 32768) as usize] = Some(k);
        }
        self.version += 1;
    }

    /// Every id that names the object no longer does.
    fn unindex(&mut self, k: ObjectKey) {
        let index = &mut self.id_index;
        self.by_id.retain(|&id, v| {
            if *v == k {
                index[(id as i32 + 32768) as usize] = None;
            }
            *v != k
        });
    }

    /// The objects by id, and a count that changes whenever an object comes, goes or changes its
    /// id (for those that keep their own order of the objects, as the simulator does).
    pub fn by_id(&self) -> &BTreeMap<i16, ObjectKey> {
        &self.by_id
    }
    pub fn version(&self) -> u64 {
        self.version
    }
}

/// A table's entry, the table grown to reach it (the counts are each object's own; to be taken
/// from its definition).
fn at<T: Default + Clone>(v: &mut Vec<T>, i: i32) -> Option<&mut T> {
    if !(0..=4096).contains(&i) {
        return None;
    }
    let i = i as usize;
    if i >= v.len() {
        v.resize(i + 1, T::default());
    }
    Some(&mut v[i])
}

/// Where a variable is stored: the table, by its owner, so that two scopes reaching the same
/// storage give the same cell (for checks against the original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Table {
    Attributes(Option<ObjectKey>),
    Data(Option<ObjectKey>),
    PersonData(Option<ObjectKey>),
    Motives(Option<ObjectKey>),
    Temps(Option<ObjectKey>),
    SpriteFlags(Option<ObjectKey>),
    Globals,
    Neighbourhood,
    AdRange,
    Params,
    Locals,
    StackObject,
}

impl Table {
    /// The object whose table this is, for an object's table.
    pub fn owner(&self) -> Option<Option<ObjectKey>> {
        match *self {
            Table::Attributes(o) | Table::Data(o) | Table::PersonData(o) | Table::Motives(o) | Table::Temps(o) | Table::SpriteFlags(o) => Some(o),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub table: Table,
    pub index: i32,
    pub is_float: bool,
}

/// The variables a primitive of `me` sees while running `frame`.
pub struct ScriptVariables<'a, 'lib> {
    pub world: &'a mut World<'lib>,
    pub me: ObjectKey,
    pub frame: &'a mut Frame,
}

impl<'a, 'lib> ScriptVariables<'a, 'lib> {
    pub fn new(world: &'a mut World<'lib>, me: ObjectKey, frame: &'a mut Frame) -> Self {
        ScriptVariables { world, me, frame }
    }

    fn stack_object(&self) -> Option<ObjectKey> {
        self.world.find(self.frame.stack_object)
    }

    fn cell16(&mut self, scope: u8, d: i16) -> Option<&mut i16> {
        use scope::*;
        let di = d as i32;
        let me = self.me;
        match scope {
            MY_ATTRIBUTE => at(&mut self.world.get_mut(me).attributes, di),
            STACK_ATTRIBUTE => at(&mut self.world.get_mut(self.stack_object()?).attributes, di),
            MY_DATA => {
                if d as usize == DATA_LOCKOUT {
                    let id = self.world.get(me).id();
                    self.world.wakes.push((id, Wake::Lockout));
                }
                at(&mut self.world.get_mut(me).data, di)
            }
            STACK_DATA => {
                let k = self.stack_object()?;
                if d as usize == DATA_LOCKOUT {
                    let id = self.world.get(k).id();
                    self.world.wakes.push((id, Wake::Lockout));
                }
                at(&mut self.world.get_mut(k).data, di)
            }
            GLOBAL => at(&mut self.world.globals, di),
            TEMP => self.world.get_mut(me).temps.get_mut(usize::try_from(d).ok()?),
            PARAMETER => self.frame.params.get_mut(usize::try_from(d).ok()?),
            STACK_OBJECT => Some(&mut self.frame.stack_object),
            MY_PERSON_DATA => {
                let o = self.world.get_mut(me);
                if o.person { at(&mut o.person_data, di) } else { None }
            }
            STACK_PERSON_DATA => {
                let o = self.world.get_mut(self.stack_object()?);
                if o.person { at(&mut o.person_data, di) } else { None }
            }
            STACK_ATTRIBUTE_BY_PARAMETER => {
                let p = *self.frame.params.get(usize::try_from(d).ok()?)?;
                at(&mut self.world.get_mut(self.stack_object()?).attributes, p as i32)
            }
            LOCAL => self.frame.locals.get_mut(usize::try_from(d).ok()?),
            STACK_SPRITE_FLAGS => {
                // the stack object's flag word numbered by my temp `d`
                let t = *self.world.get(me).temps.get(usize::try_from(d).ok()?)? as i32;
                at(&mut self.world.get_mut(self.stack_object()?).sprite_flags, t)
            }
            MY_PERSON_DATA_BY_TEMP => {
                let o = self.world.get_mut(me);
                if !o.person || !(0..8).contains(&d) {
                    return None;
                }
                let t = o.temps[d as usize] as i32;
                at(&mut o.person_data, t)
            }
            NEIGHBOURHOOD => at(&mut self.world.neighbourhood, di),
            CHECK_TREE_AD_RANGE => at(&mut self.world.ad_range, di),
            _ => None,
        }
    }

    fn cell_float(&mut self, scope: u8, d: i16) -> Option<&mut f32> {
        let k = match scope {
            scope::MY_MOTIVE => self.me,
            scope::STACK_MOTIVE => self.stack_object()?,
            _ => return None,
        };
        let o = self.world.get_mut(k);
        if o.person { at(&mut o.motives, d as i32) } else { None }
    }

    /// Where a variable is: its cell, for the scopes that are storage (nothing for literals,
    /// constants and scopes not modelled yet). For checks against the original.
    pub fn cell(&self, scope: u8, d: i16) -> Option<Cell> {
        use scope::*;
        let (me, so) = (Some(self.me), self.stack_object());
        let c = |table, index| Some(Cell { table, index, is_float: false });
        let di = d as i32;
        match scope {
            MY_ATTRIBUTE => c(Table::Attributes(me), di),
            STACK_ATTRIBUTE => c(Table::Attributes(so), di),
            MY_DATA => c(Table::Data(me), di),
            STACK_DATA => c(Table::Data(so), di),
            MY_PERSON_DATA => c(Table::PersonData(me), di),
            STACK_PERSON_DATA => c(Table::PersonData(so), di),
            MY_MOTIVE => Some(Cell { table: Table::Motives(me), index: di, is_float: true }),
            STACK_MOTIVE => Some(Cell { table: Table::Motives(so), index: di, is_float: true }),
            TEMP => c(Table::Temps(me), di),
            GLOBAL => c(Table::Globals, di),
            NEIGHBOURHOOD => c(Table::Neighbourhood, di),
            CHECK_TREE_AD_RANGE => c(Table::AdRange, di),
            PARAMETER => c(Table::Params, di),
            LOCAL => c(Table::Locals, di),
            STACK_SPRITE_FLAGS => c(Table::SpriteFlags(so), *self.world.get(self.me).temps.get(usize::try_from(d).ok()?)? as i32),
            STACK_OBJECT => c(Table::StackObject, 0),
            STACK_ATTRIBUTE_BY_PARAMETER => {
                let p = *self.frame.params.get(usize::try_from(d).ok()?)?;
                c(Table::Attributes(so), p as i32)
            }
            MY_PERSON_DATA_BY_TEMP => {
                if !(0..8).contains(&d) {
                    return None;
                }
                c(Table::PersonData(me), self.world.get(self.me).temps[d as usize] as i32)
            }
            _ => None,
        }
    }
}

impl Variables for ScriptVariables<'_, '_> {
    fn get(&mut self, scope: u8, d: i16) -> Option<Variable> {
        let int = |value| Some(Variable { value, f: None });
        match scope {
            scope::LITERAL => return int(d),
            scope::ROOM_BY_TEMP0 => {
                // A room's datum d, the room my temp 0 names (the PC game's "Room by temp 0").
                // Datum 1 is whether it is the outside, room 0: the 12 reads of the home log
                // (tree 301 node 1, the doors' room-changed tree 8218 and the newspaper), where
                // temp 0 is the stack object's room (data 29): 1 for room 0, 0 for rooms 1 to 3.
                // The other data are not read by any script of the logs: not modelled.
                if d != 1 {
                    return None;
                }
                let room = *self.world.get(self.me).temps.first()?;
                return int(i16::from(room == 0));
            }
            scope::NEIGHBOUR_IN_STACK_OBJECT => {
                // The stack object holds a neighbour id; field 0 is the id of the person on the
                // lot with that neighbour id (person data 31), 0 if none is here. Measured: all
                // 30,684 reads of the home log (docs/objects.md, "The neighbours").
                if d != 0 {
                    return None;
                }
                let so = self.frame.stack_object;
                let id = self
                    .world
                    .objects()
                    .find(|o| o.person && so != 0 && o.person_data.get(PERSON_NEIGHBOUR_ID) == Some(&so))
                    .map_or(0, Object::id);
                return int(id);
            }
            scope::NEIGHBOUR_DATA_OF_STACK_OBJECT => {
                // the person data of the neighbour whose id the stack object holds (the
                // original's InterpValue: the neighbourhood's record by that id, its 16-bit field
                // d, d below 79)
                let so = self.frame.stack_object;
                if !(0..79).contains(&d) {
                    return None;
                }
                // no such neighbour: 0 (the original's lookup fails with 0x37 and gives 0: the
                // club log's 45 reads of ids 4, 6, 7, 23, 56)
                let v = self.world.neighbours.iter().find(|n| n.id == so).and_then(|n| n.person_data.get(d as usize).copied()).unwrap_or(0);
                return int(v);
            }
            scope::STACK_SLOT | scope::MY_SLOT => {
                // The object in the stack object's (16) or my (20) slot d: the one whose container
                // field (data 2) is that object and whose slot field (data 3) is d; 0 if none
                let so = if scope == scope::MY_SLOT { self.world.get(self.me).id() } else { self.frame.stack_object };
                // a person's slot 0 is its hand: whatever it holds, whichever slot field
                let hand = d == 0 && self.world.find_object(so).is_some_and(|o| o.person);
                let id = self
                    .world
                    .objects()
                    .find(|o| so != 0 && o.id() != so && o.data.get(DATA_CONTAINER) == Some(&so) && (hand || o.data.get(DATA_SLOT) == Some(&d)))
                    .map_or(0, Object::id);
                return int(id);
            }
            scope::STACK_FUNCTION => {
                // whether the stack object has function d: its file's function table (OBJf)
                // holds an action tree for it
                let so = self.world.get(self.stack_object()?);
                let f = self.world.library().file(so.file?);
                let has = f.functions.values().next().and_then(|t| t.get(usize::try_from(d).ok()?)).is_some_and(|&(_, a)| a != 0);
                return int(i16::from(has));
            }
            scope::MY_RELATIONSHIP | scope::STACK_RELATIONSHIP => {
                // A relationship's value d: mine with the stack object (40), or the stack
                // object's with me (41); 0 when there is none or it is shorter
                let (me_id, so) = (self.world.get(self.me).id(), self.frame.stack_object);
                let (from, to) = if scope == scope::MY_RELATIONSHIP { (me_id, so) } else { (so, me_id) };
                let v = self
                    .world
                    .find_object(from)
                    .and_then(|o| o.relationships.iter().find(|r| r.with == to as i32))
                    .and_then(|r| usize::try_from(d).ok().and_then(|d| r.values.get(d)))
                    .map_or(0, |&v| v as i16);
                return int(v);
            }
            scope::STACK_DEFINITION => {
                // The stack object's definition as the engine keeps it: a 32-bit version, then
                // the OBJD's fields, the datum counting 16-bit words from the start (so field
                // datum - 2).
                let so = self.world.get(self.stack_object()?);
                // a character has no definition on the disc (the engine makes one): its file's
                // (the template person's) stands in for it
                let lib = self.world.library();
                let def = self.world.definition(so).or_else(|| so.file.and_then(|f| lib.file(f).definitions.first()))?;
                return match d {
                    ..0 => None,
                    0 => int((def.version >> 16) as i16),
                    1 => int((def.version & 0xFFFF) as i16),
                    _ => int(def.field(d as usize - 2) as i16),
                };
            }
            scope::CONSTANT => {
                // From the frame's file: the tree's, not necessarily the object's own (a Sim
                // running a fridge's interaction reads the fridge's constants).
                // from the frame's file (the tree's: a Sim running a fridge's interaction
                // reads the fridge's constants), else my own (a semi-global tree's frame reading
                // an own table)
                let lib = self.world.library();
                let mine = self.world.get(self.me).file;
                let theirs = self.stack_object().and_then(|k| self.world.get(k).file);
                return int(lib.constant(self.frame.file.or(mine), d).or_else(|| lib.constant(mine, d)).or_else(|| lib.constant(theirs, d))?);
            }
            _ => {}
        }
        if let Some(&mut f) = self.cell_float(scope, d) {
            // to the nearest (19.5 and up reads 20): the home log's 4155 reads of 19.5, 19.875,
            // 19.92 as 20; truncating put 248 expressions and 50 motive reads off
            let v = (f + 0.5).floor() as i32 as i16;
            return Some(Variable { value: v, f: Some(f) });
        }
        let v = *self.cell16(scope, d)?;
        int(v)
    }

    fn set(&mut self, scope: u8, d: i16, value: i16) -> bool {
        let Some(p) = self.cell16(scope, d) else { return false };
        *p = value;
        // a local written is no longer unwritten (Frame::unset)
        if scope == scope::LOCAL && (0..64).contains(&d) {
            self.frame.unset &= !(1u64 << d);
        }
        if d as usize == DATA_OBJECT_ID && d >= 0 {
            if scope == scope::MY_DATA {
                self.world.reindex(self.me);
            }
            if scope == scope::STACK_DATA
                && let Some(s) = self.stack_object()
            {
                self.world.reindex(s);
            }
        }
        true
    }

    fn set_float(&mut self, scope: u8, d: i16, value: f32) -> bool {
        let Some(f) = self.cell_float(scope, d) else { return false };
        *f = value;
        true
    }
}
