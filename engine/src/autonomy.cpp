#include "housefire/autonomy.h"

#include <algorithm>

namespace housefire {

std::vector<Candidate> autonomy_candidates(const World& world, const Object& me) {
    std::vector<Candidate> out;
    for (const auto& [id, o] : world.by_id()) {
        if (o == &me || !o->file) continue;
        const std::vector<Interaction>* table = nullptr;
        if (o->person) {
            // A person's interactions are its semi-global file's one table: person_sg's for a
            // Sim (whose file is template_person), NPC_SG's for an NPC (the paper girl).
            const ObjectFile* semi = o->file->semi;
            if (semi && semi->tables.size() == 1) table = &semi->tables.begin()->second;
        } else {
            if (!o->definition) continue;
            // A definition of type 7 (field 7: the phone's and the stereo's controllers, kept
            // off the lot) is not considered; an off-lot one of type 4 is (Mom's house's
            // scenario).
            if (o->definition->field(7) == 7) continue;
            // A part whose definition's field 6 is 1 to 0xFFFD (a bed's, a mailbox's lesser
            // parts) is not considered; 0, 0xFFFE and 0xFFFF are.
            const uint16_t f6 = o->definition->field(6);
            if (f6 >= 1 && f6 <= 0xFFFD) continue;
            table = world.library().interactions_of(*o->definition, *o->file);
        }
        if (!table) continue;
        std::vector<uint16_t> guards;
        for (auto it = table->rbegin(); it != table->rend(); ++it) {
            if (it->autonomy_threshold >= 100 || (it->flags & 0xC0) || !it->guard) continue;
            if (std::find(guards.begin(), guards.end(), it->guard) != guards.end()) continue;
            guards.push_back(it->guard);
            out.push_back({o, &*it});
        }
    }
    return out;
}

std::vector<EvaluationCheck> run_evaluation(const World& world, const Object& me, Host& host,
                                            const std::vector<Candidate>& candidates) {
    (void)me;
    std::vector<EvaluationCheck> out;
    const Object* last = nullptr;
    for (const Candidate& c : candidates) {
        const auto tab = world.library().table_of(*c.object);
        if (c.object != last) {  // a new object: its table's first tree, once
            last = c.object;
            if (tab.first) {
                const Tree* t = world.library().tree(tab.file, tab.first);
                out.push_back({c.object, tab.first, nullptr, t ? host.run_check(t, tab.file, c.object->id()) : std::nullopt});
            }
        }
        const Tree* g = world.library().tree(tab.file, c.interaction->guard);
        out.push_back({c.object, c.interaction->guard, &c, g ? host.run_check(g, tab.file, c.object->id()) : std::nullopt});
    }
    return out;
}

}  // namespace housefire
