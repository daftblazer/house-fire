
use super::study::has;
use super::counts::Counts;

/// The '3' records of tick `t` against the engine's clock and the light it gives
/// (`light::time_of_day`, `light::sun_direction`).
pub(super) fn check_time_of_day(log: &crate::tree_log::Log, t: u32, world: &crate::world::World, at: &mut usize, counts: &mut Counts) {
    while *at < log.times_of_day.len() && log.times_of_day[*at].0 < t {
        *at += 1;
    }
    let g = |k: usize| world.globals.get(k).copied().unwrap_or(0) as i32;
    let (hour, minute, second) = (g(0), g(5), g(6));
    if has("REPLAY_CLOCK") && t < 3 {
        println!("  tick {t}: globals 0-8 {:?}", &world.globals[..world.globals.len().min(9)]);
    }
    while *at < log.times_of_day.len() && log.times_of_day[*at].0 == t {
        let (_, r, _, o) = &log.times_of_day[*at];
        *at += 1;
        if [r[2] as i32, r[3] as i32, r[4] as i32] != [hour, minute, second] {
            counts.note("light: time of day: DIFFERENT clock");
            if has("REPLAY_CLOCK") {
                println!("  tick {t}: the engine's clock {hour}:{minute}:{second}, the log's {}:{}:{} (globals {:?})", r[2] as i32, r[3] as i32, r[4] as i32, world.globals);
            }
            continue;
        }
        counts.note("light: time of day: the clock as the log's");
        let d = crate::light::time_of_day(hour, minute, second);
        let dir = crate::light::sun_direction(hour, minute, second).unwrap_or([f32::NAN; 3]);
        let mine = [d.sun_factor, d.sun[0], d.sun[1], d.sun[2], dir[0], dir[1], dir[2], d.extra, d.ambient_factor, d.ambient[0], d.ambient[1], d.ambient[2]].map(f32::to_bits);
        let theirs = [o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11], o[12]];
        counts.note(if mine == theirs { "light: time of day: the sun and ambient as the log's" } else { "light: time of day: DIFFERENT sun or ambient" });
    }
}
