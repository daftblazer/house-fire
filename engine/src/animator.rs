//! A Sim's figure animator: the tracks the original poses a Sim from, with their weights, made
//! from what the Sim does (standing, walking at a speed, turning on the spot, playing an
//! action). Presentation only and deterministic: the simulation never reads it. It steps once
//! per original frame (`FigureAnimator::frame`) and gives the poses in the order the game folds
//! them (`FigureAnimator::poses`). The rules are the ones `LOCKSTEP_ANIM_TRACKS` measured
//! against the logged '=' records (`docs/assets.md`, "The figure animator").
//!
//! The tracks, in pose order:
//! - the standing loop: always playing, weight 1;
//! - the walking loop: its weight is the walker's speed over its top speed, kept after the walk
//!   (the speed it ended at) until the Sim starts an action;
//! - the turn on the spot (90 degrees, clockwise or counter), while the walker waits to turn
//!   (before its walk, after its arrival): its weight fades in over `TURN_FADE` by a smoothstep
//!   and out by the same curve backwards;
//! - the Sim's current action (an Animate), weight 1, and the action before it still fading
//!   out over `ACTION_FADE`;
//! - the carrying loop and the face's layer, as `figure::tracks` has them.
//!
//! A track below the topmost non-masked track at weight 1 is not posed at all.

use crate::anim::{Catalog, track_step};
use crate::figure::pose_place;
use crate::routing::WALK_FRAME;

/// How long the turn takes to fade in or out, seconds (the logged weights of the turn track:
/// smoothstep of n frames of `WALK_FRAME` over this).
pub const TURN_FADE: f32 = 0.2;
/// How long the action before the current one takes to fade out, seconds.
pub const ACTION_FADE: f32 = 0.1;

/// Which way a turn on the spot goes (the animation's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Clockwise,
    CounterClockwise,
}

impl Turn {
    /// The turn that closes `gap` radians (`routing::turn_gap`: positive grows the facing angle).
    pub fn of_gap(gap: f32) -> Turn {
        if gap < 0.0 { Turn::Clockwise } else { Turn::CounterClockwise }
    }
}

/// What the Sim's walker did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkerFrame {
    /// The route's first frame: the Sim's action ends (it fades out).
    pub starts: bool,
    /// The walker's speed over its top, when it walks this frame (the weight of the walking loop).
    pub speed_fraction: Option<f32>,
    /// The turn on the spot it waits for, when it does (before its walk and after its arrival).
    pub turn: Option<Turn>,
}

/// The Sim's Animate play as of this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionPlay {
    /// The animation's id.
    pub anim: u32,
    /// How far along it is, in frames (not yet clamped to its last frame).
    pub place: f32,
    pub backward: bool,
    /// Which play it is: a play that starts anew has another number, though it may be the same
    /// animation (`anim::Plays::serial`).
    pub serial: u32,
}

/// One original frame of what drives the Sim's tracks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// The game's speed this frame: 0 paused, 1, 5 fast-forward.
    pub speed: f32,
    pub walker: Option<WalkerFrame>,
    pub action: Option<ActionPlay>,
    pub carrying: bool,
    /// The mood motive (0 to 100) the face's weight follows.
    pub mood: f32,
}

/// Which skeleton and animation set a Sim has.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub adult: bool,
    pub male: bool,
}

/// The slot a posed track came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Standing,
    Walking,
    Turning,
    Action,
    Fading,
    Carrying,
    Face,
}

/// A track to pose: its animation, place along it (frames) and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Posed {
    pub slot: Slot,
    pub anim: u32,
    pub place: f32,
    pub weight: f32,
}

/// The animations of a Sim's loops and turns, with what a step needs of each.
#[derive(Clone, Copy, Debug, Default)]
struct Clip {
    id: u32,
    last: f32,
    rate: f32,
    end_mode: u8,
}

impl Clip {
    fn find(cat: &Catalog, name: &str) -> Option<Clip> {
        let id = cat.id_of(name)?;
        let a = cat.by_id.get(&id)?;
        Some(Clip { id, last: a.frames.saturating_sub(1) as f32, rate: a.rate, end_mode: a.end_mode() })
    }

    /// The place a frame on at the game `speed`.
    fn step(&self, place: f32, speed: f32) -> f32 {
        track_step(place, WALK_FRAME, speed, 1.0, self.rate, self.last, self.end_mode).0
    }
}

