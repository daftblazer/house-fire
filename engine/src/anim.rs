//! Animations as Bustin' Out keeps them (`animatio.arc`'s entries, `docs/assets.md`,
//! "Animations"): the engine's ERAnim in its old layout (The Sims 2's `ERAnim::OldLoad` reads
//! the same fields behind a chunk header), here behind the animation's name. Big-endian and
//! packed: nothing is aligned after the name.

use std::sync::{Arc, OnceLock};

/// A node (a bone's track): seven words, read as they are (what each means is read with the
/// evaluator; the pool table's give the third word 3k + 1 and the fourth -1 - 66k).
pub type Node = [u32; 7];

/// An event (an animation note the scripts can wait for): its name (64 bytes, NUL padded) and
/// four words.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub words: [u32; 4],
}

/// An animation: its name, frame count, two triples, its nodes, a word, its constants (the
/// tracks' values), its bit array (as words, `bits` long), the frame rate, two more floats, a
/// byte, four words, two bytes and the events.
#[derive(Clone, Debug, PartialEq)]
pub struct Anim {
    pub name: String,
    pub frames: u32,
    pub first: [f32; 3],
    pub second: [f32; 3],
    pub nodes: Vec<Node>,
    pub word: u32,
    pub constants: Vec<f32>,
    pub bits: u32,
    pub bit_words: Vec<u32>,
    pub rate: f32,
    pub floats: [f32; 2],
    pub byte: u8,
    pub words: [u32; 4],
    pub flags: [u8; 2],
    pub events: Vec<Event>,
    /// The nodes' streams decoded key by key, made on first use (`Anim::node_keys`).
    compiled: Compiled,
}

impl Anim {
    /// How long it runs, in seconds (frames over the rate).
    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.rate
    }

    /// The end mode a play of it gets (a track's +13): 0 loops, 1 stops at its end. The
    /// animation's second flag byte.
    pub fn end_mode(&self) -> u8 {
        self.flags[1]
    }

    /// Every node's scale, translation, vector and rotation streams with their keys decoded,
    /// made once per animation and shared by every play of it.
    pub(crate) fn node_keys(&self) -> &[NodeKeys] {
        self.compiled.0.get_or_init(|| {
            let words = &self.bit_words;
            let stream = |word: u32| match Channel::of(word) {
                Channel::Stream(at) => {
                    let header = stream_header(words, at);
                    Some((header, Arc::new(KeyTable3::decode(words, &header))))
                }
                _ => None,
            };
            self.nodes
                .iter()
                .map(|n| NodeKeys {
                    rotation: match Channel::of(n[0]) {
                        Channel::Stream(at) => {
                            let header = rotation_header(words, at);
                            Some((header, Arc::new(RotTable::decode(words, &header))))
                        }
                        _ => None,
                    },
                    vectors: [n[1], n[2], n[4]].map(stream),
                    scalars: [n[5], n[6]].map(|w| match Channel::of(w) {
                        Channel::Stream(at) => Some(Arc::new(ScalarStream::decode(words, at))),
                        _ => None,
                    }),
                })
                .collect()
        })
    }
}

/// One node's streams as `Anim::node_keys` holds them: the rotation (word 0) and the vectors of
/// words 1, 2 and 4, each its header and its decoded keys.
#[derive(Debug)]
pub(crate) struct NodeKeys {
    pub rotation: Option<(RotHeader, Arc<RotTable>)>,
    pub vectors: [Option<(StreamHeader, Arc<KeyTable3>)>; 3],
    /// Words 5 and 6, the factor and radius.
    pub scalars: [Option<Arc<ScalarStream>>; 2],
}

/// The decoded streams of an animation, kept beside it: never part of what an animation is, so
/// it compares equal and a copy starts empty.
#[derive(Debug, Default)]
struct Compiled(OnceLock<Vec<NodeKeys>>);

impl Clone for Compiled {
    fn clone(&self) -> Compiled {
        Compiled::default()
    }
}

impl PartialEq for Compiled {
    fn eq(&self, _: &Compiled) -> bool {
        true
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    fn triple(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    /// A count, refused when its items could not fit in what is left.
    fn count(&mut self, size: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.checked_mul(size)? <= self.b.len() - self.at).then_some(n)
    }
}

/// Reads an entry: `None` unless its parts fill it exactly.
pub fn read(e: &[u8]) -> Option<Anim> {
    let len = e.iter().position(|&c| c == 0)?;
    let name: String = e[..len].iter().map(|&c| c as char).collect();
    let mut r = Reader { b: e, at: len + 1 };
    let frames = r.u32()?;
    let (first, second) = (r.triple()?, r.triple()?);
    let n = r.count(28)?;
    let nodes = (0..n).map(|_| Some([r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?])).collect::<Option<Vec<_>>>()?;
    let word = r.u32()?;
    let n = r.count(4)?;
    let constants = (0..n).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
    let bits = r.u32()?;
    let bit_words = (0..bits.div_ceil(32)).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
    let rate = r.f32()?;
    let floats = [r.f32()?, r.f32()?];
    let byte = r.u8()?;
    let words = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
    let flags = [r.u8()?, r.u8()?];
    let n = r.count(80)?;
    let events = (0..n)
        .map(|_| {
            let raw = r.take(64)?;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(64);
            let name = raw[..end].iter().map(|&c| c as char).collect();
            Some(Event { name, words: [r.u32()?, r.u32()?, r.u32()?, r.u32()?] })
        })
        .collect::<Option<Vec<_>>>()?;
    (r.at == e.len()).then_some(Anim { name, frames, first, second, nodes, word, constants, bits, bit_words, rate, floats, byte, words, flags, events, compiled: Compiled::default() })
}

/// The animations in memory, as the original's resource manager has them for the Animate
/// primitive (EA's `EResourceManager`: `IsLoaded`, `AddRefAsync`). A lot comes up with its
/// preload sets in memory (`datasets.arc`: House1Preload and housepre01 for house 1; their
/// resources' ids are found in the sets' bytes, at any offset); an animation asked for that is
/// not in memory is loaded in the background and is there from the next tick.
#[derive(Clone, Debug, Default)]
pub struct Loads {
    present: std::collections::HashSet<u32>,
    pending: std::collections::BTreeMap<u32, u32>,
}

impl Loads {
    /// The ids `sets` (the preload sets' bytes) hold, as present; `ids` limits them to the
    /// resources asked about (the animation archive's entries).
    pub fn with_sets(sets: &[&[u8]], ids: &std::collections::HashSet<u32>) -> Loads {
        let mut present = std::collections::HashSet::new();
        for s in sets {
            for w in s.windows(4) {
                let v = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
                if ids.contains(&v) {
                    present.insert(v);
                }
            }
        }
        Loads { present, pending: Default::default() }
    }

    /// Whether `id` is in memory at `tick`; if not, its load is asked for (it is there from the
    /// next tick on).
    pub fn ask(&mut self, id: u32, tick: u32) -> bool {
        if self.present.contains(&id) {
            return true;
        }
        match self.pending.get(&id) {
            Some(&t) if tick > t => {
                self.present.insert(id);
                self.pending.remove(&id);
                true
            }
            Some(_) => false,
            None => {
                self.pending.insert(id, tick);
                false
            }
        }
    }
}

/// A note of an animation (the AnimRef row's, `docs/primitives.md` "Animate"): its time in ms,
/// its type (6 the script's events; 1 steps, 5 sounds) and its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub ms: u32,
    pub kind: u32,
    pub value: i32,
}

/// The notes of every animation, by its id, from the quick data's AnimRef rows: (id, length in
/// ms, a pointer to the notes, their count in the word before it).
pub fn notes(img: &crate::quickdata::Image) -> std::collections::BTreeMap<u32, Vec<Note>> {
    img.rows("AnimRef")
        .filter_map(|r| {
            let id = img.u32(r)?;
            let p = img.ptr(r + 8)?;
            let n = (img.u32(p.checked_sub(4)?)? as usize).min(64);
            let list = (0..n).map(|k| Some(Note { ms: img.u32(p + 12 * k)?, kind: img.u32(p + 12 * k + 4)?, value: img.u32(p + 12 * k + 8)? as i32 })).collect::<Option<Vec<_>>>()?;
            Some((id, list))
        })
        .collect()
}

/// What the Animate primitive gets when it asks its play (The Sims 2's `SAnimator2::TryAnimate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    /// Still playing: the primitive waits a tick.
    Pending,
    /// An event: the primitive returns false with this value (into a local with flag 0x20).
    Event(i32),
    /// Played to its end with every counted event handed out: the primitive returns true.
    Done,
}

/// One play of an animation by a Sim, frame by frame. Its place runs from 0 in the play's
/// direction (frames of the animation): each frame from the first after the step that starts it
/// reaches 0.033 s × the animation's rate × that frame's speed further (1, 5 in fast-forward, 0
/// on a load miss's frames). A note is given once a frame reaches past it. Played backward, the
/// first frame stands still.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    /// The animation's last frame (its frames − 1).
    last: f32,
    place: f32,
    still_first: bool,
    /// The script's notes (type 6): their places along the play and their values, in order.
    notes: Vec<(f32, i32)>,
    next: usize,
    /// The counted events (values 0 to 99) the call asks for (operand byte 6) and those given.
    wanted: i32,
    counted: i32,
    backward: bool,
    /// A backward play paused for a frame goes on forward: the speed restored after the pause is
    /// +1 (the original's, kept). Its place then runs back toward 0 (the animation's end).
    turned: bool,
    done: bool,
    /// The play has answered true (a call after it starts the animation anew).
    answered: bool,
}

impl Play {
    /// A play of `anim` with its `notes` (all types; only type 6 are the script's), `backward`
    /// (operand byte 5's 0x02) and `wanted` counted events (operand byte 6).
    pub fn new(anim: &Anim, notes: &[Note], backward: bool, wanted: i32) -> Play {
        let last = anim.frames as f32 - 1.0;
        let notes = notes
            .iter()
            .filter(|n| n.kind == 6)
            .map(|n| (n.ms as f32 * anim.rate / 1000.0, n.value))
            .filter(|n| n.0 <= last)
            .map(|(t, v)| (if backward { last - t } else { t }, v))
            .collect();
        Play { last, place: 0.0, still_first: backward, notes, next: 0, wanted, counted: 0, backward, turned: false, done: false, answered: false }
    }

