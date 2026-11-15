//! The rotation stream studies (LOCKSTEP_ANIM_ROTATIONS): each logged rotation read ('!') against
//! the stream decoded from the animation's bits alone (`anim::rotation_header`, `RotHeader`,
//! `RotStream`).

use crate::*;
use housefire::anim::{self, RotHeader, RotStream};

/// The bits a rotation stream's header takes before its first record: the key count (20) and
/// three widths (5 each).
const HEADER_BITS: u32 = 35;

/// A logged stream's header words with the fields that change as it runs (its bits object, the
/// window's frames and key, the blend flag, the last place) left out: what names the stream.
fn identity(h: &[u32; 12]) -> [u32; 12] {
    let mut k = *h;
    for i in [0, 2, 3, 5, 11] {
        k[i] = 0;
    }
    k[8] &= 0xff00_0000; // +33 is the blend flag, +34 and +35 not the header
    k
}

/// Whether a decoded header is the logged one, every field.
fn header_is_logged(x: &RotHeader, words: &[u32], h: &[u32; 12]) -> bool {
    x.first_record == h[1]
        && x.keys == h[6]
        && x.frames(words) == h[4]
        && x.record_bits as i32 == (h[7] >> 16) as i16 as i32
        && x.gap_bits == (h[7] >> 8) & 0xff
        && x.point_bits == h[7] & 0xff
        && x.bias_bits == h[8] >> 24
        && x.point_scale.to_bits() == h[9]
        && x.bias_scale.to_bits() == h[10]
}

