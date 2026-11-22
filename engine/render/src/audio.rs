//! The sound out of the play window: the engine's mixer (`housefire::sound_run`) on its own
//! thread, its frames (5 ms, 32 kHz, stereo) handed to the sound device through `cpal` (ALSA,
//! which PipeWire and Pulse both serve), or written to a WAV file in a headless run.
//!
//! The path of a sound: the simulation thread makes [`Command`]s (`SoundFront`) and pushes them
//! on a short-lock queue; the mixer thread takes them between frames, mixes frames a tenth of a
//! second ahead into a ring, and the device's callback takes samples from the ring, converting
//! the rate from 32 kHz to the device's by linear interpolation. The callback allocates nothing
//! (the ring is made at its full size, the lock is only tried, and an empty ring plays silence).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use housefire::mixer::SPF;
use housefire::replay::sound::SoundRequest;
use housefire::scene::Camera;
use housefire::sound::SoundBank;
use housefire::sound_events::SoundEvents;
use housefire::sound_front::SoundFront;
use housefire::sound_run::{Command, RATE, SoundRun};
use housefire::world::World;

/// How far ahead of the device the mixer thread mixes, in frames of 5 ms (100 ms).
const AHEAD_FRAMES: usize = 20;
/// Game frames a second (30000/1001): the WAV run's clock.
const TICKS_PER_SECOND: f64 = 30000.0 / 1001.0;
/// The commands the queue holds before it drops more (a frame of the simulation makes a few).
const QUEUE_CAPACITY: usize = 1024;

struct Shared {
    commands: Mutex<Vec<Command>>,
    /// Interleaved stereo samples at 32 kHz, mixed ahead.
    ring: Mutex<VecDeque<i16>>,
    stop: AtomicBool,
}

/// The sound device and the mixer thread feeding it.
pub struct Audio {
    shared: Arc<Shared>,
    _stream: cpal::Stream,
    thread: Option<JoinHandle<()>>,
}

impl Audio {
    /// Opens the default output and starts the mixer. An error says why there is no sound.
    pub fn open(bank: &'static SoundBank) -> Result<Audio, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no sound output device")?;
        let config = device.default_output_config().map_err(|e| format!("the sound device's format: {e}"))?;
        let shared = Arc::new(Shared {
            commands: Mutex::new(Vec::with_capacity(QUEUE_CAPACITY)),
            ring: Mutex::new(VecDeque::with_capacity(2 * SPF * (AHEAD_FRAMES + 2))),
            stop: AtomicBool::new(false),
        });
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), &shared)?,
            cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), &shared)?,
            cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), &shared)?,
            f => return Err(format!("the sound device's sample format {f:?} is not handled")),
        };
        stream.play().map_err(|e| format!("starting the sound device: {e}"))?;
        let mixer_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new().name("mixer".into()).spawn(move || mixer_thread(bank, &mixer_shared)).map_err(|e| e.to_string())?;
        Ok(Audio { shared, _stream: stream, thread: Some(thread) })
    }

    /// Hands commands to the mixer (dropped if the queue is full).
    pub fn send(&self, commands: &[Command]) {
        if commands.is_empty() {
            return;
        }
        let mut q = self.shared.commands.lock().expect("queue");
        let room = QUEUE_CAPACITY.saturating_sub(q.len());
        q.extend_from_slice(&commands[..commands.len().min(room)]);
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

/// The mixer thread: takes the commands, mixes frames until the ring holds `AHEAD_FRAMES`.
fn mixer_thread(bank: &'static SoundBank, shared: &Shared) {
    let mut run = SoundRun::new(bank);
    let mut frame = [0i16; 2 * SPF];
    let mut taken: Vec<Command> = Vec::with_capacity(QUEUE_CAPACITY);
    while !shared.stop.load(Ordering::Relaxed) {
        let queued = shared.ring.lock().map_or(usize::MAX, |r| r.len() / (2 * SPF));
        if queued >= AHEAD_FRAMES {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        {
            let mut q = shared.commands.lock().expect("queue");
            taken.clear();
            taken.extend_from_slice(&q);
            q.clear();
        }
        for c in taken.drain(..) {
            run.apply(c);
        }
        run.frame(&mut frame);
        shared.ring.lock().expect("ring").extend(frame.iter().copied());
    }
}

/// A device's sample from a float of -1 to 1.
fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let step = RATE as f64 / config.sample_rate.0 as f64;
    let shared = Arc::clone(shared);
    // the interpolation's state: the two stereo samples around the read position
    let mut pos = 1.0f64;
    let mut from = [0.0f32; 2];
    let mut to = [0.0f32; 2];
    let callback = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        let mut ring = shared.ring.try_lock().ok();
        for out in data.chunks_mut(channels) {
            while pos >= 1.0 {
                pos -= 1.0;
                from = to;
                to = match ring.as_mut().and_then(|r| (r.len() >= 2).then(|| [r.pop_front().unwrap_or(0), r.pop_front().unwrap_or(0)])) {
                    Some(s) => [s[0] as f32 / 32768.0, s[1] as f32 / 32768.0],
                    None => [0.0, 0.0],
                };
            }
            let t = pos as f32;
            let l = from[0] + (to[0] - from[0]) * t;
            let r = from[1] + (to[1] - from[1]) * t;
            pos += step;
            for (k, o) in out.iter_mut().enumerate() {
                *o = T::from_sample(match (channels, k) {
                    (1, _) => 0.5 * (l + r),
                    (_, 0) => l,
                    (_, 1) => r,
                    _ => 0.0,
                });
            }
        }
    };
    device.build_output_stream(config, callback, |e| eprintln!("play: sound device: {e}"), None).map_err(|e| format!("opening the sound device: {e}"))
}

