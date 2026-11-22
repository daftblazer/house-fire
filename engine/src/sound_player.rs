//! The engine's sound voices: the game's sound calls (acquire, play, stop, params) become
//! [`mixer`](crate::mixer) voices with the settings the original's sound layer gives its AX
//! voices. Presentation side only: nothing here reads or changes the simulation.
//!
//! Every rule below was read off the home-lot AX dump and the observer log of the same run
//! (`voicecheck LOG DUMP ISO` measures them); the voices are this engine's own structs, and the
//! samples are copied from the bank into the player's own sample memory, one region per play.
//!
//! # What the dump shows
//!
//! * **A start** is a block whose state is the mixer's input for its first 5 ms frame, `S`:
//!   running, the sample's coefficients and first-frame state, the address of the first sample
//!   (nibble 2 of the data: the first frame's header byte is not played), the ratio, volume 0 (a
//!   fade-in) or 0x7FFF, and the send levels and mixer control the voice was left with.
//! * **The library's per-frame sync.** Nothing in the dump changes a voice during a frame; every
//!   change is written between frames, and a change of the send levels takes two blocks: the
//!   *ramp block* holds the old levels, the delta `(target - old) / 160` (truncated, 160 samples to
//!   a frame) and the mixer control with the ramp bit (8) and a 0x4000 mark; the next block holds
//!   the target and no delta. The start has the same shape: `S` as above, `S + 1` with the fade's
//!   delta 0xCC (0x7FFF / 160) and the 0x4000 mark, `S + 2` settled (volume 0x7FFF).
//! * **The send levels** are `trunc(32768 * 10^(d / 200))` for an integer `d` in tenths of a dB:
//!   all 65 distinct levels of the dump fit it (see [`level`]).

use crate::mixer::{ARAM_SIZE, Aram, Mixer, Voice, new_aram};

/// The top of the sample memory that the music's ring buffers take (`sound_run`); the samples
/// of the plays are kept below it.
pub const MUSIC_ARAM: usize = 0x4_0000;
use crate::sound::{Source, SoundBank};
use std::collections::HashMap;

/// Output samples in a 5 ms frame, the divisor of a level's ramp delta.
const FRAME_SAMPLES: i32 = 160;

/// The send of the main left and main right buses in a voice (the order the mixer keeps them).
const MAIN_L: usize = 0;
const MAIN_R: usize = 1;

/// Mixer control of a sounding voice: the aux A and aux B sends enabled (their levels are 0).
const MC_ON: u16 = 3;
/// A mixer control word was rewritten this frame (set in a fresh voice and in the blocks of a
/// sync; not read by the mixer).
const MC_MARK: u16 = 0x4000;
/// Mixer control: the send levels ramp by their deltas.
const MC_RAMP: u16 = 8;

/// The voice's own volume at full, and the fade-in's step per sample (0x7FFF / 160, truncated:
/// the delta 0xCC of the block after a fading start).
const VOL_FULL: u16 = 0x7FFF;
const FADE_STEP: u16 = VOL_FULL / FRAME_SAMPLES as u16;

/// The output rate the game's ratios are made for, in hundredths of a hertz. The dump's ratios
/// fix it between 32,028.654 and 32,028.671 Hz from two sample rates at once (22,050 Hz: 0xB03D;
/// 48,000 Hz: 0x17FA8); the SDK's nominal 32,000 Hz does not fit either.
const OUTPUT_RATE_CHZ: u64 = 3_202_866;

/// Input samples per output sample, 16.16 fixed point, for a sample at `rate` Hz.
pub fn src_ratio(rate: u32) -> u32 {
    (rate as u64 * 65536 * 100 / OUTPUT_RATE_CHZ) as u32
}

/// A send level (1.15) for an attenuation in tenths of a dB (0 or below): `trunc(32768 *
/// 10^(db10 / 200))`, 0 at -960 and below (the mixer library's floor). All 65 distinct send
/// levels of the dump are exactly these (checked by `levels_of_the_dump`). 0 dB is held to 0x7FFF
/// (unseen: no send in the dump is at 0 dB).
pub fn level(db10: i32) -> u16 {
    if db10 <= -960 {
        return 0;
    }
    ((32768.0 * 10f64.powf(db10.min(0) as f64 / 200.0)) as u32).min(0x7FFF) as u16
}

/// The volume below which the sound layer sends nothing (both levels 0, the mixer control 0).
/// The dump bounds it: a volume of 0.1416 still sounds (a footstep's fade-out, -243 and -216),
/// 0.084 does not (the next frame of that fade-out), and the 2^-10 of a fresh sound's first
/// parameters does not. 0.1 is a round value in that gap, not a measured one.
const SILENT: f32 = 0.1;

