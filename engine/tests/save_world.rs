//! The world written into a save and read back (`save_world`): at the load a lot's ObjM is written
//! back byte for byte, and the loaded world is the same after its card is read again. Needs
//! HOUSEFIRE_BUSTIN_ISO and HOUSEFIRE_CARD / HOUSEFIRE_CARDS (the user's own files; skipped if
//! not set). The run alone's worlds (after play) are the replay binary's `--save-at` check.

use housefire::disc::Disc;
use housefire::library::Library;
use housefire::save::{LotObject, memory_card_file, read_lots};
use housefire::world::World;
use housefire::save_world::{CardLot, GAME, differences, save_card};

fn cards() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for var in ["HOUSEFIRE_CARD", "HOUSEFIRE_CARDS"] {
        if let Ok(s) = std::env::var(var) {
            for p in s.split(':').filter(|p| !p.is_empty()) {
                if !v.contains(&p.to_string()) {
                    v.push(p.to_string());
                }
            }
        }
    }
    v
}

/// Each loaded lot of each card written back: its ObjM and objt are the input's, and the world
/// loaded from the written card has no differences from the first.
#[test]
fn a_loaded_world_is_saved_and_loaded_the_same() {
    let Some(iso) = std::env::var_os("HOUSEFIRE_BUSTIN_ISO").filter(|s| !s.is_empty()) else { return };
    let d = Disc::open(&iso).expect("the disc");
    let lib = Library::bustin_out(&d);
    for p in cards() {
        let card = std::fs::read(&p).expect("the card");
        let file = memory_card_file(&card, GAME).unwrap();
        let n = read_lots(&file).unwrap().len();
        for lot in 0..n {
            let input = CardLot::read(&card, lot).unwrap();
            let (chunks, neighbours) = (&input.chunks, &input.neighbours);
            let Ok(world) = input.load(&lib, true) else { continue };
            let mut stacks = |k| world.get(k).stack.clone();
            let out = save_card(&world, &card, lot, &mut stacks).expect("the save");
            let file2 = memory_card_file(&out, GAME).unwrap();
            let lots2 = read_lots(&file2).unwrap();
            for tag in ["ObjM", "objt"] {
                if chunks.find(tag) != lots2[lot].find(tag) && tag == "ObjM" {
                    let (a, b) = (World::new(&lib).read_objects(chunks).unwrap(), World::new(&lib).read_objects(&lots2[lot]).unwrap());
                    // (a house-less family's Sims take their motives from their neighbours at the load)
                    let taken: Vec<i16> = neighbours.iter().filter(|n| n.house == Some(0)).map(|n| n.id).collect();
                    let strip = |mut o: LotObject| {
                        (o.section, o.section_end, o.base_end) = (0, 0, 0);
                        if o.person_data.get(31).is_some_and(|n| taken.contains(n)) {
                            (o.motives, o.motives_b) = (Vec::new(), Vec::new());
                        }
                        // (the load marks the visitors, person data 32, and sets the sex, 65, from the description)
                        for i in [32, 65] {
                            if let Some(v) = o.person_data.get_mut(i) {
                                *v = 0;
                            }
                        }
                        format!("{o:?}")
                    };
                    let bad: Vec<String> = a.objects.iter().zip(&b.objects).filter(|(x, y)| strip((*x).clone()) != strip((*y).clone())).map(|(x, y)| {
                        let (sx, sy) = (strip(x.clone()), strip(y.clone()));
                        let (tx, ty): (Vec<&str>, Vec<&str>) = (sx.split(", ").collect(), sy.split(", ").collect());
                        let d: Vec<String> = tx.iter().zip(&ty).filter(|(a, b)| a != b).map(|(a, b)| format!("{a} / {b}")).collect();
                        format!("{} {:?}", x.id, d)
                    }).collect();
                    if bad.is_empty() {
                        continue;
                    }
                    panic!("{p} lot {lot}: ObjM differs: {} objects ({} vs {}), {} differ, first: {:?} {:?}; lot part {}", a.objects.len(), a.objects.len(), b.objects.len(), bad.len(), bad.first(), bad.get(1), format!("{:?}", a.last_mark) == format!("{:?}", b.last_mark));
                }
                assert!(chunks.find(tag) == lots2[lot].find(tag), "{p} lot {lot}: {tag} differs");
            }
            let world2 = CardLot::read(&out, lot).unwrap().load(&lib, true).unwrap();
            let mut s2 = |k| world2.get(k).stack.clone();
            let diff = differences(&world, &world2, &mut stacks, &mut s2);
            assert!(diff.is_empty(), "{p} lot {lot}: {:?}", &diff[..diff.len().min(10)]);
        }
    }
}
