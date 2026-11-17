use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::autonomy::{
    autonomy_candidates, interaction_score, score_curves,
};
use crate::bhav::{TreeKind, tree_kind};
use crate::cfmt::g;
use crate::library::Advert;
use crate::motives::{MotiveRates, motive, update_motives};
use crate::tree_log::Log;
use crate::world::World;
use super::study::{has, file_name};
use super::counts::{Counts, Bench};
use super::routing::routing;
use super::run::Replay;

impl<'l> Replay<'l> {
    /// The log's lights, the lot's clock and the rest the run keeps are the world's; this ends
    /// the run: the checks that go over the whole log, and what was counted.
    pub fn finish(mut self) -> (Counts, Option<Bench>) {
        let log_rc = Rc::clone(&self.log);
        let log: &Log = &log_rc;
        let lib = self.lib;
        let best_streak = self.best_streak;
        let (club, house) = self.lot;
        let Self { h, finder, ids, .. } = &mut self;
        let world = &h.world;
        let counts = &mut h.counts;
        counts.tick = u32::MAX; // the checks over the whole log
        if has("REPLAY_SHOW_FILES") {
            let mut f: BTreeMap<String, i32> = BTreeMap::new();
            for s in &log.steps {
                let Some(o) = ids.get(&s.sim).and_then(|&id| world.find_object(id)) else { continue };
                if !o.person {
                    continue;
                }
                if let Some((_, file)) = finder.find(log, s)
                    && tree_kind(s.tree) == TreeKind::Own
                {
                    *f.entry(lib.file(file).path.clone()).or_default() += 1;
                }
            }
            for (p, n) in &f {
                println!("a lot Sim's own trees in {p}: {n} steps");
            }
        }
        let bench_on = h.bench.is_some();
        if !bench_on {
            counts.add("steps: longest run with the native stack as the log's", best_streak);
        }

        // Autonomy's evaluations: a person's many guard checks in one tick (each at node 0 in the
        // bottom frame of its checks), against the native candidates (autonomy_candidates), the
        // persons' interactions and a version-11 table's first tree set aside.
        if !bench_on {
            let mut starts: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new(); // (tick, stack) -> node-0 starts
            for (i, s) in log.steps.iter().enumerate() {
                if s.node == 0 {
                    starts.entry((s.tick, s.sim)).or_default().push(i);
                }
            }
            for (key, v) in &starts {
                if v.len() < 40 {
                    continue;
                }
                let mut per_frame: BTreeMap<u32, usize> = BTreeMap::new(); // the checks' bottom frame: the one with the most starts
                for &i in v {
                    *per_frame.entry(log.steps[i].frame).or_default() += 1;
                }
                let mut bottom = (0u32, 0usize);
                for (&f, &n) in &per_frame {
                    if n > bottom.1 || bottom.1 == 0 {
                        bottom = (f, n);
                    }
                }
                let bottom = bottom.0;
                let Some(me) = ids.get(&key.1).and_then(|&id| world.find(id)).filter(|&k| world.get(k).person) else { continue };
                // An evaluation goes over the lot: many objects (a pie menu's checks are of one).
                let over: BTreeSet<i16> = v.iter().map(|&i| &log.steps[i]).filter(|s| s.frame == bottom).map(|s| s.stack_object).collect();
                if over.len() < 10 || per_frame[&bottom] < 20 {
                    continue;
                }
                let mut logged: Vec<(i16, u16)> = Vec::new();
                for &i in v {
                    let s = &log.steps[i];
                    let Some(so) = world.find(s.stack_object) else { continue };
                    if s.frame != bottom || so == me {
                        continue;
                    }
                    let o = world.get(so);
                    let tf = if o.person { o.file.and_then(|f| lib.file(f).semi) } else { o.file };
                    let prefix = tf.is_some_and(|f| lib.file(f).prefix_of.values().any(|&p| p == s.tree));
                    if !prefix {
                        logged.push((s.stack_object, s.tree));
                    }
                }
                // The objects on the lot then (the log's list at that tick: the replay does not
                // remove objects natively yet, as the paper girl leaves).
                let mut present: BTreeSet<i16> = BTreeSet::new();
                for l in &log.lists {
                    if l.tick <= key.0 {
                        present = l.ids.iter().copied().collect();
                    }
                }
                let native: Vec<(i16, u16)> = autonomy_candidates(world, me)
                    .iter()
                    .filter(|c| present.is_empty() || present.contains(&c.id))
                    .map(|c| (c.id, c.interaction(lib).guard))
                    .collect();
                counts.note(if logged == native {
                    "autonomy: an evaluation's candidates the same natively (objects and persons)"
                } else {
                    "autonomy: an evaluation's candidates DIFFERENT"
                });
                counts.add("autonomy: candidates checked in evaluations", logged.len());
                if logged != native && has("REPLAY_SHOW_AUTONOMY") {
                    let lo: BTreeSet<i16> = logged.iter().map(|x| x.0).collect();
                    let na: BTreeSet<i16> = native.iter().map(|x| x.0).collect();
                    let mut l = "objects only native:".to_string();
                    for i in na.difference(&lo) {
                        let o = world.find_object(*i);
                        l += &format!(
                            " {i}({}, stack {})",
                            o.and_then(|o| o.file).map_or("?".to_string(), |f| file_name(lib, f)),
                            o.map_or(0, |o| o.stack.len())
                        );
                    }
                    println!("{l}");
                    let mut l = "only logged:".to_string();
                    for i in lo.difference(&na) {
                        l += &format!(" {i}");
                    }
                    println!("{l}");
                    if has("REPLAY_SHOW_CAND_FLAGS") {
                        let mut by_flag: BTreeMap<(u32, bool), Vec<i16>> = BTreeMap::new();
                        for c in autonomy_candidates(world, me).iter().filter(|c| present.is_empty() || present.contains(&c.id)) {
                            let e = c.interaction(lib);
                            by_flag.entry((e.flags, logged.contains(&(c.id, e.guard)))).or_default().push(c.id);
                        }
                        for ((f, inlog), ids) in by_flag {
                            println!("   flags {f:08x} checked by the original {inlog}: {} candidates, e.g. {:?}", ids.len(), &ids[..ids.len().min(6)]);
                        }
                        println!("   logged {logged:?}");
                        for &(id, _) in logged.iter().take(1) {
                            if let Some(o) = world.find_object(id) {
                                let d = world.definition(o);
                                println!("   {id}: file {:?} def {:?} field 6 {:?} field 7 {:?} tables {:?}", o.file.map(|f| file_name(lib, f)), o.definition, d.map(|d| d.field(6)), d.map(|d| d.field(7)), d.and_then(|d| o.file.and_then(|f| lib.interactions_of(d, f))));
                                if let (Some(d), Some(f)) = (d, o.file) && let Some(t) = lib.interactions_of(d, f) {
                                    for e in lib.table(f, t) {
                                        println!("      action {} guard {} threshold {} flags {:x}", e.action, e.guard, e.autonomy_threshold, e.flags);
                                    }
                                }
                            }
                        }
                    }
                    let mut k = 0;
                    while k < logged.len() && k < native.len() && logged[k] == native[k] {
                        k += 1;
                    }
                    println!(
                        "evaluation at tick {} by {}: {} logged, {} native, first difference at {k}: log {}/{}, native {}/{}",
                        key.0,
                        world.get(me).id(),
                        logged.len(),
                        native.len(),
                        logged.get(k).map_or(-1, |x| x.0 as i32),
                        logged.get(k).map_or(-1, |x| x.1 as i32),
                        native.get(k).map_or(-1, |x| x.0 as i32),
                        native.get(k).map_or(-1, |x| x.1 as i32)
                    );
                }
            }
        }

        // Autonomy's scores ('A', 'B' records): each recomputed natively (interaction_score, the
        // curves from STR# 501) from the motives and adverts it read; the person is the one whose
        // evaluation it was (the Sim of that tick's evaluation: Sim 12's or Sim 237's motives).
        if !bench_on && !log.scores.is_empty() {
            let curves = score_curves(lib);
            counts.add("autonomy: score curves from the global file", curves.len());
            for br in &log.bases {
                let Some(sr) = log.scores.iter().find(|x| x.tick == br.tick) else { continue };
                let Some(any) = world.objects().find(|o| o.person && o.definition.is_none()) else { continue };
                let got = interaction_score(&curves, None, any, &sr.motives);
                counts.note(if got == br.score { "autonomy: a base score the same natively (no advert)" } else { "autonomy: a base score DIFFERENT" });
            }
            for sr in &log.scores {
                // The person: the Sim whose motives these are (person data for the personality).
                let mut person = None;
                for o in world.objects() {
                    if o.person && o.definition.is_none() && o.motives.len() >= 16 {
                        let near = [5, 7, 9].iter().all(|&i| (o.motives[i] - sr.motives[i]).abs() <= 60.0);
                        if near && person.is_none_or(|p: &crate::world::Object| (o.motives[5] - sr.motives[5]).abs() < (p.motives[5] - sr.motives[5]).abs()) {
                            person = Some(o);
                        }
                    }
                }
                let Some(person) = person else {
                    counts.note("autonomy: a score's person not found");
                    continue;
                };
                let ads: Vec<Advert> = (0..16).map(|m| Advert { min: sr.adverts[m][1], delta: sr.adverts[m][2], personality: sr.adverts[m][0] as u16 }).collect();
                let got = interaction_score(&curves, Some(&ads), person, &sr.motives);
                counts.note(if got == sr.score {
                    "autonomy: a score the same natively (bit for bit)"
                } else if (got - sr.score).abs() < 1e-5 {
                    "autonomy: a score the same natively but for the last bits"
                } else {
                    "autonomy: a score DIFFERENT"
                });
            }
        }

        // Routing: each Sim's logged step from one tile to the next ('P'), against the lot's walls
        // and the doors' openings (Walls). Measured on the save's lot.
        if !bench_on && !log.places.is_empty() {
            // the walkers' bodies from the lot as it loaded (a walker's id may be another object's,
            // or none, by the run's end)
            let mut loaded = World::new(lib);
            loaded.load_lot(club).expect("the lot");
            if let Some(h) = house {
                let _ = loaded.take_house_from(h);
            }
            let _ = routing(log, lib, world, &loaded, counts, ids);
        }

        // The motives' own update (update_motives), on each person's logged motives ('N' records):
        // the native update of one record against the next. A pair it does not reproduce is a change
        // the scripts made, or a tick the original did not update it.
        if has("REPLAY_SHOW_TUNING")
            && let Some(gf) = lib.global_file()
        {
            for (id, t) in &gf.tuning {
                for (name, v) in t {
                    println!("tuning {id}: {name} = {}", g(*v as f64, 6));
                }
            }
        }
        if !bench_on && !log.motives.is_empty() {
            let (rates, ok) = MotiveRates::read(lib);
            if !ok {
                counts.note("motives: a rate missing from the tuning tables");
            }
            let mut by_person: BTreeMap<i16, Vec<usize>> = BTreeMap::new();
            for (k, m) in log.motives.iter().enumerate() {
                by_person.entry(m.id).or_default().push(k);
            }
            for (id, recs) in &by_person {
                let Some(o) = world.find_object(*id) else { continue };
                if !o.person || o.person_data.len() < 7 || o.motives_b.len() < 16 {
                    continue;
                }
                let mut last: [f32; 16] = o.motives_b[..16].try_into().unwrap();
                for k in 1..recs.len() {
                    let (prev, cur) = (&log.motives[recs[k - 1]], &log.motives[recs[k]]);
                    let mut now = prev.motives;
                    let mut copy = last;
                    update_motives(&rates, &mut now, &mut copy, o.person_data[3] as i32, o.person_data[6] as i32, 12);
                    let same = (0..16).all(|i| i == 3 || i == 13 || now[i] == cur.motives[i]);
                    if !same && has("REPLAY_SHOW_MOTIVES") {
                        let near = (0..16).filter(|&i| i != 3 && i != 13 && (now[i] - cur.motives[i]).abs() < 1e-3).count();
                        if near == 14 {
                            for (i, (&a, &b)) in now.iter().zip(&cur.motives).enumerate() {
                                if i != 3 && i != 13 && a != b {
                                    println!(
                                        "motive {i} of {id} at tick {}: native {}, log {} (before {})",
                                        cur.tick,
                                        g(a as f64, 9),
                                        g(b as f64, 9),
                                        g(prev.motives[i] as f64, 9)
                                    );
                                }
                            }
                        }
                    }
                    counts.note(if same {
                        "motives: an update the same natively (bit for bit, all but 3 and 13)"
                    } else {
                        "motives: a change not the update's (a script's, or no update)"
                    });
                    // An update step (energy moves only by the update): the copy is the motives after it.
                    let update_step = now[motive::ENERGY] == cur.motives[motive::ENERGY] && cur.motives[motive::ENERGY] != prev.motives[motive::ENERGY];
                    if update_step {
                        last = cur.motives;
                    }
                    if !same && update_step {
                        counts.note("motives: an update step with a script's change besides");
                    }
                }
            }
        }
        let bench = h.bench.take();
        (std::mem::take(&mut h.counts), bench)
    }
}
