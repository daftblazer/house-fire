//! The pose studies (LOCKSTEP_ANIM_POSES): each logged pose ('=', EAnimController::Animate
//! 801581C8) against the native one: the local pose made from the node's channels at the track's
//! place (`pose::TrackStreams`), then folded into the node state (`pose::fold`) by the logged
//! weight, and the logged weights themselves.

use crate::*;
use housefire::anim::Anim;
use housefire::pose::{self, Fold, Pose, TrackStreams};

/// The track flag (+8) that makes a pose a layer (801586B4).
const TRACK_LAYER: u32 = 0x100;
/// The track flag bits (+8, bits 0 to 2) that name the translation components the root node's
/// fix replaces (FixAccumulationForRootNode 80158184).
const TRACK_ROOT_FIX: u32 = 0x7;

/// What the log shows of a track when a pose starts: its animation's id (the last step before
/// the pose, '8').
#[derive(Clone, Copy)]
struct Play {
    at: usize,
    anim: u32,
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
        v.push(Play { at: t.at, anim: t.head[1], began });
    }
    let mut streams: HashMap<(u32, usize), TrackStreams> = HashMap::new();
    let mut next = 0;
    // per node-state array: whether a base track has posed it since its reset
    let mut bases: HashMap<u32, bool> = HashMap::new();
    let default_words = Pose::default().to_words();
    // line 1: the weight-1 copies (the local pose is the node's state after), line 2: every
    // node's fold by the logged weight, by kind
    let (mut copy_nodes, mut copy_different) = (0, 0);
    let mut folds: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let (mut root_fixed_nodes, mut records, mut shown) = (0, 0, 0);
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
        let layer = rec.flags & TRACK_LAYER != 0;
        let ts = streams.entry((p.track, play.began)).or_insert_with(|| TrackStreams::new(anim));
        // the node states are reset (every node to the default pose) before the tracks pose
        // them; the log does not show the reset, only the states it leaves
        let based = bases.entry(rec.states).or_insert(false);
        if rec.before.iter().all(|n| *n == default_words) {
            *based = false;
        }
        let track_weight = pose::base_weight(rec.weight, layer, based);
        for i in 0..rec.before.len() {
            let (before, after) = (&rec.before[i], &rec.after[i]);
            let node_weight = rec.node_weights.as_ref().map_or(1.0, |w| w[i]);
            let weight = track_weight * node_weight;
            let fold = Fold::of(weight, layer);
            let mut local = match ts.local(anim, i, p.place, before[0]) {
                Ok(l) => l,
                Err(pose::Unsupported::ScalarStream) => {
                    *skipped.entry("nodes with a scalar stream (not decoded)".into()).or_default() += 1;
                    continue;
                }
            };
            // the root node's fix: the components the track's flags name take the track's values.
            // Only +124 (the first) is logged; the other two are taken from the state after
            let root_fixed = i == 0 && rec.flags & TRACK_ROOT_FIX != 0;
            if root_fixed {
                root_fixed_nodes += 1;
                pose::fix_root(&mut local, rec.flags, [f32::from_bits(rec.word124), f32::from_bits(after[6]), f32::from_bits(after[7])]);
            }
            let name = match fold {
                Fold::Skip => "weight 0 (the state unchanged)",
                Fold::Copy => "copy (weight 1)",
                Fold::Blend(_) => "blend",
                Fold::Layer(_) => "layer",
            };
            let same = pose::fold(&Pose::from_words(before), &local, fold).to_words() == *after;
            if fold == Fold::Copy {
                copy_nodes += 1;
                copy_different += usize::from(!same);
            }
            *folds.entry(if same { name.to_string() } else { format!("DIFFERENT {name}{}", if root_fixed { " (root fix)" } else { "" }) }).or_default() += 1;
            if !same && shown < 30 {
                shown += 1;
                println!("  tick {} node {i} anim {} place {} {name} weight {} * {node_weight} flags {:x} root fix {root_fixed}: local {:x?}\n    before {before:x?}\n    after  {after:x?}", rec.tick, anim.name, p.place, rec.weight, rec.flags, local.to_words());
            }
        }
    }
    let different: usize = folds.iter().filter(|(k, _)| k.starts_with("DIFFERENT")).map(|(_, n)| n).sum();
    println!("local poses from the channels (the weight-1 copies, {copy_nodes} nodes): DIFFERENT -> {copy_different}");
    println!("node states after the pose (logged weights; {records} poses, every node, all 16 words): DIFFERENT -> {different}; {folds:?}");
    println!("  root-node fix (track flags bits 0 to 2 set on the pose's node 0): {root_fixed_nodes} nodes; not compared: {skipped:?}");
}
