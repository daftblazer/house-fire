//! The figure animator's tracks (LOCKSTEP_ANIM_TRACKS, with the run's card): per Sim and frame,
//! the tracks `animator::FigureAnimator` makes from the log-driven replay's own state (its
//! route followers' frames, `Replay::take_walk_frames`, and the Animate plays and the speed of
//! each frame, `Replay::frame_plays`) against the ones the original posed (the '(' pose records
//! with the '=' records' weights: the animation of the track's last record, its place, its
//! weight). A pose is the same when the tracks are the same animations in the same order, their
//! places within `PLACE` and their weights within `WEIGHT`. Prints `tracks: the same N of M Sim
//! poses` and the differences by kind with the first tick of each (LOCKSTEP_ANIM_TRACKS=2 lists
//! the first three of each kind in full).
//!
//! The log's frames are tick 0's last ones (the load's earlier frames pose no Sim), then those
//! the native clock puts in each tick (`Replay::frame_plays`: after the tick's steps).

use crate::*;
use housefire::anim::Catalog;
use housefire::animator::{ActionPlay, Body, FigureAnimator, Frame, Posed, Slot, Turn, WalkerFrame};
use housefire::figure::{self, Cast};
use housefire::replay::{Replay, RunWith, Start, WalkFrame};
use housefire::routing::{walk_rates, walk_step};

/// A place is the same within this, frames.
const PLACE: f32 = 0.01;
/// A weight is the same within this.
const WEIGHT: f32 = 1e-3;
/// A Sim's animator speed records ('#') name its animation controller by the address this far
/// below it (their owner's: the controller's tracks are logged by the controller).
const CONTROLLER_OFFSET: u32 = 0x350;

/// A pose as the log has it: the track's animation, place and weight.
#[derive(Clone, Copy, Debug)]
struct Logged {
    anim: u32,
    place: f32,
    weight: f32,
}

/// The Sims' pose sets in the log by (controller, tick): one set a frame, a frame's tracks in
/// the order they were posed (the face's, the layer, is the last of a set).
fn logged_sets(log: &housefire::tree_log::Log, face: u32) -> HashMap<(u32, u32), Vec<Vec<Logged>>> {
    // a track's animation: its last record before the pose (the log has a record a step)
    let mut plays: HashMap<u32, Vec<(usize, u32, u32)>> = HashMap::new();
    for t in &log.tracks {
        plays.entry(t.track).or_default().push((t.at, t.head[1], t.controller));
    }
    let mut open: HashMap<u32, Vec<Logged>> = HashMap::new();
    let mut out: HashMap<(u32, u32), Vec<Vec<Logged>>> = HashMap::new();
    for (p, r) in log.poses.iter().zip(&log.node_states) {
        let Some(&(_, anim, controller)) = plays.get(&p.track).and_then(|v| v.iter().rev().find(|x| x.0 < p.at).or(v.first())) else { continue };
        let set = open.entry(controller).or_default();
        set.push(Logged { anim, place: p.place, weight: r.weight });
        if anim == face {
            out.entry((controller, p.tick)).or_default().push(std::mem::take(set));
        }
    }
    out
}

/// The walker's speed over its top after a frame, the weight of the walking loop: `walk_step`
/// from the speed before (`speed`: the walker's, carried from frame to frame and from walk to
/// walk) and the way it had still to walk. A frame of the walker's that does not move it has none.
fn speed_fraction(w: &WalkFrame, speed: &mut f32) -> Option<f32> {
    if !(w.before == 2 || (w.before == 1 && w.after == 2)) {
        return None;
    }
    let (top, growth) = walk_rates(w.top);
    *speed = walk_step(*speed, top, growth, w.factor, w.left).0;
    Some(*speed / top)
}

/// The kind of the first difference of a pose from the logged one, or `None` when they are the
/// same.
fn kind(ours: &[Posed], logged: &[Logged], names: &BTreeMap<u32, String>) -> Option<String> {
    if ours.len() != logged.len() {
        // tracks the one has and the other has not
        let has = |s: Slot| ours.iter().any(|p| p.slot == s);
        let named = |f: &dyn Fn(&str) -> bool| logged.iter().any(|p| names.get(&p.anim).is_some_and(|n| f(n)));
        let carries = named(&|n| n.ends_with("carry-loop"));
        return Some(if has(Slot::Turning) != named(&|n| n.contains("turn-90")) {
            "skipped tracks: the turn posed or missing".into()
        } else if has(Slot::Walking) != named(&|n| n.contains("walking")) {
            "skipped tracks: the walking loop posed or missing".into()
        } else if has(Slot::Standing) != named(&|n| n.ends_with("standing-loop")) {
            "skipped tracks: the standing loop posed or missing".into()
        } else if has(Slot::Carrying) != carries {
            "skipped tracks: the carrying loop posed or missing".into()
        } else {
            "skipped tracks: the action's fade posed or missing".into()
        });
    }
    for (o, l) in ours.iter().zip(logged) {
        if o.anim != l.anim {
            return Some(format!("animation of the {:?} track", o.slot));
        }
        if (o.weight - l.weight).abs() > WEIGHT {
            return Some(match o.slot {
                Slot::Walking => "walk weight".into(),
                Slot::Turning => "turn weight".into(),
                Slot::Fading => "action fade weight".into(),
                s => format!("weight of the {s:?} track"),
            });
        }
        if (o.place - l.place).abs() > PLACE {
            return Some(match o.slot {
                Slot::Standing => "standing loop place".into(),
                Slot::Walking => "walking loop place".into(),
                Slot::Turning => "turn place".into(),
                Slot::Action | Slot::Fading => "action place".into(),
                s => format!("place of the {s:?} track"),
            });
        }
    }
    None
}