    /// How far along the animation the play is, in frames.
    pub fn place(&self) -> f32 {
        self.place
    }

    /// Whether it plays backward (its place moves toward the first frame).
    pub fn backward(&self) -> bool {
        self.backward
    }

    /// A frame: the place moves by `step` (the frame step × the rate × the speed).
    pub fn frame(&mut self, step: f32) {
        if self.still_first {
            self.still_first = false;
        } else if self.turned {
            self.place -= step;
        } else if step == 0.0 && self.backward {
            self.turned = true;
        } else {
            self.place += step;
        }
    }

    /// Whether the play is over (true given, or its last event given past its end).
    pub fn finished(&self) -> bool {
        self.done
    }

    fn expected(&self) -> i32 {
        if self.backward { self.wanted - self.counted - 1 } else { self.counted }
    }

    /// The primitive's question this tick. A note passed that is over 99 is an event; one of 0
    /// to 99 only when it is the next counted event (else it is passed over). Past the end, the
    /// counted events still owed come one a call, then done.
    pub fn poll(&mut self) -> Poll {
        const EPS: f32 = 1e-3;
        if self.done {
            self.answered = true;
            return Poll::Done;
        }
        if self.turned {
            // turned forward: it ends past the animation's last frame (place below 0)
            if self.place >= -EPS {
                return Poll::Pending;
            }
            self.done = true;
            self.answered = true;
            return Poll::Done;
        }
        while let Some(&(at, value)) = self.notes.get(self.next) {
            if self.place <= at + EPS {
                break;
            }
            self.next += 1;
            let e = self.expected();
            let given = value > 99 || ((0..self.wanted).contains(&e) && value == e);
            if given {
                if value <= 99 {
                    self.counted += 1;
                }
                // its last note given past its end, nothing owed: the play is over (the next
                // call starts it anew)
                if self.next >= self.notes.len() && self.place > self.last + EPS && self.counted >= self.wanted {
                    self.done = true;
                }
                return Poll::Event(value);
            }
        }
        if self.next < self.notes.len() || self.place <= self.last + EPS {
            return Poll::Pending;
        }
        if self.counted < self.wanted {
            let e = self.expected();
            self.counted += 1;
            return Poll::Event(e);
        }
        self.done = true;
        self.answered = true;
        Poll::Done
    }

    /// Whether the play has answered true (a call after it starts the animation anew).
    pub fn answered(&self) -> bool {
        self.answered
    }
}

/// `width` bits of `words` from bit `at`, the low bits first, a field running on into the next
/// word (Bustin' Out's 801ADA74; 0 for a width of 0).
pub fn bits(words: &[u32], at: u32, width: u32) -> u32 {
    if width == 0 {
        return 0;
    }
    let (w, s) = ((at >> 5) as usize, at & 31);
    let lo = words.get(w).copied().unwrap_or(0) >> s;
    let v = if s + width > 32 { lo | words.get(w + 1).copied().unwrap_or(0) << (32 - s) } else { lo };
    if width >= 32 { v } else { v & ((1 << width) - 1) }
}

/// A node's channel word (`docs/assets.md`, "Animation channels"): none, an index into the
/// animation's constants, or a stream at a bit of its bit array (the word negated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    None,
    Constant(usize),
    Stream(u32),
}

impl Channel {
    pub fn of(word: u32) -> Channel {
        match word as i32 {
            0 => Channel::None,
            w if w > 0 => Channel::Constant(w as usize),
            w => Channel::Stream(w.unsigned_abs()),
        }
    }
}

/// A scale or translation stream's header, as Bustin' Out's 801BC318 reads it from the
/// animation's bits: the key count (20 bits), the widths of the frame gap, the key's scalar and
/// each value (5 bits each; a value width of 0 is 32), then per value its range and minimum as
/// 32-bit floats. A value is its field times range / (2^width - 1), plus the minimum; the
/// scalar is its signed field over 2^(width - 1) - 1 (0 with no scalar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamHeader {
    pub keys: u32,
    pub gap_bits: u32,
    pub scalar_bits: u32,
    pub value_bits: u32,
    pub scalar_scale: f32,
    pub scale: [f32; 3],
    pub offset: [f32; 3],
    /// The first key's record: the bit after the header.
    pub first_record: u32,
    /// A record's bits.
    pub record_bits: u32,
}

/// Key `k`'s frame read from the bits: the gaps of keys 1..=k, each + 1 (key 0's gap is not
/// used; 801BCD2C, 801B6A1C).
fn frame_of_key(words: &[u32], first_record: u32, record_bits: u32, gap_bits: u32, k: u32) -> i32 {
    (1..=k).map(|j| bits(words, first_record + j * record_bits, gap_bits) as i32 + 1).sum()
}

/// A stream's key frames summed once, so that a key's frame is a lookup.
#[derive(Debug)]
struct Timeline {
    /// Key `k`'s frame, for each key.
    times: Vec<i32>,
    /// The frames the keys span (the sum of all their gaps + 1).
    frames: u32,
}

impl Timeline {
    fn decode(words: &[u32], keys: u32, first_record: u32, record_bits: u32, gap_bits: u32) -> Timeline {
        let mut times = Vec::with_capacity(keys as usize);
        let (mut at, mut frames) = (0i32, 0u32);
        for k in 0..keys {
            let gap = bits(words, first_record + k * record_bits, gap_bits);
            frames = frames.wrapping_add(gap).wrapping_add(1);
            if k > 0 {
                at = at.wrapping_add(gap as i32).wrapping_add(1);
            }
            times.push(at);
        }
        Timeline { times, frames }
    }
}

/// A scale or translation stream's keys decoded once (`Anim::node_keys`): what the bit fields
/// give per key, in flat arrays. A key not in them (the evaluator can step past the last)
/// is read from the bits as before.
#[derive(Debug)]
pub(crate) struct KeyTable3 {
    timeline: Timeline,
    points: Vec<[f32; 3]>,
    /// Empty for a stream without scalars (they read 0).
    scalars: Vec<f32>,
}

impl KeyTable3 {
    fn decode(words: &[u32], h: &StreamHeader) -> KeyTable3 {
        let timeline = Timeline::decode(words, h.keys, h.first_record, h.record_bits, h.gap_bits);
        let points = (0..h.keys).map(|k| h.point(words, k)).collect();
        let scalars = if h.scalar_bits == 0 { Vec::new() } else { (0..h.keys).map(|k| h.scalar(words, k)).collect() };
        KeyTable3 { timeline, points, scalars }
    }
}

/// A stream's keys as the tangents and windows read them: from its decoded keys when it has
/// them, else from the bits.
#[derive(Clone, Copy)]
struct Keys3<'a> {
    header: &'a StreamHeader,
    words: &'a [u32],
    table: Option<&'a KeyTable3>,
}

impl Keys3<'_> {
    fn time(&self, k: u32) -> i32 {
        self.table.and_then(|t| t.timeline.times.get(k as usize)).map_or_else(|| self.header.time(self.words, k), |&t| t)
    }

    fn scalar(&self, k: u32) -> f32 {
        self.table.and_then(|t| t.scalars.get(k as usize)).map_or_else(|| self.header.scalar(self.words, k), |&s| s)
    }

    fn point(&self, k: u32) -> [f32; 3] {
        self.table.and_then(|t| t.points.get(k as usize)).map_or_else(|| self.header.point(self.words, k), |&p| p)
    }

    /// Key `k`'s tangent (801BCEF4): `outgoing` towards key k + 1 (kind 1, a window's T0) or
    /// incoming from key k - 1 (kind 0, its T1). Inside, the two differences weighted by
    /// 1 + and 1 - the key's bias, times this side's frames over both sides'. At an end, the
    /// one difference (two keys) or 1.5 x it - 0.5 x the next key's tangent.
    fn tangent(&self, outgoing: bool, k: u32) -> [f32; 3] {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let end = |d: [f32; 3], next: [f32; 3]| [0, 1, 2].map(|i| d[i] * 1.5 - next[i] * 0.5);
        let keys = self.header.keys;
        let b = self.point(k);
        if k == 0 {
            let d = sub(self.point(1), b);
            return if keys == 2 { d } else { end(d, self.tangent(false, 1)) };
        }
        if k + 1 == keys {
            let d = sub(b, self.point(k - 1));
            return if keys == 2 { d } else { end(d, self.tangent(true, k - 1)) };
        }
        let (a, c) = (self.point(k - 1), self.point(k + 1));
        let (ta, tb, tc) = (self.time(k - 1), self.time(k), self.time(k + 1));
        let side = if outgoing { tc - tb } else { tb - ta };
        let ratio = side as f32 / ((tb - ta) + (tc - tb)) as f32;
        let s = self.scalar(k);
        let (din, dout) = (sub(b, a), sub(c, b));
        [0, 1, 2].map(|i| (din[i] * (s + 1.0) + dout[i] * (1.0 - s)) * ratio)
    }

    fn window(&self, k: u32) -> (i32, i32, [f32; 12]) {
        let mut w = [0.0; 12];
        w[0..3].copy_from_slice(&self.point(k));
        w[3..6].copy_from_slice(&self.tangent(true, k));
        w[6..9].copy_from_slice(&self.tangent(false, k + 1));
        w[9..12].copy_from_slice(&self.point(k + 1));
        (self.time(k), self.time(k + 1), w)
    }
}

impl StreamHeader {
    /// The frames the keys span: the sum of their gaps + 1 (801BC318's +20).
    pub fn frames(&self, words: &[u32]) -> u32 {
        (0..self.keys).map(|k| bits(words, self.first_record + k * self.record_bits, self.gap_bits) + 1).sum()
    }

    /// The bit after the stream's last record.
    pub fn end(&self) -> u32 {
        self.first_record + self.keys * self.record_bits
    }

    fn record(&self, k: u32) -> u32 {
        self.first_record + k * self.record_bits
    }

    /// Key `k`'s frame: the gaps of keys 1..=k, each + 1 (key 0's gap is not used; 801BCD2C).
    pub fn time(&self, words: &[u32], k: u32) -> i32 {
        frame_of_key(words, self.first_record, self.record_bits, self.gap_bits, k)
    }

