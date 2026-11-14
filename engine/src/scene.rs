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
                    out.push(WallPiece { x: cx + dx as f32 / 2.0, y: cy + dy as f32 / 2.0, angle, fence: true, pattern: style, cap: false, opening: walls.opening(x, y, dx, dy) });
                } else {
                    let (here, there) = if dx == 1 { (90.0, -90.0) } else { (180.0, 0.0) };
                    out.push(WallPiece { x: cx, y: cy, angle: here, fence: false, pattern: walls.pattern(x, y, dx, dy), cap: !walls.indoors(x, y), opening: walls.opening(x, y, dx, dy) });
                    let (nx, ny) = (x + dx, y + dy);
                    out.push(WallPiece { x: cx + dx as f32, y: cy + dy as f32, angle: there, fence: false, pattern: walls.pattern(nx, ny, -dx, -dy), cap: !walls.indoors(nx, ny), opening: walls.opening(x, y, dx, dy) });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_face_is_seen_from_its_side() {
        // home, tick 1,000: the eye at (32.29, -4.02); the west wall's inside face (13.5, 15.5)
        // turned -90 is seen, its outside (12.5, 15.5) turned 90 is not
        let eye = [32.29, -4.02];
        let part = |x, y, angle| WallPiece { x, y, angle, fence: false, pattern: 0, cap: false, opening: Default::default() };
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
