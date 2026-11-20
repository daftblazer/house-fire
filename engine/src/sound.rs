//! The game's sounds (`docs/assets.md`, "Sound"): the one-shot samples of `samples.arc` and the
//! music streams of `audiostr.arc`, both Nintendo DSP-ADPCM, decoded natively.
//!
//! A sound is one or two channels, each a 0x60-byte big-endian header (the sample count, the
//! nibble count, the rate, the loop flag, the format, the loop start and end as nibble
//! addresses, the 16 coefficients and the start and loop predictor/scale and history) followed
//! by the 4-bit data in 8-byte frames of 14 samples. A stereo stream stores its two channels as
//! two whole blocks, each with its own header.
//!
//! The decode is the console's DSP accelerator as the recompilation models it: the same
//! integer arithmetic, so the samples are bit for bit what the game's mixer is fed (including
//! the clamp to [-0x7FFF, 0x7FFF]).

use std::collections::HashMap;

use crate::arc::{self, be16, be32, Entry};
use crate::disc::Disc;

/// The size of a channel's header.
const HEADER: usize = 0x60;
/// The alignment of a stream's second channel block (the data shows 32 bytes).
const BLOCK_ALIGN: usize = 32;
/// Nibbles in a frame (a header byte, then 14 samples).
const FRAME: u32 = 16;

/// One channel: its header's facts and its data.
#[derive(Clone, Debug)]
pub struct Channel<'a> {
    coefs: [i16; 16],
    /// Predictor/scale of the first frame, and the history at the start.
    start: (u8, i16, i16),
    /// Predictor/scale and history at the loop start.
    loop_at: (u8, i16, i16),
    loop_start: u32,
    loop_end: u32,
    data: &'a [u8],
}

/// A sound parsed from an archive entry's bytes (borrowing them).
#[derive(Clone, Debug)]
pub struct Sound<'a> {
    /// Samples per second.
    pub rate: u32,
    /// Samples per channel.
    pub samples: u32,
    /// Nibbles per channel, the frames' header nibbles included.
    pub nibbles: u32,
    /// The loop as sample positions (start, end exclusive), if the sound loops.
    pub looping: Option<(u32, u32)>,
    /// The loop as the header gives it: nibble addresses of the start and of the last nibble
    /// played before the jump back, if the sound loops.
    pub loop_nibbles: Option<(u32, u32)>,
    /// The header's loop fields as stored: the flag, the start and the end (nibble addresses),
    /// whether or not the flag is set.
    pub header_loop: (u16, u32, u32),
    channels: Vec<Channel<'a>>,
    /// Whether the history is kept (not reloaded) when a loop jumps back: the recompilation's
    /// accelerator reloads it for samples only, not for streams.
    keep_history: bool,
}

/// A nibble address as a sample position (the frames' header nibbles take no sample).
fn sample_at(nibble: u32) -> u32 {
    (nibble / FRAME) * 14 + (nibble % FRAME).saturating_sub(2)
}

/// A channel's header as read at byte `at` of `b`: the channel, then the sample count, the
/// nibble count, the rate, the loop flag and the format.
type Parsed<'a> = (Channel<'a>, u32, u32, u32, u16, u16);

fn parse_channel(b: &[u8], at: usize) -> Option<Parsed<'_>> {
    let h = b.get(at..at + HEADER)?;
    let samples = be32(h, 0)?;
    let nibbles = be32(h, 4)?;
    let rate = be32(h, 8)?;
    let (looped, format) = (be16(h, 0xC)?, be16(h, 0xE)?);
    let (loop_start, loop_end) = (be32(h, 0x10)?, be32(h, 0x14)?);
    let mut coefs = [0i16; 16];
    for (i, c) in coefs.iter_mut().enumerate() {
        *c = be16(h, 0x1C + 2 * i)? as i16;
    }
    let ps = |p| Some(be16(h, p)? as u8);
    let hist = |p| Some(be16(h, p)? as i16);
    let start = (ps(0x3E)?, hist(0x40)?, hist(0x42)?);
    let loop_at = (ps(0x44)?, hist(0x46)?, hist(0x48)?);
    let end = at.checked_add(HEADER)?.checked_add((nibbles as usize).div_ceil(2))?;
    let data = b.get(at + HEADER..end)?;
    let ch = Channel { coefs, start, loop_at, loop_start, loop_end, data };
    Some((ch, samples, nibbles, rate, looped, format))
}

