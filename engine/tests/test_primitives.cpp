// Primitives other than the expression, on worlds made here (SetToNext's and Idle's rules,
// docs/objects.md and docs/primitives.md; checked against the recompiled game by lockstep).
#include <cstdio>

#include "housefire/primitives.h"

using namespace housefire;

static int failures = 0;
#define CHECK(c)                                                              \
    do {                                                                      \
        if (!(c)) { std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); failures++; } \
    } while (0)

struct NoHost : Host {
    PrimResult primitive(Interpreter&, Frame&, const Node&) override { return PrimResult::Stop; }
    const Tree* tree(uint16_t) override { return nullptr; }
};

static Node node(uint16_t op, std::array<uint8_t, 8> operand) {
    Node n{op, 1, 2, operand, Order::Little};
    return n;
}

int main() {
    Library lib;
    World w(lib);
    auto make = [&](int16_t id, bool person, uint32_t guid) {
        auto o = std::make_unique<Object>();
        o->data.resize(72);
        o->data[kDataObjectId] = id;
        o->person = person;
        o->guid = guid;
        w.add(std::move(o));
    };
    make(5, false, 0x111);
    make(9, true, 0x222);
    make(3, false, 0x111);
    Object me;
    Frame f;
    ScriptVariables vars(w, me, f);

    // SetToNext over the list's order (not by id), from 0: any object, then the Sims, then a GUID
    const Node any = node(31, {0, 0, 0, 0, 0, 0, 0, 0});
    f.stack_object = 0;
    CHECK(set_to_next(any, w, vars) == PrimResult::True && f.stack_object == 5);
    CHECK(set_to_next(any, w, vars) == PrimResult::True && f.stack_object == 9);
    CHECK(set_to_next(any, w, vars) == PrimResult::True && f.stack_object == 3);
    CHECK(set_to_next(any, w, vars) == PrimResult::False && f.stack_object == 3);
    f.stack_object = 0;
    CHECK(set_to_next(node(31, {0, 0, 0, 0, 1, 0, 0, 0}), w, vars) == PrimResult::True && f.stack_object == 9);
    f.stack_object = 5;
    CHECK(set_to_next(node(31, {0x11, 0x01, 0, 0, 4, 0, 0, 0}), w, vars) == PrimResult::True && f.stack_object == 3);
    // kind 3: the next part of the target's multi-tile object in list order, round from the last
    // (the list is 5, 9, 3: 5 and 3 parts of one led by 3)
    for (const auto& x : w.objects()) if (x->id() == 5 || x->id() == 3) x->leader = 3;
    f.stack_object = 5;
    CHECK(set_to_next(node(31, {0, 0, 0, 0, 3, 0, 0, 0}), w, vars) == PrimResult::True && f.stack_object == 3);
    CHECK(set_to_next(node(31, {0, 0, 0, 0, 3, 0, 0, 0}), w, vars) == PrimResult::True && f.stack_object == 5);
    f.stack_object = 9;  // not a part
    CHECK(set_to_next(node(31, {0, 0, 0, 0, 3, 0, 0, 0}), w, vars) == PrimResult::False);
    for (const auto& x : w.objects()) x->leader = 0;
    // into a local (flag 0x80, scope 25, datum 0)
    f.locals = {0};
    CHECK(set_to_next(node(31, {0, 0, 0, 0, 0x82, 25, 0, 0}), w, vars) == PrimResult::True && f.locals[0] == 5);

    // TestObjectType: the stack object (scope 10, datum 0) and a local (scope 25) against a GUID
    f.stack_object = 3;
    CHECK(test_object_type(node(32, {0x11, 0x01, 0, 0, 0, 0, 10, 0}), w, vars) == PrimResult::True);
    CHECK(test_object_type(node(32, {0x22, 0x02, 0, 0, 0, 0, 10, 0}), w, vars) == PrimResult::False);
    f.locals = {9};
    CHECK(test_object_type(node(32, {0x22, 0x02, 0, 0, 0, 0, 25, 0}), w, vars) == PrimResult::True);
    f.locals = {77};  // no such object
    CHECK(test_object_type(node(32, {0x22, 0x02, 0, 0, 0, 0, 25, 0}), w, vars) == PrimResult::False);

    // Random: into a local, a literal range; always below it, and spread over it
    f.locals = {0};
    bool seen[7] = {};
    for (int i = 0; i < 700; i++) {
        CHECK(random(node(8, {0, 0, 25, 0, 7, 0, 7, 0}), w, vars) == PrimResult::True);
        CHECK(f.locals[0] >= 0 && f.locals[0] < 7);
        if (f.locals[0] >= 0 && f.locals[0] < 7) seen[f.locals[0]] = true;
    }
    for (bool b : seen) CHECK(b);

    // a made object takes the next id and goes in front
    auto o = std::make_unique<Object>();
    Object& made = w.make_object(std::move(o));
    CHECK(made.id() == 10 && w.objects().front().get() == &made);

    // Idle: passed over by the next 29 passes that reach it, stepped on the 30th; then the
    // parameter is 0 and the result true
    NoHost h;
    Interpreter in(h);
    Frame g;
    g.params = {30};
    const Node sleep = node(0, {0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF});
    CHECK(idle(in, g, sleep) == PrimResult::Yield);
    int passed = 0;
    while (in.asleep()) { in.pass_over(); passed++; }
    CHECK(passed == 29);
    CHECK(idle(in, g, sleep) == PrimResult::True && g.params[0] == 0);
    g.params = {0};
    CHECK(idle(in, g, sleep) == PrimResult::True);

    std::printf("test_primitives: %s\n", failures ? "FAILED" : "ok");
    return failures ? 1 : 0;
}
