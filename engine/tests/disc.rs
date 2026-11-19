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
use housefire::sound::{self, SoundBank, Source};
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

/// The sound answer key (`docs/assets.md`, "Sound"): every 200th entry of samples.arc and the
/// first million samples of two streams' first channel, each by rate, sample count and the first
/// 16 hex digits of the SHA-256 of the decoded samples written little-endian. From an independent
/// Python decoder of the accelerator's algorithm.
const SOUND_KEY: [(&str, &str, u32, usize, &str); 36] = [
    ("samples.arc", "vox_nagfrnd_nagee_endm76", 22050, 36556, "69e563dbf625713b"),
    ("samples.arc", "ghost3", 22050, 106496, "86db988ab2f9e315"),
    ("samples.arc", "vox_smoothtalkee_accf6", 22050, 282240, "78d6752ba9332927"),
    ("samples.arc", "vox_slowdancer_accm46", 22050, 212151, "279396dd31836879"),
    ("samples.arc", "FRIDGE_X_OPEN", 22050, 8512, "097a91fa894cd45f"),
    ("samples.arc", "ICECHEST_SHIVER_VOXM6", 22050, 74386, "dd2a82603ce27e09"),
    ("samples.arc", "VOX_CALLHERERF8", 22050, 30282, "a0d4cbfa4bdd7990"),
    ("samples.arc", "vox_hypnotizer_rejectf15", 22050, 137121, "27397a14c59be97e"),
    ("samples.arc", "vox_dirty_joker_startf14", 22050, 135592, "e8fd9787b289f025"),
    ("samples.arc", "wave6", 22050, 67036, "57dc3c1ca0ecc436"),
    ("samples.arc", "vox_pullfingerer_acceptm7", 22050, 165111, "8fe3c630c6213142"),
    ("samples.arc", "VOX_PHONE_TALKK1", 22050, 24046, "482c2bb21206196b"),
    ("samples.arc", "vox_pwheel_loop2f3", 22050, 100196, "5e6c907e08c82f35"),
    ("samples.arc", "VOX_BABYTALKF1", 22050, 45802, "16ac595cf985de90"),
    ("samples.arc", "vox_tongue_kissee_acceptm13", 22050, 156879, "ec17096609a7b830"),
    ("samples.arc", "VOX_NO_UHUHM17", 22050, 13172, "f6dbd6bb05b6fedb"),
    ("samples.arc", "DOLLHOUSE_SOUNDS3", 22050, 15349, "a194a7c196f2b94b"),
    ("samples.arc", "ppong_server_loop1", 22050, 84270, "77cbba6a7befd6d4"),
    ("samples.arc", "catmeow3", 22050, 23020, "8376110a5d5037b5"),
    ("samples.arc", "DOLLHOUSE_SOUNDS5", 22050, 19714, "1b14e7ac6d632fed"),
    ("samples.arc", "nite7", 22050, 161700, "b6ad8092136e2674"),
    ("samples.arc", "vox_gossiper_rejectm24", 22050, 140857, "de2daf1634e530b1"),
    ("samples.arc", "VOX_THROWUPK1", 22050, 17699, "7cf1c00f14c5bd43"),
    ("samples.arc", "vox_dirty_jokee_acceptm25", 22050, 105839, "2d3a7dd8ef8020cf"),
    ("samples.arc", "VOX_CRY_HARDK2", 22050, 359298, "aff1ba50016aafb6"),
    ("samples.arc", "UI_CAC_CYCLEHEAD", 22050, 5459, "2d2c12c0654129f8"),
    ("samples.arc", "VOX_TICKLER_TICKLEK6", 22050, 64769, "5cc063c99123d343"),
    ("samples.arc", "UI_accept", 22050, 28863, "e783727510701d98"),
    ("samples.arc", "FLOOD_FLOODING", 22050, 79928, "54d380c33acdcd71"),
    ("samples.arc", "VOX_SING_FUNK7", 22050, 94832, "c9c14b286e102070"),
    ("samples.arc", "VOX_JOKEE_BOMBF2", 22050, 45101, "7c29504b11bcf056"),
    ("samples.arc", "VOX_BABYSINGF1", 22050, 440825, "57b9aab82ab54c43"),
    ("samples.arc", "BENI_PREPC2", 22050, 48200, "cdf0562fcbcb40b8"),
    ("samples.arc", "vox_geisered_cornerf12", 22050, 136219, "b7e194346fd5c5c0"),
    ("audiostr.arc", "sbo_pop4", 24000, 1000000, "b8ec2194dcac145d"),
    ("audiostr.arc", "loadloop", 48000, 1000000, "25cc0d65ebd22679"),
];

#[test]
fn sounds_decode_as_the_answer_key() {
    let Some(iso) = std::env::var_os("HOUSEFIRE_BUSTIN_ISO").filter(|s| !s.is_empty()) else {
        println!("sounds: skipped (HOUSEFIRE_BUSTIN_ISO not set)");
        return;
    };
    let d = Disc::open(&iso).expect("the disc");
    let bank = SoundBank::open(&d).expect("the sound archives");
    for (arc, name, rate, count, hash) in SOUND_KEY {
        let i = bank.find(name).unwrap_or_else(|| panic!("{name} not in the bank"));
        assert_eq!(bank.entries()[i].source == Source::Stream, arc == "audiostr.arc", "{name}");
        let s = bank.sound(i).unwrap_or_else(|| panic!("{name} does not parse"));
        let v: Vec<i16> = s.decode(0).take(1_000_000).collect();
        assert_eq!((s.rate, v.len(), sound::hash16(&v).as_str()), (rate, count, hash), "{name}");
    }
    // every sound parses and decodes to its header's count, the stereo ones in both channels
    for (i, e) in bank.entries().iter().enumerate() {
        let s = bank.sound(i).unwrap_or_else(|| panic!("{} does not parse", e.entry.name));
        assert!(s.channel_count() == 1 || e.source == Source::Stream);
        assert_eq!(s.decode(s.channel_count() - 1).count(), s.samples as usize, "{}", e.entry.name);
    }
}
