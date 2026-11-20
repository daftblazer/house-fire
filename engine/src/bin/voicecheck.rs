//! `voicecheck LOG DUMP ISO`: drives the engine's sound player from the observer log's sound
//! calls and compares its mixer voices with the AX dump of the same run, frame by frame.
//!
//! Prints `voices: starts N, paired M, settings the same S of T (voice-frames)`, the first
//! mismatches by word of the game's parameter block, the offsets of the starts, and then
//! `mix: frames N, the same M, first different F` for the engine's voices mixed with the dump's
//! other inputs (the music stream is the dump's own voice: music is not the player's).
//!
//! Timing. The log stamps a call with the game's frame counter; the dump counts AX frames of 5 ms.
//! A start is paired with the dump's start of the same sample (by its length) in the window the
//! call can lead to (the dump's start is up to 300 frames after the call); a paired voice's later
//! calls are timed from its own start by `AX_PER_FRAME` (200 AX frames a second over the game's
//! 29.97 frames), the call's frame being the one before the dump's block shows its first effect.
//! An unpaired play is timed by the median offset of its class.

use housefire::axdump::{Item, next_record, pb, Rec, replay};
use housefire::disc::Disc;
use housefire::mixer::{self, Mixer};
use housefire::sound::SoundBank;
use housefire::sound_player::SoundPlayer;
use housefire::tree_log::{SoundRec, read_log};
use std::collections::{BTreeMap, HashMap};

/// AX frames (5 ms) per game frame: 200 a second over the game's 30000/1001 frames a second.
/// Fits the dump: starts of calls 468 game frames apart are 3,123 AX frames apart (6.6731).
const AX_PER_FRAME: f64 = 200.0 * 1001.0 / 30000.0;
/// A play's start in the dump is no earlier than this many AX frames before the call's time
/// (the UI's plays start 9 to 10 frames before the counter says) and no later than the window.
const EARLIEST: f64 = -30.0;
const LATEST: f64 = 300.0;
/// The frames the mix's difference is measured over (one minute).
const MIX_WINDOW: usize = 12_000;

/// A voice start in the dump.
struct Start {
    frame: usize,
    addr: u32,
    /// The sample's size as the dump shows it: its end address less the first sample's, plus 3.
    nibbles: u32,
    /// The frame of the next start of the same voice (the end of this run), or the dump's end.
    until: usize,
}

type Block = (u32, [u16; pb::WORDS]);

fn cur_of(w: &[u16; pb::WORDS]) -> u32 {
    (w[61] as u32) << 16 | w[62] as u32
}
fn end_of(w: &[u16; pb::WORDS]) -> u32 {
    (w[59] as u32) << 16 | w[60] as u32
}

/// The dump's voice blocks by frame.
fn read_blocks(data: &[u8]) -> Vec<Vec<Block>> {
    let mut frames: Vec<Vec<Block>> = vec![Vec::new()];
    let mut p = 0;
    while let Some(rec) = next_record(data, &mut p) {
        match rec {
            Rec::Voice(a, w) => frames.last_mut().unwrap().push((a, w)),
            Rec::Output(_) => frames.push(Vec::new()),
            _ => {}
        }
    }
    frames.pop();
    frames
}

/// The starts of the dump's sound-effect voices: a running block with no running block of the
/// voice in the frame before, or whose address went back without a loop (a restart). Streams
/// (the music) are left out.
fn find_starts(blocks: &[Vec<Block>]) -> Vec<Start> {
    let mut prev: HashMap<u32, [u16; pb::WORDS]> = HashMap::new();
    let mut starts: Vec<Start> = Vec::new();
    for (f, frame) in blocks.iter().enumerate() {
        let mut now = HashMap::new();
        for (a, w) in frame {
            now.insert(*a, *w);
            if w[8] != 0 || w[7] != 1 {
                continue;
            }
            let fresh = match prev.get(a) {
                None => true,
                Some(p) => p[7] != 1 || (cur_of(w) < cur_of(p) && w[55] == 0),
            };
            if fresh {
                starts.push(Start { frame: f, addr: *a, nibbles: end_of(w) - cur_of(w) + 3, until: usize::MAX });
            }
        }
        prev = now;
    }
    for i in 0..starts.len() {
        let next = starts[i + 1..].iter().find(|s| s.addr == starts[i].addr).map(|s| s.frame);
        starts[i].until = next.unwrap_or(usize::MAX);
    }
    starts
}

