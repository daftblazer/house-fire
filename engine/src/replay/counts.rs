use std::collections::{BTreeMap, HashMap};
use std::time::Instant;


/// What was seen, counted by kind (--bench: nothing, only the timings).
#[derive(Default)]
pub struct Counts {
    pub map: BTreeMap<String, usize>,
    pub quiet: bool,
    /// The tick being run (0 before the first and for the checks over the whole log).
    pub tick: u32,
    /// The tick each kind was first counted in (`replay --free`).
    pub first: BTreeMap<String, u32>,
    /// The PlaySound requests made (`sound::figure` compares them with the log's plays).
    pub sound_asks: Vec<super::sound::SoundAsk>,
}

impl Counts {
    pub(super) fn note(&mut self, k: &str) {
        self.add(k, 1);
    }
    pub(super) fn add(&mut self, k: &str, n: usize) {
        if !self.quiet {
            // (the key is made a String only the first time it is counted)
            match self.map.get_mut(k) {
                Some(c) => *c += n,
                None => {
                    self.map.insert(k.to_string(), n);
                    self.first.insert(k.to_string(), self.tick);
                }
            }
        }
    }
}

/// --bench: the native work's time, per tick and per part. The replay's own work (finding trees,
/// comparing with the log, setting the native state to the log's) is not counted.
#[derive(Default, Clone)]
pub struct Bench {
    /// The native work of each tick that ran, in µs.
    pub ticks: Vec<f64>,
    pub steps_us: f64,
    pub expression_us: f64,
    pub set_to_next_us: f64,
    pub idle_us: f64,
    pub other_us: f64,
    pub pass_us: f64,
    pub sync_us: f64,
    pub steps: usize,
    pub worst_tick: usize,
}

pub(super) fn us(a: Instant) -> f64 {
    a.elapsed().as_secs_f64() * 1e6
}

/// The studies' "shown" counts (the C++'s static ones).
#[derive(Default)]
pub(super) struct Shown(pub(super) HashMap<String, i32>);

impl Shown {
    /// Whether a study of this kind may print once more (below `limit`), counting it.
    pub(super) fn next(&mut self, kind: &str, limit: i32) -> bool {
        let n = self.0.entry(kind.to_string()).or_default();
        *n += 1;
        *n <= limit
    }
}

/// True the first time `what` is asked.
pub(super) fn shown_once(shown: &mut Shown, what: &'static str) -> bool {
    shown.next(what, 1)
}
