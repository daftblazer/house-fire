//! The Sims' outfits (LOCKSTEP_ANIM_OUTFITS, stage 4.4e): each Sim dressed ('?', Sim_SetModels
//! 800591CC) with its description's words and the model part slots it set, the slots named by
//! the models.arc entries whose name hash they are; then two checks of `outfit`:
//!
//! 1. the descriptions from the save: each logged description's bytes (the gender, the age and
//!    the 18 bytes of looks, 0 to 25) against the engine's, found by the Sim's neighbour (its
//!    person data 31) and that neighbour's made Sim in the save (`save::read_created_sims`);
//! 2. the slots from the tables: each Sim's nine part slots, from the logged description and
//!    from the engine's, against the logged slots.

use crate::*;
use housefire::outfit::{Description, PartTables, Slot};
use housefire::shader::name_hash;
use housefire::tree_log::SimDressRec;

/// How far below a Sim's record the pointer of its object can be (the objects' pointers are the
/// original's own: its object lists and the scripts' reads of an object's id name them).
const SIM_SPAN: u32 = 0x2000;

/// The Sim's model part slots: words +1088..+1132 (the record holds +1020 on).
const SLOTS: core::ops::Range<usize> = (1088 - 1020) / 4..(1136 - 1020) / 4;

/// Where the original keeps each slot's hash in a Sim (the record's words are from +1020).
fn slot_word(slot: Slot) -> usize {
    let at = match slot {
        Slot::Accessory => 1092,
        Slot::Head => 1100,
        Slot::Upper => 1104,
        Slot::Lower => 1108,
        Slot::Shoes => 1112,
        Slot::Eyes => 1116,
        Slot::Nose => 1120,
        Slot::Mouth => 1124,
        Slot::Extra => 1128,
    };
    (at - 1020) / 4
}

/// The description's bytes 0 to 25 as the original keeps them: the gender and the age in words
/// 0 and 1, the looks from byte 8.
fn description_bytes(d: &Description) -> [u8; 26] {
    let mut b = [0; 26];
    b[3] = d.male as u8;
    b[7] = d.adult as u8;
    b[8..].copy_from_slice(&d.looks);
    b
}

/// The slots `d` gives against the slots the original set: how many are the same, how many not.
fn count_slots(tables: &PartTables, r: &SimDressRec, d: &Description, count: &mut (u32, u32)) {
    let o = tables.dress(d);
    for slot in Slot::ALL {
        let logged = r.sim_words[slot_word(slot)];
        if o.part(slot) == logged {
            count.0 += 1;
        } else {
            count.1 += 1;
            println!("  sim {:08x} slot {slot:?}: table {:08x} logged {logged:08x}", r.sim, o.part(slot));
        }
    }
}

