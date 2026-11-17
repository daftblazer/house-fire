//! The persons' directions (LOCKSTEP_DIRECTIONS=1, with the run's --card and --lot): at the end
//! of every tick (the start of the next), each person's data 1 in the engine's world against
//! the latest direction the log holds for it (the '_' records: the observer logs a person's
//! direction at the start of a tick when it changed). Prints one line, the person-ticks the
//! same and the different, and the first few differences (LOCKSTEP_DIRECTIONS_SHOW=N, default 8;
//! LOCKSTEP_DIRECTIONS_PERSONS=1: the figures by person).

use crate::*;
use housefire::replay::{Replay, RunWith, Start};

pub fn directions(log: &housefire::tree_log::Log, d: &Disc, lib: &Library, card: Option<&str>, lot_index: i64) {
    let Ok(start) = Start::read(d, card, lot_index) else { return };
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let catalog = housefire::anim::Catalog::read(d, house);
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, cat: Some(catalog) }) else { return };
    let show: usize = std::env::var("LOCKSTEP_DIRECTIONS_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    if let Some(id) = std::env::var("LOCKSTEP_DIRECTIONS_DUMP").ok().and_then(|v| v.parse::<i16>().ok()) {
        println!("ALLDIRS {:?}", log.directions);
        let mut fs: BTreeMap<u32, Vec<&housefire::tree_log::FollowRec>> = BTreeMap::new();
        for f in &log.follows {
            fs.entry(f.follower).or_default().push(f);
        }
        for (fo, v) in &fs {
            let mut i = 0;
            while i < v.len() {
                let mut j = i;
                while j + 1 < v.len() && v[j + 1].before.state == v[i].before.state && v[j + 1].after.state == v[i].after.state && v[j + 1].before.factor == v[i].before.factor {
                    j += 1;
                }
                println!("RUN {fo:x} person {}: ticks {}..{} x{}: state {} -> {} factor {} place ({:.1},{:.1}) -> ({:.1},{:.1}) point {:?} mode {}", v[i].person, v[i].tick, v[j].tick, j - i + 1, v[i].before.state, v[j].after.state, v[i].before.factor, v[i].before.x, v[i].before.y, v[j].after.x, v[j].after.y, v[j].after.point, v[j].after.mode);
                i = j + 1;
            }
        }
        for pl in &log.plans {
            println!("PLAN tick {} state {} points {:?}", pl.tick, pl.state, pl.points);
        }
        println!("follows {} directions {}", log.follows.len(), log.directions.len());
        let mut ev: Vec<(u32, String)> = Vec::new();
        for r in log.directions.iter().filter(|r| r.1 == id) {
            ev.push((r.0, format!("dir {}", r.2)));
        }
        for r in log.places.iter().filter(|r| r.id == id) {
            ev.push((r.tick, format!("place {},{}", r.x, r.y)));
        }
        for f in log.follows.iter().filter(|f| f.tick <= 130) {
            ev.push((f.tick, format!("follow {:x}: state {} -> {} place ({:.1},{:.1}) -> ({:.1},{:.1}) factor {} point {:?} mode {}", f.follower, f.before.state, f.after.state, f.before.x, f.before.y, f.after.x, f.after.y, f.before.factor, f.after.point, f.after.mode)));
        }
        for r in log.directions.iter() {
            ev.push((r.0, format!("ALL dir person {} {}", r.1, r.2)));
        }
        ev.sort();
        for (t, s) in ev.iter().take(400) {
            println!("   {t}: {s}");
        }
    }
    if let Some((id, t0, t1)) = std::env::var("LOCKSTEP_DIRECTIONS_STEPS").ok().and_then(|v| { let p: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect(); (p.len() == 3).then(|| (p[0] as i16, p[1] as u32, p[2] as u32)) }) {
        for s in log.steps.iter().filter(|s| s.tick >= t0 && s.tick <= t1 && s.stack_object == id) {
            println!("   step tick {} tree {:04x} node {} result {} params {:?} file {:x}", s.tick, s.tree, s.node, s.result, s.params, s.file);
        }
    }
    if std::env::var_os("LOCKSTEP_DIRECTIONS_ROUTING").is_some() {
        for s in log.steps.iter().filter(|s| s.tick <= 200) {
            let Some(t) = log.tree_of(s) else { continue };
            let n = s.node as usize * 12;
            if t.tree != s.tree || n + 12 > t.raw.len() {
                continue;
            }
            let op = u16::from_be_bytes([t.raw[n], t.raw[n + 1]]) & 0x7FFF;
            if op == 45 {
                println!("   routing step tick {} sim {:x} stack object {} -> {} tree {:04x} node {} result {} operand {:02x?} params {:?}", s.tick, s.sim, s.stack_object, s.stack_object_after, s.tree, s.node, s.result, &t.raw[n + 4..n + 12], s.params);
            }
        }
    }
    if std::env::var_os("LOCKSTEP_DIRECTIONS_OBJECTS").is_some() {
        for id in [135, 153, 155] {
            if let Some(o) = replay.world().find_object(id) {
                println!("   object {id}: place ({}, {}) facing {} file {:?} person {}", o.x16, o.y16, o.facing(), o.file.map(|f| lib.file(f).path.clone()), o.person);
            }
        }
    }
    let mut logged: BTreeMap<i16, i16> = BTreeMap::new();
    let mut next = 0;
    let (mut same, mut different) = (0u64, 0u64);
    let mut shown = 0;
    let trace: Option<i16> = std::env::var("LOCKSTEP_DIRECTIONS_TRACE").ok().and_then(|v| v.parse().ok());
    let mut last_native: BTreeMap<i16, i16> = BTreeMap::new();
    let mut by_person: BTreeMap<i16, (u64, u64)> = BTreeMap::new();
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
                println!("   trace tick {t}: person {} native {} (the log's {want})", o.id(), o.facing());
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
