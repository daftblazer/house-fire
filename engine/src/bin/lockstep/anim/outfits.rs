//! The Sims' outfits (LOCKSTEP_ANIM_OUTFITS, stage 4.4e): each Sim dressed ('?', Sim_SetModels
//! 800591CC) with its description's words and the model part slots it set, the slots named by
//! the models.arc entries whose name hash they are.

use crate::*;
use housefire::shader::name_hash;

/// The Sim's model part slots: words +1088..+1132 (the record holds +1020 on).
const SLOTS: core::ops::Range<usize> = (1088 - 1020) / 4..(1136 - 1020) / 4;

pub fn outfits(log: &housefire::tree_log::Log, d: &Disc) {
    let models = d.read("models.arc").unwrap_or_default();
    let by_hash: HashMap<u32, String> = housefire::arc::entries(&models).unwrap_or_default().into_iter().map(|e| (name_hash(&e.name), e.name)).collect();
    println!("Sims dressed: {}", log.dressed.len());
    for r in &log.dressed {
        let slots: Vec<String> = r.sim_words[SLOTS].iter().map(|&h| if h == 0 { "-".into() } else { by_hash.get(&h).cloned().unwrap_or_else(|| format!("{h:08x}")) }).collect();
        println!("  tick {} sim {:08x} description {:08x} {:08x?}", r.tick, r.sim, r.description, r.description_words);
        println!("    slots +1088..: {slots:?}");
    }
}
