//! Animations as Bustin' Out keeps them (`animatio.arc`'s entries, `docs/assets.md`,
//! "Animations"): the engine's ERAnim in its old layout (The Sims 2's `ERAnim::OldLoad` reads
//! the same fields behind a chunk header), here behind the animation's name. Big-endian and
//! packed: nothing is aligned after the name.

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
}

impl Anim {
    /// How long it runs, in seconds (frames over the rate).
    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.rate
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
    (r.at == e.len()).then_some(Anim { name, frames, first, second, nodes, word, constants, bits, bit_words, rate, floats, byte, words, flags, events })
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
        Play { last, place: 0.0, still_first: backward, notes, next: 0, wanted, counted: 0, backward, turned: false, done: false }
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
            return Poll::Done;
        }
        if self.turned {
            // turned forward: it ends past the animation's last frame (place below 0)
            if self.place >= -EPS {
                return Poll::Pending;
            }
            self.done = true;
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
        Poll::Done
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

/// A scale or translation stream's header, as Bustin' Out's 801BC318 reads it from the
/// animation's bits: the key count (20 bits), the widths of the frame gap, the key's scalar and
/// each value (5 bits each; a value width of 0 is 32), then per value its range and minimum as
/// 32-bit floats. A value is its field times range / (2^width - 1), plus the minimum; the
/// scalar is its signed field over 2^(width - 1) - 1 (1 with no scalar).
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
    /// The frames the keys span: the sum of their gaps + 1.
    pub frames: u32,
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
    let scalar_scale = if scalar_bits == 0 { 1.0 } else { 1.0 / ((1i64 << (scalar_bits - 1)) - 1) as i32 as f32 };
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
    let frames = (0..keys).map(|k| bits(words, p + k * record_bits, gap_bits) + 1).sum();
    StreamHeader { keys, gap_bits, scalar_bits, value_bits, scalar_scale, scale, offset, first_record: p, record_bits, frames }
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

/// The Sims' plays, as the game keeps them: one a Sim, asked by its Animate steps and stepped by
/// the frames, in the game's order. A Sim's play goes on across Animate calls of the same
/// animation until it has given true; another animation, or a call after true, starts anew (once
/// the animation is in memory: `Loads`).
#[derive(Clone, Debug, Default)]
pub struct Animator {
    pub loads: Loads,
    /// The notes of every animation, by id (`notes`).
    pub notes: std::collections::BTreeMap<u32, Vec<Note>>,
    plays: std::collections::BTreeMap<u32, (u32, Play, f32)>,
}

impl Animator {
    pub fn new(loads: Loads, notes: std::collections::BTreeMap<u32, Vec<Note>>) -> Animator {
        Animator { loads, notes, plays: Default::default() }
    }

    /// An Animate step of `sim` at `tick` for the animation `id` (`anim` its data), played
    /// `backward` and asking `wanted` counted events. Pending while it waits to be in memory.
    pub fn animate(&mut self, sim: u32, id: u32, anim: &Anim, backward: bool, wanted: i32, tick: u32) -> Poll {
        if self.plays.get(&sim).is_some_and(|p| p.0 != id || p.1.finished()) {
            self.plays.remove(&sim);
        }
        if !self.plays.contains_key(&sim) {
            if !self.loads.ask(id, tick) {
                return Poll::Pending;
            }
            let play = Play::new(anim, self.notes.get(&id).map_or(&[][..], |v| v.as_slice()), backward, wanted);
            self.plays.insert(sim, (id, play, anim.rate));
        }
        self.plays.get_mut(&sim).map_or(Poll::Pending, |p| p.1.poll())
    }

    /// `sim`'s play: its animation's id and the play.
    pub fn play(&self, sim: u32) -> Option<(u32, &Play)> {
        self.plays.get(&sim).map(|p| (p.0, &p.1))
    }

    /// A frame: every play steps by 0.033 s × its rate × its Sim's `speed` this frame.
    pub fn frame(&mut self, mut speed: impl FnMut(u32) -> f32) {
        for (sim, p) in self.plays.iter_mut() {
            p.1.frame(crate::routing::WALK_FRAME * p.2 * speed(*sim));
        }
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
        Anim { name: String::new(), frames, first: [0.0; 3], second: [0.0; 3], nodes: vec![], word: 0, constants: vec![], bits: 0, bit_words: vec![], rate, floats: [0.0; 2], byte: 0, words: [0; 4], flags: [0; 2], events: vec![] }
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
        assert_eq!((h.scale, h.offset, h.frames), ([1.0, 0.0, 0.0], [1.0, 2.0, 3.0], 2 + 5));
    }

    #[test]
    fn bits_run_on_into_the_next_word() {
        let w = [0xF000_0000u32, 0x0000_0005];
        assert_eq!(bits(&w, 28, 4), 0xF);
        assert_eq!(bits(&w, 30, 4), 0b0111);
        assert_eq!(bits(&w, 32, 3), 5);
        assert_eq!(key3(&[0x0000_0201], 0, 4, [0.5; 3], [1.0; 3]), [1.5, 1.0, 2.0]);
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
}
