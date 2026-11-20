//! Primitives other than the expression, on worlds made here (SetToNext's and Idle's rules,
//! `docs/objects.md` and `docs/primitives.md`; checked against the recompiled game by lockstep):
//! the C++ engine's `test_primitives`, check for check. Its `me`, an object outside the world
//! there, is the world's object 5 here (a primitive's object is always one of the world's), the
//! id the C++ test gives it.

use housefire::bhav::Node;
use housefire::iff::Order;
use housefire::interp::{Frame, PrimResult, Sleep};
use housefire::library::Library;
use housefire::goals::Goals;
use housefire::primitives::{complete_goal, dialog, idle, idle_for_input_waiting, may_sleep, notify_stack_object, present, random, relationship, set_to_next, test_object_type};
use housefire::save::{Neighbour, NeighbourRelationship, SavedRelationship};
use housefire::expression::Variables;
use housefire::world::{DATA_LOCKOUT, DATA_OBJECT_ID, Object, ObjectKey, PERSON_NEIGHBOUR_ID, ScriptVariables, Wake, World};

fn node(op: u16, operand: [u8; 8]) -> Node {
    Node { opcode: op, true_target: 1, false_target: 2, operand, order: Order::Little }
}

type Prim = fn(&Node, &mut ScriptVariables) -> PrimResult;

fn run(p: Prim, n: [u8; 8], op: u16, w: &mut World, me: ObjectKey, f: &mut Frame) -> PrimResult {
    p(&node(op, n), &mut ScriptVariables::new(w, me, f))
}

