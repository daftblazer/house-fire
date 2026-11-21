//! The figure animator's tracks (LOCKSTEP_ANIM_TRACKS, with the run's card): per Sim and frame,
//! the tracks `animator::FigureAnimator` makes from the log-driven replay's own state (its
//! walkers' frames, `Replay::take_walk_frames`, and the Animate plays after each frame,
//! `Replay::frame_plays`) against the ones the original posed (the '(' pose records with the
//! '=' records' weights: the animation by the track's last record, the place, the weight). A
//! pose is the same when the tracks are the same animations in the same order, their places
//! within `PLACE` and weights within `WEIGHT`. Prints `tracks: the same N of M Sim poses` and the
//! differences by kind with the first tick of each (LOCKSTEP_ANIM_TRACKS=2 lists the first
//! differences of each kind in full).

use crate::*;
use housefire::anim::Catalog;
use housefire::animator::{ActionPlay, Body, FigureAnimator, Frame, Posed, Slot, Turn, WalkerFrame};
use housefire::figure::{self, Cast};
use housefire::replay::{Replay, RunWith, Start, WalkFrame};
use housefire::routing::{WALK_FRAME, walk_rates, walk_step};

/// A place is the same within this, frames.
const PLACE: f32 = 0.01;
/// A weight is the same within this.
const WEIGHT: f32 = 1e-3;

/// A pose as the log has it: the track's animation, place and weight.
#[derive(Clone, Copy, Debug)]
struct Logged {
    anim: u32,
    place: f32,
    weight: f32,
}

/// A frame of a Sim: the poses it made.
type Set = Vec<Logged>;

/// The Sims' pose sets in the log by (controller, tick), each a frame's tracks in order (a set
/// ends with the face's).
fn logged_sets(log: &housefire::tree_log::Log, face: u32) -> HashMap<(u32, u32), Vec<Set>> {
    let mut plays: HashMap<u32, Vec<(usize, u32, u32)>> = HashMap::new();
    for t in &log.tracks {
        plays.entry(t.track).or_default().push((t.at, t.head[1], t.controller));
    }
    let mut open: HashMap<u32, Set> = HashMap::new();
    let mut out: HashMap<(u32, u32), Vec<Set>> = HashMap::new();
    for (p, r) in log.poses.iter().zip(&log.node_states) {
        let Some(&(_, anim, ctl)) = plays.get(&p.track).and_then(|v| v.iter().rev().find(|x| x.0 < p.at).or(v.first())) else { continue };
        let set = open.entry(ctl).or_default();
        set.push(Logged { anim, place: p.place, weight: r.weight });
        if anim == face {
            out.entry((ctl, p.tick)).or_default().push(std::mem::take(set));
        }
    }
    out
}

/// The walker's speed over its top at a frame, the weight of the walking loop: the speed the
/// frame ended at (`speed` is the walker's, carried from frame to frame, and from walk to walk).
/// A walking frame moves the walker by its speed, which grows by `growth` to the top (`walk_step`)
/// or brakes to stop at the end; the frame that arrives moves it what is left and sets the speed to
/// what lets it stop there.
fn speed_fraction(w: &WalkFrame, speed: &mut f32) -> Option<f32> {
    let moved = w.before == 2 || (w.before == 1 && w.after == 2);
    if !moved {
        return None;
    }
    let (top, growth) = walk_rates(w.top);
    let v = if w.factor != 1.0 {
        top
    } else if w.after == 2 {
        // (the step is the logged place's difference: a float's noise off the speed the
        // walker grows to)
        let grown = top.min(*speed + growth);
        if (w.step - grown).abs() < 1e-3 { grown } else { w.step }
    } else {
        walk_step(0.0, top, growth, 1.0, w.step).0
    };
    *speed = v;
    Some(v / top)
}