/// The animation of a turn, with the loops it fades between.
#[derive(Clone, Copy, Debug)]
struct Turning {
    way: Turn,
    place: f32,
    /// How far the fade is: 1 at full weight, gone at 0.
    fade: f32,
}

#[derive(Clone, Copy, Debug)]
struct Acting {
    anim: u32,
    place: f32,
    backward: bool,
}

#[derive(Clone, Copy, Debug)]
struct Leaving {
    act: Acting,
    /// Seconds since it was replaced.
    age: f32,
}

/// The smoothstep of the fades.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A Sim's tracks, stepped a frame at a time.
#[derive(Clone, Debug)]
pub struct FigureAnimator {
    stand: Clip,
    walk: Clip,
    turn: [Clip; 2],
    carry: Clip,
    face: u32,
    stand_place: f32,
    walk_place: f32,
    walk_weight: f32,
    turning: Option<Turning>,
    acting: Option<Acting>,
    leaving: Option<Leaving>,
    last_serial: u32,
    carry_place: f32,
    carrying: bool,
    mood: f32,
}

impl FigureAnimator {
    /// An animator for a Sim of `body`, its animations from `cat`; none (the Sim has no figure
    /// animations in this catalog) when one is missing.
    pub fn new(cat: &Catalog, body: Body) -> Option<FigureAnimator> {
        let age = if body.adult { "a2o" } else { "c2o" };
        let walk = match (body.adult, body.male) {
            (true, false) => "a2o-walking-female-loop",
            (true, true) => "a2o-walking-loop",
            (false, _) => "c2o-walking-loop",
        };
        let turn = |way: &str| Clip::find(cat, &format!("{age}-standing-turn-90-{way}"));
        Some(FigureAnimator {
            stand: Clip::find(cat, &format!("{age}-standing-loop"))?,
            walk: Clip::find(cat, walk)?,
            turn: [turn("cw")?, turn("ccw")?],
            carry: Clip::find(cat, &format!("{age}-rarm-carry-loop"))?,
            face: cat.id_of(crate::figure::FACE_ANIMATION)?,
            stand_place: 0.0,
            walk_place: 0.0,
            walk_weight: 0.0,
            turning: None,
            acting: None,
            leaving: None,
            last_serial: 0,
            carry_place: 0.0,
            carrying: false,
            mood: 0.0,
        })
    }

    fn turn_clip(&self, way: Turn) -> Clip {
        self.turn[usize::from(way == Turn::CounterClockwise)]
    }

    /// The action `act` ends: it fades out.
    fn end_action(&mut self) {
        if let Some(act) = self.acting.take() {
            self.leaving = Some(Leaving { act, age: 0.0 });
        }
    }

    /// One original frame.
    pub fn frame(&mut self, f: &Frame) {
        let dt = WALK_FRAME * f.speed;
        self.stand_place = self.stand.step(self.stand_place, f.speed);
        // (the walking loop starts over when the walker first sets its weight)
        if f.walker.is_some_and(|w| w.speed_fraction.is_some()) && self.walk_weight == 0.0 {
            self.walk_place = 0.0;
        }
        self.walk_place = self.walk.step(self.walk_place, f.speed);
        // the action: a new play replaces the one before, which fades out; a Sim that starts a
        // route lets its action go
        if let Some(a) = f.action {
            if a.serial != self.last_serial {
                self.last_serial = a.serial;
                self.end_action();
                self.walk_weight = 0.0;
                self.acting = Some(Acting { anim: a.anim, place: a.place, backward: a.backward });
            } else if let Some(act) = &mut self.acting {
                act.place = a.place;
            }
        } else {
            // (no play any more: the Sim's interaction is over)
            self.end_action();
        }
        if let Some(w) = f.walker {
            if w.starts {
                self.end_action();
            }
            if let Some(v) = w.speed_fraction {
                self.walk_weight = v;
            }
        }
        // the turn on the spot: a direction that flips starts the animation over
        let turn = f.walker.and_then(|w| w.turn);
        match (&mut self.turning, turn) {
            (Some(t), Some(way)) => {
                if t.way != way {
                    t.way = way;
                    t.place = 0.0;
                }
                t.fade = (t.fade + dt / TURN_FADE).min(1.0);
            }
            (Some(t), None) => t.fade -= dt / TURN_FADE,
            (None, Some(way)) => self.turning = Some(Turning { way, place: 0.0, fade: (dt / TURN_FADE).min(1.0) }),
            (None, None) => {}
        }
        if let Some(t) = self.turning {
            if t.fade <= 0.0 && turn.is_none() {
                self.turning = None;
            } else {
                let clip = self.turn_clip(t.way);
                self.turning = Some(Turning { place: clip.step(t.place, f.speed), ..t });
            }
        }
        if let Some(l) = &mut self.leaving {
            l.age += dt;
            if l.age / ACTION_FADE >= 1.0 {
                self.leaving = None;
            }
        }
        self.carrying = f.carrying;
        if f.carrying {
            self.carry_place = self.carry.step(self.carry_place, f.speed);
        } else {
            self.carry_place = 0.0;
        }
        self.mood = f.mood;
    }

