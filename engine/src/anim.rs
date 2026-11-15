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
        Play { last, place: 0.0, still_first: backward, notes, next: 0, wanted, counted: 0, backward, done: false }
    }

    /// A frame: the place moves by `step` (the frame step × the rate × the speed).
    pub fn frame(&mut self, step: f32) {
        if self.still_first {
            self.still_first = false;
        } else {
            self.place += step;
        }
    }

    /// Whether the play has been asked to its end (true given).
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
        while let Some(&(at, value)) = self.notes.get(self.next) {
            if self.place <= at + EPS {
                break;
            }
            self.next += 1;
            if value > 99 {
                return Poll::Event(value);
            }
            let e = self.expected();
            if (0..self.wanted).contains(&e) && value == e {
                self.counted += 1;
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
