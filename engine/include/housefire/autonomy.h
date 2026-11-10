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
