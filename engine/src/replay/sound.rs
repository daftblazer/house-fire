//! The sounds: which sound each PlaySound request starts, worked out natively, against the log's
//! plays (the observer's '/' records, `Snd_Play`) and posts ('.' records). Kind (b): it reads the
//! log to compare; the native part is `ask` (the event a request names) and `Choice`
//! (`sound_events`, the samples it may play).
//!
//! The engine's own part is a request's event: PlaySound's operand names a FWAV chunk by its id
//! (the first short), the chunk of the object's file (then the file the script runs from, their
//! semi-global files, and Global) holds the event's name, and the `snd::` tables give the
//! samples (`sound_events`). Which sample of the list plays is the presentation's own draw
//! (`docs/plan.md`, "A sound's variant"), so a play matches when its sample is among the
//! candidates.

use std::collections::{BTreeMap, BTreeSet};

use crate::FileId;
use crate::library::Library;
use crate::sound::SoundBank;
use crate::sound_events::{Choice, SoundEvents, Voice};
use crate::tree_log::{Log, SoundRec};
use crate::world::World;

/// The caller of `Snd_Play` for a sound an event request started (the sound objects' request
/// update), and for the interface's own (`fn_8006186C`).
const EVENT_PLAY: u32 = 0x8025_7504;

/// Person data 65 is the Sim's gender, 1 female (`docs/simulation.md`).
const PERSON_GENDER: usize = 65;

/// One PlaySound request the simulation made.
#[derive(Clone, Debug)]
pub struct SoundAsk {
    pub tick: u32,
    pub object: i16,
    /// The object the sound is posted for (the post's object): the stack object if operand
    /// byte 4 has bit 1, else the asking object (checked: all 147 requests of the home log).
    pub emitter: i16,
    /// The emitter's file: its definition's sound table is what the original keys a sound's
    /// handle by (the lamps of one kind share it: the log's posts at tick 1 have one handle).
    pub emitter_file: Option<FileId>,
    pub operand: [u8; 8],
    /// The event the FWAV chunk names (`None`: no chunk of that id in the files tried).
    pub event: Option<String>,
    /// The asking object's voice (a Sim's gender; `None` for an object that is not a person).
    pub voice: Voice,
    /// The asking Sim's person data (empty for an object that is no person).
    pub person_data: Vec<i16>,
    /// The emitter's place at the request (sixteenths of a tile, level), if it is on the lot.
    pub place: Option<(i32, i32, i32)>,
}

/// What a run's scripts ask of the sound: a sound (`ask`'s resolved request) or the end of an
/// object's sounds (KillSounds, opcode 48).
#[derive(Clone, Debug)]
pub enum SoundRequest {
    Play(SoundAsk),
    Kill { object: i16 },
}

/// The name in the FWAV chunk `id` of one file.
fn fwav_in(lib: &Library, file: FileId, id: i16) -> Option<String> {
    let f = lib.file(file);
    let c = f.iff.find(b"FWAV", id)?;
    let b = f.iff.data(c);
    let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
    Some(crate::iff::latin1(b).trim().to_string())
}

/// A PlaySound request: the object `me` asking about `stack_object` from a script of `from` (the
/// file of the tree running, which may be a semi-global file), with `operand`, at `tick`.
pub fn ask(lib: &Library, world: &World, from: Option<FileId>, me: i16, stack_object: i16, operand: [u8; 8], tick: u32) -> SoundAsk {
    let obj = world.find_object(me);
    let id = i16::from_le_bytes([operand[0], operand[1]]);
    let emitter = if operand[4] & 2 != 0 { stack_object } else { me };
    let emitter_file = world.find_object(emitter).and_then(|o| o.file);
    let mut tries: Vec<FileId> = Vec::new();
    for f in [emitter_file, from, obj.and_then(|o| o.file)].into_iter().flatten() {
        tries.push(f);
        tries.extend(lib.file(f).semi);
    }
    tries.extend(lib.global());
    let event = tries.into_iter().find_map(|f| fwav_in(lib, f, id));
    let place = world.find_object(emitter).filter(|o| o.on_lot()).map(|o| (o.x16, o.y16, o.level));
    let voice = match obj {
        Some(o) if o.person => Voice { female: o.person_data.get(PERSON_GENDER).map(|&g| g == 1), index: None },
        _ => Voice::default(),
    };

    SoundAsk { tick, object: me, emitter, emitter_file, operand, event, voice, person_data: obj.filter(|o| o.person).map(|o| o.person_data.clone()).unwrap_or_default(), place }
}


