//! The save writers (`save_write`) against the saves the engine reads: memory cards
//! (HOUSEFIRE_CARD, and HOUSEFIRE_CARDS, a colon-separated list; the user's own, kept out of the
//! repository) and the disc's default.ngh (HOUSEFIRE_BUSTIN, or HOUSEFIRE_BUSTIN_ISO as
//! `lots.rs`). Skipped when none is set. Every figure is a count of what writes back byte for
//! byte what the readers took.

use housefire::disc::Disc;
use housefire::library::Library;
use housefire::save::{NeighbourhoodBlock, SaveStream, memory_card_file, read_lots, read_neighbourhood, unpack_lzss};
use housefire::save_write::*;
use housefire::world::World;

const GAME: &str = "G4ME69";

fn disc() -> Option<Disc> {
    let iso = std::env::var_os("HOUSEFIRE_BUSTIN").or_else(|| std::env::var_os("HOUSEFIRE_BUSTIN_ISO")).filter(|s| !s.is_empty())?;
    Some(Disc::open(&iso).expect("the disc"))
}

/// The cards' paths, from the two variables.
fn cards() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for var in ["HOUSEFIRE_CARD", "HOUSEFIRE_CARDS"] {
        if let Ok(s) = std::env::var(var) {
            for p in s.split(':').filter(|p| !p.is_empty()) {
                if !v.iter().any(|q| q == p) {
                    v.push(p.to_string());
                }
            }
        }
    }
    v
}

/// The neighbourhood files to test: each card's save, and the disc's default.ngh; with the
/// disc a library for the objects' classes.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for p in cards() {
        let card = std::fs::read(&p).expect("the card");
        out.push((p, memory_card_file(&card, GAME).unwrap()));
    }
    if let Some(d) = disc() {
        out.push(("the disc".to_string(), d.read("Runtime/default.ngh").expect("default.ngh")));
    }
    out
}

/// Every section of every file: its unpacked bytes read into the models and written back are
/// the same, and so is each lot's ObjM on its own.
#[test]
fn sections_round_trip() {
    let Some(d) = disc() else {
        println!("save_write: skipped (HOUSEFIRE_BUSTIN not set)");
        return;
    };
    let lib = Library::bustin_out(&d);
    for (name, f) in files() {
        let blocks = file_blocks(&f).unwrap();
        let same = blocks.iter().filter(|b| rewrite_block(b, |lot| World::new(&lib).read_objects(lot)).is_ok_and(|e| e == b.data)).count();
        let lots = read_lots(&f).unwrap();
        let objm = lots
            .iter()
            .filter(|lot| {
                let lo = World::new(&lib).read_objects(lot).unwrap();
                encode_lot_objects(&lo) == lot.find("ObjM").unwrap()
            })
            .count();
        println!("{name}: sections {same}/{}, lots' ObjM {objm}/{}", blocks.len(), lots.len());
        assert_eq!(same, blocks.len(), "{name}: a section does not write back");
        assert_eq!(objm, lots.len(), "{name}: a lot's ObjM does not write back");
    }
}

