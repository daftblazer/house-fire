//! The node step's rules (`docs/interpreter.md`) on trees written here, not taken from a game:
//! the C++ engine's `test_interp`, case for case.

use std::collections::BTreeMap;

use housefire::TreeId;
use housefire::bhav::{Node, RETURN_FALSE, RETURN_TRUE, TARGET_ERROR, Tree, TreeKind, tree_kind};
use housefire::iff::Order;
use housefire::interp::{Error, Host, Interpreter, Mode, PrimResult, Step, Trees};

/// A node: opcode, targets, operand as four 16-bit fields.
fn node(op: u16, t: u8, f: u8, v: [i16; 4]) -> Node {
    let mut operand = [0u8; 8];
    for (i, x) in v.iter().enumerate() {
        operand[2 * i..2 * i + 2].copy_from_slice(&x.to_be_bytes());
    }
    Node { opcode: op, true_target: t, false_target: f, operand, order: Order::Big }
}

fn n(op: u16, t: u8, f: u8) -> Node {
    node(op, t, f, [0; 4])
}

fn tree(id: i16, nodes: Vec<Node>, args: u8, locals: u16) -> Tree {
    Tree { id, signature: 0x8002, args, locals, nodes, ..Tree::default() }
}

/// Primitive 1 answers what `answers` says next; primitive 2 yields once, then answers true;
/// primitive 3 sets local 0 to parameter 0 and answers true; primitives 4 and 5 answer whether
/// local 1 (4) or local 0 (5) is zero.
#[derive(Default)]
struct TestHost {
    trees: Vec<Tree>,
    by_id: BTreeMap<u16, TreeId>,
    answers: Vec<PrimResult>,
    defaults: Vec<i16>,
    next: usize,
    yields: i32,
    unset_reads: Vec<usize>,
}

impl TestHost {
    /// A tree the host keeps, callable by its id if `callable`.
    fn add(&mut self, t: Tree, callable: bool) -> TreeId {
        let id = TreeId(self.trees.len() as u32);
        if callable {
            self.by_id.insert(t.id as u16, id);
        }
        self.trees.push(t);
        id
    }
}

impl Trees for TestHost {
    fn tree_at(&self, t: TreeId) -> &Tree {
        &self.trees[t.0 as usize]
    }
}

impl Host for TestHost {
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        self.by_id.get(&id).copied()
    }
    fn primitive(&mut self, interp: &mut Interpreter, n: &Node) -> PrimResult {
        let top = interp.top_index();
        match n.opcode {
            1 => {
                let a = self.answers.get(self.next).copied().unwrap_or(PrimResult::Stop);
                self.next += 1;
                a
            }
            2 => {
                self.yields += 1;
                if self.yields == 1 { PrimResult::Yield } else { PrimResult::True }
            }
            3 => {
                if let Some(&p) = interp.stack()[top].params.first() {
                    interp.set_local(top, 0, p);
                }
                PrimResult::True
            }
            4 | 5 => {
                let k = if n.opcode == 4 { 1 } else { 0 };
                if interp.local(self, top, k) == 0 { PrimResult::True } else { PrimResult::False }
            }
            _ => PrimResult::Stop,
        }
    }
    fn default_params(&self) -> &[i16] {
        &self.defaults
    }
    fn unset_local(&mut self, _: &Interpreter, _: usize, k: usize) {
        self.unset_reads.push(k);
    }
}

fn check() -> Interpreter {
    Interpreter::new(Mode::Check, 10000)
}

#[test]
fn targets_and_a_return_in_a_check_tree() {
    let mut h = TestHost { answers: vec![PrimResult::True, PrimResult::False], ..Default::default() };
    let t = h.add(tree(0x1000, vec![n(1, 1, 2), n(1, RETURN_TRUE, RETURN_FALSE), n(0, RETURN_TRUE, RETURN_TRUE)], 0, 0), false);
    let mut i = check();
    i.start(&h, Some(t), &[], None);
    i.run(&mut h, 1 << 20);
    assert!(i.done() && i.error() == Error::None && !i.result());
    assert_eq!(i.iterations(), 2);
}

#[test]
fn a_main_trees_bottom_frame_may_not_return() {
    let mut h = TestHost { answers: vec![PrimResult::True], ..Default::default() };
    let t = h.add(tree(0x1000, vec![n(1, RETURN_TRUE, RETURN_TRUE)], 0, 0), false);
    let mut i = Interpreter::main();
    i.start(&h, Some(t), &[], None);
    i.run(&mut h, 1 << 20);
    assert_eq!(i.error(), Error::ReturnFromBottom);
}

#[test]
fn a_call_its_parameters_and_its_return() {
    // Node 1 is primitive 0, which stops; the call must end on its false target, node 1.
    let mut h = TestHost::default();
    h.add(tree(0x1001, vec![n(3, RETURN_FALSE, RETURN_FALSE)], 2, 1), true);
    let t = h.add(tree(0x1000, vec![node(0x1001, RETURN_TRUE, 1, [7, 9, 0, 0]), n(0, RETURN_FALSE, RETURN_FALSE)], 0, 0), false);
    let mut i = check();
    i.start(&h, Some(t), &[], None);
    assert_eq!(i.step(&mut h), Step::Go); // the call pushes
    assert!(i.stack().len() == 2 && i.stack()[1].params[..2] == [7, 9]);
    assert_eq!(i.step(&mut h), Step::Go); // primitive 3: local 0 = param 0
    assert_eq!(i.stack()[1].locals[0], 7);
    assert_eq!(i.step(&mut h), Step::Go); // the return (false)
    assert!(i.stack().len() == 1 && i.stack()[0].node == 1);
}