impl<'a> Sound<'a> {
    /// Parses an entry's bytes; `stream` for `audiostr.arc`'s (whose entries may hold a second
    /// channel block). `None` if the header is not ADPCM (format 0) or the data is short.
    pub fn parse(b: &'a [u8], stream: bool) -> Option<Sound<'a>> {
        let (ch, samples, nibbles, rate, looped, format) = parse_channel(b, 0)?;
        if format != 0 || nibbles < 2 || sample_at(nibbles) < samples {
            return None;
        }
        let mut channels = vec![ch];
        if stream {
            // The second block starts after the first's data, rounded up to the alignment.
            let at = (HEADER + (nibbles as usize).div_ceil(2)).next_multiple_of(BLOCK_ALIGN);
            if let Some((c2, s2, n2, r2, _, 0)) = parse_channel(b, at)
                && (s2, n2, r2) == (samples, nibbles, rate)
            {
                channels.push(c2);
            }
        }
        let c = &channels[0];
        let loop_nibbles = (looped != 0).then_some((c.loop_start, c.loop_end));
        let looping = loop_nibbles.map(|(s, e)| (sample_at(s), sample_at(e) + 1));
        let header_loop = (looped, c.loop_start, c.loop_end);
        Some(Sound { rate, samples, nibbles, looping, loop_nibbles, header_loop, channels, keep_history: stream })
    }

    /// The number of channels (1 or 2).
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// What a mixer voice is loaded with to play channel `ch`: its ADPCM bytes and the header's
    /// coefficients, first-frame state and loop state.
    pub fn source(&self, ch: usize) -> VoiceSource<'_> {
        let c = &self.channels[ch];
        VoiceSource {
            data: c.data,
            coefs: c.coefs,
            start: c.start,
            loop_at: c.loop_at,
            loop_start: c.loop_start,
            loop_end: c.loop_end,
            keep_history: self.keep_history,
        }
    }

    /// A one-shot decoder of channel `ch`: exactly `samples` samples, then it ends.
    pub fn decode(&self, ch: usize) -> Decoder<'_> {
        Decoder::new(self, ch, Some(self.samples))
    }

    /// A decoder of channel `ch` for loop playback: it never ends, jumping back from the
    /// header's loop end to its loop start as the console's accelerator does. The header's loop
    /// flag is not consulted: every sound on the disc has it clear and a loop span of the whole
    /// sound (2 to the last nibble), the game's voice deciding whether to loop.
    pub fn looped(&self, ch: usize) -> Decoder<'_> {
        Decoder::new(self, ch, None)
    }

    /// All of channel `ch`'s samples, one-shot.
    pub fn decode_all(&self, ch: usize) -> Vec<i16> {
        self.decode(ch).collect()
    }
}

/// One channel as a mixer voice plays it (see [`Sound::source`]).
#[derive(Clone, Copy, Debug)]
pub struct VoiceSource<'a> {
    /// The ADPCM frames, from the first frame's predictor/scale byte (nibble address 0).
    pub data: &'a [u8],
    pub coefs: [i16; 16],
    /// Predictor/scale of the first frame, and the history before its first sample.
    pub start: (u8, i16, i16),
    /// Predictor/scale and history at the loop start.
    pub loop_at: (u8, i16, i16),
    /// The header's loop span as nibble addresses (start, last nibble before the jump back).
    pub loop_start: u32,
    pub loop_end: u32,
    /// Whether the history is kept when a loop jumps back (streams).
    pub keep_history: bool,
}

/// The decoder: the accelerator's state for one voice.
#[derive(Clone, Debug)]
pub struct Decoder<'a> {
    ch: &'a Channel<'a>,
    /// The current nibble address.
    cur: u32,
    /// The current frame's predictor/scale byte.
    ps: u8,
    yn1: i32,
    yn2: i32,
    /// Samples left (one-shot), or `None` for the looping mode.
    left: Option<u32>,
    keep_history: bool,
}

