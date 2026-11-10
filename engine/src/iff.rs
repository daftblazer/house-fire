//! Maxis IFF 2.5, as the GameCube Sims games keep it (`docs/survey.md`, `tools/iff.py`).

use std::fmt;

/// A chunk's contents' byte order: Bustin' Out keeps the PC's, v3.0 big-endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Little,
    Big,
}

const MAGIC: &[u8] = b"IFF FILE 2.5:TYPE FOLLOWED BY SIZE\0 JAMIE DOORNBOS & MAXIS 1";
const HEADER: usize = 64; // the magic and the resource map's offset
const CHUNK_HEADER: usize = 76;

/// A chunk of an [`IffFile`]: its header, and where its contents are in the file's bytes.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub kind: [u8; 4],
    pub id: i16,
    pub flags: u16,
    pub label: String,
    start: usize,
    len: usize,
}

/// An IFF file and its chunks, the file's bytes owned.
#[derive(Default)]
pub struct IffFile {
    pub bytes: Vec<u8>,
    pub chunks: Vec<Chunk>,
}

impl IffFile {
    /// The chunks of the IFF file at the start of `bytes`, or `None` if it is not one. Stops at
    /// the first chunk header that is not one (the next file in an archive, or padding).
    pub fn read(bytes: Vec<u8>) -> Option<IffFile> {
        if bytes.len() < HEADER || !bytes.starts_with(MAGIC) {
            return None;
        }
        Some(Self::parse(bytes))
    }

    /// The same, a file that is not an IFF kept with no chunks.
    pub fn parse(bytes: Vec<u8>) -> IffFile {
        if bytes.len() < HEADER || !bytes.starts_with(MAGIC) {
            return IffFile { bytes, chunks: Vec::new() };
        }
        let type_char = |c: u8| c.is_ascii_alphanumeric() || c == b'#' || c == b'$' || c == b' ';
        let mut chunks = Vec::new();
        let mut p = HEADER;
        while p + CHUNK_HEADER <= bytes.len() {
            let h = &bytes[p..p + CHUNK_HEADER];
            if !h[..4].iter().all(|&c| type_char(c)) {
                break;
            }
            let size = u32::from_be_bytes([h[4], h[5], h[6], h[7]]) as usize;
            if size < CHUNK_HEADER || p + size > bytes.len() {
                break;
            }
            let label = &h[12..76];
            let label = &label[..label.iter().position(|&c| c == 0).unwrap_or(64)];
            chunks.push(Chunk {
                kind: [h[0], h[1], h[2], h[3]],
                id: i16::from_be_bytes([h[8], h[9]]),
                flags: u16::from_be_bytes([h[10], h[11]]),
                label: latin1(label),
                start: p + CHUNK_HEADER,
                len: size - CHUNK_HEADER,
            });
            p += size;
        }
        IffFile { bytes, chunks }
    }

    /// A chunk's contents, its header excluded.
    pub fn data(&self, c: &Chunk) -> &[u8] {
        &self.bytes[c.start..c.start + c.len]
    }

    /// The chunks of a type, in the file's order.
    pub fn of<'a>(&'a self, kind: &'a [u8; 4]) -> impl Iterator<Item = &'a Chunk> + 'a {
        self.chunks.iter().filter(move |c| &c.kind == kind)
    }

    pub fn find(&self, kind: &[u8; 4], id: i16) -> Option<&Chunk> {
        self.chunks.iter().find(|c| c.id == id && &c.kind == kind)
    }
}

/// Bytes as text, each byte its own character (Latin-1), so that names compare as the bytes do.
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// A read past the end of a chunk, or contents of a form not known.
#[derive(Debug, Clone)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

/// Little or big-endian reads within a slice.
pub struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    order: Order,
}

impl<'a> Reader<'a> {
    pub fn new(d: &'a [u8], order: Order) -> Self {
        Reader { d, p: 0, order }
    }
    pub fn left(&self) -> usize {
        self.d.len() - self.p
    }
    pub fn pos(&self) -> usize {
        self.p
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if n > self.left() {
            return Err(FormatError(format!("needs {n} bytes at {}", self.p)));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, FormatError> {
        let b: [u8; 2] = self.take(2)?.try_into().unwrap();
        Ok(match self.order {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        })
    }
    pub fn s16(&mut self) -> Result<i16, FormatError> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32, FormatError> {
        let b: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(match self.order {
            Order::Little => u32::from_le_bytes(b),
            Order::Big => u32::from_be_bytes(b),
        })
    }
    pub fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32()?))
    }
    /// What is left may only be padding (0x00, 0xA3, 0xCD).
    pub fn done(&self) -> Result<(), FormatError> {
        if self.d[self.p..].iter().all(|&c| matches!(c, 0 | 0xA3 | 0xCD)) {
            Ok(())
        } else {
            Err(FormatError(format!("{} bytes left over", self.left())))
        }
    }
}
