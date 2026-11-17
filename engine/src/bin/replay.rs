//! The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
//! interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: replay LOG ISO [--card CARD] [--lot N] [--bench]
//!
//! --card: the lot from a save on a memory card image (its first lot, Mom's house in the user's
//! save, unless --lot says), not the disc's club; the neighbours are the save's too.
//!
//! The world is the disc's club lot, loaded natively (the log starts as the club loads): each
//! object's saved tree stack, attributes, temps, data, and the Sims' motives and person data.
//! The simulator says whose turn it is each tick; the turns are compared with the log's. A tick
//! without the pass over the objects is a paused one, whose clock does not move, and steps
//! outside the pass are the engine's own (the pie menu's), not run (the log's 'Q' records). Each
//! stack is then stepped natively for as many steps as the log has for it in that tick, and
//! before each step its top frame (depth, tree, node) is compared with the log's. The native
//! state is kept from step to step and tick to tick: frames, sleep, the variables.
//!
//! The primitives the engine has run natively (the expression, Idle, SetToNext, ...) on the
//! native world; each variable they read is compared with what the original's lookup found
//! before it is used. The others are answered from the log: their result, and their frame's
//! parameters, locals and stack object after them.
//!
//! Where the native state differs from the log's it is counted and then set to the log's, so
//! that the run goes on: a frame (an interaction the engine pushed, which is not native yet), a
//! variable (one a primitive that is not native wrote). Objects that are not the lot's (the
//! household, made as the lot loads, and objects made later) are not run.
//!

use housefire::disc::Disc;
use housefire::library::Library;
use housefire::replay::{Bench, RunWith, Start, atoi, env, has, run};
use housefire::save::read_lot_arrays;
use housefire::tree_log::read_log;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: replay LOG ISO [--card CARD] [--lot N] [--bench]");
        return ExitCode::from(2);
    }
    let Some(log) = read_log(&args[1]) else {
        println!("cannot read {}", args[1]);
        return ExitCode::from(1);
    };
    let Ok(d) = Disc::open(&args[2]) else {
        println!("cannot read {}", args[2]);
        return ExitCode::from(1);
    };
    let lib = Library::bustin_out(&d);
    let mut bench = false;
    let mut card_path: Option<String> = None;
    let mut lot_index: i64 = -1;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--bench" => bench = true,
            "--card" if i + 1 < args.len() => {
                i += 1;
                card_path = Some(args[i].clone());
            }
            "--lot" if i + 1 < args.len() => {
                i += 1;
                lot_index = atoi(&args[i]);
            }
            a => {
                println!("unknown option {a}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let start = match Start::read(&d, card_path.as_deref(), lot_index) {
        Ok(s) => s,
        Err(e) => {
            println!("{e}");
            return ExitCode::from(1);
        }
    };
    let (the_lot, the_globals, neighbours, lot_globals, card_lot) = (start.lot(), start.globals(), &start.neighbours, &start.lot_globals, start.card_lot);
    if has("REPLAY_SHOW_LOT_CLOCKS") {
        // study: each lot's clock (globals 0, 5, 6: the hour, minute and second)
        for (k, g) in lot_globals.iter().enumerate() {
            println!("lot {k}: {:?}, house number (global 10) {:?}", [0, 5, 6].map(|i| g.get(i).copied()), g.get(10));
        }
    }
    let house = start.house.as_ref();
    if has("REPLAY_SHOW_LOT_CHUNKS") {
        // study: the lot's chunks and its arrays as read
        let kinds: Vec<(String, usize)> = the_lot.chunks.iter().map(|(k, c)| (k.clone(), c.len())).collect();
        println!("lot chunks: {kinds:?}");
        match read_lot_arrays(the_lot) {
            Ok(a) => {
                println!("lot arrays: {:?}", a.iter().map(|a| (a.width, a.height, a.element_size, a.bytes.iter().filter(|&&b| b != 0).count())).collect::<Vec<_>>());
                if let Some(w) = a.get(1) {
                    let per: Vec<usize> = (0..w.element_size as usize).map(|k| w.bytes.iter().skip(k).step_by(w.element_size as usize).filter(|&&b| b != 0).count()).collect();
                    let low: usize = w.bytes.iter().step_by(w.element_size as usize).filter(|&&b| b & 0x0F != 0).count();
                    println!("array 1 nonzero by byte {per:?}; byte 0 with wall bits {low}");
                }
            }
            Err(e) => println!("lot arrays: error {e:?}"),
        }
    }
    if !bench {
        let Some((counts, _)) = run(&log, &lib, (the_lot, house), the_globals, neighbours, None, RunWith { card_lot, goals: start.goals.clone(), cat: (!log.tracks.is_empty()).then(|| housefire::anim::Catalog::read(&d, env("REPLAY_HOUSE").and_then(|v| v.parse().ok()).unwrap_or(1))) }) else { return ExitCode::from(1) };
        for (k, n) in &counts.map {
            println!("  {k:<66} {n}");
        }
        return ExitCode::SUCCESS;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    let mut runs: Vec<Bench> = Vec::new();
    for _ in 0..5 {
        let Some((_, Some(b))) = run(&log, &lib, (the_lot, house), the_globals, neighbours, Some(Bench::default()), RunWith { card_lot, goals: start.goals.clone(), cat: None }) else { return ExitCode::from(1) };
        runs.push(b);
    }
    let median = |f: &dyn Fn(&Bench) -> f64| {
        let mut v: Vec<f64> = runs.iter().map(f).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let pct = |v: &[f64], q: f64| {
        let mut v = v.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[(q * (v.len() - 1) as f64) as usize]
    };
    let ticks = runs[0].ticks.len();
    println!(
        "bench: {ticks} ticks, {} native steps ({:.1} a tick), 5 runs, medians",
        runs[0].steps,
        runs[0].steps as f64 / ticks as f64
    );
    println!(
        "  per tick:  mean {:.2} us, median {:.2} us, 99th percentile {:.2} us, worst {:.2} us",
        median(&|b| b.ticks.iter().sum::<f64>() / b.ticks.len() as f64),
        median(&|b| pct(&b.ticks, 0.5)),
        median(&|b| pct(&b.ticks, 0.99)),
        median(&|b| b.ticks[b.worst_tick])
    );
    println!(
        "  the worst tick: tick {}; the worst from tick 10 on (past the load): {:.2} us",
        runs[0].worst_tick + 1,
        median(&|b| b.ticks[9.min(b.ticks.len() - 1)..].iter().copied().fold(f64::MIN, f64::max))
    );
    println!(
        "  per part, us a tick: scripts (steps) {:.2}, of which expression {:.2}, SetToNext {:.2}, Idle {:.2}; the pass {:.2}",
        median(&|b| (b.steps_us - b.sync_us) / b.ticks.len() as f64),
        median(&|b| b.expression_us / b.ticks.len() as f64),
        median(&|b| b.set_to_next_us / b.ticks.len() as f64),
        median(&|b| b.idle_us / b.ticks.len() as f64),
        median(&|b| b.pass_us / b.ticks.len() as f64)
    );
    println!("  not native yet (not in these figures): routing, autonomy, animation, the other primitives");
    ExitCode::SUCCESS
}
