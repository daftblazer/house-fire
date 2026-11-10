//! The readers on the player's own Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO; skipped if it is not
//! set). Nothing from the disc is kept. Every BHAV on the disc must read: 9,529, as
//! `tools/check_chunks.py` counts (the C++ engine's `test_disc` too).

use std::collections::BTreeMap;

use housefire::bhav::{bhav_order, read_bhav};
use housefire::disc::Disc;
use housefire::iff::IffFile;

#[test]
fn every_bhav_reads() {
    let Some(iso) = std::env::var_os("HOUSEFIRE_BUSTIN_ISO").filter(|s| !s.is_empty()) else {
        println!("disc: skipped (HOUSEFIRE_BUSTIN_ISO not set)");
        return;
    };
    let d = Disc::open(&iso).expect("the disc");
    assert_eq!(d.id(), "G4ME69", "not Bustin' Out");
    let (mut read, mut failed) = (0, 0);
    for path in d.files().keys() {
        if !path.starts_with("Runtime/") || path.len() < 5 || !path[path.len() - 4..].eq_ignore_ascii_case(".iff") {
            continue;
        }
        let Some(f) = d.read(path).and_then(IffFile::read) else { continue };
        let order = bhav_order(&f);
        let mut trees = BTreeMap::new(); // by id, as the library keeps them
        for c in f.of(b"BHAV") {
            match read_bhav(c.id, f.data(c), order) {
                Ok(t) => {
                    trees.insert(c.id as u16, t);
                }
                Err(_) => failed += 1,
            }
        }
        read += trees.len();
    }
    println!("BHAV read: {read}, failed {failed} (tools/check_chunks.py: 9529)");
    assert_eq!((read, failed), (9529, 0));
}
