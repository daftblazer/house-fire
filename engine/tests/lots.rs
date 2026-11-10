//! The lots of the player's Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO; skipped if not set), and a
//! save (HOUSEFIRE_CARD: a memory card image with a Bustin' Out save; the user's own, kept out
//! of the repository): the C++ engine's `test_lots`, its figures. The counts are
//! `tools/ngh.py`'s, `lot.py`'s and `objm.py`'s.

use housefire::disc::Disc;
use housefire::save::{memory_card_file, read_lot_arrays, read_lots, read_neighbourhood, read_neighbours, read_object_types};

fn disc() -> Option<Disc> {
    let iso = std::env::var_os("HOUSEFIRE_BUSTIN_ISO").filter(|s| !s.is_empty())?;
    let d = Disc::open(&iso).expect("the disc");
    assert_eq!(d.id(), "G4ME69", "not Bustin' Out");
    Some(d)
}

/// The neighbourhood file's blocks and lots, every lot's object types and arrays, and the
/// neighbours: what needs no library.
#[test]
fn the_neighbourhood_file() {
    let Some(d) = disc() else {
        println!("lots: skipped (HOUSEFIRE_BUSTIN_ISO not set)");
        return;
    };
    let ngh = d.read("Runtime/default.ngh").expect("default.ngh");
    let blocks = read_neighbourhood(&ngh).unwrap();
    let lots = read_lots(&ngh).unwrap();
    let mut types = 0;
    let mut arrays = 0;
    for lot in &lots {
        types += read_object_types(lot.find("objt").expect("objt")).unwrap().len();
        arrays += read_lot_arrays(lot).unwrap().iter().filter(|a| a.width == 64 && a.height == 64).count();
    }
    let nb = read_neighbours(&ngh).unwrap();
    println!("blocks {}, lots {}, object types {types}; arrays read, 64 by 64: {arrays}", blocks.len(), lots.len());
    println!("neighbours {}, ids {} to {}", nb.len(), nb[0].id, nb[nb.len() - 1].id);
    assert_eq!((blocks.len(), lots.len(), types, arrays), (35, 16, 2000, 48));
    assert_eq!((nb.len(), nb[0].id, nb[nb.len() - 1].id), (51, 3, 63));
}

/// A save's file on the card, and its lots.
#[test]
fn a_save() {
    let Some(card) = std::env::var_os("HOUSEFIRE_CARD").filter(|s| !s.is_empty()) else {
        println!("lots: the save skipped (HOUSEFIRE_CARD not set)");
        return;
    };
    let card = std::fs::read(card).expect("the card");
    let save = memory_card_file(&card, "G4ME69").unwrap();
    let lots = read_lots(&save).unwrap();
    println!("the save: {} bytes, lots {}", save.len(), lots.len());
    assert_eq!((save.len(), lots.len()), (1318912, 16));
}
