//! The lamps' lights at their objects' posed nodes (LOCKSTEP_ANIM_LAMPS, stage 4.4f): each lamp
//! light the original placed ('1', Obj_PlaceLights 0x8007A8CC: the light's position after, the
//! node's 16 words) against the bone matrices ('<') of the same tick: is its place a bone's
//! translation, and are the node's words a logged final node state.

use crate::*;

pub fn lamps(log: &housefire::tree_log::Log) {
    let f = |v: u32| f32::from_bits(v);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let ticks: BTreeSet<u32> = log.object_lights.iter().map(|o| o.tick).collect();
    println!("lamp records: {} over ticks {:?}..{:?}; bone records over ticks {:?}..{:?}", log.object_lights.len(), ticks.first(), ticks.last(), log.bones.first().map(|b| b.tick), log.bones.last().map(|b| b.tick));
    for o in &log.object_lights {
        for l in o.lights.iter().filter(|l| l.kind == 0) {
            let near: Vec<&housefire::tree_log::BonesRec> = log.bones.iter().filter(|b| b.tick.abs_diff(o.tick) <= 1).collect();
            if near.is_empty() {
                *counts.entry("no bones that tick".into()).or_default() += 1;
                continue;
            }
            let at_bone = near.iter().any(|b| b.matrices.iter().any(|m| [f(m[12]), f(m[13]), f(m[14])] == l.at));
            let node_words = near.iter().any(|b| b.states.iter().any(|s| s[5..8] == l.words[5..8]));
            let node_is_at = [f(l.words[5]), f(l.words[6]), f(l.words[7])] == l.at;
            *counts.entry(format!("place a bone's translation: {at_bone}; node words a state's: {node_words}; place = node words 5-7: {node_is_at}")).or_default() += 1;
        }
    }
    println!("lamp lights against the bones: {counts:?}");
}
