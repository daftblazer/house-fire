#include "housefire/motives.h"

#include <algorithm>
#include <cmath>

#include "housefire/library.h"

namespace housefire {

bool MotiveRates::from(const Library& lib) {
    bool ok = true;
    auto get = [&](const char* name) {
        const auto v = lib.tuning(name);
        if (!v) ok = false;
        return v.value_or(0.0f);
    };
    // The game's own arithmetic, in floats: a span of energy over the waking and the
    // sleeping hours, in steps of 30 a game hour.
    const float span = get("energy span");
    const int wake_hours = (int)get("wake hours");
    wake_hour = (int)get("wake hour");
    sleep_hour = wake_hour + wake_hours;
    energy_awake = -(span / (float)wake_hours) / 30.0f;
    energy_asleep = (span / (float)(24 - wake_hours)) / 30.0f;
    energy_drift = get("energy drift");
    hunger_to_bladder = get("hunger to bladder multiplier");
    const float active = get("comfort decrement active"), lazy = get("comfort decrement lazy");
    comfort_per_point = (active - lazy) / 1000.0f;
    comfort_base = lazy;
    hunger_ratio = get("hunger decrement ratio");
    social_base = get("social decrement base");
    social_per_point = get("social decrement multiplier");
    fun_awake = get("ent decrement, awake");
    fun_asleep_factor = get("ent multiplier, asleep");
    hygiene_awake = get("hyg decrement, awake");
    hygiene_asleep = get("hyg decrement, asleep");
    bladder_awake = get("bladder decrement, awake");
    bladder_asleep = get("bladder decrement, asleep");
    return ok;
}

void update_motives(const MotiveRates& r, std::span<float> m, std::span<float> last, int p3, int p6, int hour) {
    if (m.size() < 16 || last.size() < 16) return;
    const bool awake = m[kSleepState] >= 0.0f;
    if (awake) {
        m[kEnergy] += r.energy_awake;
    } else {
        m[kEnergy] += r.energy_asleep;
        if (hour > r.wake_hour && hour < r.sleep_hour) m[kEnergy] -= r.energy_drift;
    }
    // What the person ate since the last update fills the bladder.
    const float ate = m[kHunger] - last[kHunger];
    if (ate > 0.0f) m[kBladder] -= ate * r.hunger_to_bladder;
    m[kHunger] -= (m[kHunger] + 100.0f) * r.hunger_ratio;
    // These two as the original computes them, multiply and add in one rounding (fmadds).
    m[kComfort] -= std::fmaf(r.comfort_per_point, (float)p3, r.comfort_base);
    const float social = std::fmaf(r.social_per_point, (float)p6, r.social_base);
    if (awake) m[kSocial] -= social;
    if (awake) {
        m[kFun] -= r.fun_awake;
        m[kHygiene] -= r.hygiene_awake;
        m[kBladder] -= r.bladder_awake;
    } else {
        m[kFun] *= r.fun_asleep_factor;
        m[kHygiene] -= r.hygiene_asleep;
        m[kBladder] -= r.bladder_asleep;
    }
    for (size_t i = 0; i < 16; i++) {
        m[i] = std::clamp(m[i], -100.0f, 100.0f);
        last[i] = m[i];
    }
}

}  // namespace housefire
