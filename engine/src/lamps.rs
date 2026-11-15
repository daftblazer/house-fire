//! The lamps' lights placed from the engine's own data (`docs/assets.md`, "The lamps' lights").
//!
//! An object with a light plays one still animation when it is idle (its resources' idle
//! animation, `quickdata::ObjResources::idle`: two frames, every channel constant). The nodes of
//! that animation that carry the colour, factor and radius channels (words 4 to 6, the pose's
//! flags 0x20, 0x40 and 0x80) are its lights. The pose of every node is built from the animation,
//! the skeleton's bones are computed from the poses on the object's own matrix, and a light sits
//! at its node's bone, with the node's colour, factor and radius.

use std::collections::BTreeMap;

use crate::anim::Anim;
use crate::disc::Disc;
use crate::light::lamp_light;
use crate::pose::{Pose, TrackStreams};
use crate::quickdata::ObjResources;
use crate::scene::Placement;
use crate::skeleton::{self, CharNode, IDENTITY, Mat4, NodeState, PoseMode, Skeleton};

/// The pose flags a node has when its animation gives the light channels (4 to 6: colour,
/// factor and radius, the flags 0x20, 0x40 and 0x80 of `pose::Pose`).
pub const LIGHT_CHANNELS: u32 = 0xE0;

/// The place in a still animation's track the idle pose is read at (every channel is constant,
/// so any place gives the same pose).
const IDLE_PLACE: f32 = 0.0;

/// A lamp's light placed on its object: the node it hangs from, where it is (tiles, the third the
/// height), its colour (the node's by its factor) and its reach (the light's radius twice, by the
/// get-light's quarter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampLight {
    pub node: usize,
    pub at: [f32; 3],
    pub colour: [f32; 3],
    pub reach: f32,
}

/// An object's own matrix on the lot (rows, the translation last): its place (tiles) and its
/// turn about the vertical in degrees (`scene::Placement`, 90 - 45 x facing). The game builds it
/// from the facing's angle theta = 45 x facing degrees (in single precision, by the radians
/// constant) as (sin theta, cos theta) over (cos theta, -sin theta): a turn with its second row
/// mirrored. Its sines and cosines keep their rounding (cos of 270 degrees is 1.19e-8, sin of 180
/// is -8.74e-8); checked against the logged matrices of the lot's lamps (value for value: a
/// signed zero is not told apart).
pub fn object_matrix(x: f32, y: f32, angle: f32) -> Mat4 {
    let (s, c) = (90.0 - angle).to_radians().sin_cos();
    let mut m = IDENTITY;
    m[0] = s;
    m[1] = c;
    m[4] = c;
    m[5] = -s;
    m[12] = x;
    m[13] = y;
    m
}

/// The poses of every node of `anim` at its idle place; `None` if a node has a scalar stream
/// (not decoded: lamps have constants).
pub fn idle_poses(anim: &Anim) -> Option<Vec<Pose>> {
    let mut streams = TrackStreams::new(anim);
    (0..anim.nodes.len()).map(|i| streams.local(anim, i, IDLE_PLACE, 0).ok()).collect()
}

/// The lights of an object idle in `anim` on `skeleton` (the bones' own count of nodes), with the
/// object's matrix `root`: one for each node with the light channels. Empty for an object with
/// none, or whose animation's nodes do not fit its skeleton.
pub fn lights(skeleton: &Skeleton, anim: &Anim, root: &Mat4) -> Vec<LampLight> {
    let Some(poses) = idle_poses(anim).filter(|p| p.len() == skeleton.bones.len()) else {
        return Vec::new();
    };
    if poses.iter().all(|p| p.flags & LIGHT_CHANNELS != LIGHT_CHANNELS) {
        return Vec::new();
    }
    let nodes: Vec<CharNode> = skeleton.bones.iter().map(CharNode::from_bone).collect();
    let states: Vec<NodeState> = poses.iter().map(|p| NodeState { flags: p.flags, rotation: p.rotation, translation: p.translation, scale: p.scale }).collect();
    // the controller's mode word is not 0 for these objects (the logged bones'); its scale only
    // scales the matrices' 3 x 3, not the translations the lights sit at
    let bones = skeleton::bone_matrices(&nodes, &states, root, PoseMode { through_rest: true, scale: 1.0 });
    poses
        .iter()
        .enumerate()
        .filter(|(_, p)| p.flags & LIGHT_CHANNELS == LIGHT_CHANNELS)
        .map(|(node, p)| {
            let (colour, reach) = lamp_light(p.vector, p.scalar_a, p.scalar_b);
            LampLight { node, at: [bones[node][12], bones[node][13], bones[node][14]], colour, reach }
        })
        .collect()
}

/// What the lamps are made from on the disc: every skeleton (`characte.arc`, by the model's
/// id) and animation (`animatio.arc`, by id).
#[derive(Default)]
pub struct Sources {
    pub skeletons: BTreeMap<u32, Skeleton>,
    pub anims: BTreeMap<u32, Anim>,
}

impl Sources {
    pub fn read(disc: &Disc) -> Sources {
        fn all<T>(disc: &Disc, name: &str, read: impl Fn(&[u8]) -> Option<T>) -> BTreeMap<u32, T> {
            let b = disc.read(name).unwrap_or_default();
            crate::arc::entries(&b).unwrap_or_default().iter().filter_map(|e| Some((e.id, read(crate::arc::bytes(&b, e))?))).collect()
        }
        Sources { skeletons: all(disc, "characte.arc", skeleton::read), anims: all(disc, "animatio.arc", crate::anim::read) }
    }
}

/// A lamp object of the lot: its leader's id, where it is drawn and its lights.
#[derive(Clone, Debug, PartialEq)]
pub struct Lamp {
    pub leader: i16,
    pub place: Placement,
    pub lights: Vec<LampLight>,
}

/// The lamps among the lot's drawn objects: each placement whose leader (`guid_of`, its GUID)
/// has a model, a skeleton and an idle animation with light nodes.
pub fn on_lot(placements: &[Placement], guid_of: impl Fn(i16) -> Option<u32>, resources: &BTreeMap<u32, ObjResources>, sources: &Sources) -> Vec<Lamp> {
    placements
        .iter()
        .filter_map(|p| {
            let r = resources.get(&guid_of(p.leader)?)?;
            let (skeleton, anim) = (sources.skeletons.get(&r.model)?, sources.anims.get(&r.idle)?);
            let lights = lights(skeleton, anim, &object_matrix(p.x, p.y, p.angle));
            (!lights.is_empty()).then_some(Lamp { leader: p.leader, place: *p, lights })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_matrix_turns_and_mirrors() {
        let m = object_matrix(17.5, 24.5, 90.0);
        assert_eq!([m[0], m[1], m[4], m[5]], [0.0, 1.0, 1.0, 0.0]);
        // facing 6 (-180 degrees): cos of 270 degrees is not quite 0
        let m = object_matrix(0.0, 0.0, -180.0);
        assert_eq!([m[0], m[1], m[4], m[5]], [-1.0, 1.1924881e-8, 1.1924881e-8, 1.0]);
        assert_eq!((m[12], m[13], m[15]), (17.5, 24.5, 1.0));
    }
}
