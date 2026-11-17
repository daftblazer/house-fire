//! The Sims' bones from the engine's own state (LOCKSTEP_ANIM_BONES, with the run's card):
//! per Sim at the ticks the log drew them (`log.skins`), the bones (`figure::bones` through the
//! tracks `figure::tracks` makes from the replay's plays) against the logged matrices (the four
//! bones of each skinned primitive, '$', and the worlds of the parts on one bone, no bone index
//! in either): how many of a Sim's bones equal a logged matrix, the others by their index and
//! how far they are from the nearest logged one (translation, tiles), and the other way round:
//! the logged matrices no bone of any Sim equals, and which of those the tick before's bones do
//! (LOCKSTEP_ANIM_ACTION_ONLY=1: the action alone, the pose before the layers).

use crate::*;
use housefire::anim::Catalog;
use housefire::figure::{self, Cast, Masks, Rigs, Stance};
use housefire::replay::{Replay, RunWith, Start};
use housefire::skeleton::Mat4;

/// A bone equals a logged matrix when every element is within this of it, the 3 x 3 relative to
/// the matrix's largest diagonal element (the bones carry the models' unit, 2^-12) and the
/// translation in tiles (the comparison `frame_draws` makes of a world matrix).
const TOLERANCE: f32 = 1e-4;

