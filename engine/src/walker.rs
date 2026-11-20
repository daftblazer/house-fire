//! A walker: a Sim walking a plan's points a frame at a time, as the original's route follower
//! does (`docs/simulation.md`, "The walker's step", "A walk's start and end", "The turn at a
//! walk's end", "A routing primitive's return"). Built from the pieces `routing` checks against
//! the log: `walk_step` (the ramp, the brake), `along_line`, `turn_towards`, `turn_frames`,
//! `facing_direction`.
//!
//! Not modelled: the run on after a door's walk (state 5, the walker's object data 9) and the
//! game's other speeds (every frame is the normal speed, factor 1).

use crate::routing::{along_line, direction_angle, facing_direction, turn_frames, turn_towards, walk_rates, walk_step, WALK_FRAME};

/// Where a walker is in its walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Walking its line (the follower's states 1 and 2).
    Walking,
    /// Arrived, turning to face what it walked to: the frames still to run (state 3).
    Turning(u32),
    /// Done (state 4): the next frame lets it go.
    Done,
    /// Let go: the routing primitive returns at its next turn.
    Released,
}

/// A route follower on one walk.
#[derive(Clone, Debug)]
pub struct Walker {
    /// The line it walks, sixteenths.
    line: Vec<(f32, f32)>,
    /// The point past the line's end it turns to face afterwards (the plan's point after its goal).
    faced: Option<(f32, f32)>,
    length: f32,
    gone: f32,
    /// Its speed in sixteenths a frame, carried into the next walk (`speed`).
    speed: f32,
    top: f32,
    growth: f32,
    /// The angle it faces (radians, 0 towards +y, growing towards -x).
    angle: f32,
    /// The way of its last step, to turn from at the end.
    last_way: (f32, f32),
    phase: Phase,
    frames: u32,
    /// Where it is, sixteenths.
    pub at: (f32, f32),
    /// Its direction (object data 1), as the route follower gives its person after each frame.
    pub direction: i16,
}

impl Walker {
    /// A walk along `line` (at least its start), turning afterwards to face `faced` if given; the
    /// walker starts at `speed` (what its last walk left), facing `direction` (its person's),
    /// with the top speed `top` a second (`routing::walk_top`).
    pub fn new(line: Vec<(f32, f32)>, faced: Option<(f32, f32)>, speed: f32, direction: i16, top: f32) -> Self {
        let length = line.windows(2).map(|p| f32::hypot(p[1].0 - p[0].0, p[1].1 - p[0].1)).sum();
        let (top, growth) = walk_rates(top);
        let at = line.first().copied().unwrap_or_default();
        Walker { line, faced, length, gone: 0.0, speed, top, growth, angle: direction_angle(direction), last_way: (0.0, 0.0), phase: Phase::Walking, frames: 0, at, direction }
    }

    /// The speed it carries into its next walk.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Whether it has been let go: its routing primitive returns at its next turn.
    pub fn released(&self) -> bool {
        self.phase == Phase::Released
    }

    /// The way the walk goes at the distance `gone` along it (the leg it is on; the last when
    /// past the end).
    fn leg(&self, gone: f32) -> (f32, f32) {
        let mut left = gone;
        let mut way = (0.0, 0.0);
        for p in self.line.windows(2) {
            way = (p[1].0 - p[0].0, p[1].1 - p[0].1);
            let len = f32::hypot(way.0, way.1);
            if len > 0.0 && left < len {
                break;
            }
            left -= len;
        }
        way
    }

    /// One frame of the follower, at the normal speed.
    pub fn frame(&mut self) {
        let budget = WALK_FRAME;
        match self.phase {
            Phase::Walking => {
                let way = if self.frames == 0 { self.line.windows(2).map(|p| (p[1].0 - p[0].0, p[1].1 - p[0].1)).find(|w| w.0 != 0.0 || w.1 != 0.0).unwrap_or_default() } else { self.leg(self.gone) };
                self.angle = turn_towards(self.angle, way, self.frames > 0, budget, 1.0);
                let left = self.length - self.gone;
                let (v, s) = walk_step(self.speed, self.top, self.growth, 1.0, left);
                self.speed = v;
                let before = self.at;
                self.gone += s.min(left);
                self.at = along_line(&self.line, 0, 0.0, self.gone);
                if s > 0.0 {
                    self.last_way = (self.at.0 - before.0, self.at.1 - before.1);
                }
                if self.length - self.gone <= 1e-4 {
                    // arrived: it turns from its last step's way to the point it faces
                    let angle = self.faced.map_or(0.0, |f| {
                        let a1 = self.last_way.1.atan2(self.last_way.0);
                        let a2 = (f.1 - self.at.1).atan2(f.0 - self.at.0);
                        let pi = std::f32::consts::PI;
                        ((a2 - a1 + pi).rem_euclid(2.0 * pi) - pi).abs()
                    });
                    self.phase = Phase::Turning(turn_frames(angle));
                }
            }
            Phase::Turning(n) => {
                if let Some(f) = self.faced {
                    self.angle = turn_towards(self.angle, (f.0 - self.at.0, f.1 - self.at.1), false, budget, 1.0);
                }
                self.phase = if n <= 1 { Phase::Done } else { Phase::Turning(n - 1) };
            }
            Phase::Done => self.phase = Phase::Released,
            Phase::Released => return,
        }
        if self.phase != Phase::Released {
            self.direction = facing_direction(self.angle);
        }
        self.frames += 1;
    }
}
