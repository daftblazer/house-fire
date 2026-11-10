// Autonomy: what a Sim with nothing to do considers and chooses (docs/simulation.md,
// "Autonomy"). Measured on the recompiled Bustin' Out's home log (its three evaluations).
#pragma once
#include <cstdint>
#include <vector>

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
// whose autonomy threshold is below 100 and whose flags lack 0x80, a guard several share once.
// Persons are not in it yet (their interactions are the person files'). Measured: all 117
// objects' sequences in the home log's three evaluations the same.
std::vector<Candidate> autonomy_candidates(const World& world, const Object& me);

}  // namespace housefire
