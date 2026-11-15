//! The pose studies (LOCKSTEP_ANIM_POSES): each logged pose ('=', EAnimController::Animate
//! 801581C8) against the native one: the local pose made from the node's channels at the track's
//! place (`pose::TrackStreams`), then folded into the node state (`pose::fold`).

use crate::*;
use housefire::anim::Anim;
use housefire::pose::{self, Fold, Pose, TrackStreams};

/// What the log shows of a track when a pose starts: its animation's id (the last step before
/// the pose, '8') and the track's flags (+8, '*').
#[derive(Clone, Copy)]
struct Play {
    at: usize,
    anim: u32,
    flags: Option<u32>,
    /// Where in the log the play began: the last step that found the track at place 0 (a new
    /// play builds its streams anew).
    began: usize,
}

pub fn poses(log: &housefire::tree_log::Log, d: &Disc) {
    let anims = d.read("animatio.arc").unwrap_or_default();
    let by_id: BTreeMap<u32, Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, housefire::anim::read(housefire::arc::bytes(&anims, e))?))).collect();
    let mut plays: HashMap<u32, Vec<Play>> = HashMap::new();
    for t in &log.tracks {
        let v = plays.entry(t.track).or_default();
        let began = match v.last() {
            Some(last) if t.place[0] != 0.0 && last.anim == t.head[1] => last.began,
            _ => t.at,
        };
        v.push(Play { at: t.at, anim: t.head[1], flags: t.end.map(|e| e.1), began });
    }
    let mut streams: HashMap<(u32, usize), TrackStreams> = HashMap::new();
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
        let mut known = Vec::new();
        let ts = streams.entry((p.track, play.began)).or_insert_with(|| TrackStreams::new(anim));
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
            let what = match explain(&state, &local, after, layer, &mut known) {
                Some(Fold::Copy) => "copy",
                Some(Fold::Blend(_)) => "blend",
                Some(Fold::Layer(_)) => "layer",
                Some(Fold::Skip) => "skip",
                None => "DIFFERENT",
            };
            if what == "DIFFERENT" && local.to_words()[1..] == after[1..] {
                *tally.entry(format!("  flags only: before {:x} local {:x} after {:x} node0 {}", before[0], local.flags, after[0], i == 0)).or_default() += 1;
            }
            if what == "DIFFERENT" {
                let mask: String = (0..16).map(|k| if local.to_words()[k] == after[k] { '.' } else { 'x' }).collect();
                *tally.entry(format!("  different: node0 {} layer {layer:?} words {mask}", i == 0)).or_default() += 1;
            }
            if what == "DIFFERENT" && shown < 12 {
                shown += 1;
                println!("  tick {} node {i} anim {} place {} layer {layer:?}: local {:x?}\n    before {before:x?}\n    after  {after:x?}", rec.tick, anim.name, p.place, local.to_words());
            }
            *tally.entry(what.to_string()).or_default() += 1;
        }
    }
    println!("{tally:?}");
}

/// The weights a node's before, local pose and after allow (the weights are not logged): each
/// component's, and the floats next to them.
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
    // the rotation's share of the way: the blend's angle from the state over the angle between
    // the two, the layer's angle (twice the arccosine of w) over the pose's
    let (a, b, c) = (state.rotation.map(f64::from), local.rotation.map(f64::from), after.rotation.map(f64::from));
    // (the angle between unit quaternions from the perpendicular part, which stays exact
    // where the arccosine of a dot near 1 does not)
    let angle = |p: [f64; 4], q: [f64; 4]| {
        let dot: f64 = p.iter().zip(&q).map(|(x, y)| x * y).sum();
        let perpendicular = (0..4).map(|i| (q[i] - dot * p[i]).powi(2)).sum::<f64>().sqrt();
        perpendicular.atan2(dot.abs())
    };
    ratios.push((angle(a, c) / angle(a, b)) as f32);
    let conj = [-a[0], -a[1], -a[2], a[3]];
    let t = [
        c[3] * conj[0] + c[0] * conj[3] + c[1] * conj[2] - c[2] * conj[1],
        c[3] * conj[1] - c[0] * conj[2] + c[1] * conj[3] + c[2] * conj[0],
        c[3] * conj[2] + c[0] * conj[1] - c[1] * conj[0] + c[2] * conj[3],
        c[3] * conj[3] - c[0] * conj[0] - c[1] * conj[1] - c[2] * conj[2],
    ];
    ratios.push((angle([0.0, 0.0, 0.0, 1.0], t) / angle([0.0, 0.0, 0.0, 1.0], b)) as f32);
    let mut out = Vec::new();
    for r in ratios.into_iter().filter(|r| r.is_finite() && (0.0..=4.0).contains(r)) {
        for d in -3000i32..=3000 {
            out.push(f32::from_bits((r.to_bits() as i32 + d) as u32));
        }
    }
    out.sort_by_key(|w| w.to_bits());
    out.dedup();
    out
}

/// The fold of the local pose into the state before that gives the state after, if there is one.
/// The weights already found in the pose are tried first (a track has one weight, bar its nodes').
fn explain(state: &Pose, local: &Pose, after: &[u32; 16], layer: Option<bool>, known: &mut Vec<f32>) -> Option<Fold> {
    if local.to_words() == *after {
        return Some(Fold::Copy);
    }
    let want = Pose::from_words(after);
    let wanted = |f: &Fold| layer.is_none_or(|l| l == matches!(f, Fold::Layer(_)));
    let same = |f: &Fold| pose::fold(state, local, *f).to_words() == *after;
    let tried = known.iter().flat_map(|w| [Fold::Blend(*w), Fold::Layer(*w)]).filter(wanted).find(same);
    let found = tried.or_else(|| weights(state, local, &want).into_iter().flat_map(|w| [Fold::Blend(w), Fold::Layer(w)]).filter(wanted).find(same));
    if let Some(Fold::Blend(w) | Fold::Layer(w)) = found {
        if !known.contains(&w) {
            known.push(w);
        }
    }
    found
}
