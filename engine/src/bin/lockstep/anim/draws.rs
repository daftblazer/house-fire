//! The Sims' draws (LOCKSTEP_ANIM_DRAWS, stage 4.4e): each world matrix the original drew
//! ('p' 0xF1, 3x4 rows) against the bone matrices ('<', 4x4) of the same tick: which equal a
//! bone's (its 3x3 and translation, as rows or columns), and how far the rest are.

use crate::*;
use housefire::draw::{KEPT, PartIndex};
use housefire::tree_log::{BonesRec, Log, MatrixRec};

/// A bone matrix's 3x4 rows (EMat4 words: the 3x3 at 0..3, 4..7, 8..11 and the translation at
/// 12..15) laid out as the world matrix's rows (r0 = x0 y0 z0 tx), transposed or not.
fn as_rows(m: &[u32; 16], transpose: bool) -> [f32; 12] {
    let f = |i: usize| f32::from_bits(m[i]);
    core::array::from_fn(|k| {
        let (r, c) = (k / 4, k % 4);
        if c == 3 {
            f(12 + r)
        } else if transpose {
            f(c * 4 + r)
        } else {
            f(r * 4 + c)
        }
    })
}

pub fn draws(log: &Log, d: &Disc) {
    let mut bones_at: BTreeMap<u32, Vec<&BonesRec>> = BTreeMap::new();
    for b in &log.bones {
        bones_at.entry(b.tick).or_default().push(b);
    }
    let worlds: Vec<&MatrixRec> = log.matrices.iter().filter(|m| !m.projection && m.id == 0xF1 && m.values.len() >= 12).collect();
    let ticks: BTreeSet<u32> = worlds.iter().map(|m| m.tick).collect();
    let mut by_id: BTreeMap<(bool, u8), usize> = BTreeMap::new();
    for m in &log.matrices {
        *by_id.entry((m.projection, m.id)).or_default() += 1;
    }
    println!("matrix records by (projection, id): {by_id:?}");
    // the CPU-skinned primitives' bone matrices ('$') against the bone matrices of their tick
    let mut skin_counts: BTreeMap<String, usize> = BTreeMap::new();
    for k in &log.skins {
        let used = k.weights.iter().filter(|&&w| w != 0).count().max(1);
        for (j, m) in k.bones.iter().enumerate().take(used) {
            let hit = log.bones.iter().filter(|b| b.tick == k.tick).flat_map(|b| b.matrices.iter()).any(|bm| [false, true].iter().any(|&t| as_rows(bm, t) == *m));
            let near = !hit && log.bones.iter().filter(|b| b.tick.abs_diff(k.tick) <= 2).flat_map(|b| b.matrices.iter()).any(|bm| [false, true].iter().any(|&t| as_rows(bm, t) == *m));
            let ever = !hit && !near && log.bones.iter().flat_map(|b| b.matrices.iter()).any(|bm| [false, true].iter().any(|&t| as_rows(bm, t) == *m));
            let kind = if hit { "a bone's of the tick, to the bit" } else if near { "a bone's within 2 ticks" } else if ever { "a bone's of another tick" } else { "no bone's" };
            *skin_counts.entry(format!("matrix {j}: {kind}")).or_default() += 1;
        }
    }
    println!("skinned primitives ({}): {skin_counts:?}", log.skins.len());
    println!("draws: {} world matrices over ticks {:?}..{:?}; bone records over ticks {:?}..{:?}", worlds.len(), ticks.first(), ticks.last(), bones_at.keys().next(), bones_at.keys().last());
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let (mut shown, mut shown_near) = (0, 0);
    for w in &worlds {
        let Some(bs) = bones_at.get(&w.tick) else {
            *counts.entry("no bones that tick".into()).or_default() += 1;
            continue;
        };
        let mut best = (f32::MAX, false, 0usize, [0.0f32; 12]);
        for b in bs {
            for (i, m) in b.matrices.iter().enumerate() {
                for t in [false, true] {
                    let r = as_rows(m, t);
                    let d = (0..12).map(|k| (r[k] - w.values[k]).abs()).fold(0.0f32, f32::max);
                    if d < best.0 {
                        best = (d, t, i, r);
                    }
                }
            }
        }
        let kind = if best.0 == 0.0 {
            "a bone's, to the bit"
        } else if best.0 < 1e-4 {
            "a bone's, within 1e-4"
        } else if best.0 < 1e-2 {
            "near a bone's (1e-2)"
        } else {
            "no bone's"
        };
        *counts.entry(format!("{kind}{}", if best.0 < 1e-2 { if best.1 { " (transposed)" } else { " (as stored)" } } else { "" })).or_default() += 1;
        if best.0 > 0.0 && best.0 < 1e-2 && shown_near < 3 {
            shown_near += 1;
            println!("  near: tick {} world {:?}\n        bone {:?} off {}", w.tick, &w.values[..12], best.3, best.0);
        }
        if best.0 >= 1e-2 && shown < 3 {
            shown += 1;
            println!("  tick {} world {:?} nearest bone {} off {}", w.tick, &w.values[..12], best.2, best.0);
        }
    }
    println!("world matrices against the bones of their tick: {counts:?}");
    if std::env::var_os("DRAWS_GAPS").is_some() {
        gaps(log);
    }
    parts(log, d, &worlds, &bones_at);
}