pub fn animator_tracks(log: &housefire::tree_log::Log, d: &Disc, lib: &Library, ngh: &[u8], card: Option<&str>) {
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let verbose = std::env::var("LOCKSTEP_ANIM_TRACKS").is_ok_and(|v| v == "2");
    let Ok(start) = Start::read(d, card, -1) else { return };
    let Some(cast) = Cast::read(d, ngh) else { return };
    let catalog = Catalog::read(d, house);
    let names: BTreeMap<u32, String> = catalog.by_id.iter().map(|(id, a)| (*id, a.name.clone())).collect();
    let Some(face) = catalog.id_of(figure::FACE_ANIMATION) else { return };
    let sets = logged_sets(log, face);
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: Some(catalog.clone()) }) else { return };
    replay.trace_walks();
    let mut animators: BTreeMap<i16, FigureAnimator> = BTreeMap::new();
    // each follower's state after its last frame (a route starts when it enters state 1) and
    // its speed (a walk starts at the speed the last one left)
    let mut follower_state: BTreeMap<u32, u32> = BTreeMap::new();
    let mut follower_speed: BTreeMap<u32, f32> = BTreeMap::new();
    let (mut same, mut total) = (0usize, 0usize);
    // kind -> (poses, first tick)
    let mut differences: BTreeMap<String, (usize, u32)> = BTreeMap::new();
    let mut controllers: BTreeMap<i16, u32> = BTreeMap::new();
    // (tick 0 is the load's: no step before it)
    for tick in 0..=log.ticks {
        if tick > 0 && !replay.step() {
            break;
        }
        let frame_plays = replay.frame_plays(tick);
        if controllers.is_empty() {
            controllers = replay.sim_controllers();
        }
        let walks = replay.take_walk_frames();
        let sims: Vec<(i16, Body, bool, f32)> = replay
            .world()
            .objects()
            .filter(|o| o.person && o.on_lot())
            .filter_map(|o| {
                let desc = o.person_data.get(31).and_then(|n| cast.description(*n))?;
                let carrying = replay.world().objects().any(|h| h.data.get(housefire::world::DATA_CONTAINER) == Some(&o.id()));
                Some((o.id(), Body { adult: desc.adult, male: desc.male }, carrying, o.motives.get(figure::FACE_MOTIVE).copied().unwrap_or(0.0)))
            })
            .collect();
        for (id, body, carrying, mood) in sims {
            let Some(&owner) = controllers.get(&id) else { continue };
            let logged = sets.get(&(owner + CONTROLLER_OFFSET, tick));
            let animator = match animators.entry(id) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => match FigureAnimator::new(&catalog, body) {
                    Some(a) => e.insert(a),
                    None => continue,
                },
            };
            let mine: Vec<&WalkFrame> = walks.iter().filter(|w| w.person == id).collect();
            let logged_frames = logged.map_or(0, Vec::len);
            // (tick 0 holds the load's frames, a hundred and more: the Sims' tracks begin with
            // the last of them, the ones the log poses)
            let from = if tick == 0 { frame_plays.len().saturating_sub(logged_frames) } else { 0 };
            let mut made: Vec<Vec<Posed>> = Vec::new();
            for (j, state) in frame_plays.iter().enumerate().skip(from) {
                let walker = mine.get(j).map(|w| {
                    let starts = w.before == 1 && follower_state.get(&w.follower).is_none_or(|s| *s != 1);
                    follower_state.insert(w.follower, w.after);
                    let speed_fraction = speed_fraction(w, follower_speed.entry(w.follower).or_default());
                    // (the turn on the spot while the walker waits to turn: states 1 and 3)
                    WalkerFrame { starts, speed_fraction, turn: (w.after == 1 || w.after == 3).then(|| Turn::of_gap(w.gap_before)) }
                });
                let action = state.plays.iter().find(|p| p.0 == id).map(|p| ActionPlay { anim: p.1, place: p.2, backward: p.3, serial: p.4 });
                let speed = state.speeds.iter().find(|p| p.0 == id).map_or(1.0, |p| p.1);
                animator.frame(&Frame { speed, walker, action, carrying, mood });
                made.push(animator.poses(&catalog));
            }
            let Some(logged) = logged else { continue };
            for (j, l) in logged.iter().enumerate() {
                total += 1;
                let Some(m) = made.get(j) else {
                    let e = differences.entry("frames: the log has more in the tick".into()).or_insert((0, tick));
                    e.0 += 1;
                    continue;
                };
                let Some(k) = kind(m, l, &names) else {
                    same += 1;
                    continue;
                };
                let e = differences.entry(k).or_insert((0, tick));
                e.0 += 1;
                if verbose && e.0 <= 3 {
                    let show = |v: &mut dyn Iterator<Item = (u32, f32, f32)>| v.map(|(a, p, w)| format!("{}@{p:.2}*{w:.4}", names.get(&a).map_or("?", |n| n.trim_start_matches("a2o-").trim_start_matches("c2o-")))).collect::<Vec<_>>().join(" ");
                    println!("  tick {tick} frame {j} Sim {id}:\n    ours   {}\n    logged {}", show(&mut m.iter().map(|p| (p.anim, p.place, p.weight))), show(&mut l.iter().map(|p| (p.anim, p.place, p.weight))));
                }
            }
        }
    }
    for (k, (n, first)) in &differences {
        println!("  {k}: {n}, first at tick {first}");
    }
    println!("tracks: the same {same} of {total} Sim poses");
}
