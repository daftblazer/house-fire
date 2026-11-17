//! The persons' directions (LOCKSTEP_DIRECTIONS=1, with the run's --card and --lot): at the end
//! of every tick (the start of the next), each person's data 1 in the engine's world against
//! the latest direction the log holds for it (the '_' records: the observer logs a person's
//! direction at the start of a tick when it changed). Prints one line, the person-ticks the
//! same and the different, and the first few differences (LOCKSTEP_DIRECTIONS_SHOW=N, default 8).
//! LOCKSTEP_DIRECTIONS_PERSONS=1: the figures by person; LOCKSTEP_DIRECTIONS_TRACE=ID: each
//! change of one person's direction in the engine, with the log's.

use crate::*;
use housefire::replay::{Replay, RunWith, Start};

pub fn directions(log: &housefire::tree_log::Log, d: &Disc, lib: &Library, card: Option<&str>, lot_index: i64) {
    let Ok(start) = Start::read(d, card, lot_index) else { return };
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let catalog = housefire::anim::Catalog::read(d, house);
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, cat: Some(catalog) }) else { return };
    let show: usize = std::env::var("LOCKSTEP_DIRECTIONS_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let trace: Option<i16> = std::env::var("LOCKSTEP_DIRECTIONS_TRACE").ok().and_then(|v| v.parse().ok());
    let mut logged: BTreeMap<i16, i16> = BTreeMap::new();
    let mut last_native: BTreeMap<i16, i16> = BTreeMap::new();
    let mut by_person: BTreeMap<i16, (u64, u64)> = BTreeMap::new();
    let mut next = 0;
    let (mut same, mut different, mut shown) = (0u64, 0u64, 0usize);
    // the directions the log gives up to tick t are those at the start of tick t + 1: the end
    // of tick t
    while replay.step() {
        let t = replay.tick();
        while next < log.directions.len() && log.directions[next].0 <= t {
            logged.insert(log.directions[next].1, log.directions[next].2);
            next += 1;
        }
        for o in replay.world().objects().filter(|o| o.person && o.on_lot()) {
            let Some(&want) = logged.get(&o.id()) else { continue };
            if trace == Some(o.id()) && last_native.insert(o.id(), o.facing()) != Some(o.facing()) {
                println!("   tick {t}: person {} is turned to {} (the log's {want})", o.id(), o.facing());
            }
            let e = by_person.entry(o.id()).or_default();
            if o.facing() == want {
                same += 1;
                e.0 += 1;
            } else {
                different += 1;
                e.1 += 1;
                if shown < show {
                    shown += 1;
                    println!("   tick {t}: person {} faces {}, the log's {want}", o.id(), o.facing());
                }
            }
        }
    }
    if std::env::var_os("LOCKSTEP_DIRECTIONS_PERSONS").is_some() {
        for (id, (s, dd)) in &by_person {
            println!("   person {id}: {s} the same, {dd} different");
        }
    }
    println!("directions: {same} person-ticks the same, {different} DIFFERENT");
}
