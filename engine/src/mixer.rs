//! The audio mixer: voices read from sample memory (the console's ARAM) are converted to the
//! 32 kHz output rate, scaled and added into the main, surround and auxiliary buses in frames of
//! 5 ms (160 samples). It gives the results of the recompiled game's own mixer bit for bit (its
//! `hw/ax.cpp`; each rule below names the function it follows), checked against that mixer's
//! dump by the `mixcheck` binary.
//!
//! The voices are this engine's own structs (see [`Voice`]); the reader of the game's parameter
//! block layout is not here, so the mixer can be fed from the game's records or from a native
//! sound system alike. Nothing allocates while mixing.

/// Samples per millisecond at 32 kHz (`SPMS`).
pub const SPMS: usize = 32;
/// Samples per frame of 5 ms (`SPF`).
pub const SPF: usize = 5 * SPMS;
/// Size of the sample memory, a power of two (`ARAM_SIZE`); addresses wrap in it.
pub const ARAM_SIZE: usize = 0x0100_0000;

/// The sample memory.
pub type Aram = [u8; ARAM_SIZE];

/// A new zeroed sample memory.
pub fn new_aram() -> Box<Aram> {
    vec![0u8; ARAM_SIZE].into_boxed_slice().try_into().expect("ARAM size")
}

/// The buses, indexes into [`Mixer::bus`] (the order `setup` fills them in).
pub mod bus {
    pub const MAIN_L: usize = 0;
    pub const MAIN_R: usize = 1;
    pub const SURROUND: usize = 2;
    pub const AUX_A_L: usize = 3;
    pub const AUX_A_R: usize = 4;
    pub const AUX_A_S: usize = 5;
    pub const AUX_B_L: usize = 6;
    pub const AUX_B_R: usize = 7;
    pub const AUX_B_S: usize = 8;
    pub const COUNT: usize = 9;
}

/// The number of send levels of a voice.
pub const SENDS: usize = 9;
// Index of each send in a voice's `send_vol`/`send_delta` (the order the game keeps them in).
const SEND_MAIN_L: usize = 0;
const SEND_MAIN_R: usize = 1;
const SEND_AUX_A_L: usize = 2;
const SEND_AUX_A_R: usize = 3;
const SEND_AUX_B_L: usize = 4;
const SEND_AUX_B_R: usize = 5;
const SEND_AUX_B_S: usize = 6;
const SEND_SURROUND: usize = 7;
const SEND_AUX_A_S: usize = 8;

/// How a voice's samples are stored (`Accel::sample`'s format switch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// 4-bit DSP ADPCM, addresses in nibbles (code 0).
    #[default]
    Adpcm,
    /// 16-bit PCM, addresses in samples (code 0x0A).
    Pcm16,
    /// 8-bit PCM, addresses in bytes (code 0x19).
    Pcm8,
    /// Any other code: the voice yields silence without stopping.
    Other(u16),
}

impl Format {
    pub fn from_code(c: u16) -> Format {
        match c {
            0x00 => Format::Adpcm,
            0x0A => Format::Pcm16,
            0x19 => Format::Pcm8,
            c => Format::Other(c),
        }
    }
    pub fn code(self) -> u16 {
        match self {
            Format::Adpcm => 0x00,
            Format::Pcm16 => 0x0A,
            Format::Pcm8 => 0x19,
            Format::Other(c) => c,
        }
    }
}

/// One voice: its source, its rate conversion, its own volume and its sends.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Voice {
    pub running: bool,
    pub format: Format,
    pub looping: bool,
    /// A streamed voice keeps its history across a loop (`Accel::sample`).
    pub is_stream: bool,
    /// Rate conversion: 2 holds each sample, anything else interpolates linearly.
    pub src_type: u16,
    /// Which sends are on and whether levels ramp (see [`Mixer::mix_voice_ms`]).
    pub mixer_control: u16,
    /// Current address: nibbles for ADPCM, samples for PCM16, bytes for PCM8.
    pub cur: u32,
    pub end: u32,
    pub loop_addr: u32,
    /// ADPCM coefficient pairs.
    pub coefs: [i16; 16],
    /// ADPCM header of the frame being decoded (the predictor in bits 4-6, the scale in 0-3).
    pub pred_scale: u16,
    pub yn1: i16,
    pub yn2: i16,
    /// The header and history a loop restarts from.
    pub loop_pred_scale: u16,
    pub loop_yn1: i16,
    pub loop_yn2: i16,
    /// Input samples per output sample, 16.16 fixed point, and the fraction carried over.
    pub ratio: u32,
    pub frac: u16,
    /// The last four input samples (the interpolation's history).
    pub last: [i16; 4],
    /// The voice's own level (1.15) and its change per output sample.
    pub vol: u16,
    pub vol_delta: u16,
    /// Level and change per output sample of each send (see the `SEND_*` order).
    pub send_vol: [u16; SENDS],
    pub send_delta: [u16; SENDS],
}

