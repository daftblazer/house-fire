use std::rc::Rc;

use crate::routing::{direction_angle, facing_direction, turn_gap, turn_towards, walk_top, WALK_FRAME};
use crate::planner::{choose_goal, coarse_way, point_after_goal, refine, Growth, MapRect};
use crate::tree_log::PlanRec;
use crate::world::{DATA_FACING, World};
use super::run::Replay;

/// A plan's points natively (logs with 'r' and 'h'): the planner's search on the rects and
/// settings the plan's last search had, from its coarse way's start to its end, refined, and the
/// point after the goal (none if the plan took no goal); (x, y) like the plan's points.
pub(super) fn native_plan_points(pl: &PlanRec, world: &World) -> Option<Vec<(i32, i32)>> {
    // a request with no goals (logs with 'h', which every plan of such a log has) plans nothing
    if pl.goals.is_empty() && pl.approach.is_some() {
        return Some(Vec::new());
    }
    if pl.coarse.len() < 2 || pl.lookup_regions[0].is_empty() {
        return None;
    }
    let rects: Vec<MapRect> = pl.lookup_regions.iter().flatten().map(MapRect::from_raw).collect();
    let growth = Growth { cap: (pl.grower[2] != 0).then_some(pl.grower[3] as i32), open_cost: pl.grower[4] as i32 };
    let &start = pl.coarse.first()?;
    plan_points(pl, world, &rects, growth, start)
}

/// A plan's points from the search's rects and settings (the log's, or the native ones of
/// `route_map`): the goal the request takes (its only one, or the backward search's of several),
/// the way to it, refined, and the point after the goal; (x, y) like the plan's points. The
/// search starts at `start` (x, y), the Sim's place.
pub(super) fn plan_points(pl: &PlanRec, world: &World, rects: &[MapRect], growth: Growth, (sx, sy): (i32, i32)) -> Option<Vec<(i32, i32)>> {
    let goals: Vec<(i32, i32, i32)> = pl.goals.iter().map(|g| (g[1] as i32, g[0] as i32, g[2] as i32)).collect();
    let pick = if goals.len() == 1 { 0 } else { choose_goal(rects, growth, (sy, sx), &goals)? };
    let (gy, gx, _) = *goals.get(pick)?;
    let way = coarse_way(rects, growth, (sy, sx), (gy, gx))?;
    let mut points: Vec<(i32, i32)> = refine(&way).iter().map(|&(y, x)| (x, y)).collect();
    if let (Some(a), true) = (pl.approach, pl.chosen >= 0) {
        let facing = world.find_object(a[5] as i16).and_then(|o| o.data.get(1)).map(|&d| d as i32);
        points.extend(point_after_goal(a[0], a[1], (a[3], a[4]), facing, a[2], &points));
    }
    Some(points)
}

/// What the run keeps of a route follower to turn its person (`Replay::turn_followers`): the
/// angle it faces, the plan it walks (an index into the log's) and the state it was in after its
/// last frame.
#[derive(Default)]
pub(super) struct Heading {
    /// The person it walks: the log's, else the one that stood where it did at its first frame.
    pub(super) person: Option<i16>,
    pub(super) angle: f32,
    /// The points of the route it follows, sixteenths.
    pub(super) route: Vec<(i32, i32)>,
    pub(super) state: u32,
}

/// The leg of a walk's points a walker at `place` is on: the index of the point it goes to (at
/// least 1), the first leg that has it, else the nearest. Legs of no length have none.
pub(super) fn leg_at(points: &[(i32, i32)], place: (f32, f32)) -> usize {
    let mut best = (f32::MAX, 1);
    for i in 1..points.len() {
        let (a, b) = ((points[i - 1].0 as f32, points[i - 1].1 as f32), (points[i].0 as f32, points[i].1 as f32));
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let len2 = ex * ex + ey * ey;
        if len2 == 0.0 {
            continue;
        }
        let u = (((place.0 - a.0) * ex + (place.1 - a.1) * ey) / len2).clamp(0.0, 1.0);
        let d2 = (place.0 - a.0 - u * ex).powi(2) + (place.1 - a.1 - u * ey).powi(2);
        if d2 < best.0 - 1e-6 {
            best = (d2, i);
        }
    }
    best.1
}

/// One frame of a route follower as `Replay::turn_followers` turned its person: what the figure
/// animator takes of a walker (`animator::WalkerFrame`).
#[derive(Clone, Copy, Debug)]
pub struct WalkFrame {
    pub tick: u32,
    pub follower: u32,
    /// The person it walks.
    pub person: i16,
    /// Its state before and after the frame (1 starting, 2 walking, 3 arrived, 4 done, 5 running on).
    pub before: u32,
    pub after: u32,
    /// The speed factor of the frame (1, or 5 in the fast mode) and the distance the frame moved
    /// it (sixteenths).
    pub factor: f32,
    pub step: f32,
    /// The walker's top a second (sixteenths): its body's age.
    pub top: f32,
    /// How far it had still to turn (radians, signed as `routing::turn_gap`) before the frame and
    /// after it; zero when it has no way to go.
    pub gap_before: f32,
    pub gap_after: f32,
    /// The gap before the frame not wrapped: the direction's of the turn animation.
    pub raw_before: f32,
}

