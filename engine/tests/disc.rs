//! The readers and the interpreter on the player's own Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO;
//! skipped if it is not set): the C++ engine's `test_disc`, its very lines. Nothing from the
//! disc is kept.
//!
//! Every BHAV on the disc must read (9,529, as `tools/check_chunks.py` counts), and every tree
//! is run as a check tree with a host whose primitives answer by a fixed rule (true for an even
//! opcode, false for an odd one) and whose calls find the trees as the engine would (the
//! library's lookup: own, semi-global, global by the id's range). Each run must end: by a
//! return, by one of the node step's errors, or at the iteration limit.

use std::collections::BTreeMap;

use housefire::bhav::{Node, Tree};
use housefire::disc::Disc;
use housefire::interp::{Error, Host, Interpreter, Mode, PrimResult, Trees};
use housefire::library::Library;
use housefire::{FileId, TreeId};

struct RuleHost<'a> {
    lib: &'a Library,
    own: FileId,
}

impl Trees for RuleHost<'_> {
    fn tree_at(&self, t: TreeId) -> &Tree {
        self.lib.tree_at(t)
    }
}

impl Host for RuleHost<'_> {
    fn tree(&mut self, id: u16) -> Option<TreeId> {
        self.lib.tree(Some(self.own), id)
    }
    fn primitive(&mut self, _: &mut Interpreter, n: &Node) -> PrimResult {
        if !n.opcode.is_multiple_of(2) { PrimResult::False } else { PrimResult::True }
    }
}

#[test]
fn every_script_on_the_disc() {
    let Some(iso) = std::env::var_os("HOUSEFIRE_BUSTIN_ISO").filter(|s| !s.is_empty()) else {
        println!("test_disc: skipped (HOUSEFIRE_BUSTIN_ISO not set)");
        return;
    };
    let d = Disc::open(&iso).expect("the disc");
    assert_eq!(d.id(), "G4ME69", "not Bustin' Out");
    let lib = Library::bustin_out(&d);
    let mut out = String::new();
    use std::fmt::Write;

    let bhavs: usize = lib.files().iter().map(|f| f.trees.len()).sum();
    let bad = lib.bhav_failures();
    writeln!(out, "BHAV read: {bhavs}, failed {bad} (tools/check_chunks.py: 9529)").unwrap();
    let constants: usize = lib.files().iter().map(|f| f.constants.len()).sum();
    writeln!(out, "BCON read: {constants}").unwrap();

    let mut outcome: BTreeMap<String, usize> = BTreeMap::new();
    let mut steps = 0u64;
    for (k, f) in lib.files().iter().enumerate() {
        let mut h = RuleHost { lib: &lib, own: FileId(k as u32) };
        for &t in f.trees.values() {
            let mut i = Interpreter::new(Mode::Check, 2000);
            i.start(&h, Some(t), &[], None);
            steps += i.run(&mut h, 100000) as u64;
            let key = if !i.done() {
                "did not end".to_string()
            } else if i.error() == Error::None {
                if i.result() { "returned true" } else { "returned false" }.to_string()
            } else {
                format!("error {}", i.error() as u16)
            };
            *outcome.entry(key).or_default() += 1;
        }
    }
    let (mut ended, mut total) = (0, 0);
    for (k, v) in &outcome {
        writeln!(out, "  {k:<16} {v}").unwrap();
        total += v;
        if k != "did not end" {
            ended += v;
        }
    }
    writeln!(out, "trees run: {total}, {steps} steps; all ended: {}", if ended == total { "yes" } else { "no" }).unwrap();

    // String tables (STR#): the two CallNamedTree reads in the logged session, 303 in the party
    // controller (5 strings) and in PersonGlobals (1), as tools/chunks.py reads them.
    let table = |file: &str, id: u16| -> i64 {
        let Some(f) = lib.find(file) else { return -1 };
        lib.file(f).strings.get(&id).map_or(-1, |t| t.len() as i64)
    };
    writeln!(out, "STR# 303: controller_party {}, PersonGlobals {}", table("controller_party", 303), table("PersonGlobals", 303)).unwrap();

    // Tuning tables (FCNS): every one read to its end; the global file's four (17, 12, 19 and 9
    // values), and the motive rates the home log measured (docs/simulation.md, "The motives").
    let tuning_tables: usize = lib.files().iter().map(|f| f.tuning.len()).sum();
    let tuning_values: usize = lib.files().iter().flat_map(|f| f.tuning.values()).map(Vec::len).sum();
    let tv = |n: &str| lib.tuning(n).unwrap_or(-1.0);
    writeln!(
        out,
        "FCNS: {tuning_tables} tables, {tuning_values} values, {} not read; hyg decrement, awake {}; hunger decrement ratio {}; energy span {}",
        lib.tuning_failures(),
        tv("hyg decrement, awake"),
        tv("hunger decrement ratio"),
        tv("energy span")
    )
    .unwrap();

    // Interaction tables (TTAB): the home log's three autonomous choices in their objects'
    // tables (OBJD field 5), each advertising what the Sim lacked (docs/simulation.md,
    // "Autonomy"): the fridge's snack hunger +20, the toilet bladder +70, the easel fun +10.
    let advert = |guid: u32, action: u16, motive: usize| -> i32 {
        let Some(def) = lib.definition(guid) else { return -999 };
        let Some(t) = lib.file(def.file).tables.get(&lib.def(def).interaction_table()) else { return -998 };
        match t.iter().find(|e| e.action == action) {
            Some(e) => e.adverts.get(motive).map_or(-997, |a| a.delta as i32),
            None => -996,
        }
    };
    let (fridge, toilet, easel) = (advert(0x675C18AF, 8215, 7), advert(0x3C566968, 8223, 9), advert(0x9FB223CE, 4097, 15));
    let tables: usize = lib.files().iter().map(|f| f.tables.len()).sum();
    writeln!(out, "TTAB: {tables} tables; the fridge's snack hunger {fridge}, the toilet bladder {toilet}, the easel fun {easel}").unwrap();

    // Slots (SLOT): every one of version 10 and read to its end; the global file's 15.
    let slot_tables: usize = lib.files().iter().map(|f| f.slots.len()).sum();
    let global_slots = lib.global_file().filter(|g| g.slots.len() == 1).map_or(0, |g| g.slots.values().next().unwrap().len());
    writeln!(out, "SLOT: {slot_tables} tables, {} not read; the global file's {global_slots} slots", lib.slot_failures()).unwrap();
    print!("{out}");

    assert!(bad == 0 && bhavs == 9529 && total == bhavs && ended == total);
    assert!(table("controller_party", 303) == 5 && table("PersonGlobals", 303) == 1);
    assert!(lib.tuning_failures() == 0 && lib.global_file().is_some_and(|g| g.tuning.len() == 4));
    assert!(tv("hyg decrement, awake") == 0.17 && tv("hunger decrement ratio") == 0.0021 && tv("energy span") == 180.0);
    assert!(fridge == 20 && toilet == 70 && easel == 10 && lib.slot_failures() == 0 && global_slots == 15);
    // The C++ engine's figures besides (its test_disc's lines).
    assert_eq!(constants, 1689);
    // more than the C++ engine's 1,251,290: a semi-global file's trees reach its other trees
    // (a frame whose file is PersonGlobals), as the original's do (the home log's Missing calls)
    assert_eq!(steps, 1339572);
    assert_eq!(tables, 856);
    assert_eq!(slot_tables, 764);
}