/// One field of a [`Voice`] a per-millisecond update can set; 32-bit fields are set by halves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    SrcType,
    MixerControl,
    Running,
    IsStream,
    SendVol(u8),
    SendDelta(u8),
    Vol,
    VolDelta,
    Looping,
    Format,
    LoopHi,
    LoopLo,
    EndHi,
    EndLo,
    CurHi,
    CurLo,
    Coef(u8),
    PredScale,
    Yn1,
    Yn2,
    RatioHi,
    RatioLo,
    Frac,
    Last(u8),
    LoopPredScale,
    LoopYn1,
    LoopYn2,
}

fn set_hi(v: &mut u32, x: u16) {
    *v = (*v & 0xFFFF) | ((x as u32) << 16);
}
fn set_lo(v: &mut u32, x: u16) {
    *v = (*v & 0xFFFF_0000) | x as u32;
}

impl Voice {
    /// Set one field, as a parameter update does (`process_pb_list`'s updates).
    pub fn apply(&mut self, p: Param, x: u16) {
        match p {
            Param::SrcType => self.src_type = x,
            Param::MixerControl => self.mixer_control = x,
            Param::Running => self.running = x != 0,
            Param::IsStream => self.is_stream = x != 0,
            Param::SendVol(i) => self.send_vol[i as usize] = x,
            Param::SendDelta(i) => self.send_delta[i as usize] = x,
            Param::Vol => self.vol = x,
            Param::VolDelta => self.vol_delta = x,
            Param::Looping => self.looping = x != 0,
            Param::Format => self.format = Format::from_code(x),
            Param::LoopHi => set_hi(&mut self.loop_addr, x),
            Param::LoopLo => set_lo(&mut self.loop_addr, x),
            Param::EndHi => set_hi(&mut self.end, x),
            Param::EndLo => set_lo(&mut self.end, x),
            Param::CurHi => set_hi(&mut self.cur, x),
            Param::CurLo => set_lo(&mut self.cur, x),
            Param::Coef(i) => self.coefs[i as usize] = x as i16,
            Param::PredScale => self.pred_scale = x,
            Param::Yn1 => self.yn1 = x as i16,
            Param::Yn2 => self.yn2 = x as i16,
            Param::RatioHi => set_hi(&mut self.ratio, x),
            Param::RatioLo => set_lo(&mut self.ratio, x),
            Param::Frac => self.frac = x,
            Param::Last(i) => self.last[i as usize] = x as i16,
            Param::LoopPredScale => self.loop_pred_scale = x,
            Param::LoopYn1 => self.loop_yn1 = x as i16,
            Param::LoopYn2 => self.loop_yn2 = x as i16,
        }
    }

    /// The value of one field, as [`Voice::apply`] takes it (a 32-bit field by halves).
    pub fn get(&self, p: Param) -> u16 {
        match p {
            Param::SrcType => self.src_type,
            Param::MixerControl => self.mixer_control,
            Param::Running => self.running as u16,
            Param::IsStream => self.is_stream as u16,
            Param::SendVol(i) => self.send_vol[i as usize],
            Param::SendDelta(i) => self.send_delta[i as usize],
            Param::Vol => self.vol,
            Param::VolDelta => self.vol_delta,
            Param::Looping => self.looping as u16,
            Param::Format => self.format.code(),
            Param::LoopHi => (self.loop_addr >> 16) as u16,
            Param::LoopLo => self.loop_addr as u16,
            Param::EndHi => (self.end >> 16) as u16,
            Param::EndLo => self.end as u16,
            Param::CurHi => (self.cur >> 16) as u16,
            Param::CurLo => self.cur as u16,
            Param::Coef(i) => self.coefs[i as usize] as u16,
            Param::PredScale => self.pred_scale,
            Param::Yn1 => self.yn1 as u16,
            Param::Yn2 => self.yn2 as u16,
            Param::RatioHi => (self.ratio >> 16) as u16,
            Param::RatioLo => self.ratio as u16,
            Param::Frac => self.frac,
            Param::Last(i) => self.last[i as usize] as u16,
            Param::LoopPredScale => self.loop_pred_scale,
            Param::LoopYn1 => self.loop_yn1 as u16,
            Param::LoopYn2 => self.loop_yn2 as u16,
        }
    }

