//! The scene: where the lot's objects are drawn (`docs/assets.md`, "An object's own matrix").
//!
//! An object of several tiles is drawn once, for its leader: at the centre of the bounding box of
//! its parts' tiles (each part's tile the one its place is on, wherever on it: a tree's parts a
//! quarter tile apart, a pool table's a sixteenth off the grid), turned about the vertical
//! by 90 - 45 x the leader's facing (degrees). An object hung on a wall (any of its parts' quick
//! data flagged so) is drawn 0.45 tile towards the wall: (0, -0.45) in its own frame.

use std::collections::BTreeMap;

/// An object's part as the world has it: its id, its leader's (its own when it leads or stands
/// alone), its place in sixteenths of a tile and its facing (0 to 7).
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub id: i16,
    pub leader: i16,
    pub x16: i32,
    pub y16: i32,
    pub facing: i32,
    /// Hung on a wall (`quickdata::ObjResources::on_wall`).
    pub wall: bool,
}

/// Where an object is drawn: its leader, the place (tiles) and the turn (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub leader: i16,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
}

/// The placements of the objects whose parts are given, one for each leader.
pub fn placements(parts: &[Part]) -> Vec<Placement> {
    let mut groups: BTreeMap<i16, Vec<&Part>> = BTreeMap::new();
    for p in parts {
        groups.entry(p.leader).or_default().push(p);
    }
    groups
        .into_iter()
        .map(|(leader, ps)| {
            let lead = ps.iter().find(|p| p.id == leader).unwrap_or(&ps[0]);
            let span = |v: fn(&&Part) -> i32| {
                let (lo, hi) = ps.iter().map(|p| v(p).div_euclid(16)).fold((i32::MAX, i32::MIN), |(a, b), c| (a.min(c), b.max(c)));
                (lo + hi) as f32 / 2.0 + 0.5
            };
            let angle = (90 - 45 * lead.facing) as f32;
            let (mut x, mut y) = (span(|p| p.x16), span(|p| p.y16));
            if ps.iter().any(|p| p.wall) {
                let (s, c) = angle.to_radians().sin_cos();
                x += 0.45 * s;
                y -= 0.45 * c;
            }
            Placement { leader, x, y, angle }
        })
        .collect()
}

/// A piece of a lot's walls as the original draws it: at (x, y) (tiles), turned `angle` degrees;
/// a wall's part on one side (at the tile's centre, its back to the wall: the frame of an object
/// hung on that wall) or a fence's one piece on the edge, along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallPiece {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub fence: bool,
    /// A wall part's pattern (`Walls::pattern`, the side it faces), a fence's style.
    pub pattern: u8,
    /// A wall part on an outdoor tile draws a cap besides its face (6 vertices, the plain image;
    /// drawn whichever way it faces).
    pub cap: bool,
    /// The edge's door or window (its face drawn with the `_door` or `_window` model).
    pub opening: crate::routing::Opening,
    /// The model's form at an outer corner (`_left_extends`, `_right_extends`).
    pub form: Form,
}

/// A wall part's form: straight, or reaching past one end to meet the wall round an outer
/// corner: `_left_extends` at its end towards +t, `_right_extends` towards -t, t = (cos a, sin a)
/// for its turn a.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Form {
    #[default]
    Straight,
    LeftExtends,
    RightExtends,
}

/// The form of the part on tile (px, py) whose wall is towards (wx, wy): at an end e (along the
/// wall), an outer corner is a wall on the far tile's edge towards e, with none on the part's own
/// tile's edge towards e and the wall not going on past e.
fn form(px: i32, py: i32, wx: i32, wy: i32, solid: &dyn Fn(i32, i32, i32, i32) -> bool) -> Form {
    // t = (cos a, sin a) for the part's turn: its normal (-sin a, cos a) is (-wx, -wy), so t is
    // (-wy, wx)
    let corner = |ex: i32, ey: i32| solid(px + wx, py + wy, ex, ey) && !solid(px, py, ex, ey) && !solid(px + ex, py + ey, wx, wy);
    if corner(-wy, wx) {
        Form::LeftExtends
    } else if corner(wy, -wx) {
        Form::RightExtends
    } else {
        Form::Straight
    }
}

impl WallPiece {
    /// Whether a wall part's face is seen from `eye` (x, y in tiles): faces are one-sided, the
    /// original drawing a part's face only when it is turned towards the camera (its normal,
    /// (-sin a, cos a), pointing at the eye) and in view; the renderer culls back faces likewise.
    pub fn faces(&self, eye: [f32; 2]) -> bool {
        let (s, c) = self.angle.to_radians().sin_cos();
        -s * (eye[0] - self.x) + c * (eye[1] - self.y) > 0.0
    }
}