#[test]
fn an_operand_of_all_ones_passes_the_interpreters_own_parameters() {
    let mut h = TestHost { defaults: vec![17, 2, 3], ..Default::default() };
    h.add(tree(0x1001, vec![n(0, RETURN_TRUE, RETURN_TRUE)], 2, 0), true);
    let t = h.add(tree(0x1000, vec![node(0x1001, RETURN_TRUE, RETURN_TRUE, [-1; 4])], 3, 0), false);
    let mut i = check();
    i.start(&h, Some(t), &[65, 7, 9], None);
    i.step(&mut h);
    assert!(i.stack().len() == 2 && i.stack()[1].params == [17, 2]);
}

#[test]
fn a_type_4_tree_starts_at_its_first_parameter() {
    let mut h = TestHost::default();
    let mut c = tree(0x1001, vec![n(0, RETURN_TRUE, RETURN_TRUE); 3], 1, 0);
    c.kind = 4;
    h.add(c, true);
    let t = h.add(
        tree(
            0x1000,
            vec![
                node(0x1001, RETURN_TRUE, RETURN_TRUE, [1, 0, 0, 0]),
                node(0x1001, RETURN_TRUE, RETURN_TRUE, [9, 0, 0, 0]),
                node(0x1001, RETURN_TRUE, RETURN_TRUE, [-2, 0, 0, 0]),
            ],
            0,
            0,
        ),
        false,
    );
    let mut i = check();
    i.start(&h, Some(t), &[], None);
    i.step(&mut h);
    assert!(i.stack().len() == 2 && i.stack()[1].node == 1);
    i.restore(vec![i.stack()[0].clone()]);
    i.top().node = 1;
    i.step(&mut h);
    assert!(i.stack().len() == 2 && i.stack()[1].node == 2);
    i.restore(vec![i.stack()[0].clone()]);
    i.top().node = 2;
    assert!(i.step(&mut h) == Step::Stop && i.error() == Error::CallFailed);
}

#[test]
fn an_unset_locals_read_is_reported() {
    // Locals start at zero; reading one before it is written is reported, after it is not.
    let mut h = TestHost::default();
    let t = h.add(tree(0x1000, vec![n(4, 1, RETURN_FALSE), n(3, 2, RETURN_FALSE), n(5, RETURN_FALSE, RETURN_TRUE)], 1, 2), false);
    let mut i = check();
    i.start(&h, Some(t), &[5], None);
    i.run(&mut h, 1 << 20);
    assert!(i.done() && i.result()); // local 1 read as zero; local 0 then 5
    assert_eq!(h.unset_reads, [1]);
}

#[test]
fn a_tree_with_no_nodes_cannot_start() {
    let mut h = TestHost::default();
    let t = h.add(tree(0x1000, vec![], 0, 0), false);
    let mut i = check();
    i.start(&h, Some(t), &[], None);
    assert!(i.done() && i.error() == Error::CallFailed && i.step(&mut h) == Step::Stop);
}

#[test]
fn errors() {
    // Target 253, a missing node, a missing tree, the iteration limit.
    let mut h = TestHost { answers: vec![PrimResult::True], ..Default::default() };
    let a = h.add(tree(0x1000, vec![n(1, TARGET_ERROR, TARGET_ERROR)], 0, 0), false);
    let mut i = check();
    i.start(&h, Some(a), &[], None);
    i.run(&mut h, 1 << 20);
    assert_eq!(i.error(), Error::BadTransition);

    let b = h.add(tree(0x1000, vec![n(1, 5, 5)], 0, 0), false);
    h.answers = vec![PrimResult::True];
    h.next = 0;
    i.start(&h, Some(b), &[], None);
    i.run(&mut h, 1 << 20);
    assert_eq!(i.error(), Error::Missing);

    let c = h.add(tree(0x1000, vec![n(0x1234, RETURN_TRUE, RETURN_TRUE)], 0, 0), false);
    i.start(&h, Some(c), &[], None);
    i.run(&mut h, 1 << 20);
    assert_eq!(i.error(), Error::Missing);

    let d = h.add(tree(0x1000, vec![n(1, 0, 0)], 0, 0), false); // loops on itself
    h.answers = vec![PrimResult::True; 20000];
    h.next = 0;
    let mut lim = Interpreter::new(Mode::Check, 100);
    lim.start(&h, Some(d), &[], None);
    lim.run(&mut h, 1 << 20);
    assert!(lim.error() == Error::TooManyIterations && lim.iterations() == 101);
}

#[test]
fn a_yield_keeps_the_node() {
    // A yield keeps the node, and the next step runs it again.
    let mut h = TestHost::default();
    let t = h.add(tree(0x1000, vec![n(2, RETURN_TRUE, RETURN_FALSE)], 0, 0), false);
    let mut i = check();
    i.start(&h, Some(t), &[], None);
    assert!(i.step(&mut h) == Step::Yield && i.stack()[0].node == 0);
    assert_eq!(i.step(&mut h), Step::Go);
    i.run(&mut h, 1 << 20);
    assert!(i.done() && i.result());
}

#[test]
fn tree_kinds_by_id() {
    assert!(tree_kind(255) == TreeKind::Primitive && tree_kind(256) == TreeKind::Global);
    assert!(tree_kind(4095) == TreeKind::Global && tree_kind(4096) == TreeKind::Own);
    assert!(tree_kind(8191) == TreeKind::Own && tree_kind(8192) == TreeKind::SemiGlobal);
    assert!(tree_kind(9000) == TreeKind::SemiGlobal && tree_kind(9001) == TreeKind::None);
}
