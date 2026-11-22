//! The simulation's side of the sound in a run: the PlaySound and KillSounds requests of the
//! scripts (resolved to events by `replay::sound::ask`), the interface's sounds and the music
//! become [`Command`]s for the mixer side ([`SoundRun`](crate::sound_run::SoundRun)), and each
//! frame the params of the sounds heard from a place follow the camera (`sound_params`).
//!
//! Which sample of an event's list plays is drawn from the presentation's own random numbers
//! (`docs/plan.md`, "A sound's variant"), so the sounds never move the simulation's draws.
//!
//! Rules taken from the original's log (`docs/simulation.md`, "Sound starts"), each a comment
//! below: a handle (the emitter's definition and the FWAV id) plays only once while it sounds,
//! except `counter_chop`'s, which every post plays; a voice event plays with a chance of 500 in
//! 10,000. Left out on purpose: the original starts an object's sound about 1.15 s after the
//! request (a latency of its sound loading, `docs/assets.md`, "The voices"); here it starts at
//! once.

use crate::FileId;
use crate::replay::sound::{SoundAsk, SoundRequest};
use crate::scene::Camera;
use crate::sound::SoundBank;
use crate::sound_events::SoundEvents;
use crate::sound_params::{Listener, positional, range_of, words};
use crate::sound_run::Command;
use crate::world::{Rng, World};

/// Seconds in a game frame (30000/1001 a second).
const FRAME_SECONDS: f64 = 1001.0 / 30000.0;
/// The chance, in 10,000, that a voice event plays: `counter_voxchop`'s 500 in its track's script
/// (and about one in twenty of the other voice events in the log).
const VOICE_CHANCE: u32 = 500;
/// The handles of the interface's sounds start here (one handle a sound, restarted in place as
/// the original's `UI_accept` and `UI_move` are).
const UI_HANDLES: u32 = 0x7000_0000;
/// The handles of the world's sounds start here.
const WORLD_HANDLES: u32 = 0x8000_0000;
/// A sound's tail, seconds, kept after its sample's length before its handle is let go.
const TAIL: f64 = 0.1;

/// A sound the front keeps while it plays.
struct Live {
    handle: u32,
    /// The handle's key: the emitter's definition and the FWAV id.
    key: Option<(Option<FileId>, i16)>,
    /// The object whose script asked (KillSounds' object).
    asker: i16,
    emitter: i16,
    /// Where it was heard from at the request, if it is heard from a place.
    place: Option<[f32; 3]>,
    range: f32,
    /// The seconds on the front's clock at which its sample has played out (infinite for a loop).
    ends: f64,
    /// The last params sent, as bits.
    sent: [u32; 2],
}

/// A sound started, for the logs of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub tick: u32,
    pub event: String,
    pub sample: String,
}

pub struct SoundFront<'b> {
    bank: &'b SoundBank,
    events: &'b SoundEvents,
    rng: Rng,
    listener: Option<Listener>,
    live: Vec<Live>,
    next_handle: u32,
    now: f64,
    out: Vec<Command>,
    /// The sounds started so far, in order.
    pub started: Vec<Started>,
}

/// Whether a sample loops (the object's script says so in the original and the calls do not
/// show it; the names do: `FRIDGE_CHEAP_LOOP`, `STOVE_BUBBLE`).
fn loops(sample: &str) -> bool {
    let s = sample.to_ascii_lowercase();
    s.contains("loop") || s.contains("bubble")
}