    /// The next input sample, with its decode, the end of the data and the loop
    /// (`Accel::sample`). `cur` is the read position held by the caller for the millisecond and
    /// `stopped` the millisecond's end-of-data flag.
    #[inline]
    fn fetch(&mut self, cur: &mut u32, stopped: &mut bool, aram: &Aram) -> i16 {
        const MASK: usize = ARAM_SIZE - 1;
        if *stopped {
            return 0;
        }
        let val: i32 = match self.format {
            Format::Adpcm => {
                if *cur & 15 == 0 {
                    self.pred_scale = aram[(*cur >> 1) as usize & MASK] as u16;
                    *cur = cur.wrapping_add(2);
                }
                let ps = self.pred_scale;
                let scale = 1i32 << (ps & 0xF);
                let ci = ((ps >> 4) & 7) as usize;
                let c1 = self.coefs[ci * 2] as i32;
                let c2 = self.coefs[ci * 2 + 1] as i32;
                let b = aram[(*cur >> 1) as usize & MASK];
                let mut t = (if *cur & 1 != 0 { b & 0xF } else { b >> 4 }) as i32;
                if t >= 8 {
                    t -= 16;
                }
                let acc = 0x400i32
                    .wrapping_add(c1.wrapping_mul(self.yn1 as i32))
                    .wrapping_add(c2.wrapping_mul(self.yn2 as i32));
                let v = (scale * t + (acc >> 11)).clamp(-0x7FFF, 0x7FFF);
                self.yn2 = self.yn1;
                self.yn1 = v as i16;
                *cur = cur.wrapping_add(1);
                v
            }
            Format::Pcm16 => {
                let a = (*cur as usize).wrapping_mul(2);
                let v = i16::from_be_bytes([aram[a & MASK], aram[a.wrapping_add(1) & MASK]]) as i32;
                self.yn2 = self.yn1;
                self.yn1 = v as i16;
                *cur = cur.wrapping_add(1);
                v
            }
            Format::Pcm8 => {
                let v = ((aram[*cur as usize & MASK] as u16) << 8) as i16 as i32;
                self.yn2 = self.yn1;
                self.yn1 = v as i16;
                *cur = cur.wrapping_add(1);
                v
            }
            Format::Other(_) => {
                *stopped = true;
                return 0;
            }
        };
        if *cur > self.end {
            if self.looping {
                *cur = self.loop_addr;
                self.pred_scale = self.loop_pred_scale;
                if !self.is_stream {
                    self.yn1 = self.loop_yn1;
                    self.yn2 = self.loop_yn2;
                }
            } else {
                self.running = false;
                *stopped = true;
            }
        }
        val as i16
    }
}

/// Add `in_` into `out` scaled by a level (1.15) that may ramp by `delta` per sample, each
/// product clamped to +-32767 (`mix_add`); returns the level after the block.
#[inline]
fn mix_add(out: &mut [i32; SPMS], in_: &[i16; SPMS], mut vol: u16, delta: u16) -> u16 {
    for (o, &s) in out.iter_mut().zip(in_) {
        let p = ((s as i64 * vol as i64) >> 15) as i32;
        *o += p.clamp(-32767, 32767);
        vol = vol.wrapping_add(delta);
    }
    vol
}

/// The buses and the work on them.
pub struct Mixer {
    buses: [[i32; SPF]; bus::COUNT],
}

impl Default for Mixer {
    fn default() -> Self {
        Mixer { buses: [[0; SPF]; bus::COUNT] }
    }
}

impl Mixer {
    pub fn new() -> Mixer {
        Mixer::default()
    }

    /// One bus's frame (`bus::*`).
    pub fn bus(&self, b: usize) -> &[i32; SPF] {
        &self.buses[b]
    }

    /// Start a frame: each bus a ramp from `init` by `delta` per sample (`setup`); the game
    /// sends zero for both.
    pub fn setup(&mut self, init: &[(i32, i16); bus::COUNT]) {
        for (b, &(mut v, d)) in self.buses.iter_mut().zip(init) {
            for s in b.iter_mut() {
                *s = v;
                v = v.wrapping_add(d as i32);
            }
        }
    }

