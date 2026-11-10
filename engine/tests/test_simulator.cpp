// The simulation tick's order (docs/simulation.md) on objects and trees written here.
#include <cstdio>
#include <map>
#include <vector>

#include "housefire/simulator.h"

using namespace housefire;

static int failures = 0;
#define CHECK(c)                                                              \
    do {                                                                      \
        if (!(c)) { std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); failures++; } \
    } while (0)

// Primitive 1: note the object's id and yield. Primitive 2: sleep two ticks, then go on to
// node 0 (Idle's way). Primitive 3: make a new object running the same tree, then go on.
// Primitive 4: note the id and go on.
static Tree tree() {
    Tree t;
    t.id = 4096;
    t.signature = 0x8002;
    t.nodes = {Node{3, 1, 1, {}, Order::Big}, Node{1, 1, 1, {}, Order::Big}};
    return t;
}
static Tree sleeper() {
    Tree t;
    t.id = 4097;
    t.signature = 0x8002;
    t.nodes = {Node{4, 1, 1, {}, Order::Big}, Node{2, 0, 0, {}, Order::Big}};
    return t;
}

struct Fixture;
struct TestHost : Host {
    Fixture& fx;
    Object& me;
    TestHost(Fixture& f, Object& o) : fx(f), me(o) {}
    PrimResult primitive(Interpreter& in, Frame& f, const Node& n) override;
    const Tree* tree(uint16_t) override { return nullptr; }
};

struct Fixture {
    Library lib;
    World world{lib};
    Tree main = tree(), slow = sleeper();
    std::vector<int16_t> turns;  // ids, in the order primitive 1 saw them
    Simulator sim{world, [this](Object& o) { return std::make_unique<TestHost>(*this, o); }};
    bool made = false;
    Object& add(int16_t id, const Tree* t) {
        auto o = std::make_unique<Object>();
        o->data.resize(72);
        o->data[kDataObjectId] = id;
        Frame f;
        f.tree = t;
        o->stack.push_back(f);
        return world.add(std::move(o));
    }
};

PrimResult TestHost::primitive(Interpreter& in, Frame& f, const Node& n) {
    switch (n.opcode) {
        case 1: fx.turns.push_back(me.id()); return PrimResult::Yield;
        case 4: fx.turns.push_back(me.id()); return PrimResult::True;
        case 2:
            if (in.woke()) { in.wake(); return PrimResult::True; }
            in.sleep_until(fx.sim.now() + 2);
            return PrimResult::Yield;
        case 3:
            if (me.id() == 2 && !fx.made) {
                fx.made = true;
                auto o = std::make_unique<Object>();
                o->data.resize(72);
                Frame g;
                g.tree = &fx.main;
                o->stack.push_back(g);
                fx.world.make_object(std::move(o));
            }
            (void)f;
            return PrimResult::True;
        default: return PrimResult::Stop;
    }
}

int main() {
    Fixture fx;
    // The world's list order is not the tick's: 3, 1, 2 in the list, 5 asleep after its
    // first turn.
    fx.add(3, &fx.main);
    fx.add(1, &fx.main);
    fx.add(2, &fx.main);
    fx.add(5, &fx.slow);
    fx.sim.tick();
    // By id; object 2 makes object 6 (one more than the highest), which waits for the next
    // tick. Object 5 sleeps two ticks from tick 0.
    CHECK((fx.turns == std::vector<int16_t>{1, 2, 3, 5}));
    CHECK(fx.world.find(6) != nullptr);
    fx.turns.clear();
    fx.sim.tick();  // tick 1: 5 asleep
    CHECK((fx.turns == std::vector<int16_t>{1, 2, 3, 6}));
    fx.turns.clear();
    fx.sim.tick();  // tick 2: 5 wakes, goes round to node 0 and sleeps again
    CHECK((fx.turns == std::vector<int16_t>{1, 2, 3, 5, 6}));
    fx.turns.clear();
    fx.sim.tick();  // tick 3: asleep
    CHECK((fx.turns == std::vector<int16_t>{1, 2, 3, 6}));
    CHECK(fx.sim.now() == 4);
    std::printf("test_simulator: %s\n", failures ? "FAILED" : "ok");
    return failures ? 1 : 0;
}