/// The pan (0 left to 127 right, 64 the centre) from a sound's two position floats: the
/// parameters' words 1 and 2 (`x`, `volume`). Fitted to 30 voices of the dump: the pan follows
/// `127 * (1 - |x| / (2 * volume))` (the sign of `x` makes no difference), rounded; 10 of the 30
/// fit to the tenth of a dB, the rest within 0.8 dB (see `docs/assets.md`, "The voices").
pub fn pan_of(x: f32, volume: f32) -> u8 {
    let p = 127.0 * (1.0 - 0.5 * (x.abs() / volume) as f64);
    (p + 0.5).floor().clamp(0.0, 127.0) as u8
}

/// The send levels (main left, main right) of a sound at linear `volume` and `pan`: the
/// attenuation in tenths of a dB is `100 * ln(volume)` (so 1.0 is 0 dB, 0.5742 is -5.5 dB), plus
/// the pan law `20 log10(cos(pan * pi / 254))` on the left and `sin` on the right, each sum
/// truncated toward zero (pan 64 of volume 1.0 gives -3.0 and -2.9 dB: the dump's 0x5A9D and
/// 0x5BAA). Silent below [`SILENT`].
pub fn mix_of(volume: f32, pan: u8) -> Mix {
    if volume < SILENT {
        return Mix { l: 0, r: 0 };
    }
    let v = 100.0 * (volume as f64).ln();
    let th = pan as f64 * std::f64::consts::PI / 254.0;
    let side = |a: f64| if a <= 0.0 { -960 } else { (v + 200.0 * a.log10()).trunc() as i32 };
    Mix { l: level(side(th.cos())), r: level(side(th.sin())) }
}

/// The two send levels a sound sets (main left and right; the other sends stay 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mix {
    pub l: u16,
    pub r: u16,
}

impl Mix {
    /// What a fresh voice holds: volume 1.0 at the centre.
    fn fresh() -> Mix {
        mix_of(1.0, 64)
    }

    fn audible(self) -> bool {
        self.l | self.r != 0
    }
}

/// Where a voice is in the library's sync (see the module's description).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// The start block is the next or just taken.
    Start,
    /// The block just taken was a sync block: its successor is settled.
    Sync,
    Settled,
}

/// One of the engine's mixer voices, with what the sound layer keeps for it.
struct Ax {
    v: Voice,
    /// A handle holds it and the play has not been stopped.
    live: bool,
    handle: u32,
    play_id: u32,
    sample: usize,
    /// The sample's first nibble in the player's sample memory.
    base: u32,
    age: u32,
    stage: Stage,
    /// Whether the start fades in (the first parameters' third word is 0).
    fade: bool,
    /// The levels the voice has reached or is ramping to, and the ones waiting for the next sync.
    target: Mix,
    pending: Option<Mix>,
}

/// A voice as the mixer is about to take it, for the checks.
pub struct Playing<'p> {
    /// Counts the plays, from 0.
    pub play_id: u32,
    pub handle: u32,
    /// The sample's index in the bank.
    pub sample: usize,
    /// The sample's first nibble in the player's sample memory ([`Playing::voice`]'s `cur` is
    /// 2 more at the start).
    pub base: u32,
    /// Frames since the start block.
    pub age: u32,
    pub voice: &'p Voice,
}

/// The sound voices of the engine.
pub struct SoundPlayer<'b> {
    bank: &'b SoundBank,
    aram: Box<Aram>,
    /// The next free byte of the sample memory (it wraps; one play's sample is far smaller than
    /// the memory, so a playing sample is not overwritten in practice).
    next_byte: usize,
    ax: Vec<Ax>,
    /// Voices of finished plays, the latest freed first (taken again first).
    free: Vec<usize>,
    handles: HashMap<u32, usize>,
    plays: u32,
    /// The options' sound volume as a gain on every params call's volume.
    gain: f32,
}

