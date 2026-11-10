// The simulation tick: which objects' tree stacks run, and in what order (docs/simulation.md).
// Measured on the recompiled Bustin' Out (BUSTIN_TREE_LOG) and read from its tick,
// fn_80238150.
#pragma once
#include <cstdint>
#include <functional>
#include <memory>
#include <unordered_map>
#include <vector>

#include "housefire/interp.h"
#include "housefire/motives.h"
#include "housefire/world.h"

namespace housefire {

class Simulator {
public:
    // The host an object's scripts run with (their primitives, and the trees they name).
    using MakeHost = std::function<std::unique_ptr<Host>(Object&)>;

    Simulator(World& world, MakeHost make_host) : world_(world), make_host_(std::move(make_host)) {
        motive_rates_ok_ = motive_rates_.from(world.library());
    }

    // An object's turn (the pass has reached it, asleep or not): a person's turns are
    // counted, and every 60th ends with its motives' own update (Person_SimMotives,
    // 8022501C), after its stack has run: in the log the turn's scripts read the motives as
    // they were before it. For a Sim of the family's, not a person with a file of its own (an
    // NPC: the paper girl, whose gate is set). Measured: docs/simulation.md, "The motives".
    // A person's running motive changes (SetMotiveDelta's) apply at the end of each of its
    // turns too, before the update: the log's first change is in the turn that set it.
    // begin_turn counts the turn and says whether it ends with the update; end_turn applies
    // the changes, and the update if due.
    bool begin_turn(Object& o);
    void end_turn(Object& o, bool update_due);

    // One tick: the pass over the objects, by id, from where the last one stopped (or id 1)
    // up to the highest id in use when the pass starts (an object made during it waits for
    // the next). A sleeping stack is passed over, which counts its sleep down; any other is
    // stepped until it yields or stops. A turn that asks for it (stop_pass: a dialog waiting
    // for the player) ends the pass there, and the next tick's goes on from that object.
    void tick();
    void stop_pass() { stop_ = true; }  // from a primitive, during a turn
    bool stop_asked() const { return stop_ || world_.stop_pass; }  // in a pass done by hand: since the last end_pass
    int16_t cursor() const { return cursor_; }  // where the next pass goes on from (0: id 1)
    uint32_t now() const { return now_; }  // ticks run
    // For checks that step the stacks themselves: the objects the pass would reach, in order
    // (to the highest id; a stop is not foreseen), with whether each is asleep; and the pass
    // done by hand: an object passed over, a pass's end (at an id, or 0 for through).
    std::vector<std::pair<Object*, bool>> pass_order();
    void pass_over(Object& o) { stack_of(o).pass_over(); }
    void end_pass(int16_t stopped_at) { cursor_ = stopped_at; stop_ = false; world_.stop_pass = false; now_++; }

    // An object's running stack, made on first need from the frames the object has (a lot's
    // object: as the lot saved them). Its id is the object's, its lifetime the object's in
    // the simulator.
    Interpreter& stack_of(Object& o);
    void forget(const Object& o) { runs_.erase(&o); seen_version_ = ~uint64_t(0); }  // the object is gone

    // Called after each object's turn (for checks): the object, the steps it ran.
    std::function<void(const Object&, int steps)> on_turn;

private:
    struct Run {
        std::unique_ptr<Host> host;
        std::unique_ptr<Interpreter> in;
    };
    // The objects that have a stack, by id, each with its run: rebuilt only when the world's
    // objects change (World::version), so that a pass is a walk down a vector.
    struct Entry {
        int16_t id;
        Object* object;
        Interpreter* in;
    };
    void refresh();
    size_t first_from(int16_t id) const;  // the first entry with an id at least this

    World& world_;
    MakeHost make_host_;
    std::unordered_map<const Object*, Run> runs_;
    std::vector<Entry> order_;
    uint64_t seen_version_ = ~uint64_t(0);
    int16_t top_ = 0;  // the highest id in use
    uint32_t now_ = 0;
    int16_t cursor_ = 0;
    bool stop_ = false;
    MotiveRates motive_rates_;
    bool motive_rates_ok_ = false;
};

}  // namespace housefire
