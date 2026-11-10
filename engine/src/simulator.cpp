#include "housefire/simulator.h"

#include <algorithm>

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

void Simulator::refresh() {
    if (seen_version_ == world_.version()) return;
    seen_version_ = world_.version();
    const auto& by_id = world_.by_id();
    top_ = by_id.empty() ? 0 : by_id.rbegin()->first;
    // The runs of objects that are gone go with them.
    std::unordered_map<const Object*, bool> present;
    for (const auto& [id, o] : by_id) present[o] = true;
    std::erase_if(runs_, [&](const auto& r) { return !present.count(r.first); });
    order_.clear();
    for (const auto& [id, o] : by_id) {
        if (id <= 0 || (o->stack.empty() && !runs_.count(o))) continue;
        order_.push_back({id, o, &stack_of(*o)});
    }
}

size_t Simulator::first_from(int16_t id) const {
    return (size_t)(std::lower_bound(order_.begin(), order_.end(), id, [](const Entry& e, int16_t v) { return e.id < v; }) - order_.begin());
}

std::vector<std::pair<Object*, bool>> Simulator::pass_order() {
    refresh();
    std::vector<std::pair<Object*, bool>> out;
    for (size_t i = first_from(cursor_ > 0 ? cursor_ : 1); i < order_.size(); i++)
        if (!order_[i].in->done()) out.push_back({order_[i].object, order_[i].in->asleep()});
    return out;
}

void Simulator::tick() {
    // As the original's cursor (its +1424) does, each object is looked at when its id comes
    // up, so a turn that wakes a later object gives it its turn in this pass. The highest id
    // is taken as the pass starts: an object made during it waits for the next.
    refresh();
    const int16_t top = top_;
    int16_t stopped_at = 0;
    for (size_t i = first_from(cursor_ > 0 ? cursor_ : 1); i < order_.size() && order_[i].id <= top;) {
        const Entry e = order_[i];
        Interpreter& in = *e.in;
        if (in.done()) { i++; continue; }
        if (in.asleep()) { in.pass_over(); i++; continue; }
        stop_ = false;
        in.reset_iterations();
        const int steps = in.run();
        if (on_turn) on_turn(*e.object, steps);
        if (stop_) { stopped_at = e.id; break; }
        // A turn that made, removed or renumbered objects: on from the next id.
        if (seen_version_ != world_.version()) {
            refresh();
            i = first_from((int16_t)(e.id + 1));
        } else {
            i++;
        }
    }
    stop_ = false;
    end_pass(stopped_at);
}

}  // namespace housefire