impl<'b> SoundPlayer<'b> {
    pub fn new(bank: &'b SoundBank) -> SoundPlayer<'b> {
        SoundPlayer { bank, aram: new_aram(), next_byte: 0, ax: Vec::new(), free: Vec::new(), handles: HashMap::new(), plays: 0, gain: 1.0 }
    }

    /// The sample memory the voices read from.
    pub fn aram(&self) -> &Aram {
        &self.aram
    }

    /// The sample memory, for the music's buffers in the top `MUSIC_ARAM` bytes.
    pub fn aram_mut(&mut self) -> &mut Aram {
        &mut self.aram
    }

    /// Frees the voices whose sample has played to its end (a one-shot's voice stays held by its
    /// handle until the handle is stopped); returns their handles.
    pub fn reap(&mut self) -> Vec<u32> {
        let done: Vec<u32> = self.ax.iter().filter(|a| a.live && !a.v.running).map(|a| a.handle).collect();
        for &h in &done {
            self.stop(h);
        }
        done
    }

    /// A handle gets a voice at its first play (the log's acquire calls reserve none: a voice
    /// first sounds when its handle first plays): the one freed last, or a new one.
    fn bind(&mut self, handle: u32) {
        if self.handles.contains_key(&handle) {
            return;
        }
        let i = self.free.pop().unwrap_or_else(|| {
            let mut v = Voice { src_type: 1, mixer_control: MC_MARK | MC_ON, ..Voice::default() };
            let m = Mix::fresh();
            v.send_vol[MAIN_L] = m.l;
            v.send_vol[MAIN_R] = m.r;
            self.ax.push(Ax {
                v,
                live: false,
                handle,
                play_id: 0,
                sample: 0,
                base: 0,
                age: 0,
                stage: Stage::Settled,
                fade: true,
                target: m,
                pending: None,
            });
            self.ax.len() - 1
        });
        self.ax[i].handle = handle;
        self.handles.insert(handle, i);
    }

    /// Plays the sample with resource id `sample_id` on the handle (a play on a handle that is
    /// sounding starts it over); `looping` is the sound's own (the calls do not say: the
    /// object's script does). Returns the play's number, or `None` if the sample is not a
    /// mono sample of the bank (music streams are not played here).
    pub fn play(&mut self, handle: u32, sample_id: u32, looping: bool) -> Option<u32> {
        let idx = self.bank.find_id(sample_id)?;
        if self.bank.entries()[idx].source != Source::Samples {
            return None;
        }
        let snd = self.bank.sound(idx)?;
        let src = snd.source(0);
        self.bind(handle);
        let i = self.handles[&handle];
        // The sample goes to its own region of the sample memory, 32-byte aligned.
        if self.next_byte + src.data.len() + 32 > ARAM_SIZE - MUSIC_ARAM {
            self.next_byte = 0;
        }
        let at = self.next_byte;
        self.aram[at..at + src.data.len()].copy_from_slice(src.data);
        self.next_byte = (at + src.data.len()).next_multiple_of(32);
        let base = (at * 2) as u32;
        let id = self.plays;
        self.plays += 1;

        let ax = &mut self.ax[i];
        // A restarted voice keeps the levels it had reached.
        if ax.live {
            let m = ax.pending.unwrap_or(ax.target);
            settle(ax, m);
        }
        let v = &mut ax.v;
        v.running = true;
        v.is_stream = false;
        v.src_type = 1;
        v.cur = base + 2;
        v.end = base + 2 * src.data.len() as u32 - 1;
        // A looping voice repeats the whole sound (the header carries the span of the whole sound
        // for every sample, with the loop flag clear: see docs/assets.md, "Sound").
        v.looping = looping;
        v.loop_addr = base + src.loop_start;
        v.coefs = src.coefs;
        v.pred_scale = src.start.0 as u16;
        v.yn1 = src.start.1;
        v.yn2 = src.start.2;
        v.loop_pred_scale = src.loop_at.0 as u16;
        v.loop_yn1 = src.loop_at.1;
        v.loop_yn2 = src.loop_at.2;
        v.ratio = src_ratio(snd.rate);
        v.frac = 0;
        v.last = [0; 4];
        v.vol = 0;
        v.vol_delta = 0;
        v.send_delta = [0; crate::mixer::SENDS];
        ax.live = true;
        ax.play_id = id;
        ax.sample = idx;
        ax.base = base;
        ax.age = 0;
        ax.stage = Stage::Start;
        ax.fade = true;
        ax.pending = None;
        Some(id)
    }

    /// Parameters for a handle's play: the call's eight words (the flags, then floats). Word 1 is
    /// the position across the sound's field, word 2 its volume, and (on the call that comes with
    /// the play, flag bit 8) word 3 is 1.0 for a sound that starts at full volume and 0 for one
    /// that fades in; the rest (word 4 a number, words 5 and 6 repeat 1 and 2, word 7 0) are not
    /// used by any voice setting of the dump.
    pub fn params(&mut self, handle: u32, words: &[u32; 8]) {
        let Some(&i) = self.handles.get(&handle) else { return };
        let ax = &mut self.ax[i];
        if !ax.live {
            return;
        }
        let f = |k: usize| f32::from_bits(words[k]);
        if ax.age == 0 && ax.stage == Stage::Start && words[0] & 8 != 0 {
            ax.fade = f(3) < 0.5;
            if !ax.fade {
                ax.v.vol = VOL_FULL;
            }
        }
        let (x, volume) = (f(1), f(2));
        let heard = volume * self.gain;
        ax.pending = Some(if heard < SILENT { mix_of(0.0, 64) } else { mix_of(heard, pan_of(x, volume)) });
    }

    /// The options' sound volume, as a gain on the volumes the params calls give from now on.
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    /// The play on a handle ends and its voice is left as it is: the levels it was taking are
    /// kept, and the voice is the first taken by the next handle.
    pub fn stop(&mut self, handle: u32) {
        let Some(i) = self.handles.remove(&handle) else { return };
        let ax = &mut self.ax[i];
        if ax.live {
            let m = ax.pending.unwrap_or(ax.target);
            settle(ax, m);
            ax.live = false;
            ax.v.running = false;
        }
        self.free.push(i);
    }

    /// The library's sync between two frames: what the next block of each voice shows.
    pub fn advance(&mut self) {
        for ax in self.ax.iter_mut().filter(|a| a.live) {
            ax.age += 1;
            match ax.stage {
                Stage::Start => {
                    ax.v.mixer_control |= MC_MARK;
                    if ax.fade {
                        ax.v.vol_delta = FADE_STEP;
                    }
                    if let Some(p) = ax.pending.take() {
                        ramp_to(ax, p);
                    }
                    ax.stage = Stage::Sync;
                }
                Stage::Sync => {
                    let m = ax.target;
                    settle(ax, m);
                    if ax.fade {
                        ax.v.vol = VOL_FULL;
                        ax.v.vol_delta = 0;
                    }
                    ax.stage = Stage::Settled;
                }
                Stage::Settled => {
                    if let Some(p) = ax.pending.take() {
                        ramp_to(ax, p);
                        ax.stage = Stage::Sync;
                    }
                }
            }
        }
    }

    /// The voices the mixer takes in the next frame: those playing.
    pub fn playing(&self) -> impl Iterator<Item = Playing<'_>> {
        self.ax.iter().filter(|a| a.live && a.v.running).map(|a| Playing {
            play_id: a.play_id,
            handle: a.handle,
            sample: a.sample,
            base: a.base,
            age: a.age,
            voice: &a.v,
        })
    }