/// A play of the log.
struct Play {
    /// Index in the log's sound calls.
    rec: usize,
    frame: u32,
    handle: u32,
    nibbles: u32,
    /// Handle acquired with kind 0 (an object's sound) rather than the UI's.
    object: bool,
    paired: Option<usize>,
    /// The start frame in AX frames, found or predicted.
    start: usize,
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// What a call does to the player, and when.
struct Event {
    at: usize,
    rec: usize,
}

struct Schedule {
    plays: Vec<Play>,
    events: Vec<Event>,
}

fn build_schedule(log: &[SoundRec], bank: &SoundBank, starts: &[Start]) -> Schedule {
    // Handles' kinds from the acquire calls (a = kind, b = handle).
    let kinds: HashMap<u32, u32> = log.iter().filter(|s| s.call == 0).map(|s| (s.b, s.a)).collect();
    let mut plays = Vec::new();
    for (i, s) in log.iter().enumerate() {
        if s.call != 1 {
            continue;
        }
        let nibbles = bank.find_id(s.a).and_then(|k| bank.sound(k)).map(|k| k.nibbles.next_multiple_of(2)).unwrap_or(0);
        plays.push(Play {
            rec: i,
            frame: s.frame,
            handle: s.handle,
            nibbles,
            object: kinds.get(&s.handle) == Some(&0),
            paired: None,
            start: 0,
        });
    }
    // Pair each play with the earliest unused start of a sample of its size in its window.
    let mut used = vec![false; starts.len()];
    for p in plays.iter_mut() {
        let t = p.frame as f64 * AX_PER_FRAME;
        let best = (0..starts.len())
            .filter(|&k| !used[k] && starts[k].nibbles == p.nibbles)
            .filter(|&k| (starts[k].frame as f64 - t) >= EARLIEST && (starts[k].frame as f64 - t) <= LATEST)
            .min_by_key(|&k| starts[k].frame);
        if let Some(k) = best {
            used[k] = true;
            p.paired = Some(k);
            p.start = starts[k].frame;
        }
    }
    // A play on a handle that another play of the same game frame has just started restarts the
    // voice in place: one start in the dump for both.
    for i in 0..plays.len() {
        if plays[i].paired.is_none()
            && let Some(j) = (0..i).find(|&j| plays[j].handle == plays[i].handle && plays[j].frame == plays[i].frame && plays[j].paired.is_some())
        {
            plays[i].paired = plays[j].paired;
            plays[i].start = plays[j].start;
        }
    }
    // The class offsets, for the plays that found no start.
    let mut off = [Vec::new(), Vec::new()];
    for p in &plays {
        if p.paired.is_some() {
            off[p.object as usize].push(p.start as f64 - p.frame as f64 * AX_PER_FRAME);
        }
    }
    let med = [median(&mut off[0]), median(&mut off[1])];
    for p in plays.iter_mut().filter(|p| p.paired.is_none()) {
        p.start = (p.frame as f64 * AX_PER_FRAME + med[p.object as usize]).round().max(0.0) as usize;
    }
    // Each call's time: a play is its start, a later call of its handle is timed from it, an
    // acquire goes with the handle's next play.
    let mut events = Vec::new();
    let mut run: HashMap<u32, (usize, u32)> = HashMap::new();
    let mut next_play = 0usize;
    for (i, s) in log.iter().enumerate() {
        let at = match s.call {
            0 => plays[next_play..].iter().find(|p| p.handle == s.b).map(|p| p.start).unwrap_or(0),
            1 => {
                let p = &plays[next_play];
                next_play += 1;
                run.insert(s.handle, (p.start, p.frame));
                p.start
            }
            _ => match run.get(&s.handle) {
                Some(&(s0, f0)) => s0 + ((s.frame.saturating_sub(f0)) as f64 * AX_PER_FRAME) as usize,
                None => continue,
            },
        };
        events.push(Event { at, rec: i });
    }
    // Within a frame the UI's calls (a handle acquired with a kind other than 0) come after the
    // objects': the dump gives the UI sound of frame 12223 the voice an object's stop freed in
    // that frame, and the object's play a new one, against the log's order.
    let ui = |h: u32| kinds.get(&h).is_some_and(|&k| k != 0);
    events.sort_by_key(|e| (e.at, log[e.rec].call != 0 && ui(log[e.rec].handle)));
    Schedule { plays, events }
}

/// Feeds a frame's calls to the player.
fn deliver(player: &mut SoundPlayer, log: &[SoundRec], ev: &[Event], loops: &HashMap<usize, bool>, next: &mut usize, frame: usize, on_play: &mut impl FnMut(Option<u32>, usize)) {
    while *next < ev.len() && ev[*next].at <= frame {
        let r = &log[ev[*next].rec];
        match r.call {
            0 => {} // an acquire reserves no voice: the handle gets one at its first play
            1 => {
                let id = player.play(r.handle, r.a, loops.get(&ev[*next].rec).copied().unwrap_or(false));
                on_play(id, ev[*next].rec);
            }
            2 => player.stop(r.handle),
            _ => player.params(r.handle, &r.params),
        }
        *next += 1;
    }
}

/// The differences between a player voice and a dump block: (word of the block, engine, dump).
fn differences(v: &mixer::Voice, base: u32, age: u32, w: &[u16; pb::WORDS], dump_base: u32) -> Vec<(usize, u32, u32)> {
    let mut d = Vec::new();
    let mut chk = |word: usize, mine: u32, theirs: u32| {
        if mine != theirs {
            d.push((word, mine, theirs));
        }
    };
    chk(6, v.mixer_control as u32, w[6] as u32);
    chk(50, v.vol as u32, w[50] as u32);
    chk(51, v.vol_delta as u32, w[51] as u32);
    for k in 0..mixer::SENDS {
        chk(9 + 2 * k, v.send_vol[k] as u32, w[9 + 2 * k] as u32);
        chk(10 + 2 * k, v.send_delta[k] as u32, w[10 + 2 * k] as u32);
    }
    chk(83, v.ratio, (w[83] as u32) << 16 | w[84] as u32);
    chk(55, v.looping as u32, w[55] as u32);
    if v.looping && w[55] != 0 {
        chk(58, v.loop_addr - base, ((w[57] as u32) << 16 | w[58] as u32).wrapping_sub(dump_base));
    }
    if age == 0 {
        chk(62, v.cur - base, cur_of(w).wrapping_sub(dump_base));
    }
    d
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: voicecheck LOG DUMP ISO");
        std::process::exit(2);
    }
    let log = read_log(&args[1]).expect("read the log");
    let data = std::fs::read(&args[2]).expect("read the dump");
    let disc = Disc::open(&args[3]).expect("open the disc");
    let bank = SoundBank::open(&disc).expect("the sound archives");
    let blocks = read_blocks(&data);
    let starts = find_starts(&blocks);
    let sched = build_schedule(&log.sounds, &bank, &starts);
    let paired = sched.plays.iter().filter_map(|p| p.paired).collect::<std::collections::BTreeSet<_>>().len();
    // Whether a play loops is not in the log's calls (the game's sound layer decides from the
    // object's script); it is taken from the dump's voice, the one setting not worked out.
    let mut loops: HashMap<usize, bool> = HashMap::new();
    for p in &sched.plays {
        if let Some(k) = p.paired
            && let Some((_, w)) = blocks[starts[k].frame].iter().find(|(a, _)| *a == starts[k].addr)
        {
            loops.insert(p.rec, w[55] != 0);
        }
    }