    /// Key `k`'s scalar (a bias): its signed field times the header's factor (801ADB4C).
    pub fn scalar(&self, words: &[u32], k: u32) -> f32 {
        if self.scalar_bits == 0 {
            return 0.0;
        }
        signed_bits(words, self.record(k) + self.gap_bits, self.scalar_bits) as f32 * self.scalar_scale
    }

    /// Key `k`'s three values (801BCE50, kind 2).
    pub fn point(&self, words: &[u32], k: u32) -> [f32; 3] {
        key3(words, self.record(k) + self.gap_bits + self.scalar_bits, self.value_bits, self.scale, self.offset)
    }

    /// Key `k`'s tangent (801BCEF4), read from the bits; see `Keys3::tangent`.
    pub fn tangent(&self, words: &[u32], outgoing: bool, k: u32) -> [f32; 3] {
        Keys3 { header: self, words, table: None }.tangent(outgoing, k)
    }

    /// The window between keys `k` and k + 1 as the evaluator holds it: first and last frames
    /// and P0, T0, T1, P1 (801BCBBC / 801BCCB0 at the start, 801BC958 stepping forward).
    pub fn window(&self, words: &[u32], k: u32) -> (i32, i32, [f32; 12]) {
        Keys3 { header: self, words, table: None }.window(k)
    }
}

/// The header of the stream at bit `at` of `words` (801BC318).
pub fn stream_header(words: &[u32], at: u32) -> StreamHeader {
    let mut p = at;
    let mut take = |w: u32| {
        let v = bits(words, p, w);
        p += w;
        v
    };
    let keys = take(20);
    let gap_bits = take(5);
    let scalar_bits = take(5);
    let scalar_scale = if scalar_bits == 0 { 0.0 } else { 1.0 / ((1i64 << (scalar_bits - 1)) - 1) as i32 as f32 };
    let value_bits = match take(5) {
        0 => 32,
        w => w,
    };
    let factor = 1.0 / ((1u64 << value_bits) - 1) as u32 as i32 as f32;
    let (mut scale, mut offset) = ([0.0; 3], [0.0; 3]);
    for i in 0..3 {
        scale[i] = factor * f32::from_bits(take(32));
        offset[i] = f32::from_bits(take(32));
    }
    let record_bits = gap_bits + scalar_bits + 3 * value_bits;
    StreamHeader { keys, gap_bits, scalar_bits, value_bits, scalar_scale, scale, offset, first_record: p, record_bits }
}

/// A scalar channel's stream (a node's words 5 and 6: a light's factor and radius; the evaluator
/// is 801BD7BC, `Anim_ScalarStream`). It is a vector stream with one value: the key count (20
/// bits), the widths of the gap, the bias and the value (5 bits each, a value width of 0 is 32),
/// the value's range and minimum as two 32-bit floats, then records of the gap, the bias and
/// the value. The window, the tangents and the curve are the vector streams' (`Keys3`,
/// `channel3`), on the first component.
#[derive(Debug)]
pub struct ScalarStream {
    header: StreamHeader,
    table: KeyTable3,
}

impl ScalarStream {
    /// The stream at bit `at` of `words`, its keys decoded.
    pub fn decode(words: &[u32], at: u32) -> ScalarStream {
        let mut p = at;
        let mut take = |w: u32| {
            let v = bits(words, p, w);
            p += w;
            v
        };
        let keys = take(20);
        let gap_bits = take(5);
        let scalar_bits = take(5);
        let scalar_scale = if scalar_bits == 0 { 0.0 } else { 1.0 / ((1i64 << (scalar_bits - 1)) - 1) as i32 as f32 };
        let value_bits = match take(5) {
            0 => 32,
            w => w,
        };
        let factor = 1.0 / ((1u64 << value_bits) - 1) as u32 as i32 as f32;
        let scale = factor * f32::from_bits(take(32));
        let offset = f32::from_bits(take(32));
        let record_bits = gap_bits + scalar_bits + value_bits;
        let header = StreamHeader { keys, gap_bits, scalar_bits, value_bits, scalar_scale, scale: [scale, 0.0, 0.0], offset: [offset, 0.0, 0.0], first_record: p, record_bits };
        let timeline = Timeline::decode(words, keys, p, record_bits, gap_bits);
        let points = (0..keys).map(|k| [(bits(words, p + k * record_bits + gap_bits + scalar_bits, value_bits) as f32).mul_add(scale, offset), 0.0, 0.0]).collect();
        let scalars = if scalar_bits == 0 { Vec::new() } else { (0..keys).map(|k| header.scalar(words, k)).collect() };
        ScalarStream { header, table: KeyTable3 { timeline, points, scalars } }
    }

    /// The value at `place` (frames), the stream read from its start: the window that holds the
    /// place (the last for a place past the end), curved.
    pub fn read(&self, words: &[u32], place: f32) -> f32 {
        let keys = self.header.keys;
        if keys < 2 {
            return self.table.points.first().map_or(0.0, |p| p[0]);
        }
        let k = Keys3 { header: &self.header, words, table: Some(&self.table) };
        let at = (0..keys - 1).find(|&i| place <= k.time(i + 1) as f32).unwrap_or(keys - 2);
        let (first, last, window) = k.window(at);
        channel3(first, last, 0, &window, place)[0]
    }
}

/// A key's three values in a stream's bits (801BCE50): `width`-bit unsigned fields from bit
/// `at`, each times its scale plus its offset (one fused multiply-add).
pub fn key3(words: &[u32], at: u32, width: u32, scale: [f32; 3], offset: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| (bits(words, at + i as u32 * width, width) as f32).mul_add(scale[i], offset[i]))
}

/// A scale or translation channel's value inside its stream's window (Bustin' Out's 801BC4E0,
/// called from EAnimController::Animate): the window's key frames `first` and `last`, its
/// `keys` (P0, T0, T1, P1: the two keys and their tangents), at `place` (in frames). A
/// nonzero `mode` blends the keys straight; else a cubic Hermite curve. The f32 operations
/// and their order are the original's (one fused multiply-add), so the result is its own to
/// the bit.
pub fn channel3(first: i32, last: i32, mode: u8, keys: &[f32; 12], place: f32) -> [f32; 3] {
    let (f0, f1) = (first as f32, last as f32);
    let t = (place - f0) / (f1 - f0);
    let p0 = [keys[0], keys[1], keys[2]];
    let t0 = [keys[3], keys[4], keys[5]];
    let t1 = [keys[6], keys[7], keys[8]];
    let p1 = [keys[9], keys[10], keys[11]];
    if mode != 0 {
        return [0, 1, 2].map(|i| p0[i] + (p1[i] - p0[i]) * t);
    }
    let t2 = t * t;
    let t3 = t2 * t;
    let three_t2 = t2 * 3.0;
    let h00 = ((t3 + t3) - three_t2) + 1.0;
    let h10 = (t3 - (t2 + t2)) + t;
    let h01 = t3.mul_add(-2.0, three_t2);
    let h11 = t3 - t2;
    [0, 1, 2].map(|i| ((p0[i] * h00 + t0[i] * h10) + p1[i] * h01) + t1[i] * h11)
}

/// A track's place one frame on (Bustin' Out's Anim_UpdateTrack, 80157318): the frame time
/// (`frame_time`, the game's r13-29720, 0.033) times the controller's speed (+80, 5 in
/// fast-forward) times the track's +72, then a fused multiply-add of that by the track's +0 (its
/// rate) onto the place, in double precision rounded to single as the game's fmadd/frsp does.
pub fn track_advance(place: f32, frame_time: f32, controller_speed: f32, track_speed: f32, rate: f32) -> f32 {
    let step = (frame_time * controller_speed) * track_speed;
    (step as f64).mul_add(rate as f64, place as f64) as f32
}

/// A looping track's place past its last frame, brought back by the last frame as many times as
/// it takes (Anim_UpdateTrack's end).
pub fn wrap_back(mut place: f32, last: f32) -> f32 {
    while place > last && last > 0.0 {
        place -= last;
    }
    place
}

/// A rotation stream's window evaluated at `t` (fn_801B5F48, mode 0): four quaternions w0..w3,
/// m = lerp(w1, w2), then lerp(lerp(w0, m), lerp(m, w3)), normalized (`reciprocal_length`, then
/// each component times it).
pub fn rotation_window(w: [[f32; 4]; 4], t: f32) -> [f32; 4] {
    let lerp = |a: [f32; 4], b: [f32; 4]| quat_lerp(a, b, t);
    let m = lerp(w[1], w[2]);
    normalize_quat(lerp(lerp(w[0], m), lerp(m, w[3])))
}

/// The rotation stream's blend (fn_801B5F48, mode 1, after a jump): lerp(w0, w3, t) normalized
/// as the window is.
pub fn rotation_blend(w: [[f32; 4]; 4], t: f32) -> [f32; 4] {
    normalize_quat(quat_lerp(w[0], w[3], t))
}

fn quat_lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// `a * b + c` as the original's fmadds do it: the product exact, the sum rounded in double and
/// then to single.
pub fn fma32(a: f32, b: f32, c: f32) -> f32 {
    f64::from(a).mul_add(f64::from(b), f64::from(c)) as f32
}

/// `a * a + acc` fused (`fma32`).
fn fused_square(a: f32, acc: f32) -> f32 {
    fma32(a, a, acc)
}

/// A quaternion's reciprocal length as the game computes it: y*y, then x, z and w fused in, each
/// rounded to single; `None` for length 0.
fn reciprocal_length(v: [f32; 4]) -> Option<f32> {
    let length = fused_square(v[3], fused_square(v[2], fused_square(v[0], v[1] * v[1]))).sqrt();
    (length != 0.0).then(|| 1.0 / length)
}

fn normalize_quat(v: [f32; 4]) -> [f32; 4] {
    reciprocal_length(v).map_or(v, |k| v.map(|x| x * k))
}

