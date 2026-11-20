//! Where a sound is heard from: the position and volume the sound layer gives a voice
//! (`Snd_Params`' words 1 and 2: `x` and `volume`), worked out from the emitter's place against
//! the camera. Presentation side only.
//!
//! What the original's log shows (`docs/assets.md`, "The sound out"):
//! - A sound that is not in the world (the interface's, and the `dialog_*` voices) has
//!   `x = volume = 1`. The event's track says so: bit 15 of its flags word is clear.
//! - A sound of an object has `x = volume = 1/1024` while it is out of range (every object sound
//!   of the home log but one is, the camera being 27 or more tiles from the kitchen), and a
//!   volume that follows the camera's place when it is not (the lamp's, the Sim's vox): the
//!   params calls come each frame the camera moves, in steps of 7 to 11 over 1024.
//! - `x` goes to zero as the sound crosses the middle of the view and changes sign: a lateral
//!   offset.
//!
//! The rule here is the simplest that has these: the volume falls linearly from 1 at the eye to
//! the floor at a range that the event's track sets (its parameter word's low byte: 0x40 for
//! nearly every event, 0xC8 for `vox_meal_good`), in proportion, and `x` is the lateral offset over a fixed
//! 20 tiles. The constants are fitted to the few places the log has (named below), not read from
//! the game.

use crate::scene::Camera;

/// The volume of a sound too far to hear (and its `x`): 1/1024 (read: the first params call of
/// every object sound of the home log that is out of range).
pub const FLOOR: f32 = 1.0 / 1024.0;
/// Tiles of range for each unit of a track's parameter byte (0x40 for nearly every track, 0xC8
/// for `vox_meal_good`). Fitted to the home log: `vox_meal_good` has 0.454 at 24.0 tiles from the
/// eye (a range of 44 = 200 * 0.22), and the 0x40 tracks (a range of 14.1) are at the floor at
/// 19.6 tiles and more (the chops, the door, the chair at 24.0, the kitchen at 27.4).
pub const RANGE_PER_BYTE: f32 = 0.22;
/// The range of a track with no parameter word (0x40's).
pub const RANGE: f32 = 64.0 * RANGE_PER_BYTE;
/// `x` is the lateral offset (tiles, to the camera's right) over this: the vox's first call has
/// 6.58 tiles of offset and x 0.328.
pub const LATERAL_SCALE: f32 = 20.0;

/// The range of a track with parameter word `params` (its low byte).
pub fn range_of(params: u32) -> f32 {
    (params & 0xFF) as f32 * RANGE_PER_BYTE
}

/// Whether a track is heard from a place in the world: bit 15 of its flags word (`0800828A`; the
/// interface's and `dialog_*` tracks, `0800028A` and `0800029A`, have it clear).
pub fn positional(flags: u32) -> bool {
    flags & 0x8000 != 0
}

/// Where the sounds are heard from: the camera's eye and its right-hand direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub eye: [f32; 3],
    pub right: [f32; 3],
}

impl Listener {
    pub fn new(cam: &Camera) -> Listener {
        let eye = cam.eye();
        let f = [cam.target[0] - eye[0], cam.target[1] - eye[1]];
        let n = f[0].hypot(f[1]).max(1e-6);
        // right = forward x up, up being +z
        Listener { eye, right: [f[1] / n, -f[0] / n, 0.0] }
    }

    /// The position and volume of a sound at `at` (`x`, `volume`) for a track of range `range`.
    pub fn target(&self, at: [f32; 3], range: f32) -> (f32, f32) {
        let q = [0, 1, 2].map(|k| at[k] - self.eye[k]);
        let d = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
        let volume = (1.0 - d / range).clamp(FLOOR, 1.0);
        if volume <= FLOOR {
            return (FLOOR, FLOOR);
        }
        let lateral = q[0] * self.right[0] + q[1] * self.right[1] + q[2] * self.right[2];
        (lateral / LATERAL_SCALE, volume)
    }
}

/// The params call's words for a position and volume (word 0 the flags, 1 `x`, 2 `volume`, 3 the
/// no-fade flag as a float; `first` marks a sound's first call, which also sets the fade).
pub fn words(x: f32, volume: f32, first: bool, no_fade: bool) -> [u32; 8] {
    let mut w = [0u32; 8];
    w[0] = if first { 0xB } else { 3 };
    w[1] = x.to_bits();
    w[2] = volume.to_bits();
    w[3] = if no_fade { 1.0f32.to_bits() } else { 0 };
    w[5] = x.to_bits();
    w[6] = volume.to_bits();
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_is_the_floor_and_near_is_louder() {
        let cam = Camera { target: [23.85, 7.49, 0.25], distance: 17.67, rotation: 81.2, tilt: 36.1 };
        let l = Listener::new(&cam);
        assert_eq!(l.target([25.5, 20.5, 0.0], RANGE), (FLOOR, FLOOR));
        // zoomed in, the middle of the view is 8 tiles from the eye
        let near = Camera { distance: 8.0, tilt: 25.0, ..cam };
        let (_, v) = Listener::new(&near).target([near.target[0], near.target[1], 0.25], RANGE);
        assert!(v > 0.3, "{v}");
    }

    #[test]
    fn the_vox_points() {
        let cam = Camera { target: [23.850143, 7.4919868, 0.25], distance: 17.666666, rotation: 81.233734, tilt: 36.098763 };
        let (x, v) = Listener::new(&cam).target([25.5, 16.5, 0.0], range_of(0x640010C8));
        assert!((v - 0.454).abs() < 0.03 && (x - 0.328).abs() < 0.03, "{x} {v}");
    }

    #[test]
    fn tracks_are_positional_by_bit_15() {
        assert!(positional(0x0800828A) && !positional(0x0800028A) && !positional(0x0800029A));
    }
}
