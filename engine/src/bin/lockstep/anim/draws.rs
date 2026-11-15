//! The Sims' draws (LOCKSTEP_ANIM_DRAWS, stage 4.4e): each world matrix the original drew
//! ('p' 0xF1, 3x4 rows) against the bone matrices ('<', 4x4) of the same tick: which equal a
//! bone's (its 3x3 and translation, as rows or columns), and how far the rest are.

use crate::*;

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

pub fn draws(log: &housefire::tree_log::Log) {
    let mut bones_at: BTreeMap<u32, Vec<&housefire::tree_log::BonesRec>> = BTreeMap::new();
    for b in &log.bones {
        bones_at.entry(b.tick).or_default().push(b);
    }
    let worlds: Vec<&housefire::tree_log::MatrixRec> = log.matrices.iter().filter(|m| !m.projection && m.id == 0xF1 && m.values.len() >= 12).collect();
    let ticks: BTreeSet<u32> = worlds.iter().map(|m| m.tick).collect();
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
}
