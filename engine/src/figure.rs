//! A Sim's figure from the engine's own state: which skeleton and which parts it has (its
//! description and the part tables), and its bones where its animation stands (`pose` for the
//! node states, `skeleton` for the matrices). Simulation-side and GPU-free: a server could pose
//! the Sims too; the renderer takes the matrices and parts.
//!
//! The pose is the Sim's Animate play alone, at weight 1. The game runs more tracks over it (a
//! standing loop under, a facial animation and an arm's carrying loop over, each with node
//! weights that are not read) and the head looks about; `docs/assets.md`, "The Sims from the
//! engine's state", counts what that leaves (home, tick 1,039: 23 and 26 of the 59 logged
//! bone matrices equal to 1e-4 for the two Sims, the neck and head 0.01 to 0.02 tile off).

use crate::anim::{Anim, Catalog};
use crate::disc::Disc;
use crate::outfit::{Description, PartTables};
use crate::pose::{self, Fold, Pose, TrackStreams};
use crate::skeleton::{self, CharNode, Mat4, NodeState, PoseMode};
use std::collections::HashMap;

/// The controller's scale of a Sim's matrices (+84 of every logged '<' record: 2^-12, the
/// models' own unit): the bone matrices carry it, so a part's vertices are drawn in raw units.
pub const BONE_SCALE: f32 = 2.4414063e-4;

/// The skeletons of `characte.arc` the Sims use, by age and gender.
pub struct Rigs {
    male: Vec<CharNode>,
    female: Vec<CharNode>,
    child: Vec<CharNode>,
}

impl Rigs {
    pub fn read(disc: &Disc) -> Option<Rigs> {
        let arc = disc.read("characte.arc")?;
        let entries = crate::arc::entries(&arc)?;
        let rig = |name: &str| -> Option<Vec<CharNode>> {
            let e = entries.iter().find(|e| e.name == name)?;
            Some(skeleton::read(crate::arc::bytes(&arc, e))?.bones.iter().map(CharNode::from_bone).collect())
        };
        Some(Rigs { male: rig("Male_adult")?, female: rig("female_adult")?, child: rig("child")? })
    }

    /// The nodes of the skeleton a described Sim has (the children share one).
    pub fn of(&self, d: &Description) -> &[CharNode] {
        match (d.adult, d.male) {
            (false, _) => &self.child,
            (true, true) => &self.male,
            (true, false) => &self.female,
        }
    }
}

/// Who wears what: the part tables and the made Sims' descriptions of a save, found by the
/// neighbour (a Sim's person data 31).
pub struct Cast {
    pub tables: PartTables,
    by_neighbour: HashMap<i16, Description>,
}

impl Cast {
    /// `ngh` is the save's neighbourhood file (the disc's default one for the stock lots).
    pub fn read(disc: &Disc, ngh: &[u8]) -> Option<Cast> {
        let quick = disc.read("quickdat.arc")?;
        let image = crate::arc::entries(&quick)?.into_iter().find(|e| e.name == "CreateASim").and_then(|e| crate::quickdata::Image::read(crate::arc::bytes(&quick, &e)))?;
        let tables = PartTables::read(&image)?;
        let made = crate::save::read_created_sims(ngh).unwrap_or_default();
        let by_neighbour = crate::save::read_neighbours(ngh)
            .unwrap_or_default()
            .iter()
            .filter_map(|n| made.iter().find(|m| m.guid == n.guid).map(|m| (n.id, Description::from_save(m))))
            .collect();
        Some(Cast { tables, by_neighbour })
    }

    /// The description of the Sim whose neighbour id (person data 31) is `neighbour`.
    pub fn description(&self, neighbour: i16) -> Option<&Description> {
        self.by_neighbour.get(&neighbour)
    }
}

/// A Sim's root matrix: its place on the lot (tiles) and its heading, a turn of `heading` eighths
/// about the vertical.
pub fn root(x: f32, y: f32, heading: u8) -> Mat4 {
    let (s, c) = (f32::from(heading) * std::f32::consts::FRAC_PI_4).sin_cos();
    let mut m = skeleton::IDENTITY;
    (m[0], m[1], m[4], m[5], m[12], m[13]) = (c, s, -s, c, x, y);
    m
}

