//! The motives' own update: what the engine does to a person's motives every so often, apart
//! from the scripts (Bustin' Out's Motives::Sim, fn_801D0504; `docs/simulation.md`, "The
//! motives"). Its rates are the global file's tuning values (FCNS table 1), by name.

use crate::library::Library;

/// The motives by their place in a person's table of 16.
pub mod motive {
    pub const ENERGY: usize = 5;
    pub const COMFORT: usize = 6;
    pub const HUNGER: usize = 7;
    pub const HYGIENE: usize = 8;
    pub const BLADDER: usize = 9;
    pub const SLEEP_STATE: usize = 11;
    pub const SOCIAL: usize = 14;
    pub const FUN: usize = 15;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotiveRates {
    pub energy_awake: f32,
    pub energy_asleep: f32,
    pub energy_drift: f32,
    /// The drift applies while asleep between them.
    pub wake_hour: i32,
    pub sleep_hour: i32,
    pub hunger_to_bladder: f32,
    pub hunger_ratio: f32,
    /// By person data 3.
    pub comfort_per_point: f32,
    pub comfort_base: f32,
    /// By person data 6.
    pub social_per_point: f32,
    pub social_base: f32,
    pub fun_awake: f32,
    pub fun_asleep_factor: f32,
    pub hygiene_awake: f32,
    pub hygiene_asleep: f32,
    pub bladder_awake: f32,
    pub bladder_asleep: f32,
}

impl MotiveRates {
    /// From the tuning tables as fn_801CFFB4 derives them; `None` if one is missing.
    pub fn from(lib: &Library) -> Option<MotiveRates> {
        let (r, ok) = Self::read(lib);
        ok.then_some(r)
    }

    /// The same, the rates found kept when one is missing (it counts 0), and whether all were.
    pub fn read(lib: &Library) -> (MotiveRates, bool) {
        let mut ok = true;
        let mut get = |name: &str| {
            let v = lib.tuning(name);
            ok &= v.is_some();
            v.unwrap_or(0.0)
        };
        // The game's own arithmetic, in floats: a span of energy over the waking and the
        // sleeping hours, in steps of 30 a game hour.
        let span = get("energy span");
        let wake_hours = get("wake hours") as i32;
        let wake_hour = get("wake hour") as i32;
        let active = get("comfort decrement active");
        let lazy = get("comfort decrement lazy");
        let r = MotiveRates {
            wake_hour,
            sleep_hour: wake_hour + wake_hours,
            energy_awake: -(span / wake_hours as f32) / 30.0,
            energy_asleep: (span / (24 - wake_hours) as f32) / 30.0,
            energy_drift: get("energy drift"),
            hunger_to_bladder: get("hunger to bladder multiplier"),
            comfort_per_point: (active - lazy) / 1000.0,
            comfort_base: lazy,
            hunger_ratio: get("hunger decrement ratio"),
            social_base: get("social decrement base"),
            social_per_point: get("social decrement multiplier"),
            fun_awake: get("ent decrement, awake"),
            fun_asleep_factor: get("ent multiplier, asleep"),
            hygiene_awake: get("hyg decrement, awake"),
            hygiene_asleep: get("hyg decrement, asleep"),
            bladder_awake: get("bladder decrement, awake"),
            bladder_asleep: get("bladder decrement, asleep"),
        };
        (r, ok)
    }
}

/// One update of a person's motives (`m`) and its copy of them as of the last update (`last`,
/// which it brings up to date): `p3` and `p6` its person data 3 and 6, `hour` the clock's hour
/// (only read while asleep).
pub fn update_motives(r: &MotiveRates, m: &mut [f32], last: &mut [f32], p3: i32, p6: i32, hour: i32) {
    use motive::*;
    if m.len() < 16 || last.len() < 16 {
        return;
    }
    let awake = m[SLEEP_STATE] >= 0.0;
    if awake {
        m[ENERGY] += r.energy_awake;
    } else {
        m[ENERGY] += r.energy_asleep;
        if hour > r.wake_hour && hour < r.sleep_hour {
            m[ENERGY] -= r.energy_drift;
        }
    }
    // What the person ate since the last update fills the bladder.
    let ate = m[HUNGER] - last[HUNGER];
    if ate > 0.0 {
        m[BLADDER] -= ate * r.hunger_to_bladder;
    }
    m[HUNGER] -= (m[HUNGER] + 100.0) * r.hunger_ratio;
    // These two as the original computes them, multiply and add in one rounding (fmadds).
    m[COMFORT] -= r.comfort_per_point.mul_add(p3 as f32, r.comfort_base);
    let social = r.social_per_point.mul_add(p6 as f32, r.social_base);
    if awake {
        m[SOCIAL] -= social;
        m[FUN] -= r.fun_awake;
        m[HYGIENE] -= r.hygiene_awake;
        m[BLADDER] -= r.bladder_awake;
    } else {
        m[FUN] *= r.fun_asleep_factor;
        m[HYGIENE] -= r.hygiene_asleep;
        m[BLADDER] -= r.bladder_asleep;
    }
    for i in 0..16 {
        m[i] = m[i].clamp(-100.0, 100.0);
        last[i] = m[i];
    }
}