/// A request that starts a sound, as the figure holds it.
struct Start<'a> {
    ask: &'a SoundAsk,
    /// The samples the play may be.
    samples: Vec<u32>,
    /// Whether the voice matters and is not known (the lists of every voice are the candidates).
    voice_unknown: bool,
    /// The frame of the log's post for this request, if it has one (the original's sound update
    /// runs once a frame, so the play is in that frame or the next).
    frame: Option<u32>,
    used: bool,
}

/// The names of a sample, lower case (for the studies).
fn sample_name(bank: &SoundBank, id: u32) -> String {
    bank.find_id(id).map(|i| bank.entries()[i].entry.name.to_ascii_lowercase()).unwrap_or_default()
}

/// The figure: the log's event plays against the native starts, as lines for the replay's
/// summary (`docs/simulation.md`, "Sound starts"). A log play matches a start whose tick is the
/// play's or the one before, or whose post's frame is the play's or the one before (several
/// ticks run in a frame, so the play can be a few ticks after its post), and whose samples
/// hold the played one; each start matches at most one play.
///
/// What the figure does not model is counted and named, not hidden: the sound layer's own
/// decisions in the original (a track's script draws a chance for the voices, a handle already
/// started is not started again for most events) are the misses and extras that remain.
pub fn figure(asks: &[SoundAsk], log: &Log, events: &SoundEvents, bank: &SoundBank) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |k: &str, n: usize| *out.entry(k.to_string()).or_default() += n;
    let show = super::study::has("REPLAY_SHOW_SOUNDS");

    // The log's post of each request: the same tick, the object the sound is posted for, in
    // order. (The posts at tick 0 are the lot's own at its load.)
    let mut post_used = vec![false; log.sound_posts.len()];
    let mut frames: Vec<Option<u32>> = Vec::with_capacity(asks.len());
    for a in asks {
        let k = (0..log.sound_posts.len()).find(|&k| {
            let p = &log.sound_posts[k];
            !post_used[k] && p.post == 1 && p.tick == a.tick && p.kind == a.emitter as u16 as u32
        });
        if let Some(k) = k {
            post_used[k] = true;
        }
        frames.push(k.map(|k| log.sound_posts[k].frame));
    }
    let posts = log.sound_posts.iter().filter(|p| p.post == 1 && p.tick > 0).count();
    let paired = frames.iter().flatten().count();
    add("sounds: the log's PlaySound posts after the load", posts);
    add("sounds: ... asked natively at the post's tick, object and place in the tick", paired.min(posts));

    // The requests that start a sound: the event is a row of the tables and has samples.
    let mut starts: Vec<Start> = Vec::new();
    for (a, &frame) in asks.iter().zip(&frames) {
        let Some(name) = &a.event else {
            add("sounds: request with no FWAV chunk of its id", 1);
            continue;
        };
        let Some(ev) = events.event(name) else {
            add("sounds: request whose event is no row of the tables", 1);
            continue;
        };
        let samples = ev.choice.samples(a.voice);
        if samples.is_empty() {
            add("sounds: request whose event has no samples (a command)", 1);
            continue;
        }
        let voice_unknown = matches!(ev.choice, Choice::ByVoice(_)) && a.voice.index.is_none();
        if show {
            println!("sound start: tick {} object {} emitter {} event {name} frame {frame:?}", a.tick, a.object, a.emitter);
        }
        starts.push(Start { ask: a, samples, voice_unknown, frame, used: false });
    }
    add("sounds: starts whose voice list is not known (every voice's samples taken)", starts.iter().filter(|s| s.voice_unknown).count());

    let plays: Vec<(usize, &SoundRec)> = log.sounds.iter().enumerate().filter(|(_, s)| s.call == 1 && s.caller == EVENT_PLAY).collect();
    let mut matched_asks: Vec<(usize, &SoundAsk)> = Vec::new();
    add("sounds: the log's event plays", plays.len());
    let mut matched = 0;
    for &(pi, p) in &plays {
        let fits = |s: &Start| {
            let tick_near = s.ask.tick == p.tick || s.ask.tick + 1 == p.tick;
            let frame_near = s.frame.is_some_and(|f| p.frame == f || p.frame == f + 1);
            !s.used && (tick_near || frame_near) && s.samples.contains(&p.a)
        };
        // the nearest in time first
        let hit = starts.iter().enumerate().filter(|(_, s)| fits(s)).min_by_key(|(_, s)| p.tick.abs_diff(s.ask.tick)).map(|(i, _)| i);
        if let Some(i) = hit {
            starts[i].used = true;
            matched_asks.push((pi, starts[i].ask));
            matched += 1;
            continue;
        }
        let name = sample_name(bank, p.a);
        let kind = if name.starts_with("footstep") {
            "sounds: miss, a footstep (the walk's steps are not made natively)"
        } else if starts.iter().any(|s| !s.used && (s.ask.tick == p.tick || s.ask.tick + 1 == p.tick)) {
            "sounds: miss, a start at the tick but other samples"
        } else if name.starts_with("dog") {
            "sounds: miss, the dog's (no request of the simulation)"
        } else {
            "sounds: miss, no start at the tick (the original played it later)"
        };
        add(kind, 1);
        if show {
            println!("sound miss: tick {} frame {} {name}", p.tick, p.frame);
        }
    }
    add("sounds: started natively (the event and tick as the log's)", matched);
    params_figure(log, events, &matched_asks, &mut add);

    // The starts the log has no play for, by the likely reason.
    let handle_of = |a: &SoundAsk| (a.emitter_file, i16::from_le_bytes([a.operand[0], a.operand[1]]));
    let mut handles: BTreeSet<(Option<FileId>, i16)> = BTreeSet::new();
    for s in &starts {
        if s.used {
            handles.insert(handle_of(s.ask));
        }
    }
    for s in starts.iter().filter(|s| !s.used) {
        let handle = handle_of(s.ask);
        let by_voice = s.ask.event.as_ref().and_then(|n| events.event(n)).is_some_and(|e| e.choice.by_voice());
        let kind = if by_voice {
            "sounds: start with no play, a voice event (its track script draws a chance)"
        } else if handles.contains(&handle) {
            "sounds: start with no play, a handle that already played (not started again)"
        } else {
            "sounds: start with no play, other"
        };
        add(kind, 1);
        if show {
            println!("sound extra: tick {} object {} event {:?} operand {:?}", s.ask.tick, s.ask.object, s.ask.event, s.ask.operand);
        }
    }
    out
}


