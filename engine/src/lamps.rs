//! The lamps' lights placed from the engine's own data (`docs/assets.md`, "The lamps' lights").
//!
//! An object with a light plays the animation its graphic number picks among its states
//! (`quickdata::ObjResources::animation`: the idle one, a lamp's "on", the dance floor's modes;
//! two still frames with every channel constant, or for the disco's lights a factor and radius
//! that run on scalar streams, `anim::ScalarStream`). The nodes of that animation
//! that carry the colour, factor and radius channels (words 4 to 6) are its lights. The pose of
//! every node is built from the animation, the skeleton's bones are computed from the poses on the
//! object's own matrix, and a light sits at its node's bone, with the node's colour, factor and
//! radius.

use std::collections::BTreeMap;

use crate::anim::{Anim, Channel, Node};
use crate::disc::Disc;
use crate::light::lamp_light;
use crate::pose::{Pose, TrackStreams};
use crate::quickdata::ObjResources;
use crate::scene::Placement;
use crate::skeleton::{self, CharNode, IDENTITY, Mat4, NodeState, PoseMode, Skeleton};

/// The place in a still animation's track the idle pose is read at (every channel is constant,
/// so any place gives the same pose).
const IDLE_PLACE: f32 = 0.0;

/// A node's words of the light channels (colour, factor, radius: 4 to 6).
const LIGHT_WORDS: [usize; 3] = [4, 5, 6];

/// A lamp's light placed on its object: the node it hangs from, where it is (tiles, the third the
/// height), its colour (the node's by its factor) and its reach (the light's radius twice, by the
/// get-light's quarter). A light whose factor or radius runs on a stream (`animated`: the dance
/// floor's and laser show's) takes them at the animation's place `IDLE_PLACE`: the play's own
/// place follows the render frames since the object's graphic changed, which the simulation does
/// not keep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampLight {
    pub node: usize,
    pub at: [f32; 3],
    pub colour: [f32; 3],
    pub reach: f32,
    pub animated: bool,
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

/// Whether a node of the animation is a light: all three light channels given.
fn is_light(node: &Node) -> bool {
    LIGHT_WORDS.iter().all(|&w| Channel::of(node[w]) != Channel::None)
}

/// Whether the node's factor or radius (words 5 and 6) is a stream.
fn is_animated(node: &Node) -> bool {
    [5, 6].iter().any(|&w| matches!(Channel::of(node[w]), Channel::Stream(_)))
}

/// The poses of every node of `anim` at its idle place.
pub fn idle_poses(anim: &Anim) -> Vec<Pose> {
    poses_at(anim, IDLE_PLACE)
}

/// The poses of every node of `anim` at `place` (a frame, as the animation's tracks are read).
pub fn poses_at(anim: &Anim, place: f32) -> Vec<Pose> {
    let mut streams = TrackStreams::new(anim);
    (0..anim.nodes.len()).map(|i| streams.local(anim, i, place, 0)).collect()
}

/// The poses of an object idle in `anim` on `skeleton`: the bone matrices (rows, the
/// translation last) on the object's matrix `root`, and the poses. `None` when the animation's
/// nodes do not fit the skeleton's bones. `scale` is the controller's (1: the matrices' 3 x 3 stay
/// unscaled).
pub fn idle_bones(skeleton: &Skeleton, anim: &Anim, root: &Mat4, scale: f32) -> Option<(Vec<Mat4>, Vec<Pose>)> {
    bones_at(skeleton, anim, IDLE_PLACE, root, scale)
}

/// `idle_bones` at `place` of the animation.
pub fn bones_at(skeleton: &Skeleton, anim: &Anim, place: f32, root: &Mat4, scale: f32) -> Option<(Vec<Mat4>, Vec<Pose>)> {
    if anim.nodes.len() != skeleton.bones.len() {
        return None;
    }
    let poses = poses_at(anim, place);
    let nodes: Vec<CharNode> = skeleton.bones.iter().map(CharNode::from_bone).collect();
    let states: Vec<NodeState> = poses.iter().map(|p| NodeState { flags: p.flags, rotation: p.rotation, translation: p.translation, scale: p.scale }).collect();
    // the controller's mode word is not 0 for these objects (the logged bones')
    let bones = skeleton::bone_matrices(&nodes, &states, root, PoseMode { through_rest: true, scale });
    Some((bones, poses))
}

