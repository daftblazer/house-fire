//! The pose studies (LOCKSTEP_ANIM_POSES): each logged pose ('=', EAnimController::Animate
//! 801581C8) against the native one: the local pose made from the node's channels at the track's
//! place (`pose::TrackStreams`), then folded into the node state (`pose::fold`).

use crate::*;
use housefire::anim::Anim;
use housefire::pose::{self, Fold, Pose, TrackStreams, Unsupported};

/// What the log shows of a track when a pose starts: its animation's id (the last step before
/// the pose, '8') and the track's flags (+8, '*').
#[derive(Clone, Copy)]
struct Play {
    at: usize,
    anim: u32,
    flags: Option<u32>,
}

pub fn poses(log: &housefire::tree_log::Log, d: &Disc) {
    let anims = d.read("animatio.arc").unwrap_or_default();
    let by_id: BTreeMap<u32, Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, housefire::anim::read(housefire::arc::bytes(&anims, e))?))).collect();
    let mut plays: HashMap<u32, Vec<Play>> = HashMap::new();
    for t in &log.tracks {
        plays.entry(t.track).or_default().push(Play { at: t.at, anim: t.head[1], flags: t.end.map(|e| e.1) });
    }
    let mut streams: HashMap<(u32, u32), TrackStreams> = HashMap::new();
    let mut next = 0;
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for p in &log.poses {
        let Some(rec) = log.node_states.get(next).filter(|r| r.tick == p.tick && r.track.is_some_and(|(t, pl)| t == p.track && pl.to_bits() == p.place.to_bits())) else {
            *tally.entry("pose without a record".into()).or_default() += 1;
            continue;
        };
        next += 1;
        // the track's play: the last step before the pose
        let play = plays.get(&p.track).and_then(|v| v.iter().rev().find(|x| x.at < p.at)).copied();
        let Some((play, anim)) = play.and_then(|x| Some((x, by_id.get(&x.anim)?))) else {
            *tally.entry("track's animation not found".into()).or_default() += 1;
            continue;
        };
        if anim.nodes.len() != rec.before.len() {
            *tally.entry("node count differs".into()).or_default() += 1;
            continue;
        }
        let layer = play.flags.map(|f| f & 0x100 != 0);
        let ts = streams.entry((p.track, play.anim)).or_insert_with(|| TrackStreams::new(anim));
        for i in 0..rec.before.len() {
            let (before, after) = (&rec.before[i], &rec.after[i]);
            if before == after {
                *tally.entry("unchanged".into()).or_default() += 1;
                continue;
            }
            let local = match ts.local(anim, i, p.place, before[0]) {
                Ok(l) => l,
                Err(u) => {
                    *tally.entry(format!("unsupported {u:?}")).or_default() += 1;
                    continue;
                }
            };
            let state = Pose::from_words(before);
            let what = match explain(&state, &local, after, layer) {
                Explained::Fold(Fold::Copy) => "copy".to_string(),
                Explained::Fold(Fold::Blend(_)) => "blend".to_string(),
                Explained::Fold(Fold::Layer(_)) => "layer".to_string(),
                Explained::Fold(_) => "skip".to_string(),
                Explained::Slerp(f) => format!("{} with a slerp, the rest the same", if matches!(f, Fold::Layer(_)) { "layer" } else { "blend" }),
                Explained::No => "DIFFERENT".to_string(),
            };
            if what == "DIFFERENT" && local.to_words()[1..] == after[1..] {
                *tally.entry(format!("  flags only: before {:x} local {:x} after {:x} node0 {}", before[0], local.flags, after[0], i == 0)).or_default() += 1;
            }
            if what == "DIFFERENT" && shown < 12 {
                shown += 1;
                println!("  tick {} node {i} anim {} place {} layer {layer:?}: local {:x?}\n    before {before:x?}\n    after  {after:x?}", rec.tick, anim.name, p.place, local.to_words());
            }
            *tally.entry(what).or_default() += 1;
        }
    }
    println!("{tally:?}");
}

/// What a node's logged state after the pose is: a fold of the local pose into the state before
/// (the weights are not logged: found from the vectors), or one whose rotation needs the
/// arccosine.
enum Explained {
    Fold(Fold),
    Slerp(Fold),
    No,
}

/// The weights a node's before, local pose and after allow: each component's, and the floats
/// next to them.
fn weights(state: &Pose, local: &Pose, after: &Pose) -> Vec<f32> {
    let mut ratios = Vec::new();
    for k in 0..3 {
        let (s, l, a) = (state.translation[k], local.translation[k], after.translation[k]);
        ratios.push((a - s) / (l - s));
        ratios.push((a - s) / l);
        let (s, l, a) = (state.scale[k], local.scale[k], after.scale[k]);
        ratios.push((a - s) / (l - s));
        ratios.push((a - s) / (s * l - s));
    }
    let mut out = Vec::new();
    for r in ratios.into_iter().filter(|r| r.is_finite() && (0.0..=4.0).contains(r)) {
        for d in -24i32..=24 {
            out.push(f32::from_bits((r.to_bits() as i32 + d) as u32));
        }
    }
    out.sort_by_key(|w| w.to_bits());
    out.dedup();
    out
}

fn explain(state: &Pose, local: &Pose, after: &[u32; 16], layer: Option<bool>) -> Explained {
    if local.to_words() == *after {
        return Explained::Fold(Fold::Copy);
    }
    let want = Pose::from_words(after);
    let mut slerp = None;
    for w in weights(state, local, &want) {
        for f in [Fold::Blend(w), Fold::Layer(w)] {
            if layer.is_some_and(|l| l != matches!(f, Fold::Layer(_))) {
                continue;
            }
            match pose::fold(state, local, f) {
                Ok(p) if p.to_words() == *after => return Explained::Fold(f),
                Err(Unsupported::Slerp) => {
                    // everything but the rotation (the pose given the state's own: a straight blend)
                    let mut mine = pose::fold(state, &Pose { rotation: state.rotation, ..*local }, f).unwrap_or(*state);
                    mine.rotation = want.rotation;
                    if mine.to_words() == *after {
                        slerp = Some(f);
                    }
                }
                _ => {}
            }
        }
    }
    slerp.map_or(Explained::No, Explained::Slerp)
}
