//! Picking: what a click on the picture points at. Each candidate's place (the Sims and the
//! objects drawn) is projected through the camera and the nearest within a radius (in pixels of
//! the picture) that the caller accepts is taken; with none, the click is a ray from the eye
//! through the point, cut with the floor's plane.

use crate::frame::Mat3x4;
use housefire::scene::Camera;

/// How near a click must be to an object's projected place, in pixels of the 640x448 picture.
pub const PICK_RADIUS: f32 = 24.0;
/// The heights (tiles above the floor) the places are projected at: about a Sim's chest, and the
/// middle of an object (chosen: the logs hold no object heights).
pub const SIM_HEIGHT: f32 = 1.0;
pub const OBJECT_HEIGHT: f32 = 0.5;

/// What a click points at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// An object, by id.
    Object(i16),
    /// The floor at a point of the lot (tiles).
    Floor([f32; 2]),
    /// Nothing: off the lot.
    Nothing,
}

/// A point of the lot on the picture, in pixels of a picture of `size`; none behind the eye.
pub fn project(view: &Mat3x4, projection: &[f32; 6], p: [f32; 3], size: (f32, f32)) -> Option<(f32, f32)> {
    Camera::on_screen(view, projection, p).map(|s| (s[0] * size.0, s[1] * size.1))
}

/// Where the ray through a pixel meets the floor (the plane z = 0): the view's rows are the
/// camera's axes in the lot's coordinates, so the pixel's direction in the camera's space is
/// turned back through them.
pub fn floor_at(view: &Mat3x4, projection: &[f32; 6], pixel: (f32, f32), size: (f32, f32)) -> Option<[f32; 2]> {
    let (nx, ny) = (pixel.0 / size.0 * 2.0 - 1.0, 1.0 - pixel.1 / size.1 * 2.0);
    // clip x = p0 e0 + p1 e2 and y = p2 e1 + p3 e2 over w = -e2: at e2 = -1
    let e = [(nx + projection[1]) / projection[0], (ny + projection[3]) / projection[2], -1.0];
    let axis = |r: usize| [view[4 * r], view[4 * r + 1], view[4 * r + 2]];
    let dir = [0, 1, 2].map(|k| axis(0)[k] * e[0] + axis(1)[k] * e[1] + axis(2)[k] * e[2]);
    // the eye: the view maps it to the origin
    let origin = [0, 1, 2].map(|k| -(axis(0)[k] * view[3] + axis(1)[k] * view[7] + axis(2)[k] * view[11]));
    if dir[2] >= -1e-6 {
        return None;
    }
    let s = -origin[2] / dir[2];
    (s > 0.0).then(|| [origin[0] + s * dir[0], origin[1] + s * dir[1]])
}

/// The object whose place is nearest the pixel and within `PICK_RADIUS`, among those `clickable`
/// says (the ones with a menu); else the floor under it if that is on the lot (`lot` tiles).
pub fn pick(candidates: &[(i16, [f32; 3])], view: &Mat3x4, projection: &[f32; 6], pixel: (f32, f32), size: (f32, f32), lot: (f32, f32), mut accept: impl FnMut(i16) -> bool) -> Pick {
    let mut near: Vec<(f32, i16)> = candidates
        .iter()
        .filter_map(|&(id, at)| {
            let (sx, sy) = project(view, projection, at, size)?;
            let d = f32::hypot(sx - pixel.0, sy - pixel.1);
            (d <= PICK_RADIUS).then_some((d, id))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some(&(_, id)) = near.iter().find(|&&(_, id)| accept(id)) {
        return Pick::Object(id);
    }
    match floor_at(view, projection, pixel, size) {
        Some(p) if p[0] > 0.0 && p[1] > 0.0 && p[0] < lot.0 && p[1] < lot.1 => Pick::Floor(p),
        _ => Pick::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> (Mat3x4, [f32; 6]) {
        let c = Camera { target: [20.0, 20.0, 0.0], distance: 20.0, rotation: 30.0, tilt: 40.0 };
        (c.view(), Camera::projection(c.distance))
    }

    #[test]
    fn the_floor_point_projects_back_to_its_pixel() {
        let (view, proj) = camera();
        let size = (640.0, 448.0);
        for p in [[20.0, 20.0], [15.0, 24.5], [28.0, 12.0]] {
            let (px, py) = project(&view, &proj, [p[0], p[1], 0.0], size).unwrap();
            let back = floor_at(&view, &proj, (px, py), size).unwrap();
            assert!((back[0] - p[0]).abs() < 0.01 && (back[1] - p[1]).abs() < 0.01, "{p:?} came back as {back:?}");
        }
    }

    #[test]
    fn the_target_is_the_middle_of_the_picture() {
        let (view, proj) = camera();
        let (px, py) = project(&view, &proj, [20.0, 20.0, 0.0], (640.0, 448.0)).unwrap();
        assert!((px - 320.0).abs() < 0.5 && (py - 224.0).abs() < 0.5);
    }
}
