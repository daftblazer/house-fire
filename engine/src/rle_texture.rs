//! The Sims' run-length textures (`rletextu.arc`: hair, faces, clothing), as the original's
//! `ERRleTexture` reads them (`docs/assets.md`, "Run-length textures"): a depth byte, a palette,
//! the coded bytes' length and the coded bytes. The coding is a series of runs, each a count
//! (read at the reader's nibble position) then its indices: a count with its top bit set is
//! that many (the count negated, as a byte) indices, each its own; otherwise one index for
//! `count` texels. With 16 colours an index is a nibble, with 256 a byte. No size is kept: the
//! texels are taken one at a time by what draws the Sim's textures.

/// A run-length texture: its palette (RGBA as the original swaps it: the stored word's bytes
/// reversed) and its coded bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RleTexture {
    pub palette: Vec<u32>,
    pub coded: Vec<u8>,
    /// Indices are nibbles (16 colours) rather than bytes.
    pub nibbles: bool,
}

/// Reads an entry: `None` if it is shorter than its parts.
pub fn read(e: &[u8]) -> Option<RleTexture> {
    let (&depth, rest) = e.split_first()?;
    let nibbles = depth == 0x10;
    let colours = if nibbles { 16 } else { 256 };
    let pal = rest.get(..colours * 4)?;
    let palette = pal.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let at = colours * 4;
    let len = u32::from_be_bytes(rest.get(at..at + 4)?.try_into().ok()?) as usize;
    let coded = rest.get(at + 4..at + 4 + len)?.to_vec();
    Some(RleTexture { palette, coded, nibbles })
}

/// A reader over the coded bytes, at a nibble position.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    high: bool,
}

impl Reader<'_> {
    /// A nibble (the original's fn_8003E5DC).
    fn nibble(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        if self.high {
            self.high = false;
            Some(v >> 4)
        } else {
            self.high = true;
            self.at += 1;
            Some(v & 0xF)
        }
    }
    /// A byte, from the nibble position (fn_8003E628).
    fn byte(&mut self) -> Option<u8> {
        if self.high {
            let v = *self.b.get(self.at)?;
            self.at += 1;
            Some(v)
        } else {
            let hi = *self.b.get(self.at)?;
            let lo = *self.b.get(self.at + 1)?;
            self.at += 1;
            Some((hi << 4) | (lo >> 4))
        }
    }
    fn index(&mut self, nibbles: bool) -> Option<u8> {
        if nibbles { self.nibble() } else { self.byte() }
    }
    fn done(&self) -> bool {
        self.at >= self.b.len()
    }
}

impl RleTexture {
    /// Every texel's palette index, run by run until the coded bytes end (a run cut short by the
    /// end ends there).
    pub fn indices(&self) -> Vec<u8> {
        let mut r = Reader { b: &self.coded, at: 0, high: true };
        let mut out = Vec::new();
        // after a literal run of 16 colours the next run is a repeat, its count's top bit not
        // looked at (fn_8003E41C); a repeat of count 0 still gives one texel (the count is
        // compared after each texel)
        let mut after_literal = false;
        while !r.done() {
            let Some(count) = r.byte() else { break };
            if count >> 7 != 0 && !(self.nibbles && after_literal) {
                for _ in 0..count.wrapping_neg() {
                    match r.index(self.nibbles) {
                        Some(i) => out.push(i),
                        None => return out,
                    }
                }
                after_literal = true;
            } else {
                let Some(i) = r.index(self.nibbles) else { break };
                out.extend(std::iter::repeat_n(i, (count as usize).max(1)));
                after_literal = false;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_of_bytes_and_of_nibbles() {
        // 256 colours: three of index 7, then two literals 1 and 2 (count 0xFE = -2)
        let t = RleTexture { palette: vec![0; 256], coded: vec![3, 7, 0xFE, 1, 2], nibbles: false };
        assert_eq!(t.indices(), [7, 7, 7, 1, 2]);
        // 16 colours: count 2 (a byte), index 5 (a nibble), then count 0xFF (one literal) from
        // the next nibble: 0x02 0x5F 0xF3 -> [5, 5, 3]
        let t = RleTexture { palette: vec![0; 16], coded: vec![0x02, 0x5F, 0xF3], nibbles: true };
        assert_eq!(t.indices(), [5, 5, 3]);
    }
}