/// The lights of an object idle in `anim` on `skeleton` (the bones' own count of nodes), with the
/// object's matrix `root`: one for each node with the light channels. Empty for an object with
/// none, or whose animation's nodes do not fit its skeleton.
pub fn lights(skeleton: &Skeleton, anim: &Anim, root: &Mat4) -> Vec<LampLight> {
    if !anim.nodes.iter().any(is_light) {
        return Vec::new();
    }
    // a lamp's scale only scales the matrices' 3 x 3, not the translations the lights sit at
    let Some((bones, poses)) = idle_bones(skeleton, anim, root, 1.0) else { return Vec::new() };
    anim.nodes
        .iter()
        .zip(&poses)
        .enumerate()
        .filter(|(_, (n, _))| is_light(n))
        .map(|(node, (n, p))| {
            let (colour, reach) = lamp_light(p.vector, p.scalar_a, p.scalar_b);
            LampLight { node, at: [bones[node][12], bones[node][13], bones[node][14]], colour, reach, animated: is_animated(n) }
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

/// An object's graphic number: its data field 0, which its scripts set (a lamp's switch, a dance
/// floor's mode) and which picks its animation among the object's states. 0 for an object not
/// in the world.
pub fn object_graphic(world: &crate::world::World, id: i16) -> i16 {
    world.find_object(id).and_then(|o| o.data.first().copied()).unwrap_or(0)
}

/// A lamp object of the lot: its leader's id, where it is drawn and its lights.
#[derive(Clone, Debug, PartialEq)]
pub struct Lamp {
    pub leader: i16,
    pub place: Placement,
    pub lights: Vec<LampLight>,
}

/// The lamps among the lot's drawn objects: each placement whose group has a model, a skeleton
/// and an animation with light nodes. `guids_of` gives the GUIDs of a leader's parts (its
/// own first): a multi-tile object's resources are on one of its parts' definitions, not always
/// the leader's. `graphic` gives a leader's graphic number (`object_graphic`), which picks the
/// animation the lamp is posed from (`ObjResources::animation`: a lit lamp's "on", a dance
/// floor's strobe).
pub fn on_lot(placements: &[Placement], guids_of: impl Fn(i16) -> Vec<u32>, graphic: impl Fn(i16) -> i16, resources: &BTreeMap<u32, ObjResources>, sources: &Sources) -> Vec<Lamp> {
    placements
        .iter()
        .filter_map(|p| {
            let (skeleton, anim) = guids_of(p.leader).iter().filter_map(|g| resources.get(g)).find_map(|r| Some((sources.skeletons.get(&r.model)?, sources.anims.get(&r.animation(graphic(p.leader)))?)))?;
            let lights = lights(skeleton, anim, &object_matrix(p.x, p.y, p.angle));
            (!lights.is_empty()).then_some(Lamp { leader: p.leader, place: *p, lights })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graphic_picks_its_state_s_animation() {
        let r = ObjResources { dataset: 0, model: 0, flags: 0, key: 7, idle: 10, states: vec![10, 11, 0] };
        assert_eq!([0, 1, 2, 3, -1].map(|g| r.animation(g)), [10, 11, 10, 10, 10]);
    }

    #[test]
    fn an_object_matrix_turns_and_mirrors() {
        let m = object_matrix(17.5, 24.5, 90.0);
        assert_eq!([m[0], m[1], m[4], m[5]], [0.0, 1.0, 1.0, 0.0]);
        assert_eq!((m[12], m[13], m[15]), (17.5, 24.5, 1.0));
        // facing 6 (-180 degrees): cos of 270 degrees is not quite 0
        let m = object_matrix(0.0, 0.0, -180.0);
        assert_eq!([m[0], m[1], m[4], m[5]], [-1.0, 1.1924881e-8, 1.1924881e-8, 1.0]);
    }
}