    // Phase 1: the settings, voice by voice and frame by frame.
    let mut player = SoundPlayer::new(&bank);
    let mut next = 0usize;
    let play_of_rec: HashMap<usize, usize> = sched.plays.iter().enumerate().map(|(k, p)| (p.rec, k)).collect();
    let mut play_of_id: HashMap<u32, usize> = HashMap::new();
    let mut scratch = Mixer::new();
    let (mut compared, mut same, mut dump_frames) = (0usize, 0usize, 0usize);
    let mut by_word: BTreeMap<usize, (usize, usize, usize, u32, u32, u32)> = BTreeMap::new();
    for (f, frame) in blocks.iter().enumerate() {
        if f > 0 {
            player.advance();
        }
        deliver(&mut player, &log.sounds, &sched.events, &loops, &mut next, f, &mut |id, rec| {
            if let (Some(id), Some(&k)) = (id, play_of_rec.get(&rec)) {
                play_of_id.insert(id, k);
            }
        });
        dump_frames += frame.iter().filter(|(_, w)| w[8] == 0 && w[7] == 1).count();
        for p in player.playing() {
            let Some(&k) = play_of_id.get(&p.play_id) else { continue };
            let Some(si) = sched.plays[k].paired else { continue };
            let st = &starts[si];
            if f < st.frame || f >= st.until {
                continue;
            }
            let Some((_, w)) = frame.iter().find(|(a, w)| *a == st.addr && w[7] == 1) else { continue };
            // The dump's first nibble of the sample: its end less the sample's nibbles.
            let dump_base = end_of(w).wrapping_sub(st.nibbles - 1);
            let d = differences(p.voice, p.base, p.age, w, dump_base);
            compared += 1;
            if d.is_empty() {
                same += 1;
            }
            for (word, mine, theirs) in d {
                if std::env::var_os("VOICECHECK_DIFFS").is_some() {
                    println!("diff frame {f} play {k} age {} w{word} {mine:#x} {theirs:#x}", p.age);
                }
                let e = by_word.entry(word).or_insert((0, f, k, mine, theirs, 0));
                e.0 += 1;
            }
        }
        player.mix(&mut scratch);
    }
    println!(
        "voices: starts {}, paired {}, settings the same {} of {} (voice-frames)",
        starts.len(),
        paired,
        same,
        compared
    );
    println!("  plays in the log {}, of the dump's voice-frames {} compared {}", sched.plays.len(), dump_frames, compared);
    let mut words: Vec<_> = by_word.iter().collect();
    words.sort_by_key(|(_, e)| std::cmp::Reverse(e.0));
    println!("  differences by word of the block (count, first: frame, play, engine, dump):");
    for (w, e) in words.iter().take(14) {
        println!("    w{w}: {} first at frame {} play {} {:#x} vs {:#x}", e.0, e.1, e.2, e.3, e.4);
    }
    // The start offsets (the start's AX frame less the call's frame counter in AX frames): two
    // groups, the sounds that start at once and those that start about 1.2 s later.
    for late in [false, true] {
        let mut o: Vec<f64> = sched
            .plays
            .iter()
            .filter(|p| p.paired.is_some())
            .map(|p| p.start as f64 - p.frame as f64 * AX_PER_FRAME)
            .filter(|&d| (d > 100.0) == late)
            .collect();
        if !o.is_empty() {
            let (lo, hi) = (o.iter().cloned().fold(f64::MAX, f64::min), o.iter().cloned().fold(f64::MIN, f64::max));
            let m = median(&mut o);
            println!(
                "  start offsets, {} plays {}: median {m:.1} AX frames, min {lo:.1}, max {hi:.1}",
                o.len(),
                if late { "starting late" } else { "starting at once" },
            );
        }
    }
    let unpaired_starts = starts.len() - paired;
    println!("  plays without a start in the dump {}, dump starts without a play {unpaired_starts}", sched.plays.iter().filter(|p| p.paired.is_none()).count());

