#include "housefire/simulator.h"

namespace housefire {

Interpreter& Simulator::stack_of(Object& o) {
    Run& r = runs_[&o];
    if (!r.in) {
        r.host = make_host_(o);
        r.in = std::make_unique<Interpreter>(*r.host);
        r.in->restore(o.stack);
    }
    return *r.in;
}

void Simulator::tick() {
    // The original's cursor walks ids from 1 to the highest, taken once at the start; it can
    // stop partway and go on from there on the next call, which no logged tick has done.
    int16_t top = 0;
    for (const auto& o : world_.objects()) top = std::max(top, o->id());
    for (int16_t id = 1; id > 0 && id <= top; id++) {
        Object* o = world_.find(id);
        if (!o || (o->stack.empty() && !runs_.count(o))) continue;
        Interpreter& in = stack_of(*o);
        if (in.done() || in.asleep(now_)) continue;
        in.reset_iterations();
        const int steps = in.run();
        if (on_turn) on_turn(*o, steps);
    }
    now_++;
}

}  // namespace housefire
