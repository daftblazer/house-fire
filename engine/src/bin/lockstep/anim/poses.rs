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
    // line 1: the weight-1 copies (poses where a node's state became its local pose), and
    // line 2: every node's fold, by kind
    let (mut copy_poses, mut copy_nodes, mut copy_different) = (0, 0, 0);
    let mut folds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let (mut root_flagged, mut records, mut shown) = (0, 0, 0);
    for p in &log.poses {
        let Some(rec) = log.node_states.get(next).filter(|r| r.tick == p.tick && r.track.is_some_and(|(t, pl)| t == p.track && pl.to_bits() == p.place.to_bits())) else {
            *skipped.entry("pose without a record (after the log's first 40,000)".into()).or_default() += 1;
            continue;
        };
        next += 1;
        // the track's play: the last step before the pose
        let play = plays.get(&p.track).and_then(|v| v.iter().rev().find(|x| x.at < p.at)).copied();
        let Some((play, anim)) = play.and_then(|x| Some((x, by_id.get(&x.anim)?))) else {
            *skipped.entry("nodes of a track whose animation the log does not name".into()).or_default() += rec.before.len();
            continue;
        };
        if anim.nodes.len() != rec.before.len() {
            *skipped.entry("nodes of a pose whose count is not its animation's".into()).or_default() += rec.before.len();
            continue;
        }
        records += 1;
        let layer = play.flags.map(|f| f & 0x100 != 0);
        let mut known = Vec::new();
        let ts = streams.entry((p.track, play.began)).or_insert_with(|| TrackStreams::new(anim));
        // each node's fold: Skip for one no fold gives, None for one not changed or not made
        let mut found: Vec<Option<Fold>> = Vec::with_capacity(rec.before.len());
        for i in 0..rec.before.len() {
            let (before, after) = (&rec.before[i], &rec.after[i]);
            if before == after {
                *folds.entry("weight 0 (the state unchanged)").or_default() += 1;
                found.push(None);
                continue;
            }
            let local = match ts.local(anim, i, p.place, before[0]) {
                Ok(l) => l,
                Err(pose::Unsupported::ScalarStream) => {
                    *folds.entry("unsupported channel (a scalar stream)").or_default() += 1;
                    found.push(None);
                    continue;
                }
            };
            if i == 0 && play.flags.is_some_and(|f| f & 7 != 0) {
                root_flagged += 1;
            }
            let fold = explain(&Pose::from_words(before), &local, after, layer, &mut known);
            let kind = match fold {
                Some(Fold::Copy) => "copy (weight 1)",
                Some(Fold::Blend(_)) => "blend",
                Some(Fold::Layer(_)) => "layer",
                _ => "DIFFERENT",
            };
            *folds.entry(kind).or_default() += 1;
            if fold.is_none() && shown < 6 {
                shown += 1;
                println!("  tick {} node {i} anim {} place {} layer {layer:?} known weights {known:?}: local {:x?}\n    before {before:x?}\n    after  {after:x?}", rec.tick, anim.name, p.place, local.to_words());
            }
            found.push(fold.or(Some(Fold::Skip)));
        }
        // a pose where some node's state became its local pose has weight 1: every node it
        // changed should have, bar the nodes' own weights (+152)
        if found.contains(&Some(Fold::Copy)) {
            copy_poses += 1;
            copy_nodes += found.iter().filter(|f| **f == Some(Fold::Copy)).count();
            copy_different += found.iter().filter(|f| **f == Some(Fold::Skip)).count();
        }
    }
    let different = folds.get("DIFFERENT").copied().unwrap_or(0);
    println!("local poses from the channels (the weight-1 copies, {copy_nodes} nodes in {copy_poses} poses; a node of such a pose that no fold gives counts): DIFFERENT -> {copy_different}");
    println!("node states after the pose ({records} poses, every node, all 16 words, the weights found from the states: not logged): DIFFERENT -> {different}; {folds:?}");
    println!("  root-node fix (track flags bits 0 to 2 set on the pose's node 0, values not logged): {root_flagged} nodes; not compared: {skipped:?}");
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
    if let Some(Fold::Blend(w) | Fold::Layer(w)) = found
        && !known.contains(&w)
    {
        known.push(w);
    }
    found
}
