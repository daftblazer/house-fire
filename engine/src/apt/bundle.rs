//! A `flashes.arc` entry: a `BIGF` bundle of one movie's files (`docs/assets.md`, "Menus and
//! HUD (Apt)").

use super::img::{Img, Res};

/// A file in a bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

/// A bundle's file list, and the bytes of it that no file and no header word accounts for.
#[derive(Clone, Debug)]
pub struct Bundle {
    pub files: Vec<File>,
    /// Messages for the byte runs that nothing reads.
    pub unread: Vec<String>,
}

impl Bundle {
    /// Reads a bundle's header: `BIGF`, the whole size (little-endian), the file count, the
    /// header's size (both big-endian), then each file's offset, size and NUL-ended name.
    pub fn read(data: &[u8]) -> Res<Bundle> {
        let mut m = Img::new(data);
        if m.take(0, 4)? != b"BIGF" {
            return Err("not a BIGF bundle".into());
        }
        let total = u32::from_le_bytes(m.take(4, 4)?.try_into().unwrap()) as usize;
        if total != data.len() {
            return Err(format!("the bundle says {total} bytes, it has {}", data.len()));
        }
        let (count, header) = (m.u32(8)? as usize, m.u32(12)? as usize);
        let mut p = 16;
        let mut files = Vec::new();
        for _ in 0..count {
            let (offset, mut size) = (m.ptr(p)?, m.ptr(p + 4)?);
            // the placeholder `*.wav`: at the bundle's end, its size -1 (no file)
            if size == u32::MAX as usize && offset == data.len() {
                size = 0;
            }
            let name = m.cstr_unpadded(p + 8)?;
            p += 8 + name.len() + 1;
            if offset.checked_add(size).is_none_or(|e| e > data.len()) {
                return Err(format!("file {name} runs past the bundle"));
            }
            files.push(File { name, offset, size });
        }
        if p != header {
            return Err(format!("the header ends at {p:#x}, it says {header:#x}"));
        }
        // what is neither a header word the above read nor a file is unread
        let mut seen = vec![true; data.len()];
        for g in m.gaps().into_iter().filter(|g| g.end <= header) {
            seen[g.clone()].fill(false);
        }
        seen[header..].fill(false);
        // zero bytes pad the header to a word, as they pad each file
        let mut q = header;
        while !q.is_multiple_of(4) && data.get(q) == Some(&0) {
            seen[q] = true;
            q += 1;
        }
        for f in &files {
            seen[f.offset..f.offset + f.size].fill(true);
            // zero bytes pad a file to a word
            let mut q = f.offset + f.size;
            while !q.is_multiple_of(4) && data.get(q) == Some(&0) {
                seen[q] = true;
                q += 1;
            }
        }
        let mut unread = Vec::new();
        let mut i = 0;
        while i < seen.len() {
            if seen[i] {
                i += 1;
                continue;
            }
            let s = i;
            while i < seen.len() && !seen[i] {
                i += 1;
            }
            unread.push(format!("bundle: bytes {s:#x}..{i:#x} are in no file and no header word"));
        }
        Ok(Bundle { files, unread })
    }

    /// A file's bytes.
    pub fn bytes<'a>(&self, data: &'a [u8], f: &File) -> &'a [u8] {
        &data[f.offset..f.offset + f.size]
    }
}