impl<'a> Decoder<'a> {
    fn new(s: &'a Sound<'a>, ch: usize, left: Option<u32>) -> Decoder<'a> {
        let c = &s.channels[ch];
        Decoder {
            ch: c,
            cur: 2,
            ps: c.start.0,
            yn1: c.start.1 as i32,
            yn2: c.start.2 as i32,
            left,
            keep_history: s.keep_history,
        }
    }

    fn byte(&self, nibble: u32) -> u8 {
        self.ch.data.get((nibble >> 1) as usize).copied().unwrap_or(0)
    }
}

impl Iterator for Decoder<'_> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        match &mut self.left {
            Some(0) => return None,
            Some(n) => *n -= 1,
            None => {}
        }
        if self.cur.is_multiple_of(FRAME) {
            self.ps = self.byte(self.cur);
            self.cur += 2;
        }
        let b = self.byte(self.cur);
        let t = if self.cur & 1 != 0 { b & 15 } else { b >> 4 };
        let t = (t as i32 ^ 8) - 8;
        let pair = ((self.ps >> 4) & 7) as usize * 2;
        let (c1, c2) = (self.ch.coefs[pair] as i32, self.ch.coefs[pair + 1] as i32);
        let v = ((1i32 << (self.ps & 15)) * t + ((0x400 + c1 * self.yn1 + c2 * self.yn2) >> 11)).clamp(-0x7FFF, 0x7FFF);
        self.yn2 = self.yn1;
        self.yn1 = v;
        self.cur += 1;
        if self.left.is_none() && self.cur > self.ch.loop_end {
            self.cur = self.ch.loop_start;
            self.ps = self.ch.loop_at.0;
            if !self.keep_history {
                self.yn1 = self.ch.loop_at.1 as i32;
                self.yn2 = self.ch.loop_at.2 as i32;
            }
        }
        Some(v as i16)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.left {
            Some(n) => (n as usize, Some(n as usize)),
            None => (usize::MAX, None),
        }
    }
}

/// Which archive a bank entry is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `samples.arc`: one-shot and looping samples.
    Samples,
    /// `audiostr.arc`: music streams.
    Stream,
}

/// A sound's place in the bank.
#[derive(Clone, Debug)]
pub struct BankEntry {
    pub source: Source,
    pub entry: Entry,
}

/// Both sound archives of the disc, indexed by name (case-insensitive: the scripts name samples
/// in mixed case) and by the archives' resource ids. The archives' bytes are kept.
pub struct SoundBank {
    samples: Vec<u8>,
    streams: Vec<u8>,
    entries: Vec<BankEntry>,
    by_name: HashMap<String, usize>,
    by_id: HashMap<u32, usize>,
}

impl SoundBank {
    /// The bank of a disc's `samples.arc` and `audiostr.arc`, if both read.
    pub fn open(d: &Disc) -> Option<SoundBank> {
        SoundBank::from_archives(d.read("samples.arc")?, d.read("audiostr.arc")?)
    }

    /// The bank of the two archives' bytes. If a name or an id occurs twice the first wins.
    pub fn from_archives(samples: Vec<u8>, streams: Vec<u8>) -> Option<SoundBank> {
        let mut entries = Vec::new();
        for (source, data) in [(Source::Samples, &samples), (Source::Stream, &streams)] {
            entries.extend(arc::entries(data)?.into_iter().map(|entry| BankEntry { source, entry }));
        }
        let (mut by_name, mut by_id) = (HashMap::new(), HashMap::new());
        for (i, e) in entries.iter().enumerate() {
            by_name.entry(e.entry.name.to_ascii_lowercase()).or_insert(i);
            by_id.entry(e.entry.id).or_insert(i);
        }
        Some(SoundBank { samples, streams, entries, by_name, by_id })
    }

    /// Every entry, samples first, in archive order.
    pub fn entries(&self) -> &[BankEntry] {
        &self.entries
    }

