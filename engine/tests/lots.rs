//! The lots of the player's Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO; skipped if not set), and a
//! save (HOUSEFIRE_CARD: a memory card image with a Bustin' Out save; the user's own, kept out
//! of the repository): the C++ engine's `test_lots`, its figures. The counts are
//! `tools/ngh.py`'s, `lot.py`'s and `objm.py`'s.

use housefire::disc::Disc;
use housefire::library::Library;
use housefire::save::{SavedClass, memory_card_file, read_lot_arrays, read_lots, read_neighbourhood, read_neighbours, read_object_types};
use housefire::world::World;

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

/// Every lot as a world: its objects, their classes (every section read to exactly its mark),
/// their saved tree stacks against the trees they name, the Sims' motives, and the club's
/// object order (the running game's: docs/loop.md, iteration 5 of the first loop).
#[test]
fn every_lot_as_a_world() {
    let Some(d) = disc() else { return };
    let lib = Library::bustin_out(&d);
    let ngh = d.read("Runtime/default.ngh").expect("default.ngh");
    let lots = read_lots(&ngh).unwrap();
    let (mut objects, mut missing) = (0, 0);
    let (mut frames, mut found, mut fit, mut person_frames) = (0, 0, 0, 0);
    let (mut by_class, mut sims_with_motives, mut lot_parts, mut club_relationships) = ([0usize; 4], 0, 0, 0);
    let mut club = Vec::new();
    for (k, lot) in lots.iter().enumerate() {
        let mut w = World::new(&lib);
        missing += w.load_lot(lot).unwrap();
        objects += w.len();
        let lo = w.read_objects(lot).unwrap();
        for o in &lo.objects {
            by_class[o.saved_class as usize] += 1;
        }
        if k == lots.len() - 1 {
            club_relationships += lo.objects.iter().map(|o| o.relationships.len()).sum::<usize>();
        }
        // The lot's own part, read to the stream's end: on the disc no objects, and an empty
        // string.
        lot_parts += (lo.lot_object == 0 && lo.lot_objects.is_empty() && lo.lot_string.is_empty()) as usize;
        for o in w.objects() {
            sims_with_motives += (o.person && o.motives.len() == 16 && o.person_data.len() == 80) as usize;
            for f in &o.stack {
                frames += 1;
                if o.person {
                    person_frames += 1;
                    continue;
                }
                let Some(t) = f.tree else { continue };
                found += 1;
                let t = lib.tree_at(t);
                fit += ((f.node as usize) < t.nodes.len() && f.params.len() == t.args as usize && f.locals.len() == t.locals as usize) as usize;
            }
        }
        if k == lots.len() - 1 {
            club = w.objects().take(6).map(|o| o.id()).collect();
        }
    }
    println!("objects {objects} ({missing} without a definition on the disc)");
    println!("tree stack frames {frames} ({person_frames} of characters), found {found}, fitting their tree {fit}");
    println!(
        "classes: {} objects, {} parts, {} door parts, {} Sims ({sims_with_motives} with motives and person data)",
        by_class[0], by_class[1], by_class[2], by_class[3]
    );
    println!("lots' own parts as expected: {lot_parts}; the club's relationships: {club_relationships}");
    println!("the club's first objects: {club:?}");
    assert_eq!((objects, missing), (4572, 53));
    assert_eq!(club, [178, 177, 176, 174, 172, 171]);
    assert_eq!((frames, person_frames, found, fit), (4575, 53, 4522, 4522));
    assert_eq!(by_class, [1754, 1821, 944, 53]);
    assert_eq!((sims_with_motives, lot_parts, club_relationships), (53, 16, 16));
}

/// A save's lots read as the disc's, every object to its end. Checked for the save of Mom's
/// house (docs/lots.md, "Saves"): its first lot's 206 objects are the running game's world list
/// when it loads.
#[test]
fn a_saves_objects() {
    let Some(d) = disc() else { return };
    let Some(card) = std::env::var_os("HOUSEFIRE_CARD").filter(|s| !s.is_empty()) else { return };
    let lib = Library::bustin_out(&d);
    let save = memory_card_file(&std::fs::read(card).unwrap(), "G4ME69").unwrap();
    let lots = read_lots(&save).unwrap();
    let (mut objects, mut sims, mut deltas) = (0, 0, 0);
    for lot in &lots {
        let lo = World::new(&lib).read_objects(lot).unwrap();
        objects += lo.objects.len();
        for o in lo.objects.iter().filter(|o| o.saved_class == SavedClass::Sim) {
            sims += 1;
            deltas += o.motive_deltas.len();
        }
    }
    let first = World::new(&lib).read_objects(&lots[0]).unwrap().objects.len();
    println!("the save: lots {}, objects {objects} (the first lot {first}), Sims {sims}, motive deltas {deltas}", lots.len());
    assert_eq!((lots.len(), objects, first, sims, deltas), (16, 4582, 206, 54, 15));
}
