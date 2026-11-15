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
}
