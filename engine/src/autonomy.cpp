#include "housefire/autonomy.h"

#include <algorithm>

namespace housefire {

std::vector<Candidate> autonomy_candidates(const World& world, const Object& me) {
    std::vector<Candidate> out;
    for (const auto& [id, o] : world.by_id()) {
        if (o == &me || o->person || !o->definition || !o->file) continue;
        // A definition of type 7 (field 7: the phone's and the stereo's controllers, kept off
        // the lot) is not considered; an off-lot one of type 4 is (Mom's house's scenario).
        if (o->definition->field(7) == 7) continue;
        // A part whose definition's field 6 is 1 to 0xFFFD (a bed's, a mailbox's lesser parts)
        // is not considered; 0, 0xFFFE and 0xFFFF are.
        const uint16_t f6 = o->definition->field(6);
        if (f6 >= 1 && f6 <= 0xFFFD) continue;
        const std::vector<Interaction>* table = world.library().interactions_of(*o->definition, *o->file);
        if (!table) continue;
        std::vector<uint16_t> guards;
        for (auto it = table->rbegin(); it != table->rend(); ++it) {
            if (it->autonomy_threshold >= 100 || (it->flags & 0x80) || !it->guard) continue;
            if (std::find(guards.begin(), guards.end(), it->guard) != guards.end()) continue;
            guards.push_back(it->guard);
            out.push_back({o, &*it});
        }
    }
    return out;
}

}  // namespace housefire
