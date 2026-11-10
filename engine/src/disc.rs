//! Files from a GameCube disc image (.iso, read in place; `tools/disc.py` reads the others).

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub struct Disc {
    file: File,
    id: String,
    /// path -> (offset, size), paths as on the disc ("Runtime/GameData/Behavior.iff").
    files: BTreeMap<String, (u32, u32)>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

impl Disc {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Disc> {
        let file = File::open(path)?;
        let mut h = [0u8; 0x440];
        file.read_exact_at(&mut h, 0)?;
        let id = String::from_utf8_lossy(&h[..6]).into_owned();
        let (fst_off, fst_size) = (be32(&h[0x424..]), be32(&h[0x428..]) as usize);
        if !(12..=64 << 20).contains(&fst_size) {
            return Err(bad("the file system table's size"));
        }
        let mut fst = vec![0u8; fst_size];
        file.read_exact_at(&mut fst, fst_off as u64)?;
        let n = be32(&fst[8..]) as usize;
        if n * 12 > fst_size {
            return Err(bad("the file system table's count"));
        }
        let names = &fst[n * 12..];
        let mut dirs: Vec<(usize, String)> = vec![(n, String::new())]; // (end, path)
        let mut files = BTreeMap::new();
        for i in 1..n {
            while dirs.last().is_some_and(|d| i >= d.0) {
                dirs.pop();
            }
            let e = &fst[i * 12..i * 12 + 12];
            let name_off = (e[1] as usize) << 16 | (e[2] as usize) << 8 | e[3] as usize;
            if name_off >= names.len() {
                break;
            }
            let name = &names[name_off..];
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            let p = format!("{}{}", dirs.last().map_or("", |d| d.1.as_str()), String::from_utf8_lossy(name));
            if e[0] != 0 {
                dirs.push((be32(&e[8..]) as usize, p + "/"));
            } else {
                files.insert(p, (be32(&e[4..]), be32(&e[8..])));
            }
        }
        Ok(Disc { file, id, files })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn files(&self) -> &BTreeMap<String, (u32, u32)> {
        &self.files
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let &(off, size) = self.files.get(path)?;
        let mut b = vec![0u8; size as usize];
        self.file.read_exact_at(&mut b, off as u64).ok()?;
        Some(b)
    }
}