/// The whole file: its blocks rewritten from the models, packed, checksummed and put at their
/// places come to the file's bytes; every block of the result unpacks to the original's and
/// verifies; and the file goes back into its card unchanged.
#[test]
fn files_round_trip() {
    let Some(d) = disc() else {
        println!("save_write: skipped (HOUSEFIRE_BUSTIN not set)");
        return;
    };
    let lib = Library::bustin_out(&d);
    for (name, f) in files() {
        let blocks = file_blocks(&f).unwrap();
        let packed_same = blocks.iter().filter(|b| encode_block(b) == f[b.offset as usize..b.offset as usize + encode_block(b).len()]).count();
        let rewritten: Vec<NeighbourhoodBlock> =
            blocks.iter().map(|b| NeighbourhoodBlock { data: rewrite_block(b, |lot| World::new(&lib).read_objects(lot)).unwrap(), ..b.clone() }).collect();
        let g = assemble_file(&f, &rewritten).unwrap();
        let unpacked = read_neighbourhood(&g).unwrap();
        let original = read_neighbourhood(&f).unwrap();
        assert_eq!(unpacked.len(), original.len());
        for (a, b) in unpacked.iter().zip(&original) {
            assert_eq!((a.offset, a.section, a.count, &a.data), (b.offset, b.section, b.count, &b.data));
        }
        let crcs = blocks.iter().filter(|b| verify_block(&f, b.offset as usize) && verify_block(&g, b.offset as usize)).count();
        println!("{name}: LZSS blocks repacked identically {packed_same}/{}, CRCs verified {crcs}/{}, file identical {}", blocks.len(), blocks.len(), g == f);
        assert_eq!(packed_same, blocks.len());
        assert_eq!(crcs, blocks.len());
        assert!(g == f, "{name}: the file differs");
    }
    // Back into the cards.
    for p in cards() {
        let card = std::fs::read(&p).unwrap();
        let f = memory_card_file(&card, GAME).unwrap();
        assert!(replace_card_file(&card, GAME, &f).unwrap() == card, "{p}: the card changed");
        assert!(replace_card_file(&card, GAME, &f[1..]).is_err());
    }
}

/// A file made from nothing (zeros between the blocks) reads back: the blocks as they were.
#[test]
fn a_file_from_nothing() {
    let Some(d) = disc() else { return };
    let ngh = d.read("Runtime/default.ngh").expect("default.ngh");
    let blocks = file_blocks(&ngh).unwrap();
    let g = assemble_file(&vec![0; ngh.len()], &blocks).unwrap();
    let back = read_neighbourhood(&g).unwrap();
    assert_eq!(back.len(), blocks.len());
    for (a, b) in back.iter().zip(&blocks) {
        assert_eq!((a.offset, a.section, &a.data), (b.offset, b.section, &b.data));
    }
}

/// The packer's output unpacks to its input, whatever the input.
#[test]
fn lzss_round_trips() {
    let mut seed = 12345u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) as u8
    };
    let mut cases: Vec<Vec<u8>> = vec![vec![], vec![7], vec![0; 5000], [1, 2, 3].repeat(2000), (0..=255).collect()];
    cases.push((0..30000).map(|_| next()).collect());
    cases.push((0..30000).map(|_| next() & 3).collect());
    for c in cases {
        let p = pack_lzss(&c);
        let (u, used) = unpack_lzss(&p, c.len()).unwrap();
        assert!(u == c && used == p.len(), "a case of {} bytes does not round-trip", c.len());
    }
}

/// The stream writer's values read back as written, at each width and the edges between them.
#[test]
fn save_stream_round_trips() {
    let v8 = [0, 1, -1, 1, 7, -8, 8, 31, -32, 32, 127, -128];
    let v16 = [0, 1, -1, 15, -16, 16, 127, -128, 128, 4095, -4096, 4096, 32767, -32768];
    let v32 = [0, 1, -1, 31, -32, 32, 1023, -1024, 1024, 1048575, -1048576, 1048576, i32::MAX, i32::MIN];
    let mut w = SaveWriter::new();
    w.compressed = true;
    for &v in &v8 {
        w.w8(v);
    }
    for &v in &v16 {
        w.w16(v);
    }
    for &v in &v32 {
        w.w32(v);
    }
    w.wfloat(-1.5);
    w.pad();
    w.mark(0xDEAD);
    w.w16(9);
    let bytes = w.into_bytes();
    let mut r = SaveStream::new(&bytes, 0);
    r.compressed = true;
    for &v in &v8 {
        assert_eq!(r.r8().unwrap(), v);
    }
    for &v in &v16 {
        assert_eq!(r.r16().unwrap(), v);
    }
    for &v in &v32 {
        assert_eq!(r.r32().unwrap(), v);
    }
    assert_eq!(r.rfloat().unwrap(), -1.5);
    r.pad();
    assert_eq!(r.mark().unwrap(), 0xDEAD);
    assert_eq!(r.r16().unwrap(), 9);
}
