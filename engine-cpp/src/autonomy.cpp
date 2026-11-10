#include "housefire/autonomy.h"

#include <algorithm>
#include <iterator>
#include <cstdio>
#include <cmath>

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
                const Tree* t = world.library().tree(c.object->file, tab.first);
                out.push_back({c.object, tab.first, nullptr, t ? host.run_check(t, c.object->file, c.object->id()) : std::nullopt});
            }
        }
        // The trees as the object's own file names them (a person's table is its semi-global
        // file's, whose ids the object's file resolves).
        const Tree* g = world.library().tree(c.object->file, c.interaction->guard);
        out.push_back({c.object, c.interaction->guard, &c, g ? host.run_check(g, c.object->file, c.object->id()) : std::nullopt});
    }
    return out;
}

float ScoreCurve::at(float v) const {
    // The segment from the end: the last point at or below v.
    int i = (int)points.size() - 1;
    while (i >= 0 && v - points[(size_t)i].first < 0.0f) i--;
    if (i < 0) return points.front().second;
    if (i == (int)points.size() - 1) return points.back().second;
    const auto& [x, y] = points[(size_t)i];
    const float y1 = points[(size_t)i + 1].second;
    return std::fmaf((v - x) * slopes[(size_t)i], y1 - y, y);
}

std::vector<ScoreCurve> score_curves(const Library& lib) {
    static const int kMotives[] = {5, 6, 7, 8, 9, 3, 13, 14, 15};
    std::vector<ScoreCurve> out;
    const ObjectFile* g = lib.global();
    if (!g) return out;
    auto t = g->strings.find(501);
    if (t == g->strings.end()) return out;
    for (size_t k = 0; k < std::size(kMotives) && k < t->second.size(); k++) {
        ScoreCurve c;
        c.motive = kMotives[k];
        const std::string& s = t->second[k];
        for (size_t p = s.find('('); p != std::string::npos; p = s.find('(', p + 1)) {
            float x = 0, y = 0;
            if (std::sscanf(s.c_str() + p, "(%f;%f)", &x, &y) == 2) c.points.push_back({x, y});
        }
        std::sort(c.points.begin(), c.points.end());
        for (size_t i = 0; i + 1 < c.points.size(); i++) c.slopes.push_back(1.0f / (c.points[i + 1].first - c.points[i].first));
        if (!c.points.empty()) out.push_back(std::move(c));
    }
    return out;
}

float advert_amount(const Interaction::Advert& a, const Object& person) {
    // The personality table (0x802D17F8, 12 bytes an entry): entry p's person data field is
    // p / 2 + 1, the odd entries counting 1000 less it (1: field 2; 2: field 2, inverted; ...).
    if (a.personality == 0) return (float)(a.min + a.delta);
    const int field = (int)(a.personality + 1) / 2 + 1;
    const bool inverted = a.personality % 2 == 0;
    int v = (size_t)field < person.person_data.size() ? person.person_data[(size_t)field] : 0;
    if (inverted) v = 1000 - v;
    return (float)a.min + (float)(a.delta * v) / 1000.0f;
}

float interaction_score(const std::vector<ScoreCurve>& curves, const std::vector<Interaction::Advert>* adverts,
                        const Object& person, const std::vector<float>& motives) {
    if (curves.empty()) return 0.0f;
    float total = 0.0f;
    for (const ScoreCurve& c : curves) {
        float ad = 0.0f;
        if (adverts && (size_t)c.motive < adverts->size()) ad = advert_amount((*adverts)[(size_t)c.motive], person);
        const float m = (size_t)c.motive < motives.size() ? motives[(size_t)c.motive] : 0.0f;
        total += c.at(std::fmaf(ad, 0.001f, m));
    }
    return total / (float)curves.size();
}

// The game's square root (fn_801F1BB8): a first guess of 1 + s / 20, then two of Heron's
// steps; up to 4% over near s = 25, which the choices' values show (docs/simulation.md).
float game_sqrt(float s) {
    if (s <= 0.0f) return 0.0f;
    float x = std::fmaf(s, 0.05f, 1.0f);
    x = (x + s / x) * 0.5f;
    x = (x + s / x) * 0.5f;
    return x;
}

float person_distance(const Object& a, const Object& b) {
    const int32_t dx = a.x16 - b.x16, dy = a.y16 - b.y16;
    const float flat = (float)(dx * dx + dy * dy) * 0.00390625f;  // 1/256: sixteenths to tiles, squared
    const float up = (float)((a.level - b.level) * 20);
    return game_sqrt(std::fmaf(up, up, flat));
}

float attenuation(const Library& lib, const Interaction& e, bool visitor) {
    switch (e.attenuation_code) {
        case 0: return e.attenuation_value;
        case 2: return lib.tuning(visitor ? "visitor low attenuation" : "low attenuation").value_or(0.0f);
        case 3: return lib.tuning(visitor ? "visitor moderate attenuation" : "moderate attenuation").value_or(0.0f);
        case 4: return lib.tuning(visitor ? "visitor high attenuation" : "high attenuation").value_or(0.0f);
        default: return 0.0f;
    }
}

float candidate_value(const Library& lib, const std::vector<ScoreCurve>& curves, const Object& person,
                      const Candidate& c, float base) {
    const float score = interaction_score(curves, &c.interaction->adverts, person, person.motives);
    const float factor = 1.0f / std::fmaf(person_distance(person, *c.object), attenuation(lib, *c.interaction, false), 1.0f);
    return (score - base) * factor;
}

std::vector<Valued> autonomy_list(const Library& lib, std::vector<Valued> values, bool visitor) {
    const float least = lib.tuning(visitor ? "min autonomy score for visitors" : "min autonomy score for family").value_or(0.0f);
    std::erase_if(values, [&](const Valued& v) { return v.value < least; });
    std::stable_sort(values.begin(), values.end(), [](const Valued& a, const Valued& b) { return b.value - a.value < -1e-7f; });
    return values;
}

std::optional<size_t> autonomy_pick(const Library& lib, const std::vector<Valued>& list, uint32_t random) {
    if (list.empty()) return std::nullopt;
    const uint32_t count = (uint32_t)lib.tuning("random selection count").value_or(1.0f);
    const uint32_t k = std::min<uint32_t>(std::max<uint32_t>(count, 1), (uint32_t)list.size());
    return random % k;
}

}  // namespace housefire
