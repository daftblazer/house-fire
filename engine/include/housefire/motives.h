// The motives' own update: what the engine does to a person's motives every so often, apart
// from the scripts (Bustin' Out's Motives::Sim, fn_801D0504; docs/simulation.md, "The
// motives"). Its rates are the global file's tuning values (FCNS table 1), by name.
#pragma once
#include <array>
#include <cstdint>
#include <span>

namespace housefire {

class Library;

// The motives by their place in a person's table of 16.
enum Motive : int { kEnergy = 5, kComfort = 6, kHunger = 7, kHygiene = 8, kBladder = 9, kSleepState = 11,
                    kSocial = 14, kFun = 15 };

struct MotiveRates {
    float energy_awake = 0, energy_asleep = 0, energy_drift = 0;
    int wake_hour = 0, sleep_hour = 0;            // the drift applies while asleep between them
    float hunger_to_bladder = 0, hunger_ratio = 0;
    float comfort_per_point = 0, comfort_base = 0;  // by person data 3
    float social_per_point = 0, social_base = 0;    // by person data 6
    float fun_awake = 0, fun_asleep_factor = 0, hygiene_awake = 0, hygiene_asleep = 0, bladder_awake = 0, bladder_asleep = 0;
    // From the tuning tables as fn_801CFFB4 derives them; false if one is missing.
    bool from(const Library& lib);
};

// One update of a person's motives (`now`) and its copy of them as of the last update
// (`last`, which it brings up to date): `p3` and `p6` its person data 3 and 6, `hour` the
// clock's hour (only read while asleep).
void update_motives(const MotiveRates& r, std::span<float> now, std::span<float> last, int p3, int p6, int hour);

}  // namespace housefire
