// Autonomy: what a Sim with nothing to do considers and chooses (docs/simulation.md,
// "Autonomy"). Measured on the recompiled Bustin' Out's home log (its three evaluations).
#pragma once
#include <cstdint>
#include <optional>
#include <vector>

#include <functional>

#include "housefire/interp.h"
#include "housefire/library.h"
#include "housefire/world.h"

namespace housefire {

// One interaction considered: the object, its interaction (in the object's table).
struct Candidate {
    const Object* object;
    const Interaction* interaction;
};

// The interactions an evaluation considers, in the order it checks their guards: the lot's
// objects by id (not of definition type 7, nor a part whose definition's field 6 is 1 to
// 0xFFFD), each one's table (Library::interactions_of) in reverse order, an interaction
// whose autonomy threshold is below 100 and whose flags lack 0x80 and 0x40, a guard several
// share once. A person's table is its semi-global file's (person_sg, NPC_SG). Measured: the
// home log's three evaluations, objects and persons, the same.
std::vector<Candidate> autonomy_candidates(const World& world, const Object& me);

// The interaction score's curves (Bustin' Out's fn_80267E9C; docs/simulation.md, "The
// curves"): one a motive, its points (x, y) by x ascending and each segment's 1 / width.
struct ScoreCurve {
    int motive;
    std::vector<std::pair<float, float>> points;
    std::vector<float> slopes;
    float at(float v) const;
};
// From the global file's STR# 501, "InteractionScoreCurves": "(x;y) (x;y) ...", its strings
// for motives 5, 6, 7, 8, 9, 3, 13, 14 and 15 in turn (the tenth is not used here).
std::vector<ScoreCurve> score_curves(const Library& lib);

// An advert's amount for a person (its minimum, plus the delta scaled by the personality
// field it names, or 1000 less it), as the score takes it.
float advert_amount(const Interaction::Advert& a, const Object& person);
// The score of a set of adverts (one a motive; null: none) for a person with these motives.
float interaction_score(const std::vector<ScoreCurve>& curves, const std::vector<Interaction::Advert>* adverts,
                        const Object& person, const std::vector<float>& motives);

// A candidate's value to a person (fn_802266BC): its score less the base (the score of no
// advert), over 1 + the attenuation × the distance in tiles (fn_801F1C18: from the two
// places, 20 a level). The attenuation by the interaction's code: 1 none, 2, 3, 4 the tuning
// table's low, moderate and high (FCNS 2), 0 the interaction's own value.
float game_sqrt(float s);
float person_distance(const Object& a, const Object& b);
float attenuation(const Library& lib, const Interaction& e, bool visitor);
float candidate_value(const Library& lib, const std::vector<ScoreCurve>& curves, const Object& person,
                      const Candidate& c, float base);

// The list a choice is made from (fn_802266BC): the passing candidates whose value reaches
// the tuning's minimum (FCNS 2, "min autonomy score for family" or "... for visitors"), best
// first (values within 1e-7 kept in the candidates' order).
struct Valued {
    float value;
    const Candidate* candidate;
};
std::vector<Valued> autonomy_list(const Library& lib, std::vector<Valued> values, bool visitor);

// The choice's place in that list: one of the best "random selection count" (4) by a random
// number (its remainder by that count, or by the list's length if shorter). Nullopt when the
// list is empty. The original takes the best instead for a visitor's evaluation and when the
// best entry has a flag of its interaction's set (not identified: none in the home log), and
// none for a sitting Sim below "min autonomy score for sitting"; not built, as the log has
// none of them. Measured: the home log's three choices, from the original's own random
// numbers ('R'), the same as the original's.
std::optional<size_t> autonomy_pick(const Library& lib, const std::vector<Valued>& list, uint32_t random);

// One check of an evaluation: the object, the tree (a table's first tree, or a candidate's
// guard; `candidate` null for a first tree), and its result (nullopt if it did not finish).
struct EvaluationCheck {
    const Object* object;
    uint16_t tree;
    const Candidate* candidate;
    std::optional<bool> passed;
};

// The evaluation (opcode 3, run by the Sims' "try autonomy", PersonGlobals 8233): every
// candidate's checks through the host (Host::run_check, the stack object the candidate), an
// object's table's first tree once before its guards. `present` (if given) says which objects
// are on the lot. Measured: the home log's three evaluations (docs/simulation.md, "Autonomy").
std::vector<EvaluationCheck> run_evaluation(const World& world, const Object& me, Host& host,
                                            const std::vector<Candidate>& candidates);

}  // namespace housefire