/// A rotation stream's header, as Bustin' Out's 801B5DC8 reads it from the animation's bits:
/// the key count (20 bits), the widths of the frame gap, of the bias and of each point
/// component (5 bits each; a point width of 0 is 32). A record is its gap, its bias and the
/// point's y, z and w, each a signed field, then one sign bit for x. The point's fields are
/// scaled by 1 / (2^(width - 1) - 1), the bias likewise (0 with no bias).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotHeader {
    pub keys: u32,
    pub gap_bits: u32,
    pub bias_bits: u32,
    pub point_bits: u32,
    pub point_scale: f32,
    pub bias_scale: f32,
    /// The first key's record: the bit after the header.
    pub first_record: u32,
    /// A record's bits.
    pub record_bits: u32,
}

/// The header of the rotation stream at bit `at` of `words` (801B5DC8).
pub fn rotation_header(words: &[u32], at: u32) -> RotHeader {
    let mut p = at;
    let mut take = |w: u32| {
        let v = bits(words, p, w);
        p += w;
        v
    };
    let keys = take(20);
    let gap_bits = take(5);
    let bias_bits = take(5);
    let point_bits = match take(5) {
        0 => 32,
        w => w,
    };
    // 801ADBCC: 1 / the largest signed field, in single precision
    let scale = |w: u32| 1.0 / ((1i64 << (w - 1)) - 1) as i32 as f32;
    let bias_scale = if bias_bits == 0 { 0.0 } else { scale(bias_bits) };
    let record_bits = gap_bits + bias_bits + 3 * point_bits + 1;
    RotHeader { keys, gap_bits, bias_bits, point_bits, point_scale: scale(point_bits), bias_scale, first_record: p, record_bits }
}

/// A flag channel (a node's fourth word (+16), `Animate`'s state bit 0x10): one bit per frame in the
/// animation's bit array, the stream being nothing but the bit its first frame sits at (801BD4C8
/// stores the bits object and that bit; the node word is it negated, as the other channels').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlagStream {
    pub start: u32,
}

impl FlagStream {
    /// The flag stream a node's flag word names (`Channel::Stream`), if it does.
    pub fn of(word: u32) -> Option<FlagStream> {
        match Channel::of(word) {
            Channel::Stream(start) => Some(FlagStream { start }),
            _ => None,
        }
    }

    /// The flag at `place` (801BD4D4): the bit at the start plus the place, the sum in single
    /// precision, cut toward zero; a bit past the array's end, or before its start, is clear.
    pub fn read(&self, words: &[u32], total_bits: u32, place: f32) -> bool {
        // 802C38B0 is the double 2^52 + 2^31 the original's int-to-float trick subtracts: the
        // start as an exact integer, then the float sum
        let at = (self.start as i32 as f32 + place) as i32 as u32;
        at < total_bits && words.get((at >> 5) as usize).is_some_and(|w| w >> (at & 31) & 1 != 0)
    }
}

/// `width` bits at bit `at` as a signed field (801ADB4C): sign-extended from its top bit.
fn signed_bits(words: &[u32], at: u32, width: u32) -> i32 {
    let v = bits(words, at, width);
    if width != 0 && width < 32 && v >> (width - 1) & 1 != 0 { (v | !0u32 << (width - 1)) as i32 } else { v as i32 }
}

fn quat_add(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + b[i])
}

fn quat_sub(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    core::array::from_fn(|i| a[i] - b[i])
}

fn quat_scale(a: [f32; 4], s: f32) -> [f32; 4] {
    a.map(|v| s * v)
}

/// A quaternion made unit length as 801B6C44 does it (the length and division of
/// `normalize_quat`); a zero length gives the identity (801B52F8).
pub fn unit_or_identity(v: [f32; 4]) -> [f32; 4] {
    reciprocal_length(v).map_or([0.0, 0.0, 0.0, 1.0], |k| v.map(|x| x * k))
}

/// A rotation stream's keys decoded once (`Anim::node_keys`); see `KeyTable3`.
#[derive(Debug)]
pub(crate) struct RotTable {
    timeline: Timeline,
    points: Vec<[f32; 4]>,
    /// Empty for a stream without biases (they read 0).
    biases: Vec<f32>,
}

impl RotTable {
    fn decode(words: &[u32], h: &RotHeader) -> RotTable {
        let timeline = Timeline::decode(words, h.keys, h.first_record, h.record_bits, h.gap_bits);
        let points = (0..h.keys).map(|k| h.point(words, k)).collect();
        let biases = if h.bias_bits == 0 { Vec::new() } else { (0..h.keys).map(|k| h.bias(words, k)).collect() };
        RotTable { timeline, points, biases }
    }
}

/// A rotation stream's keys as the control points and windows read them; see `Keys3`.
#[derive(Clone, Copy)]
struct RotKeys<'a> {
    header: &'a RotHeader,
    words: &'a [u32],
    table: Option<&'a RotTable>,
}

impl RotKeys<'_> {
    fn time(&self, k: u32) -> i32 {
        self.table.and_then(|t| t.timeline.times.get(k as usize)).map_or_else(|| self.header.time(self.words, k), |&t| t)
    }

    fn bias(&self, k: u32) -> f32 {
        self.table.and_then(|t| t.biases.get(k as usize)).map_or_else(|| self.header.bias(self.words, k), |&b| b)
    }

    fn point(&self, k: u32) -> [f32; 4] {
        self.table.and_then(|t| t.points.get(k as usize)).map_or_else(|| self.header.point(self.words, k), |&p| p)
    }

    /// The control point of key `k` (801B6C44, slots 1 and 0): `outgoing` towards key k + 1 (a
    /// window's w1) or incoming from key k - 1 (the w2 of the window before). At the first key
    /// the key moves (1 + its bias) / 2 of the way to key 1, at the last (1 - its bias) / 2 of
    /// the way to the one before. Between them it moves that way to each neighbour (X, Y), M
    /// is the unit mean of the two, and the control point is the key moved from M by 2 x
    /// this side's frames over both sides'. Each result is made unit length.
    fn control(&self, k: u32, outgoing: bool) -> [f32; 4] {
        let a = self.point(k);
        let bias = self.bias(k);
        let moved = |to: [f32; 4], weight: f32| unit_or_identity(quat_add(a, quat_scale(quat_sub(to, a), weight)));
        if k == 0 {
            return moved(self.point(1), (bias + 1.0) * 0.5);
        }
        if k + 1 == self.header.keys {
            return moved(self.point(k - 1), (1.0 - bias) * 0.5);
        }
        let (before, after) = (self.time(k) - self.time(k - 1), self.time(k + 1) - self.time(k));
        let (side, sign) = if outgoing { (after, -1.0f32) } else { (before, 1.0f32) };
        let ratio = side as f32 / (before + after) as f32;
        let x = moved(self.point(k - 1), -((bias + 1.0) * 0.5));
        let y = moved(self.point(k + 1), (1.0 - bias) * 0.5);
        let m = unit_or_identity(quat_add(x, quat_scale(quat_sub(y, x), 0.5)));
        moved(m, (sign * ratio) * -2.0)
    }

    fn window(&self, k: u32) -> (i32, i32, [[f32; 4]; 4]) {
        let w = [self.point(k), self.control(k, true), self.control(k + 1, false), self.point(k + 1)];
        (self.time(k), self.time(k + 1), w)
    }
}

impl RotHeader {
    fn record(&self, k: u32) -> u32 {
        self.first_record + k * self.record_bits
    }

    /// The bit after the stream's last record.
    pub fn end(&self) -> u32 {
        self.record(self.keys)
    }

    /// The frames the keys span: the sum of their gaps + 1 (801B5DC8's +16).
    pub fn frames(&self, words: &[u32]) -> u32 {
        (0..self.keys).map(|k| bits(words, self.record(k), self.gap_bits) + 1).sum()
    }

    /// Key `k`'s frame: the gaps of keys 1..=k, each + 1 (801B6A1C).
    pub fn time(&self, words: &[u32], k: u32) -> i32 {
        frame_of_key(words, self.first_record, self.record_bits, self.gap_bits, k)
    }

    /// Key `k`'s bias: its signed field times the header's factor (801B6A1C).
    pub fn bias(&self, words: &[u32], k: u32) -> f32 {
        let v = signed_bits(words, self.record(k) + self.gap_bits, self.bias_bits);
        self.bias_scale * (v as f32)
    }

    /// Key `k`'s quaternion x, y, z, w (801B6B40): y, z and w are fields; x is the root of what
    /// is left of 1 (0 when nothing is), negative when the record's last bit is set. The
    /// squares are summed z first, then y and w fused in, each rounded to single.
    pub fn point(&self, words: &[u32], k: u32) -> [f32; 4] {
        let at = self.record(k) + self.gap_bits + self.bias_bits;
        let field = |i: u32| self.point_scale * (signed_bits(words, at + i * self.point_bits, self.point_bits) as f32);
        let (y, z, w) = (field(0), field(1), field(2));
        let left = 1.0 - fused_square(w, fused_square(y, z * z));
        let x = if left <= 0.0 {
            0.0
        } else if bits(words, at + 3 * self.point_bits, 1) != 0 {
            -left.sqrt()
        } else {
            left.sqrt()
        };
        [x, y, z, w]
    }

    /// The control point of key `k`, read from the bits; see `RotKeys::control`.
    pub fn control(&self, words: &[u32], k: u32, outgoing: bool) -> [f32; 4] {
        RotKeys { header: self, words, table: None }.control(k, outgoing)
    }

    /// The window between keys `k` and k + 1 as the evaluator holds it: first and last frames
    /// and the quaternions w0 (key k), w1 (its outgoing control point), w2 (key k + 1's
    /// incoming control point) and w3 (key k + 1) (801B69A0).
    pub fn window(&self, words: &[u32], k: u32) -> (i32, i32, [[f32; 4]; 4]) {
        RotKeys { header: self, words, table: None }.window(k)
    }
}

/// A rotation stream as the evaluator runs it (Anim_RotStream 801B5F48): the vec3 `Stream`'s
/// stepping, on quaternion windows.
#[derive(Clone, Debug)]
pub struct RotStream {
    pub header: RotHeader,
    pub key: u32,
    pub first: i32,
    pub last: i32,
    /// 1: the window is blended straight (moved a third of it or more since the last read).
    pub mode: u8,
    /// Whether a window step has set `mode` yet: the game's constructor leaves the flag as the
    /// memory held it (the native one starts at 0), so a read that steps no window uses a flag
    /// this does not know.
    pub mode_set: bool,
    pub last_place: f32,
    pub window: [[f32; 4]; 4],
    table: Arc<RotTable>,
}

