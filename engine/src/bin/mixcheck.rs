//! `mixcheck DUMP`: runs the engine's mixer on the inputs of a recorded AX dump (the recompiled
//! game's `GCN_AXDUMP`, see `tools/axdump.py` for the records) and compares each frame with the
//! mixer the game ran. Prints `mix: frames N, the same M, first different F`, the first few
//! differences, and how often a voice's state after its frame was not the dump's next block of it.
//!
//! The parameter block's word layout is known only here (module `pb`); the mixer has its own
//! voices.

use housefire::mixer::{self, Mixer, Param, Voice, bus};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The parameter block's layout: 93 big-endian words, and which voice field each holds.
mod pb {
    use super::{Param, Voice};
    pub const WORDS: usize = 93;
    pub const UPDATE_COUNTS: usize = 34; // five u16 counts, then the updates' address (hi, lo)

    pub fn param_of(i: usize) -> Option<Param> {
        Some(match i {
            4 => Param::SrcType,
            6 => Param::MixerControl,
            7 => Param::Running,
            8 => Param::IsStream,
            9..=26 => {
                let k = ((i - 9) / 2) as u8;
                if i % 2 == 1 { Param::SendVol(k) } else { Param::SendDelta(k) }
            }
            50 => Param::Vol,
            51 => Param::VolDelta,
            55 => Param::Looping,
            56 => Param::Format,
            57 => Param::LoopHi,
            58 => Param::LoopLo,
            59 => Param::EndHi,
            60 => Param::EndLo,
            61 => Param::CurHi,
            62 => Param::CurLo,
            63..=78 => Param::Coef((i - 63) as u8),
            80 => Param::PredScale,
            81 => Param::Yn1,
            82 => Param::Yn2,
            83 => Param::RatioHi,
            84 => Param::RatioLo,
            85 => Param::Frac,
            86..=89 => Param::Last((i - 86) as u8),
            90 => Param::LoopPredScale,
            91 => Param::LoopYn1,
            92 => Param::LoopYn2,
            _ => return None,
        })
    }

    pub fn voice(w: &[u16; WORDS]) -> Voice {
        let mut v = Voice::default();
        for (i, &x) in w.iter().enumerate() {
            if let Some(p) = param_of(i) {
                v.apply(p, x);
            }
        }
        v
    }

    /// The block's words that a voice holds (the others zero).
    pub fn words(v: &Voice) -> [u16; WORDS] {
        let mut w = [0u16; WORDS];
        for (i, x) in w.iter_mut().enumerate() {
            if let Some(p) = param_of(i) {
                *x = v.get(p);
            }
        }
        w
    }
}