/// One of a Sim's animation tracks as the pose takes it: an animation at a place, folded into
/// the node states by a weight (`pose::Fold`), layered over them or blended in, and by node
/// weights that mask it to some of the nodes.
#[derive(Clone, Copy, Debug)]
pub struct Track<'a> {
    pub anim: &'a Anim,
    /// In frames from the animation's first.
    pub place: f32,
    /// The track's own weight (its +76).
    pub weight: f32,
    /// The track's layer flag (+8 bit 8): the pose goes on top of the states, not into them.
    pub layer: bool,
    /// The weight of each node (+152); none: every node's is 1.
    pub nodes: Option<&'a [f32]>,
}

/// The node states the tracks leave, posed in order from the default pose: a node's weight is its
/// track's times its own (`pose::base_weight` for the first track that is not a layer, which is
/// the base). A track of another node count than `count`, or a node whose channels are not all
/// decoded, leaves that part as it was.
pub fn node_states(count: usize, tracks: &[Track]) -> Vec<NodeState> {
    let mut states = vec![Pose::default(); count];
    let mut based = false;
    for t in tracks.iter().filter(|t| t.anim.nodes.len() == count) {
        let weight = pose::base_weight(t.weight, t.layer, &mut based);
        let mut streams = TrackStreams::new(t.anim);
        for (i, state) in states.iter_mut().enumerate() {
            let w = weight * t.nodes.map_or(1.0, |n| n[i]);
            if w == 0.0 {
                continue;
            }
            if let Ok(local) = streams.local(t.anim, i, t.place, state.flags) {
                *state = pose::fold(state, &local, Fold::of(w, t.layer));
            }
        }
    }
    states.iter().map(|p| NodeState { flags: p.flags, rotation: p.rotation, translation: p.translation, scale: p.scale }).collect()
}

/// The nodes the carrying loop's weights are 1 on (the right arm: the node above its clavicle to
/// its hand; 0 on the rest): the node weights of that track in the logged '=' records of
/// home-a39.log.
pub const CARRY_NODES: std::ops::RangeInclusive<usize> = 47..=51;
/// The one node the face track's weights are 1 on (the neck; 0 on the rest), from the same
/// records.
pub const FACE_NODE: usize = 16;
/// The animation a Sim's face track plays, held at its first frame (the face's speed is 0).
pub const FACE_ANIMATION: &str = "happyFace";

/// What a Sim does, as far as its pose goes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stance {
    /// Its Animate play: the animation, how far along (frames) and whether it runs backward.
    pub action: Option<(u32, f32, bool)>,
    /// Whether it holds something in its hand (the carrying loop plays on its right arm).
    pub carrying: bool,
    /// Its mood motive (`FACE_MOTIVE`, 0 to 100): the face track's weight is it over 100 (the
    /// logged weights of the face track equal the motive's value over 100 on every tick of
    /// home-a39.log's first 643).
    pub mood: f32,
    pub adult: bool,
}

/// The index of the motive the face's weight follows.
pub const FACE_MOTIVE: usize = 3;

/// The node weights of the tracks that mask: the carrying loop's and the face's, for a skeleton
/// of `count` nodes.
pub struct Masks {
    carry: Vec<f32>,
    face: Vec<f32>,
}

impl Masks {
    pub fn new(count: usize) -> Masks {
        let weights = |on: &dyn Fn(usize) -> bool| (0..count).map(|i| f32::from(u8::from(on(i)))).collect();
        Masks { carry: weights(&|i| CARRY_NODES.contains(&i)), face: weights(&|i| i == FACE_NODE) }
    }
}