/// The kind of the first difference of a pose against the logged one.
fn kind(ours: &[Posed], logged: &[Logged], names: &BTreeMap<u32, String>) -> String {
    if ours.len() != logged.len() {
        let name = |a: u32| names.get(&a).cloned().unwrap_or_default().replace("a2o-", "").replace("c2o-", "");
        let o: Vec<String> = ours.iter().map(|p| format!("{:?}", p.slot)).collect();
        let l: Vec<String> = logged.iter().map(|p| name(p.anim)).collect();
        let _ = (o, l);
        let slots = |v: &[Posed]| v.iter().map(|p| p.slot).collect::<Vec<_>>();
        let has = |s: Slot, v: &[Posed]| v.iter().any(|p| p.slot == s);
        let ls: Vec<&str> = logged.iter().map(|p| names.get(&p.anim).map_or("", |s| s.as_str())).collect();
        let l_turn = ls.iter().any(|n| n.contains("turn-90"));
        let l_walk = ls.iter().any(|n| n.contains("walking"));
        let l_stand = ls.iter().any(|n| n.ends_with("standing-loop"));
        let _ = slots(ours);
        return if has(Slot::Turning, ours) != l_turn {
            "skipped tracks: a turn posed or missing".into()
        } else if has(Slot::Walking, ours) != l_walk {
            "skipped tracks: the walking loop posed or missing".into()
        } else if has(Slot::Standing, ours) != l_stand {
            "skipped tracks: the standing loop posed or missing".into()
        } else if has(Slot::Fading, ours) != (logged.len() > 1 && ours.len() < logged.len() && !l_turn) {
            "skipped tracks: an action fade posed or missing".into()
        } else {
            "skipped tracks: other (action, carry)".into()
        };
    }
    for (o, l) in ours.iter().zip(logged) {
        if o.anim != l.anim {
            return format!("animation of the {:?} track", o.slot);
        }
        if (o.weight - l.weight).abs() > WEIGHT {
            return match o.slot {
                Slot::Walking => "walk weight".into(),
                Slot::Turning => "turn weight".into(),
                Slot::Fading => "action fade weight".into(),
                s => format!("weight of the {s:?} track"),
            };
        }
        if (o.place - l.place).abs() > PLACE {
            return match o.slot {
                Slot::Standing => "standing loop place".into(),
                Slot::Walking => "walking loop place".into(),
                Slot::Turning => "turn place".into(),
                Slot::Action | Slot::Fading => "action place".into(),
                s => format!("place of the {s:?} track"),
            };
        }
    }
    "same".into()
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
    // each Sim's animator speed at each frame: the '#' records of its animator's owner, one a
    // frame (0 on the frames its animations load)
    let mut speeds: BTreeMap<(u32, u32), Vec<f32>> = BTreeMap::new();
    for r in &log.speeds {
        speeds.entry((r.owner, r.tick)).or_default().push(r.after.abs());
    }
    if std::env::var_os("TRACKS_SPEEDS").is_some() {
        for r in log.speeds.iter().filter(|r| r.tick <= 2) {
            println!("  speed t{} owner {:x} {} -> {} state {:x?}", r.tick, r.owner, r.before, r.after, r.state);
        }
    }
    let mut animators: BTreeMap<i16, FigureAnimator> = BTreeMap::new();
    let mut walk_state: BTreeMap<u32, u32> = BTreeMap::new();
    let mut walk_speed: BTreeMap<u32, f32> = BTreeMap::new();
    let (mut same, mut total) = (0usize, 0usize);
    let mut differences: BTreeMap<String, (usize, u32, String)> = BTreeMap::new();
    let mut controllers: BTreeMap<i16, u32> = BTreeMap::new();
    let mut mapped_offsets: BTreeSet<i64> = BTreeSet::new();
    let ctl_ids: BTreeSet<u32> = log.tracks.iter().map(|t| t.controller).collect();
    let mut next = 0u32;
    loop {
        // (the frames of a tick come after its steps; tick 0 is the load's, before the first step)
        if next > 0 && !replay.step() {
            break;
        }
        let tick = next;
        next += 1;
        let frame_plays = replay.frame_plays(tick);
        if controllers.is_empty() {
            controllers = replay.sim_controllers();
            for c in controllers.values() {
                for off in [0x350i64, -0x350] {
                    let guess = (i64::from(*c) + off) as u32;
                    if ctl_ids.contains(&guess) {
                        mapped_offsets.insert(off);
                    }
                }
            }
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
            let ctl = owner + 0x350;
            let logged = sets.get(&(ctl, tick));
            let animator = match animators.entry(id) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => match FigureAnimator::new(&catalog, body) {
                    Some(a) => e.insert(a),
                    None => continue,
                },
            };
                        let mine: Vec<&WalkFrame> = walks.iter().filter(|w| w.person == id).collect();
            let frames = frame_plays.len();
            let mut made: Vec<Vec<Posed>> = Vec::new();
            for j in 0..frames {
                let walker = mine.get(j).map(|w| {
                    let starts = w.before == 1 && walk_state.get(&w.follower).is_none_or(|s| *s != 1);
                    walk_state.insert(w.follower, w.after);
                    let gap = w.raw_before;
                    WalkerFrame { starts, speed_fraction: speed_fraction(w, walk_speed.entry(w.follower).or_default()), turn: (w.after == 1 || w.after == 3).then(|| Turn::of_gap(gap)) }
                });
                let action = frame_plays[j].0.iter().find(|p| p.0 == id).map(|p| ActionPlay { anim: p.1, place: p.2, backward: p.3, serial: p.4 });
                if std::env::var("TRACKS_DEBUG").is_ok_and(|v| v.parse() == Ok(id)) && (std::env::var("TRACKS_LO").ok().and_then(|v| v.parse().ok()).unwrap_or(0)..=std::env::var("TRACKS_HI").ok().and_then(|v| v.parse().ok()).unwrap_or(0)).contains(&tick) {
                    println!("  dbg tick {tick} frame {j}/{frames} walker {walker:?} z {:?} action {action:?}", mine.get(j));
                }
                let speed = frame_plays[j].1.iter().find(|p| p.0 == id).map_or(1.0, |p| p.1);
                animator.frame(&catalog, &Frame { speed, walker, action, carrying, mood });
                made.push(animator.poses(&catalog));
            }
            let Some(logged) = logged else { continue };
            for (j, l) in logged.iter().enumerate() {
                total += 1;
                let Some(m) = made.get(j) else {
                    let e = differences.entry("frames: the log has more frames in the tick".into()).or_insert((0, tick, String::new()));
                    e.0 += 1;
                    continue;
                };
                let k = kind(m, l, &names);
                if k == "same" {
                    same += 1;
                    continue;
                }
                let e = differences.entry(k).or_insert((0, tick, String::new()));
                e.0 += 1;
                if verbose && e.0 <= 3 {
                    let n = |a: u32| names.get(&a).cloned().unwrap_or_default().replace("a2o-", "").replace("c2o-", "");
                    let ours: Vec<String> = m.iter().map(|p| format!("{}@{:.2}*{:.4}", n(p.anim), p.place, p.weight)).collect();
                    let theirs: Vec<String> = l.iter().map(|p| format!("{}@{:.2}*{:.4}", n(p.anim), p.place, p.weight)).collect();
                    println!("  tick {tick} frame {j} Sim {id}:\n    ours   {}\n    logged {}", ours.join(" "), theirs.join(" "));
                }
            }
            if logged.len() < frames {
                let e = differences.entry("frames: the log has fewer frames in the tick".into()).or_insert((0, tick, String::new()));
                e.0 += 1;
            }
        }
    }
    let known: usize = sets.iter().filter(|((c, _), _)| ctl_ids.contains(c)).map(|(_, v)| v.len()).sum();
    println!("(controller offsets that name logged tracks: {mapped_offsets:?}; {known} logged Sim pose sets)");
    for (k, (n, first, _)) in &differences {
        println!("  {k}: {n}, first at tick {first}");
    }
    let _ = WALK_FRAME;
    println!("tracks: the same {same} of {total} Sim poses");
}
