// The node step's rules (docs/interpreter.md) on trees written here, not taken from a game.
#include <cstdio>
#include <map>

#include "housefire/interp.h"

using namespace housefire;

static int failures = 0;
#define CHECK(c)                                                              \
    do {                                                                      \
        if (!(c)) { std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); failures++; } \
    } while (0)

// A node: opcode, targets, operand as four 16-bit fields.
static Node node(uint16_t op, uint8_t t, uint8_t f, int16_t a = 0, int16_t b = 0, int16_t c = 0, int16_t d = 0) {
    Node n{op, t, f, {}, Order::Big};
    const int16_t v[4] = {a, b, c, d};
    for (int i = 0; i < 4; i++) {
        n.operand[2 * i] = (uint8_t)((uint16_t)v[i] >> 8);
        n.operand[2 * i + 1] = (uint8_t)v[i];
    }
    return n;
}

static Tree tree(int16_t id, std::vector<Node> nodes, uint8_t args = 0, uint16_t locals = 0) {
    Tree t;
    t.id = id;
    t.signature = 0x8002;
    t.args = args;
    t.locals = locals;
    t.nodes = std::move(nodes);
    return t;
}

// Primitive 1 answers what `answers` says next; primitive 2 yields once, then answers true;
// primitive 3 sets local 0 to parameter 0 and answers true.
struct TestHost : Host {
    std::map<uint16_t, Tree> trees;
    std::vector<PrimResult> answers;
    size_t next = 0;
    int yields = 0;
    PrimResult primitive(Interpreter&, Frame& f, const Node& n) override {
        switch (n.opcode) {
            case 1: return next < answers.size() ? answers[next++] : PrimResult::Stop;
            case 2: return yields++ == 0 ? PrimResult::Yield : PrimResult::True;
            case 3: if (!f.locals.empty() && !f.params.empty()) f.locals[0] = f.params[0]; return PrimResult::True;
            default: return PrimResult::Stop;
        }
    }
    const Tree* tree(uint16_t id) override {
        auto it = trees.find(id);
        return it == trees.end() ? nullptr : &it->second;
    }
};

int main() {
    {   // true and false targets, then a return, in a check tree
        TestHost h;
        h.answers = {PrimResult::True, PrimResult::False};
        Tree t = tree(0x1000, {node(1, 1, 2), node(1, kReturnTrue, kReturnFalse), node(0, kReturnTrue, kReturnTrue)});
        Interpreter in(h, Interpreter::Mode::Check);
        in.start(&t);
        in.run();
        CHECK(in.done() && in.error() == Error::None && in.result() == false);
        CHECK(in.iterations() == 2);
    }
    {   // a main tree's bottom frame may not return
        TestHost h;
        h.answers = {PrimResult::True};
        Tree t = tree(0x1000, {node(1, kReturnTrue, kReturnTrue)});
        Interpreter in(h);
        in.start(&t);
        in.run();
        CHECK(in.error() == Error::ReturnFromBottom);
    }
    {   // a call: parameters from the operand, the callee's return takes the caller's target
        TestHost h;
        h.trees[0x1001] = tree(0x1001, {node(3, kReturnFalse, kReturnFalse)}, 2, 1);
        Tree t = tree(0x1000, {node(0x1001, kReturnTrue, 1, 7, 9), node(0, kReturnFalse, kReturnFalse)});
        // node 1 is primitive 0, which stops; the call must end on its false target, node 1
        Interpreter in(h, Interpreter::Mode::Check);
        in.start(&t);
        CHECK(in.step() == Step::Go);                      // the call pushes
        CHECK(in.stack().size() == 2 && in.stack()[1].params[0] == 7 && in.stack()[1].params[1] == 9);
        CHECK(in.step() == Step::Go);                      // primitive 3: local 0 = param 0
        CHECK(in.stack()[1].locals[0] == 7);
        CHECK(in.step() == Step::Go);                      // the return (false)
        CHECK(in.stack().size() == 1 && in.stack()[0].node == 1);
    }
    {   // a call whose operand is all -1 passes no parameters
        TestHost h;
        h.trees[0x1001] = tree(0x1001, {node(0, kReturnTrue, kReturnTrue)}, 2);
        Tree t = tree(0x1000, {node(0x1001, kReturnTrue, kReturnTrue, -1, -1, -1, -1)});
        Interpreter in(h, Interpreter::Mode::Check);
        in.start(&t);
        in.step();
        CHECK(in.stack().size() == 2 && in.stack()[1].params[0] == 0 && in.stack()[1].params[1] == 0);
    }
    {   // errors: target 253, a missing node, a missing tree, the iteration limit
        TestHost h;
        h.answers = {PrimResult::True};
        Tree a = tree(0x1000, {node(1, kTargetError, kTargetError)});
        Interpreter in(h, Interpreter::Mode::Check);
        in.start(&a);
        in.run();
        CHECK(in.error() == Error::BadTransition);

        Tree b = tree(0x1000, {node(1, 5, 5)});
        h.answers = {PrimResult::True}; h.next = 0;
        in.start(&b);
        in.run();
        CHECK(in.error() == Error::Missing);

        Tree c = tree(0x1000, {node(0x1234, kReturnTrue, kReturnTrue)});
        in.start(&c);
        in.run();
        CHECK(in.error() == Error::Missing);

        Tree d = tree(0x1000, {node(1, 0, 0)});            // loops on itself
        h.answers.assign(20000, PrimResult::True); h.next = 0;
        Interpreter lim(h, Interpreter::Mode::Check, 100);
        lim.start(&d);
        lim.run();
        CHECK(lim.error() == Error::TooManyIterations && lim.iterations() == 101);
    }
    {   // a yield keeps the node, and the next step runs it again
        TestHost h;
        Tree t = tree(0x1000, {node(2, kReturnTrue, kReturnFalse)});
        Interpreter in(h, Interpreter::Mode::Check);
        in.start(&t);
        CHECK(in.step() == Step::Yield && in.stack()[0].node == 0);
        CHECK(in.step() == Step::Go);
        in.run();
        CHECK(in.done() && in.result() == true);
    }
    {   // tree kinds by id
        CHECK(tree_kind(255) == TreeKind::Primitive && tree_kind(256) == TreeKind::Global);
        CHECK(tree_kind(4095) == TreeKind::Global && tree_kind(4096) == TreeKind::Own);
        CHECK(tree_kind(8191) == TreeKind::Own && tree_kind(8192) == TreeKind::SemiGlobal);
        CHECK(tree_kind(9000) == TreeKind::SemiGlobal && tree_kind(9001) == TreeKind::None);
    }
    std::printf("test_interp: %s\n", failures ? "FAILED" : "ok");
    return failures ? 1 : 0;
}
