//! The animation load study: `anim::Loads` against the original's load asks.

use crate::*;

pub fn loads(log: &housefire::tree_log::Log, d: &Disc) {
    let house: u32 = std::env::var("LOCKSTEP_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let anims = d.read("animatio.arc").unwrap_or_default();
    let ids: std::collections::HashSet<u32> = housefire::arc::entries(&anims).unwrap_or_default().iter().map(|e| e.id).collect();
    let ds = d.read("datasets.arc").unwrap_or_default();
    let sets: Vec<Vec<u8>> = housefire::arc::entries(&ds)
        .unwrap_or_default()
        .iter()
        .filter(|e| e.name == "House1Preload" || e.name == format!("housepre{house:02}"))
        .map(|e| housefire::arc::bytes(&ds, e).to_vec())
        .collect();
    let refs: Vec<&[u8]> = sets.iter().map(|v| v.as_slice()).collect();
    let mut loads = housefire::anim::Loads::with_sets(&refs, &ids);
    let (mut same, mut differ) = (0, 0);
    for l in log.loaded.iter().filter(|l| l.caller == 0x800A_C7E4) {
        if loads.ask(l.id, l.tick) == l.loaded {
            same += 1;
        } else {
            differ += 1;
            println!("  load ask differs: tick {} id {:08x}: the original's {}", l.tick, l.id, l.loaded);
        }
    }
    println!("animate loads natively (sets {}): {same} asks answered as the original's, {differ} not", sets.len());
}