impl<'b> SoundFront<'b> {
    /// A front over the bank and the events; `seed` seeds the presentation's draws.
    pub fn new(bank: &'b SoundBank, events: &'b SoundEvents, seed: u64) -> SoundFront<'b> {
        SoundFront { bank, events, rng: Rng { state: seed | 1 }, listener: None, live: Vec::new(), next_handle: WORLD_HANDLES, now: 0.0, out: Vec::new(), started: Vec::new() }
    }

    /// The camera the sounds are heard through.
    pub fn set_camera(&mut self, cam: &Camera) {
        self.listener = Some(Listener::new(cam));
    }

    /// The commands made since the last call.
    pub fn take(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.out)
    }

    fn sample_name(&self, id: u32) -> String {
        self.bank.find_id(id).map(|i| self.bank.entries()[i].entry.name.clone()).unwrap_or_default()
    }

    /// The seconds a sample plays.
    fn seconds(&self, id: u32) -> f64 {
        self.bank.find_id(id).and_then(|i| self.bank.sound(i)).map_or(0.0, |s| s.samples as f64 / s.rate.max(1) as f64)
    }

    /// A request of the simulation, made at `tick`.
    pub fn request(&mut self, tick: u32, r: &SoundRequest) {
        match r {
            SoundRequest::Play(ask) => self.play_ask(tick, ask),
            SoundRequest::Kill { object } => {
                let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.live).into_iter().partition(|l| l.asker == *object);
                self.live = kept;
                self.out.extend(gone.iter().map(|l| Command::Stop { handle: l.handle }));
            }
        }
    }

    fn play_ask(&mut self, tick: u32, ask: &SoundAsk) {
        let Some(name) = &ask.event else { return };
        let Some(ev) = self.events.event(name) else { return };
        let samples = ev.choice.samples(ask.voice);
        if samples.is_empty() {
            return;
        }
        let key = (ask.emitter_file, i16::from_le_bytes([ask.operand[0], ask.operand[1]]));
        // A handle that is sounding is not started again (the log: of 63 re-posts none played,
        // but `counter_chop`'s, which every post plays).
        if self.live.iter().any(|l| l.key == Some(key) && l.ends > self.now) && !name.to_ascii_lowercase().starts_with("counter_chop") {
            return;
        }
        if ev.choice.by_voice() && self.rng.below(10_000) >= VOICE_CHANCE {
            return;
        }
        let sample = samples[self.rng.below(samples.len() as u32) as usize];
        let sample_name = self.sample_name(sample);
        let looping = loops(&sample_name);
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1).max(WORLD_HANDLES);
        let flat = !positional(ev.track_words[0]);
        let place = ask.place.map(|(x, y, _)| [x as f32 / 16.0, y as f32 / 16.0, 0.0]);
        let range = range_of(ev.track_words[1]);
        let (x, v) = match (flat, &self.listener, place) {
            (false, Some(l), Some(p)) => l.target(p, range),
            (true, ..) | (false, None, _) | (false, _, None) => (1.0, 1.0),
        };
        self.out.push(Command::Play { handle, sample, looping });
        self.out.push(Command::Params { handle, words: words(x, v, true, false) });
        self.live.push(Live {
            handle,
            key: Some(key),
            asker: ask.object,
            emitter: ask.emitter,
            place: (!flat).then_some(place).flatten(),
            range,
            ends: if looping { f64::INFINITY } else { self.now + self.seconds(sample) + TAIL },
            sent: [x.to_bits(), v.to_bits()],
        });
        self.started.push(Started { tick, event: name.clone(), sample: sample_name });
    }

    /// An interface sound by its name (`UI_accept`, `UI_move`): at full volume from nowhere.
    pub fn ui(&mut self, tick: u32, name: &str) {
        let Some(i) = self.bank.find(name) else { return };
        let id = self.bank.entries()[i].entry.id;
        let handle = UI_HANDLES + (id & 0xFFFF);
        self.out.push(Command::Play { handle, sample: id, looping: false });
        self.out.push(Command::Params { handle, words: words(1.0, 1.0, true, true) });
        self.started.push(Started { tick, event: name.to_ascii_lowercase(), sample: self.sample_name(id) });
    }

    /// The options' volumes (0 to 10 each).
    pub fn volumes(&mut self, sound: u8, music: u8) {
        self.out.push(Command::Volumes { sound, music });
    }

    /// Music: the stream called `name` (`loadloop`, `sbo_pop1`), or none.
    pub fn music(&mut self, name: Option<&str>) {
        let index = name.and_then(|n| self.bank.find(n)).map(|i| i as u32);
        self.out.push(Command::Music(index));
    }

    /// A game frame passes: sounds that have played out let go, and each sound heard from a place
    /// takes the params its emitter's place gives against the camera.
    pub fn frame(&mut self, world: &World) {
        self.now += FRAME_SECONDS;
        let now = self.now;
        let listener = self.listener;
        for l in &mut self.live {
            if l.ends <= now {
                continue;
            }
            let (Some(listener), Some(_)) = (listener, l.place) else { continue };
            // the emitter's place now (a Sim moves), or where it was
            let at = world.find_object(l.emitter).filter(|o| o.on_lot()).map_or(l.place, |o| Some([o.x16 as f32 / 16.0, o.y16 as f32 / 16.0, 0.0]));
            let Some(at) = at else { continue };
            let (x, v) = listener.target(at, l.range);
            let bits = [x.to_bits(), v.to_bits()];
            if bits != l.sent {
                l.sent = bits;
                self.out.push(Command::Params { handle: l.handle, words: words(x, v, false, false) });
            }
        }
        let (done, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.live).into_iter().partition(|l| l.ends <= now);
        self.live = kept;
        self.out.extend(done.iter().map(|l| Command::Stop { handle: l.handle }));
    }
}