/// Study aid (DRAWS_GAPS): per gap between strip records, the draws logged in it ('g' records
/// and the strip before) against the world matrices.
fn gaps(log: &Log) {
    let mut draws: HashMap<usize, usize> = HashMap::new();
    for (i, s) in log.strips.iter().enumerate() {
        if (1000..=1014).contains(&s.tick) {
            *draws.entry(i + 1).or_default() += 1;
        }
    }
    for (g, before, _) in &log.other_draws {
        if (1000..=1014).contains(&g.tick) {
            *draws.entry(*before).or_default() += 1;
        }
    }
    let mut mats: HashMap<usize, usize> = HashMap::new();
    for m in log.matrices.iter().filter(|m| m.id == 0xF1 && (1000..=1014).contains(&m.tick)) {
        *mats.entry(m.strips_before).or_default() += 1;
    }
    let mut hist: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for k in draws.keys().chain(mats.keys()).collect::<BTreeSet<_>>() {
        *hist.entry((draws.get(k).copied().unwrap_or(0), mats.get(k).copied().unwrap_or(0))).or_default() += 1;
    }
    println!("gaps (draws, world matrices): gaps {hist:?}");
    println!("draws {} matrices {}", draws.values().sum::<usize>(), mats.values().sum::<usize>());
    let by_tick: BTreeMap<u32, usize> = log.strips.iter().fold(BTreeMap::new(), |mut m, s| {
        *m.entry(s.tick).or_default() += 1;
        m
    });
    println!("strips {} by tick {:?}", log.strips.len(), by_tick.iter().take(20).collect::<Vec<_>>());
    println!("other draws by tick {:?}", log.other_draws.iter().fold(BTreeMap::new(), |mut m: BTreeMap<u32, usize>, g| { *m.entry(g.0.tick).or_default() += 1; m }));
}

/// The bone matrices of a tick that equal a world matrix, to the bit: (the record's place in the
/// tick's list, the bone), either layout.
fn bone_matches(bs: &[&BonesRec], w: &[f32]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (k, b) in bs.iter().enumerate() {
        for (i, m) in b.matrices.iter().enumerate() {
            if [false, true].iter().any(|&t| as_rows(m, t).iter().zip(w).all(|(a, b)| a.to_bits() == b.to_bits())) {
                out.push((k, i));
            }
        }
    }
    out
}