/// The pieces of the walls between the tiles of a `size`-tile lot: each edge with a wall once.
pub fn wall_pieces(walls: &crate::routing::Walls, size: i32, fences: &[u32]) -> Vec<WallPiece> {
    let mut out = Vec::new();
    let solid = |x: i32, y: i32, dx: i32, dy: i32| walls.wall_between(x, y, dx, dy) && !fences.contains(&u32::from(walls.style(x, y, dx, dy)));
    for y in 0..size {
        for x in 0..size {
            for (dx, dy) in [(1, 0), (0, 1)] {
                if !walls.wall_between(x, y, dx, dy) {
                    continue;
                }
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let style = walls.style(x, y, dx, dy);
                if fences.contains(&u32::from(style)) {
                    let angle = if dx == 1 { 90.0 } else { 0.0 };
                    out.push(WallPiece { x: cx + dx as f32 / 2.0, y: cy + dy as f32 / 2.0, angle, fence: true, pattern: style, cap: false, opening: walls.opening(x, y, dx, dy), form: Form::Straight });
                } else {
                    let (here, there) = if dx == 1 { (90.0, -90.0) } else { (180.0, 0.0) };
                    out.push(WallPiece { x: cx, y: cy, angle: here, fence: false, pattern: walls.pattern(x, y, dx, dy), cap: !walls.indoors(x, y), opening: walls.opening(x, y, dx, dy), form: form(x, y, dx, dy, &solid) });
                    let (nx, ny) = (x + dx, y + dy);
                    out.push(WallPiece { x: cx + dx as f32, y: cy + dy as f32, angle: there, fence: false, pattern: walls.pattern(nx, ny, -dx, -dy), cap: !walls.indoors(nx, ny), opening: walls.opening(x, y, dx, dy), form: form(nx, ny, -dx, -dy, &solid) });
                }
            }
        }
    }
    out
}

/// A run of floor along a row, as the original draws it (one strip a run): tiles x .. x + n - 1
/// of row y, with pattern `value`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FloorRun {
    pub y: i32,
    pub x: i32,
    pub n: i32,
    pub value: u8,
}

/// A lot's floor runs: along each row, each stretch of tiles of one pattern (not a value past the
/// list); a tile of 255 (half a tile, cut by a diagonal wall) just before a stretch is drawn as its
/// first tile.
pub fn floor_runs(walls: &crate::routing::Walls, size: i32, patterns: &[crate::quickdata::FloorPattern]) -> Vec<FloorRun> {
    let mut out = Vec::new();
    for y in 0..size {
        let mut x = 0;
        while x < size {
            let v = walls.floor(x, y);
            if v == 0 || patterns.get(usize::from(v)).is_none() {
                x += 1;
                continue;
            }
            let start = x;
            while x < size && walls.floor(x, y) == v {
                x += 1;
            }
            let first = if start > 0 && walls.floor(start - 1, y) == 255 { start - 1 } else { start };
            out.push(FloorRun { y, x: first, n: x - first, value: v });
        }
    }
    out
}

/// The lot's camera as the original keeps it (`ESimsCam`'s parameters): where it looks (the target,
/// its z 0.25 above the floor), from how far, turned how far about the vertical and tilted how far
/// up (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: [f32; 3],
    pub distance: f32,
    pub rotation: f32,
    pub tilt: f32,
}

impl Camera {
    /// The eye: from the target, along (-1, -1, 0) / sqrt 2 tilted up by `tilt` (about the
    /// horizontal axis across it) and turned by `rotation` about the vertical, `distance` away.
    pub fn eye(&self) -> [f32; 3] {
        let (st, ct) = self.tilt.to_radians().sin_cos();
        let (sr, cr) = self.rotation.to_radians().sin_cos();
        let h = -ct / std::f32::consts::SQRT_2;
        let (x, y) = (h * cr - h * sr, h * sr + h * cr);
        [self.target[0] + self.distance * x, self.target[1] + self.distance * y, self.target[2] + self.distance * st]
    }

    /// The zoom's nearest distance and its tilt there; the farthest distance (0.65 of it in the
    /// original's other case, `fn_800655C4`) and its tilt.
    pub const NEAR: f32 = 8.0;
    pub const FAR: f32 = 35.0;
    pub const NEAR_TILT: f32 = 25.0;
    pub const FAR_TILT: f32 = 56.0;
    /// The turn's speed at a full stick, degrees a second.
    pub const TURN_SPEED: f32 = 65.0;

