use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::library::Library;
use crate::tree_log::{Log, StepRec};
use crate::{FileId, TreeId};
use super::study::{env, has};

/// The Animate primitive run natively through the engine's `anim::Animator`, its Sims' plays
/// stepped by the log's frames ('k' records) where they fall among the steps. The speed of a
/// frame: 0 for the Sims a load miss of its tick stops (the asker of a miss the original asked,
/// replayed through `anim::Loads`, and every Sim with an Animate step that tick), 5 on the
/// fast-forward's frames (the log's), else 1.
pub(super) struct AnimReplay {
    pub(super) cat: crate::anim::Catalog,
    pub(super) m: crate::anim::Animator,
    /// The frames: (tick, the steps before its first record).
    pub(super) frames: Vec<(u32, usize)>,
    pub(super) next: usize,
    /// The Sims each tick's load misses stop (the askers; with every Sim animating that tick).
    pub(super) misses: BTreeMap<u32, BTreeSet<u32>>,
    pub(super) animating: BTreeMap<u32, BTreeSet<u32>>,
    pub(super) fast: BTreeSet<u32>,
    /// The ticks of the game paused for the Sims: their speed updates all left at 0 (the
    /// log's '#' records, like the fast-forward; The Sims 2's SAnimator2::Update).
    pub(super) paused: BTreeSet<u32>,
    /// Study REPLAY_ANIMATE_OWN_SPEED: each Sim's own speed, the last '#' record of its
    /// animator's owner (the nearest owner above the Sim's address) before the frame: (owner,
    /// the records by their place among the steps).
    pub(super) own: Option<BTreeMap<u32, Vec<(usize, f32)>>>,
    pub(super) owner_of: BTreeMap<u32, u32>,
    pub(super) tree_file: HashMap<TreeId, FileId>,
}

impl AnimReplay {
    pub(super) fn new(log: &Log, lib: &Library, cat: crate::anim::Catalog) -> Option<AnimReplay> {
        if log.tracks.is_empty() {
            return None;
        }
        let mut frames: Vec<(u32, usize)> = Vec::new();
        let mut last = None;
        for t in &log.tracks {
            if last != Some(t.frame) {
                last = Some(t.frame);
                frames.push((t.tick, t.after_steps));
            }
        }
        let mut moving: BTreeMap<u32, bool> = BTreeMap::new();
        for r in &log.speeds {
            *moving.entry(r.tick).or_default() |= r.after != 0.0;
        }
        let paused = moving.into_iter().filter(|e| !e.1).map(|e| e.0).collect();
        let mut own: BTreeMap<u32, Vec<(usize, f32)>> = BTreeMap::new();
        for r in &log.speeds {
            own.entry(r.owner).or_default().push((r.after_steps, r.after.abs()));
        }
        let sims: BTreeSet<u32> = log.steps.iter().map(|s| s.sim).collect();
        let owner_of: BTreeMap<u32, u32> = sims.iter().filter_map(|&sim| own.keys().copied().filter(|&o| o > sim && o - sim < 0x1000).min().map(|o| (sim, o))).collect();
        if has("REPLAY_SHOW_SPEEDS") {
            println!("  owners: {owner_of:x?}");
        }
        if let Some((c, a, b)) = env("REPLAY_SHOW_TRACKS").and_then(|v| {
            let p: Vec<u32> = v.split(',').filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
            (p.len() == 3).then(|| (p[0], p[1], p[2]))
        }) {
            let sp: Vec<_> = log.speeds.iter().filter(|r| r.tick >= a && r.tick <= b && r.before != r.after).map(|r| (r.tick, format!("{:x}", r.owner), r.before, r.after)).collect();
            println!("  speed changes {sp:?}");
            for t in log.tracks.iter().filter(|t| t.controller == c && t.tick >= a && t.tick <= b) {
                println!("  track t {} frame {} track {} anim {:x} speeds {:?} place {:?} ran {:?} done {:?}", t.tick, t.frame, t.track, t.anim, t.speeds, t.place, t.ran, t.done);
            }
        }
        let own = has("REPLAY_ANIMATE_OWN_SPEED").then_some(own);
        let mut misses: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        let mut loads = cat.loads();
        for q in log.loaded.iter().filter(|q| q.caller == 0x800A_C7E4) {
            if !loads.ask(q.id, q.tick) {
                let e = misses.entry(q.tick).or_default();
                if let Some(st) = log.steps.get(q.after_steps.saturating_sub(1)) {
                    e.insert(st.sim);
                }
            }
        }
        let fast = log.tracks.iter().filter(|t| t.speeds[1].abs() == 5.0).map(|t| t.tick).collect();
        let mut tree_file = HashMap::new();
        for (k, f) in lib.files().iter().enumerate() {
            for t in f.trees.values() {
                tree_file.insert(*t, FileId(k as u32));
            }
        }
        if has("REPLAY_SHOW_SPEEDS") {
            for t in [20u32, 22, 23, 24, 50, 84, 85, 86] {
                let v: BTreeSet<(u32, u32, String)> = log.speeds.iter().filter(|r| r.tick == t).map(|r| (r.animator, r.owner, format!("{}->{}", r.before, r.after))).collect();
                let tr: BTreeSet<String> = log.tracks.iter().filter(|r| r.tick == t).map(|r| format!("{:x}:{}", r.controller, r.speeds[1])).collect();
                println!("  speeds tick {t}: {v:x?}; tracks {tr:?}");
            }
        }
        let m = cat.animator();
        Some(AnimReplay { cat, m, frames, next: 0, misses, animating: BTreeMap::new(), fast, paused, own, owner_of, tree_file })
    }