pub fn rotations(log: &housefire::tree_log::Log, d: &Disc) {
    let anims = d.read("animatio.arc").unwrap_or_default();
    let all: Vec<anim::Anim> = housefire::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| anim::read(housefire::arc::bytes(&anims, e))).collect();
    println!("rotation reads: {} in {} animations", log.rot_streams.len(), all.len());
    let flag_set = log.flag_streams.iter().filter(|f| f.result != 0).count();
    println!("flag channel reads: {} ({} set, {} streams); scalar channel reads: {} ({} streams)", log.flag_streams.len(), flag_set, log.flag_streams.iter().map(|f| f.stream).collect::<std::collections::BTreeSet<_>>().len(), log.scalar_streams.len(), log.scalar_streams.iter().map(|f| f.stream).collect::<std::collections::BTreeSet<_>>().len());
    // every animation's streams named by a node word: the rotation header at that bit
    let mut named: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
    for (i, a) in all.iter().enumerate() {
        for nd in &a.nodes {
            for (wi, w) in nd.iter().enumerate() {
                if let anim::Channel::Stream(at) = anim::Channel::of(*w) {
                    let x = anim::rotation_header(&a.bit_words, at);
                    named.entry((x.first_record, x.keys)).or_default().push((i, wi));
                }
            }
        }
    }

    // 1. each logged stream's header decoded from the bits where the stream starts
    let mut found: HashMap<[u32; 12], Vec<(usize, RotHeader)>> = HashMap::new();
    let mut cmp: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.rot_streams {
        let key = identity(&v.head);
        if found.contains_key(&key) {
            continue;
        }
        let h = &v.head;
        let start = h[1].wrapping_sub(HEADER_BITS);
        let near: Vec<(usize, RotHeader)> = all
            .iter()
            .enumerate()
            .map(|(i, a)| (i, anim::rotation_header(&a.bit_words, start)))
            .filter(|(_, x)| x.keys == h[6] && x.first_record == h[1])
            .collect();
        let same: Vec<(usize, RotHeader)> = near.iter().copied().filter(|(i, x)| header_is_logged(x, &all[*i].bit_words, h)).collect();
        let what = match (near.is_empty(), same.is_empty()) {
            (true, _) => "no animation's bits give its start and key count",
            (false, true) => "DIFFERENT",
            _ => "every field the same",
        };
        *cmp.entry(what.into()).or_default() += 1;
        if what == "every field the same" {
            let by: BTreeSet<usize> = same.iter().flat_map(|(i, x)| named.get(&(x.first_record, x.keys)).into_iter().flatten().filter(move |(j, _)| j == i).map(|(_, wi)| *wi)).collect();
            *cmp.entry(if by.is_empty() { "start named by no node word".into() } else { format!("start named by node word {by:?}") }).or_default() += 1;
            if same.len() > 1 {
                *cmp.entry("(several animations with the same header)".into()).or_default() += 1;
            }
        }
        if what == "DIFFERENT" && shown < 6 {
            shown += 1;
            println!("  header different: logged {h:x?}; native {:?}", near.iter().map(|(_, x)| x).collect::<Vec<_>>());
        }
        found.insert(key, same);
    }
    let different = cmp.get("DIFFERENT").copied().unwrap_or(0);
    println!("rotation headers from the animation alone ({} streams): DIFFERENT -> {different}; {cmp:?}", found.len());

    // 2. each logged window from the animation alone, given the logged key (+20): the frames,
    // the points and (not blended, mode 0) the control points
    let eq = |a: &[f32; 4], b: &[u32]| a.iter().zip(b).all(|(p, q)| p.to_bits() == *q);
    let mut win: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    let mut chosen: HashMap<[u32; 12], Option<(usize, RotHeader)>> = HashMap::new();
    for v in &log.rot_streams {
        let key = identity(&v.head);
        let Some(cands) = found.get(&key).filter(|c| !c.is_empty()) else {
            *win.entry("stream not found".into()).or_default() += 1;
            continue;
        };
        let k = v.head[5];
        // the animation whose window has the logged points (a header can fit several)
        let mut fit = None;
        let mut different = None;
        for (i, x) in cands {
            let (first, last, w) = x.window(&all[*i].bit_words, k);
            let ok = first == v.head[2] as i32 && last == v.head[3] as i32 && eq(&w[0], &v.window[0..4]) && eq(&w[3], &v.window[12..16]) && (v.mode != 0 || (eq(&w[1], &v.window[4..8]) && eq(&w[2], &v.window[8..12])));
            if ok {
                fit = Some((*i, *x));
                break;
            }
            different.get_or_insert((first, last, w));
        }
        let Some((i, x)) = fit else {
            *win.entry(format!("mode {}: window DIFFERENT", v.mode.min(1))).or_default() += 1;
            if shown < 6 {
                shown += 1;
                let (first, last, w) = different.unwrap_or((0, 0, [[0.0; 4]; 4]));
                println!("  window different: key {k} of {}, logged ({}, {}, {:08x?}) mode {}; native ({first}, {last}, {:08x?})", v.head[6], v.head[2], v.head[3], v.window, v.mode, w.map(|q| q.map(f32::to_bits)));
            }
            continue;
        };
        chosen.entry(key).or_insert(Some((i, x)));
        *win.entry(format!("mode {}: window the same to the bit", v.mode.min(1))).or_default() += 1;
    }
    let different: usize = win.iter().filter(|(k, _)| k.contains("DIFFERENT") || k.contains("not found")).map(|(_, n)| n).sum();
    println!("rotation windows from the animation alone ({} reads): DIFFERENT -> {different}; {win:?}", log.rot_streams.len());

    // 3. every read run through `anim::RotStream` from the place alone; a stream whose address
    // shows another header is a new stream there
    let mut runs: HashMap<u32, ([u32; 12], RotStream)> = HashMap::new();
    let mut run: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for v in &log.rot_streams {
        let key = identity(&v.head);
        let Some(Some((ai, x))) = chosen.get(&key) else {
            *run.entry("stream not decoded".into()).or_default() += 1;
            continue;
        };
        let words = &all[*ai].bit_words;
        let e = runs.entry(v.stream).or_insert_with(|| (key, RotStream::new(words, *x)));
        let fresh = e.0 != key;
        let first_seen = e.1.last_place == f32::MAX && e.1.key == 0 && !fresh;
        if fresh {
            *e = (key, RotStream::new(words, *x));
        }
        let kind = if fresh { "after another header" } else if first_seen { "first at its address" } else { "running" };
        let before = e.1.clone();
        let got = e.1.read(words, v.place);
        let st = e.1.clone();
        let same_window = st.key == v.head[5] && st.first == v.head[2] as i32 && st.last == v.head[3] as i32 && st.mode == v.mode;
        let same = got.iter().zip(&v.result).all(|(a, b)| a.to_bits() == b.to_bits());
        let what = match (same_window, same) {
            (true, true) => "window and result the same",
            (true, false) => "window the same, result DIFFERENT",
            _ => "window DIFFERENT",
        };
        *run.entry(format!("{what} ({kind})")).or_default() += 1;
        if what != "window and result the same" {
            if shown < 8 {
                shown += 1;
                println!("  read different: place {} logged key {} ({}, {}) mode {}; native key {} ({}, {}) mode {}; before key {} ({}, {}) last place {}", v.place, v.head[5], v.head[2] as i32, v.head[3] as i32, v.mode, st.key, st.first, st.last, st.mode, before.key, before.first, before.last, before.last_place);
            }
            // resync to the log's window (a start the log does not show)
            let mut fixed = RotStream::new(words, *x);
            fixed.key = v.head[5];
            fixed.first = v.head[2] as i32;
            fixed.last = v.head[3] as i32;
            fixed.mode = v.mode;
            fixed.window = core::array::from_fn(|i| core::array::from_fn(|j| f32::from_bits(v.window[4 * i + j])));
            fixed.last_place = v.place;
            if let Some(e) = runs.get_mut(&v.stream) {
                e.1 = fixed;
            }
        }
    }
    let different: usize = run.iter().filter(|(k, _)| k.contains("DIFFERENT") || k.contains("not decoded")).map(|(_, n)| n).sum();
    println!("rotation reads from the place alone ({} reads): DIFFERENT -> {different}; {run:?}", log.rot_streams.len());
}
