//! Primitives other than the expression, on worlds made here (SetToNext's and Idle's rules,
//! `docs/objects.md` and `docs/primitives.md`; checked against the recompiled game by lockstep):
//! the C++ engine's `test_primitives`, check for check. Its `me`, an object outside the world
//! there, is the world's object 5 here (a primitive's object is always one of the world's), the
//! id the C++ test gives it.

use housefire::bhav::Node;
use housefire::iff::Order;
use housefire::interp::{Frame, PrimResult, Sleep};
use housefire::library::Library;
use housefire::primitives::{dialog, idle, present, random, set_to_next, test_object_type};
use housefire::world::{DATA_OBJECT_ID, Object, ObjectKey, ScriptVariables, World};

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

    // SetToNext over the list's order (not by id), from 0: any object (after the last, 0 and
    // false), then the Sims, then a GUID
    let any = [0; 8];
    f.stack_object = 0;
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 5);
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
    assert!(stn(any, &mut w, &mut f) == PrimResult::True && f.stack_object == 3);
    assert!(stn(any, &mut w, &mut f) == PrimResult::False && f.stack_object == 0);
    f.stack_object = 3;
    f.stack_object = 0;
    assert!(stn([0, 0, 0, 0, 1, 0, 0, 0], &mut w, &mut f) == PrimResult::True && f.stack_object == 9);
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
    assert_eq!(idle(&mut s, &mut g, &sleep), PrimResult::Yield);
    let mut passed = 0;
    while s.asleep() {
        s.pass_over();
        passed += 1;
    }
    assert_eq!(passed, 29);
    assert!(idle(&mut s, &mut g, &sleep) == PrimResult::True && g.params[0] == 0);
    g.params = vec![0];
    assert_eq!(idle(&mut s, &mut g, &sleep), PrimResult::True);
}
