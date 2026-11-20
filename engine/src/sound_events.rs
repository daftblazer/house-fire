//! The sound events: how a name the scripts and animations give (`counter_chop`,
//! `footstep_medium`) becomes the samples that may play. The original's sound layer resolves it
//! through the `snd::` tables of quick data's `SimsObjects` (`docs/assets.md`, "The sound
//! events"):
//!
//! - `snd::EventMapping`: the event's name, a kind byte and its `snd::Track`.
//! - the track's word 2: a hit table by voice (a count in the word before, then that many
//!   pointers to `snd::GlobalHitlist` rows, a null one for no sound; a plain random event has
//!   one), or its word 3: one `snd::Patch` (a sample), a `snd::GlobalHitlist` row, or a
//!   `snd::VoxHitlist` row (a male list and a female one).
//! - a `snd::GlobalHitlist` row: a pointer to the first of `count` pointers (the count in the
//!   word before) to `snd::Patch` rows, whose word is the sample's id (the CRC-32 of its name in
//!   capitals, the archive's entry id).
//!
//! The choice among the samples of a list is random in the original (and draws from the
//! simulation's one random number there); here it is the presentation's to make, so what this
//! module gives is the set a play may come from.

use std::collections::HashMap;

use crate::arc;
use crate::disc::Disc;
use crate::quickdata::Image;

/// Whose voice speaks, for the events that have one per voice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voice {
    /// A woman's or a man's list (`snd::VoxHitlist`); `None` if unknown (both lists).
    pub female: Option<bool>,
    /// The index into a hit table by voice; `None` if unknown (every entry).
    pub index: Option<usize>,
}

/// What an event may play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The event has no track (a command: `radio_next_song`) or none that could be read.
    Silent,
    /// One list for every Sim (a sample, a list to draw from).
    Plain(Vec<u32>),
    /// A male and a female list (`snd::VoxHitlist`).
    ByGender { male: Vec<u32>, female: Vec<u32> },
    /// A list a voice (a hit table by voice).
    ByVoice(Vec<Vec<u32>>),
}

impl Choice {
    /// The sample ids a play may come from for this voice (every one that could if unknown).
    pub fn samples(&self, voice: Voice) -> Vec<u32> {
        match self {
            Choice::Silent => Vec::new(),
            Choice::Plain(v) => v.clone(),
            Choice::ByGender { male, female } => match voice.female {
                Some(true) => female.clone(),
                Some(false) => male.clone(),
                None => male.iter().chain(female).copied().collect(),
            },
            Choice::ByVoice(v) => match voice.index.and_then(|i| v.get(i)) {
                Some(l) => l.clone(),
                None => v.iter().flatten().copied().collect(),
            },
        }
    }

    /// Whether the voice matters (the set differs by voice).
    pub fn by_voice(&self) -> bool {
        matches!(self, Choice::ByGender { .. } | Choice::ByVoice(_))
    }
}

/// An event: its kind byte (1 a sound, 30 the DJ's music; 0, 19, 23, 43, 101 commands) and what
/// it plays.
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: u8,
    /// The track's first two words: the flags (`0800828A`, `0800829A` for the voices) and the
    /// parameters (`64001040`, ...); 0 for an event with no track.
    pub track_words: [u32; 2],
    pub choice: Choice,
}

/// The sound events of a disc's `SimsObjects`, by name.
pub struct SoundEvents {
    events: HashMap<String, Event>,
}

/// A table's rows and size, for finding which table an offset lies in.
struct Span {
    start: usize,
    end: usize,
}

struct Tables<'a> {
    img: &'a Image,
    patch: Span,
    global: Span,
    vox: Span,
}

