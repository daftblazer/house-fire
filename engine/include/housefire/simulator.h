// The simulation tick: which objects' tree stacks run, and in what order (docs/simulation.md).
// Measured on the recompiled Bustin' Out (BUSTIN_TREE_LOG) and read from its tick,
// fn_80238150.
#pragma once
#include <cstdint>
#include <functional>
#include <memory>
#include <unordered_map>

#include "housefire/interp.h"
#include "housefire/world.h"

namespace housefire {

class Simulator {
public:
    // The host an object's scripts run with (their primitives, and the trees they name).
    using MakeHost = std::function<std::unique_ptr<Host>(Object&)>;

    Simulator(World& world, MakeHost make_host) : world_(world), make_host_(std::move(make_host)) {}

    // One tick: every object from id 1 up to the highest id in use when the tick starts, in
    // id order (an object made during the tick waits for the next one), each stack that is
    // not asleep stepped until it yields or stops. The clock (now) then moves on; it does
    // not while the game is paused, as tick is not called then.
    void tick();
    uint32_t now() const { return now_; }

    // An object's running stack, made on first need from the frames the object has (a lot's
    // object: as the lot saved them). Its id is the object's, its lifetime the object's in
    // the simulator.
    Interpreter& stack_of(Object& o);
    void forget(const Object& o) { runs_.erase(&o); }  // the object is gone

    // Called after each object's turn (for checks): the object, the steps it ran.
    std::function<void(const Object&, int steps)> on_turn;

private:
    struct Run {
        std::unique_ptr<Host> host;
        std::unique_ptr<Interpreter> in;
    };
    World& world_;
    MakeHost make_host_;
    std::unordered_map<const Object*, Run> runs_;
    uint32_t now_ = 0;
};

}  // namespace housefire