/// Studies 3 to 5: the bone-matched draws' strips against `models.arc`'s parts (line 1), the
/// parts' bones against the matched bones (line 2), and how the skinned parts are drawn (line 3).
///
/// The log writes a strip's record when the command starts and the draw makes its world matrix
/// after it: a world matrix logged with k strips before it is the matrix of strip k - 1 (the
/// Garden_Lamp's parts on bones 0, 1, 2, 4 are followed by the matrices of bones 0, 1, 2, 4, and
/// the first strip of a tick has none before it). Several world matrices after one strip: which
/// is its own is not known.
fn parts(log: &Log, d: &Disc, worlds: &[&MatrixRec], bones_at: &BTreeMap<u32, Vec<&BonesRec>>) {
    let index = PartIndex::new(&d.read("models.arc").unwrap_or_default());
    let n = log.strips.len();
    // per gap between strip records: the world matrices, and whether a 'g' draw is in it
    let mut worlds_in: HashMap<usize, usize> = HashMap::new();
    for w in worlds {
        *worlds_in.entry(w.strips_before).or_default() += 1;
    }
    let other_in: BTreeSet<usize> = log.other_draws.iter().map(|g| g.1).collect();
    // the position matrices loaded by slot ('p', ids below the view's and the world's) after each strip
    let mut slots_in: HashMap<usize, Vec<&MatrixRec>> = HashMap::new();
    for m in log.matrices.iter().filter(|m| !m.projection && m.id < 0xF0) {
        slots_in.entry(m.strips_before).or_default().push(m);
    }
    let show = std::env::var_os("DRAWS_SHOW").is_some();
    let mut shown = 0;
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    let mut hung: BTreeMap<String, usize> = BTreeMap::new();
    let mut skinned: BTreeMap<String, usize> = BTreeMap::new();
    let mut sims: BTreeMap<bool, BTreeSet<&str>> = BTreeMap::new();
    let mut models: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for w in worlds {
        let Some(bs) = bones_at.get(&w.tick) else { continue };
        let matches = bone_matches(bs, &w.values);
        if matches.is_empty() {
            continue;
        }
        let k = w.strips_before;
        let own_strip = k > 0 && k <= n && log.strips[k - 1].tick == w.tick;
        let why = if !own_strip {
            if k >= n {
                Some("after the strip records (the observer keeps the first 8,000)")
            } else if other_in.contains(&k) {
                Some("no strip before it in its tick (a 'g' draw in the gap)")
            } else {
                Some("no strip before it in its tick")
            }
        } else if worlds_in[&k] > 1 {
            Some("several world matrices after the strip")
        } else {
            None
        };
        if let Some(why) = why {
            *found.entry(why.into()).or_default() += 1;
            continue;
        }
        let i = k - 1;
        let s = &log.strips[i];
        if s.words[2] < 0x8000_0000 || s.at[2] == [0u8; KEPT] {
            *found.entry("strip: positions not in memory".into()).or_default() += 1;
            continue;
        }
        let hits = index.find(s);
        let Some(p) = hits.first() else {
            *found.entry("strip: positions not in models.arc (made at run time)".into()).or_default() += 1;
            continue;
        };
        *found.entry(format!("strip: found in models.arc{}", if hits.len() > 1 { " (several parts alike)" } else { "" })).or_default() += 1;
        // line 2: the part's bone against the matched ones (parts alike on other bones: any)
        let on = |h: &housefire::draw::Part, b: Option<usize>| h.bone() == b && matches.iter().any(|&(_, i)| Some(i) == b);
        let key = if hits.iter().any(|h| h.bone().is_some() && on(h, h.bone())) {
            "same"
        } else if hits.iter().any(|h| h.bone().is_none()) {
            // a one-piece mesh: no bone named (the model's own matrix); equal to bone 0 of its
            // controller, or to another model's bone by chance (placements on the tile grid)
            if matches.iter().any(|&(_, i)| i == 0) { "no bone named (one-piece mesh): the matrix is a controller's bone 0" } else { "no bone named (one-piece mesh): the matrix is another bone's (no controller of its own)" }
        } else {
            "DIFFERENT"
        };
        *hung.entry(key.into()).or_default() += 1;
        if show && key == "DIFFERENT" && shown < 12 {
            shown += 1;
            println!("  DIFFERENT: strip {i} {} node bone {} slots {:?} weights {} matched {:?} ({} parts alike)", p.record.name, p.node.bone, p.batch.matrices, p.batch.weights, matches, hits.len());
        }
        if let Some(b) = p.bone() {
            models.entry(p.record.name.as_str()).or_default().insert(b);
        }
    }
    // line 3: every skinned strip (a part whose node names no bone but whose batch names matrix
    // slots) of the ticks with strip records: its slots, the world matrices and the position
    // matrices after it, and whether those are the tick's bone matrices
    let is_bone = |tick: u32, v: &[f32]| bones_at.get(&tick).is_some_and(|bs| !bone_matches(bs, v).is_empty());
    for (i, s) in log.strips.iter().enumerate() {
        if s.words[2] < 0x8000_0000 || s.at[2] == [0u8; KEPT] || !bones_at.contains_key(&s.tick) {
            continue;
        }
        let hits = index.find(s);
        let Some(p) = hits.first().filter(|p| p.node.bone < 0 && !p.batch.matrices.is_empty()) else { continue };
        let k = i + 1;
        let loaded = slots_in.get(&k).map_or(&[][..], |v| v.as_slice());
        let worlds_here: Vec<&&MatrixRec> = worlds.iter().filter(|w| w.strips_before == k).collect();
        let key = format!(
            "{} slot{}, {} world matri{} after the strip ({} a bone's), {} 'p' record{} ({} a bone's)",
            p.batch.matrices.len().min(5),
            if p.batch.matrices.len() == 1 { "" } else { "s" },
            worlds_here.len(),
            if worlds_here.len() == 1 { "x" } else { "ces" },
            worlds_here.iter().filter(|w| is_bone(w.tick, &w.values)).count(),
            loaded.len(),
            if loaded.len() == 1 { "" } else { "s" },
            loaded.iter().filter(|m| is_bone(m.tick, &m.values)).count()
        );
        *skinned.entry(key).or_default() += 1;
        if s.tick == 1000 {
            sims.entry(p.skinned()).or_default().insert(p.record.name.as_str());
        }
    }
    println!("bone-drawn strips to models: {found:?}");
    println!("parts on their bones: {hung:?}");
    println!("skinned draws: {skinned:?}");
    println!("  skinned parts at tick 1000 (blended: {{true}}, rigid on one bone: {{false}}): {sims:?}");
    if show {
        println!("  parts on bones, by model: {models:?}");
    }
}