impl<'l> Replay<'l> {
    /// Starts keeping the route followers' frames (`take_walk_frames`).
    pub fn trace_walks(&mut self) {
        self.walk_trace.get_or_insert_with(Vec::new);
    }

    /// The route followers' frames turned since the last call, in the log's order.
    pub fn take_walk_frames(&mut self) -> Vec<WalkFrame> {
        self.walk_trace.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// The tick's frames of the route followers (the log's 'Z' records; the walks and their
    /// plans are the log's) turn their persons: each follower keeps the angle it faces
    /// (`routing::turn_towards`, from the leg of its plan it is on, or in state 5 the way to its
    /// point), and a person in a state that walks or waits to turn (1 to 3) is given the
    /// direction of it (`routing::facing_direction`, object data 1) after each frame (Route_Follow,
    /// 800AE634). A route starts when a follower is in state 1 after another state: its angle is
    /// then its person's direction's, unless the last route ran on (state 5), and the person is
    /// given the direction of it (800AD1BC, 800AD5C4). A route whose plan the log has not (the
    /// original's own turn-only routes) is not turned.
    pub(super) fn turn_followers(&mut self) {
        let log = Rc::clone(&self.log);
        while self.heading_at < log.follows.len() && log.follows[self.heading_at].tick <= self.t {
            let f = &log.follows[self.heading_at];
            self.heading_at += 1;
            let heading = self.headings.entry(f.follower).or_default();
            let place = (f.before.x, f.before.y);
            if heading.person.is_none() {
                heading.person = if f.person >= 0 {
                    Some(f.person)
                } else {
                    // (the observer's id is -1 where the person is not in its list: the one
                    // person standing where the follower does, to a sixteenth)
                    let mut near = self.h.world.objects().filter(|o| o.person && o.on_lot() && (o.x16 as f32 - place.0).abs() < 1.0 && (o.y16 as f32 - place.1).abs() < 1.0);
                    near.next().map(|o| o.id()).filter(|_| near.next().is_none())
                };
            }
            let Some(key) = heading.person.and_then(|id| self.h.world.find(id)) else { continue };
            let mut direction = None;
            if f.before.state == 1 && heading.state != 1 {
                if heading.state != 5 {
                    heading.angle = direction_angle(self.h.world.get(key).facing());
                }
                direction = Some(facing_direction(heading.angle));
                // the route it starts: to face what its person was told to look towards this
                // tick (the goal twice, then the object's place), else the latest plan that
                // begins where it stands
                let start = ((place.0 + 0.5) as i32, (place.1 + 0.5) as i32);
                let upto = log.plans.partition_point(|p| p.tick <= f.tick);
                heading.route = match self.h.looks.get(&heading.person.unwrap_or(-1)) {
                    Some(&(tick, target)) if tick == f.tick => vec![start, start, target],
                    _ => log.plans[..upto].iter().rfind(|p| p.points.first() == Some(&start)).map(|p| p.points.clone()).unwrap_or_default(),
                };
            }
            heading.state = f.after.state;
            let points = &heading.route[..];
            let leg = |i: usize| (points[i].0 - points[i - 1].0, points[i].1 - points[i - 1].1);
            let way = match f.before.state {
                5 => Some((f.before.point.0 - place.0, f.before.point.1 - place.1)),
                _ if points.len() < 2 => None,
                1 => Some(leg(1)).map(|(x, y)| (x as f32, y as f32)),
                2 => Some(leg(leg_at(points, place))).map(|(x, y)| (x as f32, y as f32)),
                3 => Some(leg(points.len() - 1)).map(|(x, y)| (x as f32, y as f32)),
                _ => None,
            };
            let angle_before = heading.angle;
            if let Some(way) = way {
                heading.angle = turn_towards(heading.angle, way, f.before.state == 2, WALK_FRAME * f.before.factor, f.before.factor);
                if f.before.state != 5 {
                    direction = Some(facing_direction(heading.angle));
                }
            }
            if let Some(d) = direction
                && let Some(slot) = self.h.world.get_mut(key).data.get_mut(DATA_FACING)
            {
                *slot = d;
            }
            if let (Some(trace), Some(person)) = (&mut self.walk_trace, heading.person) {
                let age = self.h.world.get(key).file.and_then(|fi| self.lib.file(fi).strings.get(&200)).and_then(|t| t.first()).map(String::as_str);
                let gap = |angle| way.map_or(0.0, |w| turn_gap(angle, w));
                let raw = way.map_or(0.0, |(dx, dy)| if dx.abs() < 0.001 && dy.abs() < 0.001 { 0.0 } else { (-dx).atan2(dy) - angle_before });
                let step = ((f.after.x - f.before.x).powi(2) + (f.after.y - f.before.y).powi(2)).sqrt();
                trace.push(WalkFrame { tick: f.tick, follower: f.follower, person, before: f.before.state, after: f.after.state, factor: f.before.factor, step, top: walk_top(age), gap_before: gap(angle_before), gap_after: gap(heading.angle), raw_before: raw });
            }
        }
    }

    /// The route a follower is on, as the run has it (sixteenths): the engine's list for the
    /// study of the original's ('follower_lists').
    pub fn follower_route(&self, follower: u32) -> Option<&[(i32, i32)]> {
        self.headings.get(&follower).map(|h| &h.route[..]).filter(|r| !r.is_empty())
    }
}