    /// Replace a bus with the given frame (`set_bufs`).
    pub fn set_bus(&mut self, b: usize, src: &[i32; SPF]) {
        self.buses[b] = *src;
    }

    /// Add a frame into a bus (`add_bufs`).
    pub fn add_bus(&mut self, b: usize, src: &[i32; SPF]) {
        for (o, &s) in self.buses[b].iter_mut().zip(src) {
            *o = o.wrapping_add(s);
        }
    }

    /// The main buses become one mono frame, L = R = in (or L = -in), the surround cleared
    /// (`set_main_from`).
    pub fn set_main_from(&mut self, src: &[i32; SPF], opposite: bool) {
        for (l, &v) in self.buses[bus::MAIN_L].iter_mut().zip(src) {
            *l = if opposite { v.wrapping_neg() } else { v };
        }
        self.buses[bus::MAIN_R] = *src;
        self.buses[bus::SURROUND] = [0; SPF];
    }

    /// Add three downloaded buffers into each of the main, AuxA and AuxB groups (L, R, S), each
    /// group scaled by its own level (`download_and_mix`).
    pub fn download_and_mix(&mut self, src: &[[i32; SPF]; 3], vols: [u16; 3]) {
        for (g, &vol) in vols.iter().enumerate() {
            for (b, s) in src.iter().enumerate() {
                for (o, &x) in self.buses[g * 3 + b].iter_mut().zip(s) {
                    *o = o.wrapping_add(((x as i64 * vol as i64) >> 15) as i32);
                }
            }
        }
    }

    /// Add the AuxB left and right into the main ones (the end of command 0x10).
    pub fn mix_aux_b_into_main(&mut self) {
        let [l, r, _, _, _, _, bl, br, _] = &mut self.buses;
        for (o, &x) in l.iter_mut().zip(bl.iter()) {
            *o = o.wrapping_add(x);
        }
        for (o, &x) in r.iter_mut().zip(br.iter()) {
            *o = o.wrapping_add(x);
        }
    }

    /// Mix one millisecond (`ms` of 0 to 4) of a voice into the buses (`process_voice`): its
    /// samples are fetched, converted to the output rate, scaled by its own level and added to
    /// each send. `mixer_control` bit 3 makes the send levels ramp; bit 4 selects the
    /// Pro Logic II layout, where AuxB is on when bits 1-2 are clear, AuxA when the low three
    /// bits are 1 and there is no surround; otherwise bit 0 is AuxA, bit 1 AuxB, bit 2 surround.
    pub fn mix_voice_ms(&mut self, v: &mut Voice, ms: usize, aram: &Aram) {
        if !v.running {
            return;
        }
        let mut samples = [0i16; SPMS];
        let mut cur = v.cur;
        let mut stopped = false;
        // Sample rate conversion: linear interpolation (the reference's, as for Dolphin's
        // linear/polyphase); the product wraps in 32 bits as the reference's does.
        let ratio = v.ratio;
        let mut pos = v.frac as u32;
        let hold = v.src_type == 2;
        let mut last = v.last;
        for s in samples.iter_mut() {
            let frac = (pos & 0xFFFF) as i32;
            *s = if hold {
                last[3]
            } else {
                let d = (last[3] as i32 - last[2] as i32).wrapping_mul(frac) >> 16;
                (last[2] as i32 + d) as i16
            };
            pos = pos.wrapping_add(ratio);
            while pos >= 0x10000 {
                last = [last[1], last[2], last[3], 0];
                last[3] = v.fetch(&mut cur, &mut stopped, aram);
                pos -= 0x10000;
            }
        }
        v.last = last;
        v.frac = pos as u16;
        v.cur = cur;
        // The voice's own level, with its ramp.
        for s in samples.iter_mut() {
            let p = ((*s as i32 * v.vol as i32) >> 15).clamp(-32767, 32767);
            *s = p as i16;
            v.vol = v.vol.wrapping_add(v.vol_delta);
        }
        let mc = v.mixer_control;
        let ramp = mc & 8 != 0;
        let pl2 = mc & 0x10 != 0;
        let (aux_a, aux_b, surround) = if pl2 {
            ((mc & 7) == 1, (mc & 6) == 0, false)
        } else {
            (mc & 1 != 0, mc & 2 != 0, mc & 4 != 0)
        };
        let off = ms * SPMS;
        let mut send = |b: usize, idx: usize| {
            let out: &mut [i32; SPMS] =
                (&mut self.buses[b][off..off + SPMS]).try_into().expect("a millisecond");
            let delta = if ramp { v.send_delta[idx] } else { 0 };
            v.send_vol[idx] = mix_add(out, &samples, v.send_vol[idx], delta);
        };
        send(bus::MAIN_L, SEND_MAIN_L);
        send(bus::MAIN_R, SEND_MAIN_R);
        if aux_a {
            send(bus::AUX_A_L, SEND_AUX_A_L);
            send(bus::AUX_A_R, SEND_AUX_A_R);
            if pl2 {
                send(bus::AUX_A_S, SEND_AUX_A_S);
            }
        }
        if aux_b {
            send(bus::AUX_B_L, SEND_AUX_B_L);
            send(bus::AUX_B_R, SEND_AUX_B_R);
        }
        if surround {
            send(bus::SURROUND, SEND_SURROUND);
            if aux_a {
                send(bus::AUX_A_S, SEND_AUX_A_S);
            }
            if aux_b {
                send(bus::AUX_B_S, SEND_AUX_B_S);
            }
        }
    }

