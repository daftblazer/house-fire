//! The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
//! interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: replay LOG ISO [--card CARD] [--lot N] [--bench] [--free] [--drive] [--alone [TICKS]] [--seed N] [--hood CARD] [--save-at TICK OUT.raw]
//!
//! --resume: with --alone, the card's lot is a save of a run (--save-at's): the run goes on from it
//! with no load pass.
//!
//! --save-at: at that tick (of the run alone with --alone, else of the log-driven run) the world is
//! saved on a copy of --card (`save_world`), written to OUT.raw, and read back: the figure printed
//! is how many fields the loaded world differs in (0: the save holds the world).
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
use housefire::iff::FormatError;
use housefire::replay::{Alone, Bench, Replay, RunWith, Start, AloneRun, alone, atoi, drive, env, has, run};
use housefire::save::read_lot_arrays;
use housefire::tree_log::read_log;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: replay LOG ISO [--card CARD] [--lot N] [--bench] [--free] [--drive] [--alone [TICKS]] [--seed N] [--hood CARD] [--save-at TICK OUT.raw] [--resume]");
        return ExitCode::from(2);
    }
    let Some(log) = read_log(&args[1]) else {
        println!("cannot read {}", args[1]);
        return ExitCode::from(1);
    };
    if std::env::var_os("REPLAY_SHOW_MOVIES").is_some() {
        // The game and its Apt movies (']' calls in, '"' commands out), one line each.
        let (mut c, mut m) = (log.movie_calls.iter().peekable(), log.movie_commands.iter().peekable());
        loop {
            let take_call = match (c.peek(), m.peek()) {
                (Some(x), Some(y)) => (x.tick, x.frame) <= (y.tick, y.frame),
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_call {
                let x = c.next().unwrap();
                let args: Vec<String> = x.args.iter().map(|(w, t)| if t.is_empty() { format!("{w:#x}") } else { format!("{t:?}") }).collect();
                println!("movie: tick {} frame {} call screen {:#x} vt {:#x} {}({}) a {:#x} b {:#x} from {:#x}", x.tick, x.frame, x.screen, x.vtable, x.name, args.join(", "), x.a, x.b, x.caller);
            } else {
                let y = m.next().unwrap();
                println!("movie: tick {} frame {} command screen {:#x} vt {:#x} {:?} {:?} handler {:#x}", y.tick, y.frame, y.screen, y.vtable, y.command, y.argument, y.handler);
            }
        }
    }
    let Ok(d) = Disc::open(&args[2]) else {
        println!("cannot read {}", args[2]);
        return ExitCode::from(1);
    };
    let lib = Library::bustin_out(&d);
    let mut bench = false;
    let mut free = false;
    let mut driven = false;
    let mut alone_ticks: Option<u32> = None;
    let mut seed: u64 = 1;
    let mut card_path: Option<String> = None;
    let mut hood_path: Option<String> = None;
    let mut lot_index: i64 = -1;
    let mut save_at: Option<(u32, String)> = None;
    let mut resume = false;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--bench" => bench = true,
            "--card" if i + 1 < args.len() => {
                i += 1;
                card_path = Some(args[i].clone());
            }
            "--free" => free = true,
            "--resume" => resume = true,
            "--drive" => driven = true,
            "--alone" => {
                alone_ticks = Some(54_000);
                if i + 1 < args.len() && args[i + 1].bytes().all(|b| b.is_ascii_digit()) {
                    i += 1;
                    alone_ticks = Some(atoi(&args[i]) as u32);
                }
            }
            "--seed" if i + 1 < args.len() => {
                i += 1;
                seed = atoi(&args[i]) as u64;
            }
            "--hood" if i + 1 < args.len() => {
                i += 1;
                hood_path = Some(args[i].clone());
            }
            "--save-at" if i + 2 < args.len() => {
                save_at = Some((atoi(&args[i + 1]) as u32, args[i + 2].clone()));
                i += 2;
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
    let mut start = match Start::read(&d, card_path.as_deref(), lot_index) {
        Ok(s) => s,
        Err(e) => {
            println!("{e}");
            return ExitCode::from(1);
        }
    };
    if let Some(p) = &hood_path
        && let Err(e) = start.hood_from(p)
    {
        println!("{e}");
        return ExitCode::from(1);
    }
    if has("REPLAY_SHOW_FAMILIES") {
        // study: the families (house, id) and their members' neighbour ids
        let ngh = hood_path.as_deref().or(card_path.as_deref()).map(|p| housefire::save::memory_card_file(&std::fs::read(p).unwrap_or_default(), "G4ME69").unwrap_or_default());
        for f in ngh.and_then(|n| housefire::save::read_families(&n).ok()).unwrap_or_default() {
            let ids: Vec<i16> = f.members.iter().map(|g| start.neighbours.iter().find(|n| n.guid == *g).map_or(-1, |n| n.id)).collect();
            println!("family house {} id {} neighbours {ids:?}", f.house, f.id);
        }
    }
    let (the_lot, the_globals, neighbours, lot_globals, card_lot) = (start.lot(), start.globals(), &start.neighbours, &start.lot_globals, start.card_lot);
    if has("REPLAY_SHOW_LOT_CLOCKS") {
        // study: each lot's clock (globals 0, 5, 6: the hour, minute and second)
        for (k, g) in lot_globals.iter().enumerate() {
            println!("lot {k}: {:?}, house number (global 10) {:?}", [0, 5, 6].map(|i| g.get(i).copied()), g.get(10));
        }
    }
    let house = start.house.as_ref();
    // The animations in memory at the start: the lot's preload sets, by its house number (the
    // lot's global 10; the original's 0x802E68F8: housepre<n>, primitives.md "loads").
    let house_number = env("REPLAY_HOUSE").and_then(|v| v.parse().ok()).unwrap_or_else(|| the_globals.get(10).map_or(1, |&n| n.max(1) as u32));
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
    if let Some(ticks) = alone_ticks {
        let with = RunWith { card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: (!log.tracks.is_empty()).then(|| housefire::anim::Catalog::read(&d, house_number)) };
        if save_at.is_some() || resume {
            let card = card_path.as_deref().and_then(|p| std::fs::read(p).ok());
            if save_at.is_some() && card.is_none() {
                println!("--save-at needs --card");
                return ExitCode::from(2);
            }
            let lot = (the_lot, house);
            let Some(mut a) = (if resume { Alone::resume(&log, &lib, lot, the_globals, neighbours, with, seed) } else { Alone::new(&log, &lib, lot, the_globals, neighbours, with, seed) }) else { return ExitCode::from(1) };
            for t in 0..=ticks {
                if let (Some((at, out)), Some(card)) = (&save_at, &card)
                    && t == *at
                {
                    save_and_check(card, start.index, card_lot, out, &mut a);
                }
                if t < ticks {
                    a.tick();
                }
            }
            a.report(ticks);
            return ExitCode::SUCCESS;
        }
        let Some(_) = alone(&log, &lib, (the_lot, house), the_globals, neighbours, with, AloneRun { ticks, seed }) else { return ExitCode::from(1) };
        return ExitCode::SUCCESS;
    }
    if driven {
        let with = RunWith { card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: (!log.tracks.is_empty()).then(|| housefire::anim::Catalog::read(&d, house_number)) };
        let Some(r) = drive(&log, &lib, (the_lot, house), the_globals, neighbours, with) else { return ExitCode::from(1) };
        println!("drive: ticks the log's {} of {}", r.same, r.of);
        if let Some(p) = r.parting {
            println!("drive: first parting tick {} object {} tree {} node {}: {}", p.tick, p.object, p.tree, p.node, p.text);
        }
        return ExitCode::SUCCESS;
    }
    if let Some((at, out)) = &save_at {
        let Some(card) = card_path.as_deref().and_then(|p| std::fs::read(p).ok()) else {
            println!("--save-at needs --card");
            return ExitCode::from(2);
        };
        let with = RunWith { card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: None };
        let Some(mut r) = Replay::new(&log, &lib, (the_lot, house), the_globals, neighbours, None, with) else { return ExitCode::from(1) };
        while r.tick() < *at && r.step() {}
        save_and_check(&card, start.index, card_lot, out, &mut r);
        return ExitCode::SUCCESS;
    }
    if !bench {
        let Some((counts, _)) = run(&log, &lib, (the_lot, house), the_globals, neighbours, None, RunWith { card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: (!log.tracks.is_empty()).then(|| housefire::anim::Catalog::read(&d, house_number)) }) else { return ExitCode::from(1) };
        let mut counts = counts;
        if let (Some(events), Some(bank)) = (housefire::sound_events::SoundEvents::open(&d), housefire::sound::SoundBank::open(&d)) {
            counts.map.extend(housefire::replay::sound::figure(&counts.sound_asks, &log, &events, &bank));
        }
        for (k, n) in &counts.map {
            println!("  {k:<66} {n}");
        }
        if free {
            // the tick each kind was first counted in (tools/log_debt.py: how far the run is free)
            for (k, t) in &counts.first {
                println!("first {t}: {k}");
            }
        }
        return ExitCode::SUCCESS;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    let mut runs: Vec<Bench> = Vec::new();
    for _ in 0..5 {
        let Some((_, Some(b))) = run(&log, &lib, (the_lot, house), the_globals, neighbours, Some(Bench::default()), RunWith { card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: None }) else { return ExitCode::from(1) };
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

/// A run whose world can be saved on a card and checked against what the card loads as.
trait Saves {
    fn save(&mut self, card: &[u8], lot: usize) -> Result<Vec<u8>, FormatError>;
    fn reload(&mut self, saved: &[u8], lot: usize, card_lot: bool) -> Result<Vec<String>, FormatError>;
    /// The generator's state (not in the save), if the run keeps it where it is asked.
    fn rng_state(&self) -> Option<u64> {
        None
    }
}

impl Saves for Alone<'_> {
    fn rng_state(&self) -> Option<u64> {
        Some(Alone::rng_state(self))
    }
    fn save(&mut self, card: &[u8], lot: usize) -> Result<Vec<u8>, FormatError> {
        Alone::save(self, card, lot)
    }
    fn reload(&mut self, saved: &[u8], lot: usize, card_lot: bool) -> Result<Vec<String>, FormatError> {
        self.reload_differences(saved, lot, card_lot)
    }
}

impl Saves for Replay<'_> {
    fn save(&mut self, card: &[u8], lot: usize) -> Result<Vec<u8>, FormatError> {
        Replay::save(self, card, lot)
    }
    fn reload(&mut self, saved: &[u8], lot: usize, card_lot: bool) -> Result<Vec<String>, FormatError> {
        self.reload_differences(saved, lot, card_lot)
    }
}

/// Saves the run's world on `card`'s copy as lot `lot`, writes it to `out`, and prints how many
/// fields the world it loads as differs in.
fn save_and_check(card: &[u8], lot: usize, card_lot: bool, out: &str, run: &mut dyn Saves) {
    let saved = match run.save(card, lot) {
        Ok(s) => s,
        Err(e) => {
            println!("save: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(out, &saved) {
        println!("save: cannot write {out}: {e}");
        return;
    }
    match run.reload(&saved, lot, card_lot) {
        Ok(d) => {
            println!("save: written to {out}; the loaded world differs in {} fields", d.len());
            if let Some(r) = run.rng_state() {
                println!("save: the generator's state (not in the save; REPLAY_ALONE_RNG for a resumed run) {r}");
            }
            for line in d.iter().take(20) {
                println!("  {line}");
            }
        }
        Err(e) => println!("save: reload: {e}"),
    }
}