#[test]
fn the_primitives_rules() {
    let lib = Library::default();
    let mut w = World::new(&lib);
    let mut make = |id: i16, person: bool, guid: u32| {
        let mut o = Object::new();
        o.data.resize(72, 0);
        o.data[DATA_OBJECT_ID] = id;
        o.person = person;
        o.guid = guid;
        w.add(o, false)
    };
    let me = make(5, false, 0x111);
    make(9, true, 0x222);
    make(3, false, 0x111);
    let mut f = Frame::default();
    let stn = |n, w: &mut World, f: &mut Frame| run(set_to_next, n, 31, w, me, f);
    let set_leaders = |w: &mut World, of: &dyn Fn(i16) -> i16| {
        for k in w.order().to_vec() {
            let id = w.get(k).id();
            w.get_mut(k).leader = of(id);
        }
    };

    // SetToNext over the list's order (not by id), from 0: any object (after the last, false
    // with the target kept), then the Sims (after the last, 0), then a GUID
    let any = [0; 8];
    f.stack_object = 0;
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 5);
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert!(stn(any, &mut w, &mut f) == PrimResult::False && f.stack_object == 3);
    f.stack_object = 0;
    assert!(stn([0, 0, 0, 0, 1, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
    assert!(stn([0, 0, 0, 0, 1, 0, 0, 0], &mut w, &mut f) == PrimResult::False && f.stack_object == 0);
    // kind 4: the next of the GUID by id, not the list's order (5 and 3 are 0x111's: from 0, 3;
    // from 3, 5; from 5, none)
    f.stack_object = 0;
    assert!(stn([0x11, 0x01, 0, 0, 4, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert!(stn([0x11, 0x01, 0, 0, 4, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 5);
    assert!(stn([0x11, 0x01, 0, 0, 4, 0, 0, 0], &mut w, &mut f) == PrimResult::False);
    // kind 3: the next part of the target's multi-tile object in list order, round from the
    // last (the list is 5, 9, 3: 5 and 3 parts of one led by 3)
    set_leaders(&mut w, &|id| if id == 5 || id == 3 { 3 } else { 0 });
    f.stack_object = 5;
    assert!(stn([0, 0, 0, 0, 3, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert!(stn([0, 0, 0, 0, 3, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 5);
    f.stack_object = 9; // not a part
    assert_eq!(stn([0, 0, 0, 0, 3, 0, 0, 0], &mut w, &mut f), PrimResult::False);
    set_leaders(&mut w, &|_| 0);
    // kind 6: the next object whose data field 59 is the frame's first parameter (list 5, 9, 3)
    for k in w.order().to_vec() {
        let id = w.get(k).id();
        w.get_mut(k).data[59] = if id == 9 || id == 3 { 42 } else { 0 };
    }
    f.params = vec![42];
    f.stack_object = 0;
    assert!(stn([0, 0, 0, 0, 6, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
    assert!(stn([0, 0, 0, 0, 6, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert_eq!(stn([0, 0, 0, 0, 6, 0, 0, 0], &mut w, &mut f), PrimResult::False);
    for k in w.order().to_vec() {
        w.get_mut(k).data[59] = 0;
    }
    f.params.clear();
    // kinds 8 and 9 by tile (5 at tile (2, 3), 9 with it, 3 at (3, 3) beside them)
    let place = |w: &mut World, id: i16, x: i32, y: i32| {
        let k = w.find(id).unwrap();
        let o = w.get_mut(k);
        (o.x16, o.y16, o.level) = (x * 16 + 8, y * 16 + 8, 1);
    };
    place(&mut w, 5, 2, 3);
    place(&mut w, 9, 2, 3);
    place(&mut w, 3, 3, 3);
    // kind 8: the next on the target's tile by id, the target among them, round from the last
    f.stack_object = 5;
    assert!(stn([0, 0, 0, 0, 8, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
    assert!(stn([0, 0, 0, 0, 8, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 5);
    f.stack_object = 3; // alone: itself
    assert!(stn([0, 0, 0, 0, 8, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    // kind 9: beside me (5) on +x, +y, -y, -x, after the target's side; none after: false, kept
    f.stack_object = 0;
    assert!(stn([0, 0, 0, 0, 9, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert!(stn([0, 0, 0, 0, 9, 0, 0, 0], &mut w, &mut f) == PrimResult::False && f.stack_object == 3);
    // an object whose flags (data 8) have 4 is not found beside
    let k3 = w.find(3).unwrap();
    w.get_mut(k3).data[8] = 4;
    f.stack_object = 0;
    assert_eq!(stn([0, 0, 0, 0, 9, 0, 0, 0], &mut w, &mut f), PrimResult::False);
    w.get_mut(k3).data[8] = 0;

    // scope 32: the person data of the neighbour whose id the stack object holds; none, 0
    let mut nb = housefire::save::Neighbour { id: 40, ..Default::default() };
    nb.person_data = vec![0; 80];
    nb.person_data[61] = 13;
    w.neighbours = vec![nb];
    f.stack_object = 40;
    assert_eq!(ScriptVariables::new(&mut w, me, &mut f).get(32, 61).map(|v| v.value), Some(13));
    f.stack_object = 41;
    assert_eq!(ScriptVariables::new(&mut w, me, &mut f).get(32, 61).map(|v| v.value), Some(0));
    w.neighbours.clear();

    // into a local (flag 0x80, scope 25, datum 0)
    f.locals = vec![0];
    assert!(stn([0, 0, 0, 0, 0x82, 25, 0, 0], &mut w, &mut f) == PrimResult::True && f.locals[0] == 5);

    // TestObjectType: the stack object (scope 10, datum 0) and a local (scope 25) against a GUID
    let tot = |n, w: &mut World, f: &mut Frame| run(test_object_type, n, 32, w, me, f);
    f.stack_object = 3;
    assert_eq!(tot([0x11, 0x01, 0, 0, 0, 0, 10, 0], &mut w, &mut f), PrimResult::True);
    assert_eq!(tot([0x22, 0x02, 0, 0, 0, 0, 10, 0], &mut w, &mut f), PrimResult::False);
    f.locals = vec![9];
    assert_eq!(tot([0x22, 0x02, 0, 0, 0, 0, 25, 0], &mut w, &mut f), PrimResult::True);
    f.locals = vec![77]; // no such object
    assert_eq!(tot([0x22, 0x02, 0, 0, 0, 0, 25, 0], &mut w, &mut f), PrimResult::False);

    // Random: into a local, a literal range; always below it, and spread over it
    f.locals = vec![0];
    let mut seen = [false; 7];
    for _ in 0..700 {
        assert_eq!(run(random, [0, 0, 25, 0, 7, 0, 7, 0], 8, &mut w, me, &mut f), PrimResult::True);
        assert!((0..7).contains(&f.locals[0]));
        seen[f.locals[0] as usize] = true;
    }
    assert!(seen.iter().all(|&b| b));

    // PlaySound, Update, KillSounds: queued for the presentation, true
    let me_id = w.get(me).id();
    assert_eq!(present(&node(23, [7, 0, 0, 0, 0, 0, 0, 0]), &mut w, me_id), PrimResult::True);
    assert_eq!(present(&node(48, [0; 8]), &mut w, me_id), PrimResult::True);
    assert!(
        w.presentation.len() == 2
            && w.presentation[0].opcode == 23
            && w.presentation[0].object == 5
            && w.presentation[0].operand[0] == 7
            && w.presentation[1].opcode == 48
    );

    // Dialog: waits (and stops the pass) until the player answers, then true
    w.presentation.clear();
    let dn = node(36, [0, 0, 1, 0, 0, 0, 0, 0x80]);
    assert!(dialog(&dn, &mut w, me_id) == PrimResult::Yield && w.stop_pass && w.dialog_object == 5);
    assert_eq!(w.presentation.len(), 1);
    w.stop_pass = false;
    assert!(dialog(&dn, &mut w, me_id) == PrimResult::Yield && w.stop_pass && w.presentation.len() == 1);
    w.dialog_answer = Some(true);
    assert!(dialog(&dn, &mut w, me_id) == PrimResult::True && w.dialog_object == 0);

    // a made object takes the lowest id not in use and goes in front; a removed one's id
    // comes back
    let made = w.make_object(Object::new());
    assert!(w.get(made).id() == 1 && w.order()[0] == made);
    let two = w.make_object(Object::new());
    assert!(w.get(two).id() == 2);
    w.remove(made);
    let again = w.make_object(Object::new());
    assert!(w.get(again).id() == 1 && w.order()[0] == again);
    w.remove(again);
    w.remove(two);

    // Idle: passed over by the next 29 passes that reach it, stepped on the 30th; then the
    // parameter is 0 and the result true
    let mut s = Sleep::default();
    let mut g = Frame { params: vec![30], ..Default::default() };
    let sleep = node(0, [0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(idle(&mut s, &mut g, &sleep, true), PrimResult::Yield);
    let mut passed = 0;
    while s.asleep() {
        s.pass_over();
        passed += 1;
    }
    assert_eq!(passed, 29);
    assert!(idle(&mut s, &mut g, &sleep, true) == PrimResult::True && g.params[0] == 0);
    g.params = vec![0];
    assert_eq!(idle(&mut s, &mut g, &sleep, true), PrimResult::True);
    // a stack that may not sleep (a person's, say): stepped each pass, the parameter counting
    // down; at 0 true
    g.params = vec![3];
    for left in [2, 1, 0] {
        assert!(idle(&mut s, &mut g, &sleep, false) == PrimResult::Yield && g.params[0] == left && !s.asleep());
    }
    assert_eq!(idle(&mut s, &mut g, &sleep, false), PrimResult::True);
}

#[test]
fn complete_goal_reads_the_goals_mark() {
    let lib = Library::default();
    let mut w = World::new(&lib);
    let mut o = Object::new();
    o.data.resize(72, 0);
    let me = w.add(o, false);
    // house 1's goals as the disc's (marks 0x21, 0x72, 0x70), the save's progress: mark 0x21
    w.goals = Goals { houses: vec![vec![0x87], vec![0x21, 0x72, 0x70]], met: vec![0, 0, 2] };
    let mut f = Frame::default();
    // old form (byte 6 bit 0), house and goal as literals (scope 7)
    let ask = |house: u8, goal: u8, flags: u8, w: &mut World, f: &mut Frame| {
        complete_goal(&node(56, [house, 0, goal, 0, 7, 7, flags, 0]), &mut ScriptVariables::new(w, me, f))
    };
    assert_eq!(ask(1, 0, 1, &mut w, &mut f), Some(PrimResult::True));
    assert_eq!(ask(1, 1, 1, &mut w, &mut f), Some(PrimResult::False));
    assert_eq!(ask(0, 0, 1, &mut w, &mut f), Some(PrimResult::False));
    // no such goal or house: the original's script error
    assert_eq!(ask(1, 3, 1, &mut w, &mut f), Some(PrimResult::Stop));
    assert_eq!(ask(2, 0, 1, &mut w, &mut f), Some(PrimResult::Stop));
    // the new form is not modelled
    assert_eq!(ask(1, 0, 0, &mut w, &mut f), None);
}

#[test]
fn idle_for_input_counts_its_wait_down() {
    // the count in the parameter operand bytes 0-1 name: down by one a tick, then true
    let n = node(17, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut f = Frame { params: vec![9, 2], ..Frame::default() };
    let mut flags = 0x48;
    assert_eq!(idle_for_input_waiting(&n, &mut f, &mut flags, false), PrimResult::Yield);
    assert_eq!(idle_for_input_waiting(&n, &mut f, &mut flags, false), PrimResult::Yield);
    assert_eq!(flags, 0x48);
    assert_eq!(idle_for_input_waiting(&n, &mut f, &mut flags, false), PrimResult::True);
    assert_eq!(flags, 0x08);
    assert_eq!(f.params, vec![9, 0]);
    // a parameter the frame does not have: the original's script error
    assert_eq!(idle_for_input_waiting(&node(17, [2, 0, 0, 0, 0, 0, 0, 0]), &mut f, &mut flags, false), PrimResult::Stop);
    // something queued, a wait that does not deliver: the flag set, true at once, the count kept
    let mut f = Frame { params: vec![9, 2], ..Frame::default() };
    assert_eq!(idle_for_input_waiting(&n, &mut f, &mut flags, true), PrimResult::True);
    assert_eq!((flags, f.params[1]), (0x48, 2));
}

#[test]
fn relationship_reads_writes_and_creates() {
    let lib = Library::default();
    let mut w = World::new(&lib);
    let mut make = |id: i16, nid: Option<i16>, rels: Vec<SavedRelationship>| {
        let mut o = Object::new();
        o.data.resize(72, 0);
        o.data[DATA_OBJECT_ID] = id;
        if let Some(nid) = nid {
            o.person = true;
            o.person_data.resize(PERSON_NEIGHBOUR_ID + 1, 0);
            o.person_data[PERSON_NEIGHBOUR_ID] = nid;
        }
        o.relationships = rels;
        w.add(o, false)
    };
    let me = make(12, Some(3), vec![SavedRelationship { with: 7, values: vec![0, 2] }]);
    make(7, None, Vec::new());
    make(14, Some(5), Vec::new());
    w.neighbours = vec![
        Neighbour { id: 3, relationships: vec![NeighbourRelationship { with: 5, values: vec![40], ..Default::default() }], ..Neighbour::default() },
        Neighbour { id: 5, ..Neighbour::default() },
    ];
    let mut f = Frame { locals: vec![0], ..Frame::default() };
    // operand: field, who to whom, flags (1 no create, 2 neighbours, 4 write), local, scope, -, datum
    let run = |op: [u8; 8], so: i16, w: &mut World, f: &mut Frame| {
        f.stack_object = so;
        relationship(&node(26, op), &mut ScriptVariables::new(w, me, f))
    };
    // my own with an object: field 1 into local 0
    assert_eq!(run([1, 0, 1, 0, 25, 0, 0, 0], 7, &mut w, &mut f), PrimResult::True);
    assert_eq!(f.locals[0], 2);
    // no field 2, not creating: false
    assert_eq!(run([2, 0, 1, 0, 25, 0, 0, 0], 7, &mut w, &mut f), PrimResult::False);
    // two Sims: the neighbours' relationship
    assert_eq!(run([0, 0, 1, 0, 25, 0, 0, 0], 14, &mut w, &mut f), PrimResult::True);
    assert_eq!(f.locals[0], 40);
    // the other way there is none; creating makes it, zero
    assert_eq!(run([0, 1, 1, 0, 25, 0, 0, 0], 14, &mut w, &mut f), PrimResult::False);
    assert_eq!(run([0, 1, 0, 0, 25, 0, 0, 0], 14, &mut w, &mut f), PrimResult::True);
    assert_eq!(f.locals[0], 0);
    // writing a literal into it (no create needed now)
    assert_eq!(run([0, 1, 5, 0, 7, 0, 9, 0], 14, &mut w, &mut f), PrimResult::True);
    assert_eq!(w.neighbours[1].relationships[0].values, vec![9]);
    // a write does not create: false
    assert_eq!(run([3, 1, 5, 0, 7, 0, 9, 0], 14, &mut w, &mut f), PrimResult::False);
}

#[test]
fn the_lockout_keeps_an_idle_awake_and_wakes_it() {
    let lib = Library::default();
    let mut w = World::new(&lib);
    let mut o = Object::new();
    o.data.resize(72, 0);
    o.data[DATA_OBJECT_ID] = 4;
    // a person never sleeps; an object not while its lockout is set (or an animation is on it)
    assert!(may_sleep(&o, false) && !may_sleep(&o, true));
    o.data[DATA_LOCKOUT] = 3;
    assert!(!may_sleep(&o, false));
    o.data[DATA_LOCKOUT] = 0;
    o.person = true;
    assert!(!may_sleep(&o, false));
    o.person = false;
    let me = w.add(o, false);
    // an expression that touches an object's lockout asks for it to be woken
    let mut f = Frame::default();
    {
        let mut v = ScriptVariables::new(&mut w, me, &mut f);
        assert_eq!(v.get(3, DATA_LOCKOUT as i16).map(|x| x.value), Some(0));
    }
    assert_eq!(w.wakes, vec![(4, Wake::Lockout)]);
    // NotifyStackObject asks for the stack object's to end; no such object: an error
    w.wakes.clear();
    f.stack_object = 4;
    assert_eq!(notify_stack_object(&mut ScriptVariables::new(&mut w, me, &mut f)), PrimResult::True);
    assert_eq!(w.wakes, vec![(4, Wake::Ended)]);
    f.stack_object = 9;
    assert_eq!(notify_stack_object(&mut ScriptVariables::new(&mut w, me, &mut f)), PrimResult::Stop);
    // woken early, the sleep's parameter is what was left, and the Idle goes on with it
    let sleep = node(0, [0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    let mut s = Sleep::default();
    let mut g = Frame { params: vec![10], ..Default::default() };
    assert_eq!(idle(&mut s, &mut g, &sleep, true), PrimResult::Yield);
    s.pass_over();
    s.pass_over();
    s.wake_early(&mut g);
    assert!(!s.asleep() && g.params[0] == 7);
    assert_eq!(idle(&mut s, &mut g, &sleep, false), PrimResult::Yield);
    assert_eq!(g.params[0], 6);
    // ended (NotifyStackObject): true when next stepped
    assert_eq!(idle(&mut s, &mut g, &sleep, true), PrimResult::Yield);
    s.end();
    assert!(idle(&mut s, &mut g, &sleep, true) == PrimResult::True && g.params[0] == 0);
}