/// The caller of `Snd_Play` for the interface's own sounds (`fn_8006186C`): not in the world.
const UI_PLAY: u32 = 0x8006_1940;

/// How far a logged `x` or `volume` may be from the native one to count as within tolerance
/// (five thousandths: a few of the log's own steps of 1/1024).
const PARAM_TOLERANCE: f32 = 0.005;

/// The figure of the params: each of the log's params calls ('/' call 3) of a play, against what
/// `sound_params` gives for the emitter's place and the camera of the call's tick. A call is
/// exact when both floats are the log's to the bit, within tolerance when each is within
/// `PARAM_TOLERANCE`. The calls of a sound that moves are counted by kind, not hidden: the ones
/// of a sound whose emitter is not known natively (the footsteps, the dog) are named as such.
fn params_figure(log: &Log, events: &SoundEvents, matched: &[(usize, &SoundAsk)], add: &mut impl FnMut(&str, usize)) {
    use crate::sound_params::{Listener, positional, range_of};
    let total = log.sounds.iter().filter(|r| r.call == 3).count();
    add("sound params: the log's params calls", total);
    let show = super::study::has("REPLAY_SHOW_PARAMS");
    if show {
        for n in ["footstep_soft", "footstep_medium", "footstep_hard", "footstep_plant", "dognit", "dog_bark"] {
            if let Some(e) = events.event(n) {
                println!("params event {n}: track words {:08X} {:08X} kind {}", e.track_words[0], e.track_words[1], e.kind);
            }
        }
        let mut seen = BTreeSet::new();
        for (_, a) in matched {
            if let Some(n) = a.event.as_ref().filter(|n| seen.insert((*n).clone())) && let Some(e) = events.event(n) {
                println!("params event {n}: track words {:08X} {:08X} kind {}", e.track_words[0], e.track_words[1], e.kind);
            }
        }
    }
    for (k, play) in log.sounds.iter().enumerate().filter(|(_, r)| r.call == 1) {
        let ask = matched.iter().find(|(i, _)| *i == k).map(|(_, a)| *a);
        let flat = play.caller == UI_PLAY || ask.and_then(|a| a.event.as_ref()).and_then(|n| events.event(n)).is_some_and(|e| !positional(e.track_words[0]));
        let known = flat || ask.is_some_and(|a| a.place.is_some());
        let calls = log.sounds[k + 1..].iter().take_while(|r| !(r.handle == play.handle && r.call != 3)).filter(|r| r.call == 3 && r.handle == play.handle);
        for r in calls {
            let (lx, lv) = (f32::from_bits(r.params[1]), f32::from_bits(r.params[2]));
            let kind = if lv == 0.0 && lx.abs() < 1e-30 {
                "stop"
            } else if r.params[0] & 8 != 0 {
                "first"
            } else {
                "later"
            };
            if !known {
                add("sound params: calls of a sound whose emitter is not known natively (footsteps, the dog)", 1);
                continue;
            }
            let (nx, nv) = if kind == "stop" {
                (0.0, 0.0)
            } else if flat {
                (1.0, 1.0)
            } else {
                let Some(c) = log.cams.iter().rfind(|c| c.tick <= r.tick) else { continue };
                let l = Listener::new(&crate::scene::Camera { target: [c.live[0], c.live[1], c.live[2]], distance: c.live[3], rotation: c.live[4], tilt: c.live[5] });
                let (px, py) = ask.and_then(|a| a.place).map(|(x, y, _)| (x as f32 / 16.0, y as f32 / 16.0)).unwrap_or_default();
                let range = ask.and_then(|a| a.event.as_ref()).and_then(|n| events.event(n)).map_or(crate::sound_params::RANGE, |e| range_of(e.track_words[1]));
                l.target([px, py, 0.0], range)
            };
            let exact = nx.to_bits() == lx.to_bits() && nv.to_bits() == lv.to_bits() || (kind == "stop" && nv == 0.0 && lv == 0.0 && lx.abs() < 1e-30);
            let near = (nx - lx).abs() <= PARAM_TOLERANCE && (nv - lv).abs() <= PARAM_TOLERANCE;
            add("sound params: calls of a known emitter", 1);
            add(&format!("sound params: ... {kind} calls"), 1);
            if exact {
                add("sound params: ... the native rule's, exactly", 1);
            } else if near {
                add("sound params: ... the native rule's, within 0.005", 1);
            } else {
                add(&format!("sound params: ... {kind} calls the rule misses"), 1);
                if show {
                    println!("params miss: tick {} {kind} log ({lx}, {lv}) native ({nx}, {nv}) flat {flat} event {:?} place {:?} cam {:?}", r.tick, ask.and_then(|a| a.event.as_ref()), ask.and_then(|a| a.place), log.cams.iter().rfind(|c| c.tick <= r.tick).map(|c| c.live));
                }
            }
        }
    }
}