pub fn outfits(log: &housefire::tree_log::Log, d: &Disc) {
    let models = d.read("models.arc").unwrap_or_default();
    let by_hash: HashMap<u32, String> = housefire::arc::entries(&models).unwrap_or_default().into_iter().map(|e| (name_hash(&e.name), e.name)).collect();
    println!("Sims dressed: {}", log.dressed.len());
    for r in &log.dressed {
        let slots: Vec<String> = r.sim_words[SLOTS].iter().map(|&h| if h == 0 { "-".into() } else { by_hash.get(&h).cloned().unwrap_or_else(|| format!("{h:08x}")) }).collect();
        println!("  tick {} sim {:08x} description {:08x} {:08x?}", r.tick, r.sim, r.description, r.description_words);
        println!("    slots +1088..: {slots:?}");
    }

    // The save the log ran on (--card, as the lockstep's), the disc's part tables and Sims.
    let args: Vec<String> = std::env::args().collect();
    let ngh = match args.iter().position(|a| a == "--card").and_then(|i| args.get(i + 1)) {
        Some(p) => {
            let mut card = std::fs::read(p).unwrap_or_default();
            card.truncate(1 << 22);
            memory_card_file(&card, "G4ME69").unwrap_or_default()
        }
        None => d.read("Runtime/default.ngh").unwrap_or_default(),
    };
    let quick = d.read("quickdat.arc").unwrap_or_default();
    let Some(tables) = housefire::arc::entries(&quick)
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.name == "CreateASim")
        .and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)))
        .and_then(|img| PartTables::read(&img))
    else {
        println!("outfits: no part tables");
        return;
    };
    let neighbours = housefire::save::read_neighbours(&ngh).unwrap_or_default();
    let made = housefire::save::read_created_sims(&ngh).unwrap_or_default();
    let lib = Library::bustin_out(d);
    let (lots, _, _, best, _) = pick_lot(&lib, &ngh, log);
    let mut world = World::new(&lib);
    world.load_lot(&lots[best]).unwrap();

    // The original's pointers to objects with their ids (its object lists, and the scripts' reads
    // of an object's id). A Sim's own record lies above its object's pointer (the dressing is
    // given the record), so the Sim is the nearest person object below it.
    let mut objects: BTreeMap<u32, i16> = BTreeMap::new();
    for l in &log.lists {
        objects.extend(l.objects.iter().copied().zip(l.ids.iter().copied()));
    }
    for v in log.steps.iter().flat_map(|s| &s.lookups).filter(|v| v.scope == 3 && v.data as usize == housefire::world::DATA_OBJECT_ID) {
        objects.insert(v.object, v.value);
    }
    let persons: BTreeSet<i16> = world.objects().filter(|o| o.person).map(|o| o.id()).collect();
    let (mut same, mut different, mut missing) = (0, 0, 0);
    let (mut slots_logged, mut slots_native) = ((0, 0), (0, 0));
    for r in &log.dressed {
        let none = r.sim_words[SLOTS].iter().all(|&h| h == 0);
        // The description the engine has for the Sim: its object's id, its neighbour's id
        // (person data 31), the neighbour's GUID, the made Sim of that GUID.
        let id = objects.range(r.sim.saturating_sub(SIM_SPAN)..=r.sim).rev().find(|(_, id)| persons.contains(id)).map(|(_, &id)| id);
        let native = id
            .and_then(|id| world.find_object(id))
            .and_then(|o| o.person_data.get(31).copied())
            .and_then(|n| neighbours.iter().find(|x| x.id == n))
            .and_then(|n| made.iter().find(|m| m.guid == n.guid))
            .map(Description::from_save);
        if r.description == 0 {
            // not dressed from a description: no part slots either, and none from the save
            println!("  sim {:08x} (id {id:?}): no description; slots empty {none}, in the save {}", r.sim, native.is_some());
            if none && native.is_none() {
                same += 1;
            } else {
                different += 1;
            }
            continue;
        }
        // the logged description's fields through the tables
        let bytes: Vec<u8> = r.description_words.iter().flat_map(|w| w.to_be_bytes()).collect();
        let logged = Description { male: bytes[3] != 0, adult: bytes[7] != 0, looks: bytes[8..26].try_into().unwrap() };
        count_slots(&tables, r, &logged, &mut slots_logged);
        // and the engine's description: the save's
        let Some(native) = native else {
            println!("  sim {:08x} (id {id:?}): description not in the save", r.sim);
            missing += 1;
            continue;
        };
        let mine = description_bytes(&native);
        if bytes[..26] == mine {
            same += 1;
        } else {
            different += 1;
            println!("  sim {:08x} (id {id:?}) DIFFERENT description: logged {:02x?} native {mine:02x?}", r.sim, &bytes[..26]);
        }
        count_slots(&tables, r, &native, &mut slots_native);
    }
    println!("descriptions from the save: {same} same, {different} DIFFERENT, {missing} not in the save");
    println!("slots from the tables, the logged description: {} same, {} DIFFERENT", slots_logged.0, slots_logged.1);
    println!("slots from the tables, the engine's description: {} same, {} DIFFERENT", slots_native.0, slots_native.1);
}