    /// The wanted camera zoomed by the stick's `input` (in, positive: nearer by `input`), the
    /// distance kept within the zoom (`far` its far end), the tilt following it in proportion.
    pub fn zoom(&mut self, input: f32, far: f32) {
        self.distance -= input;
        self.distance = if self.distance < Self::NEAR { Self::NEAR } else if self.distance > far { far } else { self.distance };
        self.tilt = (self.distance - Self::NEAR) / (far - Self::NEAR) * (Self::FAR_TILT - Self::NEAR_TILT) + Self::NEAR_TILT;
    }

    /// The wanted camera turned by the stick `s` (-1..1, squared keeping its sign) for `dt`
    /// seconds; past 360 it goes to 0, below 0 to 360 (not wrapped by the excess).
    pub fn turn(&mut self, s: f32, dt: f32) {
        let input = if s < 0.0 { s * -s } else { s * s } * dt * Self::TURN_SPEED;
        if input == 0.0 {
            return;
        }
        self.rotation += input;
        self.rotation = if self.rotation < 0.0 {
            360.0
        } else if self.rotation > 360.0 {
            0.0
        } else {
            self.rotation
        };
    }

    /// One frame's easing of the live camera (`self`) towards the wanted one, by the frame's time
    /// step `dt` (seconds), as the original's usual mode (0): the target moves straight towards
    /// the wanted target at 5 times the distance a second (2.5 times within 1.5), the distance at
    /// twice its difference, the rotation (the short way round, kept in 0..360) at 3 times and
    /// the tilt at 2.5 times; each lands on the wanted value when the step would pass it or the
    /// difference is 0.0001 or less (an angle then not wrapped: a wanted 360 stays 360).
    pub fn ease(&mut self, wanted: &Camera, dt: f32) {
        let d = [0, 1, 2].map(|k| wanted.target[k] - self.target[k]);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len <= 0.0001 {
            self.target = wanted.target;
        } else {
            let rate = if len > 1.5 { 5.0 } else { 2.5 } * len;
            let step = (rate * dt).min(len);
            for (t, dk) in self.target.iter_mut().zip(d) {
                *t += dk * (1.0 / len) * step;
            }
        }
        // (false when the difference was 0.0001 or less: the value set, an angle not wrapped)
        fn towards(v: &mut f32, diff: f32, target: f32, speed: f32, dt: f32) -> bool {
            if diff.abs() <= 0.0001 {
                *v = target;
                return false;
            }
            let rate = diff * speed;
            if (rate * dt).abs() >= diff.abs() {
                *v = target;
            } else {
                *v += rate * dt;
            }
            true
        }
        let diff = wanted.distance - self.distance;
        let _ = towards(&mut self.distance, diff, wanted.distance, 2.0, dt);
        for (v, target, speed) in [(&mut self.rotation, wanted.rotation, 3.0), (&mut self.tilt, wanted.tilt, 2.5)] {
            let mut diff = target - *v;
            if target > *v {
                let other = 360.0 - target + *v;
                if other < diff {
                    diff = -other;
                }
            } else {
                let other = 360.0 - *v + target;
                if other < -diff {
                    diff = other;
                }
            }
            if !towards(v, diff, target, speed, dt) {
                continue;
            }
            while *v >= 360.0 {
                *v -= 360.0;
            }
            while *v < 0.0 {
                *v += 360.0;
            }
        }
    }