    /// Mixes one 5 ms frame of every playing voice into `mixer`.
    pub fn mix(&mut self, mixer: &mut Mixer) {
        for a in self.ax.iter_mut().filter(|a| a.live && a.v.running) {
            for ms in 0..5 {
                mixer.mix_voice_ms(&mut a.v, ms, &self.aram);
            }
        }
    }
}

/// The voice holds `m` as its send levels with no ramp and the mixer control of that level.
fn settle(ax: &mut Ax, m: Mix) {
    ax.target = m;
    ax.pending = None;
    ax.v.send_vol[MAIN_L] = m.l;
    ax.v.send_vol[MAIN_R] = m.r;
    ax.v.send_delta = [0; crate::mixer::SENDS];
    ax.v.mixer_control = if m.audible() { MC_ON } else { 0 };
}

/// The sync block of a change of levels: the old levels, the deltas to the new ones, the ramp bit.
fn ramp_to(ax: &mut Ax, m: Mix) {
    if m == ax.target {
        return;
    }
    let cur = [ax.v.send_vol[MAIN_L], ax.v.send_vol[MAIN_R]];
    for (k, to) in [m.l, m.r].into_iter().enumerate() {
        ax.v.send_delta[k] = ((to as i32 - cur[k] as i32) / FRAME_SAMPLES) as u16;
    }
    ax.v.mixer_control = MC_MARK | MC_RAMP | (ax.v.mixer_control & MC_ON);
    ax.target = m;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The send levels the dump shows for each attenuation (home lot, all voices but the music).
    #[test]
    fn levels_of_the_dump() {
        for (db, v) in [(-30, 0x5A9D), (-29, 0x5BAA), (-85, 0x301B), (-86, 0x2F8E), (-98, 0x296B), (-99, 0x28F2), (-100, 0x287A), (-101, 0x2803)] {
            assert_eq!(level(db), v, "{db}");
        }
        assert_eq!(level(-960), 0);
    }

    /// A sound at full volume in the centre (the UI's) and the dogs' at 0.5742 and 0.4561.
    #[test]
    fn pan_and_volume() {
        assert_eq!(mix_of(1.0, 64), Mix { l: 0x5A9D, r: 0x5BAA });
        assert_eq!(mix_of(0.574_218_75, pan_of(0.574_218_75, 0.574_218_75)), Mix { l: 0x2F8E, r: 0x301B });
        assert_eq!(mix_of(0.456_054_7, 64), Mix { l: level(-109), r: level(-108) });
        assert_eq!(mix_of(0.0009765625, 64), Mix { l: 0, r: 0 });
    }

    /// The ratios the dump has for 22,050 Hz samples and the 48,000 Hz music stream.
    #[test]
    fn ratios() {
        assert_eq!(src_ratio(22050), 0xB03D);
        assert_eq!(src_ratio(48000), 0x17FA8);
    }
}