    /// The frame as played (`output`): main L and R clamped to +-32767 and interleaved, and the
    /// surround as mixed.
    pub fn output(&self, lr: &mut [i16; 2 * SPF], surround: &mut [i32; SPF]) {
        for i in 0..SPF {
            lr[2 * i] = self.buses[bus::MAIN_L][i].clamp(-32767, 32767) as i16;
            lr[2 * i + 1] = self.buses[bus::MAIN_R][i].clamp(-32767, 32767) as i16;
        }
        *surround = self.buses[bus::SURROUND];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unity_voice() -> Voice {
        let mut v = Voice { running: true, vol: 0x8000, ratio: 0x10000, ..Voice::default() };
        v.send_vol[SEND_MAIN_L] = 0x8000;
        v.send_vol[SEND_MAIN_R] = 0x8000;
        v
    }

    #[test]
    fn adpcm_at_unit_ratio() {
        // Header 0 (scale 1, predictor 0, zero coefficients), then nibbles 1..6, -1, -2.
        let mut aram = new_aram();
        aram[..5].copy_from_slice(&[0x00, 0x12, 0x34, 0x56, 0xFE]);
        let mut v = unity_voice();
        v.end = 1000;
        let mut m = Mixer::new();
        m.mix_voice_ms(&mut v, 0, &aram);
        // The interpolation holds two samples back: 0, 0, then the decoded 1, 2, 3, ...
        let want = [0, 0, 1, 2, 3, 4, 5, 6, -1, -2];
        assert_eq!(&m.bus(bus::MAIN_L)[..10], &want);
        assert_eq!(&m.bus(bus::MAIN_R)[..10], &want);
        // 32 nibbles read, and a header skipped at each 16 (nibbles 0 and 16 and 32).
        assert_eq!(v.cur, 38);
    }

    #[test]
    fn volume_ramp() {
        let mut aram = new_aram();
        for i in 0..40 {
            aram[2 * i..2 * i + 2].copy_from_slice(&0x4000i16.to_be_bytes());
        }
        let mut v = unity_voice();
        v.format = Format::Pcm16;
        v.end = 1000;
        v.mixer_control = 8;
        v.send_vol[SEND_MAIN_L] = 0;
        v.send_delta[SEND_MAIN_L] = 0x400;
        let mut m = Mixer::new();
        m.mix_voice_ms(&mut v, 0, &aram);
        // The held samples are 0x4000 from the third on; the level rises 0x400 a sample.
        assert_eq!(m.bus(bus::MAIN_L)[1], 0);
        assert_eq!(m.bus(bus::MAIN_L)[5], 5 * 0x200);
        assert_eq!(v.send_vol[SEND_MAIN_L], 32 * 0x400);
        assert_eq!(m.bus(bus::MAIN_R)[5], 0x4000);
    }

    #[test]
    fn end_stops_a_voice() {
        let mut aram = new_aram();
        aram[..4].copy_from_slice(&[0x00, 0x11, 0x11, 0x11]);
        let mut v = unity_voice();
        v.end = 5;
        let mut m = Mixer::new();
        m.mix_voice_ms(&mut v, 0, &aram);
        assert!(!v.running);
    }
}
