//! A fast hash for the small integer keys of the per-frame lookups (shader and texture ids, mesh
//! places): the multiply-and-rotate mix of Firefox's hash, not a hash to guard against keys chosen
//! by an attacker. Deterministic, and the maps' order is never read.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The mixing constant: an odd 64-bit number with its bits spread (the one of the Fx hash).
const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl FastHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for c in chunks {
            self.add(u64::from_le_bytes(*c));
        }
        for &b in rest {
            self.add(u64::from(b));
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.add(u64::from(n));
    }
    fn write_u16(&mut self, n: u16) {
        self.add(u64::from(n));
    }
    fn write_u32(&mut self, n: u32) {
        self.add(u64::from(n));
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A `HashMap` with the fast hash.
pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
