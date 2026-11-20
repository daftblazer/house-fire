//! `mixcheck DUMP`: runs the engine's mixer on the inputs of a recorded AX dump (the recompiled
//! game's `GCN_AXDUMP`, see `tools/axdump.py` for the records) and compares each frame with the
//! mixer the game ran. Prints `mix: frames N, the same M, first different F`, the first few
//! differences, and how often a voice's state after its frame was not the dump's next block of it.
//!
//! The parameter block's word layout is in `housefire::axdump::pb`; the mixer has its own
//! voices.

use housefire::axdump::{Item, pb, replay};
use housefire::mixer::{self, Mixer, Voice};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: mixcheck DUMP");
        std::process::exit(2);
    });
    let t0 = Instant::now();
    let data = std::fs::read(&path).expect("read the dump");
    let read_time = t0.elapsed();

    // Each voice's state after the frame that mixed it, for the check against the next block.
    let mut state: HashMap<u32, Voice> = HashMap::new();
    let mut state_bad = 0usize;
    let mut state_words = [0usize; pb::WORDS];
    let (mut frames, mut same) = (0usize, 0usize);
    let mut first_diff: Option<usize> = None;
    let mut shown = 0;
    let mut mix_time = Duration::ZERO;

    replay(
        &data,
        |_, mixer, aram, items| {
            let t = Instant::now();
            mix_voices(items, mixer, aram, &mut state, &mut state_bad, &mut state_words);
            mix_time += t.elapsed();
        },
        |frame, out| {
            let mut bad = None;
            for (i, (&m, &w)) in out.lr.iter().zip(&out.want_lr).enumerate() {
                if m != w {
                    bad = Some((i / 2, if i % 2 == 0 { "L" } else { "R" }, m as i32, w as i32));
                    break;
                }
            }
            if bad.is_none() {
                for (i, (&m, &w)) in out.surround.iter().zip(&out.want_surround).enumerate() {
                    if m != w {
                        bad = Some((i, "S", m, w));
                        break;
                    }
                }
            }
            match bad {
                None => same += 1,
                Some((i, ch, mine, theirs)) => {
                    first_diff.get_or_insert(frame);
                    if shown < 8 {
                        shown += 1;
                        println!("frame {frame} sample {i} {ch}: mixer {mine}, dump {theirs}");
                    }
                }
            }
            frames += 1;
        },
    );
    let total = t0.elapsed();
    match first_diff {
        Some(f) => println!("mix: frames {frames}, the same {same}, first different {f}"),
        None => println!("mix: frames {frames}, the same {same}, first different none"),
    }
    println!("voice state not as the dump's next block: {state_bad}");
    if state_bad > 0 {
        let by: Vec<String> = state_words
            .iter()
            .enumerate()
            .filter(|&(_, &n)| n > 0)
            .map(|(i, n)| format!("w{i}:{n}"))
            .collect();
        println!("  by word: {}", by.join(" "));
    }
    eprintln!(
        "time: reading the file {:.3}s, mixing voices {:.3}s, all {:.3}s",
        read_time.as_secs_f64(),
        mix_time.as_secs_f64(),
        total.as_secs_f64()
    );
}

/// Command 3: each voice block with its updates, in the list's order.
fn mix_voices(
    items: &[Item],
    mixer: &mut Mixer,
    aram: &mixer::Aram,
    state: &mut HashMap<u32, Voice>,
    state_bad: &mut usize,
    state_words: &mut [usize; pb::WORDS],
) {
    let mut i = 0;
    while i < items.len() {
        let Item::Voice(addr, w) = &items[i] else {
            i += 1;
            continue;
        };
        i += 1;
        let counts: [usize; 5] = std::array::from_fn(|k| w[pb::UPDATE_COUNTS + k] as usize);
        let total: usize = counts.iter().sum();
        // The updates' bytes follow the block as a buffer record.
        let mut upd: &[u8] = &[];
        if total > 0
            && let Some(Item::Buffer(b)) = items.get(i)
        {
            upd = b;
            i += 1;
        }
        let mut v = pb::voice(w);
        // The reference read this block from the guest after writing back its own state.
        if let Some(prev) = state.get(addr) {
            let had = pb::words(prev);
            let now = pb::words(&v);
            let mut any = false;
            for k in 0..pb::WORDS {
                if had[k] != now[k] {
                    state_words[k] += 1;
                    any = true;
                }
            }
            *state_bad += any as usize;
        }
        let mut u = 0;
        for (ms, &n) in counts.iter().enumerate() {
            for _ in 0..n {
                let idx = u16::from_be_bytes([upd[u], upd[u + 1]]) as usize;
                let val = u16::from_be_bytes([upd[u + 2], upd[u + 3]]);
                u += 4;
                if let Some(p) = pb::param_of(idx) {
                    v.apply(p, val);
                }
            }
            mixer.mix_voice_ms(&mut v, ms, aram);
        }
        state.insert(*addr, v);
    }
}
