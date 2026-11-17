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

use crate::anim::Anim;
use crate::disc::Disc;
use crate::outfit::{Description, PartTables};
use crate::pose::{Pose, TrackStreams};
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

/// The node states of an animation at a place (in frames from its first), each node at weight 1.
/// A node whose channels are not all decoded keeps the default pose.
pub fn node_states(anim: &Anim, place: f32) -> Vec<NodeState> {
    let mut streams = TrackStreams::new(anim);
    (0..anim.nodes.len())
        .map(|i| {
            let p: Pose = streams.local(anim, i, place, 0).unwrap_or_default();
            NodeState { flags: p.flags, rotation: p.rotation, translation: p.translation, scale: p.scale }
        })
        .collect()
}

/// The place along an animation's frames of a play `at` frames in: a backward play runs from the
/// last frame down.
pub fn pose_place(frames: u32, at: f32, backward: bool) -> f32 {
    let last = frames.saturating_sub(1) as f32;
    if backward { (last - at).max(0.0) } else { at.min(last) }
}

/// A bone matrix as the renderer takes it: the three rows of x, y, z, each its three columns and
/// the translation (the matrices here are for row vectors, the translation in the last row).
pub fn rows(m: &Mat4) -> [f32; 12] {
    std::array::from_fn(|i| {
        let (r, c) = (i / 4, i % 4);
        if c == 3 { m[12 + r] } else { m[c * 4 + r] }
    })
}

/// The bone matrices of a Sim on `nodes` posed by `pose` (an animation and the place it is at,
/// else the skeleton's rest) under `root`, with the models' unit (`BONE_SCALE`) in them.
pub fn bones(nodes: &[CharNode], pose: Option<(&Anim, f32)>, root: &Mat4) -> Vec<Mat4> {
    let states = match pose {
        Some((anim, place)) if anim.nodes.len() == nodes.len() => node_states(anim, place),
        _ => vec![NodeState { flags: 0, rotation: [0.0, 0.0, 0.0, 1.0], translation: [0.0; 3], scale: [1.0; 3] }; nodes.len()],
    };
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
