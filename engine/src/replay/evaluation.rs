use std::collections::BTreeSet;
use std::rc::Rc;

use crate::autonomy::{
    Candidate, EvaluationCheck, Valued, attenuation, autonomy_candidates, autonomy_list, autonomy_pick, candidate_value, interaction_score,
    person_distance, run_evaluation, score_curves,
};
use crate::bhav::Node;
use crate::cfmt::g;
use crate::interp::{Host, Interpreter, PrimResult};
use crate::primitives::function_entry;
use crate::tree_log::{Log, StepRec};
use crate::FileId;
use super::study::{has, file_name};
use super::host::{ReplayHost};

impl ReplayHost<'_> {
    /// FindBestObjectForFunction (opcode 14), as the original's fn_8020CC8C (docs/primitives.md):
    /// the function's (rating datum, function-table entry); each object whose function table
    /// (OBJf) has an action for the entry and whose rating (its data, a: -1 for 50) is not 0,
    /// its test tree (if any) run as a check on it; the score the rating less three times the
    /// distance, at least 1, the best (the first of equals) the stack object, true; none, false.
    pub(super) fn find_best_object(&mut self, interp: &mut Interpreter, s: &StepRec, n: &Node) -> PrimResult {
        let lib = self.lib;
        let me = self.me;
        let Some((a, b)) = function_entry(n.operand[0]) else { return self.logged(s, n) };
        let (mx, my) = (self.world.get(me).x16 as f32 / 16.0, self.world.get(me).y16 as f32 / 16.0);
        let defaults = interp.defaults().to_vec();
        let candidates: Vec<(i16, Option<FileId>, u16, f32, f32, f32)> = self
            .world
            .objects()
            .filter(|o| o.id() != self.world.get(me).id())
            // in use (its data 25, the int16 at +88, above 0) left out
            .filter(|o| o.data.get(25).is_none_or(|&u| u <= 0))
            .filter_map(|o| {
                let f = o.file?;
                let &(test, action) = lib.file(f).functions.values().next()?.get(b)?;
                if action == 0 {
                    return None;
                }
                let rating = if a < 0 { 50 } else { *o.data.get(a as usize)? };
                (rating != 0).then_some((o.id(), Some(f), test, f32::from(rating), o.x16 as f32 / 16.0, o.y16 as f32 / 16.0))
            })
            .collect();
        let mut best: Option<(i16, f32)> = None;
        for (id, file, test, rating, x, y) in candidates {
            if test != 0 {
                let ok = lib.tree(file, test).and_then(|t| self.run_check(t, file, id, &defaults));
                if ok != Some(true) {
                    continue;
                }
            }
            let dist = ((x - mx) * (x - mx) + (y - my) * (y - my)).sqrt();
            let score = (rating - 3.0 * dist).max(1.0);
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((id, score));
            }
        }
        let want = self.logged(s, n);
        let got = match best {
            Some((id, _)) => {
                interp.top().stack_object = id;
                PrimResult::True
            }
            None => PrimResult::False,
        };
        self.counts.note(if got == want && best.map(|b| b.0) == Some(s.stack_object_after).or(if got == PrimResult::False { Some(s.stack_object_after) } else { None }) {
            "primitives: FindBestObjectForFunction natively, the original's object"
        } else {
            "primitives: FindBestObjectForFunction natively, DIFFERENT"
        });
        if has("REPLAY_SHOW_FIND_BEST") {
            println!("find best natively: tick {} function {} found {best:?}, log {} {:?}", s.tick, n.operand[0], s.stack_object_after, want);
        }
        got
    }

    /// The autonomy evaluation (opcode 3), natively: its checks against the log's, its scores,
    /// the list and the choice.
    pub(super) fn evaluation(&mut self, interp: &mut Interpreter, s: &StepRec, n: &Node) -> PrimResult {
        let log_rc = Rc::clone(&self.log);
        let log: &Log = &log_rc;
        let lib = self.lib;
        let me = self.me;
        let me_id = self.me_id();
        let cands: Vec<Candidate> = autonomy_candidates(&self.world, me)
            .into_iter()
            .filter(|c| self.present.is_empty() || self.present.contains(&c.id))
            .collect();
        // The engine's evaluation, each check's result against the log's: the step that ends it
        // (its bottom frame's return, 254 true or 255 false) is the last nested step used.
        self.on_check = Some((0, 0));
        if has("REPLAY_SHOW_CAND_FIELDS") && self.shown.next("cand fields", 1) {
            let pd = &self.world.get(me).person_data;
            println!("evaluator {me_id} pd32 {:?} pd36 {:?}", pd.get(32), pd.get(36));
            for c in &cands {
                let e = c.interaction(lib);
                println!("  cand {} guard {} threshold {} flags {:#x}", c.id, e.guard, e.autonomy_threshold, e.flags);
            }
        }
        if has("REPLAY_SHOW_GUARD_RUNS") {
            let pd = &self.world.get(me).person_data;
            let guards: BTreeSet<(i16, u16)> = cands.iter().map(|c| (c.id, c.interaction(lib).guard)).collect();
            let ran = self.nested.iter().any(|&i| guards.contains(&(log.steps[i].stack_object, log.steps[i].tree)));
            note!(self.counts, "study guards run by the original: Sim {me_id} pd32 {:?} pd36 {:?} pd29 {:?} ran {ran}", pd.get(32), pd.get(36), pd.get(29));
        }
        if has("REPLAY_SHOW_CAND_IDS") && self.shown.next("cand ids", 1) {
            let ids: BTreeSet<i16> = cands.iter().map(|c| c.id).collect();
            println!("candidate ids at tick {} by {me_id}: {ids:?}", s.tick);
        }
        if has("REPLAY_SHOW_EVAL_ORDER") && self.shown.next("eval order", 2) {
            let mut l = format!("evaluation order at tick {} by {me_id} ({} nested steps):", s.tick, self.nested.len());
            let mut prev = None;
            for &i in &self.nested {
                let t = &log.steps[i];
                let key = (t.tree, t.stack_object);
                if t.node == 0 && prev != Some((key, t.frame)) {
                    l += &format!(" {}/{}@{}", t.tree, t.stack_object, t.frame);
                }
                prev = Some((key, t.frame));
            }
            println!("{l}");
            for id in [3i16, 18, 33, 150, 135] {
                if let Some(o) = self.world.find_object(id) {
                    let d = self.world.definition(o);
                    println!("   {id}: {} at ({},{},{}) leader {} data 0..12 {:?} def 6 {:?} 7 {:?} 8 {:?} 11 {:?}", o.file.map_or("?".into(), |f| file_name(lib, f)), o.x16 >> 4, o.y16 >> 4, o.level, o.leader, &o.data[..12], d.map(|d| d.field(6)), d.map(|d| d.field(7)), d.map(|d| d.field(8)), d.map(|d| d.field(11)));
                }
            }
            let visited: BTreeSet<i16> = self.nested.iter().map(|&i| log.steps[i].stack_object).collect();
            let mut seen = BTreeSet::new();
            for c in &cands {
                if seen.insert(c.id) && let Some(o) = self.world.find_object(c.id) {
                    let tab = lib.table_of(c.object_file, c.person, c.def.map(|d| lib.def(d)));
                    println!("   cand {} visited {}: first {} file {}", c.id, visited.contains(&c.id), tab.first, o.file.map_or("?".into(), |f| file_name(lib, f)));
                }
            }
            if let Some(m) = self.world.find_object(me_id) {
                println!("   me {me_id} person data {:?} global 10 {:?}", m.person_data, self.world.globals.get(10));
                println!("   me {me_id} at ({},{},{}) room {:?}", m.x16 >> 4, m.y16 >> 4, m.level, self.world.walls.room(m.x16 >> 4, m.y16 >> 4));
            }
            let mut l = "   native candidates:".to_string();
            for c in cands.iter().take(40) {
                l += &format!(" {}/{}", c.id, c.interaction(lib).guard);
            }
            println!("{l}");
        }
        if has("REPLAY_SHOW_EVAL_OPERAND") {
            println!("evaluation node at tick {} by {me_id}: tree {}:{} operand {:02x?} so {} params {:?}", s.tick, s.tree, s.node, n.operand, s.stack_object, s.params);
            for id in [me_id, 150] {
                if let Some(o) = self.world.find_object(id) {
                    println!("   {id}: {} at ({},{},{}) container {} person {} data 0..12 {:?} person data 0..20 {:?}", o.file.map_or("?".into(), |f| file_name(lib, f)), o.x16 >> 4, o.y16 >> 4, o.level, o.data[2], o.person, &o.data[..12], o.person_data.get(..20));
                }
            }
        }
        if has("REPLAY_SHOW_AUTONOMY") {
            let mut l = format!("evaluation by {me_id}: {} candidates, present {}; objects", cands.len(), self.present.len());
            let mut lastid = -1;
            for c in &cands {
                if c.id != lastid {
                    lastid = c.id;
                    if lastid > 190 {
                        l += &format!(" {lastid}");
                    }
                }
            }
            println!("{l}; 197 present {}, in world {}", self.present.contains(&197) as i32, self.world.find(197).is_some() as i32);
        }
        let defaults = interp.defaults().to_vec();
        let checks: Vec<EvaluationCheck> = run_evaluation(lib, self, &cands, &defaults);
        let (same, different) = self.on_check.take().unwrap();
        let unfinished = checks.iter().filter(|c| c.passed.is_none()).count();
        for c in checks.iter().filter(|c| c.candidate.is_none()) {
            note!(self.counts, "autonomy study: a first tree's check {:?}", c.passed);
            if has("REPLAY_SHOW_FIRST_TREES") { note!(self.counts, "study first tree {} {:?}", c.tree, c.passed); }
        }
        let mut native_choice: Option<i16> = None;
        {
            let curves = score_curves(lib);
            let person = self.world.get(me);
            let base = interaction_score(&curves, None, person, &person.motives);
            if has("REPLAY_SCORE_GATE") {
                // each candidate's score natively against the base, by whether the original ran
                // its guard (a step of the guard tree with the candidate as the stack object)
                let ran: BTreeSet<(i16, u16)> = self.nested.iter().map(|&i| (log.steps[i].stack_object, log.steps[i].tree)).collect();
                for c in &cands {
                    let e = c.interaction(lib);
                    let sc = interaction_score(&curves, Some(&e.adverts), person, &person.motives);
                    let checked = ran.contains(&(c.id, e.guard));
                    let rel = if sc > base { "above base" } else if sc == base { "at base" } else { "below base" };
                    note!(self.counts, "study score gate: {} score {rel}, guard run by the original {checked}", if self.world.globals.get(10) == Some(&1) { "home" } else { "club" });
                }
            }
            let mut values: Vec<(f32, usize)> = Vec::new();
            let (mut scores_same, mut scores_diff) = (0, 0);
            let mut values_scores: Vec<f32> = Vec::new();
            for c in &checks {
                let Some(ci) = c.candidate else { continue };
                if c.passed != Some(true) {
                    continue;
                }
                let cand = &cands[ci];
                // The log's score of this one (the scores come in the order of the passing
                // guards).
                // its advertisement as its guard left the ad range (scope 12): the deltas
                let mut adverts = cand.interaction(lib).adverts.to_vec();
                for (m, a) in adverts.iter_mut().enumerate() {
                    if let Some(&d) = c.ad_range.get(m) {
                        a.delta = d;
                    }
                }
                let native_score = interaction_score(&curves, Some(&adverts), person, &person.motives);
                values_scores.push(native_score);
                while self.score_at < log.scores.len() && log.scores[self.score_at].tick < s.tick {
                    self.score_at += 1;
                }
                if self.score_at < log.scores.len() && log.scores[self.score_at].tick == s.tick {
                    let tick_scores = log.scores.iter().filter(|x| x.tick == s.tick);
                    if native_score == log.scores[self.score_at].score {
                        scores_same += 1;
                    } else if tick_scores.clone().any(|x| x.score == native_score) {
                        // the log's order of the tick's scores is not the candidates' here (one
                        // more of its calls before): one of the tick's, bit for bit
                        self.counts.note("autonomy: a candidate's score the same natively as one of the tick's (out of the log's order)");
                    } else {
                        scores_diff += 1;
                        // with motive 14's advertisement as the original scored it (its logged
                        // entry read as personality, min, delta): made at run time, not native
                        {
                            let mut adv = cand.interaction(lib).adverts.to_vec();
                            adv.resize(16, Default::default());
                            let lg = log.scores[self.score_at].adverts[14];
                            adv[14] = crate::library::Advert { min: lg[1], delta: lg[2], personality: lg[0] as u16 };
                            self.counts.note(if interaction_score(&curves, Some(&adv), person, &person.motives) == log.scores[self.score_at].score {
                                "autonomy: a differing score the log's with motive 14's advertisement as the original made it"
                            } else {
                                "autonomy: a differing score NOT the log's with motive 14's advertisement as the original made it"
                            });
                        }
                        if has("REPLAY_SHOW_SCORES") {
                            let a = &cand.interaction(lib).adverts;
                            let mut l = format!("score DIFFERENT tick {} by {} on {} (interaction {}): native {native_score:e}, log {:e}; adverts (motive: native min/delta/personality | log)", s.tick, person.id(), cand.id, cand.interaction(lib).string_index, log.scores[self.score_at].score);
                            for (m, lg) in log.scores[self.score_at].adverts.iter().enumerate() {
                                let n = a.get(m).copied().unwrap_or_default();
                                if (n.min, n.delta, n.personality as i16) != (lg[0], lg[1], lg[2]) {
                                    l += &format!(" {m}: {}/{}/{} | {}/{}/{}", n.min, n.delta, n.personality, lg[0], lg[1], lg[2]);
                                }
                            }
                            // the native score with the log's adverts: are they the original's input?
                            let logged: Vec<crate::library::Advert> = log.scores[self.score_at].adverts.iter().map(|a| crate::library::Advert { min: a[0], delta: a[1], personality: a[2] as u16 }).collect();
                            let with_logged = interaction_score(&curves, Some(&logged), person, &person.motives);
                            l += &format!("; with the log's adverts natively {with_logged:e} ({})", if with_logged == log.scores[self.score_at].score { "the log's" } else { "not the log's" });
                            // motive 14's advertisement that gives the log's score natively
                            let mut found = Vec::new();
                            let mut tryv = a.to_vec();
                            tryv.resize(16, Default::default());
                            for delta in -100i16..=100 {
                                for min in [0i16, tryv[14].min] {
                                    for pers in [0u16, tryv[14].personality] {
                                        tryv[14] = crate::library::Advert { min, delta, personality: pers };
                                        if interaction_score(&curves, Some(&tryv), person, &person.motives) == log.scores[self.score_at].score {
                                            found.push((min, delta, pers));
                                        }
                                    }
                                }
                            }
                            l += &format!("; motive 14 that gives it: {found:?}; the person's motive 14 {}", person.motives.get(14).copied().unwrap_or(0.0));
                            // the log's motive-14 entry read as (personality, min, delta)
                            let lg = log.scores[self.score_at].adverts[14];
                            tryv[14] = crate::library::Advert { min: lg[1], delta: lg[2], personality: lg[0] as u16 };
                            let turned = interaction_score(&curves, Some(&tryv), person, &person.motives);
                            l += &format!("; with its entry as (personality, min, delta): {}", if turned == log.scores[self.score_at].score { "the log's" } else { "not" });
                            println!("{l}");
                        }
                    }
                    self.score_at += 1;
                }
                // a person's place as logged at the tick (the walk is not native yet; the save's
                // place is where it stood before it walked): the chooser's and a person
                // candidate's, for the distance in the value
                let placed = |o: &crate::world::Object| {
                    let mut o = o.clone();
                    if o.person
                        && let Some(p) = log.places.iter().rfind(|r| r.id == o.id() && r.tick <= s.tick)
                    {
                        (o.x16, o.y16) = (p.x, p.y);
                    }
                    o
                };
                let (chooser, object) = (placed(person), placed(self.world.get(cand.object)));
                values.push((candidate_value(lib, &curves, &chooser, &object, cand, &adverts, base), ci));
            }
            if has("REPLAY_SHOW_SCORES") {
                // the tick's logged scores that no native candidate's matches
                let natives: Vec<f32> = values_scores.clone();
                let logged: Vec<f32> = log.scores.iter().filter(|x| x.tick == s.tick).map(|x| x.score).collect();
                let order: Vec<String> = natives.iter().enumerate().map(|(i, v)| {
                    let at = logged.iter().position(|x| x == v).map_or("-".to_string(), |p| p.to_string());
                    let c = &cands[values[i].1];
                    format!("{}/{}:{at}", c.id, c.interaction(lib).string_index)
                }).collect();
                println!("score order tick {}: native candidate:logged position {}", s.tick, order.join(" "));
                for (i, x) in log.scores.iter().filter(|x| x.tick == s.tick).enumerate() {
                    if !natives.contains(&x.score) {
                        println!("logged score {i} of tick {} with no native candidate's: {:e} (motive 14's entry {:?})", s.tick, x.score, x.adverts[14]);
                    }
                }
            }
            self.counts.add("autonomy: a candidate's score the same natively (its own adverts, the native motives)", scores_same);
            if scores_diff != 0 {
                self.counts.add("autonomy: a candidate's score DIFFERENT", scores_diff);
            }
            // The list and the choice natively (autonomy_list, autonomy_pick), against the
            // original's sorted list ('Y', the tick's last) and with its random number ('R').
            {
                let vs: Vec<Valued> = values.iter().map(|&(value, candidate)| Valued { value, candidate }).collect();
                let list = autonomy_list(lib, vs, false);
                let logged_list = log.choice_lists.iter().rfind(|l| l.tick == s.tick);
                let rnd = log.randoms.iter().rfind(|x| x.tick == s.tick && x.site <= 1);
                // The list's interaction number is the interaction's string index.
                let index_of = |c: &Candidate| c.interaction(lib).string_index;
                if let Some(ll) = logged_list {
                    let mut same = ll.entries.len() == list.len();
                    let (mut flags_same, mut flags_diff) = (0, 0);
                    for (i, e) in ll.entries.iter().enumerate().take(list.len()) {
                        let c = &cands[list[i].candidate];
                        same = same && e.id == c.id && e.interaction as i32 == index_of(c) && e.value == list[i].value;
                        if e.takes_best != 0 {
                            flags_diff += 1;
                        } else {
                            flags_same += 1;
                        }
                    }
                    self.counts.note(if same {
                        "autonomy: the choice's list the same natively (objects, interactions, values bit for bit)"
                    } else {
                        "autonomy: the choice's list DIFFERENT"
                    });
                    self.counts.add("autonomy: the list's entries without the takes-best flag", flags_same);
                    if flags_diff != 0 {
                        self.counts.add("autonomy: the list's entries WITH the takes-best flag (not built)", flags_diff);
                    }
                    if !same && has("REPLAY_SHOW_AUTONOMY") {
                        println!("list at tick {}: log {}, native {}", s.tick, ll.entries.len(), list.len());
                        for i in 0..list.len().max(ll.entries.len()) {
                            let mut l = match ll.entries.get(i) {
                                Some(e) => format!("  log {}/{}={}", e.id, e.interaction, g(e.value as f64, 9)),
                                None => "  log -".to_string(),
                            };
                            if i < list.len() && i < ll.entries.len() && has("REPLAY_SHOW_DISTANCE") {
                                let c = &cands[list[i].candidate];
                                let e = c.interaction(lib);
                                let o = self.world.get(c.object);
                                let sc = interaction_score(&curves, Some(&e.adverts), person, &person.motives);
                                let att = attenuation(lib, e, false);
                                let implied = if att > 0.0 { ((sc - base) / ll.entries[i].value - 1.0) / att } else { -1.0 };
                                l += &format!(
                                    " [d {:.4} implied {:.4} att {:.2} me {},{},{} obj {},{},{}]",
                                    person_distance(person, o),
                                    implied,
                                    att,
                                    person.x16,
                                    person.y16,
                                    person.level,
                                    o.x16,
                                    o.y16,
                                    o.level
                                );
                            }
                            match list.get(i) {
                                Some(v) => {
                                    let c = &cands[v.candidate];
                                    let e = c.interaction(lib);
                                    l += &format!(
                                        "   native {}/{}(s{},f{:x})={}",
                                        c.id,
                                        index_of(c),
                                        e.string_index,
                                        e.flags,
                                        g(v.value as f64, 9)
                                    );
                                }
                                None => l += "   native -",
                            }
                            println!("{l}");
                        }
                    }
                }
                if let Some(rnd) = rnd {
                    let at = autonomy_pick(lib, &list, rnd.value);
                    let ok = at.is_some_and(|a| cands[list[a].candidate].id == s.stack_object_after);
                    self.counts.note(if ok {
                        "autonomy: the native choice (the original's random number) the log's"
                    } else {
                        "autonomy: the native choice (the original's random number) NOT the log's"
                    });
                    if has("REPLAY_SHOW_AUTONOMY") {
                        println!(
                            "pick at tick {}: random {}, index {}, native {}, log {}",
                            s.tick,
                            rnd.value,
                            at.map_or(-1, |a| a as i64),
                            at.map_or(-1, |a| cands[list[a].candidate].id as i64),
                            s.stack_object_after
                        );
                    }
                    if ok {
                        let c = &cands[list[at.unwrap()].candidate];
                        native_choice = Some(c.id);
                        // what GosubFoundAction (opcode 30) runs: the interaction's action, in
                        // the chosen object's file
                        self.chosen = lib.tree(c.object_file, c.interaction(lib).action).map(|t| (c.id, t, c.object_file));
                    }
                }
            }
            values.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let rank = values.iter().position(|v| cands[v.1].id == s.stack_object_after).map_or(0, |p| p + 1);
            if values.is_empty() {
                // nothing passed (the club's evaluations: no guard runs there): no choice, the
                // stack object as it was
                let kept = interp.top().stack_object;
                native_choice = Some(kept);
                self.counts.note(if kept == s.stack_object_after {
                    "autonomy: no interaction passed, the stack object kept, as the log's"
                } else {
                    "autonomy: no interaction passed, the stack object kept, DIFFERENT"
                });
            } else {
            self.counts.note(if (1..=4).contains(&rank) {
                "autonomy: the log's choice among the native best four"
            } else {
                "autonomy: the log's choice NOT among the native best four"
            });
            if has("REPLAY_SHOW_AUTONOMY") {
                let mut l = format!("choice of {me_id}: object {}, native rank {rank} of {}; best:", s.stack_object_after, values.len());
                for v in values.iter().take(4) {
                    l += &format!(" {}/{}={}", cands[v.1].id, cands[v.1].interaction(lib).action, g(v.0 as f64, 6));
                }
                println!("{l}");
            }
            }
        }
        self.counts.add("autonomy: an evaluation's checks, result as the log's", same);
        self.counts.add("autonomy: an evaluation's checks, result DIFFERENT", different);
        if unfinished != 0 {
            self.counts.add("autonomy: an evaluation's checks, not finished", unfinished);
        }
        // The choice: the native one, made with the original's random number (the engine's own
        // numbers differ, docs/plan.md); the log's where there is none.
        interp.top().stack_object = native_choice.unwrap_or(s.stack_object_after);
        if native_choice.is_some() {
            self.counts.note("autonomy: an evaluation's result (the stack object) made natively");
        }
        self.logged(s, n)
    }
}
