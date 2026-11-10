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

std::vector<std::pair<Object*, bool>> Simulator::pass_order() {
    int16_t top = 0;
    for (const auto& o : world_.objects()) top = std::max(top, o->id());
    std::vector<std::pair<Object*, bool>> out;
    for (int16_t id = cursor_ > 0 ? cursor_ : 1; id > 0 && id <= top; id++) {
        Object* o = world_.find(id);
        if (!o || (o->stack.empty() && !runs_.count(o))) continue;
        Interpreter& in = stack_of(*o);
        if (!in.done()) out.push_back({o, in.asleep()});
    }
    return out;
}

void Simulator::tick() {
    // As the original's cursor (its +1424) does, each object is looked at when its id comes
    // up, so a turn that wakes a later object gives it its turn in this pass.
    int16_t top = 0;
    for (const auto& o : world_.objects()) top = std::max(top, o->id());
    int16_t stopped_at = 0;
    for (int16_t id = cursor_ > 0 ? cursor_ : 1; id > 0 && id <= top; id++) {
        Object* o = world_.find(id);
        if (!o || (o->stack.empty() && !runs_.count(o))) continue;
        Interpreter& in = stack_of(*o);
        if (in.done()) continue;
        if (in.asleep()) { in.pass_over(); continue; }
        stop_ = false;
        in.reset_iterations();
        const int steps = in.run();
        if (on_turn) on_turn(*o, steps);
        if (stop_) { stopped_at = id; break; }
    }
    stop_ = false;
    end_pass(stopped_at);
}

}  // namespace housefire
