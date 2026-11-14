//! The scene: where the lot's objects are drawn (`docs/assets.md`, "An object's own matrix").
//!
//! An object of several tiles is drawn once, for its leader: at the centre of its parts' cells'
//! bounding box (a cell is a tile, or a quarter tile for a group placed off the tile grid, as
//! trees are; an object of one tile is drawn at its tile's centre), turned about the vertical
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
            let on_grid = ps.len() == 1 || ps.iter().all(|p| p.x16 % 16 == 0 && p.y16 % 16 == 0);
            let (cell, half) = if on_grid { (16, 0.5) } else { (1, 0.125) };
            let span = |v: fn(&&Part) -> i32| {
                let (lo, hi) = ps.iter().map(|p| v(p).div_euclid(cell) * cell).fold((i32::MAX, i32::MIN), |(a, b), c| (a.min(c), b.max(c)));
                (lo + hi) as f32 / 32.0 + half
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_is_drawn_at_its_box_centre() {
        // a bed of six tiles (x 13-15, y 26-27) facing 2, and a tree on the quarter grid
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
