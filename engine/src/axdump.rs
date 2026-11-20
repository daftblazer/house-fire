//! Reading the recompiled game's AX dump (`GCN_AXDUMP`, see `tools/axdump.py` for the records):
//! the records, and the layout of the game's 93-word voice parameter block, which maps onto the
//! mixer's own [`Voice`]. Only the checks that read the dump (`mixcheck`, `voicecheck`) use it.

use crate::mixer::{ARAM_SIZE, Aram, Mixer, Param, SPF, Voice, bus, new_aram};

/// The parameter block's layout: 93 big-endian words, and which voice field each holds.
pub mod pb {
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
pub enum Rec<'a> {
    Frame,
    Command(Vec<u16>),
    Buffer(&'a [u8]),
    Aram(u32, &'a [u8]),
    Voice(u32, [u16; pb::WORDS]),
    Output(&'a [u8]),
}

pub fn u32le(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

/// The record at `*p` (None at the end, or if the last is cut short).
pub fn next_record<'a>(b: &'a [u8], p: &mut usize) -> Option<Rec<'a>> {
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


/// What one command of a frame took from the guest: its buffers and voices, in the order read.
pub enum Item<'a> {
    Buffer(&'a [u8]),
    Voice(u32, [u16; pb::WORDS]),
}

fn be32(b: &[u8], p: usize) -> u32 {
    u32::from_be_bytes(b[p..p + 4].try_into().unwrap())
}

/// A guest buffer of 160 big-endian words.
fn buffer(b: &[u8]) -> [i32; SPF] {
    let mut o = [0i32; SPF];
    for (x, c) in o.iter_mut().zip(b.as_chunks::<4>().0) {
        *x = i32::from_be_bytes(*c);
    }
    o
}

/// A frame's output as the mixer produced it and as the dump has it.
pub struct FrameOut {
    pub lr: [i16; 2 * SPF],
    pub surround: [i32; SPF],
    pub want_lr: [i16; 2 * SPF],
    pub want_surround: [i32; SPF],
}

/// Replays the dump's commands on a mixer: the setup, the downloads and the CPU's buffers as the
/// game gave them. `voices` is called for the voice command (3) of a frame with the frame's
/// index, the voice blocks and update lists the game read, and must mix its voices into the
/// mixer (the sample memory, with the dump's uploads, is passed along); `output` is called at the
/// end of each frame with the mixer's output and the dump's.
pub fn replay<'a>(
    data: &'a [u8],
    mut voices: impl FnMut(usize, &mut Mixer, &Aram, &[Item<'a>]),
    mut output: impl FnMut(usize, &FrameOut),
) {
    let mut aram = new_aram();
    let mut mixer = Mixer::new();
    let mut items: Vec<Item> = Vec::new();
    let mut frame = 0usize;
    let mut p = 0usize;
    while let Some(rec) = next_record(data, &mut p) {
        match rec {
            Rec::Frame => {}
            Rec::Aram(a, bytes) => {
                for (i, &x) in bytes.iter().enumerate() {
                    aram[(a as usize + i) & (ARAM_SIZE - 1)] = x;
                }
            }
            Rec::Buffer(b) => items.push(Item::Buffer(b)),
            Rec::Voice(a, w) => items.push(Item::Voice(a, w)),
            Rec::Output(o) => {
                let mut out = FrameOut {
                    lr: [0; 2 * SPF],
                    surround: [0; SPF],
                    want_lr: [0; 2 * SPF],
                    want_surround: [0; SPF],
                };
                mixer.output(&mut out.lr, &mut out.surround);
                for (x, c) in out.want_lr.iter_mut().zip(o[..640].as_chunks::<2>().0) {
                    *x = i16::from_le_bytes(*c);
                }
                for (x, c) in out.want_surround.iter_mut().zip(o[640..].as_chunks::<4>().0) {
                    *x = i32::from_le_bytes(*c);
                }
                output(frame, &out);
                frame += 1;
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
                    Some(0x03) => voices(frame, &mut mixer, &aram, &it),
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
}
