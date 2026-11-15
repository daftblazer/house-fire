//! The flag stream studies (LOCKSTEP_ANIM_FLAGS): each logged flag read ('~') against the
//! stream decoded from the animation's bits alone (`anim::FlagStream`). The flag stream is
//! only its start bit (801BD4C8), the node's flag word (word 3, +16) negated.

use crate::*;
use housefire::anim::{self, FlagStream};

pub fn flags(log: &housefire::tree_log::Log, d: &Disc) {
    let anims = d.read("animatio.arc").unwrap_or_default();
    let all: Vec<anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| anim::read(housefire::arc::bytes(&anims, e))).collect();
    // every animation's flag streams by their start bit
    let mut starts: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, a) in all.iter().enumerate() {
        for nd in &a.nodes {
            if let Some(f) = FlagStream::of(nd[3]) {
                starts.entry(f.start).or_default().push(i);
            }
        }
    }
    // 1. the logged +4 (the start bit) against the animations' flag words; +8 and +12 listed
    let mut seen: BTreeSet<(u32, [u32; 4])> = BTreeSet::new();
    let (mut headers, mut different) = (0, 0);
    let mut extra: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.flag_streams {
        if !seen.insert((v.stream, v.head)) {
            continue;
        }
        headers += 1;
        let h = &v.head;
        if !starts.contains_key(&h[1]) {
            different += 1;
            if shown < 6 {
                shown += 1;
                println!("  flag header different: logged {h:x?}: no animation's node word 3 names the start");
            }
        }
        *extra.entry((h[2], h[3])).or_default() += 1;
    }
    println!("flag headers from the animation alone ({headers} streams; the start named by node word 3): DIFFERENT -> {different}; words +8/+12: {} kinds", extra.len());
    for ((a, b), n) in extra.iter().take(4) {
        println!("  +8 {a:#x}, +12 {b:#x}: {n}");
    }

    // 2. each read from the place alone: the animations whose flag stream starts there, any of
    // whose bits at the place is the logged one
    let mut res: BTreeMap<&str, usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.flag_streams {
        let cands = starts.get(&v.head[1]).map(Vec::as_slice).unwrap_or(&[]);
        let f = FlagStream { start: v.head[1] };
        let right = cands.iter().filter(|&&i| f.read(&all[i].bit_words, all[i].bits, v.place) == (v.result != 0)).count();
        let what = match (cands.len(), right) {
            (0, _) => "stream not found",
            (_, 0) => "DIFFERENT",
            (n, r) if n == r => "the same (every animation)",
            _ => "the same (one of several animations)",
        };
        *res.entry(what).or_default() += 1;
        if what == "DIFFERENT" && shown < 6 {
            shown += 1;
            println!("  flag read different: stream {:#x} place {} logged {}", v.stream, v.place, v.result);
        }
    }
    let different: usize = res.iter().filter(|(k, _)| **k == "DIFFERENT" || k.contains("not found")).map(|(_, n)| n).sum();
    println!("flag reads from the place alone ({} reads): DIFFERENT -> {different}; {res:?}", log.flag_streams.len());
}
