//! A byte image read through pointers, remembering which bytes were read: what is left over
//! at the end is what the reader does not know (`docs/assets.md`, "Menus and HUD (Apt)").

use std::ops::Range;

/// A reading failure: where it was and what was wanted.
pub type Res<T> = Result<T, String>;

/// A file's bytes and a mark on each one read.
pub struct Img<'a> {
    b: &'a [u8],
    seen: Vec<bool>,
}

impl<'a> Img<'a> {
    pub fn new(b: &'a [u8]) -> Img<'a> {
        Img { b, seen: vec![false; b.len()] }
    }

    pub fn len(&self) -> usize {
        self.b.len()
    }

    pub fn is_empty(&self) -> bool {
        self.b.is_empty()
    }

    /// Marks `n` bytes at `p` as read, and gives them.
    pub fn take(&mut self, p: usize, n: usize) -> Res<&'a [u8]> {
        let end = p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| format!("{n} bytes at {p:#x} run past the end ({:#x})", self.b.len()))?;
        self.seen[p..end].fill(true);
        Ok(&self.b[p..end])
    }

    /// A byte, not marked as read.
    pub fn peek(&self, p: usize) -> Option<u8> {
        self.b.get(p).copied()
    }

    pub fn u8(&mut self, p: usize) -> Res<u8> {
        Ok(self.take(p, 1)?[0])
    }

    /// A big-endian word.
    pub fn u32(&mut self, p: usize) -> Res<u32> {
        Ok(u32::from_be_bytes(self.take(p, 4)?.try_into().unwrap()))
    }

    pub fn u16(&mut self, p: usize) -> Res<u16> {
        Ok(u16::from_be_bytes(self.take(p, 2)?.try_into().unwrap()))
    }

    pub fn i32(&mut self, p: usize) -> Res<i32> {
        Ok(self.u32(p)? as i32)
    }

    pub fn f32(&mut self, p: usize) -> Res<f32> {
        Ok(f32::from_bits(self.u32(p)?))
    }

    /// `N` floats from `p`.
    pub fn floats<const N: usize>(&mut self, p: usize) -> Res<[f32; N]> {
        let mut out = [0.0; N];
        for (i, f) in out.iter_mut().enumerate() {
            *f = self.f32(p + 4 * i)?;
        }
        Ok(out)
    }

    /// A pointer word as an offset.
    pub fn ptr(&mut self, p: usize) -> Res<usize> {
        Ok(self.u32(p)? as usize)
    }

    /// A NUL-ended string at `p`, with the zero bytes that pad it to a word.
    pub fn cstr(&mut self, p: usize) -> Res<String> {
        let rest = self.b.get(p..).ok_or_else(|| format!("string at {p:#x} is past the end"))?;
        let len = rest.iter().position(|&c| c == 0).ok_or_else(|| format!("string at {p:#x} has no end"))?;
        self.take(p, len + 1)?;
        let mut end = p + len + 1;
        while !end.is_multiple_of(4) && self.b.get(end) == Some(&0) {
            self.seen[end] = true;
            end += 1;
        }
        Ok(rest[..len].iter().map(|&c| c as char).collect())
    }

    /// A NUL-ended string at `p`, no padding after it.
    pub fn cstr_unpadded(&mut self, p: usize) -> Res<String> {
        let rest = self.b.get(p..).ok_or_else(|| format!("string at {p:#x} is past the end"))?;
        let len = rest.iter().position(|&c| c == 0).ok_or_else(|| format!("string at {p:#x} has no end"))?;
        self.take(p, len + 1)?;
        Ok(rest[..len].iter().map(|&c| c as char).collect())
    }

    /// A string pointer: `None` for 0.
    pub fn opt_str(&mut self, p: usize) -> Res<Option<String>> {
        let at = self.ptr(p)?;
        if at == 0 { Ok(None) } else { self.cstr(at).map(Some) }
    }

    /// The runs of bytes never read.
    pub fn gaps(&self) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.seen.len() {
            if self.seen[i] {
                i += 1;
                continue;
            }
            let s = i;
            while i < self.seen.len() && !self.seen[i] {
                i += 1;
            }
            out.push(s..i);
        }
        out
    }

    /// The unread runs as messages for a report, naming the file.
    pub fn unread(&self, what: &str) -> Vec<String> {
        self.gaps().into_iter().map(|g| format!("{what}: bytes {:#x}..{:#x} never read", g.start, g.end)).collect()
    }
}