    /// The view (world to eye) as 3 rows of 4 (the rotation's row and its translation): a look
    /// from the eye at the target with z up, row 0 across (level), row 2 back towards the eye.
    pub fn view(&self) -> [f32; 12] {
        let e = self.eye();
        let b = [e[0] - self.target[0], e[1] - self.target[1], e[2] - self.target[2]];
        let n = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
        let back = [b[0] / n, b[1] / n, b[2] / n];
        let h = (back[0] * back[0] + back[1] * back[1]).sqrt();
        let across = [-back[1] / h, back[0] / h, 0.0];
        let up = [back[1] * across[2] - back[2] * across[1], back[2] * across[0] - back[0] * across[2], back[0] * across[1] - back[1] * across[0]];
        let mut v = [0.0; 12];
        for (r, row) in [across, up, back].iter().enumerate() {
            v[4 * r..4 * r + 3].copy_from_slice(row);
            v[4 * r + 3] = -(row[0] * e[0] + row[1] * e[1] + row[2] * e[2]);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_view_is_the_originals() {
        // the club log's view at tick 1,000 (3 rows of 4), from its parameters: target (19.545,
        // 19.901, 0.25), distance 8, rotation 151.97, tilt 25
        let logged = [-0.291_842_3, 0.956_466_3, 0.0, -13.330_891, -0.404_220_2, -0.123_337_89, 0.906_307_8, 10.128_328, 0.866_852_9, 0.264_498_92, 0.422_618_27, -30.311_815];
        let cam = Camera { target: [19.545, 19.901, 0.25], distance: 8.0, rotation: 151.97, tilt: 25.0 };
        for (a, b) in cam.view().iter().zip(logged) {
            assert!((a - b).abs() < 0.01, "{:?}", cam.view());
        }
    }

    #[test]
    fn the_camera_eases_as_the_originals() {
        // home's first frames (z towards the target's 0.25) and the club's wanted 360, logged
        let mut live = Camera { target: [22.414_213, 8.414_213, 0.039_548_434], distance: 8.0, rotation: 0.0, tilt: 25.0 };
        let wanted = Camera { target: [22.414_213, 8.414_213, 0.25], ..live };
        live.ease(&wanted, 0.033);
        assert!((live.target[2] - 0.056_910_69).abs() < 1e-6, "{live:?}");
        let mut live = Camera { target: [22.867_266, 21.888_386, 0.25], distance: 8.0, rotation: 0.0, tilt: 25.0 };
        live.ease(&Camera { rotation: 360.0, distance: 8.024_692, tilt: 25.028_349, ..live }, 0.033);
        assert_eq!(live.rotation, 360.0);
    }

    #[test]
    fn the_camera_zooms_and_turns_as_the_originals() {
        // home: a zoom out by 1 from 16.519 to the far end's side (logged 17.519, tilt 35.929);
        // the club: a turn below 0 goes to 360
        let mut c = Camera { target: [0.0; 3], distance: 16.519, rotation: 0.0, tilt: 34.781 };
        c.zoom(-1.0, Camera::FAR);
        assert!((c.distance - 17.519).abs() < 1e-3 && (c.tilt - 35.929).abs() < 1e-3, "{c:?}");
        c.turn(-0.5, 0.033);
        assert_eq!(c.rotation, 360.0);
        c.turn(1.0, 0.033);
        assert_eq!(c.rotation, 0.0);
    }

    #[test]
    fn a_wall_face_is_seen_from_its_side() {
        // home, tick 1,000: the eye at (32.29, -4.02); the west wall's inside face (13.5, 15.5)
        // turned -90 is seen, its outside (12.5, 15.5) turned 90 is not
        let eye = [32.29, -4.02];
        let part = |x, y, angle| WallPiece { x, y, angle, fence: false, pattern: 0, cap: false, opening: Default::default(), form: Form::Straight };
        assert!(part(13.5, 15.5, -90.0).faces(eye));
        assert!(!part(12.5, 15.5, 90.0).faces(eye));
        assert!(part(14.5, 13.5, 180.0).faces(eye));
        assert!(!part(14.5, 14.5, 0.0).faces(eye));
    }

    #[test]
    fn a_group_is_drawn_at_its_box_centre() {
        // a bed of six tiles (x 13-15, y 26-27) facing 2, and a tree's parts a quarter tile apart
        let bed: Vec<Part> = [(29, 14, 26), (34, 14, 27), (35, 13, 27), (36, 13, 26), (37, 15, 26), (38, 15, 27)]
            .iter()
            .map(|&(id, x, y)| Part { id, leader: 29, x16: x * 16, y16: y * 16, facing: 2, wall: false })
            .collect();
        let tree: Vec<Part> = [(153, 140, 140), (154, 140, 144), (155, 144, 144), (156, 144, 140)]
            .iter()
            .map(|&(id, x, y)| Part { id, leader: 153, x16: x, y16: y, facing: 0, wall: false })
            .collect();
        // a painting of two panels on a wall, facing 0 (its lead with no resources of its own)
        let painting = [Part { id: 137, leader: 137, x16: 304, y16: 352, facing: 0, wall: false }, Part { id: 138, leader: 137, x16: 304, y16: 368, facing: 0, wall: true }];
        let p = placements(&[bed, tree, painting.to_vec()].concat());
        assert_eq!(p[0], Placement { leader: 29, x: 14.5, y: 27.0, angle: 0.0 });
        assert_eq!(p[1], Placement { leader: 137, x: 19.95, y: 23.0, angle: 90.0 });
        assert_eq!(p[2], Placement { leader: 153, x: 9.0, y: 9.0, angle: 90.0 });
    }
}
