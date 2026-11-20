//! The run alone (`replay --alone`): two runs with the same seed end in the same world
//! (HOUSEFIRE_BUSTIN_ISO, HOUSEFIRE_HOME_LOG and HOUSEFIRE_CARD, the user's own files kept out of
//! the repository; skipped if not all set).

use housefire::disc::Disc;
use housefire::library::Library;
use housefire::replay::{AloneRun, RunWith, Start, alone};
use housefire::tree_log::read_log;

fn env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|s| !s.is_empty())
}

fn run(seed: u64) -> u64 {
    let (iso, log, card) = (env("HOUSEFIRE_BUSTIN_ISO").unwrap(), env("HOUSEFIRE_HOME_LOG").unwrap(), env("HOUSEFIRE_CARD").unwrap());
    let d = Disc::open(&iso).expect("the disc");
    let lib = Library::bustin_out(&d);
    let log = read_log(log.to_str().unwrap()).expect("the log");
    let start = Start::read(&d, card.to_str(), -1).expect("the start");
    let with = RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: None };
    alone(&log, &lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, with, AloneRun { ticks: 900, seed }).expect("a run").hash
}

#[test]
fn the_same_seed_ends_in_the_same_world() {
    if env("HOUSEFIRE_BUSTIN_ISO").is_none() || env("HOUSEFIRE_HOME_LOG").is_none() || env("HOUSEFIRE_CARD").is_none() {
        println!("alone: skipped (HOUSEFIRE_BUSTIN_ISO, HOUSEFIRE_HOME_LOG, HOUSEFIRE_CARD not all set)");
        return;
    }
    assert_eq!(run(1), run(1));
}
