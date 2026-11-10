//! The simulation tick's order (`docs/simulation.md`) on objects and trees written here: the C++
//! engine's `test_simulator`, check for check. Its host asks the pass to stop through the world
//! (`World::stop_pass`, as a dialog does), which the C++ tick reads too.

use housefire::TreeId;
use housefire::bhav::{Node, Tree};
use housefire::iff::Order;
use housefire::interp::{Frame, Host, Interpreter, PrimResult, Trees};
use housefire::library::Library;
use housefire::simulator::{SimHost, Simulator};
use housefire::world::{DATA_OBJECT_ID, Object, ObjectKey, World};

fn n(op: u16, t: u8, f: u8) -> Node {
    Node { opcode: op, true_target: t, false_target: f, operand: [0; 8], order: Order::Big }
}

const MAIN: TreeId = TreeId(0);
const SLOW: TreeId = TreeId(1);

/// Primitive 1: note the object's id and yield. Primitive 2: sleep two ticks, then go on to
/// node 0 (Idle's way). Primitive 3: make a new object running the same tree, then go on.
/// Primitive 4: note the id and go on.
struct TestHost<'lib> {
    world: World<'lib>,
    trees: Vec<Tree>,
    me: ObjectKey,
    /// Ids, in the order primitive 1 saw them.
    turns: Vec<i16>,
    made: bool,
    stop_at: i16,
}

impl Trees for TestHost<'_> {
    fn tree_at(&self, t: TreeId) -> &Tree {
        &self.trees[t.0 as usize]
    }
}

impl Host for TestHost<'_> {
    fn tree(&mut self, _: u16) -> Option<TreeId> {
        None
    }
    fn primitive(&mut self, interp: &mut Interpreter, node: &Node) -> PrimResult {
        let id = self.world.get(self.me).id();
        match node.opcode {
            1 => {
                self.turns.push(id);
                if id == self.stop_at {
                    self.world.stop_pass = true; // a dialog waiting for the player
                }
                PrimResult::Yield
            }
            4 => {
                self.turns.push(id);
                PrimResult::True
            }
            2 => {
                let s = interp.sleep();
                if s.woke() {
                    s.wake();
                    return PrimResult::True;
                }
                s.sleep(1); // passed over once, stepped on the next pass
                PrimResult::Yield
            }
            3 => {
                if id == 2 && !self.made {
                    self.made = true;
                    let mut o = Object::new();
                    o.data.resize(72, 0);
                    o.stack.push(Frame { tree: Some(MAIN), ..Default::default() });
                    self.world.make_object(o);
                }
                PrimResult::True
            }
            _ => PrimResult::Stop,
        }
    }
}

impl<'lib> SimHost<'lib> for TestHost<'lib> {
    fn world(&mut self) -> &mut World<'lib> {
        &mut self.world
    }
    fn set_me(&mut self, me: ObjectKey) {
        self.me = me;
    }
}

#[test]
fn the_ticks_order() {
    let lib = Library::default();
    let mut h = TestHost {
        world: World::new(&lib),
        trees: vec![
            Tree { id: 4096, signature: 0x8002, nodes: vec![n(3, 1, 1), n(1, 1, 1)], ..Default::default() },
            Tree { id: 4097, signature: 0x8002, nodes: vec![n(4, 1, 1), n(2, 0, 0)], ..Default::default() },
        ],
        me: ObjectKey(0),
        turns: Vec::new(),
        made: false,
        stop_at: 0,
    };
    let mut sim = Simulator::new(&lib);
    let mut add = |id: i16, t: TreeId| {
        let mut o = Object::new();
        o.data.resize(72, 0);
        o.data[DATA_OBJECT_ID] = id;
        o.stack.push(Frame { tree: Some(t), ..Default::default() });
        h.world.add(o, false);
    };
    // The world's list order is not the tick's: 3, 1, 2 in the list, 5 asleep after its first
    // turn.
    add(3, MAIN);
    add(1, MAIN);
    add(2, MAIN);
    add(5, SLOW);
    let tick = |h: &mut TestHost, sim: &mut Simulator| {
        h.turns.clear();
        sim.tick(h);
        h.turns.clone()
    };
    // By id; object 2 makes object 6 (one more than the highest), which waits for the next
    // tick. Object 5 sleeps two ticks from tick 0.
    assert_eq!(tick(&mut h, &mut sim), [1, 2, 3, 5]);
    assert!(h.world.find(6).is_some());
    assert_eq!(tick(&mut h, &mut sim), [1, 2, 3, 6]); // tick 1: 5 asleep
    assert_eq!(tick(&mut h, &mut sim), [1, 2, 3, 5, 6]); // tick 2: 5 wakes, goes round to node 0 and sleeps again
    assert_eq!(tick(&mut h, &mut sim), [1, 2, 3, 6]); // tick 3: asleep
    assert_eq!(sim.now(), 4);
    // A turn that stops the pass: the next pass goes on from that object, and the objects
    // after it are not reached, so 5 (due again, its one pass waited in tick 3) waits.
    h.stop_at = 3;
    assert!(tick(&mut h, &mut sim) == [1, 2, 3] && sim.cursor() == 3); // tick 4: 3 stops the pass
    assert!(tick(&mut h, &mut sim) == [3] && sim.cursor() == 3); // from 3, which stops it again
    h.stop_at = 0;
    assert!(tick(&mut h, &mut sim) == [3, 5, 6] && sim.cursor() == 0); // from 3 to the end: 5 runs and sleeps again, 6
    assert_eq!(tick(&mut h, &mut sim), [1, 2, 3, 6]); // a new pass: 5 passed over
}
