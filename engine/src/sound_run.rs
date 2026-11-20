//! The sound in a run: commands (play, stop, params, music) go to the engine's own mixer, which
//! gives the output in frames of 5 ms at 32 kHz, stereo. The simulation's side
//! ([`SoundFront`](crate::sound_front::SoundFront)) makes the commands; the device (the play
//! window's `audio` module, on its own thread) or a file writer takes the frames.
//!
//! The music is a stream voice per channel: the stream is decoded once (the DSP-ADPCM the disc
//! holds, to 16-bit samples) and played from a double buffer in the top of the sample memory, a
//! half refilled each time the voice leaves it, as the original refills its stream buffers from
//! the disc (`docs/assets.md`, "The sound out"). A stereo stream is two voices, hard left and
//! hard right; a mono one is one voice on both.

use crate::mixer::{ARAM_SIZE, Aram, Format, Mixer, SPF, Voice, bus};
use crate::sound::{SoundBank, Source};
use crate::sound_player::{MUSIC_ARAM, SoundPlayer, level, src_ratio};

/// One thing the simulation's side asks of the sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Play the sample with this resource id on a handle (a handle plays one sound at a time).
    Play { handle: u32, sample: u32, looping: bool },
    Stop { handle: u32 },
    /// A params call's words (`sound_params::words`).
    Params { handle: u32, words: [u32; 8] },
    /// Play the stream at this bank index as the music (a stream replaces the one playing), or
    /// stop the music.
    Music(Option<u32>),
}

/// Samples in half of a music buffer (0.34 s of a 48 kHz stream).
const HALF: usize = 16384;
/// The music's level on each main bus: -6 dB (a chosen mix: the original's stream voices' levels
/// are not in the dump this engine has).
const MUSIC_LEVEL_DB10: i32 = -60;

/// One channel of the music: its samples and its voice's buffer.
struct Channel {
    pcm: Vec<i16>,
    /// Where the next sample to buffer is, in `pcm`.
    next: usize,
    /// The buffer's first sample in the sample memory (in samples).
    base: u32,
    /// The half the voice was in at the last frame.
    half: usize,
    voice: Voice,
}

struct Music {
    channels: Vec<Channel>,
}

impl Music {
    /// The stream `index` of the bank, ready to play from the buffers at the top of `aram`.
    fn new(bank: &SoundBank, index: usize, aram: &mut Aram) -> Option<Music> {
        let entry = bank.entries().get(index)?;
        if entry.source != Source::Stream {
            return None;
        }
        let sound = bank.sound(index)?;
        let n = sound.channel_count().min(2);
        let ratio = src_ratio(sound.rate);
        let mut channels = Vec::new();
        for ch in 0..n {
            let pcm = sound.decode_all(ch);
            if pcm.is_empty() {
                return None;
            }
            // each channel's ring: 2 halves of HALF samples, 2 bytes each
            let base_bytes = ARAM_SIZE - MUSIC_ARAM + ch * 4 * HALF;
            let mut c = Channel { pcm, next: 0, base: (base_bytes / 2) as u32, half: 0, voice: Voice::default() };
            c.fill(0, aram);
            c.fill(1, aram);
            let level = level(MUSIC_LEVEL_DB10);
            let base = c.base;
            let v = &mut c.voice;
            v.running = true;
            v.format = Format::Pcm16;
            v.looping = true;
            v.is_stream = true;
            v.src_type = 1;
            v.ratio = ratio;
            v.cur = base;
            v.loop_addr = base;
            v.end = base + 2 * HALF as u32 - 1;
            v.vol = 0x7FFF;
            v.mixer_control = 3;
            // a stereo stream's channels go to their own side, a mono one to both
            (v.send_vol[0], v.send_vol[1]) = match (n, ch) {
                (1, _) => (level, level),
                (_, 0) => (level, 0),
                _ => (0, level),
            };
            channels.push(c);
        }
        Some(Music { channels })
    }

    /// A frame of the music, and the refill of the half the voices have left.
    fn mix(&mut self, mixer: &mut Mixer, aram: &mut Aram) {
        for c in &mut self.channels {
            for ms in 0..5 {
                mixer.mix_voice_ms(&mut c.voice, ms, aram);
            }
            let half = ((c.voice.cur - c.base) as usize / HALF).min(1);
            if half != c.half {
                // the voice crossed into `half`: the one it left is free
                c.fill(c.half, aram);
                c.half = half;
            }
        }
    }
}

impl Channel {
    /// The next `HALF` samples (the stream's loop wrapping) into half `h` of the ring.
    fn fill(&mut self, h: usize, aram: &mut Aram) {
        let at = (self.base as usize + h * HALF) * 2;
        for k in 0..HALF {
            let s = self.pcm[self.next];
            self.next = (self.next + 1) % self.pcm.len();
            aram[at + 2 * k..at + 2 * k + 2].copy_from_slice(&s.to_be_bytes());
        }
    }
}

/// The mixer side of the sound: the voices and the music, a frame at a time.
pub struct SoundRun<'b> {
    bank: &'b SoundBank,
    player: SoundPlayer<'b>,
    mixer: Mixer,
    music: Option<Music>,
    frames: u64,
}

impl<'b> SoundRun<'b> {
    pub fn new(bank: &'b SoundBank) -> SoundRun<'b> {
        SoundRun { bank, player: SoundPlayer::new(bank), mixer: Mixer::new(), music: None, frames: 0 }
    }

    /// Does what a command asks, between two frames.
    pub fn apply(&mut self, c: Command) {
        match c {
            Command::Play { handle, sample, looping } => {
                self.player.play(handle, sample, looping);
            }
            Command::Stop { handle } => self.player.stop(handle),
            Command::Params { handle, words } => self.player.params(handle, &words),
            Command::Music(None) => self.music = None,
            Command::Music(Some(i)) => self.music = Music::new(self.bank, i as usize, self.player.aram_mut()),
        }
    }

    /// The next frame, stereo, interleaved.
    pub fn frame(&mut self, out: &mut [i16; 2 * SPF]) {
        if self.frames > 0 {
            self.player.advance();
        }
        self.frames += 1;
        self.mixer.setup(&[(0, 0); bus::COUNT]);
        self.player.mix(&mut self.mixer);
        self.player.reap();
        if let Some(m) = &mut self.music {
            m.mix(&mut self.mixer, self.player.aram_mut());
        }
        let mut surround = [0i32; SPF];
        self.mixer.output(out, &mut surround);
    }

    /// The frames made so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }
}

/// The sample rate of the frames (`mixer::SPMS` samples a millisecond).
pub const RATE: u32 = 32_000;
/// Samples in a frame (per channel).
pub const FRAME: usize = SPF;