    // Phase 2: the engine's voices in the mix.
    let mut player = SoundPlayer::new(&bank);
    let mut next = 0usize;
    let mut stepped = 0usize;
    let (mut frames, mut mix_same) = (0usize, 0usize);
    let mut first_diff: Option<usize> = None;
    let (mut peak, mut sumsq, mut count) = (0i32, 0f64, 0usize);
    let log_sounds = &log.sounds;
    replay(
        &data,
        |frame, mixer, aram, items| {
            while stepped <= frame {
                if stepped > 0 {
                    player.advance();
                }
                deliver(&mut player, log_sounds, &sched.events, &loops, &mut next, stepped, &mut |_, _| {});
                stepped += 1;
            }
            player.mix(mixer);
            // The dump's own streams (the music), from its sample memory.
            for it in items {
                if let Item::Voice(_, w) = it
                    && w[8] != 0
                {
                    let mut v = pb::voice(w);
                    for ms in 0..5 {
                        mixer.mix_voice_ms(&mut v, ms, aram);
                    }
                }
            }
        },
        |frame, out| {
            if out.lr == out.want_lr && out.surround == out.want_surround {
                mix_same += 1;
            } else {
                first_diff.get_or_insert(frame);
            }
            frames += 1;
            if frame < MIX_WINDOW {
                for (&m, &w) in out.lr.iter().zip(&out.want_lr) {
                    let d = m as i32 - w as i32;
                    peak = peak.max(d.abs());
                    sumsq += (d as f64) * (d as f64);
                    count += 1;
                }
            }
        },
    );
    match first_diff {
        Some(f) => println!("mix: frames {frames}, the same {mix_same}, first different {f}"),
        None => println!("mix: frames {frames}, the same {mix_same}, first different none"),
    }
    println!(
        "  difference of left and right over the first {MIX_WINDOW} frames: peak {peak}, rms {:.2}",
        (sumsq / count.max(1) as f64).sqrt()
    );
}