/// A headless run's mixer and what it has mixed.
struct WavRun {
    run: SoundRun<'static>,
    samples: Vec<i16>,
    owed: f64,
    path: std::path::PathBuf,
}

/// Where a run's sound goes.
enum Sink {
    Device(Audio),
    /// A headless run's file: the mixer is run here, a tick's frames at a time.
    Wav(Box<WavRun>),
}

/// The sound of a run: the simulation's side, and where the commands go.
pub struct Sound {
    front: SoundFront<'static>,
    sink: Sink,
    printed: usize,
}

impl Sound {
    /// Sound for a window: through the device, or an error saying why there is none.
    pub fn device(bank: &'static SoundBank, events: &'static SoundEvents, seed: u64) -> Result<Sound, String> {
        Ok(Sound { front: SoundFront::new(bank, events, seed), sink: Sink::Device(Audio::open(bank)?), printed: 0 })
    }

    /// Sound for a headless run: the mix written to `path` as a WAV when the run ends.
    pub fn wav(bank: &'static SoundBank, events: &'static SoundEvents, seed: u64, path: std::path::PathBuf) -> Sound {
        Sound { front: SoundFront::new(bank, events, seed), sink: Sink::Wav(Box::new(WavRun { run: SoundRun::new(bank), samples: Vec::new(), owed: 0.0, path })), printed: 0 }
    }

    /// An interface sound (`UI_accept`, `UI_move`).
    pub fn ui(&mut self, tick: u32, name: &str) {
        self.front.ui(tick, name);
    }

    pub fn music(&mut self, name: Option<&str>) {
        self.front.music(name);
    }

    /// The options' volumes (0 to 10 each).
    pub fn volumes(&mut self, sound: u8, music: u8) {
        self.front.volumes(sound, music);
    }

    /// A tick of the simulation has run: its requests, the camera and the world's places.
    pub fn tick(&mut self, tick: u32, requests: &[SoundRequest], camera: &Camera, world: &World) {
        self.front.set_camera(camera);
        for r in requests {
            self.front.request(tick, r);
        }
        self.front.frame(world);
        self.flush();
        if let Sink::Wav(w) = &mut self.sink {
            let WavRun { run, samples, owed, .. } = &mut **w;
            // the tick's share of frames (6.67 of 5 ms), the remainder carried
            *owed += (RATE as f64 / TICKS_PER_SECOND) / SPF as f64;
            let mut frame = [0i16; 2 * SPF];
            while *owed >= 1.0 {
                run.frame(&mut frame);
                samples.extend_from_slice(&frame);
                *owed -= 1.0;
            }
        }
    }

    /// The commands made so far go to the sink; the sounds started are printed.
    pub fn flush(&mut self) {
        for s in &self.front.started[self.printed..] {
            println!("play: sound tick {} event {} sample {}", s.tick, s.event, s.sample);
        }
        self.printed = self.front.started.len();
        let commands = self.front.take();
        if std::env::var_os("PLAY_SOUND_DEBUG").is_some() {
            for c in &commands {
                println!("play: sound command {c:?}");
            }
        }
        match &mut self.sink {
            Sink::Device(a) => a.send(&commands),
            Sink::Wav(w) => commands.into_iter().for_each(|c| w.run.apply(c)),
        }
    }

    /// The run is over: a WAV run writes its file and says what is in it.
    pub fn finish(self) -> Result<(), String> {
        let Sink::Wav(w) = self.sink else { return Ok(()) };
        let WavRun { samples, path, .. } = *w;
        let peak = samples.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0);
        let rms = (samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len().max(1) as f64).sqrt();
        write_wav(&path, &samples).map_err(|e| format!("writing {}: {e}", path.display()))?;
        println!("play: wav {} seconds, peak {peak}, rms {rms:.1}, {} sounds started", samples.len() as f64 / 2.0 / RATE as f64, self.front.started.len());
        Ok(())
    }
}

/// A 16-bit stereo PCM WAV at 32 kHz.
fn write_wav(path: &std::path::Path, samples: &[i16]) -> std::io::Result<()> {
    use std::io::Write;
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?; // stereo
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&(RATE * 4).to_le_bytes())?; // bytes a second
    f.write_all(&4u16.to_le_bytes())?; // bytes a sample frame
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    f.flush()
}