/// The logged matrices of a tick, each once: the skinned primitives' bones ('$') and the world
/// matrices of the parts that hang from one bone ('m' records of the Sims' unit, 2^-12 a column:
/// hair and glasses on the head).
fn logged(log: &housefire::tree_log::Log, tick: u32) -> Vec<[f32; 12]> {
    let mut out: Vec<[f32; 12]> = Vec::new();
    for m in log.matrices.iter().filter(|m| m.tick == tick && !m.projection && m.values.len() >= 12) {
        let unit = (0..3).all(|c| ((0..3).map(|r| m.values[4 * r + c].powi(2)).sum::<f32>().sqrt() / figure::BONE_SCALE - 1.0).abs() < 0.01);
        let v: [f32; 12] = std::array::from_fn(|i| m.values[i]);
        if unit && !out.contains(&v) {
            out.push(v);
        }
    }
    for k in log.skins.iter().filter(|k| k.tick == tick) {
        for m in k.bones {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

fn near(a: &[f32; 12], b: &[f32; 12]) -> bool {
    let unit = a[0].abs().max(a[5].abs()).max(a[10].abs()).max(1e-9);
    (0..12).all(|i| (a[i] - b[i]).abs() <= TOLERANCE * if i % 4 == 3 { 1.0 } else { unit })
}

/// How far the translation of `a` is from the nearest logged matrix's, tiles.
fn nearest(a: &[f32; 12], logged: &[[f32; 12]]) -> f32 {
    logged.iter().map(|l| ((0..3).map(|r| (a[4 * r + 3] - l[4 * r + 3]).powi(2)).sum::<f32>()).sqrt()).fold(f32::MAX, f32::min)
}

pub fn sims(log: &housefire::tree_log::Log, d: &Disc, lib: &Library, ngh: &[u8], card: Option<&str>) {
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let Ok(start) = Start::read(d, card, -1) else { return };
    let (Some(rigs), Some(cast)) = (Rigs::read(d), Cast::read(d, ngh)) else { return };
    let catalog = Catalog::read(d, house);
    let ticks: BTreeSet<u32> = log.skins.iter().map(|k| k.tick).collect();
    let mut previous: Vec<(i16, usize, [f32; 12])> = Vec::new();
    for tick in ticks {
        let logged = logged(log, tick);
        let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: Some(catalog.clone()) }) else { return };
        while replay.tick() + 1 < tick && replay.step() {}
        let plays = replay.sim_plays(tick);
        // the logged matrices no Sim's bones equal, kept with the bones of every Sim
        let mut all_bones: Vec<(i16, usize, [f32; 12])> = Vec::new();
        for o in replay.world().objects().filter(|o| o.person && o.on_lot()) {
            let Some(desc) = o.person_data.get(31).and_then(|n| cast.description(*n)) else { continue };
            let nodes = rigs.of(desc);
            let masks = Masks::new(nodes.len());
            let stance = Stance {
                action: plays.iter().find(|p| p.0 == o.id()).map(|p| (p.1, p.2, p.3)),
                carrying: replay.world().objects().any(|h| h.data.get(DATA_CONTAINER) == Some(&o.id())),
                mood: o.motives.get(figure::FACE_MOTIVE).copied().unwrap_or(0.0),
                adult: desc.adult,
            };
            let mut tracks = figure::tracks(&catalog, &stance, &masks);
            // (LOCKSTEP_ANIM_ACTION_ONLY: the action alone, the pose before the layers)
            if std::env::var_os("LOCKSTEP_ANIM_ACTION_ONLY").is_some() {
                tracks.truncate(1);
            }
            let place = (o.x16 as f32 / 16.0, o.y16 as f32 / 16.0);
            // the engine's heading: from the direction the world holds for the Sim
            let engine_heading = figure::heading_of_facing(o.facing());
            // the heading: the eighth whose bones come nearest the logged (the engine's own is
            // not found: `facing` is stale)
            let rows = |h: u8| -> Vec<[f32; 12]> {
                let m: Vec<Mat4> = figure::bones(nodes, &tracks, &figure::root(place.0, place.1, h));
                m.iter().map(figure::rows).collect()
            };
            let score = |h: u8| -> usize { rows(h).iter().filter(|r| logged.iter().any(|l| near(r, l))).count() };
            let best = (0..8u8).max_by_key(|&h| score(h)).unwrap_or(0);
            let bones = rows(engine_heading);
            all_bones.extend(bones.iter().enumerate().map(|(i, r)| (o.id(), i, *r)));
            let same = bones.iter().filter(|r| logged.iter().any(|l| near(r, l))).count();
            let mut off: Vec<(f32, usize)> = bones.iter().enumerate().filter(|(_, r)| !logged.iter().any(|l| near(r, l))).map(|(i, r)| (nearest(r, &logged), i)).filter(|(dist, _)| *dist < 1.0).collect();
            off.sort_by(|a, b| b.0.total_cmp(&a.0));
            let names: Vec<String> = off.iter().map(|(dist, i)| format!("{i} {dist:.3}")).collect();
            println!(
                "SIM tick {tick} id {} male {} heading {engine_heading} (the logged bones fit {best}; the save's facing {}) action {:?} carrying {} mood {:.1}: {same} of {} bones equal a logged matrix; {} more within a tile, worst: {}",
                o.id(),
                desc.male,
                o.facing(),
                stance.action.map(|a| a.0),
                stance.carrying,
                stance.mood,
                bones.len(),
                off.len(),
                names.join(", ")
            );
        }
        // the other way: each logged matrix against the bones of all the Sims
        let unmatched: Vec<String> = logged
            .iter()
            .filter(|l| !all_bones.iter().any(|b| near(&b.2, l)))
            .map(|l| {
                let (id, i, r) = all_bones.iter().min_by(|a, b| nearest(&a.2, &[*l]).total_cmp(&nearest(&b.2, &[*l]))).copied().unwrap_or((0, 0, *l));
                format!("(Sim {id} bone {i}, {:.4}; the matrix {l:.4?})", nearest(&r, &[*l]))
            })
            .collect();
        println!("LOGGED tick {tick}: {} distinct matrices, {} equal a bone of a Sim; the others, with the nearest bone: {}", logged.len(), logged.len() - unmatched.len(), unmatched.join(" "));
        // the lag: of the others, the ones equal to a bone of the tick before's pose (the next tick, from the tick just done)
        let lagged: Vec<String> = logged.iter().filter(|l| !all_bones.iter().any(|b| near(&b.2, l))).filter_map(|l| previous.iter().find(|b| near(&b.2, l)).map(|b| format!("(Sim {} bone {})", b.0, b.1))).collect();
        println!("LAG tick {tick}: the logged matrices no bone of this tick equals but one of the tick {} equals: {}", tick - 1, lagged.join(" "));
        previous = all_bones;
    }
}