impl RotStream {
    /// A stream at its start (801B68AC): the first window.
    pub fn new(words: &[u32], header: RotHeader) -> RotStream {
        let table = Arc::new(RotTable::decode(words, &header));
        RotStream::with_table(words, header, table)
    }

    /// A stream at its start on keys decoded already (`Anim::node_keys`).
    pub(crate) fn with_table(words: &[u32], header: RotHeader, table: Arc<RotTable>) -> RotStream {
        let mut s = RotStream { header, key: 0, first: 0, last: 0, mode: 0, mode_set: false, last_place: 0.0, window: [[0.0; 4]; 4], table };
        s.reset(words);
        s
    }

    fn keys<'a>(&'a self, words: &'a [u32]) -> RotKeys<'a> {
        RotKeys { header: &self.header, words, table: Some(&self.table) }
    }

    fn fill(&mut self, words: &[u32]) {
        self.window = self.keys(words).window(self.key).2;
    }

    /// Back to the first window (801B68AC); the last place is FLT_MAX, the flag is kept.
    fn reset(&mut self, words: &[u32]) {
        self.key = 0;
        self.first = 0;
        self.last = self.keys(words).time(1);
        self.last_place = f32::MAX;
        self.fill(words);
    }

    /// To the last window (801B6918): it ends at the stream's frames - 1; the last place is
    /// -FLT_MAX.
    fn jump_to_end(&mut self, words: &[u32]) {
        let (keys, frames) = (self.header.keys, self.table.timeline.frames);
        self.key = keys - 2;
        self.last = frames as i32 - 1;
        let k = self.keys(words);
        self.first = self.last - (k.time(keys - 1) - k.time(keys - 2));
        self.last_place = -f32::MAX;
        self.fill(words);
    }

    /// One window on (801B6638): blended when the place moved a third of the window or more,
    /// else its control points are made again.
    fn forward(&mut self, words: &[u32], place: f32) {
        self.key += 1;
        self.first = self.last;
        self.last = self.keys(words).time(self.key + 1);
        self.mode_set = true;
        self.mode = ((place - self.last_place) >= (self.last - self.first) as f32 * (1.0 / 3.0)) as u8;
        if self.mode == 0 {
            let (outgoing, incoming) = (self.keys(words).control(self.key, true), self.keys(words).control(self.key + 1, false));
            self.window[1] = outgoing;
            self.window[2] = incoming;
        }
        self.window[0] = self.window[3];
        self.window[3] = self.keys(words).point(self.key + 1);
    }

    /// One window back (801B6774), the same rule the other way.
    fn back(&mut self, words: &[u32], place: f32) {
        self.key -= 1;
        self.last = self.first;
        self.first = self.keys(words).time(self.key);
        self.mode_set = true;
        self.mode = ((self.last_place - place) >= (self.last - self.first) as f32 * (1.0 / 3.0)) as u8;
        if self.mode == 0 {
            let (outgoing, incoming) = (self.keys(words).control(self.key, true), self.keys(words).control(self.key + 1, false));
            self.window[1] = outgoing;
            self.window[2] = incoming;
        }
        self.window[3] = self.window[0];
        self.window[0] = self.keys(words).point(self.key);
    }

    /// The rotation at `place` (801B5F48): the window moved to hold it (stepping, or from the
    /// nearer end when that is closer), then evaluated or blended.
    pub fn read(&mut self, words: &[u32], place: f32) -> [f32; 4] {
        let (first, last) = (self.first as f32, self.last as f32);
        if place > last {
            if place - last <= (self.table.timeline.frames as i32 - 1) as f32 - place {
                while place > self.last as f32 {
                    self.forward(words, place);
                }
            } else {
                self.jump_to_end(words);
                while place < self.first as f32 {
                    self.back(words, place);
                }
            }
        } else if place < first {
            if first - place <= place {
                while place < self.first as f32 {
                    self.back(words, place);
                }
            } else {
                self.reset(words);
                while place > self.last as f32 {
                    self.forward(words, place);
                }
            }
        }
        self.last_place = place;
        let (first, last) = (self.first as f32, self.last as f32);
        let t = (place - first) / (last - first);
        if self.mode == 0 { rotation_window(self.window, t) } else { rotation_blend(self.window, t) }
    }
}

/// A Sim's animator speeds (EA's fn_800ABE20, fn_800B0ADC, fn_800AF06C): the animator runs at
/// the game speed (0 paused, 1, 5 fast-forward). On a change it sets the new speed on its slot
/// tracks: the base idle, walk, turn, the current action, the action blending out, the carry.
/// A new action starts at the speed, or at minus it backward. The face is held at 0.
#[derive(Clone, Copy, Debug)]
pub struct SimAnimator {
    pub speed: f32,
}

/// The face track's speed (the 802A8E44 constant).
pub const FACE_SPEED: f32 = 0.0;

impl Default for SimAnimator {
    fn default() -> Self {
        SimAnimator { speed: 1.0 }
    }
}

impl SimAnimator {
    /// The game speed for this frame; true when it changed (the slot tracks are then set to it).
    pub fn follow(&mut self, game_speed: f32) -> bool {
        let changed = self.speed != game_speed;
        self.speed = game_speed;
        changed
    }

    /// The animator's speed for a tick (Anim_SpeedUpdate 800AB820): the game speed, but 0 on a
    /// tick an Animate's load misses (anyone's) unless this Sim is walking a route.
    pub fn speed_for(game_speed: f32, load_miss: bool, routing: bool) -> f32 {
        if load_miss && !routing { 0.0 } else { game_speed }
    }

    /// A new action's track speed.
    pub fn action_speed(&self, backward: bool) -> f32 {
        if backward { -self.speed } else { self.speed }
    }
}

/// A track's frame (Anim_UpdateTrack 80157318 to 801575F4): the place moved by
/// `track_advance`, then at the ends by the track's end mode (+13). Past the last frame, mode 0
/// (a loop) comes back by the last frame (`wrap_back`), and the others stop at the last frame.
/// Before 0, mode 0 goes on by the last frame until it is not below 0 (at most the last frame),
/// and the others stop at 0. Gives the new place and whether an end was passed (+160 set).
pub fn track_step(place: f32, frame_time: f32, controller_speed: f32, track_speed: f32, rate: f32, last: f32, end_mode: u8) -> (f32, bool) {
    let next = track_advance(place, frame_time, controller_speed, track_speed, rate);
    if next > last {
        return (if end_mode == 0 { wrap_back(next, last).max(0.0) } else { last }, true);
    }
    if next < 0.0 {
        if end_mode != 0 {
            return (0.0, true);
        }
        let mut p = next;
        while p < 0.0 && last > 0.0 {
            p += last;
        }
        return (p.min(last), true);
    }
    (next, false)
}

/// A scale or translation stream as the evaluator runs it (801BC4E0): its window between keys
/// `key` and `key` + 1, the blend flag, and the place of the last read.
#[derive(Clone, Debug)]
pub struct Stream {
    pub header: StreamHeader,
    pub key: u32,
    pub first: i32,
    pub last: i32,
    /// 1: the window is blended straight (moved a third of it or more since the last read).
    pub mode: u8,
    /// Whether a window step has set `mode` yet: the game's constructor leaves the flag as the
    /// memory held it (the native one starts at 0), so a read that steps no window uses a flag
    /// this does not know.
    pub mode_set: bool,
    pub last_place: f32,
    pub window: [f32; 12],
    table: Arc<KeyTable3>,
}

impl Stream {
    /// A stream at its start (801BC318 then 801BCBBC): the first window, curved.
    pub fn new(words: &[u32], header: StreamHeader) -> Stream {
        let table = Arc::new(KeyTable3::decode(words, &header));
        Stream::with_table(words, header, table)
    }

    /// A stream at its start on keys decoded already (`Anim::node_keys`).
    pub(crate) fn with_table(words: &[u32], header: StreamHeader, table: Arc<KeyTable3>) -> Stream {
        let mut s = Stream { header, key: 0, first: 0, last: 0, mode: 0, mode_set: false, last_place: 0.0, window: [0.0; 12], table };
        s.reset(words);
        s
    }

    fn keys<'a>(&'a self, words: &'a [u32]) -> Keys3<'a> {
        Keys3 { header: &self.header, words, table: Some(&self.table) }
    }

    fn fill(&mut self, words: &[u32]) {
        self.window = self.keys(words).window(self.key).2;
    }

    /// Back to the first window (801BCBBC); the last place is FLT_MAX, the flag is kept.
    fn reset(&mut self, words: &[u32]) {
        self.key = 0;
        self.first = 0;
        self.last = self.keys(words).time(1);
        self.last_place = f32::MAX;
        self.fill(words);
    }

    /// To the last window (801BCC28): it ends at the stream's frames - 1; the last place is
    /// -FLT_MAX.
    fn jump_to_end(&mut self, words: &[u32]) {
        let (keys, frames) = (self.header.keys, self.table.timeline.frames);
        self.key = keys - 2;
        self.last = frames as i32 - 1;
        let k = self.keys(words);
        self.first = self.last - (k.time(keys - 1) - k.time(keys - 2));
        self.last_place = -f32::MAX;
        self.fill(words);
    }

    /// One window on (801BC958): blended when the place moved a third of the window or more.
    fn forward(&mut self, words: &[u32], place: f32) {
        self.key += 1;
        self.first = self.last;
        self.last = self.keys(words).time(self.key + 1);
        self.mode_set = true;
        self.mode = ((place - self.last_place) >= (self.last - self.first) as f32 * (1.0 / 3.0)) as u8;
        let p1 = [self.window[9], self.window[10], self.window[11]];
        self.window[0..3].copy_from_slice(&p1);
        if self.mode == 0 {
            let (outgoing, incoming) = (self.keys(words).tangent(true, self.key), self.keys(words).tangent(false, self.key + 1));
            self.window[3..6].copy_from_slice(&outgoing);
            self.window[6..9].copy_from_slice(&incoming);
        }
        let p1 = self.keys(words).point(self.key + 1);
        self.window[9..12].copy_from_slice(&p1);
    }

