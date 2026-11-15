//! EA/EOR `.arc` archives (`docs/survey.md`, "Archives"): a list of named entries, each an
//! offset and a size into the archive. Two layouts, the entries alike (big-endian: the offset,
//! the size, a NUL-ended name, then 12 bytes: the name's hash, a time stamp and a word):
//! - the index first: the word 0x04000000, the count, a word, the entries (`audiostr.arc`);
//! - the index last: a little-endian word at 0 giving where it is; there the count, a word,
//!   the entries (every other archive).

/// An entry: its name and where its bytes are in the archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    /// The resource id the game asks for the entry by (the word before its offset; mostly the
    /// upper-case name's CRC-32, `docs/assets.md`).
    pub id: u32,
    /// The two words after the name.
    pub words: [u32; 2],
}

/// The big-endian word at byte `p`, if `b` has it (the disc's formats are big-endian).
pub(crate) fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

/// The big-endian half word at byte `p`, if `b` has it.
pub(crate) fn be16(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(p..p + 2)?.try_into().ok()?))
}

fn entries_at(data: &[u8], mut p: usize, n: usize) -> Option<Vec<Entry>> {
    // each entry is at least 8 + 1 + 12 bytes
    if n > data.len() / 21 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (offset, size) = (be32(data, p)?, be32(data, p + 4)?);
        let rest = data.get(p + 8..)?;
        let len = rest.iter().position(|&c| c == 0)?;
        let name = rest[..len].iter().map(|&c| c as char).collect();
        let q = p + 8 + len + 1;
        let (id, words) = (be32(data, p - 4)?, [be32(data, q).unwrap_or(0), be32(data, q + 4).unwrap_or(0)]);
        out.push(Entry { name, offset, size, id, words });
        p += 8 + len + 1 + 12;
    }
    Some(out)
}

/// The archive's entries, or `None` if `data` is not one (or its index runs past its end, or
/// an entry's bytes do).
pub fn entries(data: &[u8]) -> Option<Vec<Entry>> {
    let fits = |e: &[Entry], end: usize| e.iter().all(|x| x.offset as usize + x.size as usize <= end);
    if data.len() >= 12 && be32(data, 0)? == 0x0400_0000 {
        let e = entries_at(data, 12, be32(data, 4)? as usize)?;
        return fits(&e, data.len()).then_some(e);
    }
    let at = u32::from_le_bytes(data.get(..4)?.try_into().ok()?) as usize;
    if at < 4 || at + 8 > data.len() {
        return None;
    }
    let e = entries_at(data, at + 8, be32(data, at)? as usize)?;
    fits(&e, at).then_some(e)
}

/// An entry's bytes.
pub fn bytes<'a>(data: &'a [u8], e: &Entry) -> &'a [u8] {
    &data[e.offset as usize..(e.offset + e.size) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, offset: u32, size: u32) -> Vec<u8> {
        let mut v = offset.to_be_bytes().to_vec();
        v.extend(size.to_be_bytes());
        v.extend(name.as_bytes());
        v.push(0);
        v.extend([0u8; 12]);
        v
    }

    #[test]
    fn index_last() {
        let mut a = vec![0u8; 4];
        a.extend(b"abcdef");
        let at = a.len() as u32;
        a[..4].copy_from_slice(&at.to_le_bytes());
        a.extend(2u32.to_be_bytes());
        a.extend(0u32.to_be_bytes());
        a.extend(entry("one", 4, 2));
        a.extend(entry("two", 6, 4));
        let e = entries(&a).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(bytes(&a, &e[1]), b"cdef");
        assert_eq!(e[0].name, "one");
    }

    #[test]
    fn index_first_and_not_an_archive() {
        let mut a = 0x0400_0000u32.to_be_bytes().to_vec();
        a.extend(1u32.to_be_bytes());
        a.extend(0u32.to_be_bytes());
        a.extend(entry("s", 40, 2));
        a.resize(42, 7);
        assert_eq!(bytes(&a, &entries(&a).unwrap()[0]), &[7, 7]);
        assert!(entries(b"not an archive at all").is_none());
    }
}
