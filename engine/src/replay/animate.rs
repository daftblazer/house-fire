use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::bhav::Node;
use crate::interp::{Frame, Interpreter};
use crate::library::Library;
use crate::tree_log::{Log, StepRec};
use crate::world::{ObjectKey, World};
use crate::{FileId, TreeId};
use super::study::{env, has};
use super::atoi;

/// The Animate primitive run natively through the engine's `anim::Animator`, its Sims' plays
/// stepped a frame at a time: the frames placed among the steps by the native frame clock
/// (`native_clock`; by the log's track records where the log has no clock). The speed of a
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
    /// The held frames (a speed change in a frame that ran no tick), by number: their speed,
    /// the one before the change.
    pub(super) held: BTreeMap<usize, f32>,
}

impl AnimReplay {
    pub(super) fn new(log: &Log, lib: &Library, cat: crate::anim::Catalog, counts: &mut super::counts::Counts) -> Option<AnimReplay> {
        if log.tracks.is_empty() {
            return None;
        }
        let mut frames: Vec<(u32, usize)> = Vec::new();
        let mut held: BTreeMap<usize, f32> = BTreeMap::new();
        let mut last = None;
        for t in &log.tracks {
            if last != Some(t.frame) {
                last = Some(t.frame);
                frames.push((t.tick, t.after_steps));
            }
        }
        // With the original's clock logged, the frames in play are placed natively: a frame
        // runs its ticks and then the animations, so the frames before a tick (the native
        // clock's) all fall on the boundary before it. The frames before the first tick and
        // after the last are the log's (the lot's start is not the clock's).
        let clock = native_clock(log);
        if let (Some(first), Some(last)) = (clock.first(), clock.last()) {
            let logged = std::mem::take(&mut frames);
            frames.extend(logged.iter().copied().filter(|f| f.0 < first.tick));
            let before_play = frames.len();
            for t in &clock {
                // (where the native clock differs, the log's count, so that the run goes on)
                let n = if t.same() { t.frames } else { t.log_frames };
                let at = log.tick_steps.get(t.tick as usize).copied().unwrap_or(log.steps.len());
                // A speed change the player made in a frame that ran no tick (club 3204): that
                // frame is held (no tick, the clock afresh) and played at the speed before;
                // where the change came (the frame) is the player's input, the count the log's.
                if t.speed != t.speed_before && n > t.frames {
                    for k in 0..(n - t.frames) as usize {
                        held.insert(frames.len() + k, crate::routing::speed_factor(i32::from(t.speed_before)));
                    }
                }
                frames.extend(std::iter::repeat_n((t.tick, at), n as usize));
            }
            let in_play = frames.len() - before_play;
            frames.extend(logged.iter().copied().filter(|f| f.0 > last.tick));
            // A frame runs its ticks, then the engine's own steps (the menu's checks, when the
            // player asks: not the simulation's, not run here), then the animations. So a
            // logged frame sits before such steps when they came in a later frame with no tick:
            // the same place for the simulation.
            let apart = |a: &(u32, usize), b: &(u32, usize)| a.0 == b.0 && b.1 < a.1 && log.steps[b.1..a.1].iter().all(|s| !s.in_pass);
            let own = frames.iter().zip(&logged).filter(|(a, b)| apart(a, b)).count();
            for (a, b) in frames.iter_mut().zip(&logged) {
                if apart(a, b) {
                    *a = *b;
                }
            }
            counts.add("animation: frames in play before the engine's own steps of a later frame (the log's place)", own);
            let same = frames.iter().zip(&logged).filter(|(a, b)| a == b).count() - own;
            if has("REPLAY_SHOW_FRAME_PLACES") {
                println!("frames native {} logged {}", frames.len(), logged.len());
                for (i, (a, b)) in frames.iter().zip(&logged).enumerate().filter(|(_, (a, b))| a != b).take(12) {
                    println!("  frame {i}: native {a:?} logged {b:?} (the boundary before tick {} is {:?})", b.0, log.tick_steps.get(b.0 as usize));
                }
            }
            counts.add("animation: frames before play and after the last tick (the log's)", frames.len() - in_play);
            counts.add("animation: frames in play placed by the native clock, as the log's", same.saturating_sub(frames.len() - in_play));
            counts.add("animation: frames in play placed by the native clock, DIFFERENT", frames.len().max(logged.len()) - same - own);
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
            for t in log.tracks.iter().filter(|t| (c == 0 || t.controller == c) && t.tick >= a && t.tick <= b) {
                // (one of the animation's first words is its id in the archive)
                let name = t.head.iter().find_map(|w| cat.by_id.get(w)).map_or_else(|| format!("{:08x?}", t.head), |a| a.name.clone());
                println!("  track t {} frame {} track {} anim {:x} ({name}) speeds {:?} place {:?} ran {:?} done {:?}", t.tick, t.frame, t.track, t.anim, t.speeds, t.place, t.ran, t.done);
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
        Some(AnimReplay { cat, m, frames, next: 0, misses, animating: BTreeMap::new(), fast, paused, own, owner_of, tree_file, held })
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
            let held = self.held.get(&(*next - 1)).copied();
            let fast = self.fast.contains(&tick);
            let all_paused = self.paused.contains(&tick);
            if env("REPLAY_SHOW_FRAME_SPEED").and_then(|v| v.split_once(',').map(|(a, b)| (atoi(a) as u32, atoi(b) as u32))).is_some_and(|(a, b)| (a..=b).contains(&tick)) {
                println!("frame {} tick {tick} at {at}: fast {fast} paused {all_paused} stopped {:?}", *next - 1, stopped.map(|s| s.len()));
            }
            m.frame(|sim| {
                if all_paused || stopped.is_some_and(|st| st.contains(&sim) || animating.is_some_and(|a| a.contains(&sim))) {
                    0.0
                } else if let Some(v) = held {
                    v
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

/// A tick of the native frame clock against the original's (',' records).
pub(super) struct ClockTick {
    /// The tick about to start.
    pub(super) tick: u32,
    pub(super) speed_before: i16,
    pub(super) speed: i16,
    /// The frames since the tick before, natively and as logged.
    pub(super) frames: u32,
    pub(super) log_frames: u32,
    /// The clock after this tick's share was taken.
    pub(super) ms: f32,
    pub(super) log_ms: f32,
}

impl ClockTick {
    pub(super) fn same(&self) -> bool {
        self.frames == self.log_frames && self.ms.to_bits() == self.log_ms.to_bits()
    }
}

/// The frame clock run natively from the log's first value, the speed setting an input: each
/// tick after the first. Where it differs the clock goes on from the log's.
pub(super) fn native_clock(log: &Log) -> Vec<ClockTick> {
    let Some(first) = log.clock.first() else { return Vec::new() };
    let mut clock = crate::routing::FrameClock { ms: first.2 };
    log.clock
        .windows(2)
        .map(|w| {
            let ((_, speed_before, _, frame_before), (tick, speed, log_ms, frame)) = (w[0], w[1]);
            if speed != speed_before {
                clock.restart();
            }
            let frames = clock.frames_to_tick_at(crate::routing::speed_factor(i32::from(speed)));
            let t = ClockTick { tick, speed_before, speed, frames, log_frames: frame.wrapping_sub(frame_before), ms: clock.ms, log_ms };
            if speed != speed_before && has("REPLAY_SHOW_SPEED_CHANGES") {
                println!("speed change tick {tick}: {speed_before} -> {speed}, frames native {frames} log {}, ms native {} log {log_ms}, clock before {:?}", t.log_frames, clock.ms, w[0]);
            }
            if !t.same() {
                clock.ms = log_ms;
            }
            t
        })
        .collect()
}

/// An Animate step (opcode 44) answered natively: the animation table and file by the operand's
/// byte 4, the lookup in the catalog and the animator's answer for the Sim `sim` at `tick` (`(sim, tick)`). The
/// file and the table come back too (for the studies); no answer where the archive lacks the
/// animation (`Lookup::Unknown`). Shared by the plain replay and the driven run.
pub(super) fn animate(ar: &mut AnimReplay, lib: &Library, world: &World, me: ObjectKey, interp: &mut Interpreter, n: &Node, (sim, tick): (u32, u32)) -> (Option<FileId>, u16, Option<crate::anim::Poll>) {
    use crate::anim::{Lookup, Poll};
    // source 0 in a tree whose file has no names (a semi-global's): the stack object's own file
    let stack_file = world.by_id().get(&interp.top().stack_object).and_then(|k| world.get(*k).file);
    let tree_file = interp.top().tree.and_then(|t| ar.tree_file.get(&t).copied());
    // the table (operand byte 4): 0 the tree's file's STR# 129 (the stack object's, for a
    // semi-global's tree), 1 Global's STR# 128, 2 the Sim's, 3 Global's STR# 156 (the adult
    // list; 157 the child's)
    let (file, table) = match n.operand[4] {
        0 if tree_file.is_some_and(|f| lib.file(f).strings.get(&129).is_none_or(|t| t.iter().all(|x| x.is_empty()))) => (stack_file, 129),
        0 => (tree_file, 129),
        1 => (lib.global(), 128),
        2 => (world.get(me).file, 129),
        3 => (lib.global(), 156),
        _ => (None, 129),
    };
    let native = match ar.cat.lookup(lib, file, table, &n.operand) {
        Lookup::Found(id) => {
            let a = &ar.cat.by_id[&id];
            Some(ar.m.animate(sim, id, a, n.operand[5] & 2 != 0, n.operand[6] as i8 as i32, tick))
        }
        Lookup::NotFound => Some(Poll::Done),
        Lookup::Unknown => None,
    };
    (file, table, native)
}

/// An Animate event's value goes to the frame: the local operand byte 2 names with flag 0x20,
/// else parameter 0 (primitives.md, "counted events").
pub(super) fn write_event(f: &mut Frame, n: &Node, value: i32) {
    let to = if n.operand[5] & 0x20 != 0 {
        f.unset &= !(1u64 << (n.operand[2] & 63));
        f.locals.get_mut(n.operand[2] as usize)
    } else {
        f.params.get_mut(0)
    };
    if let Some(to) = to {
        *to = value as i16;
    }
}