    /// One window back (801BCA8C), the same rule the other way.
    fn back(&mut self, words: &[u32], place: f32) {
        self.key -= 1;
        self.last = self.first;
        self.first = self.keys(words).time(self.key);
        self.mode_set = true;
        self.mode = ((self.last_place - place) >= (self.last - self.first) as f32 * (1.0 / 3.0)) as u8;
        let p0 = [self.window[0], self.window[1], self.window[2]];
        self.window[9..12].copy_from_slice(&p0);
        if self.mode == 0 {
            let (outgoing, incoming) = (self.keys(words).tangent(true, self.key), self.keys(words).tangent(false, self.key + 1));
            self.window[3..6].copy_from_slice(&outgoing);
            self.window[6..9].copy_from_slice(&incoming);
        }
        let p0 = self.keys(words).point(self.key);
        self.window[0..3].copy_from_slice(&p0);
    }

    /// The value at `place` (801BC4E0): the window moved to hold it (stepping, or from the
    /// nearer end when that is closer), then curved or blended.
    pub fn read(&mut self, words: &[u32], place: f32) -> [f32; 3] {
        let (first, last) = (self.first as f32, self.last as f32);
        if place > last {
            if place - last <= (self.table.timeline.frames as i32 - 1) as f32 - place {
                while place > self.last as f32 {
                    self.forward(words, place);
                }
            } else {
                self.jump_to_end(words);
                while place < self.first as f32 {
                    self.back(words, place);
                }
            }
        } else if place < first {
            if first - place <= place {
                while place < self.first as f32 {
                    self.back(words, place);
                }
            } else {
                self.reset(words);
                while place > self.last as f32 {
                    self.forward(words, place);
                }
            }
        }
        self.last_place = place;
        channel3(self.first, self.last, self.mode, &self.window, place)
    }
}

/// The Sims' plays, as the game keeps them: one a Sim, asked by its Animate steps and stepped by
/// the frames, in the game's order. A Sim's play goes on across Animate calls of the same
/// animation until it has given true; another animation, or a call after true, starts anew (once
/// the animation is in memory: `Loads`).
#[derive(Clone, Debug, Default)]
pub struct Animator {
    pub loads: Loads,
    /// The notes of every animation, by id (`notes`).
    pub notes: std::collections::BTreeMap<u32, Vec<Note>>,
    plays: Plays,
}

/// The Sims' plays alone: a Sim's play (its animation's id, the play and the animation's rate) by
/// the Sim. What a frame steps, so that a look one frame ahead copies only these and not the
/// animations' loads and notes.
#[derive(Clone, Debug, Default)]
pub struct Plays(std::collections::BTreeMap<u32, (u32, Play, f32)>, u32, std::collections::BTreeMap<u32, u32>);

impl Plays {
    /// The number of the Sim's current play among all the plays started (1 for the first): a
    /// play that starts anew, even of the same animation, has a number no earlier one had.
    pub fn serial(&self, sim: u32) -> Option<u32> {
        self.2.get(&sim).copied()
    }

    /// Every Sim's play: the Sim (the key `Animator::animate` was given), its animation's id and
    /// the play.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, &Play)> + '_ {
        self.0.iter().map(|(sim, p)| (*sim, p.0, &p.1))
    }

    /// A frame: every play steps by 0.033 s x its rate x its Sim's `speed` this frame.
    pub fn frame(&mut self, mut speed: impl FnMut(u32) -> f32) {
        for (sim, p) in self.0.iter_mut() {
            p.1.frame(crate::routing::WALK_FRAME * p.2 * speed(*sim));
        }
    }
}

impl Animator {
    pub fn new(loads: Loads, notes: std::collections::BTreeMap<u32, Vec<Note>>) -> Animator {
        Animator { loads, notes, plays: Default::default() }
    }

    /// An Animate step of `sim` at `tick` for the animation `id` (`anim` its data), played
    /// `backward` and asking `wanted` counted events. Pending while it waits to be in memory.
    pub fn animate(&mut self, sim: u32, id: u32, anim: &Anim, backward: bool, wanted: i32, tick: u32) -> Poll {
        if self.plays.0.get(&sim).is_some_and(|p| p.0 != id || p.1.answered()) {
            self.plays.0.remove(&sim);
        }
        if !self.plays.0.contains_key(&sim) {
            if !self.loads.ask(id, tick) {
                return Poll::Pending;
            }
            let play = Play::new(anim, self.notes.get(&id).map_or(&[][..], |v| v.as_slice()), backward, wanted);
            self.plays.0.insert(sim, (id, play, anim.rate));
            self.plays.1 += 1;
            self.plays.2.insert(sim, self.plays.1);
        }
        self.plays.0.get_mut(&sim).map_or(Poll::Pending, |p| p.1.poll())
    }

    /// `sim`'s play: its animation's id and the play.
    pub fn play(&self, sim: u32) -> Option<(u32, &Play)> {
        self.plays.0.get(&sim).map(|p| (p.0, &p.1))
    }

    /// The number of `sim`'s current play (`Plays::serial`).
    pub fn serial(&self, sim: u32) -> Option<u32> {
        self.plays.serial(sim)
    }

    /// Every Sim's play: the Sim (the key `animate` was given), its animation's id and the play.
    pub fn plays(&self) -> impl Iterator<Item = (u32, u32, &Play)> + '_ {
        self.plays.iter()
    }

    /// The plays alone, copied.
    pub fn plays_copy(&self) -> Plays {
        self.plays.clone()
    }

    /// The plays, for the frames to step.
    pub fn plays_mut(&mut self) -> &mut Plays {
        &mut self.plays
    }

    /// A frame: every play steps by 0.033 s × its rate × its Sim's `speed` this frame.
    pub fn frame(&mut self, speed: impl FnMut(u32) -> f32) {
        self.plays.frame(speed);
    }
}

/// An Animate operand's animation, looked up in its file's adult animation table (STR# 129).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// An archive animation, by its id.
    Found(u32),
    /// The table's name differs from the archive's only in case: the lookup is case-sensitive,
    /// so nothing plays and the primitive returns true at once, with no load asked (Global's
    /// "A2O-flamingo-kick"; kept as the game's).
    NotFound,
    /// No name, or one not in the archive: not modelled.
    Unknown,
}

/// The disc's animations: `animatio.arc`'s entries by id and by their name as written, every
/// animation's notes (the quick data's AnimRef rows) and a house's preload sets.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub by_id: std::collections::BTreeMap<u32, Anim>,
    exact: std::collections::HashMap<String, u32>,
    lower: std::collections::HashSet<String>,
    pub notes: std::collections::BTreeMap<u32, Vec<Note>>,
    sets: Vec<Vec<u8>>,
}