impl Tables<'_> {
    fn span(img: &Image, name: &str) -> Option<Span> {
        let t = img.tables.get(name)?;
        Some(Span { start: t.rows, end: t.rows + t.count * t.row_size })
    }

    /// The sample ids of the `snd::GlobalHitlist` row at `row` (an offset of the table).
    fn global_list(&self, row: usize) -> Option<Vec<u32>> {
        let first = self.img.ptr(row)?;
        let n = self.img.u32(first.checked_sub(4)?)? as usize;
        if n > 256 {
            return None;
        }
        (0..n).map(|k| self.patch_id(self.img.ptr(first + 4 * k)?)).collect()
    }

    fn patch_id(&self, row: usize) -> Option<u32> {
        (self.patch.start <= row && row < self.patch.end).then(|| self.img.u32(row)).flatten()
    }

    /// The list a pointer slot names, if it points at a `snd::GlobalHitlist` row (a null slot
    /// is an empty list).
    fn list_at(&self, slot: usize) -> Option<Vec<u32>> {
        match self.img.ptr(slot) {
            None => Some(Vec::new()),
            Some(row) if self.global.start <= row && row < self.global.end => self.global_list(row),
            Some(_) => None,
        }
    }

    /// A track (its 5 words at `track`): what it plays.
    fn track(&self, track: usize) -> Choice {
        let img = self.img;
        if let Some(table) = img.ptr(track + 8) {
            // a hit table by voice: its count in the word before
            let n = img.u32(table.wrapping_sub(4)).unwrap_or(0) as usize;
            if (1..=64).contains(&n) {
                let lists: Option<Vec<Vec<u32>>> = (0..n).map(|k| self.list_at(table + 4 * k)).collect();
                if let Some(mut lists) = lists {
                    if n == 1 {
                        return Choice::Plain(lists.remove(0));
                    }
                    return Choice::ByVoice(lists);
                }
            }
        }
        let Some(p) = img.ptr(track + 12) else { return Choice::Silent };
        if let Some(id) = self.patch_id(p) {
            Choice::Plain(vec![id])
        } else if self.global.start <= p && p < self.global.end {
            self.global_list(p).map_or(Choice::Silent, Choice::Plain)
        } else if self.vox.start <= p && p < self.vox.end {
            match (self.list_at(p), self.list_at(p + 4)) {
                (Some(male), Some(female)) => Choice::ByGender { male, female },
                _ => Choice::Silent,
            }
        } else {
            Choice::Silent
        }
    }
}

impl SoundEvents {
    /// The events of the disc, or `None` if its quick data does not read.
    pub fn open(d: &Disc) -> Option<SoundEvents> {
        let quick = d.read("quickdat.arc")?;
        let e = arc::entries(&quick)?.into_iter().find(|e| e.name == "SimsObjects")?;
        SoundEvents::from_image(&Image::read(arc::bytes(&quick, &e))?)
    }

    /// The events of a `SimsObjects` image. Names are trimmed and lower-cased; a name with two
    /// rows keeps the first.
    pub fn from_image(img: &Image) -> Option<SoundEvents> {
        let t = Tables {
            img,
            patch: Tables::span(img, "snd::Patch")?,
            global: Tables::span(img, "snd::GlobalHitlist")?,
            vox: Tables::span(img, "snd::VoxHitlist")?,
        };
        let map = *img.tables.get("snd::EventMapping")?;
        let mut events = HashMap::new();
        for k in 0..map.count {
            let row = map.rows + k * map.row_size;
            let Some(name) = img.ptr(map.names + 4 * k).and_then(|s| img.data.get(s..)) else { continue };
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
            let kind = img.data.get(row).copied().unwrap_or(0);
            let choice = img.ptr(row + 4).map_or(Choice::Silent, |track| t.track(track));
            let track_words = img.ptr(row + 4).map_or([0, 0], |t| [img.u32(t).unwrap_or(0), img.u32(t + 4).unwrap_or(0)]);
            events.entry(name).or_insert(Event { kind, track_words, choice });
        }
        Some(SoundEvents { events })
    }

    /// The event called `name` (any case, spaces round it ignored).
    pub fn event(&self, name: &str) -> Option<&Event> {
        self.events.get(&name.trim().to_ascii_lowercase())
    }

    /// How many events there are.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
