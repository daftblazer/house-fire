//! The animation studies of the logged scale, translation and stream reads.

use crate::*;

pub fn channels(log: &housefire::tree_log::Log, d: &Disc) {
    let mut tally: BTreeMap<(u8, &str), usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.vec_streams {
        let got = housefire::anim::channel3(v.first, v.last, v.mode, &v.keys, v.place);
        let same = got.iter().zip(&v.result).all(|(a, b)| a.to_bits() == b.to_bits());
        let close = got.iter().zip(&v.result).all(|(a, b)| (a - b).abs() <= 1e-5 * b.abs().max(1.0));
        let what = if same { "the same bits" } else if close { "within 1e-5" } else { "DIFFERENT" };
        *tally.entry((v.mode.min(1), what)).or_default() += 1;
        if what == "DIFFERENT" && shown < 8 {
            shown += 1;
            println!("  channel different: {v:?}: native {got:?}");
        }
    }
    println!("scale and translation reads ({}), by (mode, result): {tally:?}", log.vec_streams.len());
    // P1 from the stream's bits ('&'): key (+8) + 1's values through `anim::key3`
    let mut keys: BTreeMap<&str, usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.vec_streams {
        let Some((h, words)) = v.header else { continue };
        let stride = (h[4] >> 16) as i16 as i32;
        let (w30, w31, w32) = ((h[4] >> 8) as u8 as i8 as i32, h[4] as u8 as i8 as i32, (h[5] >> 24) as u8 as i8 as i32);
        let at = h[1] as i32 + (h[2] as i32 + 1) * stride + w30 + w32;
        let f = |i: usize| f32::from_bits(h[i]);
        let got = housefire::anim::key3(&words, (at & 31) as u32, w31 as u32, [f(7), f(8), f(9)], [f(10), f(11), f(12)]);
        let same = got.iter().zip(&v.keys[9..12]).all(|(a, b)| a.to_bits() == b.to_bits());
        *keys.entry(if same { "the same bits" } else { "DIFFERENT" }).or_default() += 1;
        if !same && shown < 6 {
            shown += 1;
            println!("  key different: header {h:x?} words {words:x?}: native {got:?}, P1 {:?}", &v.keys[9..12]);
        }
    }
    println!("P1 from the stream's bits: {keys:?}");
    // (LOCKSTEP_ANIM_PLACES: a read's place against the tracks stepped the same tick)
    if std::env::var_os("LOCKSTEP_ANIM_PLACES").is_some() {
        let mut by_tick: BTreeMap<u32, Vec<&housefire::tree_log::TrackRec>> = BTreeMap::new();
        for t in &log.tracks {
            by_tick.entry(t.tick).or_default().push(t);
        }
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        let mut shown = 0;
        for v in &log.vec_streams {
            let ts = by_tick.get(&v.tick).map(Vec::as_slice).unwrap_or(&[]);
            let what = if ts.iter().any(|t| t.place[1].to_bits() == v.place.to_bits()) {
                "a track's place after"
            } else if ts.iter().any(|t| t.place[0].to_bits() == v.place.to_bits()) {
                "a track's place before"
            } else if ts.iter().any(|t| (t.place[1] * 0.99).to_bits() == v.place.to_bits() || (t.place[1] * 30.0).to_bits() == v.place.to_bits()) {
                "a track's place after, scaled"
            } else if ts.is_empty() {
                "no track that tick"
            } else {
                "none of the tracks'"
            };
            *tally.entry(what.into()).or_default() += 1;
            if what == "none of the tracks'" && shown < 6 {
                shown += 1;
                println!("  place {} at tick {}: tracks {:?}", v.place, v.tick, ts.iter().map(|t| (t.place, t.speeds, t.frame)).collect::<Vec<_>>());
            }
        }
        println!("read places: {tally:?}");
        // a stream (its address, between header changes) against the track it follows
        let mut life: HashMap<(u32, u32), BTreeSet<u32>> = HashMap::new();
        let mut ambiguous = 0;
        for v in &log.vec_streams {
            let Some((h, _)) = v.header else { continue };
            let ts = by_tick.get(&v.tick).map(Vec::as_slice).unwrap_or(&[]);
            let hits: Vec<u32> = ts.iter().filter(|t| t.place[1].to_bits() == v.place.to_bits()).map(|t| t.track).collect();
            if hits.len() == 1 {
                life.entry((v.stream, h[1])).or_default().insert(hits[0]);
            } else {
                ambiguous += 1;
            }
        }
        // the link: the stream's bit array object (+0) is its track's animation + 88, and the
        // read's place is that track's place after its step the same tick
        let mut link: BTreeMap<&str, usize> = BTreeMap::new();
        for v in &log.vec_streams {
            let Some((h, _)) = v.header else { continue };
            let ts = by_tick.get(&v.tick).map(Vec::as_slice).unwrap_or(&[]);
            let mine: Vec<_> = ts.iter().filter(|t| t.anim.wrapping_add(88) == h[0]).collect();
            let what = match mine.as_slice() {
                [] => "no track with its animation that tick",
                [t] if t.place[1].to_bits() == v.place.to_bits() => "its track's place after",
                [_] => "its track's place DIFFERENT",
                m if m.iter().all(|t| t.place[1].to_bits() == v.place.to_bits()) => "its tracks' (several, one animation) place after",
                _ => "several tracks, places DIFFERENT",
            };
            *link.entry(what).or_default() += 1;
        }
        println!("read places by the stream's own track: {link:?}");
        // with '(' (newer logs): each read inside its track's pose, at the track's place
        let mut posed: BTreeMap<&str, usize> = BTreeMap::new();
        let mut owner: HashMap<(u32, u32), BTreeSet<u32>> = HashMap::new();
        for v in &log.vec_streams {
            let Some((track, tplace)) = v.track else {
                *posed.entry("outside a pose").or_default() += 1;
                continue;
            };
            let ts = by_tick.get(&v.tick).map(Vec::as_slice).unwrap_or(&[]);
            let stepped = ts.iter().rev().find(|t| t.track == track);
            let what = match stepped {
                _ if tplace.to_bits() != v.place.to_bits() => "the place is not its track's",
                Some(t) if t.place[1].to_bits() == v.place.to_bits() => "its track's place after its step this tick",
                Some(t) if t.place[0].to_bits() == v.place.to_bits() => "its track's place before its step this tick (posed first)",
                Some(_) => "its track's place, but not the step's",
                None => "its track's place (track not stepped this tick)",
            };
            *posed.entry(what).or_default() += 1;
            if let Some((h, _)) = v.header {
                owner.entry((v.stream, h[1])).or_default().insert(track);
            }
        }
        println!("reads in their track's pose: {posed:?}; streams with one track {} of {}", owner.values().filter(|t| t.len() == 1).count(), owner.len());
        // a pose against its track's steps: the pattern of steps (S) and poses (P) of the
        // track that tick, and whether the pose's place is the last step's place after
        let mut steps_of: HashMap<(u32, u32), Vec<&housefire::tree_log::TrackRec>> = HashMap::new();
        for t in &log.tracks {
            steps_of.entry((t.tick, t.track)).or_default().push(t);
        }
        let mut pattern: BTreeMap<String, usize> = BTreeMap::new();
        let mut shown = 0;
        for p in &log.poses {
            let steps = steps_of.get(&(p.tick, p.track)).map(Vec::as_slice).unwrap_or(&[]);
            let before: Vec<_> = steps.iter().filter(|t| t.at < p.at).collect();
            let after = steps.len() - before.len();
            let place_ok = match before.last() {
                Some(t) => t.place[1].to_bits() == p.place.to_bits(),
                None => steps.first().is_some_and(|t| t.place[0].to_bits() == p.place.to_bits()),
            };
            let what = format!("{}{} before, {} after: place {}", if p.tick == 0 { "(loading) " } else { "" }, before.len().min(2), after.min(2), if place_ok { "the last step's" } else if steps.is_empty() { "(no step)" } else { "OTHER" });
            if what.ends_with("OTHER") && p.tick != 0 && shown < 6 {
                shown += 1;
                println!("  pose at tick {} place {}: steps {:?}", p.tick, p.place, steps.iter().map(|t| (t.at < p.at, t.place, t.ran, t.done)).collect::<Vec<_>>());
            }
            *pattern.entry(what).or_default() += 1;
        }
        println!("poses against their track's steps ({}): {pattern:?}", log.poses.len());
        let g = 0.033f32;
        // every moved step through `anim::track_advance`, the end included: past the last
        // frame (the animation's +32 frames - 1) a play stops there or wraps back by it
        // (`anim::wrap_back`); G is the frame time (r13-29720, 0.033)
        let mut ends: BTreeMap<String, usize> = BTreeMap::new();
        for t in log.tracks.iter().filter(|t| t.tick != 0 && t.place[1] != t.place[0]) {
            let [s0, s72, c80] = t.speeds;
            let next = housefire::anim::track_advance(t.place[0], g, c80, s72, s0);
            let last = t.head[8] as f32 - 1.0;
            let what = if next.to_bits() == t.place[1].to_bits() {
                "on, exact"
            } else if next > last && t.place[1].to_bits() == last.to_bits() {
                "past the end: stopped at the last frame"
            } else if next > last && t.place[1].to_bits() == housefire::anim::wrap_back(next, last).to_bits() {
                "past the end: wrapped back by the last frame (repeatedly)"
            } else if next < 0.0 && t.place[1].to_bits() == (next + last).to_bits() {
                "before the start: wrapped by the last frame"
            } else if next < 0.0 && t.place[1] == 0.0 {
                "before the start: stopped at 0"
            } else {
                "OTHER"
            };
            *ends.entry(format!("{what} (done {:?})", t.done)).or_default() += 1;
            if what == "OTHER" && ends.get("(shown)").copied().unwrap_or(0) < 6 {
                *ends.entry("(shown)".into()).or_default() += 1;
                println!("  step other: place {:?} speeds {:?} head {:x?} done {:?}: next {next}", t.place, t.speeds, t.head, t.done);
            }
        }
        println!("track steps with their ends: {ends:?}");
        // (newer logs, with '*') every step through `anim::track_step` by its end mode: the
        // place and the done flag (+160)
        let mut whole: BTreeMap<String, usize> = BTreeMap::new();
        let mut prev_done: HashMap<u32, u32> = HashMap::new();
        for t in log.tracks.iter().filter(|t| t.tick != 0) {
            let Some((mode, _)) = t.end else { continue };
            if t.ran[0] == t.frame {
                // already stepped this frame (+52): kept as it is
                let kept = t.place[0].to_bits() == t.place[1].to_bits() && t.done[0] == t.done[1];
                *whole.entry(format!("already run this frame: {}", if kept { "kept" } else { "CHANGED" })).or_default() += 1;
                continue;
            }
            let [s0, s72, c80] = t.speeds;
            let last = t.head[8] as f32 - 1.0;
            let (place, ended) = housefire::anim::track_step(t.place[0], g, c80, s72, s0, last, mode);
            let ran = t.place[0] != t.place[1] || ended || t.done[0] != t.done[1];
            let same_place = place.to_bits() == t.place[1].to_bits();
            let same_done = (t.done[0] != 0 || ended) == (t.done[1] != 0);
            let _ = prev_done.insert(t.track, t.done[1]);
            let what = match (same_place, same_done) {
                (true, true) => "place and done the same",
                (true, false) => "place the same, done DIFFERENT",
                (false, _) if !ran => "not stepped (place kept)",
                _ => "place DIFFERENT",
            };
            *whole.entry(format!("mode {mode}: {what}")).or_default() += 1;
            if what.contains("DIFFERENT") && whole.get("(shown)").copied().unwrap_or(0) < 8 {
                *whole.entry("(shown)".into()).or_default() += 1;
                println!("  track step different: mode {mode} place {:?} speeds {:?} last {last} done {:?}: native ({place}, {ended})", t.place, t.speeds, t.done);
            }
        }
        println!("track steps by end mode: {whole:?}");
        // a play's start: each track's first step (a new animation on it) against the
        // animation in the archive, found by its id (head +4)
        let anims = d.read("animatio.arc").unwrap_or_default();
        let by_id: HashMap<u32, housefire::anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, housefire::anim::read(housefire::arc::bytes(&anims, e))?))).collect();
        let mut seen: HashMap<u32, u32> = HashMap::new();
        let mut starts: BTreeMap<String, usize> = BTreeMap::new();
        for t in log.tracks.iter().filter(|t| t.tick != 0) {
            if seen.insert(t.track, t.anim) == Some(t.anim) {
                continue;
            }
            let Some((mode, flags)) = t.end else { continue };
            let Some(a) = by_id.get(&t.head[1]) else {
                *starts.entry("animation id not in the archive".into()).or_default() += 1;
                continue;
            };
            *starts.entry(format!("frames {}", if a.frames == t.head[8] { "same" } else { "DIFFERENT" })).or_default() += 1;
            *starts.entry(format!("rate (+0) {}", if a.rate == t.speeds[0] { "the animation's" } else { "OTHER" })).or_default() += 1;
            *starts.entry(format!("+72 {}", t.speeds[1])).or_default() += 1;
            *starts.entry(format!("end mode {}", if mode == a.end_mode() { "the animation's" } else { "OTHER" })).or_default() += 1;
            let _ = flags;
        }
        let mut st: Vec<_> = starts.into_iter().collect();
        st.sort_by_key(|x| std::cmp::Reverse(x.1));
        println!("play starts: {:?}", &st[..st.len().min(24)]);
        let one = life.values().filter(|t| t.len() == 1).count();
        println!("streams following one track: {one} of {}; reads matching several tracks {ambiguous}", life.len());
        for (k, t) in life.iter().filter(|(_, t)| t.len() > 1).take(4) {
            println!("  stream {:x} (first bit {}) followed tracks {:x?}", k.0, k.1, t);
        }
    }
    // (LOCKSTEP_ANIM_HEADERS: each logged stream's header decoded from its animation's bits
    // through `anim::stream_header`)
    if std::env::var_os("LOCKSTEP_ANIM_HEADERS").is_some() {
        let anims = d.read("animatio.arc").unwrap_or_default();
        let all: Vec<housefire::anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| housefire::anim::read(housefire::arc::bytes(&anims, e))).collect();
        // the header decoded where it starts (227 bits before the first record, +4) in the
        // animation whose bits give the logged key count and floats: every field compared
        let mut cmp: BTreeMap<&str, usize> = BTreeMap::new();
        let mut done = BTreeSet::new();
        for v in &log.vec_streams {
            let Some((h, _)) = v.header else { continue };
            if !done.insert(v.stream) {
                continue;
            }
            let Some(start) = h[1].checked_sub(227) else { continue };
            let fit = all.iter().map(|a| housefire::anim::stream_header(&a.bit_words, start)).find(|x| x.keys == h[3] && x.offset[0].to_bits() == h[10] && x.offset[1].to_bits() == h[11] && x.offset[2].to_bits() == h[12]);
            let Some(x) = fit else {
                *cmp.entry("no animation's bits give it").or_default() += 1;
                continue;
            };
            let same = x.first_record == h[1]
                && x.record_bits as i32 == (h[4] >> 16) as i16 as i32
                && x.gap_bits == (h[4] >> 8) & 0xff
                && x.value_bits == h[4] & 0xff
                && x.scalar_bits == h[5] >> 24
                && x.scale.iter().zip(&h[7..10]).all(|(a, b)| a.to_bits() == *b)
                && x.scalar_scale.to_bits() == h[6];
            *cmp.entry(if same { "every field the same" } else { "DIFFERENT" }).or_default() += 1;
            // which node word names this start (study: its relation to the start bit)
            if let Some(a) = all.iter().find(|a| housefire::anim::stream_header(&a.bit_words, start) == x) {
                let mut how = Vec::new();
                for (ni, nd) in a.nodes.iter().enumerate() {
                    for (wi, w) in nd.iter().enumerate() {
                        if housefire::anim::Channel::of(*w) == housefire::anim::Channel::Stream(start) {
                            how.push(format!("node {ni} word {wi} as a stream"));
                        }
                    }
                }
                *cmp.entry(if how.is_empty() { "start named by no node word" } else { "start named by a node word" }).or_default() += 1;
                for h in &how {
                    *cmp.entry(match h.split(" as ").next().and_then(|x| x.rsplit(' ').next()) { Some("0") => "as word 0", Some("1") => "as word 1", Some("2") => "as word 2", Some("3") => "as word 3", Some("4") => "as word 4", _ => "as another word" }).or_default() += 1;
                }
                if cmp.get("shown").copied().unwrap_or(0) < 10 {
                    *cmp.entry("shown").or_default() += 1;
                    println!("  stream {:x} in {} at bit {start}: {how:?}; nodes {}", v.stream, a.name, a.nodes.len());
                }
            }
            if !same && cmp["DIFFERENT"] <= 4 {
                println!("  header different: logged {h:x?}; native {x:?} (scales {:x?})", x.scale.map(f32::to_bits));
            }
        }
        println!("stream headers decoded from the bits: {cmp:?}");
        // every logged window from the animation alone, given only its key (+8): the frames,
        // the points, and the tangents (only fresh when the window was not blended, mode 0)
        // (a stream's address is reused by later animations: found by its whole header)
        let mut found: HashMap<[u32; 13], Option<(usize, housefire::anim::StreamHeader)>> = HashMap::new();
        let mut win: BTreeMap<String, usize> = BTreeMap::new();
        let mut shown = 0;
        for v in &log.vec_streams {
            let Some((h, _)) = v.header else { continue };
            let mut key = h;
            key[2] = 0;
            key[0] = 0;
            key[5] &= 0xff00_0000; // +33 is the blend flag, +34 and +35 not the header
            let hit = found.entry(key).or_insert_with(|| {
                let start = h[1].checked_sub(227)?;
                all.iter().enumerate().map(|(i, a)| (i, housefire::anim::stream_header(&a.bit_words, start))).find(|(_, x)| {
                    x.keys == h[3] && x.first_record == h[1] && x.scale.iter().chain(&x.offset).zip(&h[7..13]).all(|(f, w)| f.to_bits() == *w)
                })
            });
            let Some((ai, x)) = hit.as_ref() else {
                *win.entry("stream not found".into()).or_default() += 1;
                continue;
            };
            let words = &all[*ai].bit_words;
            let (first, last, w) = x.window(words, h[2]);
            let eq = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(p, q)| p.to_bits() == q.to_bits());
            let frames = first == v.first && last == v.last;
            let points = eq(&w[0..3], &v.keys[0..3]) && eq(&w[9..12], &v.keys[9..12]);
            let tangents = eq(&w[3..9], &v.keys[3..9]);
            *win.entry(format!("frames {}", if frames { "same" } else { "DIFFERENT" })).or_default() += 1;
            *win.entry(format!("points {}", if points { "same" } else { "DIFFERENT" })).or_default() += 1;
            if v.mode == 0 {
                *win.entry(format!("tangents (mode 0) {}", if tangents { "same" } else { "DIFFERENT" })).or_default() += 1;
            }
            if (!frames || !points || (v.mode == 0 && !tangents)) && shown < 6 {
                shown += 1;
                println!("  window different: key {} of {}, logged ({}, {}, {:?}) mode {}; native ({first}, {last}, {w:?})", h[2], x.keys, v.first, v.last, v.keys, v.mode);
            }
        }
        println!("windows from the animation alone: {win:?}");
        // every read run through `anim::Stream` from the place alone; a stream whose address
        // shows another header (without the blend flag) is a new stream there
        let mut runs: HashMap<u32, ([u32; 13], housefire::anim::Stream)> = HashMap::new();
        let mut last_tick: HashMap<u32, u32> = HashMap::new();
        let mut run: BTreeMap<String, usize> = BTreeMap::new();
        let mut shown = 0;
        for v in &log.vec_streams {
            let Some((h, _)) = v.header else { continue };
            let mut key = h;
            key[2] = 0;
            key[0] = 0;
            key[5] &= 0xff00_0000; // +33 is the blend flag, +34 and +35 not the header
            let Some(Some((ai, x))) = found.get(&key) else { continue };
            let words = &all[*ai].bit_words;
            let e = runs.entry(v.stream).or_insert_with(|| (key, housefire::anim::Stream::new(words, *x)));
            let fresh = e.0 != key;
            let first_seen = e.1.last_place == f32::MAX && e.1.key == 0 && !fresh;
            if fresh {
                if std::env::var_os("LOCKSTEP_SHOW_NEW_STREAMS").is_some() && run.get("shown new streams").copied().unwrap_or(0) < 6 {
                    *run.entry("shown new streams".into()).or_default() += 1;
                    println!("  new stream at tick {} (last read tick {}): stream {:x}, first bit {} -> {}, keys {} -> {}", v.tick, last_tick.get(&v.stream).copied().unwrap_or(0), v.stream, e.0[1], key[1], e.0[3], key[3]);
                }
                // another animation's data at this address: a new stream there
                *e = (key, housefire::anim::Stream::new(words, *x));
            }
            let gap = v.tick.saturating_sub(*last_tick.get(&v.stream).unwrap_or(&v.tick));
            last_tick.insert(v.stream, v.tick);
            let kind = if fresh { "after another header" } else if first_seen { "first at its address" } else { "running" };
            let before = e.1.clone();
            let got = e.1.read(words, v.place);
            let st = e.1.clone();
            let same_window = st.key == h[2] && st.first == v.first && st.last == v.last && st.mode == v.mode;
            let same = got.iter().zip(&v.result).all(|(a, b)| a.to_bits() == b.to_bits());
            let what = match (same_window, same) {
                (true, true) => "window and result the same",
                (true, false) => "window the same, result DIFFERENT",
                _ => "window DIFFERENT",
            };
            *run.entry(format!("{what} ({kind})")).or_default() += 1;
            if what != "window and result the same" {
                // resync to the log's window (a start the log does not show)
                let words_here = words;
                let mut fixed = housefire::anim::Stream::new(words_here, *x);
                fixed.key = h[2];
                fixed.first = v.first;
                fixed.last = v.last;
                fixed.mode = v.mode;
                fixed.window = v.keys;
                fixed.last_place = v.place;
                if let Some(e) = runs.get_mut(&v.stream) { e.1 = fixed; }
                if shown < 8 {
                    shown += 1;
                    println!("  read different (ticks since {gap}): place {} logged key {} ({}, {}) mode {}; native key {} ({}, {}) mode {}; before key {} ({}, {}) last place {}", v.place, h[2], v.first, v.last, v.mode, st.key, st.first, st.last, st.mode, before.key, before.first, before.last, before.last_place);
                }
            }
        }
        println!("reads from the place alone: {run:?}");
        // (study: the logged starts in one animation, against a walk of headers from bit 0)
        if let Ok(name) = std::env::var("LOCKSTEP_ANIM_WALK")
            && let Some(a) = all.iter().find(|a| a.name == name)
        {
            {
                let mut starts: BTreeSet<u32> = BTreeSet::new();
                let mut once = BTreeSet::new();
                for v in log.vec_streams.iter().filter(|v| once.insert(v.stream)) {
                    if let Some((h, _)) = v.header
                        && let Some(st) = h[1].checked_sub(227)
                        && housefire::anim::stream_header(&a.bit_words, st).first_record == h[1]
                        && housefire::anim::stream_header(&a.bit_words, st).keys == h[3]
                    {
                        starts.insert(st);
                    }
                }
                println!("  {name}: bits {}, nodes {}; logged starts {starts:?}", a.bits, a.nodes.len());
                for (i, nd) in a.nodes.iter().enumerate().take(6) {
                    println!("    node {i}: {:?}", nd.map(|w| w as i32));
                }
                let mut at = 0u32;
                let mut chain = Vec::new();
                for _ in 0..12 {
                    let x = housefire::anim::stream_header(&a.bit_words, at);
                    chain.push((at, x.keys, x.record_bits));
                    at = x.end();
                    if at >= a.bits {
                        break;
                    }
                }
                println!("    walk from 0 (start, keys, record bits): {chain:?}");
            }
        }
    }
}