impl Catalog {
    /// The catalog of `disc`, with house `house`'s preload sets (House1Preload, housepreNN).
    pub fn read(disc: &crate::disc::Disc, house: u32) -> Catalog {
        let anims = disc.read("animatio.arc").unwrap_or_default();
        let quick = disc.read("quickdat.arc").unwrap_or_default();
        let ds = disc.read("datasets.arc").unwrap_or_default();
        let by_id: std::collections::BTreeMap<u32, Anim> = crate::arc::entries(&anims).unwrap_or_default().iter().filter_map(|e| Some((e.id, read(crate::arc::bytes(&anims, e))?))).collect();
        let exact = by_id.iter().map(|(id, a)| (a.name.clone(), *id)).collect();
        let lower = by_id.values().map(|a| a.name.to_ascii_lowercase()).collect();
        let notes = crate::arc::entries(&quick)
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.name == "SimsObjects")
            .and_then(|e| crate::quickdata::Image::read(crate::arc::bytes(&quick, &e)))
            .map(|img| notes(&img))
            .unwrap_or_default();
        let set = format!("housepre{house:02}");
        let sets = crate::arc::entries(&ds).unwrap_or_default().iter().filter(|e| e.name == "House1Preload" || e.name == set).map(|e| crate::arc::bytes(&ds, e).to_vec()).collect();
        Catalog { by_id, exact, lower, notes, sets }
    }

    /// The id of the animation of a name as written.
    pub fn id_of(&self, name: &str) -> Option<u32> {
        self.exact.get(name).copied()
    }

    /// The animation of a name as written.
    pub fn by_name(&self, name: &str) -> Option<&Anim> {
        self.by_id.get(self.exact.get(name)?)
    }

    /// The loads at the house's start: its preload sets in memory.
    pub fn loads(&self) -> Loads {
        let refs: Vec<&[u8]> = self.sets.iter().map(|v| v.as_slice()).collect();
        let ids = self.by_id.keys().copied().collect();
        Loads::with_sets(&refs, &ids)
    }

    /// The animation of an Animate `operand` whose table is `file`'s STR# `table` (bytes 0-1
    /// the index; byte 5's 0x04 no name).
    pub fn lookup(&self, lib: &crate::library::Library, file: Option<crate::FileId>, table: u16, operand: &[u8; 8]) -> Lookup {
        if operand[5] & 4 != 0 {
            return Lookup::Unknown;
        }
        let index = u16::from_le_bytes([operand[0], operand[1]]) as usize;
        let Some(path) = file.and_then(|f| lib.file(f).strings.get(&table)).and_then(|t| t.get(index)) else { return Lookup::Unknown };
        let base = path.rsplit(['\\', '/']).next().unwrap_or("").trim_end_matches(".max").trim_end_matches(".MAX");
        if base.is_empty() {
            // an empty name (Global's first: the scripts' "no animation"): none starts, true
            return Lookup::NotFound;
        }
        match self.exact.get(base) {
            Some(id) => Lookup::Found(*id),
            None if self.lower.contains(&base.to_ascii_lowercase()) => Lookup::NotFound,
            None => Lookup::Unknown,
        }
    }

    /// A new animator over the house's loads and the notes.
    pub fn animator(&self) -> Animator {
        Animator::new(self.loads(), self.notes.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_animation_is_read_to_its_end() {
        let mut e = b"walk\0".to_vec();
        e.extend(30u32.to_be_bytes());
        for _ in 0..6 {
            e.extend(1.0f32.to_be_bytes());
        }
        e.extend(1u32.to_be_bytes());
        for w in [0u32, 0, 1, u32::MAX, 0, 0, 0] {
            e.extend(w.to_be_bytes());
        }
        e.extend(1.5f32.to_be_bytes());
        e.extend(2u32.to_be_bytes());
        e.extend(0.25f32.to_be_bytes());
        e.extend(0.5f32.to_be_bytes());
        e.extend(33u32.to_be_bytes());
        e.extend([0u8; 8]);
        e.extend(30.0f32.to_be_bytes());
        e.extend([0u8; 8]);
        e.push(1);
        e.extend([0u8; 16]);
        e.extend([0, 0]);
        e.extend(1u32.to_be_bytes());
        let mut name = b"contact".to_vec();
        name.resize(64, 0);
        e.extend(name);
        e.extend([0u8; 16]);
        let a = read(&e).expect("read whole");
        assert_eq!((a.name.as_str(), a.frames, a.nodes.len(), a.constants.len(), a.bit_words.len(), a.events[0].name.as_str()), ("walk", 30, 1, 2, 2, "contact"));
        assert_eq!(a.seconds(), 1.0);
        e.push(0);
        assert_eq!(read(&e), None);
    }

    fn anim(frames: u32, rate: f32) -> Anim {
        Anim { name: String::new(), frames, first: [0.0; 3], second: [0.0; 3], nodes: vec![], word: 0, constants: vec![], bits: 0, bit_words: vec![], rate, floats: [0.0; 2], byte: 0, words: [0; 4], flags: [0; 2], events: vec![], compiled: Compiled::default() }
    }

    #[test]
    fn a_play_gives_its_notes_then_its_end() {
        // 31 frames at 30: notes at 100 ms (frame 3, value 101), 500 ms (15, counted 0) and
        // 600 ms (18, counted 2, not the next one: passed over)
        let notes = [Note { ms: 100, kind: 6, value: 101 }, Note { ms: 500, kind: 6, value: 0 }, Note { ms: 600, kind: 6, value: 2 }, Note { ms: 200, kind: 1, value: 7 }];
        let mut p = Play::new(&anim(31, 30.0), &notes, false, 2);
        let mut seen = vec![];
        for f in 0..40 {
            match p.poll() {
                Poll::Pending => {}
                e => seen.push((f, e)),
            }
            p.frame(1.0);
        }
        // the note at 3 seen after the frame that passes it; the end (30) passed at frame 31,
        // the counted event 1 owed at once then, and done
        assert_eq!(seen[..4], [(4, Poll::Event(101)), (16, Poll::Event(0)), (31, Poll::Event(1)), (32, Poll::Done)]);
        assert!(p.finished());
    }

    #[test]
    fn a_backward_play_stands_still_its_first_frame() {
        let mut p = Play::new(&anim(4, 30.0), &[], true, 0);
        let mut at = None;
        for f in 0..10 {
            if p.poll() == Poll::Done {
                at = Some(f);
                break;
            }
            p.frame(1.0);
        }
        // 3 frames to pass the last (3) after a still one
        assert_eq!(at, Some(5));
    }

    #[test]
    fn a_channel_word_is_none_a_constant_or_a_stream() {
        assert_eq!(Channel::of(0), Channel::None);
        assert_eq!(Channel::of(4), Channel::Constant(4));
        assert_eq!(Channel::of(-7954i32 as u32), Channel::Stream(7954));
    }

    #[test]
    fn a_stream_header_is_read_from_the_bits() {
        // 2 keys, widths 3 (gap), 0 (scalar), 4 (values); ranges 15, 0, 0 and minimums 1, 2, 3
        let mut w = vec![0u32; 12];
        let mut at = 0;
        let mut put = |v: u32, n: u32| {
            for i in 0..n {
                if v >> i & 1 != 0 {
                    w[((at + i) / 32) as usize] |= 1 << ((at + i) % 32);
                }
            }
            at += n;
        };
        for (v, n) in [(2, 20), (3, 5), (0, 5), (4, 5)] {
            put(v, n);
        }
        for (r, m) in [(15.0f32, 1.0f32), (0.0, 2.0), (0.0, 3.0)] {
            put(r.to_bits(), 32);
            put(m.to_bits(), 32);
        }
        // the gaps: 1 and 4
        put(1, 3);
        put(0, 12);
        put(4, 3);
        let h = stream_header(&w, 0);
        assert_eq!((h.keys, h.gap_bits, h.scalar_bits, h.value_bits, h.record_bits, h.first_record), (2, 3, 0, 4, 15, 227));
        assert_eq!((h.scale, h.offset, h.frames(&w), h.end()), ([1.0, 0.0, 0.0], [1.0, 2.0, 3.0], 2 + 5, 227 + 30));
    }

    #[test]
    fn a_window_has_its_keys_frames_and_tangents() {
        // 3 keys at frames 0, 2 and 3 with x = 0, 4, 6 (8-bit values, range 255, no scalar)
        let mut w = vec![0u32; 12];
        let mut at = 0;
        let mut put = |v: u32, n: u32| {
            for i in 0..n {
                if v >> i & 1 != 0 {
                    w[((at + i) / 32) as usize] |= 1 << ((at + i) % 32);
                }
            }
            at += n;
        };
        for (v, n) in [(3, 20), (2, 5), (0, 5), (8, 5)] {
            put(v, n);
        }
        for (r, m) in [(255.0f32, 0.0f32), (0.0, 0.0), (0.0, 0.0)] {
            put(r.to_bits(), 32);
            put(m.to_bits(), 32);
        }
        for (gap, x) in [(0, 0), (1, 4), (0, 6)] {
            put(gap, 2);
            put(x, 8);
            put(0, 16);
        }
        let h = stream_header(&w, 0);
        // inside: (4 + 2) x this side's frames over both sides' (3)
        assert_eq!(h.tangent(&w, true, 1)[0], 2.0);
        assert_eq!(h.tangent(&w, false, 1)[0], 4.0);
        // the ends: 1.5 x the difference - 0.5 x the next key's tangent
        assert_eq!(h.tangent(&w, true, 0)[0], 1.5 * 4.0 - 0.5 * 4.0);
        assert_eq!(h.tangent(&w, false, 2)[0], 1.5 * 2.0 - 0.5 * 2.0);
        let (first, last, k) = h.window(&w, 1);
        assert_eq!((first, last, k[0], k[3], k[6], k[9]), (2, 3, 4.0, 2.0, 2.0, 6.0));
    }

    #[test]
    fn a_stream_steps_its_window_and_blends_on_a_jump() {
        // 4 keys at frames 0, 3, 6 and 9 with x = 0, 3, 6, 9 (8-bit values, no scalar)
        let mut w = vec![0u32; 12];
        let mut at = 0;
        let mut put = |v: u32, n: u32| {
            for i in 0..n {
                if v >> i & 1 != 0 {
                    w[((at + i) / 32) as usize] |= 1 << ((at + i) % 32);
                }
            }
            at += n;
        };
        for (v, n) in [(4, 20), (2, 5), (0, 5), (8, 5)] {
            put(v, n);
        }
        for (r, m) in [(255.0f32, 0.0f32), (0.0, 0.0), (0.0, 0.0)] {
            put(r.to_bits(), 32);
            put(m.to_bits(), 32);
        }
        for (gap, x) in [(0, 0), (2, 3), (2, 6), (2, 9)] {
            put(gap, 2);
            put(x, 8);
            put(0, 16);
        }
        let h = stream_header(&w, 0);
        let mut s = Stream::new(&w, h);
        // a small step: the next window, curved
        assert!((s.read(&w, 1.0)[0] - 1.0).abs() < 1e-5);
        s.read(&w, 2.9);
        s.read(&w, 3.5);
        assert_eq!((s.key, s.first, s.last, s.mode), (1, 3, 6, 0));
        // a jump of a third of the window or more: blended straight
        s.read(&w, 7.0);
        assert_eq!((s.key, s.first, s.last, s.mode), (2, 6, 9, 1));
        assert_eq!(s.read(&w, 7.5)[0], 7.5);
        // back near the start: from the first window again
        s.read(&w, 0.5);
        assert_eq!((s.key, s.first, s.last), (0, 0, 3));
    }

    #[test]
    fn a_rotation_window_blends_its_four_quaternions() {
        let id = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(rotation_window([id; 4], 0.3), id);
        let x = [1.0, 0.0, 0.0, 0.0];
        assert_eq!(rotation_window([id, id, x, x], 0.0), id);
        assert_eq!(rotation_window([id, id, x, x], 1.0), x);
        assert_eq!(rotation_blend([id, x, x, x], 1.0), x);
        let mid = rotation_window([id, id, x, x], 0.5);
        assert!((mid[0] - mid[3]).abs() < 1e-6 && (mid[0] * mid[0] + mid[3] * mid[3] - 1.0).abs() < 1e-6);
    }

    /// A rotation stream's bits: 3 keys at frames 0, 2 and 5 (gaps 0, 1, 2 of 2 bits), biases of
    /// 4 bits and points of 8 bits: identity, a turn by y = 1 and one with y = 63 and x negative.
    fn rotation_bits() -> Vec<u32> {
        let mut w = vec![0u32; 8];
        let mut at = 0;
        let mut put = |v: u32, n: u32| {
            for i in 0..n {
                if v >> i & 1 != 0 {
                    w[((at + i) / 32) as usize] |= 1 << ((at + i) % 32);
                }
            }
            at += n;
        };
        for (v, n) in [(3, 20), (2, 5), (4, 5), (8, 5)] {
            put(v, n);
        }
        // gap, bias, y, z, w, the sign of x
        for (gap, bias, y, z, ww, neg) in [(0, 0, 0, 0, 0, 0), (1, 0, 127, 0, 0, 0), (2, 7, 63, 0, 0, 1)] {
            put(gap, 2);
            put(bias, 4);
            put(y, 8);
            put(z, 8);
            put(ww, 8);
            put(neg, 1);
        }
        w
    }

    #[test]
    fn a_rotation_header_and_its_keys_are_read_from_the_bits() {
        let w = rotation_bits();
        let h = rotation_header(&w, 0);
        assert_eq!((h.keys, h.gap_bits, h.bias_bits, h.point_bits, h.first_record, h.record_bits), (3, 2, 4, 8, 35, 2 + 4 + 24 + 1));
        assert_eq!((h.point_scale, h.bias_scale), (1.0 / 127.0, 1.0 / 7.0));
        assert_eq!((h.frames(&w), h.time(&w, 0), h.time(&w, 1), h.time(&w, 2), h.end()), (1 + 2 + 3, 0, 2, 5, 35 + 3 * 31));
        assert_eq!((h.bias(&w, 1), h.bias(&w, 2)), (0.0, 1.0));
        assert_eq!(h.point(&w, 0), [1.0, 0.0, 0.0, 0.0]);
        // y = 1 leaves nothing for x
        assert_eq!(h.point(&w, 1), [0.0, 1.0, 0.0, 0.0]);
        let p = h.point(&w, 2);
        assert!(p[0] < 0.0 && (p[0] * p[0] + p[1] * p[1] - 1.0).abs() < 1e-5);
        // no bias bits: a bias of 0 whatever follows
        let mut none = rotation_header(&w, 0);
        none.bias_bits = 0;
        none.bias_scale = 0.0;
        assert_eq!(none.bias(&w, 2), 0.0);
    }

    #[test]
    fn a_rotation_windows_control_points_move_by_the_bias_and_the_frames() {
        let w = rotation_bits();
        let h = rotation_header(&w, 0);
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(&b).all(|(p, q)| (p - q).abs() < 1e-5);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // the first key, bias 0: half way to key 1, made unit length
        assert!(near(h.control(&w, 0, true), [s, s, 0.0, 0.0]));
        // the last key, bias 1: not at all towards the key before
        assert!(near(h.control(&w, 2, false), h.point(&w, 2)));
        // between them every control point is a unit quaternion
        for outgoing in [true, false] {
            let c = h.control(&w, 1, outgoing);
            assert!((c.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5);
        }
        let (first, last, win) = h.window(&w, 0);
        assert_eq!((first, last, win[0], win[3]), (0, 2, h.point(&w, 0), h.point(&w, 1)));
        assert_eq!((win[1], win[2]), (h.control(&w, 0, true), h.control(&w, 1, false)));
    }

    #[test]
    fn a_rotation_stream_steps_its_window_and_blends_on_a_jump() {
        let w = rotation_bits();
        let mut s = RotStream::new(&w, rotation_header(&w, 0));
        assert_eq!(s.read(&w, 0.0), [1.0, 0.0, 0.0, 0.0]);
        // a small step inside the window, then one of a third of a window or more into the
        // next (the last one): blended straight
        s.read(&w, 1.0);
        assert_eq!((s.key, s.first, s.last, s.mode), (0, 0, 2, 0));
        let r = s.read(&w, 2.5);
        assert_eq!((s.key, s.first, s.last, s.mode), (1, 2, 5, 1));
        assert!((r.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5);
        // back near the start: from the first window again
        s.read(&w, 0.2);
        assert_eq!((s.key, s.first, s.last), (0, 0, 2));
    }

    #[test]
    fn a_sim_animator_follows_the_game_speed_and_sets_its_actions() {
        let mut a = SimAnimator::default();
        assert!(!a.follow(1.0));
        assert!(a.follow(0.0));
        assert_eq!(a.action_speed(true), -0.0);
        assert!(a.follow(5.0));
        assert_eq!((a.action_speed(false), a.action_speed(true)), (5.0, -5.0));
        assert_eq!(SimAnimator::speed_for(1.0, true, false), 0.0);
        assert_eq!(SimAnimator::speed_for(1.0, true, true), 1.0);
        assert_eq!(SimAnimator::speed_for(5.0, false, false), 5.0);
    }

    #[test]
    fn a_track_moves_by_the_frame_time_and_wraps_by_its_last_frame() {
        // 30 frames a second, 0.033 s a frame: 0.99 a frame, 4.95 in fast-forward
        assert_eq!(track_advance(0.0, 0.033, 1.0, 1.0, 30.0), (0.033f32 as f64 * 30.0) as f32);
        // the double-precision fused step, not two single roundings (a logged fast-forward step)
        assert_eq!(track_advance(23.348824, 0.033, 5.0, 1.0, 30.0), 28.298822);
        // a two-frame loop (last frame 1): 2.92 comes back to 0.92
        assert!((wrap_back(2.92, 1.0) - 0.92).abs() < 1e-5);
        assert_eq!(wrap_back(0.5, 1.0), 0.5);
        // the ends by the end mode: a loop wraps, a stop holds its end; both mark the end passed
        assert_eq!(track_step(9.5, 0.033, 1.0, 1.0, 30.0, 10.0, 1), (10.0, true));
        let (p, ended) = track_step(9.5, 0.033, 1.0, 1.0, 30.0, 10.0, 0);
        assert!(ended && (p - 0.49).abs() < 1e-4);
        assert_eq!(track_step(0.5, 0.033, 1.0, -1.0, 30.0, 10.0, 1), (0.0, true));
        let (p, ended) = track_step(0.5, 0.033, 1.0, -1.0, 30.0, 10.0, 0);
        assert!(ended && (p - 9.51).abs() < 1e-4);
        assert!(!track_step(2.0, 0.033, 1.0, 1.0, 30.0, 10.0, 0).1);
    }

    #[test]
    fn bits_run_on_into_the_next_word() {
        let w = [0xF000_0000u32, 0x0000_0005];
        assert_eq!(bits(&w, 28, 4), 0xF);
        assert_eq!(bits(&w, 30, 4), 0b0111);
        assert_eq!(bits(&w, 32, 3), 5);
        assert_eq!(key3(&[0x0000_0201], 0, 4, [0.5; 3], [1.0; 3]), [1.5, 1.0, 2.0]);
    }

    /// A scalar stream of two keys (80 at frame 0, 44 at frame 1; values 12 bits, range 4095, no
    /// bias) read as the laser light's radius was logged: a straight fall of 36 a frame.
    #[test]
    fn a_scalar_stream_runs_between_its_keys() {
        let mut bits_out: Vec<bool> = Vec::new();
        let mut put = |v: u64, width: u32| bits_out.extend((0..width).map(|i| v >> i & 1 != 0));
        put(2, 20);
        put(1, 5); // the gap's width
        put(0, 5); // no bias
        put(12, 5); // the value's width
        put(u64::from(4095.0f32.to_bits()), 32);
        put(u64::from(0.0f32.to_bits()), 32);
        for value in [80, 44] {
            put(0, 1);
            put(value, 12);
        }
        let mut words = vec![0u32; bits_out.len().div_ceil(32)];
        for (i, b) in bits_out.iter().enumerate() {
            words[i / 32] |= u32::from(*b) << (i % 32);
        }
        let s = ScalarStream::decode(&words, 0);
        assert_eq!(s.read(&words, 0.0), 80.0);
        assert!((s.read(&words, 0.98) - 44.72).abs() < 1e-4);
        assert!((s.read(&words, 0.5) - 62.0).abs() < 1e-4);
    }

    #[test]
    fn a_channel_blends_or_curves_between_its_keys() {
        let keys = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 4.0, 6.0];
        // straight: halfway is the keys' middle
        assert_eq!(channel3(10, 20, 1, &keys, 15.0), [1.0, 2.0, 3.0]);
        // a curve meets its keys at its ends
        assert_eq!(channel3(10, 20, 0, &keys, 10.0), [0.0, 0.0, 0.0]);
        assert_eq!(channel3(10, 20, 0, &keys, 20.0), [2.0, 4.0, 6.0]);
    }

    #[test]
    fn a_backward_play_paused_goes_on_forward() {
        let mut p = Play::new(&anim(31, 30.0), &[], true, 0);
        p.frame(1.0); // the still first frame
        for _ in 0..5 {
            p.frame(1.0);
        }
        p.frame(0.0); // paused: turned
        let mut n = 0;
        while p.poll() == Poll::Pending {
            p.frame(1.0);
            n += 1;
        }
        // back over the 5 frames it had played, then past the end
        assert_eq!(n, 6);
    }

    #[test]
    fn an_animator_waits_for_the_load_then_plays_on_across_calls() {
        let mut a = anim(4, 30.0);
        a.rate = 1.0 / crate::routing::WALK_FRAME;
        let mut m = Animator::new(Loads::default(), Default::default());
        // not in memory at tick 1: pending, there at tick 2
        assert_eq!(m.animate(7, 9, &a, false, 0, 1), Poll::Pending);
        m.frame(|_| 1.0);
        let mut t = 2;
        while m.animate(7, 9, &a, false, 0, t) == Poll::Pending {
            m.frame(|_| 1.0);
            t += 1;
        }
        // frames 0..3 then past 3: done at the fourth ask after the start
        assert_eq!(t, 6);
        // after true, the same call starts a new play
        assert_eq!(m.animate(7, 9, &a, false, 0, t + 1), Poll::Pending);
    }

    #[test]
    fn an_animation_not_in_memory_is_there_the_tick_after_its_ask() {
        let ids: std::collections::HashSet<u32> = [1, 2].into_iter().collect();
        let set = [0u8, 0, 0, 0, 1, 9];
        let mut l = Loads::with_sets(&[&set], &ids);
        assert!(l.ask(1, 5));
        assert!(!l.ask(2, 5));
        assert!(!l.ask(2, 5));
        assert!(l.ask(2, 6));
        assert!(l.ask(2, 7));
    }

    #[test]
    fn a_flag_stream_reads_the_bit_at_its_start_plus_the_place() {
        // bits 3 and 33 set in a 40-bit array; the stream starts at bit 2 (the word -2)
        let w = [1u32 << 3, 1 << 1];
        let f = FlagStream::of((-2i32) as u32).unwrap();
        assert_eq!(f, FlagStream { start: 2 });
        assert!(FlagStream::of(0).is_none() && FlagStream::of(5).is_none());
        assert!(!f.read(&w, 40, 0.0));
        assert!(f.read(&w, 40, 1.0) && f.read(&w, 40, 1.9));
        assert!(f.read(&w, 40, 31.0));
        assert!(!f.read(&w, 40, 32.0));
        // past the end of the bits, and before the start of the array: clear
        assert!(!f.read(&w, 33, 31.0));
        assert!(!f.read(&w, 40, -3.0));
    }
}