/// The tracks a Sim's animator runs, in the order it poses them: its action as the base, the
/// arm's carrying loop (held at its last frame: its end mode stops it) over the arm's nodes when
/// it carries, and the face's layer by its mood.
pub fn tracks<'a>(catalog: &'a Catalog, stance: &Stance, masks: &'a Masks) -> Vec<Track<'a>> {
    let mut out = Vec::new();
    if let Some((id, at, backward)) = stance.action
        && let Some(anim) = catalog.by_id.get(&id)
    {
        out.push(Track { anim, place: pose_place(anim.frames, at, backward), weight: 1.0, layer: false, nodes: None });
    }
    let carry = if stance.adult { "a2o-rarm-carry-loop" } else { "c2o-rarm-carry-loop" };
    if stance.carrying
        && let Some(anim) = catalog.by_name(carry)
    {
        out.push(Track { anim, place: anim.frames.saturating_sub(1) as f32, weight: 1.0, layer: false, nodes: Some(&masks.carry) });
    }
    if let Some(anim) = catalog.by_name(FACE_ANIMATION) {
        out.push(Track { anim, place: 0.0, weight: (stance.mood / 100.0).clamp(0.0, 1.0), layer: true, nodes: Some(&masks.face) });
    }
    out
}

/// The place along an animation's frames of a play `at` frames in: a backward play runs from the
/// last frame down.
pub fn pose_place(frames: u32, at: f32, backward: bool) -> f32 {
    let last = frames.saturating_sub(1) as f32;
    if backward { (last - at).max(0.0) } else { at.min(last) }
}

/// The heading (eighths of a turn, `root`'s) that faces along the step (`dx`, `dy`): a Sim's
/// forward is its local +y, which a turn of h eighths carries to (-sin, cos) of h x 45 degrees.
/// `None` for no step; a step between two eighths takes the nearer.
pub fn heading_along(dx: i32, dy: i32) -> Option<u8> {
    if dx == 0 && dy == 0 {
        return None;
    }
    let turn = (-(dx as f32)).atan2(dy as f32) / std::f32::consts::FRAC_PI_4;
    Some(turn.round().rem_euclid(8.0) as u8 % 8)
}

/// A bone matrix as the renderer takes it: the three rows of x, y, z, each its three columns and
/// the translation (the matrices here are for row vectors, the translation in the last row).
pub fn rows(m: &Mat4) -> [f32; 12] {
    std::array::from_fn(|i| {
        let (r, c) = (i / 4, i % 4);
        if c == 3 { m[12 + r] } else { m[c * 4 + r] }
    })
}

/// The bone matrices of a Sim on `nodes` posed by `tracks` (none: the skeleton's rest) under
/// `root`, with the models' unit (`BONE_SCALE`) in them.
pub fn bones(nodes: &[CharNode], tracks: &[Track], root: &Mat4) -> Vec<Mat4> {
    let states = node_states(nodes.len(), tracks);
    skeleton::bone_matrices(nodes, &states, root, PoseMode { through_rest: false, scale: BONE_SCALE })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roots_turn_is_about_the_vertical_at_its_place() {
        let m = root(3.0, 4.0, 2);
        // a quarter turn: x goes to y
        assert!((m[0]).abs() < 1e-6 && (m[1] - 1.0).abs() < 1e-6);
        assert_eq!((m[12], m[13], m[10], m[15]), (3.0, 4.0, 1.0, 1.0));
    }

    #[test]
    fn a_heading_faces_along_its_step() {
        // forward of heading 6 is +x, of 4 is -y, of 0 is +y, of 2 is -x
        assert_eq!(heading_along(16, 0), Some(6));
        assert_eq!(heading_along(0, -24), Some(4));
        assert_eq!(heading_along(0, 5), Some(0));
        assert_eq!(heading_along(-3, 0), Some(2));
        assert_eq!(heading_along(0, 0), None);
    }

    #[test]
    fn rows_are_the_transposed_rotation_and_the_translation() {
        let mut m = skeleton::IDENTITY;
        m[1] = 2.0; // x's image has a y part
        m[12..15].copy_from_slice(&[5.0, 6.0, 7.0]);
        let r = rows(&m);
        assert_eq!((r[4], r[3], r[7], r[11]), (2.0, 5.0, 6.0, 7.0));
    }

    #[test]
    fn a_backward_play_runs_from_the_last_frame() {
        assert_eq!(pose_place(61, 9.9, true), 50.1);
        assert_eq!(pose_place(61, 9.9, false), 9.9);
    }
}