    /// The frames before log step `step`.
    pub(super) fn frames_to(&mut self, step: usize) {
        let mut plays = std::mem::take(self.m.plays_mut());
        let mut next = self.next;
        self.run_frames(&mut plays, &mut next, |f| f.1 <= step);
        *self.m.plays_mut() = plays;
        self.next = next;
    }

    /// The animator as the picture of `tick` shows it: the frames of the ticks before it and the
    /// first of its own applied (a tick has one or two; the pictures of frames 2400 and 2401 on
    /// home-a39.log, ticks 1,039 and 1,040, show the pose after the first, 2398 and 2399, of
    /// their 1 and 2). The steps apply the frames as they come; this runs them on a copy.
    pub(super) fn animator_for(&self, tick: u32) -> crate::anim::Plays {
        let (mut m, mut next) = (self.m.plays_copy(), self.next);
        self.run_frames(&mut m, &mut next, |f| f.0 < tick);
        self.run_frames_up_to(&mut m, &mut next, 1, |f| f.0 == tick);
        m
    }

    /// Steps `m` by the frames from number `*next` that `due` says are due, each Sim at its speed.
    pub(super) fn run_frames(&self, m: &mut crate::anim::Plays, next: &mut usize, due: impl Fn(&(u32, usize)) -> bool) {
        self.run_frames_up_to(m, next, usize::MAX, due);
    }

    /// `run_frames`, at most `most` of them.
    pub(super) fn run_frames_up_to(&self, m: &mut crate::anim::Plays, next: &mut usize, most: usize, due: impl Fn(&(u32, usize)) -> bool) {
        let mut done = 0;
        while done < most
            && let Some(&(tick, at)) = self.frames.get(*next).filter(|f| due(f))
        {
            *next += 1;
            done += 1;
            if let Some(own) = &self.own {
                let owner_of = &self.owner_of;
                m.frame(|sim| owner_of.get(&sim).and_then(|o| own.get(o)).and_then(|v| v[..v.partition_point(|r| r.0 <= at)].last()).map_or(1.0, |r| r.1));
                continue;
            }
            let stopped = self.misses.get(&tick);
            let animating = self.animating.get(&tick);
            let fast = self.fast.contains(&tick);
            let all_paused = self.paused.contains(&tick);
            m.frame(|sim| {
                if all_paused || stopped.is_some_and(|st| st.contains(&sim) || animating.is_some_and(|a| a.contains(&sim))) {
                    0.0
                } else if fast {
                    5.0
                } else {
                    1.0
                }
            });
        }
    }
}

/// Whether a native Animate answer differs from the log's step (for REPLAY_SHOW_ANIMATE).
pub(super) fn animate_differs(native: &Option<crate::anim::Poll>, s: &StepRec, logged_true: bool) -> bool {
    use crate::anim::Poll;
    !match native {
        Some(Poll::Pending) => s.result == 1,
        Some(Poll::Done) => logged_true,
        Some(Poll::Event(_)) => s.result != 1 && !logged_true,
        None => true,
    }
}