    /// The index of the sound called `name` (any case).
    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// The index of the sound with resource id `id`.
    pub fn find_id(&self, id: u32) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    /// The sound at index `i`, or `None` if its header does not parse.
    pub fn sound(&self, i: usize) -> Option<Sound<'_>> {
        let e = self.entries.get(i)?;
        let data = if e.source == Source::Samples { &self.samples } else { &self.streams };
        Sound::parse(arc::bytes(data, &e.entry), e.source == Source::Stream)
    }
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 of `data` (FIPS 180-4), for the checks that compare decoded samples by hash.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let whole = data.len() / 64 * 64;
    let mut tail = data[whole..].to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in data[..whole].chunks(64).chain(tail.chunks(64)) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(c.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let t2 = s0.wrapping_add((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (o, x) in out.chunks_mut(4).zip(h) {
        o.copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// The first 16 hex digits of the SHA-256 of `samples` written little-endian: the form of the
/// sound answer key (`docs/assets.md`, "Sound").
pub fn hash16(samples: &[i16]) -> String {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    sha256(&bytes)[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame: predictor/scale byte 0x02 (scale shift 2, coefficient pair 0), then the
    /// nibbles 1, 2, -1, -8 and zeros.
    const FRAME0: [u8; 8] = [0x02, 0x12, 0xF8, 0, 0, 0, 0, 0];

    /// A one-channel entry of `data` (whole frames): coefficient pair 0 is (1.0, 0), so a
    /// sample is the scaled nibble plus the sample before it; zero history.
    fn entry(data: &[u8], looped: Option<(u32, u32)>) -> Vec<u8> {
        let nibbles = data.len() as u32 * 2;
        let mut h = vec![0u8; HEADER];
        h[0..4].copy_from_slice(&(nibbles / 16 * 14).to_be_bytes());
        h[4..8].copy_from_slice(&nibbles.to_be_bytes());
        h[8..12].copy_from_slice(&22050u32.to_be_bytes());
        if let Some((s, e)) = looped {
            h[0xC..0xE].copy_from_slice(&1u16.to_be_bytes());
            h[0x10..0x14].copy_from_slice(&s.to_be_bytes());
            h[0x14..0x18].copy_from_slice(&e.to_be_bytes());
        }
        h[0x1C..0x1E].copy_from_slice(&0x800i16.to_be_bytes());
        h[0x3F] = data[0];
        h.extend_from_slice(data);
        h
    }

    #[test]
    fn one_frame_decodes_by_the_formula() {
        let b = entry(&FRAME0, None);
        let s = Sound::parse(&b, false).unwrap();
        assert_eq!((s.rate, s.samples, s.channel_count(), s.looping), (22050, 14, 1, None));
        let v = s.decode_all(0);
        assert_eq!(v.len(), 14);
        assert_eq!(v[..5], [4, 12, 8, -24, -24]);
    }

    #[test]
    fn clamps_to_minus_7fff() {
        // scale 15, nibble -8: -8 * 32768 clamps to -0x7FFF
        let b = entry(&[0x0F, 0x80, 0, 0, 0, 0, 0, 0], None);
        let s = Sound::parse(&b, false).unwrap();
        assert_eq!(s.decode(0).next(), Some(-0x7FFF));
    }

    #[test]
    fn looping_jumps_back_with_the_loop_state() {
        // loop the first four samples (nibble addresses 2..=5)
        let b = entry(&FRAME0, Some((2, 5)));
        let s = Sound::parse(&b, false).unwrap();
        assert_eq!(s.looping, Some((0, 4)));
        let v: Vec<i16> = s.looped(0).take(9).collect();
        assert_eq!(v[..4], [4, 12, 8, -24]);
        // after the jump: the loop's predictor/scale (0: scale 1) and zero history, c1 = 1.0 adding the sample before
        assert_eq!(v[4..8], [1, 3, 2, -6]);
    }

    #[test]
    fn a_short_entry_does_not_parse() {
        let b = entry(&FRAME0, None);
        assert!(Sound::parse(&b[..b.len() - 1], false).is_none());
    }

    #[test]
    fn sha256_known_answers() {
        assert_eq!(sha256(b"abc")[..4], [0xba, 0x78, 0x16, 0xbf]);
        assert_eq!(sha256(&[b'a'; 1000])[..4], [0x41, 0xed, 0xec, 0xe4]);
    }
}