/// A record of the dump, borrowing its bytes.
enum Rec<'a> {
    Frame,
    Command(Vec<u16>),
    Buffer(&'a [u8]),
    Aram(u32, &'a [u8]),
    Voice(u32, [u16; pb::WORDS]),
    Output(&'a [u8]),
}

fn u32le(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

/// The record at `*p` (None at the end, or if the last is cut short).
fn next_record<'a>(b: &'a [u8], p: &mut usize) -> Option<Rec<'a>> {
    let t = *b.get(*p)?;
    let q = *p + 1;
    let (rec, end) = match t {
        b'F' => (Rec::Frame, q + 4),
        b'C' => {
            let n = u16::from_le_bytes(b.get(q..q + 2)?.try_into().unwrap()) as usize;
            let body = b.get(q + 2..q + 2 + 2 * n)?;
            let w = body.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
            (Rec::Command(w), q + 2 + 2 * n)
        }
        b'B' | b'A' => {
            let a = u32le(b.get(q..q + 8)?, 0);
            let n = u32le(b, q + 4) as usize;
            let body = b.get(q + 8..q + 8 + n)?;
            (if t == b'B' { Rec::Buffer(body) } else { Rec::Aram(a, body) }, q + 8 + n)
        }
        b'V' => {
            let body = b.get(q..q + 4 + 2 * pb::WORDS)?;
            let mut w = [0u16; pb::WORDS];
            for (x, c) in w.iter_mut().zip(body[4..].as_chunks::<2>().0) {
                *x = u16::from_le_bytes(*c);
            }
            (Rec::Voice(u32le(b, q), w), q + 4 + 2 * pb::WORDS)
        }
        b'O' => (Rec::Output(b.get(q..q + 1280)?), q + 1280),
        _ => panic!("unknown record {t:#x} at {p}"),
    };
    if end > b.len() {
        return None;
    }
    *p = end;
    Some(rec)
}

fn be32(b: &[u8], p: usize) -> u32 {
    u32::from_be_bytes(b[p..p + 4].try_into().unwrap())
}

/// A guest buffer of 160 big-endian words.
fn buffer(b: &[u8]) -> [i32; mixer::SPF] {
    let mut o = [0i32; mixer::SPF];
    for (x, c) in o.iter_mut().zip(b.as_chunks::<4>().0) {
        *x = i32::from_be_bytes(*c);
    }
    o
}

/// What one command of a frame took from the guest: its buffers and voices, in the order read.
enum Item<'a> {
    Buffer(&'a [u8]),
    Voice(u32, [u16; pb::WORDS]),
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: mixcheck DUMP");
        std::process::exit(2);
    });
    let t0 = Instant::now();
    let data = std::fs::read(&path).expect("read the dump");
    let read_time = t0.elapsed();

    let mut aram = mixer::new_aram();
    let mut mixer = Mixer::new();
    let mut items: Vec<Item> = Vec::new();
    // Each voice's state after the frame that mixed it, for the check against the next block.
    let mut state: HashMap<u32, Voice> = HashMap::new();
    let mut state_bad = 0usize;
    let mut state_words = [0usize; pb::WORDS];
    let (mut frames, mut same) = (0usize, 0usize);
    let mut first_diff: Option<usize> = None;
    let mut shown = 0;
    let mut mix_time = Duration::ZERO;

    let mut p = 0usize;
    while let Some(rec) = next_record(&data, &mut p) {
        match rec {
            Rec::Frame => {}
            Rec::Aram(a, bytes) => {
                for (i, &x) in bytes.iter().enumerate() {
                    aram[(a as usize + i) & (mixer::ARAM_SIZE - 1)] = x;
                }
            }
            Rec::Buffer(b) => items.push(Item::Buffer(b)),
            Rec::Voice(a, w) => items.push(Item::Voice(a, w)),
            Rec::Output(o) => {
                let mut lr = [0i16; 2 * mixer::SPF];
                let mut sur = [0i32; mixer::SPF];
                mixer.output(&mut lr, &mut sur);
                let want_lr = o[..640].as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c));
                let want_s = o[640..].as_chunks::<4>().0.iter().map(|&c| i32::from_le_bytes(c));
                let mut bad = None;
                for (i, w) in want_lr.enumerate() {
                    if lr[i] != w {
                        bad = Some((i / 2, if i % 2 == 0 { "L" } else { "R" }, lr[i] as i32, w as i32));
                        break;
                    }
                }
                if bad.is_none() {
                    for (i, w) in want_s.enumerate() {
                        if sur[i] != w {
                            bad = Some((i, "S", sur[i], w));
                            break;
                        }
                    }
                }
                match bad {
                    None => same += 1,
                    Some((i, ch, mine, theirs)) => {
                        first_diff.get_or_insert(frames);
                        if shown < 8 {
                            shown += 1;
                            println!("frame {frames} sample {i} {ch}: mixer {mine}, dump {theirs}");
                        }
                    }
                }
                frames += 1;
            }
            Rec::Command(w) => {
                let it = std::mem::take(&mut items);
                let mut buffers = it.iter().filter_map(|i| match i {
                    Item::Buffer(b) => Some(*b),
                    _ => None,
                });
                match w.first().copied() {
                    Some(0x00) => {
                        let b = buffers.next().expect("setup block");
                        let mut init = [(0i32, 0i16); bus::COUNT];
                        for (k, e) in init.iter_mut().enumerate() {
                            *e = (be32(b, 6 * k) as i32, i16::from_be_bytes([b[6 * k + 4], b[6 * k + 5]]));
                        }
                        mixer.setup(&init);
                    }
                    Some(0x03) => {
                        let t = Instant::now();
                        mix_voices(&it, &mut mixer, &aram, &mut state, &mut state_bad, &mut state_words);
                        mix_time += t.elapsed();
                    }
                    Some(0x07) => mixer.set_main_from(&buffer(buffers.next().unwrap()), false),
                    Some(0x11) => mixer.set_main_from(&buffer(buffers.next().unwrap()), true),
                    Some(0x10) => {
                        // Upload AuxB L/R (nothing to do), download the CPU's AuxB L/R, add to main.
                        let b = buffers.next().expect("download");
                        mixer.set_bus(bus::AUX_B_L, &buffer(&b[..640]));
                        mixer.set_bus(bus::AUX_B_R, &buffer(&b[640..]));
                        mixer.mix_aux_b_into_main();
                    }
                    Some(0x0E | 0x0F | 0x02 | 0x12) => {}
                    Some(c) => panic!("command {c:#04x} is not handled"),
                    None => {}
                }
            }
        }
    }
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