    /// The tracks to pose now, in the game's order, the hidden ones left out.
    pub fn poses(&self, cat: &Catalog) -> Vec<Posed> {
        let mut out: Vec<Posed> = Vec::new();
        // a base track at weight 1 hides every one before it
        let base = |out: &mut Vec<Posed>, p: Posed| {
            if p.weight >= 1.0 {
                out.clear();
            }
            out.push(p);
        };
        base(&mut out, Posed { slot: Slot::Standing, anim: self.stand.id, place: self.stand_place, weight: 1.0 });
        if self.walk_weight > 0.0 {
            // (a walker at its top: the division's rounding is not a lesser weight)
            let weight = if self.walk_weight > 0.999_999 { 1.0 } else { self.walk_weight };
            base(&mut out, Posed { slot: Slot::Walking, anim: self.walk.id, place: self.walk_place, weight });
        }
        if let Some(t) = self.turning {
            base(&mut out, Posed { slot: Slot::Turning, anim: self.turn_clip(t.way).id, place: t.place, weight: smooth(t.fade) });
        }
        let place_of = |a: &Acting| cat.by_id.get(&a.anim).map_or(a.place, |x| pose_place(x.frames, a.place, a.backward));
        if let Some(a) = &self.acting {
            base(&mut out, Posed { slot: Slot::Action, anim: a.anim, place: place_of(a), weight: 1.0 });
        }
        if let Some(l) = &self.leaving {
            base(&mut out, Posed { slot: Slot::Fading, anim: l.act.anim, place: place_of(&l.act), weight: 1.0 - smooth(l.age / ACTION_FADE) });
        }
        // the carrying loop and the face are masked: they hide nothing
        if self.carrying {
            out.push(Posed { slot: Slot::Carrying, anim: self.carry.id, place: self.carry_place, weight: 1.0 });
        }
        out.push(Posed { slot: Slot::Face, anim: self.face, place: 0.0, weight: (self.mood / 100.0).clamp(0.0, 1.0) });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_turn_fades_in_by_the_logged_weights() {
        // the logged weights of the turn track's first frames (home-tracks.log)
        let step = WALK_FRAME / TURN_FADE;
        let logged = [0.0727, 0.2548, 0.4925, 0.7318, 0.9188, 0.9997];
        for (n, want) in logged.iter().enumerate() {
            let got = smooth((n + 1) as f32 * step);
            assert!((got - want).abs() < 1e-4, "frame {}: {got} against {want}", n + 1);
        }
    }

    #[test]
    fn the_action_before_fades_out_in_three_frames() {
        let step = WALK_FRAME / ACTION_FADE;
        let logged = [0.7452, 0.2682, 0.0003];
        for (n, want) in logged.iter().enumerate() {
            let got = 1.0 - smooth((n + 1) as f32 * step);
            assert!((got - want).abs() < 1e-4, "frame {}: {got} against {want}", n + 1);
        }
        assert!(4.0 * step >= 1.0, "gone on the fourth");
    }

    #[test]
    fn a_turn_goes_the_way_of_its_gap() {
        assert_eq!(Turn::of_gap(-1.5), Turn::Clockwise);
        assert_eq!(Turn::of_gap(1.5), Turn::CounterClockwise);
        // no gap: the arrival frame's counter clockwise
        assert_eq!(Turn::of_gap(0.0), Turn::CounterClockwise);
    }
}
