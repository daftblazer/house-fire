//! The Animate (opcode 44) run studies, against the animations, tracks and speeds.

use crate::*;

pub fn runs(lib: &Library, log: &housefire::tree_log::Log, d: &Disc, step_object: &[Option<FileId>]) {
    let anims = d.read("animatio.arc").unwrap_or_default();
    let quick = d.read("quickdat.arc").unwrap_or_default();
    let by_id: BTreeMap<u32, housefire::anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, housefire::anim::read(housefire::arc::bytes(&anims, e))?))).collect();
    let refs: Vec<u32> = housefire::arc::entries(&quick)
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.name == "SimsObjects")
        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
        .map(|img| img.rows("AnimRef").filter_map(|r| img.u32(r)).collect())
        .unwrap_or_default();
    let _ = &refs;
    let by_name: BTreeMap<String, &housefire::anim::Anim> = by_id.values().map(|a| (a.name.to_ascii_lowercase(), a)).collect();
    // a tree's file (the one whose trees hold it)
    let mut tree_file: BTreeMap<housefire::TreeId, housefire::FileId> = BTreeMap::new();
    for (k, f) in lib.files().iter().enumerate() {
        for t in f.trees.values() {
            tree_file.insert(*t, housefire::FileId(k as u32));
        }
    }
    // an anim table's entry: the file's STR# (129 adult), its index'th string's base name
    let name_in = |f: Option<housefire::FileId>, table: u16, k: usize| -> Option<String> {
        let path = lib.file(f?).strings.get(&table)?.get(k)?;
        let base = path.rsplit(['\\', '/']).next()?;
        Some(base.trim_end_matches(".max").trim_end_matches(".MAX").to_ascii_lowercase())
    };
    let (mut named, mut unnamed, mut by_source) = (0, 0, BTreeMap::<u8, (usize, usize)>::new());
    let exact_names: BTreeSet<String> = by_id.values().map(|a| a.name.clone()).collect();
    let mut case_misses: BTreeMap<String, usize> = BTreeMap::new();
    // the Animate steps whose name is not found as written
    let mut not_found: BTreeSet<usize> = BTreeSet::new();
    // per Sim: (tree, node, frame, first tick, the animation's name)
    let mut open: BTreeMap<u32, (u16, u16, u32, u32, String)> = BTreeMap::new();
    let mut runs: BTreeMap<String, BTreeMap<u32, usize>> = BTreeMap::new();
    // each run: (name, first tick, end tick)
    let mut each: Vec<(String, u32, u32)> = Vec::new();
    // each run's Sim, in the same order
    let mut each_sim: Vec<u32> = Vec::new();
    // each run's first step (its index among the steps)
    let mut first_step: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut operand_of: BTreeMap<(String, u32), [u8; 8]> = BTreeMap::new();
    // every Animate run's start by Sim, its operand flags (named or by a variable)
    let mut start_flags: BTreeMap<(u32, u32), u8> = BTreeMap::new();
    // each run's steps: (tick, step index)
    let mut steps_of: BTreeMap<(String, u32), Vec<(u32, usize)>> = BTreeMap::new();
    let mut ends: BTreeMap<u8, usize> = BTreeMap::new();
    for (i, s) in log.steps.iter().enumerate() {
        let node = step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
        let Some(n) = node.filter(|n| n.opcode == 44) else { continue };
        let index = n.operand16(0) as usize;
        let source = n.operand[4];
        let tree = step_object[i].and_then(|o| lib.tree(Some(o), s.tree));
        let file = match source {
            0 => tree.and_then(|t| tree_file.get(&t).copied()),
            1 => lib.global(),
            2 => step_object[i],
            _ => None,
        };
        let name = if n.operand[5] & 4 != 0 { None } else { name_in(file, 129, index) };
        // the table's name as written: one whose case is not an archive entry's is not found
        // (Global's "A2O-flamingo-kick"; animatio.arc has "a2o-flamingo-kick")
        if n.operand[5] & 4 == 0
            && let Some(raw) = file.and_then(|f| lib.file(f).strings.get(&129)).and_then(|t| t.get(index)).and_then(|p| p.rsplit(['\\', '/']).next().map(|b| b.trim_end_matches(".max").trim_end_matches(".MAX").to_string()))
            && !exact_names.contains(&raw)
        {
            *case_misses.entry(raw).or_default() += 1;
            not_found.insert(i);
        }
        let e = by_source.entry(source).or_default();
        match &name {
            Some(nm) if by_name.contains_key(nm) => {
                named += 1;
                e.0 += 1;
            }
            _ => {
                unnamed += 1;
                e.1 += 1;
            }
        }
        let name = name.unwrap_or_else(|| format!("(source {source} index {index})"));
        let key = (s.tree, s.node, s.frame);
        let start = match open.get(&s.sim) {
            Some((t, nd, f, first, r)) if (*t, *nd, *f) == key && *r == name => *first,
            _ => s.tick,
        };
        if start == s.tick {
            first_step.entry((name.clone(), start)).or_insert(i);
            operand_of.entry((name.clone(), start)).or_insert([n.operand[0], n.operand[1], n.operand[2], n.operand[3], n.operand[4], n.operand[5], n.operand[6], n.operand[7]]);
            start_flags.insert((s.sim, start), n.operand[5]);
        }
        steps_of.entry((name.clone(), start)).or_default().push((s.tick, i));
        if s.result == 1 {
            open.insert(s.sim, (s.tree, s.node, s.frame, start, name));
        } else {
            open.remove(&s.sim);
            *ends.entry(s.result).or_default() += 1;
            each.push((name.clone(), start, s.tick));
            each_sim.push(s.sim);
            *runs.entry(name).or_default().entry(s.tick - start).or_default() += 1;
        }
    }
    let (mut fixed, mut total) = (0, 0);
    let mut fits: BTreeMap<String, usize> = BTreeMap::new();
    let mut rule: BTreeMap<&str, usize> = BTreeMap::new();
    let mut all: BTreeMap<&str, usize> = BTreeMap::new();
    println!("animate steps: named an animation {named}, not {unnamed}; by source (named, not) {by_source:?}");
    for (row, ticks) in &runs {
        let Some(a) = by_name.get(row) else { continue };
        total += 1;
        // the frames the track takes to pass its last frame (the original's track step a
        // frame, fn_8015708C: its place += the frame's time x the animation's rate, the
        // speeds 1), and the ticks those frames span by the frame clock at any phase
        let frames_needed = {
            let (mut pos, mut n) = (0f32, 0u32);
            while pos <= (a.frames as f32 - 1.0) && n < 100_000 {
                pos += housefire::routing::WALK_FRAME * a.rate;
                n += 1;
            }
            n
        };
        let spans: BTreeSet<u32> = (0..42)
            .map(|ph| {
                let mut clock = housefire::routing::FrameClock { ms: ph as f32 };
                let (mut t, mut f) = (0u32, 0u32);
                while f < frames_needed {
                    f += clock.frames_to_tick();
                    t += 1;
                }
                t
            })
            .collect();
        if ticks.len() == 1 {
            let t = *ticks.keys().next().unwrap();
            *rule.entry(if spans.contains(&t) { "in the spans" } else if t < *spans.first().unwrap() { "shorter" } else { "longer" }).or_default() += 1;
            if std::env::var_os("LOCKSTEP_ANIM_RUNS_FIXED").is_some() {
                println!("  fixed {row}: {t} ticks; {frames_needed} frames, spans {spans:?}");
            }
        }
        let longest = *spans.last().unwrap();
        for (t, k) in ticks {
            *all.entry(if *t <= longest { "within" } else { "beyond" }).or_default() += k;
        }
        if ticks.len() == 1 {
            fixed += 1;
            let t = *ticks.keys().next().unwrap();
            *fits.entry(format!("{:.3}", t as f32 / a.frames as f32 * a.rate / 30.0)).or_default() += 1;
        }
        if std::env::var_os("LOCKSTEP_ANIM_RUNS_EACH").is_some() {
            println!("  row {row} {}: {} frames at {}, runs {ticks:?}", a.name, a.frames, a.rate);
        }
    }
    // against the original's tracks ('8'): a track's play (its animation unchanged, its
    // place not going back) from its first step to the one that sets it done; each run's
    // nearest play of its rate starting within two ticks of it
    if !log.tracks.is_empty() {
        // a play: a track's steps from one start frame (+52); (step, first tick, done tick, frames)
        let mut plays: Vec<(f32, u32, u32, u32)> = Vec::new();
        let mut at: BTreeMap<(u32, u32), usize> = BTreeMap::new();
        for t in &log.tracks {
            let k = *at.entry((t.track, t.ran[1])).or_insert_with(|| {
                plays.push((0.0, t.tick, u32::MAX, 0));
                plays.len() - 1
            });
            let step = t.place[1] - t.place[0];
            let p = &mut plays[k];
            if p.2 == u32::MAX {
                if step > 0.0 && p.0 == 0.0 {
                    p.0 = step;
                }
                p.3 += 1;
                if t.done[1] != 0 {
                    p.2 = t.tick;
                }
            }
        }
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        let mut hits: BTreeMap<(i32, i32), usize> = BTreeMap::new();
        let mut starts: BTreeMap<(bool, bool), usize> = BTreeMap::new();
        for (name, first, end) in &each {
            let Some(a) = by_name.get(name) else { continue };
            if end == first {
                continue;
            }
            let want = housefire::routing::WALK_FRAME * a.rate;
            let near = plays
                .iter()
                .filter(|p| (p.0 - want).abs() < 0.01 && p.2 != u32::MAX && p.1 + 2 >= *first && p.1 <= first + 2)
                .min_by_key(|p| (p.1 as i64 - *first as i64).abs());
            match near {
                Some(p) => {
                    *by.entry("found".into()).or_default() += 1;
                    let predicted = ((a.frames as f32 - 1.0) / want).floor() as u32 + 1;
                    if p.3 == predicted && std::env::var_os("LOCKSTEP_ANIM_TRACKS_START").is_some() {
                        // the frames of the run's first tick and the next, against the play's first
                        let of = |tick: u32| log.tracks.iter().filter(|t| t.tick == tick).map(|t| t.frame).collect::<BTreeSet<u32>>();
                        let first_frame = log.tracks.iter().find(|t| t.tick == p.1 && t.place[0] == 0.0).map(|t| t.frame);
                        if let Some(t) = log.tracks.iter().find(|t| t.tick == p.1 && t.place[0] == 0.0) {
                            println!("  anim pointer {name}: {:08x} (tick {})", t.anim, t.tick);
                        }
                        // the first frame whose records come after the run's first step
                        let step_at = first_step.get(&(name.clone(), *first)).copied().unwrap_or(0);
                        let next_frame = log.tracks.iter().find(|t| t.after_steps > step_at).map(|t| t.frame);
                        let first_use = !each.iter().any(|e| e.0 == *name && e.1 < *first);
                        if !log.loaded.is_empty() {
                            let id = by_id.iter().find(|(_, a)| a.name.eq_ignore_ascii_case(name)).map(|e| *e.0);
                            let asks: Vec<String> = log.loaded.iter().filter(|l| Some(l.id) == id && l.tick + 40 >= *first && l.tick <= first + 2).map(|l| format!("t{} {} from {:08x}", l.tick, if l.loaded { "loaded" } else { "NOT" }, l.caller)).collect();
                            let ever_before = log.loaded.iter().filter(|l| Some(l.id) == id && l.tick < *first).count();
                            let reqs: Vec<String> = log.res_asks.iter().filter(|q| Some(q.id) == id && q.tick <= *first).take(4).map(|q| format!("t{} {} from {:08x} -> {:08x}", q.tick, if q.kind == 0 { "async" } else { "getref" }, q.caller, q.result)).collect();
                            println!("  load {name} {first} late {}: asks near {asks:?}; asked before {ever_before}; requests {reqs:?}", next_frame != first_frame);
                        }
                        *by.entry(format!("start: (first use {first_use}, late {})", next_frame != first_frame)).or_default() += 1;
                        *by.entry(if next_frame == first_frame { "first frame: the first after the step".into() } else { format!("first frame: not the first after the step ({next_frame:?} against {first_frame:?})") }).or_default() += 1;
                        let chained = each.iter().any(|e| e.2 == *first);
                        let late = first_frame.is_some_and(|f| !of(*first).contains(&f));
                        *starts.entry((chained, late)).or_default() += 1;
                        println!("  start {name} run {first}..{end} (after a run's end that tick: {chained}): frames of its tick {:?}, of the next {:?}; the play's first frame {first_frame:?} (start frame {:?}); done at tick {}", of(*first), of(first + 1), log.tracks.iter().find(|t| t.tick == p.1).map(|t| t.ran[1]), p.2);
                    }
                    if p.3 == predicted {
                        *by.entry("played whole (the predicted frames)".into()).or_default() += 1;
                        *hits.entry((p.1 as i32 - *first as i32, *end as i32 - p.2 as i32)).or_default() += 1;
                    }
                    if std::env::var_os("LOCKSTEP_ANIM_TRACKS_EACH").is_some() {
                        println!("  run {name} {first}..{end}: track play {}..{} ({} frames, predicted {})", p.1, p.2, p.3, ((a.frames as f32 - 1.0) / want).floor() as u32 + 1);
                    }
                }
                None => *by.entry("no track".into()).or_default() += 1,
            }
        }
        // (newer logs, the track's animation id at its head +4) each Animate run against the
        // plays started with its animation within two ticks of its first step: how many, how
        // late, and whether a Sim's plays are all on one controller
        if log.tracks.iter().any(|t| t.head[1] != 0) {
            let mut play_starts: Vec<(u32, u32, u32, u32)> = Vec::new(); // (tick, controller, track, anim id)
            let mut cur: HashMap<u32, (u32, u32)> = HashMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                let key = (t.head[1], t.ran[1]);
                if cur.get(&t.track) != Some(&key) {
                    cur.insert(t.track, key);
                    play_starts.push((t.tick, t.controller, t.track, t.head[1]));
                }
            }
            let mut tally: BTreeMap<String, usize> = BTreeMap::new();
            let mut ctl_of: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            // a play's runs: a run continues the Sim's run of the same animation that ended
            // on its first tick (with an event); the chain's first run starts the play
            let mut track_of: HashMap<usize, u32> = HashMap::new();
            let mut order: Vec<usize> = (0..each.len()).collect();
            order.sort_by_key(|&k| (each[k].1, k));
            for &k in &order {
                let (name, first, end) = &each[k];
                if end == first {
                    continue;
                }
                let Some(id) = by_id.iter().find(|(_, a)| a.name.eq_ignore_ascii_case(name)).map(|e| *e.0) else { continue };
                let prev = order.iter().copied().filter(|&j| j != k && each_sim[j] == each_sim[k] && each[j].0 == *name && each[j].2 == *first && each[j].1 < *first).find(|j| track_of.contains_key(j));
                if let Some(j) = prev {
                    // a continuation: the same track still has its animation at this run's end
                    let tr = track_of[&j];
                    let held = log.tracks.iter().any(|t| t.track == tr && t.head[1] == id && t.tick == *end);
                    *tally.entry(format!("continued play: its track {}", if held { "still plays it at the run's end" } else { "NOT playing it at the end" })).or_default() += 1;
                    track_of.insert(k, tr);
                    continue;
                }
                let near: Vec<_> = play_starts.iter().filter(|p| p.3 == id && p.0 >= *first && p.0 <= first + 2).collect();
                let what = match near.len() {
                    0 => "new play: none started near".to_string(),
                    1 => format!("new play: one started, {} tick(s) after", near[0].0 - first),
                    _ => "new play: several started".to_string(),
                };
                *tally.entry(what).or_default() += 1;
                if near.len() == 1 {
                    track_of.insert(k, near[0].2);
                    ctl_of.entry(each_sim[k]).or_default().insert(near[0].1);
                }
            }
            // every run through its Sim's controller (when known): a track of it with the run's
            // animation during the run, at its first and at its last tick
            let mut by_ctl: BTreeMap<String, usize> = BTreeMap::new();
            for (k, (name, first, end)) in each.iter().enumerate() {
                if end == first {
                    continue;
                }
                let Some(id) = by_id.iter().find(|(_, a)| a.name.eq_ignore_ascii_case(name)).map(|e| *e.0) else { continue };
                let Some(ctl) = ctl_of.get(&each_sim[k]).and_then(|c| (c.len() == 1).then(|| *c.iter().next().unwrap())) else {
                    *by_ctl.entry("Sim's controller unknown".into()).or_default() += 1;
                    continue;
                };
                let on = |tick: u32| log.tracks.iter().any(|t| t.controller == ctl && t.head[1] == id && t.tick == tick);
                let span = (*first..=*end).filter(|&t| on(t)).count() as u32;
                // each tick of the run without its animation on the controller, by reason
                let stepped = |tick: u32| log.tracks.iter().any(|t| t.controller == ctl && t.tick == tick);
                let mut was_done = false;
                for tick in *first..=*end {
                    if on(tick) {
                        was_done = log.tracks.iter().filter(|t| t.controller == ctl && t.head[1] == id && t.tick == tick).any(|t| t.done[1] != 0);
                        continue;
                    }
                    let any_frame = log.tracks.iter().any(|t| t.tick == tick);
                    let why = if !any_frame {
                        "no frame that tick (no track stepped at all)"
                    } else if !stepped(tick) && log.loaded.iter().any(|l| l.tick == tick && !l.loaded) {
                        "the controller waiting on a load missed that tick (others stepped)"
                    } else if !stepped(tick) {
                        "the controller not stepped that tick (others were)"
                    } else if tick == *first && log.loaded.iter().any(|l| l.id == id && l.tick == tick && !l.loaded) {
                        "the first tick, its animation not loaded yet"
                    } else if tick == *end && was_done {
                        "the end tick, the play done the tick before"
                    } else {
                        "UNEXPLAINED"
                    };
                    *by_ctl.entry(format!("  missing tick: {why}")).or_default() += 1;
                }
                if span != end - first + 1 && std::env::var_os("LOCKSTEP_ANIM_RUNS_PARTIAL").is_some() {
                    let marks: String = (first.saturating_sub(3)..=end + 3).map(|t| if t == *first { '[' } else if !on(t) { '.' } else { '#' }).collect();
                    let others: BTreeSet<String> = log.tracks.iter().filter(|t| t.controller == ctl && t.tick == *first).filter_map(|t| by_id.get(&t.head[1]).map(|a| a.name.clone())).collect();
                    let loaded_ask = log.loaded.iter().filter(|l| l.id == id && l.tick + 3 >= *first && l.tick <= *first + 1).map(|l| (l.tick, l.loaded)).collect::<Vec<_>>();
                    println!("  partial {name} {first}..{end}: ticks {}-{}: {marks}; on its controller at first {others:?}; loaded asks {loaded_ask:?}", first.saturating_sub(3), end + 3);
                }
                *by_ctl.entry(format!("first tick {}, last {}, ticks with it {}", on(*first) || on(first + 1), on(*end), if span == end - first + 1 { "all".to_string() } else if span == 0 { "none".to_string() } else { "some".to_string() })).or_default() += 1;
            }
            println!("animate runs on their Sim's controller: {by_ctl:?}");
            // frames a tick: ticks by how many frames (distinct +52 frame numbers) they hold
            let mut fr: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                fr.entry(t.tick).or_default().insert(t.frame);
            }
            let mut per: BTreeMap<usize, usize> = BTreeMap::new();
            for tick in 1..log.ticks {
                *per.entry(fr.get(&tick).map_or(0, |f| f.len())).or_default() += 1;
            }
            println!("ticks by frames in them: {per:?}");
            // a play run natively: each (controller, animation)'s records in order, the place
            // carried by the engine from the play's first frame through `anim::track_step`
            // with the animation's rate and end mode and the logged speeds (+72, the
            // controller's +80); a play starts when its last record was not the frame before
            let mut native: HashMap<(u32, u32, u32), (u32, f32)> = HashMap::new();
            let mut run_tally: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.head[1] != 0) {
                let Some(a) = by_id.get(&t.head[1]) else { continue };
                let key = (t.controller, t.track, t.head[1]);
                if t.tick == 0 {
                    // (loading: the plays' places taken as they are)
                    native.insert(key, (t.frame, t.place[1]));
                    continue;
                }
                let carried = native.get(&key).filter(|(f, _)| f + 1 == t.frame || *f == t.frame).map(|e| e.1);
                let before = match carried {
                    Some(p) => {
                        *run_tally.entry(format!("carried place {}", if p.to_bits() == t.place[0].to_bits() { "the same" } else { "DIFFERENT" })).or_default() += 1;
                        p
                    }
                    None if native.get(&key).is_some_and(|(_, p)| p.to_bits() == t.place[0].to_bits()) => {
                        let gap = t.frame - native[&key].0;
                        *run_tally.entry(format!("resumed after {} frame(s) without a step, place kept", (gap - 1).min(5))).or_default() += 1;
                        t.place[0]
                    }
                    None => {
                        let what = if t.place[0] == 0.0 { "0" } else if t.place[0].to_bits() == (a.frames as f32 - 1.0).to_bits() { "the last frame" } else { "elsewhere" };
                        *run_tally.entry(format!("a play's first frame, place {what}")).or_default() += 1;
                        if what == "elsewhere" && run_tally.get("(shown)").copied().unwrap_or(0) < 6 {
                            *run_tally.entry("(shown)".into()).or_default() += 1;
                            println!("  play first frame elsewhere: tick {} ctl {:x} track {:x} {} place {:?} speeds {:?} known {:?}", t.tick, t.controller, t.track, a.name, t.place, t.speeds, native.get(&key));
                        }
                        t.place[0]
                    }
                };
                if t.ran[0] == t.frame {
                    native.insert(key, (t.frame, before));
                    continue;
                }
                let (after, _) = housefire::anim::track_step(before, 0.033, t.speeds[2], t.speeds[1], a.rate, a.frames as f32 - 1.0, a.end_mode());
                *run_tally.entry(format!("frame step {}", if after.to_bits() == t.place[1].to_bits() { "the same" } else { "DIFFERENT" })).or_default() += 1;
                native.insert(key, (t.frame, after));
            }
            println!("plays run natively from their first frame: {run_tally:?}");
            // the controller's speed (+80) against the animators' speed updates ('#'): which
            // record (animator or owner the controller) and which value
            let mut last_speed: HashMap<u32, (f32, f32)> = HashMap::new();
            let mut si = 0;
            let mut ctl_tally: BTreeMap<String, usize> = BTreeMap::new();
            let mut cand: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                while let Some(sp) = log.speeds.get(si).filter(|sp| sp.tick < t.tick || (sp.tick == t.tick && sp.after_steps <= t.after_steps)) {
                    last_speed.insert(sp.animator, (sp.before, sp.after));
                    last_speed.insert(sp.owner | 1, (sp.before, sp.after));
                    si += 1;
                }
                let c80 = t.speeds[2];
                let fits: BTreeSet<u32> = last_speed.iter().filter(|(k, v)| *k & 1 == 0 && v.1 == c80).map(|(k, _)| *k).collect();
                let e = cand.entry(t.controller).or_insert_with(|| fits.clone());
                *e = e.intersection(&fits).copied().collect();
                *ctl_tally.entry(format!("ctl80 {c80}")).or_default() += 1;
            }
            // per frame: the controllers' +80 values, and the tracks' +72 values
            let mut per_frame: BTreeMap<u32, (BTreeSet<u32>, BTreeSet<u32>)> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                let e = per_frame.entry(t.frame).or_default();
                e.0.insert(t.speeds[2].to_bits());
                e.1.insert(t.speeds[1].to_bits());
            }
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for (c80s, s72s) in per_frame.values() {
                let f = |v: &BTreeSet<u32>| v.iter().map(|b| f32::from_bits(*b)).collect::<Vec<_>>();
                *kinds.entry(format!("ctl80 {:?} / +72 {:?}", f(c80s), f(s72s))).or_default() += 1;
            }
            println!("frames by speed values: {kinds:?}");
            // a controller's +80 over time: its runs of one value (value, frames)
            let mut by_ctl: BTreeMap<u32, Vec<(u32, u32, u32)>> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                let v = by_ctl.entry(t.controller).or_default();
                match v.last_mut() {
                    Some(l) if l.0 == t.speeds[2].to_bits() => l.2 = t.tick,
                    _ => v.push((t.speeds[2].to_bits(), t.tick, t.tick)),
                }
            }
            for (c, v) in by_ctl.iter().take(6) {
                println!("  controller {c:x} +80 spans: {:?}", v.iter().take(10).map(|x| (f32::from_bits(x.0), x.1, x.2)).collect::<Vec<_>>());
            }
            let _ = &cand;
            // the game speed a frame: the objects' controllers' +80 (all the same); the Sims'
            // controllers (ctl_of) hold 1 at +80 and carry it in the track speed +72
            let sims: BTreeSet<u32> = ctl_of.values().flatten().copied().collect();
            let mut game: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0 && !sims.contains(&t.controller)) {
                game.entry(t.frame).or_default().insert(t.speeds[2].to_bits());
            }
            let mut rule: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                let Some(g) = game.get(&t.frame).filter(|g| g.len() == 1).map(|g| f32::from_bits(*g.iter().next().unwrap())) else {
                    *rule.entry("frame without one game speed".into()).or_default() += 1;
                    continue;
                };
                let what = if sims.contains(&t.controller) {
                    format!("Sim: +80 {}, +72 {}", if t.speeds[2] == 1.0 { "1" } else { "OTHER" }, if t.speeds[1] == g { "the game speed" } else if t.speeds[1] == -g { "minus the game speed" } else if t.speeds[1] == 0.0 { "0 (game speed not)" } else { "OTHER" })
                } else {
                    format!("object: +80 the game speed, +72 {}", t.speeds[1])
                };
                *rule.entry(what).or_default() += 1;
            }
            println!("speeds by the game speed: {rule:?}");
            // a Sim's +72 against its animator's last speed ('#', owner the Sim)
            let sim_of: HashMap<u32, u32> = ctl_of.iter().filter(|(_, c)| c.len() == 1).map(|(sim, c)| (*c.iter().next().unwrap(), *sim)).collect();
            let mut last: HashMap<u32, f32> = HashMap::new();
            let mut si = 0;
            let mut sim_rule: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                while let Some(sp) = log.speeds.get(si).filter(|sp| sp.tick < t.tick || (sp.tick == t.tick && sp.after_steps <= t.after_steps)) {
                    // (the animator's owner is the controller's object, 0x350 before it)
                    last.insert(sp.owner.wrapping_add(0x350), sp.after);
                    si += 1;
                }
                if !sim_of.contains_key(&t.controller) {
                    continue;
                }
                let what = match last.get(&t.controller) {
                    None => "no speed record for the Sim yet".to_string(),
                    Some(v) if t.speeds[1].abs() == *v => "|+72| the Sim's animator speed".to_string(),
                    Some(v) => format!("OTHER (+72 {}, animator {v})", t.speeds[1]),
                };
                *sim_rule.entry(what).or_default() += 1;
            }
            println!("Sims' track speeds by their animators: {sim_rule:?}");
            // the zeros: a track held still its whole play, or stopped part of it
            let mut plays_z: HashMap<(u32, u32, u32), (usize, usize)> = HashMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0 && sim_of.contains_key(&t.controller)) {
                let e = plays_z.entry((t.track, t.head[1], t.ran[1])).or_default();
                e.0 += 1;
                if t.speeds[1] == 0.0 {
                    e.1 += 1;
                }
            }
            let mut zt: BTreeMap<String, usize> = BTreeMap::new();
            for (k, (n, z)) in &plays_z {
                if *z == 0 {
                    continue;
                }
                let name = by_id.get(&k.1).map(|a| a.name.clone()).unwrap_or_default();
                *zt.entry(format!("{} ({name})", if z == n { "held its whole play" } else { "stopped part of its play" })).or_default() += z;
            }
            println!("Sims' tracks at +72 0: {zt:?}");
            // the speed sets ('^'): by caller and speed, and each Sim track record's +72 against
            // the last set on its track (or its start at 1 by the constructor)
            let mut sets: BTreeMap<String, usize> = BTreeMap::new();
            for x in &log.speed_sets {
                *sets.entry(format!("caller {:08x} speed {} {} {}", x.caller, x.speed, if x.track == 0 { "no track" } else { "a track" }, if sim_of.contains_key(&x.controller) { "Sim" } else { "object" })).or_default() += 1;
            }
            println!("speed sets: {sets:?}");
            let mut last_set: HashMap<u32, f32> = HashMap::new();
            let mut si = 0;
            let mut by_set: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                while let Some(x) = log.speed_sets.get(si).filter(|x| x.at < t.at) {
                    if x.track != 0 {
                        last_set.insert(x.track, x.speed);
                    }
                    si += 1;
                }
                if !sim_of.contains_key(&t.controller) {
                    continue;
                }
                let what = match last_set.get(&t.track) {
                    None => format!("never set, +72 {}", t.speeds[1]),
                    Some(v) if *v == t.speeds[1] => "+72 the last set's speed".to_string(),
                    Some(v) if t.speeds[1] == 0.0 => format!("+72 0, last set {v}"),
                    Some(_) => "+72 another (the animator's)".to_string(),
                };
                *by_set.entry(what).or_default() += 1;
            }
            println!("Sims' track records by the speed sets: {by_set:?}");
            // the rule: a Sim track's +72 is the latest of its own set ('^') and its controller's
            // animator speed ('#') since the track's play began; a track never set follows the
            // animator, with the play's direction (a backward play the negative)
            let mut ev: HashMap<u32, (usize, f32)> = HashMap::new(); // track -> (at, set speed)
            let mut an: HashMap<u32, (usize, f32)> = HashMap::new(); // controller -> (at, speed)
            let (mut si, mut ai) = (0, 0);
            let mut rule2: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                while let Some(x) = log.speed_sets.get(si).filter(|x| x.at < t.at) {
                    if x.track != 0 {
                        ev.insert(x.track, (x.at, x.speed));
                    }
                    si += 1;
                }
                while let Some(sp) = log.speeds.get(ai).filter(|sp| sp.tick < t.tick || (sp.tick == t.tick && sp.after_steps <= t.after_steps)) {
                    an.insert(sp.owner.wrapping_add(0x350), (sp.tick as usize, sp.after));
                    ai += 1;
                }
                if !sim_of.contains_key(&t.controller) {
                    continue;
                }
                let a = an.get(&t.controller).map(|x| x.1);
                let own = ev.get(&t.track).map(|x| x.1);
                let what = match (own, a) {
                    (Some(o), _) if o == t.speeds[1] => "its own set",
                    (_, Some(a)) if a == t.speeds[1].abs() => if own.is_some() { "the animator's, over an older own set" } else { "the animator's (never set)" },
                    _ => "NEITHER",
                };
                *rule2.entry(what.to_string()).or_default() += 1;
                if what == "NEITHER" && rule2.get("NEITHER").copied() == Some(1) {
                    println!("  neither: tick {} track {:x} +72 {} own {own:?} animator {a:?}", t.tick, t.track, t.speeds[1]);
                }
            }
            println!("Sims' track speeds, own set or animator: {rule2:?}");
            // the script step a set came in: the step logged next (a step is logged after it runs)
            let mut in_step: BTreeMap<String, usize> = BTreeMap::new();
            for x in &log.speed_sets {
                let st = |i: usize| log.steps.get(i).map(|s| format!("tree {} node {}", s.tree, s.node)).unwrap_or_default();
                let _ = st;
                *in_step.entry(format!("from {:08x} speed {} {}", x.caller2, x.speed, if sim_of.contains_key(&x.controller) { "Sim" } else { "object" })).or_default() += 1;
            }
            println!("speed sets by the wrapper's caller: {in_step:?}");
            // each set's track's animation (its next step record) by the call site
            let mut steps_of_track: HashMap<u32, Vec<(usize, u32)>> = HashMap::new();
            for t in &log.tracks {
                steps_of_track.entry(t.track).or_default().push((t.at, t.head[1]));
            }
            let mut by_site: BTreeMap<String, usize> = BTreeMap::new();
            for x in &log.speed_sets {
                let anim = steps_of_track.get(&x.track).and_then(|v| v.iter().find(|e| e.0 > x.at)).and_then(|e| by_id.get(&e.1)).map(|a| a.name.clone()).unwrap_or_else(|| "(no later step)".into());
                *by_site.entry(format!("{:08x} {anim}", x.caller2)).or_default() += 1;
            }
            println!("speed sets by call site and animation: {by_site:?}");
            // a new action's set (fn_800B0ADC): -speed for a backward play, its first step from
            // the last frame
            let mut back: BTreeMap<String, usize> = BTreeMap::new();
            for x in log.speed_sets.iter().filter(|x| x.caller2 == 0x800b_0bb4 || x.caller2 == 0x800b_0be4) {
                let Some(t) = log.tracks.iter().find(|t| t.track == x.track && t.at > x.at) else { continue };
                let Some(a) = by_id.get(&t.head[1]) else { continue };
                let at_end = t.place[0].to_bits() == (a.frames as f32 - 1.0).to_bits();
                *back.entry(format!("{} set {}: first step {}", if x.caller2 == 0x800b_0bb4 { "backward" } else { "forward" }, x.speed, if at_end { "from the last frame" } else if t.place[0] == 0.0 { "from 0" } else { "elsewhere" })).or_default() += 1;
            }
            println!("new actions' sets: {back:?}");
            // the Sims' track speeds from the engine's animator alone (the game speed and
            // each play's start and direction): every Sim track record's +72
            let mut anim_of: HashMap<u32, housefire::anim::SimAnimator> = HashMap::new();
            let mut spd: HashMap<(u32, u32), f32> = HashMap::new(); // (controller, track)
            let mut seen: HashMap<(u32, u32), (u32, u32)> = HashMap::new(); // -> (anim id, last frame)
            let mut model: BTreeMap<String, usize> = BTreeMap::new();
            let mut cur_frame = u32::MAX;
            let mut g_now = 1.0f32;
            let mut hi = 0;
            for t in log.tracks.iter().filter(|t| t.tick != 0) {
                if t.frame != cur_frame {
                    cur_frame = t.frame;
                    if let Some(g) = game.get(&t.frame).filter(|g| g.len() == 1) {
                        g_now = f32::from_bits(*g.iter().next().unwrap());
                    }
                }
                // the animator's speed as the game sets it ('#', in the log's order)
                while let Some(sp) = log.speeds.get(hi).filter(|sp| sp.tick < t.tick || (sp.tick == t.tick && sp.after_steps <= t.after_steps)) {
                    hi += 1;
                    let c = sp.owner.wrapping_add(0x350);
                    let a = anim_of.entry(c).or_insert_with(|| housefire::anim::SimAnimator { speed: sp.before });
                    {
                        if a.follow(sp.after) {
                            // the slot tracks: the Sim's tracks stepped in the last frames, but the face
                            for (k, (id, f)) in &seen {
                                let face = by_id.get(id).is_some_and(|x| x.name == "happyFace");
                                if k.0 == c && !face && *f <= cur_frame {
                                    spd.insert(*k, a.speed);
                                }
                            }
                        }
                    }
                }
                if !sim_of.contains_key(&t.controller) {
                    continue;
                }
                let a = *anim_of.entry(t.controller).or_insert_with(|| housefire::anim::SimAnimator { speed: g_now });
                let key = (t.controller, t.track);
                let Some(info) = by_id.get(&t.head[1]) else { continue };
                let new_play = seen.get(&key).is_none_or(|(id, f)| *id != t.head[1] || f + 3 < t.frame);
                if new_play {
                    // (the Animate's operand: flags bit 2 backward, by the run's start tick)
                    // (the latest Animate run of the animation started at or before the play)
                    // (or, for an Animate naming its animation by a variable, the Sim's latest run)
                    let named = operand_of.range((info.name.clone(), 0)..=(info.name.clone(), t.tick)).next_back().map(|(k, o)| (k.1, o[5]));
                    let sim = sim_of[&t.controller];
                    let by_sim = start_flags.range((sim, t.tick.saturating_sub(2))..=(sim, t.tick)).next_back().map(|(k, f)| (k.1, *f));
                    let latest = match (named, by_sim) {
                        (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
                        (a, b) => a.or(b),
                    };
                    let backward = latest.is_some_and(|(_, f)| f & 2 != 0);
                    let v = if info.name == "happyFace" { housefire::anim::FACE_SPEED } else { a.action_speed(backward) };
                    spd.insert(key, v);
                }
                seen.insert(key, (t.head[1], t.frame));
                let v = spd[&key];
                let near_change = (t.frame.saturating_sub(3)..=t.frame + 3).any(|f| game.get(&f).is_some_and(|g| g.len() != 1 || g.iter().next().is_some_and(|b| f32::from_bits(*b) != g_now)));
                let k = if v == t.speeds[1] {
                    "the same".to_string()
                } else if near_change || t.tick < 3 {
                    "DIFFERENT within 3 frames of a game speed change (or loading)".to_string()
                } else if v == -t.speeds[1] {
                    format!("DIFFERENT direction ({}, an Animate run: {})", info.name, operand_of.range((info.name.clone(), 0)..=(info.name.clone(), t.tick)).next_back().is_some())
                } else {
                    format!("DIFFERENT (engine {v}, log {}, {})", t.speeds[1], info.name)
                };
                *model.entry(k).or_default() += 1;
            }
            println!("Sims' track speeds from the engine's animator: {model:?}");
            // the rotation reads ('!'): how many, their results' lengths (a unit quaternion?),
            // their modes and a few for a look
            {
                let r = &log.rot_streams;
                let mut lens: BTreeMap<String, usize> = BTreeMap::new();
                for x in r {
                    let l = x.result.iter().map(|v| v * v).sum::<f32>().sqrt();
                    *lens.entry(format!("mode {} length {:.3}", x.mode, l)).or_default() += 1;
                }
                let streams: BTreeSet<u32> = r.iter().map(|x| x.stream).collect();
                println!("rotation reads: {} of {} streams, in a track's pose {}; by mode and result length {lens:?}", r.len(), streams.len(), r.iter().filter(|x| x.track.is_some()).count());
                // the window's evaluation (`anim::rotation_window`, t from the window's times)
                let mut exact: BTreeMap<String, usize> = BTreeMap::new();
                for x in r {
                    let (k0, k1) = (x.head[2] as i32 as f32, x.head[3] as i32 as f32);
                    let t = (x.place - k0) / (k1 - k0);
                    let w: [[f32; 4]; 4] = core::array::from_fn(|i| core::array::from_fn(|j| f32::from_bits(x.window[4 * i + j])));
                    let v = if x.mode == 0 { housefire::anim::rotation_window(w, t) } else { housefire::anim::rotation_blend(w, t) };
                    let same = (0..4).all(|i| v[i].to_bits() == x.result[i].to_bits());
                    *exact.entry(format!("mode {}: {}", x.mode, if same { "the same to the bit" } else { "DIFFERENT" })).or_default() += 1;
                }
                println!("  rotation reads from the window alone: {exact:?}");
                for x in r.iter().step_by((r.len() / 4).max(1)).take(4) {
                    println!("  rot read tick {} place {} head {:08x?} mode {} window {:08x?} result {:?}", x.tick, x.place, x.head, x.mode, x.window, x.result);
                }
            }
            // each '#' record's speed from the engine's state: the game speed of the tick (the
            // objects' +80 at the tick's frames, variants below) and 0 on a tick its Sim's
            // Animate load misses (the Sim of the step logged after the miss)
            let mut miss: BTreeSet<(u32, u32)> = BTreeSet::new();
            // (a miss is its Sim's: the step logged next, the Animate that asked)
            for l in log.loaded.iter().filter(|l| !l.loaded) {
                if let Some(st) = log.steps.get(l.after_steps) {
                    miss.insert((st.sim, l.tick));
                }
            }
            let miss_any: BTreeSet<u32> = miss.iter().map(|m| m.1).collect();
            {
                // the dips (a '#' to 0 at game speed 1) against the ticks with any load miss
                let dips: BTreeSet<u32> = log.speeds.iter().filter(|sp| sp.after == 0.0 && sp.before == 1.0).map(|sp| sp.tick).collect();
                let any_miss: BTreeSet<u32> = log.loaded.iter().filter(|l| !l.loaded).map(|l| l.tick).collect();
                let by_run: BTreeSet<u32> = miss.iter().map(|m| m.1).collect();
                for l in log.loaded.iter().filter(|l| !l.loaded && l.tick != 0) {
                    let who = log.steps.get(l.after_steps).map(|s| s.sim);
                    let dipped: Vec<u32> = log.speeds.iter().filter(|sp| sp.tick == l.tick && sp.after == 0.0 && sp.before != 0.0).filter_map(|sp| sim_of.get(&sp.owner.wrapping_add(0x350)).copied()).collect();
                    let other_busy: Vec<_> = sim_of.values().filter(|x| Some(**x) != who).map(|x| (x, log.loaded.iter().any(|m| m.tick == l.tick && !m.loaded && log.steps.get(m.after_steps).map(|s| s.sim) == Some(*x)))).collect();
                    println!("  miss tick {} {} by {who:x?}: dipped {dipped:x?} (other Sim missing too: {other_busy:x?})", l.tick, by_id.get(&l.id).map(|a| a.name.as_str()).unwrap_or("?"));
                }
                // the state fields at each dip and the record before and after it
                for (i, sp) in log.speeds.iter().enumerate().filter(|(_, sp)| sp.after == 0.0 && sp.before != 0.0) {
                    let around: Vec<_> = log.speeds[i.saturating_sub(2)..(i + 3).min(log.speeds.len())].iter().map(|x| (x.tick, x.owner & 0xfffff, x.after, &x.state[1..])).collect();
                    println!("  dip tick {}: {around:x?}", sp.tick);
                }
                // the second path's rule: in mode 0 the speed stops when the pending count is
                // above 0 and the animator's +16 is not 1 and +28 not 5
                let mut rule: BTreeMap<String, usize> = BTreeMap::new();
                for sp in log.speeds.iter().filter(|sp| sp.tick != 0) {
                    let [_, _, mode, pending, a16, a28] = sp.state;
                    let stop = mode == 0 && (pending as i32) > 0 && a16 != 1 && a28 != 5;
                    let dip = sp.after == 0.0 && sp.before != 0.0 || sp.after == 0.0 && sp.before == 0.0 && stop;
                    let key = format!("mode {mode} pending>0 {} exempt {} -> speed {}", (pending as i32) > 0, a16 == 1 || a28 == 5, if sp.after == 0.0 { "0" } else { "not 0" });
                    let _ = dip;
                    *rule.entry(key).or_default() += 1;
                }
                println!("  speed updates by the second path's inputs: {rule:?}");
                // the pending count against the load misses, and the animator's +16/+28 against
                // what its Sim is playing (the Sim's latest new play before the record)
                let pend: BTreeMap<u32, i32> = log.speeds.iter().filter(|sp| sp.state[3] as i32 > 0).map(|sp| (sp.tick, sp.state[3] as i32)).collect();
                let miss_ticks: BTreeSet<u32> = log.loaded.iter().filter(|l| !l.loaded).map(|l| l.tick).collect();
                println!("  pending > 0 on ticks {:?}; load-miss ticks {:?}", pend.iter().take(20).collect::<Vec<_>>(), miss_ticks);
                let mut st: BTreeMap<String, usize> = BTreeMap::new();
                for sp in log.speeds.iter().filter(|sp| sp.tick != 0) {
                    let c = sp.owner.wrapping_add(0x350);
                    let playing = log.tracks.iter().rev().find(|t| t.controller == c && t.tick <= sp.tick && t.tick + 2 >= sp.tick && t.head[1] != 0 && by_id.get(&t.head[1]).is_some_and(|a| a.name != "happyFace" && !a.name.contains("carry")))
                        .and_then(|t| by_id.get(&t.head[1])).map(|a| { let n = a.name.as_str(); if n.contains("walk") { "walk" } else if n.contains("turn") { "turn" } else if n.contains("idle") || n.contains("standing-loop") || n.contains("sit-loop") { "idle" } else { "action" } }).unwrap_or("none");
                    *st.entry(format!("+16 {} +28 {} playing {playing}", sp.state[4] as i32, sp.state[5] as i32)).or_default() += 1;
                }
                println!("  the animators' +16/+28 by play: {st:?}");
                println!("  dips to 0: {} ticks; with a load miss that tick: {}; with a miss tied to a Sim's run: {}; dip ticks without a miss {:?}", dips.len(), dips.intersection(&any_miss).count(), dips.intersection(&by_run).count(), dips.difference(&any_miss).take(20).collect::<Vec<_>>());
            }
            let mut game_tick: BTreeMap<u32, Vec<f32>> = BTreeMap::new(); // tick -> its frames' game speeds in order
            {
                let mut last_f = u32::MAX;
                // (only the controllers that follow the game speed: some never leave 1)
                let pausable: BTreeSet<u32> = log.tracks.iter().filter(|t| t.speeds[2] != 1.0).map(|t| t.controller).collect();
                for t in log.tracks.iter().filter(|t| t.tick != 0 && !sim_of.contains_key(&t.controller) && pausable.contains(&t.controller)) {
                    if t.frame != last_f {
                        last_f = t.frame;
                        game_tick.entry(t.tick).or_default().push(t.speeds[2]);
                    }
                }
            }
            for variant in ["the tick's first frame", "the tick's last frame", "the last frame, a miss stopping every Sim", "the last frame, pending loads stopping the animators not exempt"] {
                let mut tally: BTreeMap<String, usize> = BTreeMap::new();
                let mut shown = 0;
                for sp in log.speeds.iter().filter(|sp| sp.tick != 0) {
                    let c = sp.owner.wrapping_add(0x350);
                    let Some(sim) = sim_of.get(&c) else { continue };
                    let g = match variant {
                        "the tick's first frame" => game_tick.range(sp.tick..).next().and_then(|e| e.1.first().copied()),
                        "the tick's last frame" => game_tick.range(..=sp.tick).next_back().and_then(|e| e.1.last().copied()),
                        _ => game_tick.range(..=sp.tick).next_back().and_then(|e| e.1.last().copied()),
                    };
                    let Some(g) = g else { continue };
                    let other = sim_of.values().find(|x| *x != sim).copied().unwrap_or(0);
                    let _ = other;
                    let [_, _, _, pending, a16, a28] = sp.state;
                    let missed = match variant {
                        "the last frame, a miss stopping every Sim" => miss_any.contains(&sp.tick),
                        "the last frame, pending loads stopping the animators not exempt" => (pending as i32) > 0 && a16 != 1 && a28 != 5,
                        _ => miss.contains(&(*sim, sp.tick)),
                    };
                    let want = if missed { 0.0 } else { g };
                    let k = if want == sp.after { "the same".to_string() } else { format!("DIFFERENT (engine {want}, log {} -> {}{})", sp.before, sp.after, if miss.contains(&(*sim, sp.tick)) { ", a miss" } else { "" }) };
                    if k != "the same" && variant == "the tick's last frame" && shown < 12 {
                        shown += 1;
                        println!("  mismatch tick {} sim {sim:x} {k}; game speeds that tick {:?}; prev {:?}", sp.tick, game_tick.get(&sp.tick), game_tick.range(..sp.tick).next_back());
                    }
                    *tally.entry(k).or_default() += 1;
                }
                println!("animator speeds from the game speed ({variant}) and load misses: {tally:?}");
            }
            // the rule from the engine's own state: the game speed (the pausable controllers),
            // the load-miss ticks, and routing = no Animate run of the Sim covering the tick
            let mut eng: BTreeMap<String, usize> = BTreeMap::new();
            for sp in log.speeds.iter().filter(|sp| sp.tick != 0) {
                let c = sp.owner.wrapping_add(0x350);
                let Some(sim) = sim_of.get(&c) else { continue };
                let Some(g) = game_tick.range(..=sp.tick).next_back().and_then(|e| e.1.last().copied()) else { continue };
                let in_animate = each.iter().zip(&each_sim).any(|((_, first, end), s)| s == sim && *first <= sp.tick && sp.tick <= *end);
                let routing = !in_animate;
                let want = housefire::anim::SimAnimator::speed_for(g, miss_any.contains(&sp.tick), routing);
                let k = if want == sp.after { "the same".to_string() } else { format!("DIFFERENT (engine {want}, log {}, routing by the log {}, by the engine {routing})", sp.after, sp.state[4] == 1) };
                *eng.entry(k).or_default() += 1;
            }
            println!("  animator speeds from the engine's state: {eng:?}");
            println!("  the animators' speed changes (of {} records): {:?}", log.speeds.len(), log.speeds.iter().filter(|sp| sp.before != sp.after).map(|sp| (sp.tick, sp.owner.wrapping_add(0x350) & 0xfffff, sp.before, sp.after)).collect::<Vec<_>>());
            // the Sims' still tracks against the game speed that frame (the objects' +80)
            let mut still: BTreeMap<String, usize> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.tick != 0 && sim_of.contains_key(&t.controller)) {
                let g = game.get(&t.frame).filter(|g| g.len() == 1).map(|g| f32::from_bits(*g.iter().next().unwrap()));
                let name = by_id.get(&t.head[1]).map(|a| a.name.as_str()).unwrap_or("");
                let k = match (t.speeds[1] == 0.0, g) {
                    (true, Some(0.0)) => "still, the game paused".to_string(),
                    (true, Some(g)) => format!("still at game speed {g} ({name})"),
                    (true, None) => format!("still, no game speed that frame ({name})"),
                    (false, Some(0.0)) => "moving while paused".to_string(),
                    (false, _) => "moving".to_string(),
                };
                *still.entry(k).or_default() += 1;
            }
            println!("Sims' tracks by the game speed: {still:?}");
            let pairs: BTreeSet<(u32, u32)> = log.speeds.iter().map(|sp| (sp.animator, sp.owner)).collect();
            println!("  speed records' (animator, owner): {:x?}; Sims {:x?}", pairs.iter().take(8).collect::<Vec<_>>(), sim_of);
            println!("controller speeds: {ctl_tally:?}");
            let one = ctl_of.values().filter(|c| c.len() == 1).count();
            println!("animate runs to their tracks (by animation id): {tally:?}; Sims on one controller {one} of {}", ctl_of.len());
        }
        if std::env::var_os("LOCKSTEP_ANIM_SPEEDS").is_some() {
            // (the tracks' +72 speed by tick: its spans per value, all tracks)
            let mut spans: Vec<(u32, u32, String)> = Vec::new();
            let mut by_tick: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
            for t in &log.tracks {
                if t.speeds[1] != 0.0 {
                    by_tick.entry(t.tick).or_default().insert(format!("{}", t.speeds[1]));
                }
            }
            for (tick, v) in by_tick {
                let v = v.into_iter().collect::<Vec<_>>().join(",");
                match spans.last_mut() {
                    Some(l) if l.2 == v => l.1 = tick,
                    _ => spans.push((tick, tick, v)),
                }
            }
            for (a, b, v) in spans {
                println!("  speeds {a}..{b}: {v}");
            }
            // the primitive steps (not 0/2/8/31) of the ticks LOCKSTEP_ANIM_SPEEDS names
            let want: Vec<u32> = std::env::var("LOCKSTEP_ANIM_SPEEDS").unwrap_or_default().split(',').filter_map(|t| t.parse().ok()).collect();
            for (i, st) in log.steps.iter().enumerate() {
                if !want.contains(&st.tick) {
                    continue;
                }
                let Some(n) = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|t| lib.tree_at(t).nodes.get(st.node as usize)).copied() else { continue };
                if n.opcode < 256 && !matches!(n.opcode, 0 | 2 | 8 | 31) {
                    println!("  step tick {} sim {:x} tree {:#x} node {} of {:?}: opcode {} operand {:02x?} result {} locals {:?}", st.tick, st.sim, st.tree, st.node, step_object[i].map(|f| &lib.file(f).path), n.opcode, n.operand, st.result, &st.locals_after[..st.locals_after.len().min(3)]);
                }
            }
        }
        if std::env::var_os("LOCKSTEP_ANIM_SPEEDREC").is_some() {
            // (the animators' speed updates, '#': the speeds seen; each tick with a speed other
            // than 1, its updates in order with that tick's load asks among them)
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            for s in &log.speeds {
                *seen.entry(format!("{}", s.after)).or_default() += 1;
            }
            println!("  log: {} ticks, {} steps, {} speed updates {seen:?}", log.ticks, log.steps.len(), log.speeds.len());
            // the track frames of the ticks at the speed spans' edges
            let frame_of = |t: u32| log.tracks.iter().filter(|r| r.tick == t).map(|r| r.frame).collect::<BTreeSet<_>>();
            for t in [1u32, 2, 22, 23, 84, 85, 425, 426, 563, 567, 2047, 2048, 4911, 4912] {
                println!("    tick {t}: track frames {:?}", frame_of(t));
            }
            // each animator's spans of one speed (first tick..last tick: speed, updates)
            let mut spans: BTreeMap<u32, Vec<(u32, u32, String, usize)>> = BTreeMap::new();
            for s in &log.speeds {
                let v = spans.entry(s.animator).or_default();
                let sp = format!("{}", s.after);
                match v.last_mut() {
                    Some(l) if l.2 == sp => {
                        l.1 = s.tick;
                        l.3 += 1;
                    }
                    _ => v.push((s.tick, s.tick, sp, 1)),
                }
            }
            for (a, v) in &spans {
                println!("  animator {a:08x} (owner {:08x}): {}", log.speeds.iter().find(|s| s.animator == *a).map_or(0, |s| s.owner), v.iter().map(|e| format!("{}..{}:{}({})", e.0, e.1, e.2, e.3)).collect::<Vec<_>>().join(" "));
            }
            let odd: BTreeSet<u32> = log.speeds.iter().filter(|s| s.after != 1.0).map(|s| s.tick).collect();
            let want: Vec<u32> = std::env::var("LOCKSTEP_ANIM_SPEEDREC_TICKS").unwrap_or_default().split(',').filter_map(|t| t.parse().ok()).collect();
            for &t in odd.iter().filter(|t| want.iter().any(|w| *w == **t || *w + 1 == **t || **t + 1 == *w)) {
                let mut line: Vec<(usize, String)> = log.speeds.iter().filter(|s| s.tick == t).map(|s| (s.after_steps, format!("{:x}:{}", s.animator & 0xffff, s.after))).collect();
                line.extend(log.loaded.iter().filter(|l| l.tick == t && l.caller == 0x800A_C7E4).map(|l| (l.after_steps, format!("ask {:08x} {}", l.id, l.loaded))));
                // the frames' track steps as markers
                let mut fr: BTreeMap<u32, usize> = BTreeMap::new();
                for tr in log.tracks.iter().filter(|r| r.tick == t) {
                    fr.entry(tr.frame).or_insert(tr.after_steps);
                }
                line.extend(fr.iter().map(|(f, a)| (*a, format!("[frame {f}]"))));
                line.sort_by_key(|e| e.0);
                println!("  tick {t}: {}", line.iter().map(|e| e.1.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if std::env::var_os("LOCKSTEP_ANIM_EVENTS").is_some() {
            // (the event messenger's CallNamedTree steps: tick, string index; against the
            // speed spans)
            for (i, st) in log.steps.iter().enumerate() {
                let Some(f) = step_object[i] else { continue };
                if !lib.file(f).path.ends_with("event_messenger.iff") {
                    continue;
                }
                let Some(n) = lib.tree(Some(f), st.tree).and_then(|tr| lib.tree_at(tr).nodes.get(st.node as usize)).copied() else { continue };
                if n.opcode == 28 {
                    println!("  event tick {} tree {:#x}: string {} result {}", st.tick, st.tree, n.operand[4], st.result);
                }
            }
        }
        if std::env::var_os("LOCKSTEP_ANIM_EVENTS").is_some() {
            for (a, b) in [(1000u32, 2047u32), (2048, 4911), (4912, 5629), (5630, 6839), (23, 84)] {
                let n = log.steps.iter().filter(|s| s.tick >= a && s.tick <= b).count();
                let sims: BTreeSet<u32> = log.steps.iter().filter(|s| s.tick >= a && s.tick <= b).map(|s| s.sim).collect();
                println!("  ticks {a}..{b}: {n} steps ({:.1} a tick), {} running", n as f64 / (b - a + 1) as f64, sims.len());
            }
        }
        if std::env::var_os("LOCKSTEP_ANIM_BY_OPERAND").is_some() {
            // (Animate steps by operand source and index: results, and whether the Sim's next
            // step comes the same tick)
            let mut t: BTreeMap<(u8, u16, u8), (usize, usize, usize)> = BTreeMap::new();
            for (i, st) in log.steps.iter().enumerate() {
                let Some(n) = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|tr| lib.tree_at(tr).nodes.get(st.node as usize)).copied() else { continue };
                if n.opcode != 44 || n.operand[4] != 1 {
                    continue;
                }
                let e = t.entry((n.operand[4], u16::from_le_bytes([n.operand[0], n.operand[1]]), st.result)).or_default();
                e.0 += 1;
                let next = log.steps[i + 1..].iter().find(|x| x.sim == st.sim);
                if next.is_some_and(|x| x.tick == st.tick) { e.1 += 1 } else { e.2 += 1 }
            }
            // the load asks ('9', '0') on the ticks of those steps
            for (i, st) in log.steps.iter().enumerate() {
                let Some(n) = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|tr| lib.tree_at(tr).nodes.get(st.node as usize)).copied() else { continue };
                if n.opcode == 44 && n.operand[4] == 1 {
                    let asks: Vec<String> = log.loaded.iter().filter(|l| l.after_steps == i + 1 || l.after_steps == i).map(|l| format!("{:08x} {} from {:08x}", l.id, l.loaded, l.caller)).collect();
                    let reqs: Vec<String> = log.res_asks.iter().filter(|q| q.after_steps == i + 1 || q.after_steps == i).map(|q| format!("{} {:08x} from {:08x}", q.kind, q.id, q.caller)).collect();
                    println!("  global-table Animate at tick {}: asks {asks:?}, requests {reqs:?}", st.tick);
                }
            }
            for ((src, idx, res), (n, same, later)) in &t {
                println!("  animate source {src} index {idx} result {res}: {n} (next step the same tick {same}, later {later})");
            }
        }
        if std::env::var_os("LOCKSTEP_ANIM_PAUSE_RULE").is_some() && !log.speeds.is_empty() {
            // (each short pause, a Sim's speed 0 on a tick outside the long spans, against that
            // tick's load misses and the Sims with Animate steps that tick, before and after
            // the miss; the Sim of an animator is the logged Sim just below its owner)
            let sims: BTreeSet<u32> = log.steps.iter().map(|s| s.sim).collect();
            let sim_of = |owner: u32| sims.range(..owner).next_back().copied().filter(|s| owner - s < 0x1000).unwrap_or(0);
            let mut zero: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            let mut any: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for s in &log.speeds {
                any.entry(s.tick).or_default().insert(sim_of(s.owner));
                if s.after == 0.0 {
                    zero.entry(s.tick).or_default().insert(sim_of(s.owner));
                }
            }
            let short: BTreeSet<u32> = zero.iter().filter(|(t, z)| any.get(t).is_some_and(|a| z.len() < a.len() || (!zero.contains_key(&(*t - 1)) && !zero.contains_key(&(*t + 1))))).map(|e| *e.0).collect();
            let mut tally: BTreeMap<String, usize> = BTreeMap::new();
            for t in short.iter().copied().chain(log.loaded.iter().filter(|l| l.caller == 0x800A_C7E4 && !l.loaded).map(|l| l.tick)).collect::<BTreeSet<_>>() {
                let miss: Vec<&_> = log.loaded.iter().filter(|l| l.tick == t && l.caller == 0x800A_C7E4 && !l.loaded).collect();
                let first_miss = miss.first().map(|l| l.after_steps);
                let mut animate: BTreeMap<u32, (bool, bool)> = BTreeMap::new();
                for (i, st) in log.steps.iter().enumerate().filter(|(_, st)| st.tick == t) {
                    let node = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|tr| lib.tree_at(tr).nodes.get(st.node as usize)).copied();
                    if node.is_some_and(|n| n.opcode == 44) {
                        let e = animate.entry(st.sim).or_default();
                        if first_miss.is_some_and(|m| i < m) { e.0 = true } else { e.1 = true }
                    }
                }
                let paused = zero.get(&t).cloned().unwrap_or_default();
                let askers: BTreeSet<u32> = miss.iter().filter_map(|l| log.steps.get(l.after_steps.saturating_sub(1)).map(|s| s.sim)).collect();
                // prediction: the asker, and every Sim with an Animate step that tick
                let predicted: BTreeSet<u32> = askers.iter().copied().chain(animate.keys().copied()).filter(|s| any.get(&t).is_some_and(|a| a.contains(s))).collect();
                let key = if predicted == paused { "the asker and the tick's Animates: native" } else { "NOT native" };
                *tally.entry(key.into()).or_default() += 1;
                if predicted != paused {
                    println!("  tick {t}: paused {paused:x?}, misses {}, askers {askers:x?}, Animate steps (before, after the miss) {animate:x?}, updated {:x?}", miss.len(), any.get(&t));
                }
            }
            println!("  pause rule: {tally:?}");
        }
        if std::env::var_os("LOCKSTEP_ANIM_PAUSES").is_some() {
            // (each controller's paused frames: all its tracks with a rate at speed 0 on a
            // frame while some had moved the frame before; with that tick's load asks)
            let mut by_frame: BTreeMap<(u32, u32), (u32, usize, usize)> = BTreeMap::new();
            for t in log.tracks.iter().filter(|t| t.speeds[0] != 0.0 && t.anim != 0x7e04_f99c) {
                let e = by_frame.entry((t.controller, t.frame)).or_insert((t.tick, 0, 0));
                e.1 += 1;
                if t.speeds[1] == 0.0 {
                    e.2 += 1;
                }
            }
            let mut was: BTreeMap<u32, bool> = BTreeMap::new();
            for (&(ctl, frame), &(tick, n, zero)) in &by_frame {
                let paused = zero == n;
                if paused && was.get(&ctl) == Some(&false) {
                    let asks: Vec<String> = log.loaded.iter().filter(|l| l.tick + 1 >= tick && l.tick <= tick).map(|l| format!("t{} {:08x} {} from {:08x}", l.tick, l.id, l.loaded, l.caller)).collect();
                    let reqs: Vec<String> = log.res_asks.iter().filter(|q| q.tick + 1 >= tick && q.tick <= tick).map(|q| format!("t{} {} {:08x} from {:08x}", q.tick, q.kind, q.id, q.caller)).collect();
                    println!("  pause: controller {ctl:08x} frame {frame} tick {tick} ({n} tracks); asks {asks:?}; requests {reqs:?}");
                }
                was.insert(ctl, paused);
            }
            // the Animate load asks that miss (800AC7E4, false), with and without a pause that tick
            let pause_ticks: BTreeSet<u32> = {
                let mut w: BTreeMap<u32, bool> = BTreeMap::new();
                let mut t = BTreeSet::new();
                for (&(ctl, _), &(tick, n, zero)) in &by_frame {
                    if zero == n && w.get(&ctl) == Some(&false) {
                        t.insert(tick);
                    }
                    w.insert(ctl, zero == n);
                }
                t
            };
            let misses: BTreeSet<u32> = log.loaded.iter().filter(|l| l.caller == 0x800A_C7E4 && !l.loaded).map(|l| l.tick).collect();
            println!("  pauses: {} ticks; load misses {} ticks, {} of them with a pause; pauses without a miss {:?}", pause_ticks.len(), misses.len(), misses.intersection(&pause_ticks).count(), pause_ticks.difference(&misses).collect::<Vec<_>>());
            println!("  misses without a pause (with that tick's track records): {:?}", misses.difference(&pause_ticks).map(|t| (*t, log.tracks.iter().filter(|r| r.tick == *t).count(), log.tracks.iter().filter(|r| r.tick == *t + 1).count())).collect::<Vec<_>>());
        }
        if let Ok(want) = std::env::var("LOCKSTEP_ANIM_TRACK_STEPS") {
            // (a track's steps whose place moves other than its usual step: the tracks
            // set done at tick LOCKSTEP_ANIM_TRACK_STEPS)
            let at: u32 = want.parse().unwrap_or(0);
            for tr in log.tracks.iter().filter(|t| t.tick == at && t.done[0] == 0 && t.done[1] != 0).map(|t| t.track).collect::<BTreeSet<_>>() {
                let steps: Vec<_> = log.tracks.iter().filter(|t| t.track == tr && t.tick + 200 > at && t.tick <= at).collect();
                let usual = steps.iter().map(|t| ((t.place[1] - t.place[0]) * 100.0).round() as i32).fold(BTreeMap::<i32, usize>::new(), |mut m, d| { *m.entry(d).or_default() += 1; m });
                println!("  track {tr:08x}: steps x100 {usual:?}");
                for t in steps.iter().filter(|t| ((t.place[1] - t.place[0]) * 100.0).round() as i32 != usual.iter().max_by_key(|e| e.1).map_or(0, |e| *e.0)) {
                    let same: Vec<_> = log.tracks.iter().filter(|u| u.frame == t.frame && u.track != t.track).collect();
                    let moved = same.iter().filter(|u| u.place[1] != u.place[0]).count();
                    let paused = same.iter().filter(|u| u.speeds[1] == 0.0).count();
                    println!("    tick {} frame {} place {:.3}->{:.3} speeds {:?} ran {:?} (that frame's other tracks: {} moved, {} at speed 0, of {}; its controller's: {:?})", t.tick, t.frame, t.place[0], t.place[1], t.speeds, t.ran, moved, paused, same.len(), same.iter().filter(|u| u.controller == t.controller).map(|u| (u.track, u.speeds[1])).collect::<Vec<_>>());
                }
            }
        }
        if let Ok(want) = std::env::var("LOCKSTEP_ANIM_TRACKS_NEAR") {
            for (name, first, end) in each.iter().filter(|e| e.0 == want) {
                println!("  {name} {first}..{end}:");
                for t in log.tracks.iter().filter(|t| t.tick + 3 >= *first && t.tick <= first + 3) {
                    println!("    tick {} ctl {:08x} track {:08x} anim {:08x} place {:.3}->{:.3} done {}->{} speeds {:?} frame {} ran {:?} head {:08x?}", t.tick, t.controller, t.track, t.anim, t.place[0], t.place[1], t.done[0], t.done[1], t.speeds, t.frame, t.ran, t.head);
                }
            }
        }
        if !starts.is_empty() {
            println!("animate plays' starts: (after a run's end that tick, first frame in the next tick): {starts:?}");
        }
        // natively: the step that finds the animation loaded (`anim::Loads` from the lot's
        // sets), the first frame after it, the frames to the last (the place's step a frame
        // 0.033 x the rate), and the return the tick after that frame's
        if std::env::var_os("LOCKSTEP_ANIM_NATIVE").is_some() {
            let house: u32 = std::env::var("LOCKSTEP_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            let ids: std::collections::HashSet<u32> = by_id.keys().copied().collect();
            let ds = d.read("datasets.arc").unwrap_or_default();
            let sets: Vec<Vec<u8>> = housefire::arc::entries(&ds).unwrap_or_default().iter().filter(|e| e.name == "House1Preload" || e.name == format!("housepre{house:02}")).map(|e| housefire::arc::bytes(&ds, e).to_vec()).collect();
            let refs: Vec<&[u8]> = sets.iter().map(|v| v.as_slice()).collect();
            let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
            // the frames in order: (frame, its tick, the steps before its first record)
            let mut frames: Vec<(u32, u32, usize)> = Vec::new();
            for t in &log.tracks {
                if frames.last().is_none_or(|f| f.0 != t.frame) {
                    frames.push((t.frame, t.tick, t.after_steps));
                }
            }
            let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
            let mut early: BTreeMap<(String, u8, &str, i16), Vec<usize>> = BTreeMap::new();
            let mut runs_in_order: Vec<&(String, u32, u32)> = each.iter().collect();
            runs_in_order.sort_by_key(|e| e.1);
            for (name, first, end) in runs_in_order {
                let (Some(a), Some(steps)) = (by_name.get(name), steps_of.get(&(name.clone(), *first))) else { continue };
                let Some(id) = by_id.iter().find(|(_, x)| x.name == a.name).map(|e| *e.0) else { continue };
                // the first of the run's steps whose ask finds it loaded (one ask a tick)
                let mut seen = BTreeSet::new();
                let Some(&(_, at)) = steps.iter().filter(|(t, _)| seen.insert(*t)).find(|(t, _)| loads.ask(id, *t)) else { continue };
                let want = housefire::routing::WALK_FRAME * a.rate;
                // played backward (operand byte 5's 0x02: the controller's speed -1, from the
                // last frame), the first frame stays put: one frame more
                let backward = operand_of.get(&(name.clone(), *first)).is_some_and(|o| o[5] & 2 != 0);
                let n = ((a.frames as f32 - 1.0) / want).floor() as usize + 1 + usize::from(backward);
                let Some(k) = frames.iter().position(|f| f.2 > at) else { continue };
                let Some(last) = frames.get(k + n - 1) else { continue };
                let predicted = last.1 + 1;
                *tally.entry(match predicted.cmp(end) {
                    std::cmp::Ordering::Equal => "returns on the native tick",
                    std::cmp::Ordering::Greater => "ends before the animation's end (an event or an interruption)",
                    std::cmp::Ordering::Less => "ends after it (NOT native)",
                }).or_default() += 1;
                if predicted > *end {
                    // an early end: the frames it played (from its first to its last step), the
                    // branch, local 0 after it
                    let last_i = steps.last().map(|e| e.1).unwrap_or(0);
                    let played = frames.iter().filter(|f| f.2 > at && f.2 <= last_i).count();
                    let ls = &log.steps[last_i];
                    let node = step_object[last_i].and_then(|o| lib.tree(Some(o), ls.tree)).and_then(|t| lib.tree_at(t).nodes.get(ls.node as usize)).copied();
                    let branch = node.map_or("?", |nd| if ls.after == nd.true_target as u16 { "true" } else if ls.after == nd.false_target as u16 { "false" } else { "other" });
                    let flags = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[5]);
                    early.entry((name.clone(), flags, branch, ls.locals_after.first().copied().unwrap_or(-1))).or_default().push(played);
                }
                if predicted == *end && std::env::var_os("LOCKSTEP_ANIM_NATIVE_EACH").is_some() {
                    println!("  native {name} {first}..{end}; operand {:02x?}", operand_of.get(&(name.clone(), *first)));
                }
                if predicted < *end && std::env::var_os("LOCKSTEP_ANIM_NATIVE_EACH").is_some() {
                    println!("  later {name} {first}..{end}: native {predicted}; operand {:02x?}", operand_of.get(&(name.clone(), *first)));
                }
            }
            println!("animate runs natively (the load, the frames, the tick after): {tally:?}");
            // (LOCKSTEP_ANIM_NOTES: event ends. A run ends on the tick after the frame whose
            // place passes its play's next note (the AnimRef row's
            // notes, ms); the script's next Animate of the same animation from that tick goes
            // on with the same play)
            if std::env::var_os("LOCKSTEP_ANIM_NOTES").is_some() {
                let notes = housefire::arc::entries(&quick)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|e| e.name == "SimsObjects")
                    .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
                    .map(|img| {
                        img.rows("AnimRef")
                            .filter_map(|r| {
                                let id = img.u32(r)?;
                                let p = img.ptr(r + 8)?;
                                let n = img.u32(p - 4)? as usize;
                                Some((id, (0..n.min(64)).filter_map(|k| Some((img.u32(p + 12 * k)?, img.u32(p + 12 * k + 4)?, img.u32(p + 12 * k + 8)? as i32))).collect::<Vec<_>>()))
                            })
                            .collect::<BTreeMap<u32, Vec<(u32, u32, i32)>>>()
                    })
                    .unwrap_or_default();
                let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
                // per Sim: the play (animation, its first frame's index, the notes passed, its end tick)
                let mut play: BTreeMap<u32, (String, usize, usize, u32)> = BTreeMap::new();
                let mut order: Vec<usize> = (0..each.len()).collect();
                order.sort_by_key(|&k| each[k].1);
                let mut t2: BTreeMap<String, usize> = BTreeMap::new();
                // per Sim: the counted events handed out, and whether its play is at its end
                // owing more
                let mut counted: BTreeMap<u32, (usize, bool)> = BTreeMap::new();
                let use_track_speed = std::env::var_os("LOCKSTEP_ANIM_TRACK_SPEED").is_some();
                let use_rule = std::env::var_os("LOCKSTEP_ANIM_RULE_SPEED").is_some();
                // the ticks with an Animate load miss (the engine's `anim::Loads` on the
                // original's asks in order) and the Sims they stop: the asker (the step before
                // the ask) and every Sim with an Animate step that tick
                let mut paused_at: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
                {
                    let mut l2 = housefire::anim::Loads::with_sets(&refs, &ids);
                    let mut animating: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
                    for (i, st) in log.steps.iter().enumerate() {
                        let node = step_object[i].and_then(|o| lib.tree(Some(o), st.tree)).and_then(|tr| lib.tree_at(tr).nodes.get(st.node as usize)).copied();
                        if node.is_some_and(|n| n.opcode == 44) {
                            animating.entry(st.tick).or_default().insert(st.sim);
                        }
                    }
                    for q in log.loaded.iter().filter(|q| q.caller == 0x800A_C7E4) {
                        if !l2.ask(q.id, q.tick) {
                            let e = paused_at.entry(q.tick).or_default();
                            if let Some(st) = log.steps.get(q.after_steps.saturating_sub(1)) {
                                e.insert(st.sim);
                            }
                            e.extend(animating.get(&q.tick).into_iter().flatten().copied());
                        }
                    }
                }
                let mut started: BTreeMap<u32, Vec<(u32, f32)>> = BTreeMap::new();
                let mut track_speed: BTreeMap<(u32, u32), f32> = BTreeMap::new();
                for t in &log.tracks {
                    if track_speed.insert((t.track, t.frame), t.speeds[1].abs()).is_none() && t.ran[1] + 1 >= t.frame && t.ran[1] <= t.frame {
                        started.entry(t.ran[1]).or_default().push((t.track, t.speeds[0]));
                    }
                }
                // the frames' ticks in fast-forward (the tracks stepping at 5: The Sims 2's
                // SAnimator2::Update sets +252 from the game's speed; a frame every ~4 ticks)
                let fast_ticks: BTreeSet<u32> = log.tracks.iter().filter(|t| t.speeds[1].abs() == 5.0).map(|t| t.tick).collect();
                if let (Some(a), Some(b)) = (fast_ticks.first(), fast_ticks.last()) {
                    let ff: BTreeSet<u32> = log.tracks.iter().filter(|t| t.speeds[1].abs() == 5.0).map(|t| t.frame).collect();
                    println!("  fast-forward frames: ticks {a}..{b} ({} frames; track frames {:?}..{:?}, {} of them)", fast_ticks.len(), ff.first(), ff.last(), ff.len());
                }
                let mut no_note: BTreeMap<(u8, &str, u8, i16, String), usize> = BTreeMap::new();
                let mut no_note_by: BTreeMap<(String, String, &str), usize> = BTreeMap::new();
                for k in order {
                    let (name, first, end) = &each[k];
                    let sim = each_sim[k];
                    let (Some(a), Some(steps)) = (by_name.get(name), steps_of.get(&(name.clone(), *first))) else { continue };
                    let Some(id) = by_id.iter().find(|(_, x)| x.name == a.name).map(|e| *e.0) else { continue };
                    let flags = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[5]);
                    let step = housefire::routing::WALK_FRAME * a.rate;
                    // going on with the Sim's play, or a new one
                    // (the play goes on when the Sim's play is of this animation and was left
                    // at a note, not at its end)
                    // (LOCKSTEP_ANIM_SKIP_ONCE: a run that returns on its first tick leaves the
                    // play alone: whether a restart's burst uses up the new play's notes)
                    if end == first && std::env::var_os("LOCKSTEP_ANIM_SKIP_ONCE").is_some() {
                        *t2.entry("at once, left out".to_string()).or_default() += 1;
                        continue;
                    }
                    // an animation not found as written: true at once, no play
                    if first_step.get(&(name.clone(), *first)).is_some_and(|i| not_found.contains(i)) {
                        let key = if end == first { "not found, at once, on the native tick" } else { "not found, at once (NOT native)" };
                        *t2.entry(key.to_string()).or_default() += 1;
                        continue;
                    }
                    let go_on = play.get(&sim).is_some_and(|p| p.0 == *name && p.3 != u32::MAX);
                    // the counted events (notes of value 0..99) the operand's byte 6 asks for
                    let wanted = operand_of.get(&(name.clone(), *first)).map_or(0, |o| o[6] as i8 as i32);
                    if !go_on {
                        counted.insert(sim, (0, false));
                    }
                    let backward = flags & 2 != 0;
                    // a play at its end still owing counted events: one at once to each call,
                    // then true at once
                    if counted.get(&sim).is_some_and(|c| c.1) {
                        let c = counted.get_mut(&sim).unwrap();
                        let key = if end == first { "at the end, at once, on the native tick" } else { "at the end, at once (NOT native)" };
                        *t2.entry(key.to_string()).or_default() += 1;
                        if (c.0 as i32) < wanted {
                            c.0 += 1;
                        } else if let Some(p) = play.get_mut(&sim) {
                            p.3 = u32::MAX;
                        }
                        continue;
                    }
                    let (k0, passed) = if go_on {
                        let p = &play[&sim];
                        (p.1, p.2)
                    } else {
                        let mut seen = BTreeSet::new();
                        let Some(&(_, at)) = steps.iter().filter(|(t, _)| seen.insert(*t)).find(|(t, _)| loads.ask(id, *t)) else { continue };
                        let Some(k0) = frames.iter().position(|f| f.2 > at) else { continue };
                        (k0, 0)
                    };
                    // the script's notes: type 6 (type 1, 5 and the rest are other notes:
                    // the approach's steps, sounds)
                    let list: Vec<(u32, u32, i32)> = notes.get(&id).cloned().unwrap_or_default().into_iter().filter(|n| n.1 == 6).collect();
                    let last = a.frames as f32 - 1.0;
                    // the frame (from the play's first) whose place passes the next note, if
                    // this run waits for notes
                    // (every note ends the run, whatever the flags: iteration 128)
                    // (The Sims 2's SAnimator2::TryAnimate: a note over 99 ends the run; one of
                    // 0..99 only when it is the next counted event, else it is passed over)
                    let mut counter = counted.get(&sim).map_or(0, |c| c.0);
                    let mut idx = passed;
                    let next_idx = loop {
                        let Some(n) = list.get(idx) else { break None };
                        if n.0 as f32 * a.rate / 1000.0 > last {
                            break None;
                        }
                        if n.2 > 99 {
                            break Some(idx);
                        }
                        let expect = if backward { wanted - counter as i32 - 1 } else { counter as i32 };
                        if (0..wanted).contains(&expect) && n.2 == expect {
                            counter += 1;
                            break Some(idx);
                        }
                        idx += 1;
                    };
                    let next = next_idx.map(|k| list[k].0 as f32 * a.rate / 1000.0);
                    let passed_after = next_idx.map_or(list.len(), |k| k + 1);
                    // at the end: a counted event still owed goes to this run (false), the
                    // rest at once to the next calls; none owed, true
                    let mut at_end = false;
                    if next.is_none() && (counter as i32) < wanted {
                        counter += 1;
                        at_end = true;
                    }
                    counted.insert(sim, (counter, at_end));
                    // forward the place runs 0 → last; backward (0x02) last → 0 after a still
                    // first frame
                    let gone = |t: f32| if backward { last - t } else { t };
                    // the frames whose place stays at or below the target, the place stepping
                    // by the step × the game's speed (5 in fast-forward, from the frame before)
                    let target = match next {
                        Some(t) => gone(t),
                        None => last,
                    };
                    // the play's track: one whose start frame (+52) is the play's first frame,
                    // at the animation's rate
                    let play_track = frames.get(k0).and_then(|f| started.get(&(f.0 - 1)).or(started.get(&f.0))).and_then(|v| v.iter().find(|(_, r)| *r == a.rate)).map(|e| e.0);
                    let mut place = 0.0f32;
                    let mut m = 0usize;
                    while frames.get(k0 + m + 1).is_some() {
                        // (the speed the frame before set: the animator's +252, read by the
                        // next frame's track step)
                        let speed = if fast_ticks.contains(&frames[k0 + m].1) { 5.0 } else { 1.0 };
                        // (LOCKSTEP_ANIM_TRACK_SPEED: the play's own track's logged speed at
                        // that frame instead: its Sim's pauses as well)
                        let speed = match play_track.filter(|_| use_track_speed).and_then(|tr| track_speed.get(&(tr, frames[k0 + m].0))) {
                            Some(&v) => v,
                            None => speed,
                        };
                        // (LOCKSTEP_ANIM_RULE_SPEED: the pause by the rule, iteration 136: a
                        // tick with an Animate load miss stops its asker and its Animate Sims)
                        let speed = if use_rule && paused_at.get(&frames[k0 + m].1).is_some_and(|p| p.contains(&sim)) { 0.0 } else { speed };
                        if place + step * speed > target + 1e-3 {
                            break;
                        }
                        place += step * speed;
                        m += 1;
                    }
                    let j = m + 1 + usize::from(backward);
                    let Some(f) = frames.get(k0 + j - 1) else { continue };
                    let predicted = f.1 + 1;
                    let what = if end == first { "at once" } else if next.is_some() { "a note's" } else { "the end's" };
                    let key = match predicted.cmp(end) {
                        std::cmp::Ordering::Equal => format!("{what} end on the native tick"),
                        std::cmp::Ordering::Greater => format!("{what}: ended before (NOT native)"),
                        std::cmp::Ordering::Less => format!("{what}: ended after (NOT native)"),
                    };
                    if predicted != *end && std::env::var_os("LOCKSTEP_ANIM_NOTES_EACH").is_some() {
                        println!("  note end {name} {first}..{end} sim {sim:x} (note {passed}, place {:?}, wanted {wanted}, counted {counter}, flags {flags:02x}): native {predicted}", next);
                    }
                    if predicted > *end && end != first {
                        // an early end with no note due: its flags, branch and local 0, and
                        // whether the next of the Sim's steps (after the run) is in another tree
                        let last_i = steps.last().map(|e| e.1).unwrap_or(0);
                        let ls = &log.steps[last_i];
                        let node = step_object[last_i].and_then(|o| lib.tree(Some(o), ls.tree)).and_then(|t| lib.tree_at(t).nodes.get(ls.node as usize)).copied();
                        let branch = node.map_or("?", |nd| if ls.after == nd.true_target as u16 { "true" } else if ls.after == nd.false_target as u16 { "false" } else { "other" });
                        let next_same_sim = log.steps[last_i + 1..].iter().find(|x| x.sim == sim);
                        let after = next_same_sim.map_or("none".to_string(), |x| if x.frame == ls.frame && x.tree == ls.tree { "the same tree".into() } else if x.frame < ls.frame { "a popped frame".into() } else { format!("frame {} (was {})", x.frame, ls.frame) });
                        *no_note_by.entry((name.clone(), format!("{:02x?}", operand_of.get(&(name.clone(), *first)).copied().unwrap_or_default()), branch)).or_default() += 1;
                        *no_note.entry((flags, branch, ls.result, ls.locals_after.first().copied().unwrap_or(-1), if after.starts_with("frame") { "another frame".to_string() } else { after })).or_default() += 1;
                    }
                    *t2.entry(key).or_default() += 1;
                    play.insert(sim, (name.clone(), k0, passed_after, if next.is_some() || at_end { *end } else { u32::MAX }));
                }
                println!("animate runs with the notes: {t2:?}");
                // (LOCKSTEP_ANIM_ENGINE: the same runs through the engine's `anim::Play`, frame
                // by frame: each Sim's play stepped by the frames before each of its Animate
                // steps, asked at each step; a play goes on with the next run of the same
                // animation until it has given true)
                if std::env::var_os("LOCKSTEP_ANIM_ENGINE").is_some() {
                    let all_notes = housefire::arc::entries(&quick)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|e| e.name == "SimsObjects")
                        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
                        .map(|img| housefire::anim::notes(&img))
                        .unwrap_or_default();
                    // the engine's `anim::Animator`, fed in the game's order: each frame where
                    // it falls among the steps, each run's Animate step once a tick
                    let mut m = housefire::anim::Animator::new(housefire::anim::Loads::with_sets(&refs, &ids), all_notes);
                    let speed = |sim: u32, i: usize| -> f32 {
                        let Some(f) = frames.get(i) else { return 1.0 };
                        if paused_at.get(&f.1).is_some_and(|p| p.contains(&sim)) {
                            0.0
                        } else if fast_ticks.contains(&f.1) {
                            5.0
                        } else {
                            1.0
                        }
                    };
                    // a step's runs: those of its (animation, first tick), in their order (runs
                    // that start on one tick, like owed events at once, share the steps)
                    let mut group: BTreeMap<(String, u32), Vec<usize>> = BTreeMap::new();
                    for (k, (name, first, _)) in each.iter().enumerate() {
                        group.entry((name.clone(), *first)).or_default().push(k);
                    }
                    for g in group.values_mut() {
                        g.sort_by_key(|&k| (each[k].2, k));
                    }
                    let mut step_run: BTreeMap<usize, &Vec<usize>> = BTreeMap::new();
                    for (key, g) in &group {
                        for &(_, idx) in steps_of.get(key).into_iter().flatten() {
                            step_run.entry(idx).or_insert(g);
                        }
                    }
                    let mut predicted: Vec<Option<u32>> = vec![None; each.len()];
                    let mut polled: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); each.len()];
                    let mut fi = 0;
                    for (&idx, g) in &step_run {
                        while frames.get(fi).is_some_and(|f| f.2 <= idx) {
                            m.frame(|sim| speed(sim, fi));
                            fi += 1;
                        }
                        let Some(&k) = g.iter().find(|&&k| predicted[k].is_none()) else { continue };
                        let (name, first, _) = &each[k];
                        let t = log.steps[idx].tick;
                        if predicted[k].is_some() || !polled[k].insert(t) {
                            continue;
                        }
                        if first_step.get(&(name.clone(), *first)).is_some_and(|i| not_found.contains(i)) {
                            // a table name not found as written: true at once, no load ask
                            predicted[k] = Some(t);
                            continue;
                        }
                        let Some(a) = by_name.get(name) else { continue };
                        let Some(id) = by_id.iter().find(|(_, x)| x.name == a.name).map(|e| *e.0) else { continue };
                        let o = operand_of.get(&(name.clone(), *first)).copied().unwrap_or_default();
                        if m.animate(each_sim[k], id, a, o[5] & 2 != 0, o[6] as i8 as i32, t) != housefire::anim::Poll::Pending {
                            predicted[k] = Some(t);
                        }
                    }
                    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
                    for (k, (name, first, end)) in each.iter().enumerate() {
                        // (the runs the study counts: an archive animation with its steps)
                        let Some(a) = by_name.get(name).filter(|_| steps_of.contains_key(&(name.clone(), *first))) else { continue };
                        if !by_id.values().any(|x| x.name == a.name) {
                            continue;
                        }
                        let ok = predicted[k] == Some(*end);
                        *tally.entry(if ok { "native" } else { "NOT native" }).or_default() += 1;
                        if !ok && std::env::var_os("LOCKSTEP_ANIM_NOTES_EACH").is_some() {
                            println!("  engine run {name} {first}..{end} sim {:x}: {:?}", each_sim[k], predicted[k]);
                        }
                    }
                    println!("animate runs through anim::Animator: {tally:?}");
                }
                for (k, v) in &no_note_by {
                    println!("  early, no note due, by: {} {} {}: {v}", k.0, k.1, k.2);
                }
                for (k, v) in &no_note {
                    println!("  early, no note due: flags {:02x}, {}, result {}, local0 {}, then {}: {v}", k.0, k.1, k.2, k.3, k.4);
                }
            }
            if std::env::var_os("LOCKSTEP_ANIM_EARLY").is_some() {
                for ((name, flags, branch, l0), played) in &early {
                    println!("  early {name} flags {flags:02x} {branch} local0 {l0}: frames played {played:?}");
                }
            }
        }
        println!("animate runs against the tracks: {by:?}; (track start - run start, run end - track done): runs {hits:?}");
    }
    println!("animate runs by the frames: fixed runs against the frame clock's spans {rule:?}; all runs against the longest span {all:?}");
    println!("animate names not found as written (case): {case_misses:?}");
    println!("animate runs: {} animations ({fixed} always the same ticks), ends by result {ends:?}; ticks over frames (at 30) of the fixed ones {fits:?}", total);
}
